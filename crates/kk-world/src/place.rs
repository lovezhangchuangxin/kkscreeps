//! 将嵌入式接口的放置 JSON 转换为房间对象。
//!
//! 坐标须在 0..=49 内。结构化对象将 TTL、强化和资源参数转换为引擎字段；
//! 其余对象使用 Generic 保存 JSON。这是构造测试场景的入口，不替代玩家动作校验。
//! 初始值参考：screeps-ok adapters/vanilla/index.ts 的各 place 方法。

use kk_core::constants as c;
use kk_schema::room::{
    BodyPart, ControllerObj, CreepObj, EnergyObj, FlagObj, GenericObj, ResourceStack, RoomObject,
    RuinObj, SpawnObj, Store, TombstoneObj,
};
use serde_json::{Value, json};

use crate::types::BodyKind;

mod place_structure;

pub use place_structure::{
    built_cooldown_time, built_decay_clock, built_store_capacity, built_store_capacity_resource,
    generic_structure_hits,
};
use place_structure::{normalize_site, normalize_structure};

/// 放置时的世界上下文，用于默认容量、到期时间与对象命名。
/// link 与 Generic 仓库的默认值取自冻结计划；调用方从引擎或内置计划提供。
pub struct PlaceCtx<'a> {
    pub time: u64,
    pub links: kk_mod_api::builtin::LinkRules,
    pub stores: kk_mod_api::builtin::StoreRules,
    /// 计划声明的建筑目录；Mod 建筑的放置默认值（耐久/容量/工地总进度）
    /// 按配方提供，无 Mod 世界为空目录。
    pub buildings: kk_mod_api::buildings::BuildingCatalog,
    /// 房内首个控制器；有无控制器和主人决定 source 默认容量。
    pub controller: Option<&'a ControllerObj>,
    /// 未指定 creep、spawn 名称时使用的序号；由调用方提供，须避免重名。
    pub name_seq: usize,
}

/// 内置贡献计划下的 link 默认规则；无自定义计划的调用方使用。
/// 冻结只发生一次，默认值与 kk-core 常量由 kk-mod-api 测试锁定一致。
pub fn builtin_link_rules() -> kk_mod_api::builtin::LinkRules {
    use std::sync::OnceLock;
    static RULES: OnceLock<kk_mod_api::builtin::LinkRules> = OnceLock::new();
    *RULES.get_or_init(|| {
        let plan = kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![],
            kk_mod_api::FreezeOptions::default(),
        )
        .expect("内置 link 贡献必须可冻结");
        kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).expect("内置计划包含全部 link 规则")
    })
}

/// 无自定义计划时的仓库容量快照；只在首次调用冻结默认计划。
pub fn builtin_store_rules() -> kk_mod_api::builtin::StoreRules {
    static RULES: std::sync::OnceLock<kk_mod_api::builtin::StoreRules> = std::sync::OnceLock::new();
    *RULES.get_or_init(|| {
        let plan = kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![],
            kk_mod_api::FreezeOptions::default(),
        )
        .expect("内置贡献必须可冻结");
        kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).expect("内置容量规则完整")
    })
}

/// 无自定义计划时的单目标战斗射程快照；与 link/仓库同批冻结默认计划。
pub fn builtin_combat_ranges() -> kk_mod_api::builtin::CombatRanges {
    static RULES: std::sync::OnceLock<kk_mod_api::builtin::CombatRanges> =
        std::sync::OnceLock::new();
    *RULES.get_or_init(|| {
        let plan = kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![],
            kk_mod_api::FreezeOptions::default(),
        )
        .expect("内置贡献必须可冻结");
        kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).expect("内置战斗射程规则完整")
    })
}

