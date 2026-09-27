//! 世界模型：typed 房间状态、place 归一化、世界 JSON 投影、格子索引。
//!
//! M1 起取代 M0 的哑世界（JSON Value 存储）成为 embed 与引擎的共同后端；
//! prelude"世界 JSON"契约形状不变（world_json 模块）。

pub mod dumb;
pub mod index;
pub mod place;
pub mod types;
pub mod world;
pub mod world_json;

pub use index::RoomIndex;
pub use world::{ObjId, RoomState, World};

// M0 哑世界：embed 切换 typed world 后退役（M1 步 5）
pub use dumb::DumbWorld;
