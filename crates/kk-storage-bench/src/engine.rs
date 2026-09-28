//! 候选引擎的统一操作面：两种引擎（redb / libmdbx）在同一 trait 下跑同一份
//! 负载，保证数字可比。操作形态对齐未来 kk-storage 的访问模式
//! （docs/architecture/storage.md：按房间名分键、按数据类型分表、定期检查点
//! 批量写、启动/恢复顺序扫描）。
//!
//! 引擎实现分别在 `redb_engine` / `mdbx_engine` 模块，靠 feature `redb` /
//! `libmdbx` 启用（默认全关——CI 的 workspace 构建不拉这两个依赖）。

use std::fmt;
use std::path::Path;

use serde::Serialize;

/// 持久化档位：两档覆盖 kk-storage 的候选运行形态（每检查点 fsync vs 攒批）。
///
/// 语义映射（崩溃恢复语义的差异本身是选型判据之一，见
/// docs/research/notes/storage-bench.md）：
///
/// | 档位 | redb | libmdbx |
/// |---|---|---|
/// | `Durable` | `Durability::Immediate`（commit 返回即落盘） | `SyncMode::Durable`（commit 即 fsync） |
/// | `Relaxed` | `Durability::None`（不主动写盘，**下一次** Immediate commit 才冲刷） | `SyncMode::SafeNoSync`（异步冲刷、保完整性，可能丢最近提交） |
///
/// 注意 `Relaxed` 两边的语义**不对齐**（redb None 攒得更狠），跨引擎对比时
/// 只作量级参考，不作为淘汰依据。
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

/// 基准用表：按数据类型分表（对照 storage.md「按房间名分键、按数据类型分表」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TableKind {
    /// 房间快照（键 = 房间名，值 = kk_schema 房间快照封套字节）。
    Rooms,
    /// 玩家常驻记录（键 = 玩家名，值 = 小 JSON 记录）。
    Users,
    /// 大 value（玩家 Memory 存档等百 KB～MB 级 blob）。
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

/// 统一操作面。计时都在实现内部（口径注释见各方法），返回原始样本由
/// [`crate::harness`] 汇总。
pub trait StorageEngine {
    fn name(&self) -> &'static str;

    /// 一个写事务批量写全部键值后提交（检查点形态：一次检查点 = 一个大事务）。
    /// 计时含事务开启到 commit 返回的全过程。
    fn write_batch(&mut self, table: TableKind, pairs: &[(Vec<u8>, Vec<u8>)])
    -> anyhow::Result<()>;

    /// 每对独立写事务提交（逐条提交形态）。返回每对"开事务→commit"的耗时（µs）。
    fn write_per_key_txn(
        &mut self,
        table: TableKind,
        pairs: &[(Vec<u8>, Vec<u8>)],
    ) -> anyhow::Result<Vec<f64>>;

    /// 逐键点读，每键独立只读事务（kk-storage 按房间名读单房的形态）。
    /// 返回 (每读耗时 µs, 读到的字节数)。缺键按错误处理（基准数据全部预置）。
    fn read_point(&self, table: TableKind, keys: &[&[u8]]) -> anyhow::Result<(Vec<f64>, u64)>;

    /// 前缀顺序扫描（空前缀 = 全表）。返回 (耗时 ms, 条数, 值字节数)。
    /// 计时含事务开启与迭代全程。
    fn scan_prefix(&self, table: TableKind, prefix: &[u8]) -> anyhow::Result<(f64, usize, u64)>;

    /// 显式冲刷到磁盘（测量点统一补一次落盘：relaxed 档把数据真正写到介质，
    /// durable 档近似 no-op）。文件体积测量前必须调用。
    fn flush(&self) -> anyhow::Result<()>;

    /// 数据文件字节数（不含锁文件/目录开销）。
    fn file_len(&self) -> anyhow::Result<u64>;

    /// 关闭并返回最终数据文件字节数（关闭后文件长度才稳定）。
    fn close(self: Box<Self>) -> anyhow::Result<u64>;
}

/// 前缀的排他上界（把前缀视为小端大数 +1）：`W1` → `W2`；`W1N9` → `W1N:`。
/// 返回 None 表示无上界（空前缀 = 全表，或前缀全 0xFF——两者调用方都走无界分支）。
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

/// 按名开引擎。feature 未启用时给出可行动的错误信息（降级路径：不装该
/// feature 即跳过该基准，而不是让构建红——见 Cargo.toml 注释）。
pub fn open_engine(
    name: &str,
    dir: &Path,
    profile: DurabilityProfile,
) -> anyhow::Result<Box<dyn StorageEngine>> {
    // 两 feature 均未启用时参数不参与任何分支——显式消费避免未用告警
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
        // 空前缀 = 全表：无上界（调用方走 Unbounded/全表迭代分支）
        assert_eq!(prefix_upper(b""), None);
        assert_eq!(prefix_upper(b"W1N9\xff"), Some(b"W1N:".to_vec()));
        assert_eq!(prefix_upper(&[0xFF, 0xFF]), None);
        // 上界排除语义：区间 [前缀, 上界) 恰好覆盖所有带前缀键（无假阳性——
        // 下界同时排除了所有更短键）
        assert!(b"W1N9".as_slice() < prefix_upper(b"W1N9").unwrap().as_slice());
        assert!(b"W1N:\xff\xff".as_slice() >= prefix_upper(b"W1N9").unwrap().as_slice());
    }
}
