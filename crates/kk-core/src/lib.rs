//! 基础数据层：游戏数值表、房间名编解码、地形编解码、带种子的随机数。
//!
//! 数值表由官方常量 dump 生成（`scripts/gen-constants.mjs`），不手抄。
//! 设计见 docs/architecture/overall.md 与 docs/plans/m1-engine-skeleton.md。

pub mod constants;
pub mod rng;
pub mod room_name;
pub mod terrain;

pub use rng::SeededRng;
pub use room_name::RoomName;
pub use terrain::{
    ROOM_SIZE, TERRAIN_LEN, TERRAIN_MASK_LAVA, TERRAIN_MASK_SWAMP, TERRAIN_MASK_WALL, Terrain,
};

#[cfg(test)]
mod tests {
    use super::constants as c;

    /// 生成物与官方常量 dump 的逐键一致性抽查：键总数 + M1 消费全集的值
    /// + 嵌套 JSON 常量可解析回原值（全量逐键断言由生成脚本自校验承担）。
    #[test]
    fn constants_match_official_dump() {
        let dump: serde_json::Value =
            serde_json::from_str(include_str!("../..//kk-runner/assets/constants.json")).unwrap();
        let obj = dump.as_object().unwrap();
        assert_eq!(obj.len(), 423, "官方 dump 键数");

        // M1 结算消费全集（对照官方 constants.js 的关键值）
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

        // 键存在性：生成物必须含 dump 的每个键（按生成规则命名）
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
