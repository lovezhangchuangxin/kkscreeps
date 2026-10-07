//! 玩家线程执行器、回执身份校验及结果转换。
use crate::{RunEpoch, RunOutcome, RunResult, RunSpec};
use kk_engine::intent::IntentBatch;
use serde_json::Value;
use std::collections::BTreeMap;

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
    next_attempt: u64,
    pub(crate) registrations: BTreeMap<String, WorkerRegistration>,
    fail_rebuild_after_first_for_test: bool,
}

pub struct PendingExecution {
    player: String,
    pending: kk_runner::PendingJob,
}
impl PendingExecution {
    pub fn deadline(&self) -> std::time::Instant {
        self.pending.deadline
    }
    pub fn try_take(&self) -> Option<RunResult> {
        match self.pending.try_take() {
            Ok(Some(receipt)) => Some(checked_job_receipt(
                &self.player,
                self.pending.token,
                receipt,
            )),
            Ok(None) => None,
            Err(e) => Some(RunResult::infra_failure(&self.player, e.to_string())),
        }
    }
    pub fn wait(self) -> RunResult {
        let expected = self.pending.token;
        match self.pending.wait() {
            Ok(receipt) => checked_job_receipt(&self.player, expected, receipt),
            Err(e) => RunResult::infra_failure(&self.player, e.to_string()),
        }
    }
}

pub(crate) struct WorkerRegistration {
    pub(crate) config: kk_runner::SandboxConfig,
    pub(crate) modules: kk_runner::ModuleTable,
}

/// 所有维度均须匹配；相同世界版本不能使旧尝试或旧线程的结果合法化。
fn checked_job_receipt(
    player: &str,
    expected: kk_runner::JobToken,
    receipt: kk_runner::RunReceipt,
) -> RunResult {
    if receipt.token != expected || receipt.report.epoch != expected.epoch {
        return RunResult::infra_failure(
            player,
            format!(
                "迟到/错轮回执拒收：期望 {expected:?}，收到 {:?}",
                receipt.token
            ),
        );
    }
    eval_report_to_run_result(player, receipt.report)
}

impl ThreadExecutor {
    pub fn new() -> Self {
        Self {
            pool: kk_runner::PlayerThreads::new(),
            handles: BTreeMap::new(),
            next_id: 0,
            next_attempt: 0,
            registrations: BTreeMap::new(),
            fail_rebuild_after_first_for_test: false,
        }
    }

