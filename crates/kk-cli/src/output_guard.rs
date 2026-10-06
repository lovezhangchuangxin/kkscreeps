//! 作者工具的输出守卫：先只读解析路径，再允许创建目录或写报告。
use anyhow::Context;
use std::path::{Component, Path, PathBuf};

/// 逐组件解析已有祖先的符号链接；缺失后缀保留，`..` 在链接解析后处理。
/// 不创建目录；悬空链接与读取失败直接拒绝，避免猜测其实体位置。
pub fn normalized(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => {
                resolved.push(other.as_os_str());
                match std::fs::symlink_metadata(&resolved) {
                    Ok(_) => {
                        resolved = std::fs::canonicalize(&resolved)
                            .with_context(|| format!("解析输出路径 {}", resolved.display()))?
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(resolved)
}

pub fn distinct(output: &Path, input: &Path) -> anyhow::Result<()> {
    let same = normalized(output)? == normalized(input)?;
    // 直接写入声明会截断硬链接共享的 inode；报告也拒绝同一输入实体。
    #[cfg(unix)]
    let same = {
        use std::os::unix::fs::MetadataExt;
        same || match (std::fs::metadata(output), std::fs::metadata(input)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    };
    anyhow::ensure!(!same, "输出不能覆盖输入文件 {}", input.display());
    Ok(())
}

pub fn outside_packages(output: &Path, root: &Path) -> anyhow::Result<()> {
    let target = normalized(output)?;
    // 原子替换写的是目录项：包内链接即使指向包外，也不能被报告替换。
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let destination_entry =
        normalized(parent)?.join(output.file_name().context("输出路径须指向文件")?);
    let root = normalized(root)?;
    anyhow::ensure!(
        !target.starts_with(&root) && !destination_entry.starts_with(&root),
        "输出须放在 Mod 包根目录之外"
    );
    // 包目录链接会使装载失败；仍保护其实体目录，失败报告不能改写输入。
    if root.is_dir() {
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            if entry.path().is_dir() {
                let package = std::fs::canonicalize(entry.path())?;
                anyhow::ensure!(
                    !target.starts_with(&package) && !destination_entry.starts_with(&package),
                    "输出须放在 Mod 包实体目录之外"
                );
            }
        }
    }
    Ok(())
}

pub fn json_report(report: &Path, root: &Path, input: Option<&Path>) -> anyhow::Result<()> {
    anyhow::ensure!(
        report.extension().is_some_and(|ext| ext == "json"),
        "报告须使用 .json 后缀"
    );
    outside_packages(report, root)?;
    if let Some(input) = input {
        distinct(report, input)?;
    }
    Ok(())
}
