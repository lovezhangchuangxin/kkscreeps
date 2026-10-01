//! libmdbx 存储实现，使用 `NoWriteMap`，写入通过文件 I/O 完成。
//!
//! 目录内的数据文件为 `mdbx.dat`，容量上限为 4 GiB，打开时创建三张命名表。
//! 点读复制值，扫描复制键和值；这些分配与复制包含在计时内。
//! 来源：Cargo.lock 锁定版本的 libmdbx `database.rs::NoWriteMap`、
//! `codec.rs::Decodable<Vec<u8>>` 和 mdbx-sys `mdbx.h::MDBX_DATANAME`。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use libmdbx::{
    Database, DatabaseOptions, Mode, NoWriteMap, ReadWriteOptions, SyncMode, TableFlags, WriteFlags,
};

use crate::engine::{DurabilityProfile, StorageEngine, TableKind};

/// 持有 libmdbx 数据库及其数据文件路径。
pub struct MdbxEngine {
    db: Database<NoWriteMap>,
    data_path: PathBuf,
}

impl MdbxEngine {
    /// 创建或打开目录内的数据库，并在一个写事务中创建缺失的基准表。
    pub fn open(dir: &Path, profile: DurabilityProfile) -> anyhow::Result<Self> {
        fs::create_dir_all(dir)?;
        let options = DatabaseOptions {
            max_tables: Some(4),
            mode: Mode::ReadWrite(ReadWriteOptions {
                sync_mode: match profile {
                    DurabilityProfile::Durable => SyncMode::Durable,
                    DurabilityProfile::Relaxed => SyncMode::SafeNoSync,
                },
                max_size: Some(4 << 30),
                ..ReadWriteOptions::default()
            }),
            ..DatabaseOptions::default()
        };
        let db = Database::<NoWriteMap>::open_with_options(dir, options)?;
        // 只读事务不能创建表，因此在运行读场景之前建好。
        let txn = db.begin_rw_txn()?;
        for name in [TableKind::Rooms, TableKind::Users, TableKind::Blobs] {
            txn.create_table(Some(name.name()), TableFlags::empty())?;
        }
        txn.commit()?;
        Ok(Self {
            db,
            data_path: dir.join("mdbx.dat"),
        })
    }
}

impl StorageEngine for MdbxEngine {
    fn name(&self) -> &'static str {
        "libmdbx"
    }

    fn write_batch(
        &mut self,
        table: TableKind,
        pairs: &[(Vec<u8>, Vec<u8>)],
    ) -> anyhow::Result<()> {
        let txn = self.db.begin_rw_txn()?;
        let t = txn.open_table(Some(table.name()))?;
        for (k, v) in pairs {
            txn.put(&t, k.as_slice(), v.as_slice(), WriteFlags::UPSERT)?;
        }
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
            let txn = self.db.begin_rw_txn()?;
            let t = txn.open_table(Some(table.name()))?;
            txn.put(&t, k.as_slice(), v.as_slice(), WriteFlags::UPSERT)?;
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
            let txn = self.db.begin_ro_txn()?;
            let t = txn.open_table(Some(table.name()))?;
            let value: Option<Vec<u8>> = txn.get(&t, key)?;
            let len = value
                .ok_or_else(|| anyhow::anyhow!("mdbx 点读缺键 {}", String::from_utf8_lossy(key)))?
                .len() as u64;
            txn.commit()?;
            samples.push(start.elapsed().as_secs_f64() * 1e6);
            bytes += len;
        }
        Ok((samples, bytes))
    }

    fn scan_prefix(&self, table: TableKind, prefix: &[u8]) -> anyhow::Result<(f64, usize, u64)> {
        let start = Instant::now();
        let txn = self.db.begin_ro_txn()?;
        let t = txn.open_table(Some(table.name()))?;
        let mut count = 0usize;
        let mut bytes = 0u64;
        {
            // Cursor 持有 txn 借用且实现 Drop——作用域先于 commit 结束
            let mut cursor = txn.cursor(&t)?;
            let iter = if prefix.is_empty() {
                cursor.iter::<Vec<u8>, Vec<u8>>()
            } else {
                cursor.iter_from::<Vec<u8>, Vec<u8>>(prefix)
            };
            for item in iter {
                let (k, v): (Vec<u8>, Vec<u8>) = item?;
                if !k.starts_with(prefix) {
                    break;
                }
                bytes += v.len() as u64;
                count += 1;
            }
        }
        txn.commit()?;
        Ok((start.elapsed().as_secs_f64() * 1e3, count, bytes))
    }

    fn flush(&self) -> anyhow::Result<()> {
        // false 只检查同步阈值，未配置阈值时可能不写盘；flush 要求强制同步。
        // 来源：mdbx-sys mdbx.h::mdbx_env_sync_ex 的 force 参数。
        self.db.sync(true)?;
        Ok(())
    }

    fn file_len(&self) -> anyhow::Result<u64> {
        Ok(fs::metadata(&self.data_path)?.len())
    }

    fn close(self: Box<Self>) -> anyhow::Result<u64> {
        let path = self.data_path.clone();
        drop(self);
        Ok(fs::metadata(path)?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn flush_syncs_relaxed_commits() {
        let dir = std::env::temp_dir().join(format!(
            "kk-storage-bench-flush-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut engine = MdbxEngine::open(&dir, DurabilityProfile::Relaxed).unwrap();
        engine.db.sync(true).unwrap();
        engine
            .write_batch(TableKind::Rooms, &[(b"W0N0".to_vec(), vec![42; 8192])])
            .unwrap();
        engine.flush().unwrap();
        // 强制同步若仍执行了写盘，说明 flush 留下了未同步的提交。
        let already_synced = engine.db.sync(true).unwrap();
        drop(engine);
        fs::remove_dir_all(dir).unwrap();
        assert!(already_synced, "flush 后仍有待同步的数据");
    }
}
