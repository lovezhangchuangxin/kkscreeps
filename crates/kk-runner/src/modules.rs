//! 保存玩家 CommonJS 源码与二进制模块，按模块名精确查询。
//!
//! 目录加载器将 .js 后缀去掉，保留 .wasm 后缀，以 / 连接子目录名。
//! JavaScript require 的缓存与名称处理见 prelude/20_cjs.js；二进制加载返回 Buffer。

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

/// 一个玩家的模块表，由 Rust 持有；JavaScript 通过 op 按名称读取。
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

    /// 递归读取目录中的 .js 和 .wasm 文件，跳过以 . 开头的目录与文件。
    /// .js 模块名去掉后缀，.wasm 保留后缀，子目录以 / 连接；其他扩展名忽略。
    /// 读取失败返回错误；目录遍历使用 is_dir，会跟随目录符号链接。
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