    /// 执行宿主预算命令，并核对回执所属的世界轮次。
    pub fn execute_job(&mut self, player: &str, job: kk_runner::RunJob) -> RunResult {
        match self.dispatch_job(player, job, None) {
            Ok(pending) => pending.wait(),
            Err(e) => RunResult::infra_failure(player, e.to_string()),
        }
    }
    pub fn dispatch_job(
        &mut self,
        player: &str,
        mut job: kk_runner::RunJob,
        notify: Option<std::sync::mpsc::Sender<kk_runner::JobToken>>,
    ) -> anyhow::Result<PendingExecution> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未注册玩家 {player}"))?;
        self.next_attempt = self
            .next_attempt
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("执行尝试编号耗尽"))?;
        job.attempt = self.next_attempt;
        let pending = self.pool.dispatch_job_notifying(id, job, notify)?;
        Ok(PendingExecution {
            player: player.into(),
            pending,
        })
    }

    /// 建立全部替代线程及模块后才切换线程表；失败时保留原注册表。
    /// 重建丢弃 V8 全局状态。内存和 CPU 预算由下一次 RunJob 从宿主重新注入。
    pub fn rebuild_all(&mut self) -> anyhow::Result<()> {
        let fail_after_first = std::mem::take(&mut self.fail_rebuild_after_first_for_test);
        let mut replacement = kk_runner::PlayerThreads::new();
        let mut handles = BTreeMap::new();
        let mut next_id = self.next_id;
        for (player, registration) in &self.registrations {
            next_id = next_id
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("工作者编号耗尽"))?;
            // 即使重建中途失败也不复用工作者编号，避免未来旧回执与替代线程同名。
            self.next_id = next_id;
            replacement.spawn_player(next_id, registration.config.clone())?;
            replacement.set_modules(next_id, registration.modules.clone())?;
            handles.insert(player.clone(), next_id);
            if fail_after_first {
                anyhow::bail!("故障注入：首个替代工作者建立后重建失败");
            }
        }
        self.handles = handles;
        self.pool = replacement;
        Ok(())
    }

    /// 首个替代线程就绪后返回错误，验证部分重建失败不切换工作者表。
    #[doc(hidden)]
    pub fn fail_next_rebuild_for_test(&mut self) {
        self.fail_rebuild_after_first_for_test = true;
    }

    #[doc(hidden)]
    pub fn gate_next_job_for_test(&mut self, player: &str) -> anyhow::Result<kk_runner::JobGate> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.gate_next_job_for_test(id)
    }

    /// 下一条玩家执行命令不返回回执，用于验证真实线程失联的恢复。
    #[doc(hidden)]
    pub fn exit_before_next_job_for_test(&mut self, player: &str) -> anyhow::Result<()> {
        let id = *self
            .handles
            .get(player)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))?;
        self.pool.exit_before_next_job_for_test(id)
    }

    /// 创建并注册玩家线程，等待沙箱创建完成。重复玩家名返回错误并保留原沙箱。
    pub fn spawn(&mut self, player: &str, config: kk_runner::SandboxConfig) -> anyhow::Result<()> {
        if self.handles.contains_key(player) {
            anyhow::bail!("玩家 {player} 已注册");
        }
        let id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("工作者编号耗尽"))?;
        self.pool.spawn_player(id, config.clone())?;
        self.next_id = id;
        self.registrations.insert(
            player.into(),
            WorkerRegistration {
                config,
                modules: kk_runner::ModuleTable::new(),
            },
        );
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
        self.pool.set_modules(id, table.clone())?;
        self.registrations
            .get_mut(player)
            .expect("注册玩家必有配置")
            .modules = table;
        Ok(())
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

    /// 独立执行通道的计费提交。EmbedEngine 使用宿主桶，此方法不修改宿主状态。
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

    /// 读取线程工作桶；EmbedEngine 的已提交桶在 PlayerState 中，不能用此值替代。
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
        self.registrations.clear();
        self.fail_rebuild_after_first_for_test = false;
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
    let intents_raw = report.intents.clone();
    let intents = match report.intents.as_object() {
        Some(list) => IntentBatch::from_json(player, &Value::Object(list.clone())),
        None => IntentBatch::default(),
    };
    RunResult {
        player: player.to_string(),
        outcome,
        intents,
        intents_raw,
        expr: report.expr,
        // 仅转换回执；协调器计算桶候选并将 Memory、段及桶值交给引擎发布。
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
                intents_raw: serde_json::Value::Null,
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
    fn account_and_market_edits_reject_pre_edit_receipt_epochs() {
        let mut engine = kk_engine::Engine::default();
        for domain in ["account", "market"] {
            let dispatched = RunEpoch {
                tick: engine.world().time(),
                generation: engine.world().generation(),
            };
            if domain == "account" {
                engine
                    .edit_players(|players| {
                        players.entry("p1".into()).or_default().money = 100;
                        Ok(())
                    })
                    .unwrap();
            } else {
                engine.edit_market(|_| Ok(())).unwrap();
            }
            let current = RunEpoch {
                tick: engine.world().time(),
                generation: engine.world().generation(),
            };
            assert_eq!(current.tick, dispatched.tick);
            assert!(
                reject_stale_receipt("p1", current, dispatched).is_some(),
                "{domain} 编辑后须拒收旧回执"
            );
        }
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

    #[test]
    fn rebuilt_workers_preserve_modules_and_reject_actual_old_receipts() -> anyhow::Result<()> {
        let mut ex = ThreadExecutor::new();
        ex.spawn("p1", kk_runner::SandboxConfig::default())?;
        let mut modules = kk_runner::ModuleTable::new();
        modules.insert_js("savedModule", "module.exports={value:7}");
        ex.set_modules("p1", modules)?;
        let code = "globalThis.n=(globalThis.n||0)+1; [globalThis.n,Memory.saved,require('savedModule').value]";
        let job = |attempt| kk_runner::RunJob {
            terrain: None,
            kkpv_bundle: None,
            kkpv_public: None,
            world: json!({"time":0,"rooms":{},"objects":[]}),
            code: code.into(),
            epoch: RunEpoch::default(),
            attempt,
            deadline: None,
            raw_memory: r#"{"saved":42}"#.into(),
            segments_json: "[]".into(),
            foreign_json: "null".into(),
            budget: kk_core::cpu::RunBudget::new(&kk_core::cpu::Bucket::default()),
        };
        let old_worker = ex.handles["p1"];
        let first = ex.pool.dispatch_job(old_worker, job(41))?;
        let second = ex.pool.dispatch_job(old_worker, job(42))?;
        let expected = second.token;
        let second = second.wait()?;
        assert_eq!(second.report.expr["value"], json!([2, 42, 7]));
        let stale = checked_job_receipt("p1", expected, first.wait()?);
        assert!(matches!(stale.outcome, RunOutcome::InfraFailure(_)));
        assert!(stale.intents.users.is_empty());
        assert_eq!(stale.cpu_billed_ms, None);
        let late = ex.pool.dispatch_job(old_worker, job(43))?;
        let old_token = late.token;
        ex.rebuild_all()?;
        let new_worker = ex.handles["p1"];
        assert_ne!(old_worker, new_worker);
        let expected = kk_runner::JobToken {
            worker: new_worker,
            ..old_token
        };
        let stale = checked_job_receipt("p1", expected, late.wait()?);
        assert!(matches!(stale.outcome, RunOutcome::InfraFailure(_)));
        let run = ex.execute_job("p1", job(0));
        assert_eq!(
            run.expr["value"],
            json!([1, 42, 7]),
            "新工作者清空全局变量并保留模块"
        );
        ex.fail_next_rebuild_for_test();
        assert!(ex.rebuild_all().is_err());
        assert_eq!(ex.handles["p1"], new_worker, "失败重建不换入部分线程");
        let run = ex.execute_job("p1", job(0));
        assert_eq!(run.expr["value"], json!([2, 42, 7]));
        ex.exit_before_next_job_for_test("p1")?;
        let run = ex.execute_job("p1", job(0));
        assert!(matches!(run.outcome, RunOutcome::InfraFailure(_)));
        ex.rebuild_all()?;
        assert_eq!(
            ex.execute_job("p1", job(0)).expr["value"],
            json!([1, 42, 7])
        );
        Ok(())
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
