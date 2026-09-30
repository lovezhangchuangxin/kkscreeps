//! 建造面归一化：structure 大分派（structureType → typed 变体/Generic 透传）、
//! 结构 hits 缺省表（放置/建造两入口单源）、工地归一。B1 结构治理批自
//! place.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::{
    ConstructedWallObj, ConstructionSiteObj, ControllerObj, ExtensionObj, ExtractorObj, GenericObj,
    MineralObj, Reservation, RoadObj, RoomObject, Sign, SourceObj, StorageObj,
};
use serde_json::{Value, json};

use super::PlaceCtx;
use super::generic_fields;
use super::{normalize_spawn, saturating_u32, spec_str, store_energy, store_extra_of};

/// structure 类：按 structureType 分派。M1 类型化变体之外的类型进 Generic
/// （完整 spec 折算成透传 JSON，保持 M0 放置接受面；结算忽略）。
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
            // level 缺省 0（vanilla 未占房分支 `rcl ?? 0`，index.ts:904）；
            // 占房缺省 1 由 embed createShard 显式传入。safeMode/downgradeTime
            // 为绝对 tick（vanilla index.ts:971-993：种子相对值 + gameTime + 1）。
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
                // 放置面不从"曾拥有"态进入（降级归零才置位——官方 tick.js:56）
                owner_cleared: false,
                is_power_enabled: spec
                    .get("isPowerEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                // B2 持久化面透传（官方 DB 字段名 camelCase，绝对 tick）：
                // reservation {user, endTime} / sign {user, text, time} /
                // upgradeBlocked / safeModeCooldown
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
            // vanilla：新建墙 hits=1（WALL_HITS），可修到 300M（WALL_HITS_MAX）
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
            // M2B P4：spec.ticksToDecay 覆写（适配器 setup 契约——衰减钟
            // 绝对形态，官方 getter/快照折算差值）；缺省 ROAD_DECAY_TIME
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
        // B4：mineral 进 typed 变体（vanilla placeMineral，index.ts:1209-1221：
        // mineralType 必填、mineralAmount 缺省 MINERAL_DENSITY[density]、density
        // 缺省 DENSITY_HIGH(3)、ticksToRegeneration 给定时挂绝对钟——与 source
        // 的 >0 过滤不同，vanilla 对 mineral 只判 !== undefined）
        "mineral" => {
            // 白名单 = MINERAL_MIN_AMOUNT 键集（官方六矿种 H/O/U/K/L/Z/X——
            // 放置面外的矿种名会让内部超容 drop 绕过官方 RESOURCES_ALL
            // 白名单直接落地（B4 评审 2 P3-3 清偿：放置面收口）
            let mineral_type = spec_str(spec, "mineralType")
                .filter(|t| {
                    kk_core::constants::MINERAL_MIN_AMOUNT
                        .iter()
                        .any(|(k, _)| k == t)
                })
                .ok_or_else(|| "mineral 缺 mineralType 或非官方矿种".to_string())?
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
                // 不过滤 0（与 source 的 >0 过滤刻意不同）：vanilla placeMineral
                // 对 mineral 只判 !== undefined（index.ts:1218-1220）——0 即
                // Some(time+0)，官方 minerals/tick 的 `time >= nrt-1` 立即成立
                // = 放置后首 tick 立即再生（复审 F3：原 >0 过滤把它改成 5 万
                // tick 延迟，方向性引入分歧，已撤）
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
        // B4：extractor 进 typed 变体（vanilla placeStructure 对 extractor 的
        // cooldown 计数器形态，index.ts:1147-1151；hits 缺省 EXTRACTOR_HITS）
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
            // M2B 扩面 P4：衰减钟缺省/覆写 + cooldown 覆写（适配器 setup
            // 契约）——绝对挂钟形态（nextDecayTime/cooldownTime），官方
            // getter/快照面折算差值。缺省对齐官方放置语义：container 在
            // 有主房 CONTAINER_DECAY_TIME_OWNED、野房 CONTAINER_DECAY_TIME
            //（containers/tick.js 的重置窗同款）；rampart 恒
            // RAMPART_DECAY_TIME。generic_fields 原样拷贝的相对键
            // ticksToDecay/cooldown 转折算后移除（不留双形态）
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
            // 平面 storeCapacity 与 per-resource storeCapacityResource 成对落
            // （官方 getStructureDefaults 同表值；spec 显式值优先）。平面容量
            // 缺失会让 prelude 的 storeCapacity getter 恒读 undefined；
            // per-resource 缺失会让 `store.getCapacity()` 与 lab/powerSpawn/
            // nuker 的 *Capacity getter 读错值（见 built_store_capacity_resource）。
            if let Some(cap) = built_store_capacity(st) {
                data.entry("storeCapacity").or_insert(json!(cap));
            }
            if let Some(res) = built_store_capacity_resource(st) {
                data.entry("storeCapacityResource").or_insert(res);
            }
            // lab 特例（vanilla placeStructure index.ts:1122-1128）：spec.store
            // 带矿物时，给 storeCapacityResource 补缺失矿物的
            // LAB_MINERAL_CAPACITY——**槽绑定语义的玩家可观测面**：填充后
            // Σsrc 含已存矿物，未存矿物的 getCapacity 走回退式
            // `max(0, flat 5000−Σ5000)`=0（官方 null，槽已绑定）；不填则错读
            // max(0,5000−2000)=3000（槽未绑定）。已存矿物本身的读数两条路
            // 径同值 3000（表定义使然）。spec 显式给的 per-resource 键不覆盖
            // （vanilla `!attrs.storeCapacityResource[res]` 同式——只补
            // falsy/缺失键）。vanilla 同段还写 DB 键 mineralType——官方读取
            // 面是 store 派生 getter（structures.js:305），DB 键为死数据，不写。
            if st == "lab"
                && let Some(spec_store) = spec.get("store").and_then(Value::as_object)
                && let Some(cap) = data
                    .get_mut("storeCapacityResource")
                    .and_then(Value::as_object_mut)
            {
                for res in spec_store.keys() {
                    if res != "energy" && cap.get(res).is_none() {
                        cap.insert(res.clone(), json!(c::LAB_MINERAL_CAPACITY));
                    }
                }
            }
            // cooldown：官方两形态（vanilla index.ts:1145-1158 明文）——link/
            // extractor 存每 tick 递减计数器 `o.cooldown`（getter 直读），
            // lab/terminal/nuker/factory 存未来时间戳 `o.cooldownTime`（getter
            // 折算）。用错字段则运行期 getter 静默取不到值。generic_fields
            // 原样拷贝的相对键先移除，再按类型落对形态。
            data.remove("cooldown");
            if let Some(cd) = spec.get("cooldown").and_then(Value::as_u64) {
                if st == "lab" || st == "terminal" || st == "nuker" || st == "factory" {
                    data.insert("cooldownTime".into(), json!(ctx.time + cd));
                } else {
                    data.insert("cooldown".into(), json!(cd));
                }
            }
            // 读取面缺省（A6 hits 族）：generic 结构的初始 hits/hitsMax 对齐
            // vanilla 适配器 getStructureDefaults（adapters/vanilla/index.ts
            // :1910-1970 的 hits 部分——只补读取面两键，store/actionLog 等
            // 归各自能力批）。spec 显式值优先（entry.or_insert：官方
            // placeStructure 的 spec.hits 覆盖同款；hitsMax 不在 spec 键集、
            // 恒用表值——vanilla `if (spec.hits !== undefined)` 只覆盖 hits）。
            if let Some((hits, hits_max)) = generic_structure_hits(st) {
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

/// generic（非 typed 变体）结构的初始 hits/hitsMax 表——vanilla 适配器
/// getStructureDefaults 的 hits 部分。typed 变体（spawn/extension/road/
/// storage/wall/extractor）在各自臂内带常量缺省，不查本表。rampart 的
/// hitsMax 用 RAMPART_HITS_MAX[8] 上限（vanilla placeStructure 同口径；
/// 官方 build 路径是建时按 rcl 存库——放置面对齐适配器契约）。
/// pub：settle 的 build 落成分支（complete_construction generic 臂）共用
/// 本表——两入口（放置/建造）读取面单源（层间一致纪律）。
/// powerBank 是刻意偏离 vanilla（其 getStructureDefaults 无此 case、且
/// placeStructure 拒收；我方 place 通道的 powerBank 走 placeObject 直通道
/// 可放，缺省取官方 DB 值 POWER_BANK_HITS——common/lib/constants.js:259）。
pub fn generic_structure_hits(structure_type: &str) -> Option<(i64, i64)> {
    let max8 = kk_core::constants::RAMPART_HITS_MAX
        .iter()
        .find(|(k, _)| *k == "8")
        .map(|(_, v)| *v)
        .unwrap_or(0);
    Some(match structure_type {
        "tower" => (c::TOWER_HITS, c::TOWER_HITS),
        "link" => (c::LINK_HITS, c::LINK_HITS_MAX),
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

/// 建造落成入口的衰减钟（官方 build.js:161-168 rampart / :255-263 container）。
/// 与放置面口径**刻意不同**：官方 build 对 container 直接给
/// CONTAINER_DECAY_TIME，不判所在房是否 owned（首次衰减后由
/// containers/tick.js 按 owned 重挂）；rampart 恒 RAMPART_DECAY_TIME。
/// 落成缺钟的后果见 `kk_engine::settle::economy::generic_decay_tick`——
/// `nextDecayTime` 缺失（0）被判为「已到点」，rampart（hits=RAMPART_HITS=1）
/// 会在落成同一 tick 被 RAMPART_DECAY_AMOUNT=300 打光移除。
pub fn built_decay_clock(structure_type: &str, time: u64) -> Option<u64> {
    match structure_type {
        "container" => Some(time + c::CONTAINER_DECAY_TIME as u64),
        "rampart" => Some(time + c::RAMPART_DECAY_TIME as u64),
        _ => None,
    }
}

/// 平面 storeCapacity。表值取自 vanilla 适配器 `getStructureDefaults`
///（placement 面权威口径，`index.ts:1908-1979`）——官方 build.js 落成面是
/// 本表子集（build.js 对 **link/tower 只落 storeCapacityResource 不落平面容量**），
/// 那处差异不可观测：`Store.getCapacity()` 无参是
/// `storeCapacityResource ? null : storeCapacity || null`（store.js:31），
/// 两键并存时恒 null，与只落 resource 同值（链接/塔的 `storeCapacity`
/// 平坦 getter 官方本就没有）。故两入口共用本表单源，不做两表复制。
/// 与 [`built_store_capacity_resource`] **必须成对落**：只落平面不落
/// per-resource 会把 `getCapacity()` 由官方的 null 变成数值（P4 清偿复审
/// P2-1 即此形态）。
pub fn built_store_capacity(structure_type: &str) -> Option<i64> {
    Some(match structure_type {
        "container" => c::CONTAINER_CAPACITY,
        "factory" => c::FACTORY_CAPACITY,
        "terminal" => c::TERMINAL_CAPACITY,
        "lab" => c::LAB_ENERGY_CAPACITY + c::LAB_MINERAL_CAPACITY,
        "powerSpawn" => c::POWER_SPAWN_ENERGY_CAPACITY + c::POWER_SPAWN_POWER_CAPACITY,
        "nuker" => c::NUKER_ENERGY_CAPACITY + c::NUKER_GHODIUM_CAPACITY,
        "link" => c::LINK_CAPACITY,
        "tower" => c::TOWER_CAPACITY,
        _ => return None,
    })
}

/// per-resource 容量（官方 `getStructureDefaults` / build.js 的
/// `storeCapacityResource`）。**必须与平面 storeCapacity 成对出现**：`Store.
/// getCapacity()`（无参）是 `storeCapacityResource ? null : storeCapacity` 的
/// 三元（store.js:31）——只落平面容量会让 getCapacity() 由官方的 null
/// 变成数值；lab/powerSpawn/nuker/tower/link 的 `energyCapacity`（及
/// `powerCapacity`/`ghodiumCapacity`）getter 也直读本键，缺失即恒 0。
pub fn built_store_capacity_resource(structure_type: &str) -> Option<Value> {
    Some(match structure_type {
        "lab" => json!({ "energy": c::LAB_ENERGY_CAPACITY }),
        "powerSpawn" => json!({
            "energy": c::POWER_SPAWN_ENERGY_CAPACITY,
            "power": c::POWER_SPAWN_POWER_CAPACITY,
        }),
        "nuker" => json!({
            "energy": c::NUKER_ENERGY_CAPACITY,
            "G": c::NUKER_GHODIUM_CAPACITY,
        }),
        "link" => json!({ "energy": c::LINK_CAPACITY }),
        "tower" => json!({ "energy": c::TOWER_CAPACITY }),
        _ => return None,
    })
}

/// 建造落成入口的冷却钟（官方 build.js:272 nuker 的
/// `cooldownTime: gameTime + (config.ptr ? 100 : C.NUKER_COOLDOWN)`）。
/// `config.ptr` 是官方 PTR 测试服的 100 tick 特例，正式服恒 NUKER_COOLDOWN
/// （=100000），我方不模拟 PTR。
///
/// **只对 nuker 给值**：官方 build.js 对 link/lab/factory 写的是计数器形态
/// `cooldown: 0`，但那三类的读取面分别是 `o.cooldown || 0`（link）与
/// `cooldownTime` 折算（lab/factory，structures.js:310/:1431）——link 的
/// `cooldown: 0` 与缺键同读 0，lab/factory 写计数器键本就读不到，故三者在
/// 读取面上「缺键 ≡ 官方落值」，不落键即可。
pub fn built_cooldown_time(structure_type: &str, time: u64) -> Option<u64> {
    match structure_type {
        "nuker" => Some(time + c::NUKER_COOLDOWN as u64),
        _ => None,
    }
}

/// vanilla placeSite：progress 缺省 0；progressTotal 由 CONSTRUCTION_COST 派生
/// （vanilla getProgressTotal:1871-1874 为裸成本，不带地形系数——放置路径与
/// 引擎 intent 建工地（create-construction-site.js:38-48，road 有系数）刻意不同）。
pub(super) fn normalize_site(spec: &Value, x: u8, y: u8) -> Result<ConstructionSiteObj, String> {
    let structure_type = spec_str(spec, "structureType")
        .or_else(|| spec_str(spec, "type"))
        .ok_or_else(|| "constructionSite 缺 structureType".to_string())?
        .to_string();
    let total = crate::types::construction_cost(&structure_type)
        .ok_or_else(|| format!("constructionSite 类型无 CONSTRUCTION_COST: {structure_type}"))?
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
