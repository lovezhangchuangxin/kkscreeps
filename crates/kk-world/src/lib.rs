//! 保存房间、地形和游戏对象，供运行时查询及引擎结算使用。
//!
//! place 将放置参数转换为对象；world_json 生成沙箱的原始世界数据，
//! snapshot 生成查询接口所需的对象快照。两种 JSON 输出的字段和缺省值不同，
//! 也都不同于 RoomState 编码的二进制房间快照。

pub mod domain;
pub mod index;
pub mod place;
pub mod snapshot;
pub mod types;
pub mod world;
pub mod world_json;

pub use index::RoomIndex;
pub use world::{ObjId, RoomState, RoomVersionHandle, World};
