//! 用每个沙箱的常驻线程在墙钟期限到达时请求 V8 中断。
//!
//! arm/disarm 更新执行代次与期限；普通墙钟模式每代至多触发一次。
//! 预算监控模式在到期后继续监控与中断，直到 disarm：deno_core 转换异常时
//! 会临时取消 V8 终止状态，错误属性 getter/格式化钩子仍可能执行用户 JS。
//! 期限更新、代次检查和 terminate 投递共用 state 互斥锁，因此 disarm 返回后
//! 旧代次不会再投递中断。下一次执行仍需清除已经投递的 V8 终止标志。
//! 已有定时唤醒不晚于新期限时复用该等待；撤销期限不必唤醒线程。
//! 这样短调用的连续 arm/disarm 不会每次触发监控线程调度，仍保留每次预算边界。
//!
//! 关闭时设置 shutdown，再获取 state 锁并通知等待线程，最后 join。
//! 这样等待线程不会在检查退出状态之后漏过最后一次唤醒。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Clone, Copy)]
enum WorkerWait {
    Active,
    Idle,
    Until(Instant),
}

struct WatchdogState {
    deadline: Option<Instant>,
    /// deadline 对应的代际。
    armed_generation: u64,
    /// 已为该代际触发过 terminate（普通模式避免解除武装前重触发）。
    fired_generation: u64,
    waiting: WorkerWait,
}

struct WatchdogShared {
    generation: AtomicU64,
    state: Mutex<WatchdogState>,
    cond: Condvar,
    shutdown: AtomicBool,
}

pub struct Watchdog {
    shared: Arc<WatchdogShared>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// 启动常驻看门狗线程。`isolate` 是 V8 的跨线程句柄（Send+Sync，terminate 线程安全）。
    /// 线程随 `Watchdog` 的 Drop 结束——**必须先于所属 JsRuntime 销毁**（由 Sandbox 的
    /// 字段声明顺序保证：watchdog 声明在 runtime 之前）。
    pub fn spawn(isolate: deno_core::v8::IsolateHandle) -> Self {
        Self::spawn_with_monitor(isolate, None)
    }

