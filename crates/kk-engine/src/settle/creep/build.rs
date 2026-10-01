//! 建造族：build intent 与工地完工结算（官方 build.js / create-construction.js
//! 完工链）。B1 结构治理批自 creep.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_BUILD, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;
use crate::settle::movement;

/// 官方 build.js：3 格范围、障碍格拒建、WORK×5、完成落成。
pub(super) fn intent_build(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, owner, energy) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), c.owner.clone(), c.store.energy as i64),
        _ => return,
    };
    if energy <= 0 {
        return;
    }
    let Some(RoomObject::ConstructionSite(site)) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&site.structure_type).is_none() {
        return;
    }
    if (site.x as i32 - pos.0 as i32).abs() > 3 || (site.y as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    // 目标格静态检查（build.js:37-60）：同类型结构已存在拒；障碍类型对象拒；
    // 障碍类型工地还被格内 creep 拒（blockingCreeps：safeMode 下仅自家 creep，
    // 否则任何 creep——官方 :53-58）
    let cell = (site.x, site.y);
    let site_is_obstacle = kk_world::types::is_obstacle_type(&site.structure_type);
    let my_safe_mode = w.controller().is_some_and(|ctl| {
        ctl.owner.as_deref().is_some_and(|u| u == owner.as_str()) && ctl.safe_mode > w.time
    });
    for tid in w.index.at(cell.0 as usize, cell.1 as usize) {
        match w.room.objects.get(tid) {
            Some(RoomObject::ConstructionSite(_)) if *tid != target_id => return,
            Some(RoomObject::Creep(c)) => {
                if site_is_obstacle && (!my_safe_mode || c.owner == owner) {
                    return;
                }
            }
            Some(other) => {
                let name = movement::type_name_of(other);
                if name == "constructionSite" {
                    continue;
                }
                if name == site.structure_type
                    || (site_is_obstacle && kk_world::types::is_obstacle_type(name.as_ref()))
                {
                    return;
                }
            }
            None => {}
        }
    }
    // 非 extractor/road 不可建在墙（build.js:68-70）
    if site.structure_type != "extractor"
        && site.structure_type != "road"
        && w.room
            .terrain
            .check_mask(cell.0 as usize, cell.1 as usize, kk_core::TERRAIN_MASK_WALL)
    {
        return;
    }
    // progressTotal 为创建时定格的存值（官方 build.js:68 读存的 progressTotal；
    // 放置路径=裸成本、intent 建工地=成本×地形系数，见 ConstructionSiteObj）。
    // remaining 允许为负（官方算术直通：适配器可播种 progress > progressTotal，
    // 此时 buildEffect 为负——progress 回钳至 total 完成落成、能量反增——
    // ROAD-HITS-001 swamp/wall 即靠此 quirk 过渡；勿加 saturate/早退门）
    let total = site.progress_total as i64;
    let remaining = total - site.progress as i64;
    // 官方两步（build.js:67-84，GPT 二轮复审 P1-1）：基础量（无强化计数 ×
    // BUILD_POWER）定耗能与效果点数 min(power, remaining, energy)；强化效果
    // 按增量数组（降序取前 effect 个、整体 floor、钳 remaining）；能量扣基础
    // 量、进度加强化量。官方无 effect<=0 早退门——零 WORK/零剩余路径仍记
    // actionLog 与事件（amount 0）
    let Some((effect, boosted)) = w.creep(id).map(|c| {
        let base = BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::BUILD_POWER;
        let effect = base.min(remaining).min(energy);
        let boosted = kk_world::types::boosted_effect(
            &c.body,
            BodyKind::Work,
            "work",
            "build",
            c::BUILD_POWER,
            effect,
            remaining,
        );
        (effect, boosted)
    }) else {
        return;
    };
    let new_progress = site.progress as i64 + boosted;
    if let Some(c) = w.creep_mut(id) {
        // 官方 `store.energy -= buildEffect`：正 effect 递减（min 含 energy 故
        // 不透支）、负 effect 递增（播种超限 quirk）；数学上不产生负值，钳 0
        // 仅防御（A6 对抗评审 P3：energy+|effect| 超 u32 仅宿主 fixture 可达，
        // saturating 转换防御截断）
        c.store.energy = u32::try_from((c.store.energy as i64 - effect).max(0)).unwrap_or(u32::MAX);
        set_action_log(&mut c.action_log, ACTION_BUILD);
    }
    let incomplete = new_progress < total;
    w.push_event(
        c::EVENT_BUILD,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            // 官方事件 amount=boostedEffect（build.js:96——进度点数非耗能量；
            // 负值合法：超限播种 quirk 下为负）
            "amount": boosted,
            "structureType": site.structure_type,
            "x": site.x,
            "y": site.y,
            "incomplete": incomplete,
        }),
    );
    if incomplete {
        if let Some(RoomObject::ConstructionSite(s)) = w.room.objects.get_mut(&target_id) {
            s.progress = new_progress as u32;
        }
    } else {
        complete_construction(w, target_id, site, owner);
    }
}