/// 按嵌入式接口的 kind 将 spec 转换为对象，产物 ID 为 0，等待 World 分配。
/// 坐标来自 `spec.pos=[x,y]`。未知 kind 进入 Generic，并保留 type=kind。
/// 参数验证按对象类型进行，不检查地形、重叠、配额或主人是否存在。
pub fn normalize(kind: &str, spec: &Value, ctx: &PlaceCtx) -> Result<RoomObject, String> {
    let (x, y) = read_pos(spec)?;
    match kind {
        "creep" => Ok(RoomObject::Creep(normalize_creep(spec, x, y, ctx)?)),
        "spawn" => Ok(RoomObject::Spawn(normalize_spawn(spec, x, y, ctx)?)),
        "structure" => normalize_structure(spec, x, y, ctx),
        "constructionSite" => Ok(RoomObject::ConstructionSite(normalize_site(
            spec,
            x,
            y,
            &ctx.links,
            &ctx.buildings,
        )?)),
        "flag" => Ok(RoomObject::Flag(normalize_flag(spec, x, y))),
        "resource" => Ok(RoomObject::Energy(normalize_resource(spec, x, y)?)),

        "tombstone" => Ok(RoomObject::Tombstone(normalize_tombstone(spec, x, y, ctx))),
        "ruin" => Ok(RoomObject::Ruin(normalize_ruin(spec, x, y, ctx))),
        // 两种入口共用结构分派，确保矿物与提取器使用相同默认值和校验。
        "mineral" | "extractor" => {
            let mut s = spec.clone();
            if let Some(m) = s.as_object_mut() {
                m.insert("structureType".into(), serde_json::json!(kind));
            }
            normalize_structure(&s, x, y, ctx)
        }
        _ => Ok(RoomObject::Generic(generic_from_spec(kind, spec, x, y))),
    }
}

/// Generic 复制除 pos 外的字段，再设置 type；ID 和坐标由投影时覆盖。
fn generic_fields(type_name: &str, spec: &Value) -> serde_json::Map<String, Value> {
    let mut data = serde_json::Map::new();
    if let Some(obj) = spec.as_object() {
        for (k, v) in obj {
            if k == "pos" {
                continue;
            }
            data.insert(k.clone(), v.clone());
        }
    }
    data.insert("type".into(), json!(type_name));
    data
}

fn generic_from_spec(type_name: &str, spec: &Value, x: u8, y: u8) -> GenericObj {
    GenericObj {
        id: 0,
        x,
        y,
        data: Value::Object(generic_fields(type_name, spec)).to_string(),
    }
}

fn read_pos(spec: &Value) -> Result<(u8, u8), String> {
    let pos = spec
        .get("pos")
        .and_then(Value::as_array)
        .ok_or_else(|| "spec 缺 pos 字段".to_string())?;
    let x = pos
        .first()
        .and_then(Value::as_i64)
        .ok_or_else(|| "pos[0] 缺失或非数值".to_string())?;
    let y = pos
        .get(1)
        .and_then(Value::as_i64)
        .ok_or_else(|| "pos[1] 缺失或非数值".to_string())?;
    if !(0..=49).contains(&x) || !(0..=49).contains(&y) {
        return Err(format!("坐标越界: ({x},{y})"));
    }
    Ok((x as u8, y as u8))
}

fn spec_str<'a>(spec: &'a Value, key: &str) -> Option<&'a str> {
    spec.get(key).and_then(Value::as_str)
}

fn store_energy(spec: &Value) -> Store {
    let (store, _) = store_with_extra(spec);
    store
}

/// 读取正整数资源存量，energy、G 使用专用字段，其余进入额外资源列表。
/// 超过 u32 的值饱和到 u32::MAX；负数、非整数和 0 被忽略。不校验资源名。
fn store_with_extra(spec: &Value) -> (Store, Vec<ResourceStack>) {
    let mut store = Store::default();
    let mut extra: Vec<ResourceStack> = Vec::new();
    let Some(map) = spec.get("store").and_then(Value::as_object) else {
        return (store, extra);
    };
    for (resource, value) in map {
        let amount = saturating_u32(value.as_u64().unwrap_or(0));
        if amount == 0 {
            continue;
        }
        match resource.as_str() {
            "energy" => store.energy = amount,
            kk_core::constants::RESOURCE_GHODIUM => store.ghodium = amount,
            other => extra.push(ResourceStack {
                resource: other.to_string(),
                amount,
            }),
        }
    }
    (store, extra)
}

fn store_extra_of(spec: &Value) -> Vec<ResourceStack> {
    store_with_extra(spec).1
}

