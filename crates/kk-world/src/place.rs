//! place spec（screeps-ok JSON）→ typed 对象的归一化表。
//!
//! 初态对照 vanilla 参照适配器（screeps-ok `adapters/vanilla/index.ts` 的
//! placeCreep/placeStructure/placeSite/placeSource/placeFlag）与官方 build.js
//! 的落成分支。spec 私有字段（boosts/ticksToLive/progressTotal 等）不泄入
//! 对象——在此折算为引擎字段（M0 第四轮"typed placement"登记项的清偿）；
//! 结算范围外的类型落 Generic 透传桶，保持 FIND 可见性。

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

/// 归一化上下文：放置依赖的世界状态（控制器归属决定 source 容量等）。
pub struct PlaceCtx<'a> {
    pub time: u64,
    /// 房间内是否已有归属 controller（placeSource 容量三分支，vanilla placeSource）。
    pub controller: Option<&'a ControllerObj>,
    /// 无名 creep/spawn 的稳定名序号（M0 契约：上游 spec 无 name 字段是合法形态）。
    pub name_seq: usize,
}

/// 归一化入口：kind 为 embed 通道的内部类别（creep/structure/spawn/
/// constructionSite/flag/resource/tombstone/ruin）；spec 为 PlaceSpec JSON。
/// 未知 kind（portal/deposit/keeperLair/invaderCore/powerBank/…）落 Generic
/// 透传并携带 `type: kind`——与 M0 哑世界相同的放置接受面。
pub fn normalize(kind: &str, spec: &Value, ctx: &PlaceCtx) -> Result<RoomObject, String> {
    let (x, y) = read_pos(spec)?;
    match kind {
        "creep" => Ok(RoomObject::Creep(normalize_creep(spec, x, y, ctx)?)),
        "spawn" => Ok(RoomObject::Spawn(normalize_spawn(spec, x, y, ctx)?)),
        "structure" => normalize_structure(spec, x, y, ctx),
        "constructionSite" => Ok(RoomObject::ConstructionSite(normalize_site(spec, x, y)?)),
        "flag" => Ok(RoomObject::Flag(normalize_flag(spec, x, y))),
        "resource" => Ok(RoomObject::Energy(normalize_resource(spec, x, y)?)),
        // B3 起进 typed 变体（此前 Generic 透传）——放置面保持官方 DB 形态
        "tombstone" => Ok(RoomObject::Tombstone(normalize_tombstone(spec, x, y, ctx))),
        "ruin" => Ok(RoomObject::Ruin(normalize_ruin(spec, x, y, ctx))),
        // B4 直通道（B4 评审 3 P3-1 登记——typed 分支原只在 structure 通道，
        // 裸 kind 落 Generic 透传致 FIND 失配）：复用 normalize_structure 的
        // structureType 分派（白名单/缺省逻辑单源）
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

/// Generic 保留原始字段，但坐标和类型由引擎统一设置。
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

/// 读 spec 的 store 对象为双槽 + 额外资源列表（B3）：energy/G 进 typed 槽，
/// 其余正量资源（boost 矿物等）进 store_extra——官方 store 是按资源名键的
/// 开放对象（_die.js:78 逐键循环的全集形态）。
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

/// [`store_with_extra`] 的额外资源列表单取（B4：creep/storage 的命名槽——
/// energy/G 之外的正量资源，与 tombstone/ruin 同口径）。
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
    // spec.boosts：{部件序号: 资源名} 并入对应部件（不泄入对象）
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
    // vanilla placeCreep（index.ts:1087）：ageTime = gameTime + (ttl ?? LIFE)。
    // 无 claim 特判、无钳制——claim 区分寿命是引擎孵化路径（creeps/tick.js:87-93
    // 的 ageTime 缺失分支）与 renew 上限语义，放置路径不适用
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

/// vanilla placeStructure 的 spawn 分支 + M0 的无名稳定名。
fn normalize_spawn(spec: &Value, x: u8, y: u8, ctx: &PlaceCtx) -> Result<SpawnObj, String> {
    let name = spec_str(spec, "name")
        .map(str::to_string)
        .unwrap_or_else(|| format!("Spawn_{}", ctx.name_seq));
    let owner = spec_str(spec, "owner").unwrap_or_default().to_string();
    let hits = spec
        .get("hits")
        .and_then(Value::as_u64)
        .unwrap_or(c::SPAWN_HITS as u64) as u32;
    // vanilla：spec 带 store 则原样（不钳）；整体无 store 才用 SPAWN_ENERGY_START
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
        // 官方 getStructureDefaults('spawn') 缺省 notifyWhenAttacked:true
        //（vanilla index.ts:1919；引擎 build.js:116 对新建结构同）——显式
        // 给了才覆盖。缺省的后果：玩家对放置的 spawn 调
        // notifyWhenAttacked(true) 会**多记一条 intent**（官方 `true != true`
        // 不记，structures.js:1220 差分门）
        notify_when_attacked: Some(
            spec.get("notifyWhenAttacked")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        ),
    })
}

