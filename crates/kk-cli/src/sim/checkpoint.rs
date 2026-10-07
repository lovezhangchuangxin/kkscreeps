//! sim 检查点保存场景身份、原始断言起点与 alias；续跑不重新放置或重置 Memory。
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    scenario_sha256: String,
    executable_sha256: String,
    modules: Value,
    aliases: BTreeMap<String, String>,
    initial: Value,
    /// 已实际生效的包切换边界（after_tick）：续跑据此选恢复用包集合——
    /// 被拒绝的切换不改变身份，越过边界不等于携带切换后身份。
    #[serde(default)]
    applied_switches: Vec<u64>,
    /// 保存时刻的日志配对游标（G26.a）：最后一段的 seq 与 tick，显式
    /// 表达「检查点不领先日志头」。未挂日志的保存为 None。
    #[serde(default)]
    log_cursor: Option<LogCursor>,
}

/// 日志配对游标：seq 从 1 起按文件内段数计（续接文件含既有段）。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LogCursor {
    pub last_seq: u64,
    pub last_tick: u64,
}

pub(super) fn resume(
    engine: &mut EmbedEngine,
    value: Value,
    loaded: Option<&mods::Loaded>,
    scenario_sha256: &str,
    executable_sha256: &str,
    modules: &BTreeMap<String, Value>,
) -> anyhow::Result<(BTreeMap<String, String>, Value)> {
    let metadata: Metadata =
        serde_json::from_value(value["metadata"].clone()).context("解析 sim 检查点元数据")?;
    anyhow::ensure!(
        metadata.executable_sha256 == executable_sha256,
        "检查点 CLI 二进制身份不匹配；尚未提供升级迁移"
    );
    anyhow::ensure!(
        metadata.scenario_sha256 == scenario_sha256,
        "检查点场景身份不匹配"
    );
    anyhow::ensure!(
        metadata.modules == serde_json::to_value(modules)?,
        "检查点玩家模块身份不匹配"
    );
    engine.restore_checkpoint(
        value,
        loaded.map(|loaded| (&loaded.packages, &loaded.options)),
    )?;
    Ok((metadata.aliases, metadata.initial))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn save(
    engine: &EmbedEngine,
    path: &Path,
    scenario_sha256: &str,
    executable_sha256: &str,
    modules: &BTreeMap<String, Value>,
    aliases: &BTreeMap<String, String>,
    initial: &Value,
    applied_switches: &[u64],
    log_cursor: Option<LogCursor>,
) -> anyhow::Result<()> {
    engine.save_checkpoint(
        path,
        serde_json::to_value(Metadata {
            scenario_sha256: scenario_sha256.into(),
            executable_sha256: executable_sha256.into(),
            modules: serde_json::to_value(modules)?,
            aliases: aliases.clone(),
            initial: initial.clone(),
            applied_switches: applied_switches.to_vec(),
            log_cursor,
        })?,
    )
}

/// 周期检查点目录的文件名形态（tick 定宽排序即恢复优先序）。
pub(super) fn checkpoint_file_name(tick: u64) -> String {
    format!("ckpt-{tick:010}.json")
}

/// 规范命名（`checkpoint_file_name` 形态）的自有文件 tick 清单：外部
/// 放置的非规范命名文件不属于本策略管理的对象，选择器与保留策略都
/// 忽略它们（既不删除也不计入坏份计数）。
pub(super) fn list_checkpoint_ticks(dir: &Path) -> anyhow::Result<Vec<u64>> {
    let ticks = std::fs::read_dir(dir)
        .with_context(|| format!("清点周期检查点目录 {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let tick = name
                .strip_prefix("ckpt-")?
                .strip_suffix(".json")?
                .parse()
                .ok()?;
            (name == checkpoint_file_name(tick)).then_some(tick)
        })
        .collect();
    Ok(ticks)
}

/// 「最新可用」恢复选择（G26.a）：按 tick 降序逐份信封校验，坏文件跳
/// 次新并计数；配对失败（缺口/接缝/检查点超出日志末段+1）是日志与检查
/// 点的关系问题而非文件损坏——硬错误不降级。撕裂日志在读取处失败并
/// 携带 valid_segments 供人工截断，不自动改写文件。
pub(super) struct Recovery {
    pub value: Value,
    pub file: PathBuf,
    pub checkpoint_time: u64,
    /// 保存时的日志游标（旧文件可能没有）。
    pub log_cursor: Option<LogCursor>,
    /// 恢复后需重放的日志段（零段=合法终态：保存后即崩的形态）。
    pub replay_segments: Vec<Value>,
    pub header: Value,
    pub skipped_broken: usize,
}

pub(super) fn recover_latest(dir: &Path, log: &Path) -> anyhow::Result<Recovery> {
    let data = kk_runtime::embed::read_input_log(log).map_err(|error| {
        // 撕裂尾是可人工修复的形态（按前缀段数截断后重试）；其他读取
        // 错误（不存在/权限等）附截断提示会误导，只透出原始错误。
        if format!("{error}").contains("撕裂") {
            anyhow::anyhow!(
                "输入日志不可读，恢复中止：{error}——撕裂尾可按前缀段数人工截断后重试（本批不自动改写）"
            )
        } else {
            anyhow::anyhow!("输入日志不可读，恢复中止：{error}")
        }
    })?;
    let last_tick = data
        .segments
        .last()
        .and_then(|segment| segment["tick"].as_u64())
        .context("日志没有任何段，无从与检查点配对")?;
    let mut ticks = list_checkpoint_ticks(dir)?;
    ticks.sort_unstable();
    ticks.reverse();
    anyhow::ensure!(
        !ticks.is_empty(),
        "周期检查点目录 {} 没有可用文件",
        dir.display()
    );
    let mut skipped = 0usize;
    for tick in ticks {
        let file = dir.join(checkpoint_file_name(tick));
        let value = match kk_runtime::checkpoint::read_checkpoint(&file) {
            Ok(value) => value,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let time = value["engine"]["world"]["time"]
            .as_u64()
            .context("检查点载荷缺少世界时间")?;
        anyhow::ensure!(
            time <= last_tick + 1,
            "检查点 tick {time} 超出日志末段 {last_tick}+1（日志曾在检查点之前被摘除，世界连续性断裂）——恢复中止"
        );
        let replay_segments = if time == last_tick + 1 {
            Vec::new()
        } else {
            kk_runtime::embed::segments_from_checkpoint(time, &data.segments)?.to_vec()
        };
        // 游标按同一 deny_unknown_fields 结构严格解码：畸形游标在此早
        // 失败并指名，而非以 0 值混过（后续 resume 的 Metadata 门虽也
        // 会拦，错误归因不如这里明确）。
        let log_cursor = value["metadata"]["log_cursor"]
            .is_object()
            .then(|| {
                serde_json::from_value::<LogCursor>(value["metadata"]["log_cursor"].clone())
                    .context("检查点元数据的 log_cursor 畸形")
            })
            .transpose()?;
        return Ok(Recovery {
            value,
            file,
            checkpoint_time: time,
            log_cursor,
            replay_segments,
            header: data.header.clone(),
            skipped_broken: skipped,
        });
    }
    anyhow::bail!("周期检查点目录全部 {} 份文件损坏", skipped)
}

/// 输出检查先于执行与失败报告写入；共享路径解析不创建任何目录。
pub(super) fn validate_paths(options: &Options) -> anyhow::Result<()> {
    use crate::output_guard::{distinct, normalized, outside_packages};
    distinct(&options.report, &options.scenario)?;
    for input in [
        options.load_checkpoint.as_ref(),
        options.save_checkpoint.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        distinct(input, &options.report).context("检查点路径不能与报告相同")?;
        distinct(input, &options.scenario).context("检查点路径不能与场景相同")?;
    }
    // 输入日志也是运行输出：与报告/检查点的临时文件+原子替换写互踩会
    // 静默毁掉对方，与场景同路径则吞掉输入。
    if let Some(log) = options.input_log.as_ref() {
        distinct(log, &options.report).context("输入日志路径不能与报告相同")?;
        distinct(log, &options.scenario).context("输入日志路径不能与场景相同")?;
        for input in [
            options.load_checkpoint.as_ref(),
            options.save_checkpoint.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            distinct(log, input).context("输入日志路径不能与检查点相同")?;
        }
    }
    // 周期检查点目录（G26.a）：目录内的文件由自动策略管理（命名/清理），
    // 其他输出不得指进目录；目录本身也不得覆盖任何输入或其他输出。
    if let Some(dir) = options.checkpoint_dir.as_ref() {
        let dir = normalized(dir)?;
        for other in [
            Some(&options.report),
            options.save_checkpoint.as_ref(),
            options.load_checkpoint.as_ref(),
            options.input_log.as_ref(),
            Some(&options.scenario),
        ]
        .into_iter()
        .flatten()
        {
            let other = normalized(other)?;
            anyhow::ensure!(
                !other.starts_with(&dir),
                "{} 不得位于周期检查点目录内（目录由自动保留策略管理）",
                other.display()
            );
            anyhow::ensure!(
                !dir.starts_with(&other),
                "周期检查点目录不得覆盖 {}",
                other.display()
            );
        }
    }
    // 这里只读路径声明，即使后续场景校验或包装载失败也保护输入。
    // 解析失败仍由 execute 输出原有 setup_failed 诊断。
    if let Ok(bytes) = std::fs::read(&options.scenario)
        && let Ok(scene) = serde_json::from_slice::<Value>(&bytes)
    {
        let scenario = normalized(&options.scenario)?;
        let base = scenario.parent().context("场景路径缺少父目录")?;
        let mut roots = Vec::new();
        if let Some(root) = scene["mods"]["root"].as_str() {
            roots.push(base.join(root));
        }
        for bot in scene["bots"].as_array().into_iter().flatten() {
            if let Some(root) = bot["modules"].as_str() {
                roots.push(base.join(root));
            }
        }
        // 输入日志与检查点同为运行期输出，同样不得落进包根（会污染输入）。
        for output in std::iter::once(&options.report)
            .chain(options.save_checkpoint.iter())
            .chain(options.input_log.iter())
            .chain(options.checkpoint_dir.iter())
        {
            for root in &roots {
                outside_packages(output, root)?;
            }
        }
    }
    Ok(())
}

pub(super) fn protect_sources(
    path: &Path,
    roots: impl Iterator<Item = PathBuf>,
) -> anyhow::Result<()> {
    for root in roots {
        crate::output_guard::outside_packages(path, &root)?;
    }
    Ok(())
}