/// 放置 creep：合并部件 boost，按显式 TTL 或默认寿命计算到期时间。
fn normalize_creep(spec: &Value, x: u8, y: u8, ctx: &PlaceCtx) -> Result<CreepObj, String> {
    let body_spec = spec
        .get("body")
        .and_then(Value::as_array)
        .ok_or_else(|| "creep 缺 body".to_string())?;
    let mut body: Vec<BodyPart> = Vec::with_capacity(body_spec.len());
    for part in body_spec {
        let type_name = part.as_str().unwrap_or_default();
        let kind =
            BodyKind::parse(type_name).ok_or_else(|| format!("非法身体部件: {type_name:?}"))?;
        body.push(BodyPart {
            kind: kind.code(),
            hits: 100,
            boost: None,
        });
    }
    // 放置参数按部件索引指定强化，产物将资源名保存在对应 BodyPart 中。
    if let Some(boosts) = spec.get("boosts").and_then(Value::as_object) {
        for (idx, res) in boosts {
            let i: usize = idx
                .parse()
                .map_err(|_| format!("boosts 索引非数值: {idx:?}"))?;
            let resource = res
                .as_str()
                .ok_or_else(|| format!("boosts 值非字符串: {res:?}"))?
                .to_string();
            body.get_mut(i)
                .ok_or_else(|| format!("boosts 索引越界: {i}"))?
                .boost = Some(resource);
        }
    }
    let name = spec_str(spec, "name")
        .map(str::to_string)
        .unwrap_or_else(|| format!("creep_{}", ctx.name_seq));
    let owner = spec_str(spec, "owner").unwrap_or_default().to_string();
    // 放置的默认寿命与 body 无关，显式 TTL 也不受 CLAIM 寿命上限约束。
    // 孵化和续命的规则由引擎另行处理。
    // 来源：screeps-ok adapters/vanilla/index.ts 的 placeCreep。
    let ttl = spec
        .get("ticksToLive")
        .and_then(Value::as_u64)
        .unwrap_or(c::CREEP_LIFE_TIME as u64);
    let hits = (body.len().saturating_mul(100)).min(u32::MAX as usize) as u32;
    Ok(CreepObj {
        id: 0,
        x,
        y,
        fatigue: 0,
        age_time: ctx.time.saturating_add(ttl),
        hits,
        spawning: false,
        action_log: 0,
        name,
        owner,
        body,
        store: store_energy(spec),
        store_extra: store_extra_of(spec),
        say_message: None,
        say_public: None,
        notify_when_attacked: None,
    })
}

