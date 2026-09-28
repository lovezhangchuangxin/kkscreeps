//! 存储选型基准（M2-C3）：redb vs libmdbx 微基准骨架 + 场景夹具。
//!
//! **非正式选型**——正式选型待 2A 落地后用真实负载跑（ADR-008 第 7 条、
//! M2 方案 D4：存储基准被 2A 阻塞，本骨架只准备方法学与负载形态）。
//!
//! 结构：
//! - [`fixtures`]：三档规模的世界快照数据（复用 kk-world place/encode_snapshot，
//!   字节形态与未来 kk-storage 落盘的房间值同源）；
//! - [`harness`]：计时/分位统计与机器可读 JSON 报告模型；
//! - [`engine`]：候选引擎的统一操作面（批量写/逐条写/点读/前缀扫描/文件体积），
//!   redb 与 libmdbx 实现分别在 feature `redb` / `libmdbx` 之后（默认关闭，
//!   CI 的 workspace 构建不拉这两个依赖——依赖隔离理由见
//!   docs/research/notes/storage-bench.md）。
//!
//! 本机跑法（bin `kk-storage-bench`）：
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
