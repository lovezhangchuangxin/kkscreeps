//! 运行协调层（ADR-013 决策 1，B0 WP1）：玩家执行编排（派发、报告收集、
//! 失败分类、提交）从 kk-engine 的 embed 上移至此。
//!
//! 职责边界：
//! - **玩家执行只产出候选 [`RunResult`]**（intents / Memory / CPU / 运行结局 /
//!   控制台），无持久副作用；
//! - **世界与玩家持久状态在协调器统一提交点一次切换**（tick 契约「状态与
//!   可见性」——消灭部分提交：世界结算失败回退时先执行玩家的状态不再泄漏）；
//! - **玩家级失败玩家级恢复**（不冒泡整轮）；基础设施故障走 shard 停顿；
//! - 真/假沙箱同一执行接口（[`PlayerExecutor`]），deno_core/V8 编译依赖只
//!   留在本 crate 与 kk-runner（kk-engine 编译期摆脱它是 B0-5 收口目标）。
//!
//! B0-4（本片）交付类型与执行器骨架；统一提交点与 embed 编排迁移随 B0-5。

pub mod embed;
pub mod world_source;

use std::collections::BTreeMap;

use kk_engine::intent::IntentBatch;

// 统一执行类型的消费面 re-export（D2：kk-cli/适配器经 kk-runtime 取型，
// 不必直接依赖 kk-engine 的 sandbox 模块）
pub use embed::{EmbedEngine, PlayerRun};
pub use kk_engine::{RunOutcome, RunResult, RunSpec, Sandbox as EngineSandbox};
use serde_json::Value;
pub use world_source::WorldSource;

/// 玩家执行器的协调层抽象（真/假沙箱同接口）：一次执行 = 一名玩家一轮的
/// 候选 RunResult。实现方负责世界投影的生成与身份清洗（intents 归注册玩家）。
pub trait PlayerExecutor {
    /// 执行一名玩家一轮（世界投影为 per-viewer JSON）。玩家结局与基础设施
    /// 故障一律经 [`RunOutcome`] 分类承载（未知玩家/线程失联等派发失败也折入
    /// InfraFailure——协调器见此整轮停顿）。
    fn execute(&mut self, world: &Value, spec: &RunSpec) -> RunResult;

    /// 注册玩家名（固定序遍历——确定性的前提）。
    fn players(&self) -> Vec<String>;
}

/// 真沙箱执行器：包装 kk-runner 的 PlayerThreads（一玩家一线程）。
///
/// 失败分类（B0-4 现状，B0-5 随 proto.rs 通道分类细化）：
/// - `run_player` Ok → [`RunOutcome::Executed`] / [`RunOutcome::Terminated`]
///   （按 expr.kind 判定，与 PlayerRun.executed 同口径）；
/// - `run_player` Ok 且 expr.kind=="inject_failed" → [`RunOutcome::InjectFailed`]
///   （玩家把环境搞坏——玩家级，不冒泡整轮；B0-5 起实装）；
/// - `run_player` Err → [`RunOutcome::InfraFailure`]（线程失联等基础设施故障）。
pub struct ThreadExecutor {
    pool: kk_runner::PlayerThreads,
    /// 玩家 handle → 线程 id（BTreeMap：固定遍历序）。
    handles: BTreeMap<String, u64>,
    next_id: u64,
}

impl ThreadExecutor {
    pub fn new() -> Self {
        Self {
            pool: kk_runner::PlayerThreads::new(),
            handles: BTreeMap::new(),
            next_id: 0,
        }
    }

    /// 启动玩家线程并注册（等待创建回执——注册序 = 创建序）。
    pub fn spawn(&mut self, player: &str, config: kk_runner::SandboxConfig) -> anyhow::Result<()> {
        self.next_id += 1;
        let id = self.next_id;
        self.pool.spawn_player(id, config)?;
        self.handles.insert(player.to_string(), id);
        Ok(())
    }

