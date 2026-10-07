//! 注册输入准备只读包与配置；输出路径检查仍由命令先行执行。
use anyhow::Context;
use kk_mod_host::{ContentSelection, ModPackage, RegisterOptions, discover_packages};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub(super) struct Input {
    pub(super) root: std::path::PathBuf,
    pub(super) packages: BTreeMap<String, ModPackage>,
    pub(super) options: RegisterOptions,
}

pub(super) fn prepare(
    root: &Path,
    configs: Option<&Path>,
    select: &[String],
    exclude: &[String],
) -> anyhow::Result<Input> {
    let root = std::fs::canonicalize(root).context("读取 Mod 包根目录")?;
    let packages = discover_packages(&root)?;
    let configs = match configs {
        Some(path) => serde_json::from_slice::<BTreeMap<String, Value>>(
            &std::fs::read(path).context("读取 Mod 配置")?,
        )
        .context("配置须为按包 ID 分组的 JSON 对象")?,
        None => BTreeMap::new(),
    };
    for id in configs.keys() {
        anyhow::ensure!(packages.contains_key(id), "Mod 配置指定了未加载的包 {id}");
    }
    Ok(Input {
        root,
        packages,
        options: RegisterOptions {
            configs,
            selection: ContentSelection {
                include: select.iter().cloned().collect(),
                exclude: exclude.iter().cloned().collect(),
            },
            ..Default::default()
        },
    })
}
