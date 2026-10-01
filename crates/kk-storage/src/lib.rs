//! 持久化存储模块的预留位置。
//!
//! 当前尚未定义存储接口，也没有数据库后端或定期写盘实现。
//! 房间状态与快照对象的转换位于 kk-world，二进制编码与解码位于 kk-schema；
//! 这些操作不负责写入磁盘。存储引擎的比较基准位于 kk-storage-bench。
//!
//! 检查点与分布式存储的目标设计见 docs/architecture/storage.md，
//! 快照格式及恢复边界见 docs/architecture/snapshot-contract.md。
