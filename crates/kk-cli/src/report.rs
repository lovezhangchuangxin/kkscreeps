//! CLI JSON 报告以同目录临时文件写入并替换；不是世界检查点协议。
use anyhow::Context;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

pub(crate) fn write_report(path: &Path, report: &Value) -> anyhow::Result<()> {
    write_file(path, |file| {
        serde_json::to_writer_pretty(&mut *file, report)?;
        file.write_all(b"\n")?;
        Ok(())
    })
}

/// 声明同样使用替换写入：失败不留下截断文件，也不改写硬链接的输入实体。
pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    write_file(path, |file| {
        file.write_all(bytes)?;
        Ok(())
    })
}

fn write_file(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .context("报告路径必须指向文件")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
