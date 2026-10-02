use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{ModHostError, ModHostResult};
use kk_mod_api::Key;

/// 本宿主实现的 SDK 主版本；`apiVersion` 声明必须与它兼容。
pub const SDK_API_VERSION: (u64, u64, u64) = (1, 0, 0);

/// 包 id 必须能充当稳定键的命名空间段；借 Key 的构造校验，
/// 避免宿主私持一份字符规则拷贝。
fn valid_namespace(value: &str) -> bool {
    Key::new(format!("{value}:probe")).is_ok()
}

/// 解析后的 `x.y.z` 版本；预发布/构建后缀不接受（包版本是身份的一部分，
/// 后缀语义没有服务器侧消费方，模糊输入直接拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(value: &str, context: &str) -> ModHostResult<Self> {
        let parts: Vec<&str> = value.split('.').collect();
        if parts.len() != 3 {
            return Err(ModHostError::new(
                context,
                format!("版本须为 x.y.z 三段数字，实得 {value:?}"),
            ));
        }
        let mut numbers = [0u64; 3];
        for (slot, part) in parts.iter().enumerate() {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ModHostError::new(
                    context,
                    format!("版本段须为纯数字，实得 {value:?}"),
                ));
            }
            numbers[slot] = part
                .parse()
                .map_err(|_| ModHostError::new(context, format!("版本数值溢出：{value:?}")))?;
        }
        Ok(Self {
            major: numbers[0],
            minor: numbers[1],
            patch: numbers[2],
        })
    }
}

/// 支持的范围形式：`*`、精确 `x.y.z`、插入符 `^x.y.z`、`>=x.y.z`。
/// 解析不了的要求直接拒绝——静默放宽范围等于允许未声明的兼容性。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionReq {
    Any,
    Exact(Version),
    Caret(Version),
    AtLeast(Version),
}

impl VersionReq {
    pub fn parse(value: &str, context: &str) -> ModHostResult<Self> {
        let value = value.trim();
        if value == "*" {
            return Ok(Self::Any);
        }
        if let Some(rest) = value.strip_prefix('^') {
            return Ok(Self::Caret(Version::parse(rest, context)?));
        }
        if let Some(rest) = value.strip_prefix(">=") {
            return Ok(Self::AtLeast(Version::parse(rest, context)?));
        }
        if value.bytes().any(|b| !b.is_ascii_digit() && b != b'.') {
            return Err(ModHostError::new(
                context,
                format!("不支持的范围写法 {value:?}（可用 *、x.y.z、^x.y.z、>=x.y.z）"),
            ));
        }
        Ok(Self::Exact(Version::parse(value, context)?))
    }

    pub fn satisfied_by(&self, version: Version) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(required) => &version == required,
            Self::Caret(required) => {
                if required.major > 0 {
                    version >= *required && version.major == required.major
                } else {
                    // 0.x 阶段的插入符按次版本收口；npm 对 0.0.x 会进一步
                    // 收口到 patch，这里不细分——宿主与包作者的约定以本实现为准。
                    version >= *required && version.minor == required.minor
                }
            }
            Self::AtLeast(required) => version >= *required,
        }
    }
}

/// mod.json 的严格子集；未实现的服务侧/客户端字段按未知字段拒绝，
/// 能力补齐时再显式放开，避免装出“字段被接受但无效果”的包。
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModManifest {
    pub id: String,
    pub version: String,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub player: Option<String>,
}

fn default_entry() -> String {
    "server.js".to_string()
}

impl ModManifest {
    pub fn parse(text: &str, context: &str) -> ModHostResult<Self> {
        let manifest: Self = serde_json::from_str(text)
            .map_err(|error| ModHostError::new(context, format!("mod.json 解析失败：{error}")))?;
        if !valid_namespace(&manifest.id) || manifest.id == "builtin" {
            return Err(ModHostError::new(
                context,
                format!("包 id {:?} 须为合法命名空间且不得为 builtin", manifest.id),
            ));
        }
        let api_req = VersionReq::parse(&manifest.api_version, context)?;
        if !api_req.satisfied_by(Version {
            major: SDK_API_VERSION.0,
            minor: SDK_API_VERSION.1,
            patch: SDK_API_VERSION.2,
        }) {
            return Err(ModHostError::new(
                context,
                format!(
                    "apiVersion {:?} 与宿主 SDK {}.{}.{} 不兼容",
                    manifest.api_version, SDK_API_VERSION.0, SDK_API_VERSION.1, SDK_API_VERSION.2
                ),
            ));
        }
        Version::parse(&manifest.version, context)?;
        manifest.check_module_path(&manifest.entry, "entry")?;
        if let Some(player) = &manifest.player {
            manifest.check_module_path(player, "player")?;
            if *player == manifest.entry {
                return Err(ModHostError::new(
                    context,
                    "player 与 entry 不得指向同一文件（该文件会被剔出服务端代码摘要）",
                ));
            }
        }
        for (dependency, range) in &manifest.dependencies {
            if !valid_namespace(dependency) || dependency == "builtin" {
                return Err(ModHostError::new(
                    context,
                    format!("依赖 id {dependency:?} 无效"),
                ));
            }
            VersionReq::parse(range, context)?;
        }
        Ok(manifest)
    }

    /// 入口/玩家模块路径只允许包内相对的纯 JS 路径；
    /// 绝对路径、盘符、反斜杠与 `..` 上跳在清单期即拒绝。
    fn check_module_path(&self, path: &str, field: &str) -> ModHostResult<()> {
        let invalid = path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.contains(':')
            || std::path::Path::new(path)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || !matches!(
                std::path::Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str()),
                Some("js" | "mjs")
            );
        if invalid {
            return Err(ModHostError::new(
                self.id.clone(),
                format!("{field} 路径 {path:?} 须为包内相对的 .js/.mjs 路径"),
            ));
        }
        Ok(())
    }
}
