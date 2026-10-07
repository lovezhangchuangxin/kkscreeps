//! 默认 link/creep 资源、移动、战斗与建造族动作描述及 Generic 仓库容量规则，
//! 不安装引擎处理器。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::*;

mod store;
pub use store::StoreRules;

/// 已有局部恢复边界的内建资源动作；宿主拒绝资格与 runtime 暂存共用此表。
pub const RESOURCE_ACTION_SLOTS: [&str; 3] = [
    "builtin:link.transfer",
    "builtin:creep.transfer",
    "builtin:creep.withdraw",
];

/// 已有局部恢复边界的内建移动动作；宿主拒绝资格与 runtime 暂存共用此表。
pub const MOVE_ACTION_SLOTS: [&str; 1] = ["builtin:creep.move"];

/// 已有局部恢复边界的内建战斗动作；宿主拒绝资格与 runtime 暂存共用此表。
pub const COMBAT_ACTION_SLOTS: [&str; 5] = [
    "builtin:creep.attack",
    "builtin:creep.rangedAttack",
    "builtin:creep.rangedMassAttack",
    "builtin:creep.heal",
    "builtin:creep.rangedHeal",
];

/// 已有局部恢复边界的内建造族动作（build/repair/dismantle）；
/// 宿主拒绝资格与 runtime 暂存共用此表。
pub const BUILD_ACTION_SLOTS: [&str; 3] = [
    "builtin:creep.build",
    "builtin:creep.repair",
    "builtin:creep.dismantle",
];

/// 对象 tick 中的受击数值链；不是玩家意图或整个 creep tick 替换。
pub const INCOMING_DAMAGE_SLOT: &str = "builtin:creep.receiveDamage";

/// 房间级移动竞争优先级链；只改变竞争者排序，不是准入或获准集合替换。
pub const MOVE_CONTEST_SLOT: &str = "builtin:room.moveContest";

/// 仲裁后的疲劳/MOVE 门槛准入链；仲裁与执行阶段同源消费准入终值。
pub const MOVE_ADMISSION_SLOT: &str = "builtin:creep.moveAdmission";

/// 仲裁期的目的格障碍链；单向追加阻挡，让位链增量语义保留在 native。
pub const MOVE_OBSTACLE_SLOT: &str = "builtin:room.moveObstacle";

/// 意图登记后的移动请求预检链；单向收紧，被否决请求等同从未登记。
pub const MOVE_PRECHECK_SLOT: &str = "builtin:room.movePrecheck";

/// 拉拽意图登记后的配对否决链；单向收紧，被否决配对等同从未登记。
pub const MOVE_PULL_REGISTRATION_SLOT: &str = "builtin:room.movePullRegistration";

/// 取消链结算后的移动代价链；逐幸存者疲劳数值替换，缺项回退原生。
pub const MOVE_COST_SLOT: &str = "builtin:room.moveCost";

/// 内建实际派发的槽；自有动作装配必须跳过它们。
pub const SLOT_KEYS: [&str; 20] = [
    RESOURCE_ACTION_SLOTS[0],
    "builtin:link.tick",
    RESOURCE_ACTION_SLOTS[1],
    RESOURCE_ACTION_SLOTS[2],
    MOVE_ACTION_SLOTS[0],
    COMBAT_ACTION_SLOTS[0],
    COMBAT_ACTION_SLOTS[1],
    COMBAT_ACTION_SLOTS[2],
    COMBAT_ACTION_SLOTS[3],
    COMBAT_ACTION_SLOTS[4],
    BUILD_ACTION_SLOTS[0],
    BUILD_ACTION_SLOTS[1],
    BUILD_ACTION_SLOTS[2],
    INCOMING_DAMAGE_SLOT,
    MOVE_CONTEST_SLOT,
    MOVE_ADMISSION_SLOT,
    MOVE_OBSTACLE_SLOT,
    MOVE_PRECHECK_SLOT,
    MOVE_PULL_REGISTRATION_SLOT,
    MOVE_COST_SLOT,
];

