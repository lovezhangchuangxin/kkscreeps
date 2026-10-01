//! redb 存储实现，写事务按 [`DurabilityProfile`] 选择持久化方式。
//!
//! 点读只借用值并统计长度，扫描也不复制键值或解码快照。
//! `flush` 用空 Immediate 事务同步此前的提交。
//! 来源：Cargo.lock 锁定版本的 redb `transactions.rs::Durability`。

use std::fs;
use std::path::Path;
use std::time::Instant;

use redb::{Database, Durability, ReadableDatabase, TableDefinition};

use crate::engine::{DurabilityProfile, StorageEngine, TableKind, prefix_upper};

fn table_def(table: TableKind) -> TableDefinition<'static, &'static [u8], &'static [u8]> {
    match table {
        TableKind::Rooms => TableDefinition::new("rooms"),
        TableKind::Users => TableDefinition::new("users"),
        TableKind::Blobs => TableDefinition::new("blobs"),
    }
}

/// 键和值均为字节串的 redb 数据库，数据文件名为 `data.redb`。
pub struct RedbEngine {
    db: Database,
    path: std::path::PathBuf,
    profile: DurabilityProfile,
}

impl RedbEngine {
    /// 在目录内创建或打开数据库；表由首次写事务创建。
    pub fn open(dir: &Path, profile: DurabilityProfile) -> anyhow::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join("data.redb");
        let db = Database::create(&path)?;
        Ok(Self { db, path, profile })
    }

    fn durability(&self) -> Durability {
        match self.profile {
            DurabilityProfile::Durable => Durability::Immediate,
            DurabilityProfile::Relaxed => Durability::None,
        }
    }
}

impl StorageEngine for RedbEngine {
    fn name(&self) -> &'static str {
        "redb"
    }

    fn write_batch(
        &mut self,
        table: TableKind,
        pairs: &[(Vec<u8>, Vec<u8>)],
    ) -> anyhow::Result<()> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(self.durability())?;
        let mut t = txn.open_table(table_def(table))?;
        for (k, v) in pairs {
            let _ = t.insert(k.as_slice(), v.as_slice())?;
        }
        drop(t);
        txn.commit()?;
        Ok(())
    }

    fn write_per_key_txn(
        &mut self,
        table: TableKind,
        pairs: &[(Vec<u8>, Vec<u8>)],
    ) -> anyhow::Result<Vec<f64>> {
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            let start = Instant::now();
            let mut txn = self.db.begin_write()?;
            txn.set_durability(self.durability())?;
            let mut t = txn.open_table(table_def(table))?;
            let _ = t.insert(k.as_slice(), v.as_slice())?;
            drop(t);
            txn.commit()?;
            out.push(start.elapsed().as_secs_f64() * 1e6);
        }
        Ok(out)
    }

    fn read_point(&self, table: TableKind, keys: &[&[u8]]) -> anyhow::Result<(Vec<f64>, u64)> {
        let mut samples = Vec::with_capacity(keys.len());
        let mut bytes = 0u64;
        for key in keys {
            let start = Instant::now();
            let txn = self.db.begin_read()?;
            let t = txn.open_table(table_def(table))?;
            let guard = t
                .get(key)?
                .ok_or_else(|| anyhow::anyhow!("redb 点读缺键 {}", String::from_utf8_lossy(key)))?;
            let len = guard.value().len() as u64;
            drop(guard);
            drop(t);
            drop(txn);
            samples.push(start.elapsed().as_secs_f64() * 1e6);
            bytes += len;
        }
        Ok((samples, bytes))
    }

    fn scan_prefix(&self, table: TableKind, prefix: &[u8]) -> anyhow::Result<(f64, usize, u64)> {
        let start = Instant::now();
        let txn = self.db.begin_read()?;
        let t = txn.open_table(table_def(table))?;
        let upper = prefix_upper(prefix);
        let mut count = 0usize;
        let mut bytes = 0u64;
        let range: (std::ops::Bound<&[u8]>, std::ops::Bound<&[u8]>) = (
            std::ops::Bound::Included(prefix),
            match &upper {
                Some(u) => std::ops::Bound::Excluded(u.as_slice()),
                None => std::ops::Bound::Unbounded,
            },
        );
        for entry in t.range::<&[u8]>(range)? {
            let (_k, v) = entry?;
            bytes += v.value().len() as u64;
            count += 1;
        }
        drop(t);
        drop(txn);
        Ok((start.elapsed().as_secs_f64() * 1e3, count, bytes))
    }

    fn flush(&self) -> anyhow::Result<()> {
        // 空 Immediate 提交：把此前 Durability::None 提交的数据冲刷落盘
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::Immediate)?;
        txn.commit()?;
        Ok(())
    }

    fn file_len(&self) -> anyhow::Result<u64> {
        Ok(fs::metadata(&self.path)?.len())
    }

    fn close(self: Box<Self>) -> anyhow::Result<u64> {
        let path = self.path.clone();
        drop(self);
        Ok(fs::metadata(path)?.len())
    }
}