/// 构造 spawn，未指定名称时使用调用方的 name_seq。
fn normalize_spawn(spec: &Value, x: u8, y: u8, ctx: &PlaceCtx) -> Result<SpawnObj, String> {
    let name = spec_str(spec, "name")
        .map(str::to_string)
        .unwrap_or_else(|| format!("Spawn_{}", ctx.name_seq));
    let owner = spec_str(spec, "owner").unwrap_or_default().to_string();
    let hits = spec
        .get("hits")
        .and_then(Value::as_u64)
        .unwrap_or(c::SPAWN_HITS as u64) as u32;
    // 不按容量限制能量；只有整个 store 字段缺失时才给初始能量。
    // 显式空对象或 null 都表示 0。来源：vanilla index.ts 的 placeStructure。
    let store = Store {
        energy: match spec.get("store") {
            Some(v) => saturating_u32(v.get("energy").and_then(Value::as_u64).unwrap_or(0)),
            None => c::SPAWN_ENERGY_START as u32,
        },
        ghodium: saturating_u32(
            spec.pointer("/store/G")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
    };
    Ok(SpawnObj {
        id: 0,
        x,
        y,
        hits,
        off: false,
        name,
        owner,
        store,
        spawning: None,
        // 默认打开攻击通知，避免 notifyWhenAttacked(true) 再产生无意义的变更指令。
        // 来源：screeps-ok adapters/vanilla/index.ts 的 getStructureDefaults。
        notify_when_attacked: Some(
            spec.get("notifyWhenAttacked")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        ),
    })
}

/// 构造旗，color 默认 1，secondaryColor 默认跟随 color。
fn normalize_flag(spec: &Value, x: u8, y: u8) -> FlagObj {
    let color = spec
        .get("color")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .min(u8::MAX as u64) as u8;
    FlagObj {
        id: 0,
        x,
        y,
        name: spec_str(spec, "name").unwrap_or_default().to_string(),
        color,
        secondary_color: spec
            .get("secondaryColor")
            .and_then(Value::as_u64)
            .unwrap_or(color as u64)
            .min(u8::MAX as u64) as u8,
        // 无主旗仅在可见房间导出，不授予房间视野。
        owner: spec_str(spec, "owner").map(str::to_string),
    }
}

/// 构造掉落资源，只接受 RESOURCES_ALL 中的资源名。
/// energy、power、G 使用对应码值；其他资源同时保存名称供投影与结算读取。
fn normalize_resource(spec: &Value, x: u8, y: u8) -> Result<EnergyObj, String> {
    let resource = match spec_str(spec, "resourceType") {
        None | Some("energy") => None,
        Some("power") => {
            return Ok(EnergyObj {
                id: 0,
                x,
                y,
                resource_type: 1,
                amount: saturating_u32(spec.get("amount").and_then(Value::as_u64).unwrap_or(0)),
                resource: None,
            });
        }
        Some(name) => {
            if !c::RESOURCES_ALL.contains(&name) {
                return Err(format!("未知掉落资源类型: {name:?}"));
            }
            Some(name.to_string())
        }
    };
    let code = match resource.as_deref() {
        Some(kk_core::constants::RESOURCE_GHODIUM) => 2,
        _ => 0,
    };
    Ok(EnergyObj {
        id: 0,
        x,
        y,
        resource_type: code,
        amount: saturating_u32(spec.get("amount").and_then(Value::as_u64).unwrap_or(0)),
        resource,
    })
}

/// 构造墓碑，deathTime 默认当前 tick，decay_time 为 deathTime+ticksToDecay。
/// 衰减间隔默认 RUIN_DECAY（500），不是根据 body 计算的自然死亡衰减时间。
/// 无法解析的 creepId 设为 None，无法识别的 creepBody 部件被忽略。
/// 初始值参考：screeps-ok adapters/vanilla/index.ts 的 placeTombstone。
fn normalize_tombstone(spec: &Value, x: u8, y: u8, ctx: &PlaceCtx) -> TombstoneObj {
    let death_time = spec
        .get("deathTime")
        .and_then(Value::as_u64)
        .unwrap_or(ctx.time);
    let ticks_to_decay = spec
        .get("ticksToDecay")
        .and_then(Value::as_u64)
        .unwrap_or(c::RUIN_DECAY as u64);
    let (store, store_extra) = store_with_extra(spec);
    TombstoneObj {
        id: 0,
        x,
        y,
        death_time,
        decay_time: death_time.saturating_add(ticks_to_decay),
        creep_id: spec
            .get("creepId")
            .and_then(Value::as_str)
            .and_then(crate::world::ObjId::parse)
            .map(|id| id.0),
        creep_name: spec_str(spec, "creepName").map(str::to_string),
        creep_ticks_to_live: saturating_u32(
            spec.get("creepTicksToLive")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
        creep_body: spec
            .get("creepBody")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.as_str().and_then(BodyKind::parse))
                    .map(|k| k.code())
                    .collect()
            })
            .unwrap_or_default(),
        creep_saying: spec_str(spec, "creepSaying").map(str::to_string),
        owner: spec
            .get("user")
            .or_else(|| spec.get("owner"))
            .and_then(Value::as_str)
            .map(str::to_string),
        store,
        store_extra,
    }
}