fn key(name: &str) -> Key {
    Key::new(format!("builtin:{name}")).expect("内建键合法")
}
fn keys(names: &[&str]) -> BTreeSet<Key> {
    names.iter().map(|name| key(name)).collect()
}
fn signed(min: i64, max: i64) -> ValueSchema {
    ValueSchema {
        kind: ValueKind::Number(NumberDomain::Signed { min, max }),
        nullable: false,
    }
}
fn string(max_bytes: u32) -> ValueSchema {
    ValueSchema {
        kind: ValueKind::String { max_bytes },
        nullable: false,
    }
}

/// link 家族的有效规则快照。装配时从冻结计划解析一次，
/// 结算与放置热路径按字段读取；稳定键解析只应发生在装配时。
/// 值只反映计划中的规则；字段描述与处理器不在此快照内。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkRules {
    pub hits: i64,
    pub hits_max: i64,
    pub capacity: i64,
    /// 冷却系数：转移后按此值乘两 link 的切比雪夫距离。
    pub cooldown: i64,
    /// 损耗率：ceil(amount * loss_ratio) 计算转移损耗。
    pub loss_ratio: f64,
    pub build_cost: i64,
}

impl LinkRules {
    /// 从计划的有效规则提取全部 link 规则；缺键说明计划未含 builtin:link。
    /// 句柄解析只发生在此处，数值域已在冻结时校验。
    pub fn from_plan(rules: &EffectiveRules) -> Result<Self, String> {
        let signed = |name: &str| -> Result<i64, String> {
            let key = key(name);
            let Some(handle) = rules.resolve(&key) else {
                return Err(format!("计划缺少规则 builtin:link.{name}"));
            };
            match rules.get(handle) {
                Some(Number::Signed(v)) => Ok(*v),
                Some(other) => Err(format!("规则 builtin:link.{name} 须为整数，实得 {other:?}")),
                None => Err(format!("规则 builtin:link.{name} 句柄越界")),
            }
        };
        let loss_ratio = {
            let handle = rules
                .resolve(&key("link.lossRatio"))
                .ok_or_else(|| "计划缺少规则 builtin:link.lossRatio".to_string())?;
            match rules.get(handle) {
                Some(Number::Float(v)) => *v,
                Some(other) => {
                    return Err(format!(
                        "规则 builtin:link.lossRatio 须为浮点，实得 {other:?}"
                    ));
                }
                None => return Err("规则 builtin:link.lossRatio 句柄越界".into()),
            }
        };
        Ok(Self {
            hits: signed("link.hits")?,
            hits_max: signed("link.hitsMax")?,
            capacity: signed("link.capacity")?,
            cooldown: signed("link.cooldown")?,
            loss_ratio,
            build_cost: signed("link.buildCost")?,
        })
    }
}

/// 内建结构 RCL 配额的有效规则快照（G11.a）。键集与官方
/// CONTROLLER_STRUCTURES 一一对应（逐型逐级，官方无的级别键不声明），
/// 默认值即官方表值。引擎结算配额判定与玩家面 CONTROLLER_STRUCTURES
/// 常量注入同源消费；空快照（默认引擎）在判定处回退官方静态表。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureQuotaSnapshot {
    /// structureType →（RCL → 配额）。from_plan 恒填充全部官方键。
    pub entries: std::collections::BTreeMap<String, std::collections::BTreeMap<u8, i64>>,
}

impl StructureQuotaSnapshot {
    /// 从计划的有效规则提取全部配额规则；缺键说明计划不含配额族。
    pub fn from_plan(rules: &EffectiveRules) -> Result<Self, String> {
        let table: serde_json::Value = serde_json::from_str(c::CONTROLLER_STRUCTURES)
            .expect("内建 RCL 表合法（kk-core 生成物测试锁定）");
        let mut entries = std::collections::BTreeMap::new();
        for (stype, levels) in table.as_object().expect("官方表为对象") {
            let mut per_level = std::collections::BTreeMap::new();
            for level in levels.as_object().expect("逐级键").keys() {
                let rule_key = key(&format!("structure.quota.{stype}.rcl{level}"));
                let Some(handle) = rules.resolve(&rule_key) else {
                    return Err(format!(
                        "计划缺少规则 builtin:structure.quota.{stype}.rcl{level}"
                    ));
                };
                let quota = match rules.get(handle) {
                    Some(Number::Signed(v)) => *v,
                    Some(other) => {
                        return Err(format!(
                            "规则 builtin:structure.quota.{stype}.rcl{level} 须为整数，实得 {other:?}"
                        ));
                    }
                    None => {
                        return Err(format!(
                            "规则 builtin:structure.quota.{stype}.rcl{level} 句柄越界"
                        ));
                    }
                };
                let parsed: u8 = level.parse().expect("级别键为 0..=8");
                per_level.insert(parsed, quota);
            }
            entries.insert(stype.clone(), per_level);
        }
        Ok(Self { entries })
    }

