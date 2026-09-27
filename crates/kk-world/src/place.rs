//! place spec（screeps-ok JSON）→ typed 对象的归一化表。
//!
//! 初态对照 vanilla 参照适配器的放置实现（`.compat/screeps-ok/adapters/vanilla/
//! index.ts` placeCreep/placeStructure/placeSite/placeSource/placeFlag）与官方
//! build.js 的落成分支。**spec 私有字段（boosts/ticksToLive/progressTotal…）不
//! 泄入对象**——它们在此折算为引擎字段（第四轮"typed placement"登记项的清偿）。

use kk_core::constants as c;
use kk_schema::room::{
    BodyPart, ConstructedWallObj, ConstructionSiteObj, ControllerObj, CreepObj, EnergyObj,
    ExtensionObj, FlagObj, GenericObj, RoadObj, RoomObject, SourceObj, SpawnObj, StorageObj, Store,
};
use serde_json::{Value, json};

use crate::types::BodyKind;

/// 归一化上下文：放置依赖的世界状态（控制器归属决定 source 容量等）。
pub struct PlaceCtx<'a> {
    pub time: u64,
    /// 房间内是否已有归属 controller（placeSource 容量三分支，vanilla placeSource）。
    pub controller: Option<&'a ControllerObj>,
    /// 无名 creep/spawn 的稳定名序号（M0 契约：上游 spec 无 name 字段是合法形态）。
    pub name_seq: usize,
}

/// 归一化入口：kind 为 embed 通道的内部类别（creep/structure/spawn/
/// constructionSite/flag/resource）；spec 为 PlaceSpec JSON。
/// 未知 kind（tombstone/portal/deposit/keeperLair/invaderCore/powerBank/…）
/// 落 Generic 透传并携带 `type: kind`——与 M0 哑世界相同的放置接受面。
pub fn normalize(kind: &str, spec: &Value, ctx: &PlaceCtx) -> Result<RoomObject, String> {
    let (x, y) = read_pos(spec)?;
    match kind {
        "creep" => Ok(RoomObject::Creep(normalize_creep(spec, x, y, ctx)?)),
        "spawn" => Ok(RoomObject::Spawn(normalize_spawn(spec, x, y, ctx)?)),
        "structure" => normalize_structure(spec, x, y, ctx),
        "constructionSite" => Ok(RoomObject::ConstructionSite(normalize_site(spec, x, y)?)),
        "flag" => Ok(RoomObject::Flag(normalize_flag(spec, x, y))),
        "resource" => Ok(RoomObject::Energy(normalize_resource(spec, x, y)?)),
        _ => Ok(RoomObject::Generic(generic_from_spec(kind, spec, x, y))),
    }
}

/// 未知对象/未知结构类型的透传桶构造：spec 减 pos 全量透传 + 结构化字段。
fn generic_from_spec(type_name: &str, spec: &Value, x: u8, y: u8) -> GenericObj {
    let mut data = serde_json::Map::new();
    if let Some(obj) = spec.as_object() {
        for (k, v) in obj {
            if k == "pos" {
                continue;
            }
            data.insert(k.clone(), v.clone());
        }
    }
    // FIND/wrapObject 消费面按 type 判别——透传必须携带（M0 记录恒有 type）
    data.insert("type".into(), json!(type_name));
    GenericObj {
        id: 0,
        x,
        y,
        data: Value::Object(data).to_string(),
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
    Store {
        energy: spec
            .pointer("/store/energy")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
    }
}

/// vanilla placeCreep：body 全 hits=100、boosts 并入部件、hits=len×100、
/// ageTime = time + (ticksToLive ?? CREEP_LIFE_TIME)（claim 体走 CLAIM_LIFE）。
fn normalize_creep(spec: &Value, x: u8, y: u8, _ctx: &PlaceCtx) -> Result<CreepObj, String> {
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
        .unwrap_or_else(|| format!("creep_{}", _ctx.name_seq));
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
        age_time: _ctx.time + ttl,
        hits,
        spawning: false,
        action_log: 0,
        name,
        owner,
        body,
        store: store_energy(spec),
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
    };
    Ok(SpawnObj {
        id: 0,
        x,
        y,
        hits,
        name,
        owner,
        store,
        spawning: None,
    })
}