    /// 注册（或替换）玩家模块表。
    pub fn set_modules(
        &mut self,
        player: &str,
        table: kk_runner::ModuleTable,
    ) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.set_modules(id, table)
    }

    /// 注入玩家 RawMemory 初值（tick 边界；B0-5 提交点统一驱动）。
    pub fn set_raw_memory(&mut self, player: &str, data: &str) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.set_raw_memory(id, data)
    }

    /// 提交玩家的计费候选（统一提交点：世界结算成功后——计费落桶）。
    pub fn settle_bill(&mut self, player: &str) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.settle_bill(id)
    }

    /// 丢弃玩家的计费候选（结算失败回退——玩家桶不扣）。
    pub fn discard_bill(&mut self, player: &str) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.discard_bill(id)
    }

    /// 读玩家桶当前值（测试断言面——O3 embed 停跑回归锁消费）。
    #[doc(hidden)]
    pub fn bucket(&mut self, player: &str) -> anyhow::Result<f64> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.bucket(id)
    }

    pub fn len(&self) -> usize {
        self.handles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    pub fn shutdown_all(&mut self) {
        self.pool.shutdown_all();
        self.handles.clear();
    }
}

impl Default for ThreadExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ThreadExecutor {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

impl PlayerExecutor for ThreadExecutor {
    fn execute(&mut self, world: &Value, spec: &RunSpec) -> RunResult {
        let Some(&id) = self.handles.get(&spec.player) else {
            return RunResult::infra_failure(&spec.player, format!("未注册玩家 {}", spec.player));
        };
        match self.pool.run_player(id, world, &spec.code) {
            Ok(report) => eval_report_to_run_result(&spec.player, report),
            // 保守分类（B0-5 拆分注入失败为玩家级）
            Err(e) => RunResult::infra_failure(&spec.player, e.to_string()),
        }
    }

