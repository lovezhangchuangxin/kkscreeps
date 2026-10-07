//! sim 级周期检查点（G26.a）：到点保存、保留策略与 v10 报告字段装配。
//! 保存时机与恢复选择的语义契约见 docs/architecture/checkpoints.md
//! 「周期保存与恢复选择」。
use super::checkpoint::{self, LogCursor};
use super::*;

/// 周期保存的运行内统计（报告 auto_checkpoint 字段的来源）。
#[derive(Default)]
pub(super) struct Stats {
    saved: u32,
    last_tick: Option<u64>,
    save_ms: Vec<f64>,
    deleted: u32,
    delete_errors: Vec<String>,
}

/// 一次到点保存尝试的全部输入：检查点身份束与本次运行选项。
pub(super) struct Periodic<'a> {
    pub(super) engine: &'a EmbedEngine,
    pub(super) every: u32,
    pub(super) dir: &'a Path,
    pub(super) keep: Option<u32>,
    pub(super) log: Option<&'a Path>,
    pub(super) scenario_sha256: &'a str,
    pub(super) executable_sha256: &'a str,
    pub(super) modules: &'a BTreeMap<String, Value>,
    pub(super) aliases: &'a BTreeMap<String, String>,
    pub(super) initial: &'a Value,
}

/// 到点保存：此刻=本轮已提交、日志段已写、无开放轮次（配对规则「检查
/// 点不得领先日志头」天然满足）。保存失败以 stage=checkpoint 的 failure
/// 值返回，由调用方终结运行（耐久承诺未达成不静默继续，旧版不被替
/// 换）；mods_switch 与周期保存互斥（校验保证），已生效集恒空。
pub(super) fn save_if_due(input: &Periodic, stats: &mut Stats) -> Option<Value> {
    let time = input.engine.get_game_time();
    if !time.is_multiple_of(u64::from(input.every)) {
        return None;
    }
    // 游标与日志末段严格一致：段 tick 为结算前世界时间，结算成功后写
    // 段并使世界 +1，故此刻 time-1 恰为末段 tick。
    let cursor = input
        .log
        .and(input.engine.input_log_segments())
        .map(|last_seq| LogCursor {
            last_seq,
            last_tick: time - 1,
        });
    let saved_at = std::time::Instant::now();
    let outcome = checkpoint::save(
        input.engine,
        &input.dir.join(checkpoint::checkpoint_file_name(time)),
        input.scenario_sha256,
        input.executable_sha256,
        input.modules,
        input.aliases,
        input.initial,
        &[],
        cursor,
    );
    match outcome {
        Ok(()) => {
            stats.saved += 1;
            stats.last_tick = Some(time);
            stats
                .save_ms
                .push(saved_at.elapsed().as_secs_f64() * 1000.0);
            enforce_keep(input.dir, input.keep, stats);
            None
        }
        Err(error) => Some(json!({
            "stage": "checkpoint", "tick": time,
            "error": format!("{error:#}"),
        })),
    }
}

/// 保留策略：超出 keep 的最旧份删除；清点与删除失败只记录不致命（残
/// 留旧文件不影响正确性——选择器逐份信封校验，读取失败跳次新；目录
/// 中途不可读属运行期 I/O 异常，不应把已提交轮次伪装成 setup_failed）。
fn enforce_keep(dir: &Path, keep: Option<u32>, stats: &mut Stats) {
    let Some(keep) = keep else { return };
    let mut ticks = match checkpoint::list_checkpoint_ticks(dir) {
        Ok(ticks) => ticks,
        Err(error) => {
            stats.delete_errors.push(format!("清点目录: {error}"));
            return;
        }
    };
    ticks.sort_unstable();
    let excess = ticks.len().saturating_sub(keep as usize);
    for tick in &ticks[..excess] {
        match std::fs::remove_file(dir.join(checkpoint::checkpoint_file_name(*tick))) {
            Ok(()) => stats.deleted += 1,
            Err(error) => stats.delete_errors.push(format!("tick {tick}: {error}")),
        }
    }
}

/// v10 报告字段：周期保存摘要（RPO 声明兑现「报告所选耐久策略」）。
/// last_tick 为保存时刻世界时间（与所选份 log_cursor.last_tick 差一：
/// 后者是该份对应日志末段的 tick）。
pub(super) fn record(every: u32, dir: Option<&Path>, keep: Option<u32>, stats: &Stats) -> Value {
    json!({
        "every": every,
        "dir": dir,
        "saved": stats.saved,
        "last_tick": stats.last_tick,
        "keep": keep,
        "deleted": stats.deleted,
        "delete_errors": stats.delete_errors,
        "save_ms_head": stats.save_ms.iter().take(5).collect::<Vec<_>>(),
        "rpo": format!("仅检查点：最多丢 {every} tick；配合 --input-log 恢复可到日志尾（保存后崩溃 RPO=0）——不承诺零丢失"),
    })
}

/// v10 报告字段：目录恢复记录（所选文件/检查点时间/保存时游标/重放段
/// 数/跳过坏份数）。
pub(super) fn recovery_record(found: &checkpoint::Recovery) -> Value {
    json!({
        "selected": found.file,
        "checkpoint_time": found.checkpoint_time,
        "log_cursor": found.log_cursor.as_ref().map(|cursor| json!({
            "last_seq": cursor.last_seq, "last_tick": cursor.last_tick,
        })),
        "replayed_segments": found.replay_segments.len(),
        "skipped_broken": found.skipped_broken,
    })
}
