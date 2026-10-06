//! 注册期包模块装载器：字节来自 ModRealm 装载时的包内存快照，不触碰磁盘。
//! 装载面只接受包内相对导入与 `@kkscreeps/mod` 虚拟模块。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource,
    ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind, resolve_import,
};
use deno_error::JsErrorBox;

use crate::package::ModPackage;

/// SDK 虚拟模块的说明符与解析后的 URL。
pub(crate) const SDK_SPECIFIER: &str = "@kkscreeps/mod";
pub(crate) const SDK_URL: &str = "kkmods:sdk";

// 固定表防止 Mod 猜测内部 URL 绕过注册入口。解析器只为 SDK 引用者开放。
fn sdk_source(url: &str) -> Option<&'static str> {
    match url {
        SDK_URL => Some(include_str!("sdk.js")),
        "kkmods:sdk/state.js" => Some(include_str!("sdk/state.js")),
        "kkmods:sdk/registration.js" => Some(include_str!("sdk/registration.js")),
        "kkmods:sdk/rules.js" => Some(include_str!("sdk/rules.js")),
        "kkmods:sdk/refresh.js" => Some(include_str!("sdk/refresh.js")),
        "kkmods:sdk/chains.js" => Some(include_str!("sdk/chains.js")),
        "kkmods:sdk/resources.js" => Some(include_str!("sdk/resources.js")),
        "kkmods:sdk/objects.js" => Some(include_str!("sdk/objects.js")),
        "kkmods:sdk/movement.js" => Some(include_str!("sdk/movement.js")),
        "kkmods:sdk/combat.js" => Some(include_str!("sdk/combat.js")),
        "kkmods:sdk/build.js" => Some(include_str!("sdk/build.js")),
        "kkmods:sdk/determinism.js" => Some(include_str!("sdk/determinism.js")),
        _ => None,
    }
}

/// 包内模块加载器：字节来自装载期的内存快照，注册期不触碰磁盘。
/// 只允许包内相对导入与 `@kkscreeps/mod`；跨包导入与裸导入拒绝。
pub(crate) struct PackageModuleLoader {
    pub(crate) packages: Rc<RefCell<BTreeMap<PathBuf, Rc<ModPackage>>>>,
}

impl PackageModuleLoader {
    fn load_sync(
        &self,
        specifier: &ModuleSpecifier,
        maybe_referrer: Option<&ModuleLoadReferrer>,
    ) -> Result<ModuleSource, JsErrorBox> {
        if let Some(source) = sdk_source(specifier.as_str()) {
            let sdk = source.as_bytes().to_vec();
            return Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::Bytes(sdk.into_boxed_slice().into()),
                specifier,
                None,
            ));
        }
        if specifier.scheme() != "file" {
            return Err(JsErrorBox::generic(format!(
                "仅支持包内 file 模块：{specifier}"
            )));
        }
        let path = specifier
            .to_file_path()
            .map_err(|_| JsErrorBox::generic(format!("非法模块路径：{specifier}")))?;
        let package = self
            .packages
            .borrow()
            .values()
            .find(|package| path.starts_with(&package.root))
            .cloned();
        let Some(package) = package else {
            return Err(JsErrorBox::generic(format!(
                "模块在全部包根之外：{}",
                path.display()
            )));
        };
        if let Some(referrer) = maybe_referrer
            && referrer.specifier.scheme() == "file"
        {
            let referrer_path = referrer
                .specifier
                .to_file_path()
                .map_err(|_| JsErrorBox::generic("非法引用者路径"))?;
            if !referrer_path.starts_with(&package.root) {
                return Err(JsErrorBox::generic(format!(
                    "跨包导入被拒绝：{} 引用 {}",
                    referrer_path.display(),
                    path.display()
                )));
            }
        }
        let relative = path
            .strip_prefix(&package.root)
            .map_err(|_| JsErrorBox::generic("模块不在包根内"))?;
        let bytes = package
            .file(relative)
            .ok_or_else(|| JsErrorBox::generic(format!("包内没有模块：{}", path.display())))?;
        let code = String::from_utf8(bytes.to_vec())
            .map_err(|_| JsErrorBox::generic(format!("模块须为 UTF-8：{}", path.display())))?;
        Ok(ModuleSource::new(
            ModuleType::JavaScript,
            ModuleSourceCode::Bytes(code.into_bytes().into_boxed_slice().into()),
            specifier,
            None,
        ))
    }
}

impl ModuleLoader for PackageModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if specifier == SDK_SPECIFIER || specifier == SDK_URL {
            return ModuleSpecifier::parse(SDK_URL)
                .map_err(|error| JsErrorBox::generic(error.to_string()));
        }
        // 动态导入与 import.meta.resolve 一并拒绝：动态导入的根装载没有
        // referrer，跨包一致性检查结构性缺席，静态导入图是唯一的装载形态。
        if matches!(kind, ResolutionKind::DynamicImport) {
            return Err(JsErrorBox::generic(
                "不支持动态导入：注册面只接受静态导入（包内相对路径与 @kkscreeps/mod）",
            ));
        }
        if referrer.starts_with("kkmods:") {
            if let Some(module) = specifier.strip_prefix("./") {
                let url = format!("{SDK_URL}/{module}");
                if sdk_source(&url).is_some() {
                    return ModuleSpecifier::parse(&url).map_err(JsErrorBox::from_err);
                }
            }
            return Err(JsErrorBox::generic("SDK 只允许固定内部静态模块"));
        }
        let absolute = specifier.contains("://");
        if absolute && !specifier.starts_with("file://") {
            return Err(JsErrorBox::generic(format!(
                "不支持的外部导入 {specifier:?}：仅包内相对路径与 @kkscreeps/mod"
            )));
        }
        if !absolute {
            let bare = !specifier.starts_with("./")
                && !specifier.starts_with("../")
                && !specifier.starts_with('/');
            if bare {
                return Err(JsErrorBox::generic(format!(
                    "不支持的外部导入 {specifier:?}：仅包内相对路径与 @kkscreeps/mod"
                )));
            }
        }
        // 绝对 file URL 是宿主装载入口的形态；模块自身的绝对 file 导入
        // 由 load 侧的包根包含约束拦截，二者共用此分支。
        resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        ModuleLoadResponse::Sync(self.load_sync(specifier, maybe_referrer))
    }
}
