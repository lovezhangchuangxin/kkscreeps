//! 游戏字符串与快照码值的转换，以及结算和投影共用的资源、容量与强化计算。

use kk_core::constants as c;

/// 身体部件种类，枚举顺序决定快照中保存的 u8 码值。
/// 顺序与 kk-core 的 BODYPARTS_ALL 常量表一致；调整时须考虑已有快照。
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

    /// 统计 hits>0 的指定类型部件数，不计算强化乘数或基础功率。
    pub fn count_active(body: &[kk_schema::room::BodyPart], kind: Self) -> usize {
        body.iter()
            .filter(|p| p.hits > 0 && p.kind == kind.code())
            .count()
    }
}

/// 查询 CONSTRUCTION_COST 中的基础建造成本，未知类型返回 None。
/// 不包含道路的地形系数；常量表的键无需排序。
pub fn construction_cost(structure_type: &str) -> Option<i64> {
    c::CONSTRUCTION_COST
        .iter()
        .find(|(k, _)| *k == structure_type)
        .map(|(_, v)| *v)
}

/// 是否属于 kk-core 的 OBSTACLE_OBJECT_TYPES 静态障碍类型表。
pub fn is_obstacle_type(type_name: &str) -> bool {
    c::OBSTACLE_OBJECT_TYPES.contains(&type_name)
}

/// 按房间控制器等级查询单个 extension 的能量容量，未知等级返回 0。
/// 无控制器的房间由调用方传入 0。
/// 来源：engine/src/processor/intents/extensions/tick.js 的容量更新。
pub fn extension_energy_capacity(rcl: u8) -> i64 {
    let key = rcl.to_string();
    c::EXTENSION_ENERGY_CAPACITY
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// 对 hits>0 且类型匹配的部件求和，每个部件贡献 base × 强化乘数。
/// part_key 与 effect 指定 BOOSTS 表中的部件名和效果名，找不到乘数时使用 1。
/// 每个部件的浮点结果转换为 i64 后再求和；不使用上游的 _oldHits 字段。
/// 参考：engine/src/utils.js 的 calcBodyEffectiveness。
pub fn calc_body_effectiveness(
    body: &[kk_schema::room::BodyPart],
    kind: BodyKind,
    part_key: &str,
    effect: &str,
    base: i64,
) -> i64 {
    static BOOSTS: std::sync::OnceLock<Option<serde_json::Value>> = std::sync::OnceLock::new();
    let table = BOOSTS.get_or_init(|| serde_json::from_str(c::BOOSTS).ok());
    let mut power = 0i64;
    for part in body {
        if part.hits == 0 || BodyKind::from_code(part.kind) != Some(kind) {
            continue;
        }
        let multiplier = match &part.boost {
            None => 1.0,
            Some(boost) => table
                .as_ref()
                .and_then(|t| t.get(part_key))
                .and_then(|t| t.get(boost.as_str()))
                .and_then(|t| t.get(effect))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0),
        };
        power += (base as f64 * multiplier) as i64;
    }
    power
}

