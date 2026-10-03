//! 可信玩法 VM 的总资源配置；共享 realm 不提供逐 Mod 计账。
use crate::{ModHostError, ModHostResult};
use kk_vm::budget::ExecutionLimits;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModVmLimits {
    pub registration: ExecutionLimits,
    pub settlement: ExecutionLimits,
    /// V8 已用堆与 V8 统计到的堆外内存合计上限；不是整个进程 RSS 上限。
    pub memory_bytes: usize,
}

impl Default for ModVmLimits {
    fn default() -> Self {
        Self {
            registration: ExecutionLimits {
                cpu: Duration::from_millis(500),
                wall: Duration::from_secs(2),
            },
            settlement: ExecutionLimits {
                cpu: Duration::from_millis(100),
                wall: Duration::from_secs(1),
            },
            memory_bytes: 128 * 1024 * 1024,
        }
    }
}

impl ModVmLimits {
    pub(crate) fn validate(&self) -> ModHostResult<()> {
        for limit in [self.registration, self.settlement] {
            if limit.cpu.is_zero()
                || limit.wall.is_zero()
                || limit.cpu > Duration::from_secs(3600)
                || limit.wall > Duration::from_secs(3600)
            {
                return Err(ModHostError::new(
                    "VM 配置",
                    "CPU/墙钟限额须为正且不超过一小时",
                ));
            }
        }
        if self.memory_bytes < 16 * 1024 * 1024 || self.memory_bytes as u64 > 4 * 1024 * 1024 * 1024
        {
            return Err(ModHostError::new("VM 配置", "内存限额须为 16 MiB..=4 GiB"));
        }
        Ok(())
    }
}

pub use kk_vm::budget::ExecutionLimits as ModExecutionLimits;
