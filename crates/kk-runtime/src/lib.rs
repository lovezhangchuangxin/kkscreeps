//! 协调玩家沙箱执行与世界结算，为嵌入式调用方提供运行接口。
//!
//! 执行器返回候选指令、Memory 和运行报告；世界结算成功后，协调器提交玩家的
//! Memory、segments 和 CPU 桶候选。结算失败时缓存候选，重试不再次执行代码。
//! 玩家的 V8 全局变量不随世界回退；线程通信失败也不提供跨线程原子提交保证。

pub mod embed;
pub mod world_source;

use std::collections::BTreeMap;

use kk_engine::intent::IntentBatch;

pub use embed::{EmbedEngine, PlayerRun};
pub use kk_engine::{RunOutcome, RunResult, RunSpec, Sandbox as EngineSandbox};
pub use kk_runner::RunEpoch;
use serde_json::Value;
pub use world_source::WorldSource;

/// 玩家执行接口，输入世界 JSON、玩家代码和轮次标识，输出候选运行结果。
/// 实现方应将意图绑定到注册玩家，并区分玩家执行错误与线程通信等基础设施错误。
pub trait PlayerExecutor {
    /// 运行一名玩家，world 应已按该玩家的视野过滤。实现方需核对回执的 epoch；派发失败归 InfraFailure。
    fn execute(&mut self, world: &Value, spec: &RunSpec, epoch: RunEpoch) -> RunResult;

    /// 返回注册玩家名，顺序应稳定。ThreadExecutor 使用玩家名字典序。
    fn players(&self) -> Vec<String>;
}

/// 轮次标识不匹配时返回 InfraFailure，防止旧结果进入当前轮的结算。
fn reject_stale_receipt(player: &str, expected: RunEpoch, got: RunEpoch) -> Option<RunResult> {
    (expected != got).then(|| {
        RunResult::infra_failure(
            player,
            format!(
                "迟到/错轮回执拒收：派发轮 (tick={}, gen={})，回执轮 (tick={}, gen={})",
                expected.tick, expected.generation, got.tick, got.generation
            ),
        )
    })
}

/// 每名玩家使用独立线程及 V8 沙箱，通过通道同步等待命令回执。
///
/// 注入失败和负桶停跑返回玩家结局；代码错误及执行后的硬中断由 expr 携带，
/// 仍归 Executed。未知玩家、通道失败或错轮回执归 InfraFailure。
pub struct ThreadExecutor {
    pool: kk_runner::PlayerThreads,
    /// 玩家名到线程 id 的映射，按玩家名字典序遍历。
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

    /// 创建并注册玩家线程，等待沙箱创建完成。重复玩家名返回错误并保留原沙箱。
    pub fn spawn(&mut self, player: &str, config: kk_runner::SandboxConfig) -> anyhow::Result<()> {
        if self.handles.contains_key(player) {
            anyhow::bail!("玩家 {player} 已注册");
        }
        self.next_id += 1;
        let id = self.next_id;
        self.pool.spawn_player(id, config)?;
        self.handles.insert(player.to_string(), id);
        Ok(())
    }

    /// 替换玩家的模块表；玩家未注册或通道调用失败时返回错误。
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

    /// 设置沙箱下一轮注入的 RawMemory，不修改引擎的 PlayerState。
    pub fn set_raw_memory(&mut self, player: &str, data: &str) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.set_raw_memory(id, data)
    }

    /// 设置下一轮注入的 raw、活动 segments 窗口和 foreignSegment JSON。
    pub fn set_memory_state(
        &mut self,
        player: &str,
        raw: &str,
        segments_json: &str,
        foreign_json: &str,
    ) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool
            .set_memory_state(id, raw, segments_json, foreign_json)
    }

    /// 提交沙箱保存的 CPU 计费或停跑回充候选；协调器在世界结算成功后调用。
    pub fn settle_bill(&mut self, player: &str) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.settle_bill(id)
    }

    /// 丢弃沙箱保存的 CPU 计费和停跑回充候选，不改变当前桶余额。
    pub fn discard_bill(&mut self, player: &str) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.discard_bill(id)
    }

    /// 读取已提交的 CPU 桶余额，不包含尚未提交的计费或回充。
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
    fn execute(&mut self, world: &Value, spec: &RunSpec, epoch: RunEpoch) -> RunResult {
        let Some(&id) = self.handles.get(&spec.player) else {
            return RunResult::infra_failure(&spec.player, format!("未注册玩家 {}", spec.player));
        };
        match self.pool.run_player(id, world, &spec.code, epoch) {
            Ok(report) => {
                if let Some(rejected) = reject_stale_receipt(&spec.player, epoch, report.epoch) {
                    return rejected;
                }
                eval_report_to_run_result(&spec.player, report)
            }
            Err(e) => RunResult::infra_failure(&spec.player, e.to_string()),
        }
    }

    fn players(&self) -> Vec<String> {
        self.handles.keys().cloned().collect()
    }
}

