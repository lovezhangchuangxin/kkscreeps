//! 基准使用的存储接口：批量写、逐键提交、点读、前缀扫描和文件大小。
//!
//! 两个引擎使用相同数据集和事务粒度，但读取时的复制行为不同，
//! `Relaxed` 档的持久化保证也不同；比较结果时需结合实现说明。

use std::fmt;
use std::path::Path;

use serde::Serialize;

/// 写事务的持久化档位。
///
/// | 档位 | redb | libmdbx |
/// |---|---|---|
/// | `Durable` | `Durability::Immediate`：提交返回前完成持久化 | `SyncMode::Durable`：提交时同步数据及元数据 |
/// | `Relaxed` | `Durability::None`：由后续 Immediate 提交持久化 | `SyncMode::SafeNoSync`：提交省略同步，保留最近已同步版本供恢复 |
///
/// `Relaxed` 两种实现均可能丢失未同步的提交，但写入方式和恢复机制不同，
/// 不能把它们当作相同持久化保证下的性能比较。本基准未做崩溃恢复测试。
/// 来源：Cargo.lock 锁定版本的 redb `transactions.rs::Durability`、
/// libmdbx `flags.rs::SyncMode` 和 mdbx-sys `mdbx.h::MDBX_SAFE_NOSYNC`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DurabilityProfile {
    Durable,
    Relaxed,
}

impl DurabilityProfile {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "durable" => Some(Self::Durable),
            "relaxed" => Some(Self::Relaxed),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Durable => "durable",
            Self::Relaxed => "relaxed",
        }
    }
}

impl fmt::Display for DurabilityProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// 按数据类型划分的基准表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TableKind {
    /// 房间快照（键 = 房间名，值 = kk_schema 房间快照封套字节）。
    Rooms,
    /// 玩家常驻记录（键 = `u:<玩家名>`，值 = 小 JSON 记录）。
    Users,
    /// 模拟 Memory 大小的字节块，内容不是有效的 Memory JSON。
    Blobs,
}

impl TableKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Rooms => "rooms",
            Self::Users => "users",
            Self::Blobs => "blobs",
        }
    }
}

/// 存储操作与计时样本。批量写由运行器计时，其余操作由引擎实现计时。
/// 各场景只检查缺键、字节总数或扫描条数，不校验读回内容是否逐字节相同。
pub trait StorageEngine {
    fn name(&self) -> &'static str;

    /// 用一个写事务插入或覆盖全部键值；未提供的已有键不会删除。
    /// 运行器测量整个方法调用，数据集构造在计时前完成。
    fn write_batch(&mut self, table: TableKind, pairs: &[(Vec<u8>, Vec<u8>)])
    -> anyhow::Result<()>;

    /// 每对键值用独立写事务提交，返回每次从开启事务到提交返回的耗时（微秒）。
    fn write_per_key_txn(
        &mut self,
        table: TableKind,
        pairs: &[(Vec<u8>, Vec<u8>)],
    ) -> anyhow::Result<Vec<f64>>;

    /// 每键开启独立只读事务，返回每次读取耗时（微秒）及值的字节总数。
    /// 缺键返回错误。redb 借用值并取长度，libmdbx 复制值后取长度；
    /// 耗时均包含事务结束，但没有解码房间快照。
    fn read_point(&self, table: TableKind, keys: &[&[u8]]) -> anyhow::Result<(Vec<f64>, u64)>;

    /// 扫描带前缀的键，空前缀扫描全表；返回耗时（毫秒）、条数和值的字节总数。
    /// 计时包含开启和结束事务，以及遍历和字节数统计。
    /// redb 借用键值，libmdbx 逐项复制键和值；均不解码值。
    fn scan_prefix(&self, table: TableKind, prefix: &[u8]) -> anyhow::Result<(f64, usize, u64)>;

    /// 强制同步此前的提交，运行器在测量文件大小前调用，耗时不计入场景样本。
    /// redb 使用空 Immediate 写事务；libmdbx 使用强制同步。
    fn flush(&self) -> anyhow::Result<()>;

    /// 数据文件字节数（不含锁文件/目录开销）。
    fn file_len(&self) -> anyhow::Result<u64>;

    /// 关闭引擎后读取数据文件大小。
    fn close(self: Box<Self>) -> anyhow::Result<u64>;
}

/// 按字节字典序求前缀的排他上界，用于范围 `[prefix, upper)`。
/// 从右向左找到非 0xFF 字节，加一并截去其后字节，例如 `W1` → `W2`。
/// 空前缀或全 0xFF 前缀返回 `None`，调用方仍保留下界，只省略上界。
pub fn prefix_upper(prefix: &[u8]) -> Option<Vec<u8>> {
    for i in (0..prefix.len()).rev() {
        if prefix[i] != 0xFF {
            let mut upper = prefix[..=i].to_vec();
            upper[i] += 1;
            return Some(upper);
        }
    }
    None
}

/// 打开已启用的引擎；未知名称或未启用对应 feature 时返回错误。
pub fn open_engine(
    name: &str,
    dir: &Path,
    profile: DurabilityProfile,
) -> anyhow::Result<Box<dyn StorageEngine>> {
    // 默认不启用引擎，此时这些参数不会进入下面的构造分支。
    let _ = (name, dir, profile);
    match name {
        #[cfg(feature = "redb")]
        "redb" => Ok(Box::new(crate::redb_engine::RedbEngine::open(
            dir, profile,
        )?)),
        #[cfg(feature = "libmdbx")]
        "libmdbx" => Ok(Box::new(crate::mdbx_engine::MdbxEngine::open(
            dir, profile,
        )?)),
        other => Err(anyhow::anyhow!(
            "引擎 {other:?} 不可用（编译期未启用对应 feature）。\n\
             本机构建：cargo build -p kk-storage-bench --release --features redb,libmdbx\n\
             可用引擎：{}",
            available_engines().join(", ")
        )),
    }
}

/// 当前二进制里可用的引擎列表。
pub fn available_engines() -> Vec<&'static str> {
    let mut out = Vec::new();
    if cfg!(feature = "redb") {
        out.push("redb");
    }
    if cfg!(feature = "libmdbx") {
        out.push("libmdbx");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_upper_basics() {
        // 末字节 +1 进位：'1'+1='2'、'9'+1=':'；尾随 0xFF 被吸收进前字节进位
        assert_eq!(prefix_upper(b"W1"), Some(b"W2".to_vec()));
        assert_eq!(prefix_upper(b"W1N9"), Some(b"W1N:".to_vec()));
        assert_eq!(prefix_upper(b""), None);
        assert_eq!(prefix_upper(b"W1N9\xff"), Some(b"W1N:".to_vec()));
        assert_eq!(prefix_upper(&[0xFF, 0xFF]), None);
        // 这两个比较检查前缀自身位于区间内，而上界及其后续键被排除。
        assert!(b"W1N9".as_slice() < prefix_upper(b"W1N9").unwrap().as_slice());
        assert!(b"W1N:\xff\xff".as_slice() >= prefix_upper(b"W1N9").unwrap().as_slice());
    }
}
