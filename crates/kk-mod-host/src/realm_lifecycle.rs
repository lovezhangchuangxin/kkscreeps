//! 同线程 realm 生命周期与冷恢复切换的 V8 栈纪律。
use super::*;

impl ModRealm {
    pub fn is_current_realm(&self) -> bool {
        self.isolate_order.is_current()
    }

    /// 冷恢复替代 VM 与旧 VM 须在本线程 realm 栈中相邻；跨活跃分片拒绝切换。
    pub fn can_retire_previous_realm(&self, previous: &Self) -> bool {
        self.isolate_order
            .immediately_above(&previous.isolate_order)
    }

    /// 在已验证的替代 realm 发布前回收其下方的旧 realm。
    ///
    /// # Safety
    /// 两者须同线程，self 必须是当前 isolate，previous 必须紧邻其下；
    /// 不得存在开放的 V8 scope 或执行调用。仅用于完整冷恢复的最后切换点。
    /// 来源：rusty_v8 150.4.0 isolate.rs 的 Enter/Exit 与 OwnedIsolate::drop。
    pub unsafe fn retire_previous_realm(&mut self, previous: Self) {
        // SAFETY: 调用方保证隔离替代实例位于旧实例之上；暂时退出替代实例，
        // 按 V8 栈序回收旧实例，再恢复替代实例的进入状态及后续 Drop 配对。
        assert!(
            self.can_retire_previous_realm(&previous),
            "检查点替代 realm 不能越过活跃分片"
        );
        unsafe {
            self.runtime.v8_isolate().exit();
        }
        drop(previous);
        unsafe {
            self.runtime.v8_isolate().enter();
        }
    }
}
