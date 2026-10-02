//! 用每个沙箱的常驻线程在墙钟期限到达时请求 V8 中断。
//!
//! arm/disarm 更新执行代次与期限；工作线程只终止当前代次且每代至多触发一次。
//! 期限更新、代次检查和 terminate 投递共用 state 互斥锁，因此 disarm 返回后
//! 旧代次不会再投递中断。下一次执行仍需清除已经投递的 V8 终止标志。
//!
//! 关闭时设置 shutdown，再获取 state 锁并通知等待线程，最后 join。
//! 这样等待线程不会在检查退出状态之后漏过最后一次唤醒。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

struct WatchdogState {
    deadline: Option<Instant>,
    /// deadline 对应的代际。
    armed_generation: u64,
    /// 已为该代际触发过 terminate（避免解除武装前的忙转重触发）。
    fired_generation: u64,
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
        let shared = Arc::new(WatchdogShared {
            generation: AtomicU64::new(0),
            state: Mutex::new(WatchdogState {
                deadline: None,
                armed_generation: 0,
                fired_generation: 0,
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
                    if worker.shutdown.load(Ordering::Acquire) {
                        return;
                    }
                    match st.deadline {
                        None => {
                            st = worker.cond.wait(st).unwrap();
                        }
                        Some(deadline) => {
                            let now = Instant::now();
                            if now >= deadline {
                                // 代次检查与投递均在 state 锁内；disarm 返回后旧代次不会再投递中断。
                                // 已触发的代次等待下一次状态变化，不重复触发。
                                let armed = st.armed_generation;
                                if armed != st.fired_generation
                                    && worker.generation.load(Ordering::Acquire) == armed
                                {
                                    st.fired_generation = armed;
                                    isolate.terminate_execution();
                                }
                                // 等待下一次 arm/disarm/shutdown 唤醒，避免忙转
                                st = worker.cond.wait(st).unwrap();
                                continue;
                            }
                            let (guard, _timed_out) =
                                worker.cond.wait_timeout(st, deadline - now).unwrap();
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
        // 状态更新和通知共用工作线程等待时的锁，线程醒来后重新检查期限。
        self.shared.cond.notify_all();
        drop(st);
        generation
    }

    /// 清除期限并递增代次；返回后旧代次不会再投递中断。
    /// 不会清除 V8 中已经投递的终止标志。
    pub fn disarm(&self) {
        let mut st = self.shared.state.lock().unwrap();
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        st.deadline = None;
        self.shared.cond.notify_all(); // 持锁 notify，见 arm 的注释
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

    // 这里只检查共享状态的代次变化；真实线程的中断和回收由沙箱集成测试覆盖。

    #[test]
    fn arm_disarm_and_shutdown() {
        let shared = Arc::new(WatchdogShared {
            generation: AtomicU64::new(0),
            state: Mutex::new(WatchdogState {
                deadline: None,
                armed_generation: 0,
                fired_generation: 0,
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
}
