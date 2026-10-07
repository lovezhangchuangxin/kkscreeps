//! sim 级输入日志摘要与进程内回放核对（G25.b 起）。契约细节见
//! docs/architecture/input-log.md。
use super::*;

/// 进程内回放核对（G25.b）：restore 起点快照后逐段重放，比较整份检查点
/// （剥离 last_console——回放无沙箱执行；剥离 metadata——调用方自定）。
/// CPU 桶按记录桶逐位参与比较（ADR-013 决策 5，不做 freeze 处理）。
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
        let data = kk_runtime::embed::read_input_log(log)?;
        let start_time = start["engine"]["world"]["time"]
            .as_u64()
            .context("起点快照缺少世界时间")?;
        let segments = kk_runtime::embed::segments_from_checkpoint(start_time, &data.segments)?;
        let replay_count = segments.len();
        let final_live = engine.checkpoint(Value::Null)?;
        engine.restore_checkpoint(start.clone(), mods)?;
        engine.replay_input_rounds(&data.header, segments)?;
        let replayed = engine.checkpoint(Value::Null)?;
        let mut live = final_live;
        let mut back = replayed;
        for value in [&mut live, &mut back] {
            let object = value.as_object_mut().expect("检查点是对象");
            object.remove("last_console");
            object.remove("metadata");
        }
        if live == back {
            Ok(json!({
                "status": "passed",
                "segments": replay_count,
                "first_tick": segments.first().and_then(|s| s["tick"].as_u64()),
                "last_tick": segments.last().and_then(|s| s["tick"].as_u64()),
                "note": "整份检查点逐位相等（剥离 last_console/metadata；CPU 按记录桶比较）",
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

/// 输入日志的运行后摘要：整读校验（链/摘要/段号），写读两侧段数并
/// 列以便对账；写侧计数为 None 表示运行中 I/O 失败已停用日志。
pub(super) fn input_log_summary(engine: &EmbedEngine, path: &Path) -> Value {
    match engine.input_log_segments() {
        Some(segments) => match kk_runtime::embed::read_input_log(path) {
            Ok(data) => {
                let first_tick = data.segments.first().and_then(|s| s["tick"].as_u64());
                let last_tick = data.segments.last().and_then(|s| s["tick"].as_u64());
                json!({
                    "path": path, "segments": segments,
                    "read_back_segments": data.segments.len(),
                    "first_tick": first_tick, "last_tick": last_tick,
                    "validation": "ok",
                })
            }
            Err(error) => json!({
                "path": path, "segments": segments, "validation": "failed",
                "validation_error": error.to_string(),
            }),
        },
        // None＝运行期日志已摘除：包切换/检查点恢复/重建分片（生命周期
        // 语义）或写入失败（failure 里带「已提交」错误文案）都会走到这里；
        // 磁盘文件仍在，照常整读校验并按实际段数对账。
        None => match kk_runtime::embed::read_input_log(path) {
            Ok(data) => json!({
                "path": path, "status": "detached",
                "read_back_segments": data.segments.len(),
                "first_tick": data.segments.first().and_then(|s| s["tick"].as_u64()),
                "last_tick": data.segments.last().and_then(|s| s["tick"].as_u64()),
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
