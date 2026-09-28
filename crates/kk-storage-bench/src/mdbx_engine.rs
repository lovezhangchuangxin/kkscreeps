//! libmdbx 0.9 绑定实现（feature `libmdbx`；C++ 库经 mdbx-sys cc/bindgen 构建，
//! 仓储层为 OpenLDAP 许可）。构建失败时的降级路径：不开该 feature 即可
//! （workspace 其余构建完全不受影响）。
//!
//! 口径要点：
//! - 目录形态打开（`<dir>/data.mdb` + `lock.mdb`），几何上限 4 GiB；
//! - 三张命名表在 open 时一次建好（mdbx 命名表须先存在，只读事务才能 open）；
//! - 点读每键独立 `begin_ro_txn`（与 redb 实现同口径）；
//! - `NoWriteMap`（缓冲文件 IO 而非 mmap 直写——与 redb 的写路径更可比；
//!   WriteMap 变体留作后续变奏）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use libmdbx::{
    Database, DatabaseOptions, Mode, NoWriteMap, ReadWriteOptions, SyncMode, TableFlags, WriteFlags,
};

use crate::engine::{DurabilityProfile, StorageEngine, TableKind};

pub struct MdbxEngine {
    db: Database<NoWriteMap>,
    data_path: PathBuf,
}

impl MdbxEngine {
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
        // 命名表一次建好
        let txn = db.begin_rw_txn()?;
        for name in [TableKind::Rooms, TableKind::Users, TableKind::Blobs] {
            txn.create_table(Some(name.name()), TableFlags::empty())?;
        }
        txn.commit()?;
        // libmdbx 0.14 目录形态的默认文件名：数据 mdbx.dat、锁 mdbx.lck
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
        self.db.sync(false)?;
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
