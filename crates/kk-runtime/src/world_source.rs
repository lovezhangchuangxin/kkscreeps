//! 提供世界 JSON 和查询快照的只读接口，目前由 kk-world::World 实现。
//!
//! 玩家执行期间通过不可变借用读取同一已提交世界，结算在执行循环结束后进行。
//! 世界 JSON 按 viewer 过滤；对象及 FIND 查询用于宿主管理接口，不执行玩家视野检查。

use serde_json::Value;

use kk_world::RoomVersionHandle;

/// 世界读取接口，注入 JSON 与查询快照各有自己的数据格式。
pub trait WorldSource {
    /// 生成按玩家视野过滤的世界 JSON；gcl_points 是玩家累计 GCL 点数。
    fn world_json_for_viewer(&self, viewer: &str, gcl_points: i64) -> Value;

    /// 按对象字符串 id 返回查询快照，不按观察者过滤；非法 id 或不支持的对象返回 None。
    fn object_snapshot(&self, id: &str) -> Option<Value>;

    /// 按 FIND 常量返回房间对象的查询快照，不执行观察者视野检查。
    fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value>;

    /// 房间 controller 坐标。
    fn controller_pos(&self, room: &str) -> Option<(u64, u64)>;

    /// 世界内容版本号，用于检测内容变化；它与世界 tick、沙箱派发轮次用途不同。
    fn generation(&self) -> u64;

    /// 地形版本号，用于使缓存的地形编码失效。
    fn terrain_epoch(&self) -> u64;

    /// 房间的世界时间与内容版本标识，不携带房间数据或维持数据生命周期。
    fn room_version(&self, room: &str) -> Option<RoomVersionHandle>;

    /// 世界时间（玩家可见 Game.time 的来源）。
    fn game_time(&self) -> u64;
}

impl WorldSource for kk_world::World {
    fn world_json_for_viewer(&self, viewer: &str, gcl_points: i64) -> Value {
        kk_world::World::world_json_for_viewer(self, viewer, gcl_points)
    }

    fn object_snapshot(&self, id: &str) -> Option<Value> {
        kk_world::ObjId::parse(id).and_then(|id| self.object_snapshot_by_id(id))
    }

    fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        self.find_snapshots_in_room(room, find_const)
    }

    fn controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        kk_world::World::controller_pos(self, room)
    }

    fn generation(&self) -> u64 {
        kk_world::World::generation(self)
    }

    fn terrain_epoch(&self) -> u64 {
        kk_world::World::terrain_epoch(self)
    }

    fn room_version(&self, room: &str) -> Option<RoomVersionHandle> {
        kk_world::World::room_version(self, room)
    }

    fn game_time(&self) -> u64 {
        kk_world::World::time(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_core::Terrain;

    /// 检查 WorldSource 转发的时间、版本、空查询和注入时间字段。
    #[test]
    fn world_source_delegates_to_world() {
        let mut world = kk_world::World::new(7);
        world.register_room("W1N1", Terrain::plains());
        let source: &dyn WorldSource = &world;
        assert_eq!(source.generation(), world.generation());
        assert_eq!(source.terrain_epoch(), world.terrain_epoch());
        assert_eq!(source.game_time(), world.time());
        assert_eq!(
            source.room_version("W1N1").unwrap(),
            world.room_version("W1N1").unwrap()
        );
        assert!(source.room_version("nope").is_none());
        assert_eq!(source.world_json_for_viewer("p", 0)["time"], world.time());
        assert!(source.object_snapshot("zz").is_none());
        assert!(source.find_in_room("W1N1", 1).is_empty());
    }
}
