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
}

pub(super) fn resume(
    engine: &mut EmbedEngine,
    path: &Path,
    loaded: Option<&mods::Loaded>,
    scenario_sha256: &str,
    executable_sha256: &str,
    modules: &BTreeMap<String, Value>,
) -> anyhow::Result<(BTreeMap<String, String>, Value)> {
    let value = kk_runtime::checkpoint::read_checkpoint(path)?;
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

pub(super) fn save(
    engine: &EmbedEngine,
    path: &Path,
    scenario_sha256: &str,
    executable_sha256: &str,
    modules: &BTreeMap<String, Value>,
    aliases: &BTreeMap<String, String>,
    initial: &Value,
) -> anyhow::Result<()> {
    engine.save_checkpoint(
        path,
        serde_json::to_value(Metadata {
            scenario_sha256: scenario_sha256.into(),
            executable_sha256: executable_sha256.into(),
            modules: serde_json::to_value(modules)?,
            aliases: aliases.clone(),
            initial: initial.clone(),
        })?,
    )
}

/// 先规范化现有父目录，避免报告覆盖检查点、场景或源码；输出目录由调用方准备。
pub(super) fn validate_paths(options: &Options) -> anyhow::Result<()> {
    fn absolute(path: &Path) -> anyhow::Result<PathBuf> {
        if path.exists() {
            return Ok(std::fs::canonicalize(path)?);
        }
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                other => normalized.push(other.as_os_str()),
            }
        }
        if let Some(parent) = normalized.parent().filter(|parent| parent.exists()) {
            return Ok(std::fs::canonicalize(parent)?
                .join(normalized.file_name().context("输出路径缺少文件名")?));
        }
        Ok(normalized)
    }
    let report = absolute(&options.report)?;
    let scenario = absolute(&options.scenario)?;
    for input in [
        options.load_checkpoint.as_ref(),
        options.save_checkpoint.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        let path = absolute(input)?;
        anyhow::ensure!(
            path != report && path != scenario,
            "检查点路径不能与报告或场景相同"
        );
    }
    Ok(())
}

pub(super) fn protect_sources(
    path: &Path,
    roots: impl Iterator<Item = PathBuf>,
) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let destination =
        std::fs::canonicalize(parent)?.join(path.file_name().context("检查点路径缺少文件名")?);
    for root in roots {
        anyhow::ensure!(
            !destination.starts_with(std::fs::canonicalize(root)?),
            "检查点输出不能位于玩家或 Mod 源码目录中"
        );
    }
    Ok(())
}
