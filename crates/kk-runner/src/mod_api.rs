//! Mod 玩家面的沙箱侧数据与模块加载器。
//!
//! [`ModPlayerApi`] 是纯数据清单（宿主从冻结计划与包快照导出，经
//! `SandboxConfig` 注入）；`PlayerModuleLoader` 在玩家 VM 内按
//! `kkmodplayer://{namespace}/{path}` 说明符解析包内文件——字节与注册期
//! 内存快照同源（Arc 共享，不逐玩家复制），装载纪律与注册 realm 同构：
//! 仅包内相对导入、拒动态导入、拒跨命名空间导入与查询串/锚点。

use std::collections::BTreeMap;
use std::sync::Arc;

use deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource,
    ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind, resolve_import,
};
use deno_error::JsErrorBox;

/// 一个包的玩家模块集：入口（包内相对路径）与全部可装载源文件。
#[derive(Debug, Clone)]
pub struct ModPlayerModule {
    pub namespace: String,
    pub entry: String,
    pub files: BTreeMap<String, String>,
}

/// Mod 玩家面安装清单。`types` 的 FIND 常量由冻结计划确定性分配
/// （下界 `find_base` 随清单注入沙箱，Rust 侧是唯一权威来源）；
/// `intents` 是玩家可发出的 Mod 意图键（emit 预检目录）。
#[derive(Debug, Clone)]
pub struct ModPlayerApi {
    /// 依依赖拓扑序（被依赖者在前）。Arc 共享：装载器与沙箱配置引用
    /// 同一份，不逐玩家复制源码。
    pub modules: Arc<Vec<ModPlayerModule>>,
    /// (类型键, FIND 常量, LOOK 常量)。
    pub types: Vec<(String, i64, String)>,
    pub intents: Vec<String>,
}

/// 玩家模块的虚拟方案：命名空间作为 URL authority（manifest id 字符集
/// 为小写字母数字与 `_-`，经 URL 规范化不变义），路径为包内相对路径。
pub(crate) const PLAYER_SCHEME: &str = "kkmodplayer";

pub(crate) fn entry_specifier(module: &ModPlayerModule) -> Result<ModuleSpecifier, String> {
    let url = format!("{}://{}/{}", PLAYER_SCHEME, module.namespace, module.entry);
    ModuleSpecifier::parse(&url).map_err(|error| format!("玩家模块入口说明符非法：{error}"))
}

/// 玩家 VM 的模块加载器：只解析清单内包的字节，静态导入图是唯一装载
/// 形态（动态导入拒绝，同注册 realm 的纪律）。持有 `Arc` 共享清单，
/// 不逐沙箱复制源码。
pub(crate) struct PlayerModuleLoader {
    pub modules: Arc<Vec<ModPlayerModule>>,
}

impl PlayerModuleLoader {
    fn load_sync(&self, specifier: &ModuleSpecifier) -> Result<ModuleSource, JsErrorBox> {
        if specifier.scheme() != PLAYER_SCHEME {
            return Err(JsErrorBox::generic(format!(
                "玩家 VM 仅装载 {PLAYER_SCHEME} 模块：{specifier}"
            )));
        }
        let namespace = specifier.host_str().ok_or_else(|| {
            JsErrorBox::generic(format!("玩家模块说明符缺少命名空间：{specifier}"))
        })?;
        let path = specifier.path();
        let Some(path) = path.strip_prefix('/') else {
            return Err(JsErrorBox::generic(format!(
                "玩家模块路径须为包内相对路径：{specifier}"
            )));
        };
        let module = self
            .modules
            .iter()
            .find(|module| module.namespace == namespace)
            .ok_or_else(|| JsErrorBox::generic(format!("未知玩家模块命名空间：{namespace}")))?;
        // URL 路径是百分号编码形态（含空格/非 ASCII 的文件名），解码后
        // 才是包内文件键；解码失败按不可装载处理。
        let decoded = percent_decode(path);
        let source = module.files.get(&decoded).ok_or_else(|| {
            JsErrorBox::generic(format!(
                "包 {namespace} 内没有玩家模块：{path}（快照内文件才可装载）"
            ))
        })?;
        Ok(ModuleSource::new(
            ModuleType::JavaScript,
            ModuleSourceCode::Bytes(source.clone().into_bytes().into_boxed_slice().into()),
            specifier,
            None,
        ))
    }
}

/// URL 路径的百分号解码（%XX → 字节）；无效序列保持原样由键查找失败
/// 兜底。ASCII 路径（常规形态）原样通过。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl ModuleLoader for PlayerModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if matches!(kind, ResolutionKind::DynamicImport) {
            return Err(JsErrorBox::generic(
                "不支持动态导入：玩家面只接受静态导入（包内相对路径）",
            ));
        }
        let referrer_namespace = referrer
            .strip_prefix(&format!("{PLAYER_SCHEME}://"))
            .and_then(|rest| rest.split('/').next())
            .map(str::to_string);
        if let Some(rest) = specifier.strip_prefix(&format!("{PLAYER_SCHEME}://"))
            && rest.contains('/')
        {
            // 模块内部的绝对说明符只允许回到本包（同注册 realm 的包根
            // 核对纪律）：跨包导入会绕过依赖拓扑在他包玩家 VM 执行代码。
            // 入口装载也经此分支（referrer 非本方案）——宿主发起的入口
            // 命名空间恒在清单内，放行；模块内部（referrer 为本方案）才
            // 做同包核对。
            let resolved = ModuleSpecifier::parse(specifier)
                .map_err(|error| JsErrorBox::generic(error.to_string()))?;
            let target = resolved.host_str().unwrap_or_default();
            if let Some(namespace) = &referrer_namespace
                && namespace != target
            {
                return Err(JsErrorBox::generic(format!(
                    "跨包导入被拒绝：{specifier}（自 {referrer}；依赖须在 manifest 声明）"
                )));
            }
            // 查询串/锚点会让同一文件得到第二个模块缓存键（双实例）。
            if resolved.query().is_some() || resolved.fragment().is_some() {
                return Err(JsErrorBox::generic(format!(
                    "玩家模块说明符不得携带查询串或锚点：{specifier}"
                )));
            }
            return Ok(resolved);
        }
        if referrer.starts_with(&format!("{PLAYER_SCHEME}://")) {
            let resolved = resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)?;
            if resolved.scheme() != PLAYER_SCHEME {
                return Err(JsErrorBox::generic(format!(
                    "玩家模块只允许包内相对导入：{specifier}（自 {referrer}）"
                )));
            }
            // 查询串/锚点会让同一文件得到第二个模块缓存键（双实例、双份
            // 顶层状态），一律拒绝。
            if resolved.query().is_some() || resolved.fragment().is_some() {
                return Err(JsErrorBox::generic(format!(
                    "玩家模块说明符不得携带查询串或锚点：{specifier}"
                )));
            }
            return Ok(resolved);
        }
        Err(JsErrorBox::generic(format!(
            "玩家 VM 不装载外部模块：{specifier}"
        )))
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        ModuleLoadResponse::Sync(self.load_sync(specifier))
    }
}