/// vanilla placeFlag：color 缺省 1、secondary 缺省 = color。
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
        // M2B 扩面 P2：owner（缺省 None=无主旗——投影按全局可见兜底）
        owner: spec_str(spec, "owner").map(str::to_string),
    }
}

/// 掉落资源（0=energy/1=power/2=G；B3 起任意 RESOURCES_ALL 资源名经命名槽
/// 承载——tombstone/ruin 衰减掉的 boost 矿物等）。
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
            // 未知资源名拒绝（RESOURCES_ALL 白名单——官方常量表外的名字
            // 无游戏语义）
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

/// vanilla placeTombstone（adapters/vanilla/index.ts:1263-1280）：deathTime
/// 缺省 gameTime、ticksToDecay 缺省 500（decayTime = deathTime + ticksToDecay）、
/// creepName/store 透传、creepBody 缺省 []、creepTicksToLive 缺省 0、creepId
/// 缺省 null（官方 DB 字段面见 _die.js:22-34）。
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

/// vanilla placeRuin（adapters/vanilla/index.ts:1286-1313）：structure
/// {id 缺省自分配、type、hits:0、hitsMax 缺省 0、user 缺省无}、destroyTime
/// 缺省 gameTime、ticksToDecay 缺省 500、store 缺省 {}。官方引擎侧创建路径
/// 见 structures/_destroy.js:22-40。
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

/// u64 → u32 的饱和转换（放置值超域不改值语义、不静默回绕）。
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
        // hits 与 TTL 使用放置 spec 的值。
        assert_eq!(c.hits, 300);
        assert_eq!(c.age_time, 600);
        assert_eq!(c.store.energy, 30);
        assert_eq!(c.fatigue, 0);
        assert!(!c.spawning);
        // 无名稳定名
        assert_eq!(c.name, "creep_7");
        // spec 私有字段不泄入：typed 对象无 ticksToLive/boosts 字段（编译期保证）
    }

    /// vanilla placeCreep 对放置体不按 claim 区分寿命、不钳制显式 ttl
    /// （index.ts:1087）；claim 寿命区分是引擎孵化路径的 ageTime 缺失分支。
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

    /// placeStructure 通道（kind=structure）的 spawn 必须归一为 typed Spawn
    /// （M0 embed 归一语义——Game.spawns/FIND_MY_SPAWNS/instanceof 的根基）。
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

    /// B2：controller 持久化面透传（官方 DB 字段名 camelCase）+ store 的
    /// ghodium 槽（官方 store 键 RESOURCE_GHODIUM = "G"）。
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

    /// B3：tombstone/ruin 进 typed 变体（vanilla placeTombstone index.ts:1263-1280 /
    /// placeRuin :1286-1313 的缺省面）——deathTime/destroyTime 缺省当前 tick、
    /// ticksToDecay 缺省 500、store 的额外资源进 store_extra。
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

    /// B4：mineral/extractor 的放置缺省面（vanilla placeMineral index.ts:1209-1221 /
    /// placeStructure 的 extractor cooldown 计数器形态 :1147-1151）。
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
        // tower：M1 无结算变体 → Generic 透传（保持放置接受面）
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
        // 未知类别落 Generic 透传（M0 接受面）且携带 type 字段（B3 起
        // tombstone/ruin 已进 typed 变体，用 deposit 示例）
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
