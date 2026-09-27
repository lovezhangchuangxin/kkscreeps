//! 沙箱抽象（ADR-008 第 5 条）：世界视图进、intent 批出。
//!
//! 一个真实现（M2：prelude 收集玩家 API 调用为 intent，经 endTick 通道传出）
//! 加一个测试假实现（ScriptedSandbox 按脚本回放固定指令）。
//! 引擎不依赖 deno_core，单元测试不必拖着真 V8 跑。

use crate::intent::IntentBatch;

/// 结算前的世界只读视图（M1 最小面：整个 typed 世界；沙箱按需读房间）。
pub struct WorldView<'a> {
    pub world: &'a kk_world::World,
}

/// 玩家代码沙箱。实现方负责把"玩家看到的"与"玩家想做的"隔离：
/// 视图只读，产出的 intent 一律视为不可信（结算前过白名单清洗）。
pub trait Sandbox: Send {
    fn collect_intents(&mut self, view: &WorldView<'_>, player: &str) -> IntentBatch;
}

/// 固定指令脚本假实现：每 tick 回放同一段（或按 tick 序切换）。
#[derive(Debug, Default)]
pub struct ScriptedSandbox {
    /// 脚本表：tick → 该 tick 注入的批（缺 tick 为空批）。
    script: std::collections::BTreeMap<u64, IntentBatch>,
}

impl ScriptedSandbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置某一 tick 的指令批。
    pub fn at(&mut self, tick: u64, batch: IntentBatch) -> &mut Self {
        self.script.insert(tick, batch);
        self
    }
}

impl Sandbox for ScriptedSandbox {
    fn collect_intents(&mut self, view: &WorldView<'_>, _player: &str) -> IntentBatch {
        self.script
            .get(&view.world.time)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::Action;
    use kk_world::ObjId;

    #[test]
    fn replays_by_tick() {
        let mut world = kk_world::World::new(1);
        world.time = 5;
        {
            let view = WorldView { world: &world };
            let mut sb = ScriptedSandbox::new();
            let mut batch = IntentBatch::default();
            batch.add_object_intent(
                "p1",
                ObjId(1),
                Action::Move,
                crate::intent::IntentData::bare(),
            );
            sb.at(5, batch.clone());
            let got = sb.collect_intents(&view, "p1");
            assert_eq!(got, batch);
        }
        world.time = 6;
        let view = WorldView { world: &world };
        assert!(
            ScriptedSandbox::new()
                .collect_intents(&view, "p1")
                .users
                .is_empty()
        );
    }
}
