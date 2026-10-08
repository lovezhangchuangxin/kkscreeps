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

/// storage 的当前权威容量：结算、玩家投影与查询快照同源。
/// 基础量按官方 storages/tick.js——控制器在场时要求 level>0、归属与
/// storage 一致且 CONTROLLER_STRUCTURES.storage[级]>0，否则 0；无控制器
/// 房间保持建造时的 1M。PWR_OPERATE_STORAGE 效果仅在基础量 >0 时叠加
/// （endTime > time 严格有效期）。容器存储快照无该字段，容量恒为派生值。
/// 已知偏差：官方把权威 storeCapacity 落库并在对象 tick 相（意图相之后）
/// 重算，意图相读到上一拍末的值；kk 读时派生，同 tick 控制器升级/易主的
/// 窗口内更宽松，tick 末两侧收敛一致。
pub fn storage_capacity(
    controller: Option<&kk_schema::room::ControllerObj>,
    storage_owner: &str,
    effects: Option<&Vec<crate::EffectEntry>>,
    time: u64,
) -> i64 {
    let base = match controller {
        None => c::STORAGE_CAPACITY,
        Some(ctl)
            if ctl.level > 0
                && ctl.owner.as_deref() == Some(storage_owner)
                && controller_allows("storage", i64::from(ctl.level)) =>
        {
            c::STORAGE_CAPACITY
        }
        Some(_) => 0,
    };
    if base == 0 {
        return 0;
    }
    match crate::effects::active_power(effects, c::PWR_OPERATE_STORAGE, time) {
        Some(effect) => base + power_effect(c::PWR_OPERATE_STORAGE, effect.level).unwrap_or(0),
        None => base,
    }
}

/// CONTROLLER_STRUCTURES[type][level] 的进程级查表；缺键或非法返回 0。
fn controller_allows(structure_type: &str, level: i64) -> bool {
    use std::sync::OnceLock;
    static TABLE: OnceLock<serde_json::Value> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        serde_json::from_str::<serde_json::Value>(c::CONTROLLER_STRUCTURES).unwrap_or_default()
    });
    table[structure_type][level.to_string()]
        .as_i64()
        .is_some_and(|n| n > 0)
}

/// POWER_INFO[power].effect[level-1] 取整；缺失或越界返回 None。
/// 官方 storages/tick.js 的效果增量是整数表，与 kk-engine 的同名私有
/// 查表语义一致；此处独立缓存供投影/结算共用。
fn power_effect(power: i64, level: i64) -> Option<i64> {
    use std::sync::OnceLock;
    static INFO: OnceLock<serde_json::Value> = OnceLock::new();
    let info = INFO.get_or_init(|| {
        serde_json::from_str::<serde_json::Value>(c::POWER_INFO).unwrap_or_default()
    });
    info[power.to_string()]["effect"]
        .as_array()?
        .get((level - 1).max(0) as usize)?
        .as_f64()
        .map(|f| f as i64)
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

/// 反应表查询：两种试剂的产物（REACTIONS 双向对称）。找不到产物返回 None。
/// 参考：engine/src/utils.js 的 REACTIONS 常量。
pub fn reaction_product(a: &str, b: &str) -> Option<&'static str> {
    static REACTIONS: std::sync::OnceLock<Option<serde_json::Value>> = std::sync::OnceLock::new();
    let table = REACTIONS.get_or_init(|| serde_json::from_str(c::REACTIONS).ok());
    let table = table.as_ref()?;
    let hit = |x: &str, y: &str| {
        table
            .get(x)
            .and_then(|t| t.get(y))
            .and_then(serde_json::Value::as_str)
    };
    hit(a, b).or(hit(b, a))
}

/// 逆反应查询：化合物的两种试剂（按名称排序的稳定对）。基础矿物（无
/// 逆反应条目）返回 None。仅供 total_reaction_time 的链递归——顺序不
/// 影响求和；reverseReaction 结算的试剂方向在 kk-engine 按表序单独取。
fn reaction_reagents(compound: &str) -> Option<(String, String)> {
    static INVERSE: std::sync::OnceLock<Option<Vec<(String, String, String)>>> =
        std::sync::OnceLock::new();
    let inverse = INVERSE.get_or_init(|| {
        let table: serde_json::Value = serde_json::from_str(c::REACTIONS).ok()?;
        let mut rows: Vec<(String, String, String)> = Vec::new();
        for (a, inner) in table.as_object()?.iter() {
            for (b, product) in inner.as_object()?.iter() {
                // 同一产物可能来自对称条目（A+B 与 B+A），只留一对。
                let product = product.as_str()?.to_string();
                if rows.iter().any(|(_, _, p)| *p == product) {
                    continue;
                }
                rows.push((a.clone(), b.clone(), product));
            }
        }
        Some(rows)
    });
    let rows = inverse.as_ref()?;
    let (a, b, _) = rows.iter().find(|(_, _, p)| p == compound)?;
    // 排序对：与官方 labReverseCases 矩阵的 localeCompare 序一致。
    Some(if a <= b {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    })
}

/// 化合物的完整反应链时长（reactionTime 的递归和，官方
/// calcTotalReactionsTime）：产物时间 + 两试剂各自的链时长。基础矿物为 0。
/// 用于 unboost 冷却公式（每部件 total×15/5）。
pub fn total_reaction_time(compound: &str) -> i64 {
    fn chain(name: &str, seen: &mut Vec<String>) -> i64 {
        if seen.iter().any(|n| n == name) {
            return 0; // 表成环（理论不可达）时按不重复计
        }
        seen.push(name.to_string());
        let own = c::REACTION_TIME
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| *v)
            .unwrap_or(0);
        let Some((a, b)) = reaction_reagents(name) else {
            seen.pop();
            return own;
        };
        let total = own + chain(&a, seen) + chain(&b, seen);
        seen.pop();
        total
    }
    chain(compound, &mut Vec::new())
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
        // 官方 build/repair/upgradeController 的 _.map 不过滤 hits==0 的
        // 死亡部件（增量池含死亡强化部件，基础量另按存活部件计）。
        if BodyKind::from_code(part.kind) != Some(kind) {
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

#[cfg(test)]
mod reaction_tests {
    use super::*;

    /// REACTIONS 对称查询、逆反应排序对与反应链时长（官方矩阵与
    /// UNBOOST-005 的锚值：UH 链 = REACTION_TIME[UH] = 10）。
    #[test]
    fn reaction_helpers_match_official_tables() {
        assert_eq!(reaction_product("H", "O"), Some("OH"));
        assert_eq!(reaction_product("O", "H"), Some("OH"));
        assert_eq!(reaction_product("H", "H"), None);
        assert_eq!(reaction_product("ZK", "UL"), Some("G"));
        let oh = reaction_reagents("OH");
        assert_eq!(
            oh.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            Some(("H", "O"))
        );
        let g = reaction_reagents("G");
        assert_eq!(
            g.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            Some(("UL", "ZK"))
        );
        assert_eq!(reaction_reagents("H"), None);
        assert_eq!(total_reaction_time("UH"), 10);
        assert_eq!(total_reaction_time("OH"), 20);
        // G 链：G(5) + ZK(5) + UL(5) 的递归和。
        assert_eq!(total_reaction_time("G"), 15);
        // X 化合物：60 + UH2O 链（UH2O 5 + UH 10 + OH 20）。
        assert_eq!(total_reaction_time("XUH2O"), 60 + 5 + 10 + 20);
    }
}
