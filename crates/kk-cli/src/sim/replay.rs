//! sim 级输入日志摘要与进程内回放核对（G25.b 起）。契约细节见
//! docs/architecture/input-log.md。
use anyhow::Context;
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// 进程内回放核对（G25.b）：restore 起点快照后逐段重放，比较整份检查点
/// （剥离 last_console——回放无沙箱执行；剥离 metadata——调用方自定）。
/// CPU 桶按记录桶逐位参与比较（ADR-013 决策 5，不做 freeze 处理）。
/// 玩家 Memory 不随日志重建——比较前剥离两侧 raw_memory，以
/// 「终态 Memory 摘要 == 回放窗口内最后非空摘要」对账；日志读取为流
/// 式扫描，只驻留检查点之后的可回放后缀。
pub(super) fn verify_in_process(
    engine: &mut EmbedEngine,
    start: &Value,
    log: &Path,
    mods: Option<(
        &BTreeMap<String, kk_mod_host::ModPackage>,
        &kk_mod_host::RegisterOptions,
    )>,
) -> Value {
    let outcome = (|| -> anyhow::Result<Value> {
        let start_time = start["engine"]["world"]["time"]
            .as_u64()
            .context("起点快照缺少世界时间")?;
        let data = if start["metadata"]["log_cursor"].is_null() {
            kk_runtime::embed::scan_input_log(log, kk_runtime::embed::Retain::FromTick(start_time))?
        } else {
            let cursor = serde_json::from_value(start["metadata"]["log_cursor"].clone())?;
            kk_runtime::embed::scan_input_log(
                log,
                kk_runtime::embed::Retain::FromCursor {
                    cursor,
                    time: start_time,
                },
            )?
        };
        let segments = &data.segments;
        let replay_count = segments.len();
        let final_live = engine.checkpoint(Value::Null)?;
        engine.restore_checkpoint(start.clone(), mods)?;
        engine.replay_input_rounds(&data.header, segments)?;
        let replayed = engine.checkpoint(Value::Null)?;
        let mut live = final_live;
        let mut back = replayed;
        // 摘要对账须在剥离前（live 侧 Memory 是对账基准）。
        kk_runtime::embed::verify_logged_memory(segments, &live)?;
        for value in [&mut live, &mut back] {
            let object = value.as_object_mut().expect("检查点是对象");
            object.remove("last_console");
            object.remove("metadata");
            kk_runtime::embed::strip_checkpoint_memory(value);
        }
        if live == back {
            Ok(json!({
                "status": "passed",
                "segments": replay_count,
                "first_tick": segments.first().and_then(|s| s["tick"].as_u64()),
                "last_tick": segments.last().and_then(|s| s["tick"].as_u64()),
                "note": "整份检查点逐位相等（剥离 last_console/metadata/玩家 Memory；CPU 按记录桶比较；Memory 以窗口末摘要对账）",
            }))
        } else {
            let mut diverged: Vec<&String> = live
                .as_object()
                .expect("检查点是对象")
                .keys()
                .filter(|key| live.get(key) != back.get(key))
                .collect();
            diverged.sort_unstable();
            anyhow::bail!("回放终态与直跑终态不等，分歧域：{diverged:?}")
        }
    })();
    match outcome {
        Ok(value) => value,
        Err(error) => json!({"status": "failed", "error": format!("{error:#}")}),
    }
}

/// 输入日志的运行后摘要：流式零保留扫描校验（链/摘要/段号），写读两侧
/// 段数并列以便对账；写侧计数为 None 表示运行中 I/O 失败已停用日志。
pub(super) fn input_log_summary(engine: &EmbedEngine, path: &Path) -> Value {
    let scanned = kk_runtime::embed::scan_input_log(path, kk_runtime::embed::Retain::None);
    match engine.input_log_segments() {
        Some(segments) => match scanned {
            Ok(data) => json!({
                "path": path, "segments": segments,
                "read_back_segments": data.stats.segments,
                "first_tick": data.stats.first_tick, "last_tick": data.stats.last_tick,
                "validation": "ok",
            }),
            Err(error) => json!({
                "path": path, "segments": segments, "validation": "failed",
                "validation_error": error.to_string(),
            }),
        },
        // None＝运行期日志已摘除：包切换/检查点恢复/重建分片（生命周期
        // 语义）或写入失败（failure 里带「已提交」错误文案）都会走到这里；
        // 磁盘文件仍在，照常扫描校验并按实际段数对账。
        None => match scanned {
            Ok(data) => json!({
                "path": path, "status": "detached",
                "read_back_segments": data.stats.segments,
                "first_tick": data.stats.first_tick, "last_tick": data.stats.last_tick,
                "validation": "ok",
                "note": "运行期日志已摘除（包切换/检查点恢复/重建分片，或写入失败——详情见 failure）；此后轮次不记段",
            }),
            Err(error) => json!({
                "path": path, "status": "detached", "validation": "failed",
                "validation_error": error.to_string(),
            }),
        },
    }
}
