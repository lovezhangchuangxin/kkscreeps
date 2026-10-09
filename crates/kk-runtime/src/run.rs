//! 玩家执行参数、结果分类与尚未提交的状态候选。
//!
//! 这些类型属于执行编排层；kk-engine 只接收指令和玩家写回候选。

use kk_engine::intent::IntentBatch;

/// 一次执行的注册玩家身份和代码。
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
/// 此结构不自行提交状态。kk-runtime 从中构造 Memory、segments 和 CPU 桶
/// 写回候选，随世界统一发布；玩家 JavaScript 全局变量不在此事务内。
#[derive(Debug, Clone)]
pub struct RunResult {
    /// 执行器提供的玩家身份；调用方仍须按注册身份绑定指令。
    pub player: String,
    pub outcome: RunOutcome,
    /// 经身份绑定的指令批；未执行或中断时通常为空。
    pub intents: IntentBatch,
    /// 沙箱报告的原始意图 JSON（G25.a 输入日志记录面）：与 intents 的
    /// 转换同源（`IntentBatch::from_json`），是回放的权威输入形态。
    pub intents_raw: serde_json::Value,
    /// runExpr 结果 JSON（screeps-ok 通道：`{ok, value?} | {ok:false, kind, error}`）。
    pub expr: serde_json::Value,
    /// 本轮 CPU 计费候选，单位毫秒；None 表示没有产出。
    pub cpu_billed_ms: Option<f64>,
    /// 本轮 RawMemory 写回候选；None 表示未产出或写回失败。
    pub raw_memory: Option<String>,
    /// InterShardMemory 本地段候选；仅 setLocal 触发的拍为 Some。
    pub ism_local: Option<String>,
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
            intents_raw: serde_json::Value::Null,
            expr: serde_json::json!({
                "ok": false, "kind": "inject_failed", "error": error.into(),
            }),
            cpu_billed_ms: None,
            raw_memory: None,
            ism_local: None,
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
            intents_raw: serde_json::Value::Null,
            expr: serde_json::Value::Null,
            cpu_billed_ms: None,
            raw_memory: None,
            ism_local: None,
            segments_out: None,
            console: Vec::new(),
        }
    }
}
