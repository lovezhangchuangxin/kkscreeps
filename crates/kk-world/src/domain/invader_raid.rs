//! Invader raid 房间状态与生成器静态表。
//!
//! 上游来源：backend-npm/lib/cronjobs.js 的 `genInvaders`（官方 raid 生成
//! cron）与 lib/utils.js 的 `isCenter`。房间状态是本项目的持久域：官方
//! 把 `invaderGoal`/`status`/`active` 放在 rooms 集合、采集预算放在各
//! source 的 `invaderHarvested` 字段；kk 将四项合并为本结构并随完整检查
//! 点保存。任意玩家/管理快照都不暴露这些内部字段（套件契约）。

use serde::{Deserialize, Serialize};

/// 单房间 raid 状态。`harvested_energy` 聚合官方各 source 的
/// `invaderHarvested` 计数——只有总和被生成器读取（cronjobs.js:387），
/// source 对象本身不可移除，聚合与逐源求和等价。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidRoomState {
    /// 房间级阈值覆盖；None 回退 `INVADERS_ENERGY_GOAL` 常量。
    pub invader_goal: Option<i64>,
    /// 已采集能量预算；成功袭击后归零（cronjobs.js:438）。
    pub harvested_energy: i64,
    /// 是否在后端活跃房间集合中；活跃房不生成新 raid（cronjobs.js:378）。
    pub active: bool,
    /// 后端房间状态；仅 `normal` 参与生成（cronjobs.js:378）。
    pub status: String,
}

impl Default for RaidRoomState {
    fn default() -> Self {
        Self {
            invader_goal: None,
            harvested_energy: 0,
            active: false,
            status: "normal".into(),
        }
    }
}

/// 恢复/注入面的值域校验：阈值为 null 或非负（生成器成功重掷可合法
/// 写 0——官方乘数怪癖）；预算非负；房态非空。种子命令与生成器写点
/// 恒产合法值，本门只拦检查点注入的畸形值（负阈值会令每次扫描必生成）。
pub fn validate(state: &RaidRoomState) -> Result<(), String> {
    if state.invader_goal.is_some_and(|goal| goal < 0) {
        return Err(format!(
            "raid invader_goal 非法（须为 null 或非负）: {:?}",
            state.invader_goal
        ));
    }
    if state.harvested_energy < 0 {
        return Err(format!(
            "raid harvested_energy 非法（须为非负）: {}",
            state.harvested_energy
        ));
    }
    if state.status.is_empty() {
        return Err("raid status 非法（须为非空字符串）".into());
    }
    Ok(())
}

/// 官方 raid 身体表，键为 `type + subtype` 拼名（cronjobs.js:266-273）。
/// 部件顺序逐字保留；生成器按件赋 hits:100 与可选 boost。
pub const RAID_BODIES: &[(&str, &[&str])] = &[
    (
        "bigHealer",
        &[
            "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move",
            "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move",
            "move", "move", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal",
            "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal",
            "heal", "heal", "heal", "heal", "heal", "move",
        ],
    ),
    (
        "bigRanged",
        &[
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "work",
            "move",
        ],
    ),
    (
        "bigMelee",
        &[
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "tough",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "move",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "work",
            "work",
            "work",
            "work",
            "attack",
            "attack",
            "move",
        ],
    ),
    (
        "smallHealer",
        &[
            "move", "move", "move", "move", "heal", "heal", "heal", "heal", "heal", "move",
        ],
    ),
    (
        "smallRanged",
        &[
            "tough",
            "tough",
            "move",
            "move",
            "move",
            "move",
            "ranged_attack",
            "ranged_attack",
            "ranged_attack",
            "move",
        ],
    ),
    (
        "smallMelee",
        &[
            "tough",
            "tough",
            "move",
            "move",
            "move",
            "move",
            "ranged_attack",
            "work",
            "attack",
            "move",
        ],
    ),
];

/// 按身体键查表；拼名与官方 `type + subtype` 一致（big/small × Melee/
/// Ranged/Healer）。
pub fn raid_body(key: &str) -> Option<&'static [&'static str]> {
    RAID_BODIES
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, parts)| *parts)
}

/// 生成器 boost 映射：部件类型 → (扇区中心 X 级, 普通房基础矿)
/// （cronjobs.js:291-308）。不在映射内的部件不强化。
pub fn raid_boost(part: &str, center: bool) -> Option<&'static str> {
    let (base, x_tier) = match part {
        "heal" => ("LO", "XLHO2"),
        "ranged_attack" => ("KO", "XKHO2"),
        "work" => ("ZH", "XZH2O"),
        "attack" => ("UH", "XUH2O"),
        "tough" => ("GO", "XGHO2"),
        _ => return None,
    };
    Some(if center { x_tier } else { base })
}