/// 汇总 energy、G 及额外资源的数量，包括值为 0 的条目。
/// 额外资源名应唯一且不重复专用字段，否则会被重复计数。
pub fn store_total(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> i64 {
    // 先扩宽再相加，避免两个合法 u32 存量的和在 u32 中溢出。
    (store.energy as i64)
        + (store.ghodium as i64)
        + extra.iter().map(|s| s.amount as i64).sum::<i64>()
}

/// 读取资源存量：energy、G 使用专用字段，其余返回首个同名额外条目。
/// 找不到资源时返回 0。
pub fn store_extra_slot(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
    resource: &str,
) -> i64 {
    match resource {
        "energy" => store.energy as i64,
        kk_core::constants::RESOURCE_GHODIUM => store.ghodium as i64,
        other => extra
            .iter()
            .find(|s| s.resource == other)
            .map(|s| s.amount as i64)
            .unwrap_or(0),
    }
}

/// 计算承载当前资源所需的活跃 CARRY 部件数，用作移动重量。
/// 从 body 尾部向前扣除各部件的容量，强化容量也计入；资源量为 0 时重量为 0。
/// 即使资源超出总容量，返回值也不超过活跃 CARRY 数量。
/// 参考：engine/src/processor/intents/movement.js 的 calcResourcesWeight。
pub fn resources_weight(
    body: &[kk_schema::room::BodyPart],
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> usize {
    let mut total_carry = store_total(store, extra);
    let mut weight = 0usize;
    for part in body.iter().rev() {
        if total_carry <= 0 {
            break;
        }
        if part.hits == 0 || BodyKind::from_code(part.kind) != Some(BodyKind::Carry) {
            continue;
        }
        let boost = match &part.boost {
            None => 1,
            Some(b) => boost_multiplier(part, b, "carry", "capacity"),
        };
        total_carry -= (c::CARRY_CAPACITY * boost).min(total_carry);
        weight += 1;
    }
    weight
}

/// 查询 MINERAL_DENSITY 常量表；不在表中的密度返回 0。
pub fn mineral_density(density: u8) -> i64 {
    let key = density.to_string();
    c::MINERAL_DENSITY
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

fn boost_multiplier(
    _part: &kk_schema::room::BodyPart,
    boost: &str,
    part_key: &str,
    effect: &str,
) -> i64 {
    static BOOSTS: std::sync::OnceLock<Option<serde_json::Value>> = std::sync::OnceLock::new();
    let table = BOOSTS.get_or_init(|| serde_json::from_str(c::BOOSTS).ok());
    table
        .as_ref()
        .and_then(|t| t.get(part_key))
        .and_then(|t| t.get(boost))
        .and_then(|t| t.get(effect))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(1)
}

/// 计算活跃部件的正强化增量，并按从大到小排序。
/// 每个部件贡献 (乘数−1) × base，保留浮点值，交给 apply_boost 最后取整。
/// 建造、修理与升级先确定未强化的效果量和耗能，再应用这些增量，
/// 不能替换为 calc_body_effectiveness 的直接乘法。
/// 来源：engine/src/processor/intents/creeps/{build,repair,upgradeController}.js。
pub fn boost_increments(
    body: &[kk_schema::room::BodyPart],
    kind: BodyKind,
    part_key: &str,
    effect: &str,
    base: i64,
) -> Vec<f64> {
    static BOOSTS: std::sync::OnceLock<Option<serde_json::Value>> = std::sync::OnceLock::new();
    let table = BOOSTS.get_or_init(|| serde_json::from_str(c::BOOSTS).ok());
    let mut increments: Vec<f64> = Vec::new();
    for part in body {
        if part.hits == 0 || BodyKind::from_code(part.kind) != Some(kind) {
            continue;
        }
        let inc = match &part.boost {
            None => 0.0,
            Some(boost) => {
                let m = table
                    .as_ref()
                    .and_then(|t| t.get(part_key))
                    .and_then(|t| t.get(boost.as_str()))
                    .and_then(|t| t.get(effect))
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                (m - 1.0) * base as f64
            }
        };
        if inc > 0.0 {
            increments.push(inc);
        }
    }
    increments.sort_by(|a, b| b.partial_cmp(a).unwrap());
    increments
}

/// 取前 max(effect_points, 0) 个增量，加到基础效果量后向下取整，再限制到 cap。
/// increments 应已按降序排列。基础效果或 cap 为负时保留负值，
/// 以支持放置工地 progress 超过总进度时的结算行为，不额外限制为 0。
pub fn apply_boost(increments: &[f64], effect_points: i64, cap: i64) -> i64 {
    let n = effect_points.max(0) as usize;
    let sum: f64 = increments.iter().take(n).sum();
    let boosted = (effect_points as f64 + sum).floor() as i64;
    boosted.min(cap)
}

/// 为已确定的基础效果量计算强化结果；耗能仍由调用方按未强化效果计算。
pub fn boosted_effect(
    body: &[kk_schema::room::BodyPart],
    kind: BodyKind,
    part_key: &str,
    effect: &str,
    base: i64,
    effect_points: i64,
    cap: i64,
) -> i64 {
    let incs = boost_increments(body, kind, part_key, effect, base);
    apply_boost(&incs, effect_points, cap)
}
