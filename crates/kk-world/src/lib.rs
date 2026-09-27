//! 世界模型：typed 房间状态、place 归一化、世界 JSON 投影、格子索引。
//!
//! M1 起（步 5 完成）embed 与引擎共用本 crate 的 typed world；
//! prelude"世界 JSON"契约形状不变（world_json 模块）。

pub mod index;
pub mod place;
pub mod types;
pub mod world;
pub mod world_json;

pub use index::RoomIndex;
pub use world::{ObjId, RoomState, World};
