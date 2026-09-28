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

/// 官方 calcBodyEffectiveness（engine/src/utils.js:623-636）的等价物：遍历
/// body，hits>0 且类型匹配的部件按 base × BOOSTS[type][boost][effect] 乘数
/// 求和。GPT 复审 P1-2 清偿：容量投影/移动重量/仲裁 moves/疲劳恢复四消费点
/// 共用（官方 movement.js:41-59 的 weight 用同表的 capacity、:117-118 的
/// moves 与 creeps/tick.js:105 的恢复用 fatigue）。
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

/// 官方 calcResourcesWeight（movement.js:41-59）：store 全槽求和作为携带量，
/// 自 body 尾部往前，hits>0 的 CARRY 按 CARRY_CAPACITY × 强化容量乘数逐个
/// 扣减——装下携带量所需的 CARRY 部件数即重量。
pub fn resources_weight(
    body: &[kk_schema::room::BodyPart],
    store: &kk_schema::room::Store,
) -> usize {
    let mut total_carry = (store.energy + store.ghodium) as i64;
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

/// 官方 build/repair/upgradeController 的强化两步算法（GPT 二轮复审 P1-1
/// 清偿）：能量与基础量按无强化功率计（count × base），效果按增量数组——
/// 每强化部件贡献 (multiplier-1) × base 浮点增量，降序排序后取前
/// effect_points（基础效果点数）个，整体 floor 再钳 cap（官方
/// build.js:71-81 / repair.js:28-38 / upgradeController.js:51-58——
/// 官方 build/repair 不走 calcBodyEffectiveness 的直乘路径）。
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

/// 应用强化增量（官方 slice(0, effect) + floor(effect + sum) + 钳 cap）。
pub fn apply_boost(increments: &[f64], effect_points: i64, cap: i64) -> i64 {
    let n = effect_points.max(0) as usize;
    let sum: f64 = increments.iter().take(n).sum();
    let boosted = (effect_points as f64 + sum).floor() as i64;
    boosted.min(cap)
}

/// 一步式（效果点数已在手的调用方——build/repair）。
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