/// 扇区键：官方用房间名正则去掉两轴末位数字分组（cronjobs.js:393），
/// 等价于按「坐标符号 + 房间号/10」分组；W/N 为负减一坐标（W0 = -1），
/// 负轴先取房间号（|xx|-1）再除 10，W0..W9 与 E0..E9 各自成组、符号
/// 区分扇区。房名非法时返回 None（调用方跳过该房）。
pub fn sector_key(name: &str) -> Option<(bool, u32, bool, u32)> {
    let parsed = kk_core::RoomName::parse(name).ok()?;
    let key = |v: i32| {
        let group = if v < 0 {
            v.unsigned_abs() - 1
        } else {
            v.unsigned_abs()
        } / 10;
        (v < 0, group)
    };
    let (sx, xg) = key(parsed.xx);
    let (sy, yg) = key(parsed.yy);
    Some((sx, xg, sy, yg))
}

/// 扇区中心带：两轴都落在坐标带 |x|（W 轴为 |x+1|）模 10 的 4..=6 区间
/// （backend-npm/lib/utils.js:301 的 `isCenter`，含 W 轴 +1 错位）。
/// 中心房 raid 起手 Ranged、计数直接 escalated（cronjobs.js:328-331）。
pub fn is_center_room(name: &str) -> bool {
    let Ok(parsed) = kk_core::RoomName::parse(name) else {
        return false;
    };
    let in_band = |v: i32| {
        let n = if v < 0 {
            v.unsigned_abs() - 1
        } else {
            v.unsigned_abs()
        };
        (4..=6).contains(&(n % 10))
    };
    in_band(parsed.xx) && in_band(parsed.yy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raid_bodies_match_official_layout() {
        // 大 body 50 件、小 body 10 件；按键可查；无重复键。
        let counts = [
            ("bigHealer", 50),
            ("bigRanged", 50),
            ("bigMelee", 50),
            ("smallHealer", 10),
            ("smallRanged", 10),
            ("smallMelee", 10),
        ];
        assert_eq!(RAID_BODIES.len(), counts.len());
        for (name, len) in counts {
            assert_eq!(raid_body(name).unwrap().len(), len, "{name}");
        }
        // 抽查边界件（官方数组逐字核对：bigMelee 尾段 3×ranged+4×work+2×attack+move）。
        assert_eq!(raid_body("smallMelee").unwrap()[7], "work");
        assert_eq!(raid_body("bigMelee").unwrap()[46], "work");
        assert_eq!(raid_body("bigMelee").unwrap()[47], "attack");
        assert_eq!(raid_body("bigRanged").unwrap()[48], "work");
        assert_eq!(raid_body("bigRanged").unwrap()[49], "move");
        assert!(
            raid_body("bigRanged")
                .unwrap()
                .starts_with(&["tough"; 6][..])
        );
    }

    #[test]
    fn raid_boost_covers_combat_parts_only() {
        assert_eq!(raid_boost("heal", false), Some("LO"));
        assert_eq!(raid_boost("heal", true), Some("XLHO2"));
        assert_eq!(raid_boost("tough", false), Some("GO"));
        assert_eq!(raid_boost("tough", true), Some("XGHO2"));
        assert_eq!(raid_boost("attack", true), Some("XUH2O"));
        assert_eq!(raid_boost("ranged_attack", false), Some("KO"));
        assert_eq!(raid_boost("work", true), Some("XZH2O"));
        // move 不在官方映射内。
        assert_eq!(raid_boost("move", true), None);
    }

    #[test]
    fn sector_key_groups_official_regex_equivalence() {
        use std::collections::BTreeSet;
        // W1N1 的扇区包含 W1N2/W9N9，不含 W10N10（官方正则 ^W\dN\d$）。
        let base = sector_key("W1N1").unwrap();
        assert_eq!(sector_key("W1N2").unwrap(), base);
        assert_eq!(sector_key("W9N9").unwrap(), base);
        assert_ne!(sector_key("W10N10").unwrap(), base);
        // W10..W19 同扇区（floor(10..=19 / 10) == 1）。
        let mut group = BTreeSet::new();
        for n in 10..=19 {
            group.insert(sector_key(&format!("W{n}N10")).unwrap());
        }
        assert_eq!(group.len(), 1);
        // 同数字的 W/E 分属不同扇区。
        assert_ne!(sector_key("E1N1").unwrap(), base);
        assert_eq!(sector_key("E1N1").unwrap(), sector_key("E9N9").unwrap());
    }

    #[test]
    fn center_band_matches_official_quirk() {
        // W 轴带 +1 错位：W4/W5/W6 是中心带，W3/W7 不是；E 轴 E4/E5/E6。
        assert!(is_center_room("W5N5"));
        assert!(is_center_room("W5N6"));
        assert!(is_center_room("W6N4"));
        assert!(is_center_room("E4N4"));
        assert!(!is_center_room("W1N1"));
        assert!(!is_center_room("W3N5"));
        assert!(!is_center_room("W5N3"));
        assert!(!is_center_room("E7N4"));
        assert!(!is_center_room("W7N5"));
    }
}