/// 官方 build.js 落成分支（124-230）：移除工地 + 新结构初值。
fn complete_construction(
    w: &mut Working,
    site_id: ObjId,
    site: kk_schema::room::ConstructionSiteObj,
    builder_owner: String,
) {
    let time = w.time;
    w.remove_obj(site_id);
    let obj = match site.structure_type.as_str() {
        "spawn" => RoomObject::Spawn(kk_schema::room::SpawnObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::SPAWN_HITS as u32,
            off: false,
            name: site.name.unwrap_or_default(),
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
            spawning: None,
            // 官方 build.js:116：玩家建造的结构出生即 notifyWhenAttacked: true
            notify_when_attacked: Some(true),
        }),
        "extension" => RoomObject::Extension(kk_schema::room::ExtensionObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::EXTENSION_HITS as u32,
            off: false,
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
            notify_when_attacked: Some(true),
        }),
        "constructedWall" => RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::WALL_HITS as u32,
            notify_when_attacked: Some(true),
        }),
        "road" => {
            // 官方：swamp/wall 上的路耐久按系数缩放（build.js:172-187）
            let mut hits = c::ROAD_HITS as u64;
            if w.room.terrain.check_mask(
                site.x as usize,
                site.y as usize,
                kk_core::TERRAIN_MASK_SWAMP,
            ) {
                hits *= c::CONSTRUCTION_COST_ROAD_SWAMP_RATIO as u64;
            }
            if w.room.terrain.check_mask(
                site.x as usize,
                site.y as usize,
                kk_core::TERRAIN_MASK_WALL,
            ) {
                hits *= c::CONSTRUCTION_COST_ROAD_WALL_RATIO as u64;
            }
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: site.x,
                y: site.y,
                hits: hits.min(u32::MAX as u64) as u32,
                next_decay_time: time + c::ROAD_DECAY_TIME as u64,
                notify_when_attacked: Some(true),
            })
        }
        "storage" => RoomObject::Storage(kk_schema::room::StorageObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::STORAGE_HITS as u32,
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
            notify_when_attacked: Some(true),
            store_extra: Vec::new(),
        }),
        // 官方 build.js:214-220 的 extractor 分支：user/hits=EXTRACTOR_HITS/
        // hitsMax=EXTRACTOR_HITS（+ :111-117 出生即 notifyWhenAttacked）；
        // cooldown 出生无键（undefined——首次 harvest 后才出现）
        "extractor" => RoomObject::Extractor(kk_schema::room::ExtractorObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::EXTRACTOR_HITS as u32,
            owner: site.owner,
            cooldown: 0,
            notify_when_attacked: Some(true),
        }),
        other => {
            // 其余类型（tower/rampart/lab/…）：透传桶落成（官方全字段表 M2 对齐）。
            // owner 键名与放置路径（place 透传 spec）及读取侧（movement 障碍
            // 判定）统一为 "owner"。hits/hitsMax 走放置面同源表
            //（generic_structure_hits——官方 build.js 各类型分支的 hits 部分；
            // 未知类型缺省 hits:1 与官方兜底一致）；rampart 的 hitsMax 官方
            // 建时按 controller 归属与 rcl 存库（build.js:161-166——归属比较
            // 的 object 是**执行 build 的 creep**（非工地主），跨主建造时
            // 归属一致 → RAMPART_HITS_MAX[rcl]，否则 0）
            let (hits, hits_max) = kk_world::place::generic_structure_hits(other).unwrap_or((1, 1));
            let hits_max = if other == "rampart" {
                match w.controller() {
                    Some(ctl) if ctl.owner.as_deref() == Some(builder_owner.as_str()) => {
                        kk_core::constants::RAMPART_HITS_MAX
                            .iter()
                            .find(|(k, _)| *k == ctl.level.to_string())
                            .map(|(_, v)| *v)
                            .unwrap_or(0)
                    }
                    _ => 0,
                }
            } else {
                hits_max
            };
            let mut data = serde_json::json!({
                "type": "structure",
                "structureType": other,
                "owner": site.owner,
                "hits": hits,
                "hitsMax": hits_max,
                // build.js:116：出生即 true
                "notifyWhenAttacked": true,
            });
            // 容量/衰减钟/冷却钟由放置面同源函数给出（官方 build.js:167
            // rampart nextDecayTime / :261 container / :230 lab storeCapacity /
            // :249 terminal / :258 container / :280 factory / :272 nuker
            // cooldownTime）——两入口读取面单源。漏钟的后果：phase 8 把
            // nextDecayTime 缺失（0）判为「已到点」，落成 rampart
            //（hits=RAMPART_HITS=1）在本 tick 即被 RAMPART_DECAY_AMOUNT=300
            // 打光移除；container 首 tick 误掉 CONTAINER_DECAY=5000。落成对象
            // 在 phase 8 的 id 快照之内（build 阶段早于 phase 8），所以不是
            //「下一 tick」而是本 tick。
            // 平面容量与 per-resource 容量**成对落**（只落平面会让
            // `Store.getCapacity()` 由官方 null 变成数值；只落 per-resource
            // 会让 energyCapacity getter 读右值而 storeCapacity getter 读
            // undefined）——两键同表值，见 built_store_capacity*。
            if let Some(cap) = kk_world::place::built_store_capacity(other) {
                data["storeCapacity"] = serde_json::json!(cap);
            }
            if let Some(res) = kk_world::place::built_store_capacity_resource(other) {
                data["storeCapacityResource"] = res;
            }
            if let Some(clock) = kk_world::place::built_decay_clock(other, time) {
                data["nextDecayTime"] = serde_json::json!(clock);
            }
            // nuker 落成即带 cooldownTime（官方 build.js:272——lab/factory/link
            // 的 `cooldown: 0` 不落：link 的 getter 读 `cooldown||0`、lab/factory
            // 的 getter 读 cooldownTime，缺键与 0 在读取面等价，见
            // built_cooldown_time 文档）。
            if let Some(cd) = kk_world::place::built_cooldown_time(other, time) {
                data["cooldownTime"] = serde_json::json!(cd);
            }
            // 官方 build.js 对各 owned 结构还落 `store: {energy: 0}`。此处刻意
            // 不落：Store 构造器吃 `object.store || {}`（prelude structures.js:135
            // ＝官方 store.js:21），而 0 值键被构造函数过滤——缺 store 与
            // `{energy: 0}` 在 getUsedCapacity/getFreeCapacity/Store 三函数族上
            // 同值（落成结构出生恒空仓）。
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: site.x,
                y: site.y,
                data: data.to_string(),
            })
        }
    };
    w.insert_new(obj);
}
