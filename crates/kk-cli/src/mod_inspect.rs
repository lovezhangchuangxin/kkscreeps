//! 作者使用真实包注册路径检查组合；停用参数仅生成执行预览，
//! 选择参数与 sim 场景、宿主注册选项共用同一展开语义。
use crate::report::write_report;
use anyhow::Context;
use kk_mod_host::{ContentSelection, ModRealm, RegisterOptions, discover_packages};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub struct Options {
    pub root: PathBuf,
    pub configs: Option<PathBuf>,
    pub rule_sources: Vec<String>,
    pub disable: Vec<String>,
    pub select: Vec<String>,
    pub exclude: Vec<String>,
    pub report: PathBuf,
}

pub fn run(options: Options) -> i32 {
    let output = match output_path(&options) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("mod inspect: {error:#}");
            return 1;
        }
    };
    let result = inspect(&options);
    let (code, report) = match result {
        Ok(report) => (0, report),
        Err(error) => (
            1,
            json!({"report_version":INSPECT_REPORT_VERSION,"status":"setup_failed","error":format!("{error:#}"),"execution_strategy":null,"composition":null}),
        ),
    };
    if let Err(error) = write_report(&output, &report) {
        eprintln!("mod inspect 报告写入失败: {error:#}");
        return 1;
    }
    if code != 0 {
        eprintln!("mod inspect: {}", report["error"]);
    }
    code
}

/// mod inspect 报告 schema 版本：passed 与 setup_failed 两条路径同源
/// 引用（v5 起顶层输出 execution_strategy）。
const INSPECT_REPORT_VERSION: u32 = 5;
fn output_path(options: &Options) -> anyhow::Result<PathBuf> {
    crate::output_guard::json_report(&options.report, &options.root, options.configs.as_deref())?;
    crate::output_guard::normalized(&options.report)
}

fn inspect(options: &Options) -> anyhow::Result<Value> {
    let root = std::fs::canonicalize(&options.root).context("读取 Mod 包根目录")?;
    let packages = discover_packages(&root)?;
    let configs = match &options.configs {
        Some(path) => serde_json::from_slice::<BTreeMap<String, Value>>(
            &std::fs::read(path).context("读取 Mod 配置")?,
        )
        .context("配置须为按包 ID 分组的 JSON 对象")?,
        None => BTreeMap::new(),
    };
    for id in configs.keys() {
        anyhow::ensure!(packages.contains_key(id), "Mod 配置指定了未加载的包 {id}");
    }
    let selection = ContentSelection {
        include: options.select.iter().cloned().collect(),
        exclude: options.exclude.iter().cloned().collect(),
    };
    let mut register_options = RegisterOptions {
        configs,
        selection,
        ..Default::default()
    };
    for input in &options.rule_sources {
        let (key, source) = input
            .split_once('=')
            .context("rule-source 须为 key=namespace")?;
        let key = kk_mod_api::Key::new(key).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            register_options
                .freeze
                .rule_sources
                .insert(key, source.into())
                .is_none(),
            "rule-source 重复指定规则"
        );
    }
    let mut realm = ModRealm::with_options(&register_options)?;
    let registered = realm.register(&packages, register_options)?;
    let requested: BTreeSet<_> = options.disable.iter().cloned().collect();
    let composition = registered
        .composition
        .with_disabled(&requested)
        .map_err(anyhow::Error::msg)?;
    let identities: BTreeMap<_, _> = packages
        .iter()
        .map(|(id, package)| {
            (
                id,
                json!({
                    "directory": package.root,
                    "version": package.manifest.version,
                    "api_version": package.manifest.api_version,
                    "dependencies": package.manifest.dependencies,
                }),
            )
        })
        .collect();
    let executable = std::env::current_exe()?;
    let hash = Sha256::digest(std::fs::read(executable)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    // 执行策略身份与 sim 报告同源（G22.c 跟进项）：作者可离线预判
    // 停用对身份的影响；停用预览后按同口径重算。
    let execution_strategy = composition
        .execution_strategy_digest()
        .map_err(anyhow::Error::msg)?;
    Ok(
        json!({"report_version":INSPECT_REPORT_VERSION,"status":"passed","root":root,"packages":identities,"executable_sha256":hash,"requested_disabled":requested,"execution_strategy":execution_strategy,"composition":composition}),
    )
}
