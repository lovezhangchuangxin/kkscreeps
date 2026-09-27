//! 看门狗：预算硬超时的跨线程强制掐断。
//!
//! spike 的 bucket_sim 用"每 tick 一次性看门狗线程 + 代际守卫"验证了机制；
//! 这里按 M0 方案升级为**每个沙箱一个常驻看门狗线程**（避免每 tick 建线程的开销与退出竞态）。
//!
//! 代际守卫语义（防止上一个 tick 迟到的掐断误杀下一个 tick）：
//! - `arm(deadline)` 把代际 +1 并登记期限；只有"当前代际"的期限到达才有权 terminate；
//! - 执行自然结束调用 `disarm()`（代际 +1，期限清空），迟到的到期检查因代际已变而作废；
//! - **原子性**：arm/disarm 的代际递增与状态变更、看门狗的"代际检查 + terminate 投递"
//!   都在 state 互斥锁内完成——看门狗持锁跨过 terminate 投递（非阻塞的异步标志设置），
//!   因此 disarm 的完成意味着 terminate 已投递完毕，下一 tick 开始时的
//!   `cancel_terminate_execution()`（双保险）必然发生在任何迟到 terminate 之后。
//! - **唤醒不丢失**：arm/disarm/shutdown 的 `notify_all` 一律**持锁**调用——锁外
//!   notify 会与看门狗"持锁检查 → cond.wait 原子注册等待"的临界区赛跑，notify
//!   落空则看门狗睡死（join 永久挂死，八-7 实测偶发）。

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
                                // 只有当前代际才有权终止；已触发过的代际不再重复触发。
                                // 持锁跨过 terminate 投递（非阻塞的异步标志设置）：
                                // 与 arm/disarm 原子——disarm 完成即意味着 terminate 已投递，
                                // 迟到的 terminate 不可能越过 disarm/arm 交界误杀下一个 tick。
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

    /// 武装：`deadline` 到达且代际未变时 terminate。返回本次代际。
    /// 代际递增在 state 锁内——与看门狗的"检查 + 投递"互斥（见模块注释）。
    pub fn arm(&self, deadline: Instant) -> u64 {
        let mut st = self.shared.state.lock().unwrap();
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        st.deadline = Some(deadline);
        st.armed_generation = generation;
        // 持锁 notify（Mesa 模式）：锁外 notify 会与看门狗"持锁检查 → cond.wait
        // 原子注册"的临界区赛跑，notify 落空则看门狗睡死（丢失唤醒，八-7 实测
        // 偶发 join 永久挂死）；持锁时对方要么已在等待队列、要么拿锁后必见新状态
        self.shared.cond.notify_all();
        drop(st);
        generation
    }

    /// 解除武装（执行自然结束，或硬中断后的善后）。
    /// 代际递增同样在 state 锁内；完成即意味着任何旧代际的 terminate 已投递完毕。
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
            // 持锁 notify：shutdown 的唤醒若落空，看门狗将永久睡死在 cond.wait，
            // join 挂死整个进程收尾（这是唯一没有"下一次 notify"补救的唤醒点）
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

    // terminate 语义由 sandbox 集成测试覆盖；这里验证调度器本身的代际与停机逻辑。

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