    /// 查某型某级的生效配额；快照无该型/级时返回 None（调用方回退官方
    /// 静态表——默认引擎的空快照即此路径）。
    pub fn get(&self, structure_type: &str, level: i64) -> Option<i64> {
        if !(0..=8).contains(&level) {
            return None;
        }
        self.entries
            .get(structure_type)?
            .get(&(level as u8))
            .copied()
    }

    /// 官方 CONTROLLER_STRUCTURES 常量表折叠本快照后的玩家面注入值。
    /// 键形保持官方（快照键集恒等于官方键集），无覆盖时与官方表同值。
    /// 键序保真依赖 workspace 经 deno_core 统一启用 serde_json 的
    /// preserve_order（BTreeMap 后备会按字典序重排类型键，值不变但
    /// 玩家面 Object.keys 顺序可观察）。
    pub fn fold_controller_structures(&self) -> serde_json::Value {
        let mut table: serde_json::Value = serde_json::from_str(c::CONTROLLER_STRUCTURES)
            .expect("内建 RCL 表合法（kk-core 生成物测试锁定）");
        for (stype, levels) in &self.entries {
            for (level, quota) in levels {
                table[stype][level.to_string()] = serde_json::json!(quota);
            }
        }
        table
    }
}

/// 单目标战斗动作射程的有效规则快照。装配时从冻结计划解析一次，
/// 结算热路径与玩家常量注入按字段读取。近战/远程分开声明：mod 可以
/// 只放宽 heal 而保持 rangedHeal 原值。rangedMassAttack 是三格面积
/// 语义，不在此快照内（首批不开放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombatRanges {
    /// attack 的切比雪夫射程。
    pub attack: i64,
    /// rangedAttack 的切比雪夫射程。
    pub ranged_attack: i64,
    /// heal 的切比雪夫射程。
    pub heal: i64,
    /// rangedHeal 的切比雪夫射程。
    pub ranged_heal: i64,
}

impl CombatRanges {
    /// 从计划的有效规则提取全部战斗射程；缺键说明计划未含 builtin:creep。
    /// 句柄解析只发生在此处，数值域已在冻结时校验。
    pub fn from_plan(rules: &EffectiveRules) -> Result<Self, String> {
        let signed = |name: &str| -> Result<i64, String> {
            let key = key(&format!("creep.{name}"));
            let Some(handle) = rules.resolve(&key) else {
                return Err(format!("计划缺少规则 builtin:creep.{name}"));
            };
            match rules.get(handle) {
                Some(Number::Signed(v)) => Ok(*v),
                Some(other) => Err(format!(
                    "规则 builtin:creep.{name} 须为整数，实得 {other:?}"
                )),
                None => Err(format!("规则 builtin:creep.{name} 句柄越界")),
            }
        };
        Ok(Self {
            attack: signed("attack.range")?,
            ranged_attack: signed("rangedAttack.range")?,
            heal: signed("heal.range")?,
            ranged_heal: signed("rangedHeal.range")?,
        })
    }
}