/// 构造废墟，destroyTime 默认当前 tick，衰减间隔默认 RUIN_DECAY（500）。
/// 未指定或无法解析的 structureId 使用 0，未指定结构最大 hits 保持 None。
/// 这是放置场景的初始值；正常毁坏产生废墟时由引擎填入原结构信息。
fn normalize_ruin(spec: &Value, x: u8, y: u8, ctx: &PlaceCtx) -> RuinObj {
    let destroy_time = spec
        .get("destroyTime")
        .and_then(Value::as_u64)
        .unwrap_or(ctx.time);
    let ticks_to_decay = spec
        .get("ticksToDecay")
        .and_then(Value::as_u64)
        .unwrap_or(c::RUIN_DECAY as u64);
    let (store, store_extra) = store_with_extra(spec);
    RuinObj {
        id: 0,
        x,
        y,
        structure_id: spec
            .get("structureId")
            .and_then(Value::as_str)
            .and_then(crate::world::ObjId::parse)
            .map(|id| id.0)
            .unwrap_or(0),
        structure_type: spec_str(spec, "structureType")
            .or_else(|| spec_str(spec, "type"))
            .unwrap_or_default()
            .to_string(),
        structure_hits_max: spec
            .get("structureHitsMax")
            .and_then(Value::as_u64)
            .map(saturating_u32),
        destroy_time,
        decay_time: destroy_time.saturating_add(ticks_to_decay),
        owner: spec
            .get("structureOwner")
            .or_else(|| spec.get("owner"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        store,
        store_extra,
    }
}

/// 超过 u32 范围时取 u32::MAX，避免强制转换导致回绕。
fn saturating_u32(v: u64) -> u32 {
    v.min(u32::MAX as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_schema::room::{Reservation, Sign};

    fn ctx() -> PlaceCtx<'static> {
        PlaceCtx {
            time: 100,
            links: builtin_link_rules(),
            stores: builtin_store_rules(),
            buildings: Default::default(),
            controller: None,
            name_seq: 7,
        }
    }

    fn spec(v: Value) -> Value {
        v
    }

    #[test]
    fn creep_defaults_match_vanilla() {
        let s = spec(json!({
            "pos": [10, 11], "owner": "p1",
            "body": ["work", "carry", "move"],
            "boosts": { "0": "XUHO2" },
            "store": { "energy": 30 },
            "ticksToLive": 500,
        }));
        let RoomObject::Creep(c) = normalize("creep", &s, &ctx()).unwrap() else {
            panic!("应产出 creep");
        };
        assert_eq!((c.x, c.y), (10, 11));
        assert_eq!(c.body.len(), 3);
        assert_eq!(c.body[0].boost.as_deref(), Some("XUHO2"));
        assert_eq!(c.body[1].boost, None);

        assert_eq!(c.hits, 300);
        assert_eq!(c.age_time, 600);
        assert_eq!(c.store.energy, 30);
        assert_eq!(c.fatigue, 0);
        assert!(!c.spawning);
        // 无名稳定名
        assert_eq!(c.name, "creep_7");
    }

    /// 放置 CLAIM creep 时，显式 TTL 不受正常孵化寿命上限限制；默认仍为 1500。
    #[test]
    fn claim_body_ttl_follows_vanilla_placement() {
        let s = spec(json!({
            "pos": [0, 0], "owner": "p1", "body": ["claim"], "ticksToLive": 5000,
        }));
        let RoomObject::Creep(c) = normalize("creep", &s, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(c.age_time, 100 + 5000);
        // 缺省恒 1500（与 body 无关）
        let s2 = spec(json!({ "pos": [0, 0], "owner": "p1", "body": ["claim"] }));
        let RoomObject::Creep(c2) = normalize("creep", &s2, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(c2.age_time, 100 + c::CREEP_LIFE_TIME as u64);
    }

    #[test]
    fn source_capacity_by_controller_state() {
        // 无 controller → keeper 4000
        let s = spec(json!({ "pos": [5, 5], "structureType": "source" }));
        let RoomObject::Source(src) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(src.energy_capacity, 4000);
        assert_eq!(src.energy, 4000);
        assert_eq!(src.next_regeneration_time, None);
        // 归属 controller → 3000
        let ctl = ControllerObj {
            id: 0,
            x: 1,
            y: 1,
            level: 3,
            safe_mode: 0,
            safe_mode_available: 1,
            progress: 0,
            downgrade_time: None,
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        };
        let c2 = PlaceCtx {
            controller: Some(&ctl),
            ..ctx()
        };
        let RoomObject::Source(src2) = normalize("structure", &s, &c2).unwrap() else {
            panic!()
        };
        assert_eq!(src2.energy_capacity, 3000);
        // 未满挂 300 tick 重生钟（vanilla placeSource）
        let s3 = spec(json!({ "pos": [5, 5], "structureType": "source", "energy": 100 }));
        let RoomObject::Source(src3) = normalize("structure", &s3, &c2).unwrap() else {
            panic!()
        };
        assert_eq!(src3.next_regeneration_time, Some(100 + 300));
        // spec.energyCapacity 覆盖
        let s4 = spec(json!({ "pos": [5, 5], "structureType": "source", "energyCapacity": 1500 }));
        let RoomObject::Source(src4) = normalize("structure", &s4, &c2).unwrap() else {
            panic!()
        };
        assert_eq!(src4.energy_capacity, 1500);
    }

    #[test]
    fn structure_defaults() {
        // spawn：无名稳定名、能量起点、hits
        let s = spec(json!({ "pos": [25, 25], "owner": "p1" }));
        let RoomObject::Spawn(sp) = normalize("spawn", &s, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(sp.name, "Spawn_7");
        assert_eq!(sp.store.energy, c::SPAWN_ENERGY_START as u32);
        assert_eq!(sp.hits, c::SPAWN_HITS as u32);
        assert!(sp.spawning.is_none());
        // 墙：hits=1（可修到 300M）
        let s = spec(json!({ "pos": [1, 1], "structureType": "constructedWall" }));
        let RoomObject::ConstructedWall(w) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(w.hits, 1);
        // 工地：progressTotal 派生 + progress 缺省 0
        let s = spec(json!({ "pos": [2, 2], "structureType": "extension", "owner": "p1" }));
        let RoomObject::ConstructionSite(site) = normalize("constructionSite", &s, &ctx()).unwrap()
        else {
            panic!()
        };
        assert_eq!(site.structure_type, "extension");
        assert_eq!(site.progress, 0);
        // flag：color 缺省 1
        let s = spec(json!({ "pos": [3, 3], "name": "f1" }));
        let RoomObject::Flag(f) = normalize("flag", &s, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!((f.color, f.secondary_color), (1, 1));
        // resource
        let s = spec(json!({ "pos": [4, 4], "amount": 55 }));
        let RoomObject::Energy(e) = normalize("resource", &s, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(e.amount, 55);
    }

    #[test]
    fn oversized_store_energy_saturates_instead_of_wrapping() {
        let s = json!({
            "pos": [10, 10], "body": ["carry"],
            "store": { "energy": u32::MAX as u64 + 1 },
        });
        let RoomObject::Creep(creep) = normalize("creep", &s, &ctx()).unwrap() else {
            panic!("应产出 creep");
        };
        assert_eq!(creep.store.energy, u32::MAX);
    }

    /// structureType=spawn 须产生 Spawn 变体，使玩家集合和类型判断识别为 spawn。
    #[test]
    fn spawn_via_structure_channel_routes_to_spawn() {
        let s = spec(json!({
            "pos": [20, 20], "structureType": "spawn", "owner": "p1",
        }));
        let RoomObject::Spawn(sp) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!("structure+spawn 必须产出 typed Spawn，不能落 Generic")
        };
        assert_eq!(sp.owner, "p1");
        assert_eq!(sp.store.energy, c::SPAWN_ENERGY_START as u32);
        // 显式 store 原样（不钳）
        let s2 = spec(json!({
            "pos": [20, 20], "structureType": "spawn", "owner": "p1",
            "store": { "energy": 5000 },
        }));
        let RoomObject::Spawn(sp2) = normalize("structure", &s2, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(sp2.store.energy, 5000);
        // 空 store → 0（vanilla 语义：有 store 原样）
        let s3 = spec(json!({
            "pos": [20, 20], "structureType": "spawn", "owner": "p1", "store": {},
        }));
        let RoomObject::Spawn(sp3) = normalize("structure", &s3, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(sp3.store.energy, 0);
    }

    /// 控制器接收绝对计时和完整的签名、预订记录；creep 将 G 保存到专用资源字段。
    #[test]
    fn controller_b2_fields_and_ghodium_passthrough() {
        let s = spec(json!({
            "pos": [9, 9], "structureType": "controller", "level": 4, "owner": "p1",
            "upgradeBlocked": 1200,
            "safeModeCooldown": 50100,
            "reservation": { "user": "p2", "endTime": 4321 },
            "sign": { "user": "p1", "text": "hi", "time": 77 }
        }));
        let RoomObject::Controller(c) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!("应产出 controller");
        };
        assert_eq!(c.upgrade_blocked, Some(1200));
        assert_eq!(c.safe_mode_cooldown, Some(50_100));
        assert_eq!(
            c.reservation,
            Some(Reservation {
                user: "p2".into(),
                end_time: 4321,
            })
        );
        assert_eq!(
            c.sign,
            Some(Sign {
                user: "p1".into(),
                text: "hi".into(),
                time: 77,
                datetime: None,
            })
        );
        // 缺省形态：四新字段全 None
        let s2 = spec(json!({ "pos": [9, 9], "structureType": "controller" }));
        let RoomObject::Controller(c2) = normalize("structure", &s2, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(
            (
                c2.reservation,
                c2.sign,
                c2.upgrade_blocked,
                c2.safe_mode_cooldown
            ),
            (None, None, None, None)
        );
        // 畸形嵌套对象（缺键）→ None（不部分落地）
        let s3 = spec(json!({
            "pos": [9, 9], "structureType": "controller",
            "reservation": { "user": "p2" }
        }));
        let RoomObject::Controller(c3) = normalize("structure", &s3, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(c3.reservation, None, "缺 endTime 的预约整票丢弃");
        // creep store 的 ghodium 槽（官方键 "G"）
        let s4 = spec(json!({
            "pos": [5, 5], "owner": "p1", "body": ["move"],
            "store": { "energy": 30, "G": 1200 }
        }));
        let RoomObject::Creep(cr) = normalize("creep", &s4, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(cr.store.ghodium, 1200);
    }

    /// 墓碑、废墟采用默认衰减时间，并保存专用字段之外的资源。
    #[test]
    fn tombstone_and_ruin_placement_defaults() {
        let s = spec(json!({
            "pos": [8, 8],
            "creepName": "w1",
            "store": { "energy": 30, "XUHO2": 6 },
            "ticksToDecay": 100,
        }));
        let RoomObject::Tombstone(t) = normalize("tombstone", &s, &ctx()).unwrap() else {
            panic!("tombstone 直通道进 typed 变体");
        };
        assert_eq!(t.death_time, 100, "缺省 deathTime = gameTime");
        assert_eq!(t.decay_time, 200, "deathTime + ticksToDecay");
        assert_eq!(t.creep_name.as_deref(), Some("w1"));
        assert_eq!(t.creep_id, None, "vanilla 缺省 null");
        assert_eq!(t.creep_ticks_to_live, 0);
        assert_eq!(t.creep_body, Vec::<u8>::new());
        assert_eq!(t.owner, None, "vanilla 放置不落 user");
        assert_eq!(t.store.energy, 30);
        assert_eq!(
            t.store_extra,
            vec![ResourceStack {
                resource: "XUHO2".into(),
                amount: 6,
            }],
            "额外资源进列表槽"
        );
        // 全缺省形态
        let s2 = spec(json!({ "pos": [9, 9], "creepName": "x" }));
        let RoomObject::Tombstone(t2) = normalize("tombstone", &s2, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(t2.decay_time, 100 + 500, "ticksToDecay 缺省 500（vanilla）");

        let r = spec(json!({
            "pos": [10, 10], "structureType": "spawn", "structureOwner": "p1",
            "structureId": "kk0000000000000004", "structureHitsMax": 5000,
            "destroyTime": 90, "ticksToDecay": 50,
            "store": { "energy": 300 },
        }));
        let RoomObject::Ruin(ru) = normalize("ruin", &r, &ctx()).unwrap() else {
            panic!("ruin 直通道进 typed 变体");
        };
        assert_eq!(ru.structure_type, "spawn");
        assert_eq!(ru.structure_id, 4, "kk id 字符串解析");
        assert_eq!(ru.structure_hits_max, Some(5000));
        assert_eq!(ru.owner.as_deref(), Some("p1"));
        assert_eq!(ru.destroy_time, 90);
        assert_eq!(ru.decay_time, 140);
        assert_eq!(ru.store.energy, 300);
        // structureOwner 缺省（vanilla 无 user 键）
        let r2 = spec(json!({ "pos": [11, 11], "structureType": "wall" }));
        let RoomObject::Ruin(ru2) = normalize("ruin", &r2, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(ru2.owner, None);
        assert_eq!(ru2.decay_time, 100 + 500);
    }

    /// 检查矿物默认密度、存量和相对再生时间，以及提取器的默认值与冷却计数。
    #[test]
    fn mineral_and_extractor_placement_defaults() {
        // mineral：density 缺省 DENSITY_HIGH(3)、amount 缺省 MINERAL_DENSITY[3]
        let s = spec(json!({
            "pos": [10, 10], "structureType": "mineral", "mineralType": "X",
        }));
        let RoomObject::Mineral(m) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!("structure 通道的 mineral 进 typed 变体");
        };
        assert_eq!(m.mineral_type, "X");
        assert_eq!(m.density, 3);
        assert_eq!(m.mineral_amount, 70_000, "MINERAL_DENSITY[DENSITY_HIGH]");
        assert_eq!(m.next_regeneration_time, None);
        // 显式 density/amount + ticksToRegeneration 绝对钟（vanilla 只判
        // !== undefined——无 source 的 >0 过滤）
        let s2 = spec(json!({
            "pos": [10, 10], "structureType": "mineral", "mineralType": "U",
            "density": 1, "mineralAmount": 123, "ticksToRegeneration": 45,
        }));
        let RoomObject::Mineral(m2) = normalize("structure", &s2, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(m2.density, 1);
        assert_eq!(m2.mineral_amount, 123);
        assert_eq!(
            m2.next_regeneration_time,
            Some(100 + 45),
            "gameTime + ticks"
        );
        // 缺 mineralType 拒（vanilla 的 MineralSpec 必填字段）
        let bad = spec(json!({ "pos": [10, 10], "structureType": "mineral" }));
        assert!(normalize("structure", &bad, &ctx()).is_err());

        // extractor：hits 缺省 EXTRACTOR_HITS、cooldown 透传、owner 可缺省
        let e = spec(json!({
            "pos": [10, 10], "structureType": "extractor", "owner": "p1", "cooldown": 4,
        }));
        let RoomObject::Extractor(ex) = normalize("structure", &e, &ctx()).unwrap() else {
            panic!("structure 通道的 extractor 进 typed 变体");
        };
        assert_eq!(ex.hits, c::EXTRACTOR_HITS as u32);
        assert_eq!(ex.owner.as_deref(), Some("p1"));
        assert_eq!(ex.cooldown, 4);
        let e2 = spec(json!({ "pos": [10, 10], "structureType": "extractor", "hits": 111 }));
        let RoomObject::Extractor(ex2) = normalize("structure", &e2, &ctx()).unwrap() else {
            panic!()
        };
        assert_eq!(ex2.hits, 111);
        assert_eq!(ex2.owner, None, "无主 extractor（harvest 无主放行分支）");
        assert_eq!(ex2.cooldown, 0);
    }

    #[test]
    fn unknown_types_go_generic_passthrough() {
        // tower 使用 Generic 保存数据，仍可被结构查询和结算识别。
        let s = spec(json!({
            "pos": [7, 7], "structureType": "tower", "owner": "p1", "hits": 999,
        }));
        let RoomObject::Generic(g) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!()
        };
        let data: Value = serde_json::from_str(&g.data).unwrap();
        assert_eq!(data["structureType"], "tower");
        assert_eq!(data["hits"], 999);
        // 非法输入拒绝
        let bad = spec(json!({ "pos": [50, 0] }));
        assert!(normalize("creep", &bad, &ctx()).is_err());
        // 未知类别保存 type 和原始字段，供投影及后续能力使用。
        let s2 = spec(json!({ "pos": [1, 1], "decayTime": 88 }));
        let RoomObject::Generic(g2) = normalize("deposit", &s2, &ctx()).unwrap() else {
            panic!()
        };
        let d2: Value = serde_json::from_str(&g2.data).unwrap();
        assert_eq!(d2["type"], "deposit");
        assert_eq!(d2["decayTime"], 88);
        // structure 通道的 tower 透传也带 type
        let RoomObject::Generic(g3) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!()
        };
        let d3: Value = serde_json::from_str(&g3.data).unwrap();
        assert_eq!(d3["type"], "structure");
        assert_eq!(d3["structureType"], "tower");
    }
}
