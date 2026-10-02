//! 玩家任务所属的已提交世界轮次；执行尝试和工作者身份由执行协议另行携带。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunEpoch {
    pub tick: u64,
    pub generation: u64,
}