/// 移动玩家门规则快照：玩家面 `move()`/`moveTo()` 的疲劳/MOVE 预检
/// 与引擎仲裁原生准入预填（`can_move`）同源读取。默认值即官方行为
/// （疲劳 0 才可自发移动、至少 1 个存活 MOVE）；被拉豁免经
/// `move(target)` 结构通道到达仲裁，不在此规则面内。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveGate {
    /// 自发移动允许的意图时疲劳上界；0 即官方「疲劳须为 0」。
    pub fatigue_limit: i64,
    /// 自发移动要求的最少存活 MOVE 部件数；1 即官方门槛。
    pub min_active_moves: i64,
}

impl Default for MoveGate {
    /// 默认即官方行为：疲劳须为 0、至少 1 个存活 MOVE。与常量表
    /// 同源锁定（catalog 逐键比对），测试与无计划引擎共用。
    fn default() -> Self {
        Self {
            fatigue_limit: c::MOVE_FATIGUE_LIMIT,
            min_active_moves: c::MOVE_MIN_ACTIVE_MOVES,
        }
    }
}

impl MoveGate {
    /// 从计划的有效规则提取移动玩家门；缺键说明计划未含 builtin:creep。
    /// 句柄解析只发生在此处，数值域已在冻结时校验。
    pub fn from_plan(rules: &EffectiveRules) -> Result<Self, String> {
        let signed = |name: &str| -> Result<i64, String> {
            let key = key(&format!("creep.{name}"));
            let Some(handle) = rules.resolve(&key) else {
                return Err(format!("计划缺少规则 builtin:creep.{name}"));
            };
            match rules.get(handle) {
                Some(Number::Signed(v)) => Ok(*v),
                Some(other) => Err(format!(
                    "规则 builtin:creep.{name} 须为整数，实得 {other:?}"
                )),
                None => Err(format!("规则 builtin:creep.{name} 句柄越界")),
            }
        };
        Ok(Self {
            fatigue_limit: signed("move.fatigueLimit")?,
            min_active_moves: signed("move.minActiveMoves")?,
        })
    }
}

/// 地形静态通行规则快照：仲裁静态障碍判定的墙项与玩家面原生寻路
/// 地形包同源读取。默认即官方行为（墙不可通行）；`wall_passable`
/// 将墙位按平原参与通行（wall|swamp 保留沼泽语义，疲劳与代价按
/// 对应地形费率）。物理地形查询（getTerrain/look/FIND_EXIT）与
/// 放置检查不随本规则改变，未注册房的全墙保护环不放开。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainPassability {
    pub wall_passable: bool,
}

impl TerrainPassability {
    /// 从计划的有效规则提取地形静态通行；缺键说明计划未含 builtin:terrain。
    /// 句柄解析只发生在此处，数值域已在冻结时校验。
    pub fn from_plan(rules: &EffectiveRules) -> Result<Self, String> {
        let handle = rules
            .resolve(&key("terrain.wallPassable"))
            .ok_or_else(|| "计划缺少规则 builtin:terrain.wallPassable".to_string())?;
        match rules.get(handle) {
            Some(Number::Signed(0)) => Ok(Self {
                wall_passable: false,
            }),
            Some(Number::Signed(1)) => Ok(Self {
                wall_passable: true,
            }),
            Some(other) => Err(format!(
                "规则 builtin:terrain.wallPassable 须为 0 或 1，实得 {other:?}"
            )),
            None => Err("规则 builtin:terrain.wallPassable 句柄越界".into()),
        }
    }
}

