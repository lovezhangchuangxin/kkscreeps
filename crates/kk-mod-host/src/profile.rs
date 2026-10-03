//! 可选整槽诊断计时。只累加计数与墙钟，不保存逐调用样本或世界数据。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

use deno_core::{OpMetricsEvent, OpMetricsFactoryFn};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ProfileSample {
    pub calls: u64,
    pub errors: u64,
    pub wall_ns: u128,
}

/// 线程亲和的诊断句柄；重建 realm 时继续记录，调用方显式划定采集窗口。
/// 各阶段存在包含关系，不可直接相加；开启时有计时与 op 包装开销。
#[derive(Clone, Debug, Default)]
pub struct ChainProfiler(Rc<RefCell<BTreeMap<&'static str, ProfileSample>>>);

impl ChainProfiler {
    pub(crate) fn same_sink(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub fn reset(&self) {
        self.0.borrow_mut().clear();
    }

    pub fn snapshot(&self) -> BTreeMap<&'static str, ProfileSample> {
        self.0.borrow().clone()
    }

    pub fn measure(&self, phase: &'static str) -> ProfileTimer {
        ProfileTimer {
            profiler: self.clone(),
            phase,
            started: Instant::now(),
        }
    }

    fn record(&self, phase: &'static str, started: Instant, error: bool) {
        let elapsed = started.elapsed().as_nanos();
        let mut data = self.0.borrow_mut();
        let sample = data.entry(phase).or_default();
        sample.calls += 1;
        sample.errors += u64::from(error);
        sample.wall_ns += elapsed;
    }

    pub(crate) fn op_metrics(&self) -> OpMetricsFactoryFn {
        let profiler = self.clone();
        Box::new(move |_, _, op| {
            let phase = match op.name {
                "op_mod_chain_read" => "op_read",
                "op_mod_chain_write" => "op_write",
                "op_mod_chain_native" => "op_native",
                "op_mod_chain_fault" | "op_mod_chain_fault_detail" => "op_fault",
                _ => return None,
            };
            let profiler = profiler.clone();
            // serde 可访问 JS getter，允许同一 op 嵌套；诊断耗时为包含耗时。
            let starts = RefCell::new(Vec::new());
            Some(Rc::new(move |_, event, _| match event {
                OpMetricsEvent::Dispatched => starts.borrow_mut().push(Instant::now()),
                OpMetricsEvent::Completed | OpMetricsEvent::Error => {
                    if let Some(started) = starts.borrow_mut().pop() {
                        profiler.record(phase, started, event == OpMetricsEvent::Error);
                    }
                }
                _ => {}
            }))
        })
    }
}

#[must_use]
pub struct ProfileTimer {
    profiler: ChainProfiler,
    phase: &'static str,
    started: Instant,
}

impl Drop for ProfileTimer {
    fn drop(&mut self) {
        self.profiler.record(self.phase, self.started, false);
    }
}
