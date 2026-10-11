//! 目录单写者租约：固定 inode 锁文件上的 OS 独占锁。
//!
//! 锁随打开的文件描述持有，进程退出或崩溃即由操作系统释放；持有期间
//! 不得删除或替换锁文件——另一个进程可能锁到新 inode 而绕过互斥。第二
//! 个进程立即拒绝，不凭 PID 推测“旧锁”。文件锁的平台差异见
//! std::fs::File 文档；本仓库只验证 macOS/Linux 与本地文件系统，网络
//! 文件系统上的锁与耐久保证不默认成立。
//!
//! 已知平台怪癖（2026-10-12 探针实证，可独立复现：两线程各持循环——
//! 线程 A 反复 flock→争锁→drop→重取同一文件，线程 B 持续 spawn
//! /usr/bin/true——约 4% 循环命中）：macOS XNU 上，同进程内 flock 的
//! 快速“加锁→争锁→释放→重取”循环与并发 fork/posix_spawn 交织时，会在
//! vnode 上留下无任何 fd 持有者的残锁（约 250ms 后自愈）。生产形态是
//! 每进程生命周期内只获取一次租约、跨进程争用，不构成该循环；跨进程争
//! 锁与崩溃释放由本模块测试覆盖，测试间须串行避免触达该竞态。
use std::fs::{File, OpenOptions};
use std::path::Path;

/// 持有期间独占目录写权；`Drop` 关闭文件即释放，进程终止同样释放。
pub struct WriterLease {
    // 字段只负责维持锁的生存期；显式命名避免误优化告警。
    _file: File,
}

impl WriterLease {
    /// 在锁文件上获取独占锁；文件不存在时创建（目录须已存在）。
    pub fn acquire(lock_path: &Path) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|error| anyhow::anyhow!("打开锁文件 {}: {error}", lock_path.display()))?;
        if let Err(error) = file.try_lock() {
            anyhow::bail!(
                "目录 {} 已被其他写者持有（{error}）；同一服务目录同时只允许一个进程写入",
                lock_path.parent().unwrap_or(Path::new(".")).display()
            );
        }
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 串行化本模块测试：并发 spawn 会触达上述 XNU flock 残锁竞态，使
    /// “释放后重取”步骤间歇性 WouldBlock（与租约语义无关的内核伪影）。
    static SERIALIZE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_dir(tag: &str) -> anyhow::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("kk-lease-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn second_acquire_in_process_is_rejected() -> anyhow::Result<()> {
        let _guard = SERIALIZE.lock().expect("lease 测试串行锁");
        let dir = temp_dir("double")?;
        let lock = dir.join("writer.lock");
        let _first = WriterLease::acquire(&lock)?;
        assert!(
            WriterLease::acquire(&lock).is_err(),
            "同进程第二次获取必须被拒（flock 按打开文件描述互斥）"
        );
        drop(_first);
        // 释放后同进程可重新获取。
        let _second = WriterLease::acquire(&lock)?;
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// 子进程占锁并等待指令：先验证父进程争锁失败，再让子进程硬退出
    /// （不运行 Drop），验证锁随进程退出释放。
    #[test]
    #[ignore = "由 lease_contention_parent 起子进程调用"]
    fn lease_contention_child() -> anyhow::Result<()> {
        let lock = std::path::PathBuf::from(std::env::var("KK_LEASE_LOCK_PATH")?);
        let marker = std::path::PathBuf::from(std::env::var("KK_LEASE_MARKER_PATH")?);
        let command = std::path::PathBuf::from(std::env::var("KK_LEASE_COMMAND_PATH")?);
        let _lease = WriterLease::acquire(&lock)?;
        std::fs::write(&marker, b"held")?;
        loop {
            // 等父进程写指令；硬退出模拟崩溃（无 Drop、无清理）。
            if let Ok(text) = std::fs::read_to_string(&command)
                && text.trim() == "crash"
            {
                std::process::exit(9);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn lease_contention_parent() -> anyhow::Result<()> {
        let _guard = SERIALIZE.lock().expect("lease 测试串行锁");
        let dir = temp_dir("contention")?;
        let lock = dir.join("writer.lock");
        let marker = dir.join("held.marker");
        let command = dir.join("command");
        let mut output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--ignored",
                "--exact",
                "lease::tests::lease_contention_child",
            ])
            .env("KK_LEASE_LOCK_PATH", &lock)
            .env("KK_LEASE_MARKER_PATH", &marker)
            .env("KK_LEASE_COMMAND_PATH", &command)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        // 等待窗放宽到 30s：整机测试并行时子进程装载可能偏慢。
        for _ in 0..1500 {
            if marker.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        anyhow::ensure!(
            marker.exists(),
            "子进程未在 30s 内取得锁并留下标记；stderr: {:?}",
            output.stderr.take().map(|mut pipe| {
                use std::io::Read;
                let mut text = String::new();
                let _ = pipe.read_to_string(&mut text);
                text
            })
        );
        let contested = WriterLease::acquire(&lock);
        anyhow::ensure!(
            contested.is_err(),
            "子进程持锁期间父进程必须立即被拒，而不是等待"
        );
        // 指令子进程硬退出：锁必须随进程消失释放（不依赖优雅 Drop）。
        std::fs::write(&command, b"crash")?;
        let status = output.wait_with_output()?.status;
        anyhow::ensure!(status.code() == Some(9), "子进程应按指令以 9 退出");
        let _reacquired = WriterLease::acquire(&lock)?;
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
