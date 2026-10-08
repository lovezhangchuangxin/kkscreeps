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

/// 无自定义计划时的移动玩家门快照；与 link/仓库同批冻结默认计划。
pub fn builtin_move_gate() -> kk_mod_api::builtin::MoveGate {
    static GATE: std::sync::OnceLock<kk_mod_api::builtin::MoveGate> = std::sync::OnceLock::new();
    *GATE.get_or_init(|| {
        let plan = kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![],
            kk_mod_api::FreezeOptions::default(),
        )
        .expect("内置贡献必须可冻结");
        kk_mod_api::builtin::MoveGate::from_plan(plan.rules()).expect("内置移动门规则完整")
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
        // nuke 以 landTime 为权威键：适配器 spec 给相对 timeToLand，
        // 放置不推进时间，此处按放置时刻换算（适配器契约：放置后一次
        // tick 读 timeToLand 恰好减一）。launchRoomName 原样保留。
        "nuke" => Ok(RoomObject::Generic(normalize_nuke(spec, x, y, ctx))),
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

/// 把 timeToLand 换算为 landTime 并丢弃相对键；timeToLand 缺失或非法
/// 时不写 landTime（该 nuke 永不落地，与官方不可达形态一致）。
fn normalize_nuke(spec: &Value, x: u8, y: u8, ctx: &PlaceCtx) -> GenericObj {
    let mut data = generic_fields("nuke", spec);
    if let Some(ttl) = data.remove("timeToLand").and_then(|v| v.as_i64()) {
        data.insert("landTime".into(), json!(ctx.time + ttl.max(0) as u64));
    }
    GenericObj {
        id: 0,
        x,
        y,
        data: Value::Object(data).to_string(),
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
mod tests;
