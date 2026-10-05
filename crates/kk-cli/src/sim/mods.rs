//! 场景中的显式 Mod 输入、实际包身份与自有对象初始化。
use super::*;
use kk_mod_host::{ContentSelection, ModPackage, RegisterOptions, discover_packages};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Mods {
    root: PathBuf,
    #[serde(default)]
    configs: BTreeMap<String, Value>,
    /// 新世界启动期的显式内容选择；省略表示安装全部发现的包。
    /// 与宿主注册选项、mod inspect 共用同一选择语义。
    #[serde(default)]
    selection: ContentSelection,
}

pub(super) struct Loaded {
    pub packages: BTreeMap<String, ModPackage>,
    pub options: RegisterOptions,
    pub metadata: Value,
}

pub(super) fn load(base: &Path, input: &Mods) -> anyhow::Result<Loaded> {
    anyhow::ensure!(!input.root.as_os_str().is_empty(), "mods.root 不能为空");
    let root = std::fs::canonicalize(base.join(&input.root)).context("读取 Mod 包根目录")?;
    let packages = discover_packages(&root)?;
    for key in input.configs.keys() {
        anyhow::ensure!(packages.contains_key(key), "Mod 配置指定了未加载的包 {key}");
    }
    let entries: BTreeMap<_, _> = packages
        .iter()
        .map(|(id, package)| {
            let manifest = &package.manifest;
            (
                id.clone(),
                json!({
                    "version": manifest.version, "api_version": manifest.api_version,
                    "dependencies": manifest.dependencies, "directory": package.root,
                    "code_sha256": hex(package.code_digest()),
                    "player_api_sha256": hex(package.player_api_digest()),
                    "config_sha256": hex(ModPackage::config_digest(input.configs.get(id))),
                }),
            )
        })
        .collect();
    // mods 元数据记录的是输入（含显式选择）；展开后的实际安装清单与
    // 自动加入项由组合报告的 selection 提供，不在两处复制结论。
    let selection = serde_json::to_value(&input.selection).context("序列化场景的选择输入失败")?;
    Ok(Loaded {
        packages,
        options: RegisterOptions {
            configs: input.configs.clone(),
            selection: input.selection.clone(),
            ..Default::default()
        },
        metadata: json!({"root": root, "packages": entries, "selection": selection}),
    })
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ModObject {
    type_key: String,
    fields: Value,
}

pub(super) fn place(engine: &mut EmbedEngine, object: &Object) -> anyhow::Result<String> {
    if object.kind != "mod" {
        return engine.place_object(&object.room, &object.kind, &object.spec);
    }
    let spec: ModObject =
        serde_json::from_value(object.spec.clone()).context("解析 Mod 对象 spec")?;
    engine.place_mod_object(&object.room, &spec.type_key, &spec.fields)
}
