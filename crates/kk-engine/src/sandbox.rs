//! 沙箱抽象（ADR-008 第 5 条；B0 WP1 演进为统一执行接口）。
//!
//! 本 trait 的实现当前只有测试假实现（ScriptedSandbox 按脚本回放固定指令，
//! 经 [`Engine::tick`](crate::settle::Engine::tick) 驱动）；真执行器在
//! kk-runtime（`PlayerExecutor` 面，per-viewer 世界投影进出）——Sandbox 的
//! 真实现/桥接随 B0-5 协调器接线落定。引擎不依赖 deno_core，单元测试
//! 不必拖着真 V8 跑。

use std::collections::BTreeMap;

use crate::intent::IntentBatch;

/// 结算前的世界只读视图（M1 最小面：整个 typed 世界；沙箱按需读房间）。
pub struct WorldView<'a> {
    pub world: &'a kk_world::World,
}

/// 一次玩家执行的输入（代码 + 身份；世界投影由实现方按需从 [`WorldView`]
/// 生成——假沙箱零投影开销，真沙箱内部 world_json_for_viewer）。
pub struct RunSpec {
    pub player: String,
    /// 玩家代码（screeps-ok runPlayer 通道；假沙箱忽略）。
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

/// 玩家运行结局（tick 契约「状态与可见性」：玩家结局与基础设施故障必须
/// 分类——前者依官方语义产出结果、不中止整个 tick；后者走 shard 停顿）。
#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    /// 正常执行完毕（玩家逻辑自身的错误由 expr/console 携带）。
    Executed,
    /// 硬中断/停跑（计费已按官方语义结算）。
    Terminated,
    /// 世界注入失败（玩家把环境搞坏——玩家级失败，不冒泡整轮；B0-5 接线）。
    InjectFailed,
    /// 基础设施故障（线程失联/协议损坏）——协调器转整轮停顿（Err）。
    InfraFailure(String),
}

impl RunOutcome {
    /// 是否为有定义的玩家结局（协调器继续本轮；InfraFailure 停顿）。
    pub fn is_player_outcome(&self) -> bool {
        !matches!(self, RunOutcome::InfraFailure(_))
    }
}

/// 一次玩家执行的候选结果（tick 契约「状态与可见性」的最小形态）：**无持久
/// 副作用**——世界与玩家持久状态（RawMemory/CPU 桶）在协调器统一提交点一次
/// 切换；候选字段（cpu_billed_ms/raw_memory）由提交点消费，B0-5 接线生产。
#[derive(Debug, Clone)]
pub struct RunResult {
    /// 玩家身份（执行器按注册身份填；结算侧仍做冒名过滤）。
    pub player: String,
    pub outcome: RunOutcome,
    /// 该玩家的意图批（已按玩家身份清洗；空批 = 无意图/硬中断丢弃）。
    pub intents: IntentBatch,
    /// runExpr 结果 JSON（screeps-ok 通道：`{ok, value?} | {ok:false, kind, error}`）。
    pub expr: serde_json::Value,
    /// CPU 计费候选（end_tick 产出的 used+intents 计费；None = 本轮未产出）。
    pub cpu_billed_ms: Option<f64>,
    /// RawMemory 候选（Memory 写回产物；None = 本轮未产出/写回失败）。
    pub raw_memory: Option<String>,
    /// segments 信封候选（M2B 扩面 P6：{writes,active?,public?,defaultPublic?,
    /// foreign?}——窗口写回+四请求快照；None = 写回失败/未达）。
    pub segments_out: Option<serde_json::Value>,
    /// 本次执行期间的控制台输出。
    pub console: Vec<String>,
}

impl RunResult {
    /// 注入失败的占位构造（执行器在生产侧无法完成注入时；B0-5 接线）。
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

    /// 基础设施故障占位构造（线程失联等；协调器见此转整轮停顿）。
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

/// 玩家代码沙箱的统一执行接口（B0 WP1，ADR-013 决策 1）：视图进、候选
/// RunResult 出。实现方负责把「玩家看到的」与「玩家想做的」隔离：视图只读，
/// 产出的 intent 一律视为不可信（结算前过白名单清洗）。
pub trait Sandbox: Send {
    fn run(&mut self, view: &WorldView<'_>, spec: &RunSpec) -> RunResult;
}

/// 固定指令脚本假实现（同接口）：按 tick 序回放固定意图批。
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
