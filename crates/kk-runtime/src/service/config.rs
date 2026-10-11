//! 服务运行配置（严格版本化 JSON v1）。
//!
//! 只承载操作配置（tick 节奏、并发/通道、保存/保留策略、退出等待与
//! Mod 包来源）；世界与执行身份（种子、世界 ID、工作者预算、模块、
//! 内容选择）属于检查点与目录身份，不在此重复声明。字段集合与取值域
//! 变化须提升 `config_format_version` 并提供迁移，未知字段一律拒绝。
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const CONFIG_FORMAT_VERSION: u32 = 1;

/// Mod 包来源：与场景的 mods 声明同形（root 相对配置文件目录解析），
/// 供 run 恢复时装载并经检查点 ModIdentity 核验。
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ModsSource {
    pub root: std::path::PathBuf,
    #[serde(default)]
    pub configs: BTreeMap<String, Value>,
    #[serde(default)]
    pub selection: kk_mod_host::ContentSelection,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Kkpv,
    Json,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ServeConfig {
    pub config_format_version: u32,
    /// 服务状态目录；相对路径相对配置文件目录解析。
    pub state_dir: std::path::PathBuf,
    #[serde(default = "default_tick_interval_ms")]
    pub tick_interval_ms: u64,
    #[serde(default = "default_player_concurrency")]
    pub player_concurrency: usize,
    #[serde(default = "default_channel")]
    pub channel: Channel,
    /// 新世界创建时的 V8 堆上限；run 时作为恢复工作者的硬一致性约束
    /// （与任一工作者不一致即拒绝启动），不运行中修改。
    #[serde(default)]
    pub player_heap_limit_bytes: Option<u64>,
    #[serde(default = "default_auto_save_ticks")]
    pub auto_save_ticks: u32,
    #[serde(default = "default_auto_save_wall_secs")]
    pub auto_save_wall_secs: u64,
    #[serde(default = "default_keep_checkpoints")]
    pub keep_checkpoints: u32,
    #[serde(default = "default_exit_wait_secs")]
    pub exit_wait_secs: u64,
    #[serde(default)]
    pub mods: Option<ModsSource>,
}

fn default_tick_interval_ms() -> u64 {
    1000
}
fn default_player_concurrency() -> usize {
    2
}
fn default_channel() -> Channel {
    Channel::Kkpv
}
fn default_auto_save_ticks() -> u32 {
    60
}
fn default_auto_save_wall_secs() -> u64 {
    60
}
fn default_keep_checkpoints() -> u32 {
    3
}
fn default_exit_wait_secs() -> u64 {
    10
}

impl ServeConfig {
    /// 读取并整体校验配置；`base` 为配置文件所在目录（解析相对路径）。
    pub fn load(path: &Path) -> anyhow::Result<(Self, std::path::PathBuf)> {
        let bytes = std::fs::read(path)
            .map_err(|error| anyhow::anyhow!("读取服务配置 {}: {error}", path.display()))?;
        let config: ServeConfig = serde_json::from_slice(&bytes)
            .map_err(|error| anyhow::anyhow!("解析服务配置 {}: {error}", path.display()))?;
        config.validate()?;
        let state_dir = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join(&config.state_dir),
            _ => config.state_dir.clone(),
        };
        Ok((config, state_dir))
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.config_format_version == CONFIG_FORMAT_VERSION,
            "服务配置版本 {} 不被支持（期望 {}）；需按迁移流程升级",
            self.config_format_version,
            CONFIG_FORMAT_VERSION
        );
        anyhow::ensure!(self.tick_interval_ms >= 1, "tick_interval_ms 须为正整数");
        anyhow::ensure!(
            (1..=32).contains(&self.player_concurrency),
            "player_concurrency 须为 1..=32"
        );
        if let Some(bytes) = self.player_heap_limit_bytes {
            anyhow::ensure!(
                (16 * 1024 * 1024..=4 * 1024 * 1024 * 1024).contains(&bytes),
                "player_heap_limit_bytes 须为 16MiB..=4GiB：{bytes}"
            );
        }
        anyhow::ensure!(self.auto_save_ticks >= 1, "auto_save_ticks 须 ≥ 1");
        anyhow::ensure!(self.auto_save_wall_secs >= 1, "auto_save_wall_secs 须 ≥ 1");
        anyhow::ensure!(self.keep_checkpoints >= 1, "keep_checkpoints 须 ≥ 1");
        anyhow::ensure!(self.exit_wait_secs >= 1, "exit_wait_secs 须 ≥ 1");
        if let Some(mods) = &self.mods {
            anyhow::ensure!(!mods.root.as_os_str().is_empty(), "mods.root 不能为空");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_text(text: &str) -> anyhow::Result<(ServeConfig, std::path::PathBuf)> {
        let dir = std::env::temp_dir().join(format!("kk-svcfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("serve.json");
        std::fs::write(&path, text)?;
        let loaded = ServeConfig::load(&path);
        let result = loaded?;
        std::fs::remove_dir_all(&dir)?;
        Ok(result)
    }

    #[test]
    fn strict_versioned_config_roundtrip() -> anyhow::Result<()> {
        let (config, state) = load_text(
            r#"{"config_format_version": 1, "state_dir": "state", "tick_interval_ms": 250}"#,
        )?;
        assert_eq!(config.tick_interval_ms, 250);
        assert_eq!(config.player_concurrency, 2);
        assert_eq!(config.channel, Channel::Kkpv);
        assert_eq!(config.auto_save_ticks, 60);
        assert!(state.ends_with("state"));
        // 默认值与最小配置：缺 state_dir / 未知字段 / 未来版本 / 越界值拒绝。
        assert!(load_text(r#"{"config_format_version": 1}"#).is_err());
        assert!(
            load_text(r#"{"config_format_version": 1, "state_dir": "s", "surprise": 1}"#).is_err()
        );
        assert!(load_text(r#"{"config_format_version": 2, "state_dir": "s"}"#).is_err());
        assert!(
            load_text(r#"{"config_format_version": 1, "state_dir": "s", "tick_interval_ms": 0}"#)
                .is_err()
        );
        assert!(
            load_text(r#"{"config_format_version": 1, "state_dir": "s", "player_concurrency": 0}"#)
                .is_err()
        );
        assert!(
            load_text(r#"{"config_format_version": 1, "state_dir": "s", "keep_checkpoints": 0}"#)
                .is_err()
        );
        Ok(())
    }
}