/// 转换线程报告，将意图归到注册玩家名下。
/// expr.kind 为 terminated 或 inject_failed 时分别标记停跑和注入失败；
/// 包括语法、运行和序列化错误在内的其他报告归 Executed。
fn eval_report_to_run_result(player: &str, report: kk_runner::EvalReport) -> RunResult {
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
        // Memory 和段结果暂不写入 PlayerState；计费字段仅用于报告，
        // 实际待提交计费保存在沙箱中，由协调器在世界结算成功后通知提交。
        cpu_billed_ms: report.cpu_billed_ms,
        raw_memory: report.raw_memory,
        segments_out: report.segments_out,
        console: report.console.iter().map(|l| l.text.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct FakeExecutor {
        player: String,
        expr: Value,
    }

    impl PlayerExecutor for FakeExecutor {
        fn execute(&mut self, _world: &Value, spec: &RunSpec, _epoch: RunEpoch) -> RunResult {
            RunResult {
                player: spec.player.clone(),
                outcome: RunOutcome::Executed,
                intents: IntentBatch::default(),
                expr: self.expr.clone(),
                cpu_billed_ms: None,
                raw_memory: None,
                segments_out: None,
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
        let r = ex.execute(&json!({}), &RunSpec::new("p1", "6*7"), RunEpoch::default());
        assert_eq!(r.outcome, RunOutcome::Executed);
        assert_eq!(r.expr["value"], 42);
        assert!(r.intents.users.is_empty());
    }

    /// tick 或 generation 任一不匹配都拒收，同轮回执放行。
    #[test]
    fn stale_receipt_epoch_rejected() {
        let dispatched = RunEpoch {
            tick: 5,
            generation: 3,
        };
        let stale_tick = RunEpoch {
            tick: 4,
            generation: 3,
        };
        let stale_gen = RunEpoch {
            tick: 5,
            generation: 2,
        };
        for stale in [stale_tick, stale_gen] {
            let Some(rejected) = reject_stale_receipt("p1", dispatched, stale) else {
                panic!("错轮回执 {stale:?} 必须拒收");
            };
            assert!(
                matches!(rejected.outcome, RunOutcome::InfraFailure(ref e) if e.contains("拒收")),
                "拒收形态为 InfraFailure：{:?}",
                rejected.outcome
            );
        }
        assert!(reject_stale_receipt("p1", dispatched, dispatched).is_none());
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

    /// 注入失败和基础设施失败的占位结果不携带意图或玩家状态候选。
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

    /// 停跑报告归 Terminated，运行错误归 Executed，并保留错误报告和控制台输出。
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
            segments_out: None,
            epoch: RunEpoch::default(),
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

    /// 验证真实线程执行、重复注册拒绝、原沙箱保留及未知玩家的错误分类。
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
        let r = ex.execute(
            &world,
            &RunSpec::new("p1", "globalThis.registration = 42; 1 + 1"),
            RunEpoch::default(),
        );
        assert_eq!(r.outcome, RunOutcome::Executed, "简单表达式正常执行");
        assert_eq!(r.expr["value"], 2);
        assert!(r.intents.users.is_empty());
        assert!(
            ex.spawn("p1", kk_runner::SandboxConfig::default()).is_err(),
            "重复注册不得替换现有玩家或多建线程"
        );
        assert_eq!(ex.len(), 1);
        let r = ex.execute(
            &world,
            &RunSpec::new("p1", "globalThis.registration"),
            RunEpoch::default(),
        );
        assert_eq!(r.expr["value"], 42, "重复注册失败后保留原沙箱");
        let r = ex.execute(&world, &RunSpec::new("ghost", ""), RunEpoch::default());
        assert!(!r.outcome.is_player_outcome(), "未注册玩家按 infra 兜底");
        ex.shutdown_all();
        Ok(())
    }
}