    fn players(&self) -> Vec<String> {
        self.handles.keys().cloned().collect()
    }
}

/// EvalReport（kk-runner 通道报告）→ RunResult：intents 清洗（IntentBatch::
/// from_json 按玩家键——冒名面结构上不存在）、结局分类、PlayerRun 投影的
/// 数据源（expr/console 原样携带）。
fn eval_report_to_run_result(player: &str, report: kk_runner::EvalReport) -> RunResult {
    // 结局分类：停跑（代码未执行）= Terminated；注入失败（玩家把环境搞坏，
    // B0-5 玩家级）= InjectFailed；软异常 runtime = Executed（错误经 expr 携带）
    let outcome = match report.expr.get("kind").and_then(Value::as_str) {
        Some("terminated") => RunOutcome::Terminated,
        Some("inject_failed") => RunOutcome::InjectFailed,
        _ => RunOutcome::Executed,
    };
    let intents = match report.intents.as_object() {
        Some(list) => IntentBatch::from_json(player, &Value::Object(list.clone())),
        None => IntentBatch::default(),
    };
    RunResult {
        player: player.to_string(),
        outcome,
        intents,
        expr: report.expr,
        // 候选（B0-5 统一提交点）：end_tick 产出的计费与 RawMemory 由协调器
        // 在世界结算成功后提交
        cpu_billed_ms: report.cpu_billed_ms,
        raw_memory: report.raw_memory,
        console: report.console.iter().map(|l| l.text.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 假执行器（同接口的测试面）：固定 expr/intents 构造。
    struct FakeExecutor {
        player: String,
        expr: Value,
    }

    impl PlayerExecutor for FakeExecutor {
        fn execute(&mut self, _world: &Value, spec: &RunSpec) -> RunResult {
            RunResult {
                player: spec.player.clone(),
                outcome: RunOutcome::Executed,
                intents: IntentBatch::default(),
                expr: self.expr.clone(),
                cpu_billed_ms: None,
                raw_memory: None,
                console: Vec::new(),
            }
        }
        fn players(&self) -> Vec<String> {
            vec![self.player.clone()]
        }
    }

    #[test]
    fn executor_interface_shapes() {
        let mut ex = FakeExecutor {
            player: "p1".into(),
            expr: json!({ "ok": true, "value": 42 }),
        };
        assert_eq!(ex.players(), vec!["p1".to_string()]);
        let r = ex.execute(&json!({}), &RunSpec::new("p1", "6*7"));
        assert_eq!(r.outcome, RunOutcome::Executed);
        assert_eq!(r.expr["value"], 42);
        assert!(r.intents.users.is_empty());
    }

    #[test]
    fn outcome_classification() {
        assert!(RunOutcome::Executed.is_player_outcome());
        assert!(RunOutcome::Terminated.is_player_outcome());
        assert!(RunOutcome::InjectFailed.is_player_outcome());
        assert!(!RunOutcome::InfraFailure("x".into()).is_player_outcome());
        let r = RunResult::infra_failure("p1", "线程失联");
        assert_eq!(r.outcome, RunOutcome::InfraFailure("线程失联".into()));
        let r = RunResult::inject_failed("p1", "注入失败");
        assert_eq!(r.expr["kind"], "inject_failed");
    }

    /// 占位构造器的空批不变量（B0-4 评审清偿）：inject_failed/infra_failure
    /// 恒不携带 intents/候选字段——Engine::tick 结局门依赖此前提。
    #[test]
    fn placeholder_results_carry_no_intents() {
        for r in [
            RunResult::inject_failed("p1", "x"),
            RunResult::infra_failure("p1", "x"),
        ] {
            assert!(r.intents.users.is_empty(), "{:?} 空批", r.outcome);
            assert_eq!(r.cpu_billed_ms, None);
            assert_eq!(r.raw_memory, None);
            assert!(r.console.is_empty());
        }
    }

    /// eval_report_to_run_result 的结局分类（B0-4 评审清偿）：与 embed.rs
    /// finish_player_run 的 executed 口径逐字一致——kind=="terminated" →
    /// Terminated（含 SkipPenalty 停跑），runtime 软异常/正常 → Executed
    /// （玩家逻辑错误经 expr 携带，不单列结局）。
    #[test]
    fn eval_report_outcome_matches_player_run() {
        use serde_json::json;
        let mk = |kind: &str| kk_runner::EvalReport {
            expr: json!({ "ok": false, "kind": kind, "error": "e" }),
            console: vec![kk_runner::LogLine {
                level: "log".into(),
                text: "hi".into(),
            }],
            bucket_after: 1.0,
            intents: serde_json::Value::Null,
            cpu_billed_ms: None,
            raw_memory: None,
        };
        let r = eval_report_to_run_result("p1", mk("terminated"));
        assert_eq!(r.outcome, RunOutcome::Terminated);
        assert!(r.intents.users.is_empty(), "Null intents → 空批");
        let r = eval_report_to_run_result("p1", mk("runtime"));
        assert_eq!(
            r.outcome,
            RunOutcome::Executed,
            "软异常=已执行（expr 携带错误）"
        );
        assert_eq!(r.console, vec!["hi".to_string()]);
    }

    /// 真执行器与 kk-runner 线程池的接线（真实 V8）。**测试纪律**：本 crate
    /// 的 V8 测试目前仅此一个、无进程级计数断言，独立 #[test] 无碍；未来新增
    /// V8 场景（尤其带 live_sandbox_count/lifo_violations 断言）须并入单一
    /// 测试函数串行（kk-runner tests 同款纪律——进程级计数器并行污染）。
    #[test]
    fn thread_executor_roundtrip() -> anyhow::Result<()> {
        let mut ex = ThreadExecutor::new();
        ex.spawn("p1", kk_runner::SandboxConfig::default())?;
        let world = json!({
            "time": 1,
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "user": "p1",
            "rooms": { "W1N1": { "name": "W1N1" } },
            "objects": []
        });
        let r = ex.execute(&world, &RunSpec::new("p1", "1 + 1"));
        assert_eq!(r.outcome, RunOutcome::Executed, "简单表达式正常执行");
        assert_eq!(r.expr["value"], 2);
        assert!(r.intents.users.is_empty());
        // 未知玩家：协调层错误（非 RunOutcome 形态的兜底——infra 分类）
        let r = ex.execute(&world, &RunSpec::new("ghost", ""));
        assert!(!r.outcome.is_player_outcome(), "未注册玩家按 infra 兜底");
        ex.shutdown_all();
        Ok(())
    }
}
