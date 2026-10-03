//! 在 V8 安全点检查执行线程 CPU 与 V8 堆/堆外计数，墙钟看门狗独立兜底。
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{cpu_clock::thread_cpu_time, watchdog::Watchdog};
use deno_core::v8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionLimits {
    pub cpu: Duration,
    pub wall: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub cpu: Duration,
    pub wall: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitError {
    Cpu,
    Wall,
    Memory,
    Clock,
    Poisoned,
}

impl std::fmt::Display for LimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cpu => "CPU 预算耗尽",
            Self::Wall => "墙钟预算耗尽",
            Self::Memory => "V8 内存预算耗尽",
            Self::Clock => "CPU 时钟不可用",
            Self::Poisoned => "VM 已因资源故障毒化",
        })
    }
}

struct Running {
    start: Instant,
    cpu_start: Duration,
    limits: ExecutionLimits,
}

/// 调用方必须持有此句柄直至 isolate 销毁；排队的 V8 回调使用其稳定地址。
pub struct ResourceState {
    running: Mutex<Option<Running>>,
    reason: AtomicU8,
    queued: AtomicBool,
    memory_limit: usize,
}

impl ResourceState {
    fn fault(&self, code: u8) {
        let _ = self
            .reason
            .compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire);
    }
    fn error(&self) -> Option<LimitError> {
        match self.reason.load(Ordering::Acquire) {
            0 => None,
            1 => Some(LimitError::Cpu),
            2 => Some(LimitError::Wall),
            3 => Some(LimitError::Memory),
            _ => Some(LimitError::Clock),
        }
    }
}

// SAFETY: 回调数据来自 ResourceState 的 Arc 稳定地址，宿主持有直到 isolate 销毁。
// V8 在执行线程的安全点调用此函数；只读统计与请求中断，不执行 JS 或再次进入 realm。
unsafe extern "C" fn monitor(isolate: v8::UnsafeRawIsolatePtr, data: *mut std::ffi::c_void) {
    let state = unsafe { &*data.cast::<ResourceState>() };
    let mut isolate = unsafe { v8::Isolate::from_raw_isolate_ptr(isolate) };
    state.queued.store(false, Ordering::Release);
    let running = state.running.lock().unwrap();
    let Some(run) = running.as_ref() else {
        return;
    };
    match thread_cpu_time() {
        Ok(now) if now.saturating_sub(run.cpu_start) >= run.limits.cpu => state.fault(1),
        Err(_) => state.fault(4),
        _ => {}
    }
    let heap = isolate.get_heap_statistics();
    if heap.used_heap_size().saturating_add(heap.external_memory()) >= state.memory_limit {
        state.fault(3);
    }
    if state.error().is_some() {
        isolate.terminate_execution();
    }
}

pub struct VmBudget {
    // 先停看门狗；ResourceState 的另一个 Arc 由宿主持有到 isolate 销毁后。
    watchdog: Watchdog,
    state: Arc<ResourceState>,
    isolate: v8::IsolateHandle,
}

impl VmBudget {
    /// # Safety
    /// 宿主必须将 keepalive 的 Arc 保留到 isolate 销毁后，再销毁 ResourceState。
    /// start/finish 与所属 isolate 必须在同一执行线程使用。
    pub unsafe fn new(isolate: v8::IsolateHandle, memory_limit: usize) -> Self {
        let state = Arc::new(ResourceState {
            running: Mutex::new(None),
            reason: AtomicU8::new(0),
            queued: AtomicBool::new(false),
            memory_limit,
        });
        let shared = state.clone();
        let handle = isolate.clone();
        let watchdog = Watchdog::spawn_with_monitor(
            isolate.clone(),
            Some(Box::new(move |wall_expired| {
                // 到期即留存原因，不依赖尚未返回的 V8 安全点或 finish。
                if wall_expired {
                    shared.fault(2);
                }
                if !shared.queued.swap(true, Ordering::AcqRel) {
                    handle.request_interrupt(monitor, Arc::as_ptr(&shared).cast_mut().cast());
                }
            })),
        );
        Self {
            watchdog,
            state,
            isolate,
        }
    }

    pub fn keepalive(&self) -> Arc<ResourceState> {
        self.state.clone()
    }

    pub fn start(&self, limits: ExecutionLimits) -> Result<(), LimitError> {
        if self.state.error().is_some() {
            return Err(LimitError::Poisoned);
        }
        if limits.cpu.is_zero() {
            self.state.fault(1);
            return Err(LimitError::Cpu);
        }
        if limits.wall.is_zero() {
            self.state.fault(2);
            return Err(LimitError::Wall);
        }
        let cpu_start = thread_cpu_time().map_err(|_| {
            self.state.fault(4);
            LimitError::Clock
        })?;
        let start = Instant::now();
        let deadline = start.checked_add(limits.wall).ok_or(LimitError::Wall)?;
        *self.state.running.lock().unwrap() = Some(Running {
            start,
            cpu_start,
            limits,
        });
        self.watchdog.arm(deadline);
        Ok(())
    }

    pub fn finish(&self, isolate: &mut v8::Isolate) -> Result<Usage, LimitError> {
        self.watchdog.disarm();
        let run = self
            .state
            .running
            .lock()
            .unwrap()
            .take()
            .expect("预算已开始");
        let usage = Usage {
            cpu: thread_cpu_time()
                .map_err(|_| {
                    self.state.fault(4);
                    LimitError::Clock
                })?
                .saturating_sub(run.cpu_start),
            wall: run.start.elapsed(),
        };
        if usage.cpu >= run.limits.cpu {
            self.state.fault(1);
        }
        if usage.wall >= run.limits.wall {
            self.state.fault(2);
        }
        let heap = isolate.get_heap_statistics();
        if heap.used_heap_size().saturating_add(heap.external_memory()) >= self.state.memory_limit {
            self.state.fault(3);
        }
        match self.state.error() {
            Some(error) => Err(error),
            None => Ok(usage),
        }
    }

    /// 临时增加有限余量，让 V8 能传播中断；调用方随后销毁 VM，不恢复健康标记。
    pub fn near_heap_callback(&self) -> impl FnMut(usize, usize) -> usize + use<> {
        let state = self.state.clone();
        let isolate = self.isolate.clone();
        move |limit, _initial| {
            state.fault(3);
            isolate.terminate_execution();
            limit.saturating_add(16 * 1024 * 1024)
        }
    }
}