    /// 武装期间定期监控；参数表示墙钟已到期。预算模式到期后持续中断到 disarm。
    pub fn spawn_with_monitor(
        isolate: deno_core::v8::IsolateHandle,
        monitor: Option<Box<dyn Fn(bool) + Send>>,
    ) -> Self {
        let shared = Arc::new(WatchdogShared {
            generation: AtomicU64::new(0),
            state: Mutex::new(WatchdogState {
                deadline: None,
                armed_generation: 0,
                fired_generation: 0,
                waiting: WorkerWait::Active,
            }),
            cond: Condvar::new(),
            shutdown: AtomicBool::new(false),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("kk-watchdog".into())
            .spawn(move || {
                let mut st = worker.state.lock().unwrap();
                loop {
                    st.waiting = WorkerWait::Active;
                    if worker.shutdown.load(Ordering::Acquire) {
                        return;
                    }
                    match st.deadline {
                        None => {
                            st.waiting = WorkerWait::Idle;
                            st = worker.cond.wait(st).unwrap();
                        }
                        Some(deadline) => {
                            let now = Instant::now();
                            if now >= deadline {
                                // 代次检查与投递均在 state 锁内；disarm 返回后旧代次不会再投递中断。
                                let armed = st.armed_generation;
                                if worker.generation.load(Ordering::Acquire) == armed
                                    && (armed != st.fired_generation || monitor.is_some())
                                {
                                    if let Some(monitor) = &monitor {
                                        monitor(true);
                                    }
                                    st.fired_generation = armed;
                                    isolate.terminate_execution();
                                }
                                // 普通玩家模式保持单次投递；预算模式覆盖异常转换取消终止的窗口。
                                st = if monitor.is_some() {
                                    let wait = std::time::Duration::from_millis(1);
                                    st.waiting = WorkerWait::Until(Instant::now() + wait);
                                    worker.cond.wait_timeout(st, wait).unwrap().0
                                } else {
                                    st.waiting = WorkerWait::Idle;
                                    worker.cond.wait(st).unwrap()
                                };
                                continue;
                            }
                            let wake_at = if let Some(monitor) = &monitor {
                                monitor(false);
                                deadline.min(Instant::now() + std::time::Duration::from_millis(1))
                            } else {
                                deadline
                            };
                            st.waiting = WorkerWait::Until(wake_at);
                            let wait = wake_at.saturating_duration_since(Instant::now());
                            let (guard, _timed_out) = worker.cond.wait_timeout(st, wait).unwrap();
                            st = guard;
                        }
                    }
                }
            })
            .expect("spawn kk-watchdog");
        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// 登记执行期限并返回新的代次；期限到达且代次未变时请求中断。
    /// 更新在 state 锁内完成，与工作线程的检查和投递互斥。
    pub fn arm(&self, deadline: Instant) -> u64 {
        let mut st = self.shared.state.lock().unwrap();
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        st.deadline = Some(deadline);
        st.armed_generation = generation;
        // 等待状态在同一把锁内登记，避免线程检查期限后漏过唤醒。
        // 已定时等待且会更早醒来时，直接复用；醒来后读取最新代次与期限。
        if match st.waiting {
            WorkerWait::Idle => true,
            WorkerWait::Until(wake_at) => deadline < wake_at,
            WorkerWait::Active => false,
        } {
            self.shared.cond.notify_all();
        }
        drop(st);
        generation
    }

    /// 清除期限并递增代次；返回后旧代次不会再投递中断。
    /// 不会清除 V8 中已经投递的终止标志。
    pub fn disarm(&self) {
        let mut st = self.shared.state.lock().unwrap();
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        st.deadline = None;
        // 工作线程仍可等到原定采样点；共用锁已阻止旧代次继续投递。
        drop(st);
    }

    /// 停止并回收线程。幂等。
    pub fn shutdown(&mut self) {
        if self.shared.shutdown.swap(true, Ordering::AcqRel) {
            return;
        }
        {
            // 先取得等待线程使用的锁，再通知退出；join 前必须确保等待线程能醒来。
            let _st = self.shared.state.lock().unwrap();
            self.shared.cond.notify_all();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // 同时核对代次变化与真实等待/投递；执行中的 V8 行为由沙箱集成测试覆盖。

    #[test]
    fn arm_disarm_and_shutdown() {
        let shared = Arc::new(WatchdogShared {
            generation: AtomicU64::new(0),
            state: Mutex::new(WatchdogState {
                deadline: None,
                armed_generation: 0,
                fired_generation: 0,
                waiting: WorkerWait::Active,
            }),
            cond: Condvar::new(),
            shutdown: AtomicBool::new(false),
        });
        let mut w = Watchdog {
            shared: shared.clone(),
            thread: None,
        };
        let g1 = w.arm(Instant::now() + Duration::from_millis(10));
        let g2 = w.arm(Instant::now() + Duration::from_millis(10));
        assert_eq!(g2, g1 + 1, "每次 arm 代际递增");
        w.disarm();
        assert_eq!(w.shared.generation.load(Ordering::SeqCst), g2 + 1);
        w.shutdown();
        assert!(w.shared.shutdown.load(Ordering::Acquire));
    }

    fn wait_for(mut ready: impl FnMut() -> bool) {
        let started = Instant::now();
        while !ready() {
            assert!(started.elapsed() < Duration::from_secs(2), "等待看门狗超时");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn earlier_deadline_wakes_timed_wait_and_cancelled_deadline_does_not_fire() {
        let mut runtime = deno_core::JsRuntime::new(Default::default());
        let mut watchdog = Watchdog::spawn(runtime.v8_isolate().thread_safe_handle());
        wait_for(|| {
            matches!(
                watchdog.shared.state.lock().unwrap().waiting,
                WorkerWait::Idle
            )
        });
        watchdog.arm(Instant::now() + Duration::from_secs(30));
        wait_for(|| {
            matches!(watchdog.shared.state.lock().unwrap().waiting,
                WorkerWait::Until(at) if at > Instant::now() + Duration::from_secs(10))
        });
        // 工作者正在长定时等待；较早期限必须打断等待，而不能等旧期限。
        watchdog.arm(Instant::now() + Duration::from_millis(10));
        // 终止请求需进入 V8 安全点才能观测；有界表达式不会把测试挂死。
        wait_for(|| runtime.execute_script("watchdog-probe", "1+1").is_err());
        watchdog.disarm();
        runtime.v8_isolate().cancel_terminate_execution();

        watchdog.arm(Instant::now() + Duration::from_secs(30));
        wait_for(|| {
            matches!(watchdog.shared.state.lock().unwrap().waiting,
                WorkerWait::Until(at) if at > Instant::now() + Duration::from_secs(10))
        });
        watchdog.arm(Instant::now() + Duration::from_millis(10));
        watchdog.disarm();
        runtime.v8_isolate().cancel_terminate_execution();
        std::thread::sleep(Duration::from_millis(30));
        assert!(runtime.execute_script("watchdog-probe", "1+1").is_ok());

        watchdog.arm(Instant::now() + Duration::from_secs(30));
        wait_for(|| {
            matches!(
                watchdog.shared.state.lock().unwrap().waiting,
                WorkerWait::Until(_)
            )
        });
        let started = Instant::now();
        watchdog.shutdown();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "长等待中也须及时回收"
        );
    }

    #[test]
    fn budget_monitor_repeats_after_expiry_and_stops_on_disarm() {
        let mut runtime = deno_core::JsRuntime::new(Default::default());
        let expired = Arc::new(AtomicU64::new(0));
        let count = expired.clone();
        let mut watchdog = Watchdog::spawn_with_monitor(
            runtime.v8_isolate().thread_safe_handle(),
            Some(Box::new(move |wall_expired| {
                if wall_expired {
                    count.fetch_add(1, Ordering::Relaxed);
                }
            })),
        );
        watchdog.arm(Instant::now());
        wait_for(|| expired.load(Ordering::Relaxed) >= 3);
        watchdog.disarm();
        let stopped = expired.load(Ordering::Relaxed);
        runtime.v8_isolate().cancel_terminate_execution();
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(expired.load(Ordering::Relaxed), stopped);
        assert!(runtime.execute_script("watchdog-probe", "1+1").is_ok());
        watchdog.shutdown();
    }
}
