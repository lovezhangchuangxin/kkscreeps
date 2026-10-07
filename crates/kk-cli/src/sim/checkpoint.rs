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
        })?,
    )
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
