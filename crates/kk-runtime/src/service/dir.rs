//! 服务状态目录布局：固定内部路径是租约、身份与恢复选择的共同依赖。
//!
//! 目录内部结构（文件名、子目录）不是调用方可配置项；升级须走服务格式
//! 版本迁移。服务目录与玩家/Mod 输入目录分离，运行期不再依赖种子场景
//! 目录（init 时完成内容装载并写入检查点）。
use anyhow::Context;
use std::path::{Path, PathBuf};

/// 单写者锁文件名；持锁期间不得删除或替换（固定 inode）。
pub const WRITER_LOCK: &str = "writer.lock";
/// 初始化完成标记与服务身份；init 的最后一原子步写入。
pub const IDENTITY_FILE: &str = "identity.json";
/// 服务检查点子目录；文件名携带发布序号。
pub const CHECKPOINTS_DIR: &str = "checkpoints";

/// 已解析的服务状态目录：只负责路径解析与目录级核验，不持有租约。
#[derive(Clone)]
pub struct ServiceDir {
    root: PathBuf,
}

impl ServiceDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn writer_lock(&self) -> PathBuf {
        self.root.join(WRITER_LOCK)
    }

    pub fn identity_file(&self) -> PathBuf {
        self.root.join(IDENTITY_FILE)
    }

    pub fn checkpoints_dir(&self) -> PathBuf {
        self.root.join(CHECKPOINTS_DIR)
    }

    /// init 用：创建目录骨架并同步相关父目录（新建目录的目录项也须耐久）。
    /// run 路径不得以此替代对已初始化目录的核验。
    pub fn ensure_layout(&self) -> anyhow::Result<()> {
        let created_root = !self.root.exists();
        if created_root {
            std::fs::create_dir(&self.root)
                .with_context(|| format!("创建服务目录 {}", self.root.display()))?;
            if let Some(parent) = self.root.parent().filter(|p| !p.as_os_str().is_empty()) {
                kk_storage::publish::sync_directory(parent)
                    .context("同步服务目录的父目录（新建目录耐久性）")?;
            }
        } else {
            anyhow::ensure!(
                self.root.is_dir(),
                "服务目录路径 {} 不是目录",
                self.root.display()
            );
        }
        let created_checkpoints = !self.checkpoints_dir().exists();
        if created_checkpoints {
            std::fs::create_dir(self.checkpoints_dir())
                .with_context(|| format!("创建检查点目录 {}", self.checkpoints_dir().display()))?;
        }
        if created_root || created_checkpoints {
            kk_storage::publish::sync_directory(&self.root)
                .context("同步服务目录（检查点子目录耐久性）")?;
        }
        Ok(())
    }

    /// init 前置核验：目录须不存在、为空，或只含 init 自建的骨架项
    /// （writer.lock 与空的 checkpoints/）。已初始化（identity.json 在）
    /// 或含外来内容时拒绝——不能在不明目录上静默创建新世界。
    pub fn validate_for_init(&self) -> anyhow::Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        anyhow::ensure!(self.root.is_dir(), "服务目录路径不是目录");
        if self.identity_file().exists() {
            anyhow::bail!(
                "服务目录已初始化（{} 存在）；重复 init 须先显式迁移或换目录",
                self.identity_file().display()
            );
        }
        for entry in std::fs::read_dir(&self.root)
            .with_context(|| format!("清点服务目录 {}", self.root.display()))?
        {
            let entry = entry?;
            let name = entry.file_name().into_string().unwrap_or_default();
            if name == WRITER_LOCK {
                continue;
            }
            // 发布协议的崩溃残留（.identity.json.<pid>.<seq>.tmp 形态）可
            // 在持租约后清理，不算来源不明内容；init 流程负责清理。
            if kk_storage::publish::is_temporary_name(&name) {
                continue;
            }
            if name == CHECKPOINTS_DIR {
                let path = entry.path();
                anyhow::ensure!(path.is_dir(), "checkpoints 不是目录");
                // 与根目录同口径：仅允许协议临时残留（可清理），任何
                // 非临时内容都算来源不明。清理后是否真正清空由 init
                // 在租约内复核，此处只做无权写入的门。
                for checkpoint_entry in std::fs::read_dir(&path)
                    .with_context(|| format!("清点检查点目录 {}", path.display()))?
                {
                    let checkpoint_name = checkpoint_entry?
                        .file_name()
                        .into_string()
                        .unwrap_or_default();
                    if kk_storage::publish::is_temporary_name(&checkpoint_name) {
                        continue;
                    }
                    anyhow::bail!(
                        "checkpoints/ 含非临时内容 {checkpoint_name} 且无初始化标记：init 中途崩溃或来源不明（处置=确认无价值后删除整个目录重新 init；不会自动复用或覆盖）"
                    );
                }
                continue;
            }
            anyhow::bail!(
                "服务目录含未识别项 {name}；init 只接受空目录（或仅骨架）以避免覆盖未知数据"
            );
        }
        Ok(())
    }

    /// run 前置核验：目录与检查点子目录必须已在（缺任一即拒绝，不做任何
    /// 创建）；identity 的存在性与内容由调用方经 identity 读取判定。
    pub fn validate_for_run(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.root.is_dir(),
            "服务目录 {} 不存在或不是目录；运行须先 init，不会静默创建新世界",
            self.root.display()
        );
        anyhow::ensure!(
            self.checkpoints_dir().is_dir(),
            "服务目录缺少 {}；目录不完整，拒绝运行",
            self.checkpoints_dir().display()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> anyhow::Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("kk-service-dir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(dir)
    }

    #[test]
    fn init_validation_rejects_unknown_or_initialized() -> anyhow::Result<()> {
        let root = temp_dir("validate")?;
        // 不存在 → 允许。
        ServiceDir::new(&root).validate_for_init()?;
        // 空目录 / 仅骨架 → 允许。
        std::fs::create_dir_all(root.join(CHECKPOINTS_DIR))?;
        ServiceDir::new(&root).validate_for_init()?;
        // 外来内容 → 拒绝。
        std::fs::write(root.join("foreign.json"), b"x")?;
        assert!(ServiceDir::new(&root).validate_for_init().is_err());
        std::fs::remove_file(root.join("foreign.json"))?;
        // 发布协议崩溃残留的临时文件（如 identity 发布中断）→ 可清理
        // 残留，不阻塞重新 init。
        std::fs::write(root.join(".identity.json.123.4.tmp"), b"torn")?;
        ServiceDir::new(&root).validate_for_init()?;
        // checkpoints 仅含协议临时残留 → 允许（init 持租约清理后复核）。
        std::fs::write(
            root.join(CHECKPOINTS_DIR).join(".cp-1.json.123.4.tmp"),
            b"torn",
        )?;
        ServiceDir::new(&root).validate_for_init()?;
        // 半初始化（checkpoints 含非临时内容、无 identity）→ 拒绝。
        std::fs::write(root.join(CHECKPOINTS_DIR).join("cp-1.json"), b"x")?;
        assert!(ServiceDir::new(&root).validate_for_init().is_err());
        std::fs::remove_file(root.join(CHECKPOINTS_DIR).join("cp-1.json"))?;
        std::fs::remove_file(root.join(CHECKPOINTS_DIR).join(".cp-1.json.123.4.tmp"))?;
        // 已初始化 → 拒绝。
        std::fs::write(root.join(IDENTITY_FILE), b"x")?;
        assert!(ServiceDir::new(&root).validate_for_init().is_err());
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn run_validation_requires_existing_layout() -> anyhow::Result<()> {
        let root = temp_dir("run")?;
        assert!(ServiceDir::new(&root).validate_for_run().is_err());
        std::fs::create_dir_all(&root)?;
        assert!(ServiceDir::new(&root).validate_for_run().is_err());
        std::fs::create_dir(root.join(CHECKPOINTS_DIR))?;
        ServiceDir::new(&root).validate_for_run()?;
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn ensure_layout_creates_directory_tree() -> anyhow::Result<()> {
        let root = temp_dir("layout")?;
        let service = ServiceDir::new(&root);
        service.validate_for_init()?;
        service.ensure_layout()?;
        assert!(service.checkpoints_dir().is_dir());
        // 幂等：已存在的骨架再次 ensure 不报错、不同步额外父目录。
        service.ensure_layout()?;
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }
}
