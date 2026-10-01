//! 沙箱接口与玩家运行结果类型。
//!
//! ScriptedSandbox 为引擎测试回放指令；实际 JavaScript 执行位于
//! kk-runtime，并直接使用这里的结果类型。该接口不依赖 V8。

use std::collections::BTreeMap;

use crate::intent::IntentBatch;

/// 结算前的只读世界。按玩家可见范围构造沙箱数据是执行器的责任。
pub struct WorldView<'a> {
    pub world: &'a kk_world::World,
}

/// 一次执行的玩家身份和代码。测试沙箱忽略代码。
pub struct RunSpec {
    pub player: String,
    /// 玩家代码。
    pub code: String,
}

impl RunSpec {
    pub fn new(player: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            player: player.into(),
            code: code.into(),
        }
    }
}

/// 一次玩家执行的结果分类。
/// kk-runtime 的协调器允许玩家级失败继续结算，基础设施故障则返回错误。
#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    /// 执行器完成本次调用；玩家代码异常仍可记录在 expr 或 console 中。
    Executed,
    /// 执行被硬中断或因预算不足而跳过；计费候选由执行器提供。
    Terminated,
    /// 无法注入本轮世界数据，未执行玩家代码。
    InjectFailed,
    /// 执行线程或通信协议等基础设施发生故障。
    InfraFailure(String),
}

impl RunOutcome {
    /// 是否属于玩家级结果；基础设施故障返回 false。
    pub fn is_player_outcome(&self) -> bool {
        !matches!(self, RunOutcome::InfraFailure(_))
    }
}

/// 一次执行产生的指令、输出与玩家状态候选。
///
/// 此结构不自行提交状态。kk-runtime 在世界结算成功后写回 Memory、segments
/// 并提交 CPU 计费；玩家 JavaScript 的全局变量等沙箱内部变化不在此事务内。
/// 直接调用 Engine::tick 只收集 Executed 的指令，不消费其余字段。
#[derive(Debug, Clone)]
pub struct RunResult {
    /// 执行器提供的玩家身份；调用方仍须按注册身份绑定指令。
    pub player: String,
    pub outcome: RunOutcome,
    /// 经身份绑定的指令批；未执行或中断时通常为空。
    pub intents: IntentBatch,
    /// runExpr 结果 JSON（screeps-ok 通道：`{ok, value?} | {ok:false, kind, error}`）。
    pub expr: serde_json::Value,
    /// 本轮 CPU 计费候选，单位毫秒；None 表示没有产出。
    pub cpu_billed_ms: Option<f64>,
    /// 本轮 RawMemory 写回候选；None 表示未产出或写回失败。
    pub raw_memory: Option<String>,
    /// 段写回与请求：`{writes, active?, public?, defaultPublic?, foreign?}`。
    /// None 表示没有产出；请求的校验和提交由 kk-runtime 完成。
    pub segments_out: Option<serde_json::Value>,
    /// 本次执行期间的控制台输出。
    pub console: Vec<String>,
}

impl RunResult {
    /// 构造注入失败结果：指令为空，Memory、segments 和 CPU 候选均缺失。
    pub fn inject_failed(player: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            player: player.into(),
            outcome: RunOutcome::InjectFailed,
            intents: IntentBatch::default(),
            expr: serde_json::json!({
                "ok": false, "kind": "inject_failed", "error": error.into(),
            }),
            cpu_billed_ms: None,
            raw_memory: None,
            segments_out: None,
            console: Vec::new(),
        }
    }

    /// 构造基础设施故障结果，携带原因且不产出玩家状态候选。
    pub fn infra_failure(player: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            player: player.into(),
            outcome: RunOutcome::InfraFailure(error.into()),
            intents: IntentBatch::default(),
            expr: serde_json::Value::Null,
            cpu_billed_ms: None,
            raw_memory: None,
            segments_out: None,
            console: Vec::new(),
        }
    }
}

/// 接收只读世界与执行参数，返回候选结果。
/// 实现可以改变自身的沙箱状态；只读视图不代表执行没有副作用。
/// 玩家指令须先清洗并绑定注册身份，再交给结算器。
pub trait Sandbox: Send {
    fn run(&mut self, view: &WorldView<'_>, spec: &RunSpec) -> RunResult;
}

/// 按世界时间返回预设指令的测试沙箱，不执行代码。
/// 脚本按 tick 索引，不按玩家区分；Engine::tick 只取注册玩家的指令。
#[derive(Debug, Default)]
pub struct ScriptedSandbox {
    /// 脚本表：tick → 该 tick 注入的批（缺 tick 为空批）。
    script: BTreeMap<u64, IntentBatch>,
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
    fn run(&mut self, view: &WorldView<'_>, spec: &RunSpec) -> RunResult {
        RunResult {
            player: spec.player.clone(),
            outcome: RunOutcome::Executed,
            intents: self
                .script
                .get(&view.world.time())
                .cloned()
                .unwrap_or_default(),
            expr: serde_json::json!({ "ok": true }),
            cpu_billed_ms: None,
            raw_memory: None,
            segments_out: None,
            console: Vec::new(),
        }
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
        world.set_time_for_test(5);
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
            let got = sb.run(&view, &RunSpec::new("p1", ""));
            assert_eq!(got.intents, batch);
            assert_eq!(got.outcome, RunOutcome::Executed);
        }
        world.set_time_for_test(6);
        let view = WorldView { world: &world };
        assert!(
            ScriptedSandbox::new()
                .run(&view, &RunSpec::new("p1", ""))
                .intents
                .users
                .is_empty()
        );
    }
}
