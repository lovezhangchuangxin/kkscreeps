//! 世界注入来源 seam（M2B 前置批 P2，ADR-013 决策 2「性能第二形态 = seam
//! 先行」）。
//!
//! 沙箱/玩家面的世界读取经此抽象：M2B 新命名空间的 getter（玩家 API
//! ~150→340 的扩面）一律加在本 trait 之上，不直连 [`kk_world::World`]——
//!（宿主建片与放置路径的 World 写面/方法不在 seam 范畴）——
//! 后续列式/零拷贝实现（snapshot 契约的字节快照形态）替换后端时 getter
//! 面零改写。接口按「沙箱需要什么」塑形，不按 World 恰好暴露什么。
//!
//! 版本纪律（snapshot 契约「代际」节的 typed 层起步）：一轮玩家执行开始时
//! 固定 [`WorldSource::generation`]，本轮全部注入与查询共用同一已提交版本
//! ——`embed::execute_players` 以 debug 断言守护该绑定（循环期间世界不得
//! 变异，结算在循环之后）。

use serde_json::Value;

use kk_world::RoomVersionHandle;

/// 世界注入来源（seam，消费方 crate 持接口——kk-engine `Sandbox` trait
/// 先例）。
pub trait WorldSource {
    /// 每 tick 世界注入（per-viewer 投影；地形协议的缓存与失效在本实现
    /// 之内，M2B 前置批 P3）。
    fn world_json_for_viewer(&self, viewer: &str, gcl_points: i64) -> Value;

    /// 单对象查询（screeps-ok 契约形状；id 为玩家可见字符串形态）。
    fn object_snapshot(&self, id: &str) -> Option<Value>;

    /// 房间 FIND 查询（契约快照形状；FIND 语义单源 matches_find）。
    fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value>;

    /// 房间 controller 坐标。
    fn controller_pos(&self, room: &str) -> Option<(u64, u64)>;

    /// 当前世界内容代际（提交点单调递增——投影缓存失效键）。
    fn generation(&self) -> u64;

    /// 当前地形纪元（仅地形实际替换时递增——地形缓存失效键）。
    fn terrain_epoch(&self) -> u64;

    /// 房间版本句柄（tick + generation；「同一 tick 内所有 getter 绑定同一
    /// 版本」纪律的绑定物）。
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

    /// seam 委托等价（P2 评审清偿）：&dyn WorldSource 与 World 固有访问器
    /// 逐值一致——seam 是纯转发，无自有状态。
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
