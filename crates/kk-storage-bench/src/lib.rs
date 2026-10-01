//! 用固定种子的合成数据比较 redb 与 libmdbx 的存储操作。
//!
//! 房间数据使用 kk-world 构造，并由 kk-schema 编码；玩家记录和大字节块
//! 用于模拟不同大小的值。这些数据不代表真实玩家负载，结果不能直接决定存储选型。
//! 两个引擎分别由 `redb`、`libmdbx` feature 启用，默认均关闭。
//!
//! ```text
//! cargo run -p kk-storage-bench --release --features redb,libmdbx -- --json report.json
//! ```

pub mod engine;
pub mod fixtures;
pub mod harness;

#[cfg(feature = "redb")]
pub mod redb_engine;

#[cfg(feature = "libmdbx")]
pub mod mdbx_engine;
