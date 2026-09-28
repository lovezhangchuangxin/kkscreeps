//! 游戏词汇表：官方字符串名 ↔ schema 原始码的映射。

use kk_core::constants as c;

/// 身体部件种类（码值 = 官方 BODYPARTS_ALL 顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Move,
    Work,
    Carry,
    Attack,
    RangedAttack,
    Tough,
    Heal,
    Claim,
}

impl BodyKind {
    pub const ALL: [BodyKind; 8] = [
        Self::Move,
        Self::Work,
        Self::Carry,
        Self::Attack,
        Self::RangedAttack,
        Self::Tough,
        Self::Heal,
        Self::Claim,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(code as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Work => "work",
            Self::Carry => "carry",
            Self::Attack => "attack",
            Self::RangedAttack => "ranged_attack",
            Self::Tough => "tough",
            Self::Heal => "heal",
            Self::Claim => "claim",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// 官方 calcBodyEffectiveness 的 M1 无 boost 形态：统计活跃部件数 × 基础功率。
    pub fn count_active(body: &[kk_schema::room::BodyPart], kind: Self) -> usize {
        body.iter()
            .filter(|p| p.hits > 0 && p.kind == kind.code())
            .count()
    }
}

/// 工程进度总量（官方 progressTotal = CONSTRUCTION_COST[structureType]）。
/// 生成物保持 dump 键序（当前恰好有序），查找不假设有序。
pub fn construction_cost(structure_type: &str) -> Option<i64> {
    c::CONSTRUCTION_COST
        .iter()
        .find(|(k, _)| *k == structure_type)
        .map(|(_, v)| *v)
}

/// 是否官方 OBSTACLE_OBJECT_TYPES（静态障碍判定用）。
pub fn is_obstacle_type(type_name: &str) -> bool {
    c::OBSTACLE_OBJECT_TYPES.contains(&type_name)
}

/// extension 的单结构能量容量（官方 extensions/tick.js:10-17 按 RCL 归一
/// `storeCapacityResource.energy = EXTENSION_ENERGY_CAPACITY[level] || 0`）。
/// 表键 0..=8；无 controller 的房查 0 档（官方该情形不归一，M1 按同表取值）。
pub fn extension_energy_capacity(rcl: u8) -> i64 {
    let key = rcl.to_string();
    c::EXTENSION_ENERGY_CAPACITY
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}