/// 内置贡献入口，保留早期 link 函数名；现含 link、creep 资源/移动/战斗/建造族槽与仓库规则。
pub fn link() -> Contribution {
    let consumers = keys(&[
        "link.init",
        "link.build",
        "link.capacity",
        "link.active",
        "link.precheck",
        "link.transfer.native",
        "link.tick.native",
        "link.constants",
        "link.project",
    ]);
    let mut bindings = consumers.clone();
    bindings.insert(key("link.sanitize"));
    let mut fields = Vec::new();
    let mut field = |name: &str, schema: ValueSchema, default: FieldDefault| {
        let binding = key(&format!("link.field.{name}"));
        bindings.insert(binding.clone());
        fields.push(FieldDescriptor {
            key: key(name),
            schema,
            default,
            visibility: Visibility::Public,
            binding,
        });
    };
    field("id", string(128), FieldDefault::Missing);
    field("room", string(128), FieldDefault::Missing);
    let coordinate = ValueSchema {
        kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
        nullable: false,
    };
    field("x", coordinate.clone(), FieldDefault::Missing);
    field("y", coordinate, FieldDefault::Missing);
    field(
        "owner",
        ValueSchema {
            nullable: true,
            ..string(128)
        },
        FieldDefault::Missing,
    );
    field(
        "structureType",
        ValueSchema {
            kind: ValueKind::Enum(["link".into()].into()),
            nullable: false,
        },
        FieldDefault::Value(json!("link")),
    );
    field(
        "hits",
        signed(0, i64::from(u32::MAX)),
        FieldDefault::Missing,
    );
    field(
        "hitsMax",
        signed(0, i64::from(u32::MAX)),
        FieldDefault::Missing,
    );
    field("cooldown", signed(0, i64::MAX), FieldDefault::Missing);
    field(
        "store",
        ValueSchema {
            kind: ValueKind::Map {
                value: Box::new(signed(0, i64::MAX)),
                max_len: 4096,
                max_key_bytes: 128,
            },
            nullable: false,
        },
        FieldDefault::Missing,
    );
    field("storeCapacity", signed(0, i64::MAX), FieldDefault::Missing);
    field(
        "storeCapacityResource",
        ValueSchema {
            kind: ValueKind::Map {
                value: Box::new(signed(0, i64::MAX)),
                max_len: 4096,
                max_key_bytes: 128,
            },
            nullable: false,
        },
        FieldDefault::Missing,
    );

    let mut rules = Vec::new();
    for (name, value, max) in [
        ("link.hits", c::LINK_HITS, i64::from(u32::MAX)),
        ("link.hitsMax", c::LINK_HITS_MAX, i64::from(u32::MAX)),
        ("link.capacity", c::LINK_CAPACITY, i64::from(u32::MAX)),
        ("link.cooldown", c::LINK_COOLDOWN, i64::MAX / 49),
        (
            "link.buildCost",
            c::CONSTRUCTION_COST
                .iter()
                .find(|(name, _)| *name == "link")
                .expect("link 建造成本")
                .1,
            i64::from(u32::MAX),
        ),
    ] {
        rules.push(RuleDescriptor {
            key: key(name),
            domain: NumberDomain::Signed { min: 0, max },
            default: Number::Signed(value),
            consumers: consumers.clone(),
        });
    }
    rules.push(RuleDescriptor {
        key: key("link.lossRatio"),
        domain: NumberDomain::Float { min: 0.0, max: 1.0 },
        default: Number::Float(c::LINK_LOSS_RATIO),
        consumers: consumers.clone(),
    });
    // 内建结构 RCL 配额规则（G11.a）：逐型逐级标量，键集与官方
    // CONTROLLER_STRUCTURES 一一对应（官方无的级别键不声明，折叠注入
    // 保持官方键形）；默认即官方表值。消费三端同源：引擎结算配额判定
    // （allowed_for 族）与玩家面 CONTROLLER_STRUCTURES 常量注入均从同一
    // 冻结计划派生。link 原有的 link.rcl0..8 已并入本族（键集改官方形
    // 1..8），修复其只进引擎不进玩家面的既有缺口。
    let quota_consumers = keys(&["structure.quota.native"]);
    let quotas: serde_json::Value =
        serde_json::from_str(c::CONTROLLER_STRUCTURES).expect("内建 RCL 表合法");
    for (stype, levels) in quotas.as_object().expect("官方表为对象") {
        for level in levels.as_object().expect("逐级键").keys() {
            rules.push(RuleDescriptor {
                key: key(&format!("structure.quota.{stype}.rcl{level}")),
                domain: NumberDomain::Signed {
                    min: 0,
                    max: i64::from(u32::MAX),
                },
                default: Number::Signed(quotas[stype][level].as_i64().expect("官方配额为整数")),
                consumers: quota_consumers.clone(),
            });
        }
    }
    // 单目标战斗射程规则：结算（Plane 快照）与玩家常量注入同源消费。
    // 上限 50 为房间对角线内的安全放宽空间；rangedMassAttack 的面积
    // 语义不在此族（首批不开放）。
    let combat_consumers = keys(&[
        "creep.attack.native",
        "creep.rangedAttack.native",
        "creep.heal.native",
        "creep.rangedHeal.native",
    ]);
    for (name, value) in [
        ("creep.attack.range", c::ATTACK_RANGE),
        ("creep.rangedAttack.range", c::RANGED_ATTACK_RANGE),
        ("creep.heal.range", c::HEAL_RANGE),
        ("creep.rangedHeal.range", c::RANGED_HEAL_RANGE),
    ] {
        rules.push(RuleDescriptor {
            key: key(name),
            domain: NumberDomain::Signed { min: 0, max: 50 },
            default: Number::Signed(value),
            consumers: combat_consumers.clone(),
        });
    }
    // 移动玩家门规则：玩家面预检常量注入与仲裁原生准入预填（Plane
    // 快照）同源消费。fatigueLimit 上界对齐疲劳 u32 域；minActiveMoves
    // 上界 50 为身体部件数上限。被拉豁免与 visualizePathStyle 官方怪癖
    // 不在规则面内。
    let move_gate_consumers = keys(&["creep.move.native"]);
    for (name, value, max) in [
        (
            "creep.move.fatigueLimit",
            c::MOVE_FATIGUE_LIMIT,
            i64::from(u32::MAX),
        ),
        ("creep.move.minActiveMoves", c::MOVE_MIN_ACTIVE_MOVES, 50),
    ] {
        rules.push(RuleDescriptor {
            key: key(name),
            domain: NumberDomain::Signed { min: 0, max },
            default: Number::Signed(value),
            consumers: move_gate_consumers.clone(),
        });
    }
    // 地形静态通行规则：结算静态障碍判定的墙项与玩家面原生寻路地形
    // 同源消费。默认 0 即官方「墙不可通行」；1 将墙位按平原放行
    // （wall|swamp 保留沼泽语义）。物理地形查询与放置检查不在此面内。
    let terrain_consumers = keys(&["terrain.passability.native"]);
    rules.push(RuleDescriptor {
        key: key("terrain.wallPassable"),
        domain: NumberDomain::Signed { min: 0, max: 1 },
        default: Number::Signed(0),
        consumers: terrain_consumers,
    });
    let (store_bindings, store_rules) = store::declarations();
    bindings.extend(store_bindings);
    rules.extend(store_rules);
    // creep 的类型化对象、玩家 API 和参数清洗沿用原通路；本片声明
    // 具名处理槽，不把只读宿主视图冒充完整 creep 对象/意图描述目录。
    bindings.extend(keys(&[
        "creep.transfer.native",
        "creep.withdraw.native",
        "creep.move.native",
        "creep.attack.native",
        "creep.rangedAttack.native",
        "creep.rangedMassAttack.native",
        "creep.heal.native",
        "creep.rangedHeal.native",
        "creep.build.native",
        "creep.repair.native",
        "creep.dismantle.native",
        "creep.receiveDamage.native",
        "room.moveContest.native",
        "creep.moveAdmission.native",
        "room.moveObstacle.native",
        "room.movePrecheck.native",
        "room.movePullRegistration.native",
        "room.moveCost.native",
        "terrain.passability.native",
        "structure.quota.native",
    ]));
    let slots = vec![
        SlotDescriptor {
            key: key("link.transfer"),
            stage: Stage::ObjectAction,
            native_binding: key("link.transfer.native"),
        },
        SlotDescriptor {
            key: key("link.tick"),
            stage: Stage::ObjectTick,
            native_binding: key("link.tick.native"),
        },
        SlotDescriptor {
            key: key("creep.transfer"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.transfer.native"),
        },
        SlotDescriptor {
            key: key("creep.withdraw"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.withdraw.native"),
        },
        SlotDescriptor {
            key: key("creep.move"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.move.native"),
        },
        SlotDescriptor {
            key: key("creep.attack"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.attack.native"),
        },
        SlotDescriptor {
            key: key("creep.rangedAttack"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.rangedAttack.native"),
        },
        SlotDescriptor {
            key: key("creep.rangedMassAttack"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.rangedMassAttack.native"),
        },
        SlotDescriptor {
            key: key("creep.heal"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.heal.native"),
        },
        SlotDescriptor {
            key: key("creep.rangedHeal"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.rangedHeal.native"),
        },
        SlotDescriptor {
            key: key("creep.build"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.build.native"),
        },
        SlotDescriptor {
            key: key("creep.repair"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.repair.native"),
        },
        SlotDescriptor {
            key: key("creep.dismantle"),
            stage: Stage::ObjectAction,
            native_binding: key("creep.dismantle.native"),
        },
        SlotDescriptor {
            key: key("creep.receiveDamage"),
            stage: Stage::ObjectTick,
            native_binding: key("creep.receiveDamage.native"),
        },
        SlotDescriptor {
            key: key("room.moveContest"),
            stage: Stage::Movement,
            native_binding: key("room.moveContest.native"),
        },
        SlotDescriptor {
            key: key("creep.moveAdmission"),
            stage: Stage::Movement,
            native_binding: key("creep.moveAdmission.native"),
        },
        SlotDescriptor {
            key: key("room.moveObstacle"),
            stage: Stage::Movement,
            native_binding: key("room.moveObstacle.native"),
        },
        SlotDescriptor {
            key: key("room.movePrecheck"),
            stage: Stage::Movement,
            native_binding: key("room.movePrecheck.native"),
        },
        SlotDescriptor {
            key: key("room.movePullRegistration"),
            stage: Stage::Movement,
            native_binding: key("room.movePullRegistration.native"),
        },
        SlotDescriptor {
            key: key("room.moveCost"),
            stage: Stage::Movement,
            native_binding: key("room.moveCost.native"),
        },
    ];
    Contribution {
        source: Source {
            namespace: "builtin".into(),
            api_version: 1,
            revision:
                "link-creep-store-move-combat-build-incoming-contest-admission-obstacle-precheck-pull-cost-ranges-static-precheck-movegate-terrain-quota-v18"
                    .into(),
            code_digest: {
                let mut hash = Sha256::new();
                hash.update(include_bytes!("builtin.rs"));
                hash.update(include_bytes!("builtin/store.rs"));
                hash.finalize().into()
            },
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings,
        field_mounts: Vec::new(),
        object_types: vec![ObjectTypeDescriptor {
            key: key("link"),
            schema_version: 1,
            owner_field: Some(key("owner")),
            position_fields: Some([key("x"), key("y")]),
            fields,
            capabilities: vec![Capability {
                key: key("store"),
                consumers,
            }],
            lifecycle_slots: keys(&["link.tick"]),
            visibility: ObjectVisibility::RoomVisible,
            player_class: "StructureLink".into(),
        }],
        intents: vec![IntentDescriptor {
            key: key("link.transfer"),
            scope: IntentScope::Object,
            receiver: Some(TypeRef {
                key: key("link"),
                schema_version: 1,
            }),
            arguments: ValueSchema {
                kind: ValueKind::Record {
                    fields: BTreeMap::from([
                        (
                            "id".into(),
                            ValueSchema {
                                kind: ValueKind::ObjectRef(TypeRef {
                                    key: key("link"),
                                    schema_version: 1,
                                }),
                                nullable: false,
                            },
                        ),
                        ("amount".into(), signed(0, i64::MAX)),
                        (
                            "resourceType".into(),
                            ValueSchema {
                                nullable: true,
                                ..string(128)
                            },
                        ),
                    ]),
                    optional: ["resourceType".into()].into(),
                },
                nullable: false,
            },
            conversion: ArgumentConversion::Native(key("link.sanitize")),
            authority: IntentAuthority::Player,
            merge: Merge::Last,
            slot: key("link.transfer"),
            suppressed_by: BTreeSet::new(),
        }],
        rules,
        overrides: Vec::new(),
        constraints: [RuleConstraint::LessOrEqual {
            left: key("link.hits"),
            right: key("link.hitsMax"),
        }]
        .into(),
        slots,
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
    }
}
