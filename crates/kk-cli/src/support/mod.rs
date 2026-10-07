//! CLI 共用的输出保护、原子文件发布与执行身份。
mod identity;
mod output;
pub(crate) mod paths;

pub(crate) use identity::{digest, executable_digest};
pub(crate) use output::{write_bytes, write_report};
