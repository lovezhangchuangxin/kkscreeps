//! 游戏基础数据与纯计算规则：常量、坐标、地形、伪随机数和 CPU 桶预算。
//!
//! [`constants`] 由 `scripts/gen-constants.mjs` 根据上游常量数据生成。
//! [`RoomName`] 使用带方向的房间坐标，不负责限制世界大小；
//! [`Terrain`] 保存单房间的地形位码；[`SeededRng`] 提供可重复的随机序列。
//! 本 crate 不执行游戏结算，也不管理世界状态或磁盘存储。

pub mod constants;
pub mod cpu;
pub mod rng;
pub mod room_name;
pub mod terrain;
pub mod tick;
pub use tick::RunEpoch;

pub use rng::SeededRng;
pub use room_name::RoomName;
pub use terrain::{
    ROOM_SIZE, TERRAIN_LEN, TERRAIN_MASK_LAVA, TERRAIN_MASK_SWAMP, TERRAIN_MASK_WALL, Terrain,
};

#[cfg(test)]
mod tests {
    use super::constants as c;

    /// 检查常量数据的键数、生成文件的键完整性、部分关键值和嵌套 JSON。
    /// 这不是全部常量值的逐键比较；生成器自校验也只检查键是否存在。
    #[test]
    fn constants_match_official_dump() {
        let dump: serde_json::Value =
            serde_json::from_str(include_str!("../../kk-runner/assets/constants.json")).unwrap();
        let obj = dump.as_object().unwrap();
        // 423 个上游键 + 4 个本地新增射程常量（上游 dump 无此名字，
        // 见 builtin:creep.*.range 规则与 player-precheck-rules 契约）。
        assert_eq!(obj.len(), 427, "官方 dump + 本地新增键数");

        // 关键游戏数值的固定期望，用于发现常量变化。
        assert_eq!(c::HARVEST_POWER, 2);
        assert_eq!(c::BUILD_POWER, 5);
        assert_eq!(c::UPGRADE_CONTROLLER_POWER, 1);
        assert_eq!(c::CARRY_CAPACITY, 50);
        assert_eq!(c::CREEP_LIFE_TIME, 1500);
        assert_eq!(c::CREEP_CLAIM_LIFE_TIME, 600);
        assert_eq!(c::CREEP_SPAWN_TIME, 3);
        assert_eq!(c::MAX_CREEP_SIZE, 50);
        assert_eq!(c::ENERGY_REGEN_TIME, 300);
        assert_eq!(c::ENERGY_DECAY, 1000);
        assert_eq!(c::SPAWN_ENERGY_CAPACITY, 300);
        assert_eq!(c::SOURCE_ENERGY_CAPACITY, 3000);
        assert_eq!(c::SOURCE_ENERGY_NEUTRAL_CAPACITY, 1500);
        assert_eq!(c::SOURCE_ENERGY_KEEPER_CAPACITY, 4000);
        assert_eq!(c::WALL_HITS, 1);
        assert_eq!(c::WALL_HITS_MAX, 300000000);
        assert_eq!(c::CONTROLLER_MAX_UPGRADE_PER_TICK, 15);
        assert_eq!(c::CONTROLLER_DOWNGRADE_RESTORE, 100);
        assert_eq!(c::TERRAIN_MASK_WALL, 1);
        assert_eq!(c::TERRAIN_MASK_SWAMP, 2);
        assert_eq!(c::TERRAIN_MASK_LAVA, 4);

        // 每个源键都必须有对应声明；数字开头的键按生成规则加下划线。
        let written = include_str!("constants.rs");
        for key in obj.keys() {
            let ident = if key.starts_with(|ch: char| ch.is_ascii_digit()) {
                format!("_{key}")
            } else {
                key.clone()
            };
            assert!(
                written.contains(&format!("pub const {ident}:")),
                "生成物缺键 {key}"
            );
        }

        // 嵌套 JSON 常量可解析回原值（键序无关比较）
        for key in ["CONTROLLER_STRUCTURES", "BOOSTS", "POWER_INFO", "REACTIONS"] {
            let raw = match key {
                "CONTROLLER_STRUCTURES" => c::CONTROLLER_STRUCTURES,
                "BOOSTS" => c::BOOSTS,
                "POWER_INFO" => c::POWER_INFO,
                _ => c::REACTIONS,
            };
            let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
            assert_eq!(parsed, obj[key], "{key} 内嵌 JSON 与 dump 不一致");
        }
    }
}
