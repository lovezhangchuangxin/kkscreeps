//! 玩家求值生命周期：注入、共享预算表达式/主代码、候选写回。
use super::*;

/// 执行注入、表达式与写回，返回待协调器提交的结果；原样回带 epoch。
pub(super) fn run_eval_tick(
    sandbox: &mut Sandbox,
    world: &Value,
    bundle: Option<KkpvPrivateBundle>,
    public: Option<crate::shared_bundle::SharedKkpvBundle>,
    code: &str,
    epoch: RunEpoch,
) -> anyhow::Result<EvalReport> {
    run_eval_tick_with_budget(sandbox, world, bundle, public, code, None, epoch, None)
}

/// 控制台表达式结果的序列化字节上限：plain-JSON 门之外的总量界。
const CONSOLE_EXPR_MAX_BYTES: usize = 64 * 1024;

fn clamp_console_expr(value: Value) -> Value {
    match serde_json::to_string(&value) {
        Ok(text) if text.len() <= CONSOLE_EXPR_MAX_BYTES => value,
        _ => serde_json::json!({
            "ok": false, "kind": "serialization",
            "error": format!("表达式结果超出 {CONSOLE_EXPR_MAX_BYTES} 字节上限"),
        }),
    }
}

#[allow(clippy::too_many_arguments)] // 内部求值协议：通道/输入/身份/预算各一参，收束会引入装箱
pub(super) fn run_eval_tick_with_budget(
    sandbox: &mut Sandbox,
    world: &Value,
    bundle: Option<KkpvPrivateBundle>,
    public: Option<crate::shared_bundle::SharedKkpvBundle>,
    code: &str,
    pre_expr: Option<&str>,
    epoch: RunEpoch,
    budget: Option<&kk_core::cpu::RunBudget>,
) -> anyhow::Result<EvalReport> {
    // 世界注入失败作为该玩家的 inject_failed 结果返回，让协调器继续处理其他玩家。
    // 通道断开等基础设施故障仍返回 Err。
    let begin = match (channel_input(bundle, public)?, budget) {
        (ChannelInput::Kkpv { public, private }, Some(budget)) => {
            sandbox.begin_tick_kkpv_with_budget(world, &public, &private, budget)
        }
        (ChannelInput::Kkpv { public, private }, None) => {
            sandbox.begin_tick_kkpv(world, &public, &private)
        }
        (ChannelInput::Json, Some(budget)) => sandbox.begin_tick_with_budget(world, budget),
        (ChannelInput::Json, None) => sandbox.begin_tick(world),
    };
    let plan = match begin {
        Ok(plan) => plan,
        Err(e) => {
            // 注入失败的费用候选按完整预算计算，报告携带同一候选；此处尚不修改余额。
            let bill = sandbox.pending_bill();
            return Ok(EvalReport {
                expr: serde_json::json!({
                    "ok": false, "kind": "inject_failed",
                    "error": format!("世界注入失败: {e}"),
                }),
                console_expr: None,
                console: sandbox.drain_console(),
                bucket_after: sandbox.bucket(),
                intents: Value::Null,
                cpu_billed_ms: bill,
                raw_memory: None,
                ism_local: None,
                segments_out: None,
                epoch,
            });
        }
    };
    match plan {
        TickPlan::SkipPenalty => Ok(EvalReport {
            // 代码未执行时使用 terminated，区别于成功求值得到 undefined。
            // 停跑回充留给协调器提交，bucket_after 仍是提交前余额。
            expr: serde_json::json!({
                "ok": false, "kind": "terminated", "error": "本 tick 停跑（桶透支为负，玩家代码未执行）",
            }),
            // 停跑轮表达式同样未执行：结果按"未求值"归 None，由命令面
            // 下一拍重新挂起求值，不补造结果。
            console_expr: None,
            console: sandbox.drain_console(),
            bucket_after: sandbox.bucket(),
            intents: Value::Null,
            cpu_billed_ms: None,
            raw_memory: None,
            ism_local: None,
            segments_out: None,
            epoch,
        }),
        TickPlan::Run { .. } => {
            // 控制台表达式（S2.3）：主代码前按命令序求值，共用本次注入
            // 与预算。软异常由 runExpr 归为有界错误结果并继续主代码；
            // 硬中断与主代码同路径（预算共用，后续自然停跑/终止）。
            let console_expr = match pre_expr {
                Some(source) => Some(clamp_console_expr(sandbox.run_expr(source)?)),
                None => None,
            };
            // run_expr 的硬中断会解除看门狗并清 V8 终止标志，不能再执行
            // 主代码，否则它会在无期限的窗口中运行（甚至永久阻塞）。
            let mut expr = match &console_expr {
                Some(result) if result["kind"] == "terminated" => result.clone(),
                _ => sandbox.run_expr(code)?,
            };
            let writeback = sandbox.end_tick()?;
            let intents = sandbox.take_intents().unwrap_or(Value::Null);
            // 代码开始执行后被中断，或写回失败，按适配器契约归为 runtime。
            // 只有上面的负余额停跑分支使用 terminated 表示尚未执行。
            if expr.get("ok").and_then(Value::as_bool) == Some(false)
                && expr.get("kind").and_then(Value::as_str) == Some("terminated")
            {
                expr = serde_json::json!({
                    "ok": false, "kind": "runtime",
                    "error": "硬中断（超时或内存闸门，代码已开始执行）",
                });
            } else if expr.get("ok").and_then(Value::as_bool) == Some(true)
                && writeback.get("ok").and_then(Value::as_bool) == Some(false)
            {
                expr = serde_json::json!({
                    "ok": false, "kind": "runtime",
                    "error": writeback.get("error").cloned().unwrap_or_else(|| "Memory 写回失败".into()),
                });
            }
            Ok(EvalReport {
                expr,
                console_expr,
                console: sandbox.drain_console(),
                bucket_after: sandbox.bucket(),
                intents,
                // 计费与 RawMemory 均作为候选返回；协调器在世界结算成功后提交。
                cpu_billed_ms: sandbox.pending_bill(),
                segments_out: writeback.get("segmentsOut").cloned(),
                raw_memory: writeback
                    .get("rawMemory")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                ism_local: writeback
                    .get("ismLocal")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                epoch,
            })
        }
    }
}
