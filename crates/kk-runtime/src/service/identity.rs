//! 服务目录身份（identity.json）：初始化完成标记、世界身份与服务格式版本。
//!
//! 该文件是初始化完成的事实边界——init 的最后一原子步写入；run 见不到
//! 合法身份即拒绝半初始化或坏目录，不静默创建新世界。S2 的服务格式升级
//! 在目录租约下整体切换后更新此文件；本文件不随游戏检查点回滚。
use serde::{Deserialize, Serialize};
use std::path::Path;

/// identity.json 自身的线格式版本（信封版本）。
pub const IDENTITY_FORMAT_VERSION: u32 = 1;
/// 服务域格式版本：v1=S1 常驻与恢复；v2=S2 耐久命令面（inbox/屏障/
/// 恢复下限，字段见 metadata）。v1 目录经 `serve enable-inbox` 显式升级。
pub const SERVICE_FORMAT_V1: u32 = 1;
pub const SERVICE_FORMAT_V2: u32 = 2;
/// 新写入身份使用的当前版本（enable 前的 init 仍写 v1）。
pub const SERVICE_FORMAT_VERSION: u32 = SERVICE_FORMAT_V1;
/// 当前二进制可运行的身份版本集。
pub const SUPPORTED_SERVICE_FORMATS: [u32; 2] = [SERVICE_FORMAT_V1, SERVICE_FORMAT_V2];

const IDENTITY_SPEC: kk_storage::EnvelopeSpec = kk_storage::EnvelopeSpec {
    format: "kkscreeps-service-identity",
    version: IDENTITY_FORMAT_VERSION,
    max_bytes: 1024 * 1024,
};

/// 目录身份载荷；读取方按严格格式整体解码。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceIdentity {
    pub service_format_version: u32,
    /// 世界身份：init 生成，检查点与目录须一致；32 位小写十六进制。
    pub world_id: String,
    /// init 时的世界种子（冗余自首个检查点，供不解码引擎载荷的快查）。
    pub world_seed: u64,
    /// init 种子场景的内容摘要；运行期不重读场景，仅作身份参考。
    pub scenario_sha256: String,
    /// 必须为 true：写入即代表初始化完成。
    pub initialized: bool,
}

/// 以发布协议原子写出身份；init 在初始检查点耐久之后调用。
pub fn write(path: &Path, identity: &ServiceIdentity) -> anyhow::Result<()> {
    validate(identity)?;
    kk_storage::publish_json(
        path,
        IDENTITY_SPEC,
        &serde_json::to_value(identity)?,
        |_| Ok(()),
    )
}

/// 读取并整体校验身份；任何失败都视为目录不可运行（不降级、不重建）。
pub fn read(path: &Path) -> anyhow::Result<ServiceIdentity> {
    let identity: ServiceIdentity = kk_storage::envelope::read_json(path, IDENTITY_SPEC)
        .map_err(|error| anyhow::anyhow!("读取服务身份 {}: {error}", path.display()))
        .and_then(|value| {
            serde_json::from_value(value)
                .map_err(|error| anyhow::anyhow!("服务身份数据非法: {error}"))
        })?;
    validate(&identity)?;
    Ok(identity)
}

fn validate(identity: &ServiceIdentity) -> anyhow::Result<()> {
    anyhow::ensure!(
        !identity.world_id.is_empty()
            && identity.world_id.len() == 32
            && identity
                .world_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "world_id 必须是 32 位小写十六进制"
    );
    anyhow::ensure!(
        identity.initialized,
        "身份标记未完成初始化；目录处于半初始化状态，拒绝使用"
    );
    anyhow::ensure!(
        SUPPORTED_SERVICE_FORMATS.contains(&identity.service_format_version),
        "服务格式版本 {} 不被当前二进制支持（支持 {:?}）；需按迁移流程升级",
        identity.service_format_version,
        SUPPORTED_SERVICE_FORMATS
    );
    anyhow::ensure!(
        identity.scenario_sha256.len() == 64
            && identity
                .scenario_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "scenario_sha256 必须是 64 位小写十六进制摘要"
    );
    Ok(())
}

/// 生成 128 位世界身份：进程熵（RandomState 种子）加时间戳的两轮哈希
/// 拼接。两轮哈希种子独立刷新，但不假设密码学强度——该值只求全局唯一
/// （碰撞概率在现实初始化次数下可忽略），不承担防猜测职责。
pub fn generate_world_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut id = String::with_capacity(32);
    for _ in 0..2 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(std::process::id() as u64);
        hasher.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
        );
        id.push_str(&format!("{:016x}", hasher.finish()));
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ServiceIdentity {
        ServiceIdentity {
            service_format_version: SERVICE_FORMAT_VERSION,
            world_id: generate_world_id(),
            world_seed: 42,
            scenario_sha256: "a".repeat(64),
            initialized: true,
        }
    }

    #[test]
    fn roundtrip_and_strict_validation() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("kk-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("identity.json");
        let identity = sample();
        write(&path, &identity)?;
        assert_eq!(read(&path)?.world_seed, identity.world_seed);
        // 未完成初始化、畸形 world_id 与未来服务格式都在读取处拒绝。
        for broken in [
            ServiceIdentity {
                initialized: false,
                ..sample()
            },
            ServiceIdentity {
                world_id: "XYZ".into(),
                ..sample()
            },
            ServiceIdentity {
                service_format_version: 3,
                ..sample()
            },
        ] {
            let value = serde_json::to_value(&broken)?;
            kk_storage::publish_json(&path, IDENTITY_SPEC, &value, |_| Ok(()))?;
            assert!(read(&path).is_err(), "应拒绝 {:?}", broken.world_id);
        }
        // 截断的身份文件同样拒绝（不做“重建身份”式降级）。
        let bytes = std::fs::read(&path)?;
        std::fs::write(&path, &bytes[..bytes.len() / 2])?;
        assert!(read(&path).is_err());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn generated_world_ids_are_distinct_and_well_formed() {
        let a = generate_world_id();
        let b = generate_world_id();
        assert_ne!(a, b);
        for id in [a, b] {
            assert_eq!(id.len(), 32);
            assert!(
                id.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            );
        }
    }
}
