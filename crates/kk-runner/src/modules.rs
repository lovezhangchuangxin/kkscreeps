//! 玩家模块表：模块名 → 源码/二进制。
//!
//! Screeps 玩家上传多个 CommonJS 模块；官方存储里模块名可含 `.` 与 `/`（数据库转义为
//! `$DOT$`/`$SLASH$`，语义上就是原始名）。二进制模块（`.wasm`）的官方语义是
//! `require` 返回 `Buffer`（官方 engine/src/game/game.js 的 binary 模块分支）。

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleKind {
    Js,
    Binary,
}

#[derive(Debug, Clone)]
pub enum ModuleCode {
    Js(String),
    Binary(Vec<u8>),
}

/// 一个玩家的全部模块。Rust 侧持有，沙箱经 op 按名加载（spike 实验 I 验证的模式）。
#[derive(Debug, Clone, Default)]
pub struct ModuleTable {
    modules: HashMap<String, ModuleCode>,
}

impl ModuleTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_js(&mut self, name: impl Into<String>, source: impl Into<String>) {
        self.modules
            .insert(name.into(), ModuleCode::Js(source.into()));
    }

    pub fn insert_binary(&mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.modules
            .insert(name.into(), ModuleCode::Binary(bytes.into()));
    }

    pub fn kind(&self, name: &str) -> Option<ModuleKind> {
        match self.modules.get(name) {
            Some(ModuleCode::Js(_)) => Some(ModuleKind::Js),
            Some(ModuleCode::Binary(_)) => Some(ModuleKind::Binary),
            None => None,
        }
    }

    pub fn load_js(&self, name: &str) -> Option<&str> {
        match self.modules.get(name) {
            Some(ModuleCode::Js(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn load_binary(&self, name: &str) -> Option<&[u8]> {
        match self.modules.get(name) {
            Some(ModuleCode::Binary(b)) => Some(b.as_slice()),
            _ => None,
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// 从 bot 代码目录加载：递归收集 `*.js`（模块名去掉 `.js` 后缀，与主流上传工具一致）
    /// 与 `*.wasm`（二进制模块）。子目录用 `/` 拼入模块名。
    pub fn from_directory(root: &Path) -> std::io::Result<Self> {
        let mut table = Self::new();
        fn walk(table: &mut ModuleTable, root: &Path, rel: &str) -> std::io::Result<()> {
            for entry in std::fs::read_dir(root)? {
                let entry = entry?;
                let path = entry.path();
                let file_name = entry.file_name().into_string().unwrap_or_default();
                if file_name.starts_with('.') {
                    continue;
                }
                let child_rel = if rel.is_empty() {
                    file_name.clone()
                } else {
                    format!("{rel}/{file_name}")
                };
                if path.is_dir() {
                    walk(table, &path, &child_rel)?;
                } else if let Some(name) = file_name.strip_suffix(".js") {
                    let module_name = if rel.is_empty() {
                        name.to_string()
                    } else {
                        format!("{rel}/{name}")
                    };
                    table.insert_js(module_name, std::fs::read_to_string(&path)?);
                } else if file_name.ends_with(".wasm") {
                    table.insert_binary(child_rel, std::fs::read(&path)?);
                }
                // 其余扩展名忽略（地图/配置/资源文件不进模块表）
            }
            Ok(())
        }
        walk(&mut table, root, "")?;
        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_loading() {
        let mut table = ModuleTable::new();
        table.insert_js("main", "module.exports = 1");
        table.insert_binary("main.wasm", [0u8, 1, 2]);
        assert_eq!(table.kind("main"), Some(ModuleKind::Js));
        assert_eq!(table.kind("main.wasm"), Some(ModuleKind::Binary));
        assert_eq!(table.kind("nope"), None);
        assert_eq!(table.load_js("main"), Some("module.exports = 1"));
        assert_eq!(table.load_binary("main.wasm"), Some(&[0u8, 1, 2][..]));
        assert_eq!(table.len(), 2);
    }
}
