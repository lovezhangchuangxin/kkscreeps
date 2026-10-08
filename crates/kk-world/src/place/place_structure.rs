//! 将 structure、constructionSite 放置参数转换为对象。
//! 同时提供引擎建造完成时共用的结构 hits、容量及计时默认值。

use kk_core::constants as c;
use kk_mod_api::builtin::{LinkRules, StoreRules};
use kk_schema::room::{
    ConstructedWallObj, ConstructionSiteObj, ControllerObj, ExtensionObj, ExtractorObj, GenericObj,
    MineralObj, Reservation, RoadObj, RoomObject, Sign, SourceObj, StorageObj,
};
use serde_json::{Value, json};

use super::PlaceCtx;
use super::generic_fields;
use super::{normalize_spawn, saturating_u32, spec_str, store_energy, store_extra_of};

/// 按 structureType（缺失时用 type）选择对象变体。
/// 没有专用变体时使用 Generic；它仍可由引擎按类型处理，并非一律忽略结算。
pub(super) fn normalize_structure(
    spec: &Value,
    x: u8,
    y: u8,
    ctx: &PlaceCtx,
) -> Result<RoomObject, String> {
    let st = spec_str(spec, "structureType")
        .or_else(|| spec_str(spec, "type"))
        .unwrap_or_default();
    let owner = spec_str(spec, "owner").map(str::to_string);
    let hits_override = spec.get("hits").and_then(Value::as_u64);
    match st {
        "spawn" => {
            // spawn 须使用专用变体，否则玩家的 spawn 集合与 API 无法识别。
            normalize_spawn(spec, x, y, ctx).map(RoomObject::Spawn)
        }
        "source" => {
            // 容量默认取决于控制器是否存在及是否有主人；能量默认等于容量。
            // 显式正再生时间优先，否则未满源设置默认再生时间。
            // 来源：screeps-ok adapters/vanilla/index.ts 的 placeSource。
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
            // 等级默认 0；createShard 会显式传入占有房间的初始等级。
            // safeMode、downgradeTime 接收绝对 tick，不在这里加当前时间。
            let level = spec
                .get("level")
                .or_else(|| spec.get("rcl"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as u8;
            Ok(RoomObject::Controller(ControllerObj {
                id: 0,
                x,
                y,
                level,
                safe_mode: spec.get("safeMode").and_then(Value::as_u64).unwrap_or(0),
                safe_mode_available: saturating_u32(
                    spec.get("safeModeAvailable")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                ),
                progress: spec.get("progress").and_then(Value::as_u64).unwrap_or(0),
                downgrade_time: spec.get("downgradeTime").and_then(Value::as_u64),
                owner,
                // 放置没有“归属被清除”的标记；该状态由引擎降级处理设置。
                owner_cleared: false,
                is_power_enabled: spec
                    .get("isPowerEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                // 嵌套预订与签名缺少必填字段时整体忽略，计时字段保留绝对值。
                reservation: spec.get("reservation").and_then(|r| {
                    Some(Reservation {
                        user: r.get("user")?.as_str()?.to_string(),
                        end_time: r.get("endTime")?.as_u64()?,
                    })
                }),
                sign: spec.get("sign").and_then(|s| {
                    Some(Sign {
                        user: s.get("user")?.as_str()?.to_string(),
                        text: s.get("text")?.as_str()?.to_string(),
                        time: s.get("time")?.as_u64()?,
                        datetime: s.get("datetime").and_then(|d| d.as_u64()),
                    })
                }),
                upgrade_blocked: spec.get("upgradeBlocked").and_then(Value::as_u64),
                safe_mode_cooldown: spec.get("safeModeCooldown").and_then(Value::as_u64),
            }))
        }
        "extension" => Ok(RoomObject::Extension(ExtensionObj {
            id: 0,
            x,
            y,
            hits: hits_override
                .map(saturating_u32)
                .unwrap_or(c::EXTENSION_HITS as u32),
            off: false,
            owner: owner.unwrap_or_default(),
            store: store_energy(spec),
            notify_when_attacked: spec.get("notifyWhenAttacked").and_then(Value::as_bool),
        })),
        "constructedWall" => Ok(RoomObject::ConstructedWall(ConstructedWallObj {
            id: 0,
            x,
            y,
            // 墙初始 hits 仅为 WALL_HITS，最大可修复值由投影的 WALL_HITS_MAX 表示。
            hits: hits_override
                .map(saturating_u32)
                .unwrap_or(c::WALL_HITS as u32),
            notify_when_attacked: spec.get("notifyWhenAttacked").and_then(Value::as_bool),
        })),
        "road" => Ok(RoomObject::Road(RoadObj {
            id: 0,
            x,
            y,
            hits: hits_override
                .map(saturating_u32)
                .unwrap_or(c::ROAD_HITS as u32),
            // 放置参数是相对衰减间隔，对象字段保存绝对截止 tick。
            next_decay_time: ctx.time
                + spec
                    .get("ticksToDecay")
                    .and_then(Value::as_u64)
                    .unwrap_or(c::ROAD_DECAY_TIME as u64),
            notify_when_attacked: spec.get("notifyWhenAttacked").and_then(Value::as_bool),
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
            notify_when_attacked: spec.get("notifyWhenAttacked").and_then(Value::as_bool),
            store_extra: store_extra_of(spec),
        })),
        // 矿量默认按密度查表，显式再生间隔转换为绝对 tick。
        // 矿物的 0 间隔也有效，不能套用 source 的正值过滤。
        "mineral" => {
            // 只接受官方资源目录内的名字（RESOURCES_ALL）：矿物类型本身
            // 不限七基础矿——兼容夹具用 silicon 等 deposit 资源名摆矿物
            //（nuke 矩阵用例），采掉落仍是资源目录可接受的名称。
            let mineral_type = spec_str(spec, "mineralType")
                .filter(|t| kk_core::constants::RESOURCES_ALL.iter().any(|k| k == t))
                .ok_or_else(|| "mineral 缺 mineralType 或非官方资源名".to_string())?
                .to_string();
            let density = spec
                .get("density")
                .and_then(Value::as_u64)
                .unwrap_or(c::DENSITY_HIGH as u64)
                .min(u8::MAX as u64) as u8;
            let mineral_amount = spec
                .get("mineralAmount")
                .and_then(Value::as_u64)
                .map(saturating_u32)
                .unwrap_or(saturating_u32(crate::types::mineral_density(density) as u64));
            let next_regen = spec
                .get("ticksToRegeneration")
                .and_then(Value::as_u64)
                // 0 也表示已设置再生时间；立即到点仅在放置时 ctx.time > 0
                // 成立——time 0 放 0 间隔得 Some(0)，结算按官方 falsy 视为
                // 未设置重挂 50000 钟。
                // 来源：vanilla index.ts 的 placeMineral，及引擎 minerals/tick.js。
                .map(|t| ctx.time.saturating_add(t));
            Ok(RoomObject::Mineral(MineralObj {
                id: 0,
                x,
                y,
                mineral_type,
                mineral_amount,
                density,
                next_regeneration_time: next_regen,
            }))
        }
        // 提取器保存剩余冷却计数，不加当前时间。
        "extractor" => Ok(RoomObject::Extractor(ExtractorObj {
            id: 0,
            x,
            y,
            hits: hits_override
                .map(saturating_u32)
                .unwrap_or(c::EXTRACTOR_HITS as u32),
            owner,
            cooldown: spec.get("cooldown").and_then(Value::as_u64).unwrap_or(0),
            notify_when_attacked: spec.get("notifyWhenAttacked").and_then(Value::as_bool),
        })),
        _ => {
            let mut data = generic_fields("structure", spec);
            if !data.contains_key("structureType") && !st.is_empty() {
                data.insert("structureType".into(), json!(st));
            }
            // 相对衰减间隔转换为 nextDecayTime，随后删除相对字段。
            // container 默认间隔由控制器等级是否大于 0 决定，rampart 使用固定间隔。
            // 放置默认值参考：screeps-ok adapters/vanilla/index.ts 的 placeStructure。
            let decay_default: Option<u64> = match st {
                "container" => Some(match ctx.controller {
                    Some(ctl) if ctl.level > 0 => c::CONTAINER_DECAY_TIME_OWNED,
                    _ => c::CONTAINER_DECAY_TIME,
                } as u64),
                "rampart" => Some(c::RAMPART_DECAY_TIME as u64),
                _ => None,
            };
            let ttd = spec
                .get("ticksToDecay")
                .and_then(Value::as_u64)
                .or(decay_default);
            if let Some(v) = ttd {
                data.insert("nextDecayTime".into(), json!(ctx.time + v));
            }
            data.remove("ticksToDecay");
            // 放置 rampart 时默认私有，显式 isPublic 优先。
            if st == "rampart" {
                data.entry("isPublic").or_insert(json!(false));
            }
            // 通用容量和按资源容量分别补默认值，显式字段优先。
            // 特定资源容量会影响 Store.getCapacity() 是否返回 null，不能只补总容量。
            if let Some(cap) = built_store_capacity(st, &ctx.links, &ctx.stores) {
                data.entry("storeCapacity").or_insert(json!(cap));
            }
            if let Some(res) = built_store_capacity_resource(st, &ctx.links, &ctx.stores) {
                data.entry("storeCapacityResource").or_insert(res);
            }
            // 空仓也必须有原始 store，玩家 transfer 预检据此识别可存储目标。
            // 来源：vanilla getStructureDefaults 与 engine creeps.js 的目标检查。
            if data.contains_key("storeCapacity") || data.contains_key("storeCapacityResource") {
                let store = if data
                    .get("storeCapacityResource")
                    .and_then(Value::as_object)
                    .is_some_and(|slots| slots.contains_key("energy"))
                {
                    json!({"energy": 0})
                } else {
                    json!({})
                };
                data.entry("store").or_insert(store);
            }
            // lab 的矿物槽由已有 store 资源绑定，为未声明容量的矿物补容量。
            // 绑定后，其他矿物查询不会错误地使用尚未分配的剩余总容量。
            // 此实现只补缺键，保留显式 0；vanilla placeStructure 使用 falsy 检查。
            // 参考：vanilla index.ts 的 lab 分支，prelude structures.js 的 capacityForResource。
            if st == "lab"
                && let Some(spec_store) = spec.get("store").and_then(Value::as_object)
                && let Some(cap) = data
                    .get_mut("storeCapacityResource")
                    .and_then(Value::as_object_mut)
            {
                for res in spec_store.keys() {
                    if res != "energy" && cap.get(res).is_none() {
                        cap.insert(res.clone(), json!(ctx.stores.lab_mineral));
                    }
                }
            }
            // lab、terminal、nuker、factory 使用绝对 cooldownTime，其他类型保留计数 cooldown。
            // 转换前删除原 cooldown，避免同一记录同时保存相对值和绝对值。
            // 来源：screeps-ok adapters/vanilla/index.ts 的 placeStructure。
            data.remove("cooldown");
            if let Some(cd) = spec.get("cooldown").and_then(Value::as_u64) {
                if st == "lab" || st == "terminal" || st == "nuker" || st == "factory" {
                    data.insert("cooldownTime".into(), json!(ctx.time + cd));
                } else {
                    data.insert("cooldown".into(), json!(cd));
                }
            }
            // Mod 建筑的放置默认值来自配方（显式字段保留）；随后内建表。
            if let Some(building) = ctx.buildings.get(st) {
                let hits = building.hits_max.min(u32::MAX as u64) as i64;
                data.entry("hits").or_insert(json!(hits));
                data.entry("hitsMax").or_insert(json!(hits));
                if let Some(cap) = building.store_capacity {
                    data.entry("storeCapacity")
                        .or_insert(json!(cap.min(u32::MAX as u64)));
                    data.entry("store").or_insert(json!({ "energy": 0 }));
                }
            }
            // 仅补缺失的 hits、hitsMax；显式值保留，包括 Generic 中的 hitsMax。
            if let Some((hits, hits_max)) = generic_structure_hits(st, &ctx.links) {
                data.entry("hits").or_insert(json!(hits));
                data.entry("hitsMax").or_insert(json!(hits_max));
            }
            Ok(RoomObject::Generic(GenericObj {
                id: 0,
                x,
                y,
                data: Value::Object(data).to_string(),
            }))
        }
    }
}

/// 返回 Generic 结构的初始 hits 和 hitsMax，未知类型返回 None。
/// 放置与建造完成共用本表；专用对象变体使用各自默认值。
/// rampart 采用 `RAMPART_HITS_MAX[8]`，不按当前房间等级限制。
/// powerBank 也有默认值，即使 vanilla 的 placeStructure 不接受该类型。
/// 参考：screeps-ok adapters/vanilla/index.ts 的 getStructureDefaults。
pub fn generic_structure_hits(structure_type: &str, links: &LinkRules) -> Option<(i64, i64)> {
    let max8 = kk_core::constants::RAMPART_HITS_MAX
        .iter()
        .find(|(k, _)| *k == "8")
        .map(|(_, v)| *v)
        .unwrap_or(0);
    Some(match structure_type {
        "tower" => (c::TOWER_HITS, c::TOWER_HITS),
        "link" => (links.hits, links.hits_max),
        "container" => (c::CONTAINER_HITS, c::CONTAINER_HITS),
        "rampart" => (c::RAMPART_HITS, max8),
        "lab" => (c::LAB_HITS, c::LAB_HITS),
        "observer" => (c::OBSERVER_HITS, c::OBSERVER_HITS),
        "terminal" => (c::TERMINAL_HITS, c::TERMINAL_HITS),
        "powerSpawn" => (c::POWER_SPAWN_HITS, c::POWER_SPAWN_HITS),
        "nuker" => (c::NUKER_HITS, c::NUKER_HITS),
        "factory" => (c::FACTORY_HITS, c::FACTORY_HITS),
        "powerBank" => (c::POWER_BANK_HITS, c::POWER_BANK_HITS),
        _ => return None,
    })
}

/// 返回建造完成时 container、rampart 的绝对衰减截止 tick。
/// container 首次使用 CONTAINER_DECAY_TIME，与放置时按控制器等级选间隔不同。
/// 不能省略截止时间，否则衰减结算会立即扣除新结构 hits。
/// 来源：engine/src/processor/intents/creeps/build.js 的对应结构分支。
pub fn built_decay_clock(structure_type: &str, time: u64) -> Option<u64> {
    match structure_type {
        "container" => Some(time + c::CONTAINER_DECAY_TIME as u64),
        "rampart" => Some(time + c::RAMPART_DECAY_TIME as u64),
        _ => None,
    }
}

/// 返回结构的通用容量字段默认值，未知或无需该字段的类型返回 None。
/// 对有按资源容量的结构，应同时应用 built_store_capacity_resource；
/// 否则 Store.getCapacity() 无参可能由 null 变成数值。
/// 默认值参考：vanilla index.ts 的 getStructureDefaults。
pub fn built_store_capacity(
    structure_type: &str,
    links: &LinkRules,
    stores: &StoreRules,
) -> Option<i64> {
    Some(match structure_type {
        "container" => stores.container,
        "factory" => stores.factory,
        "terminal" => stores.terminal,
        "lab" => stores.lab_energy + stores.lab_mineral,
        "powerSpawn" => stores.power_spawn_energy + stores.power_spawn_power,
        "nuker" => stores.nuker_energy + stores.nuker_ghodium,
        "link" => links.capacity,
        "tower" => stores.tower,
        _ => return None,
    })
}

/// 返回按资源划分的容量，适用于 lab、powerSpawn、nuker、link 和 tower。
/// lab 这里只包含 energy；放置已有矿物时须另外绑定矿物容量。
/// 与 built_store_capacity 配合，供 Store 及各结构资源容量 getter 使用。
/// 参考：engine/src/game/store.js、vanilla index.ts 的 getStructureDefaults。
pub fn built_store_capacity_resource(
    structure_type: &str,
    links: &LinkRules,
    stores: &StoreRules,
) -> Option<Value> {
    Some(match structure_type {
        "lab" => json!({ "energy": stores.lab_energy }),
        "powerSpawn" => json!({
            "energy": stores.power_spawn_energy,
            "power": stores.power_spawn_power,
        }),
        "nuker" => json!({
            "energy": stores.nuker_energy,
            "G": stores.nuker_ghodium,
        }),
        "link" => json!({ "energy": links.capacity }),
        "tower" => json!({ "energy": stores.tower }),
        _ => return None,
    })
}

/// 仅为新建 nuker 返回绝对冷却截止 tick，不模拟 PTR 的短冷却。
/// 其他类型不设置此字段，玩家 getter 按缺失值读取为 0。
/// 来源：engine/src/processor/intents/creeps/build.js 的 nuker 分支。
pub fn built_cooldown_time(structure_type: &str, time: u64) -> Option<u64> {
    match structure_type {
        "nuker" => Some(time + c::NUKER_COOLDOWN as u64),
        _ => None,
    }
}

/// 构造工地，progress 默认 0，总进度来自 CONSTRUCTION_COST。
/// 不使用 spec.progressTotal，也不加入道路地形系数；玩家建造指令另有地形规则。
/// 来源：screeps-ok adapters/vanilla/index.ts 的 placeSite、getProgressTotal。
pub(super) fn normalize_site(
    spec: &Value,
    x: u8,
    y: u8,
    links: &LinkRules,
    buildings: &kk_mod_api::buildings::BuildingCatalog,
) -> Result<ConstructionSiteObj, String> {
    let structure_type = spec_str(spec, "structureType")
        .or_else(|| spec_str(spec, "type"))
        .ok_or_else(|| "constructionSite 缺 structureType".to_string())?
        .to_string();
    // Mod 建筑的总进度取配方最大成本量；link 的建造成本取有效规则；
    // 其余类型查 CONSTRUCTION_COST 官方表。
    let total = if let Some(building) = buildings.get(&structure_type) {
        building.progress_total().min(u32::MAX as u64) as i64
    } else if structure_type == "link" {
        links.build_cost
    } else {
        crate::types::construction_cost(&structure_type)
            .ok_or_else(|| format!("constructionSite 类型无 CONSTRUCTION_COST: {structure_type}"))?
    }
    .max(0);
    Ok(ConstructionSiteObj {
        id: 0,
        x,
        y,
        structure_type,
        progress: saturating_u32(spec.get("progress").and_then(Value::as_u64).unwrap_or(0)),
        progress_total: saturating_u32(total as u64),
        owner: spec_str(spec, "owner").map(str::to_string),
        name: spec_str(spec, "name").map(str::to_string),
    })
}