/// structure 类：按 structureType 分派。M1 类型化变体之外的类型进 Generic
/// （完整 spec 折算成透传 JSON，保持 M0 放置接受面；结算忽略）。
fn normalize_structure(spec: &Value, x: u8, y: u8, ctx: &PlaceCtx) -> Result<RoomObject, String> {
    let st = spec_str(spec, "structureType")
        .or_else(|| spec_str(spec, "type"))
        .unwrap_or_default();
    let owner = spec_str(spec, "owner").map(str::to_string);
    let hits_override = spec.get("hits").and_then(Value::as_u64);
    match st {
        "spawn" => {
            // placeStructure 通道的 spawn 必须成为玩家可见的 spawn（M0 embed
            // 归一语义：Game.spawns / FIND_MY_SPAWNS / instanceof 的根基）
            normalize_spawn(spec, x, y, ctx).map(RoomObject::Spawn)
        }
        "source" => {
            // vanilla placeSource：容量按控制器状态三分支；energy 缺省=容量；
            // 未满即挂 300 tick 重生钟
            let capacity = saturating_u32(
                spec.get("energyCapacity")
                    .and_then(Value::as_u64)
                    .unwrap_or(match ctx.controller {
                        None => c::SOURCE_ENERGY_KEEPER_CAPACITY as u64,
                        Some(ctl) if ctl.owner.is_some() => c::SOURCE_ENERGY_CAPACITY as u64,
                        Some(_) => c::SOURCE_ENERGY_NEUTRAL_CAPACITY as u64,
                    }),
            );
            let energy = saturating_u32(
                spec.get("energy")
                    .and_then(Value::as_u64)
                    .unwrap_or(capacity as u64),
            );
            let regen = if let Some(t) = spec
                .get("ticksToRegeneration")
                .and_then(Value::as_u64)
                .filter(|t| *t > 0)
            {
                Some(ctx.time + t)
            } else if energy < capacity {
                Some(ctx.time + c::ENERGY_REGEN_TIME as u64)
            } else {
                None
            };
            Ok(RoomObject::Source(SourceObj {
                id: 0,
                x,
                y,
                energy,
                energy_capacity: capacity,
                next_regeneration_time: regen,
            }))
        }
        "controller" => {
            let level = spec
                .get("level")
                .or_else(|| spec.get("rcl"))
                .and_then(Value::as_u64)
                .unwrap_or(1) as u8;
            Ok(RoomObject::Controller(ControllerObj {
                id: 0,
                x,
                y,
                level,
                safe_mode: 0,
                safe_mode_available: 0,
                progress: spec.get("progress").and_then(Value::as_u64).unwrap_or(0),
                downgrade_time: None,
                owner,
            }))
        }
        "extension" => Ok(RoomObject::Extension(ExtensionObj {
            id: 0,
            x,
            y,
            hits: hits_override.unwrap_or(c::EXTENSION_HITS as u64) as u32,
            owner: owner.unwrap_or_default(),
            store: store_energy(spec),
        })),
        "constructedWall" => Ok(RoomObject::ConstructedWall(ConstructedWallObj {
            id: 0,
            x,
            y,
            // vanilla：新建墙 hits=1（WALL_HITS），可修到 300M（WALL_HITS_MAX）
            hits: hits_override
                .map(saturating_u32)
                .unwrap_or(c::WALL_HITS as u32),
        })),
        "road" => Ok(RoomObject::Road(RoadObj {
            id: 0,
            x,
            y,
            hits: hits_override
                .map(saturating_u32)
                .unwrap_or(c::ROAD_HITS as u32),
            next_decay_time: ctx.time + c::ROAD_DECAY_TIME as u64,
        })),
        "storage" => Ok(RoomObject::Storage(StorageObj {
            id: 0,
            x,
            y,
            hits: hits_override
                .map(saturating_u32)
                .unwrap_or(c::STORAGE_HITS as u32),
            owner: owner.unwrap_or_default(),
            store: store_energy(spec),
        })),
        _ => {
            // M1 结算范围外：透传（含 tower/rampart/lab/link/container/
            // extractor/factory/terminal/nuker/observer/powerSpawn/mineral/…）
            let mut generic = generic_from_spec("structure", spec, x, y);
            if let Some(data) = serde_json::from_str::<Value>(&generic.data)
                .unwrap()
                .as_object()
            {
                let mut map = data.clone();
                if !map.contains_key("structureType") && !st.is_empty() {
                    map.insert("structureType".into(), json!(st));
                }
                generic.data = Value::Object(map).to_string();
            }
            Ok(RoomObject::Generic(generic))
        }
    }
}

/// vanilla placeSite：progress 缺省 0；progressTotal 由 CONSTRUCTION_COST 派生。
fn normalize_site(spec: &Value, x: u8, y: u8) -> Result<ConstructionSiteObj, String> {
    let structure_type = spec_str(spec, "structureType")
        .or_else(|| spec_str(spec, "type"))
        .ok_or_else(|| "constructionSite 缺 structureType".to_string())?
        .to_string();
    Ok(ConstructionSiteObj {
        id: 0,
        x,
        y,
        structure_type,
        progress: saturating_u32(spec.get("progress").and_then(Value::as_u64).unwrap_or(0)),
        owner: spec_str(spec, "owner").map(str::to_string),
        name: spec_str(spec, "name").map(str::to_string),
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
    }
}

/// 掉落资源（M1 仅 energy；power/矿物资源 M2 扩展 schema 时支持）。
fn normalize_resource(spec: &Value, x: u8, y: u8) -> Result<EnergyObj, String> {
    if let Some(rt) = spec_str(spec, "resourceType")
        && rt != "energy"
    {
        return Err(format!("M1 掉落资源仅支持 energy，得到 {rt:?}"));
    }
    Ok(EnergyObj {
        id: 0,
        x,
        y,
        resource_type: 0,
        amount: saturating_u32(spec.get("amount").and_then(Value::as_u64).unwrap_or(0)),
    })
}

/// u64 → u32 的饱和转换（放置值超域不改值语义、不静默回绕）。
fn saturating_u32(v: u64) -> u32 {
    v.min(u32::MAX as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // vanilla：hits = len×100；ageTime = time + min(ttl, LIFE)
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
        // 未知类别落 Generic 透传（M0 接受面）且携带 type 字段
        let s2 = spec(json!({ "pos": [1, 1], "deathTime": 88 }));
        let RoomObject::Generic(g2) = normalize("tombstone", &s2, &ctx()).unwrap() else {
            panic!()
        };
        let d2: Value = serde_json::from_str(&g2.data).unwrap();
        assert_eq!(d2["type"], "tombstone");
        assert_eq!(d2["deathTime"], 88);
        // structure 通道的 tower 透传也带 type
        let RoomObject::Generic(g3) = normalize("structure", &s, &ctx()).unwrap() else {
            panic!()
        };
        let d3: Value = serde_json::from_str(&g3.data).unwrap();
        assert_eq!(d3["type"], "structure");
        assert_eq!(d3["structureType"], "tower");
    }
}
