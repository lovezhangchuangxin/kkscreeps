//! creep 指令执行与 creep tick（官方 creeps/intents.js 执行序 + tick.js）。
//! B1 增补近战 attack / repair / dismantle 执行器（伤害机制见 damage.rs）。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_schema::room::{BodyPart, RoomObject};
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::damage;
use super::movement::{self, offsets_by_direction};
use super::{SettleError, Working};
use crate::intent::{Action, EXECUTION_ORDER, IntentData};

/// 官方 creeps/intents.js：按 creepActions 序执行（压制过滤内建）。
pub fn process_creep_intents(w: &mut Working, id: ObjId, actions: &BTreeMap<Action, IntentData>) {
    let present: BTreeSet<Action> = actions.keys().copied().collect();
    for action in EXECUTION_ORDER {
        if !Action::executes(&present, action) {
            continue;
        }
        let data = &actions[&action];
        match action {
            Action::Move => intent_move(w, id, data),
            Action::Harvest => intent_harvest(w, id, data),
            Action::Build => intent_build(w, id, data),
            Action::UpgradeController => intent_upgrade_controller(w, id, data),
            Action::Transfer => intent_transfer(w, id, data),
            Action::Withdraw => intent_withdraw(w, id, data),
            Action::Drop => intent_drop(w, id, data),
            Action::Pickup => intent_pickup(w, id, data),
            Action::Attack => intent_attack(w, id, data),
            Action::Repair => intent_repair(w, id, data),
            Action::Dismantle => intent_dismantle(w, id, data),
            // M1 无执行器的动作（战斗/占位/杂项）：压制图已生效，此处静默跳过
            _ => {}
        }
    }
}

/// 官方 move.js：静态障碍预检后入矩阵（creep 不预检——挤格语义）。
/// 位移符号 → 方向常量（官方 move.js:24-29 的 `d = [dx, dy]` 判定；两轴同零
/// 不可达——同格 creep 被 API 层 isNearTo 排除，防御性返回 0 丢弃）。
fn direction_from_delta(dx: i32, dy: i32) -> u8 {
    match (dx.signum(), dy.signum()) {
        (0, -1) => 1,  // TOP
        (1, -1) => 2,  // TOP_RIGHT
        (1, 0) => 3,   // RIGHT
        (1, 1) => 4,   // BOTTOM_RIGHT
        (0, 1) => 5,   // BOTTOM
        (-1, 1) => 6,  // BOTTOM_LEFT
        (-1, 0) => 7,  // LEFT
        (-1, -1) => 8, // TOP_LEFT
        _ => 0,
    }
}

fn intent_move(w: &mut Working, id: ObjId, data: &IntentData) {
    let (spawning, fatigue, x, y, owner) = match w.creep(id) {
        Some(c) => (
            c.spawning,
            c.fatigue,
            c.x as i32,
            c.y as i32,
            c.owner.clone(),
        ),
        None => return,
    };
    if spawning {
        return;
    }
    // 官方 object._oldFatigue = object.fatigue（canMove 判定用）
    w.old_fatigue.insert(id, fatigue);

    // 官方双形态（move.js:15-29）：朝相邻 creep 移动（intent 带 id）按位移
    // 符号换算方向；direction 形态直取。id 形态目标不可达/不邻接 → 整条丢弃
    let direction = match data.direction {
        Some(d) => d,
        None => {
            let Some(target_id) = data.id else {
                return;
            };
            let Some((tx, ty)) = w.creep(target_id).map(|c| (c.x as i32, c.y as i32)) else {
                return;
            };
            let (ddx, ddy) = (tx - x, ty - y);
            if ddx.abs() > 1 || ddy.abs() > 1 {
                return; // 官方 API 层已挡 isNearTo；结算复核
            }
            direction_from_delta(ddx, ddy)
        }
    };
    let Some((dx, dy)) = offsets_by_direction(direction) else {
        return;
    };
    if x + dx < 0 || x + dx > 49 || y + dy < 0 || y + dy > 49 {
        return;
    }
    // 目标格静态障碍预检（官方 move.js:33-37）：OBSTACLE 非 creep 对象 +
    // 非公开 rampart + powerCreep 的跨 shard portal（M1 无）
    let target = ((x + dx) as u8, (y + dy) as u8);
    let mover_owner = owner;
    let has_static_obstacle = w
        .index
        .at(target.0 as usize, target.1 as usize)
        .iter()
        .any(|tid| {
            match w.room.objects.get(tid) {
                Some(RoomObject::Creep(_)) => false, // creep 不预检
                Some(RoomObject::Generic(o)) => {
                    let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
                    let st = data
                        .get("structureType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if st == "rampart" {
                        let public = data
                            .get("isPublic")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let owner = data.get("owner").and_then(|v| v.as_str()).unwrap_or("");
                        !public && owner != mover_owner
                    } else {
                        kk_world::types::is_obstacle_type(st)
                    }
                }
                Some(other) => kk_world::types::is_obstacle_type(&movement::type_name_of(other)),
                None => false,
            }
        });
    if has_static_obstacle {
        return;
    }
    w.movement.add(id, (x as u8, y as u8), dx, dy);
}

/// 官方 harvest.js source 分支（29-70）：邻接、能量、controller 归属、WORK×2。
fn intent_harvest(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (creep_pos, owner, work_power, store_energy, store_cap) = match w.creep(id) {
        Some(c) => (
            (c.x, c.y),
            c.owner.clone(),
            harvest_power(c),
            c.store.energy as i64,
            store_capacity(c),
        ),
        None => return,
    };
    if w.creep(id).is_some_and(|c| c.spawning) {
        return;
    }
    // 目标必须是同格邻接的 source
    let target_ok = matches!(w.room.objects.get(&target_id), Some(RoomObject::Source(_)))
        && w.room.objects.get(&target_id).is_some_and(|o| {
            let (tx, ty) = o.pos();
            (tx as i32 - creep_pos.0 as i32).abs() <= 1
                && (ty as i32 - creep_pos.1 as i32).abs() <= 1
        });
    if !target_ok {
        return;
    }
    // 官方 harvest.js:37 `if (harvestAmount)`：零功率（无 WORK）整体跳过——
    // 不扣减、无 actionLog、无事件
    if work_power <= 0 {
        return;
    }
    // controller 非本人/非本预约拒（harvest.js:37-39；M1 无 reservation）
    if let Some(ctl) = w.controller()
        && ctl.owner.as_deref().is_some_and(|u| u != owner)
    {
        return;
    }
    let energy = match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Source(s)) if s.energy > 0 => s.energy.min(work_power as u32),
        _ => return,
    };
    let amount = energy as i64;
    if let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&target_id) {
        s.energy -= amount as u32;
    }
    // 官方顺序（harvest.js:49-60）：先满额入仓，再对超容部分 drop
    let sum = store_energy + amount;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy = sum.min(u32::MAX as i64) as u32;
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if sum > store_cap {
        intent_drop_amount(w, id, sum - store_cap);
    }
    w.push_event(
        c::EVENT_HARVEST,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "amount": amount }),
    );
}

/// 官方 build.js：3 格范围、障碍格拒建、WORK×5、完成落成。
fn intent_build(w: &mut Working, id: ObjId, data: &IntentData) {
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
                if site_is_obstacle && kk_world::types::is_obstacle_type(name.as_ref()) {
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
    let build_power = match w.creep(id) {
        Some(c) => BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::BUILD_POWER,
        None => 0,
    };
    // progressTotal 为创建时定格的存值（官方 build.js:68 读存的 progressTotal；
    // 放置路径=裸成本、intent 建工地=成本×地形系数，见 ConstructionSiteObj）
    let total = site.progress_total as i64;
    let remaining = total.saturating_sub(site.progress as i64);
    let effect = build_power.min(remaining).min(energy);
    if effect <= 0 {
        return;
    }
    let new_progress = site.progress as i64 + effect;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy -= effect as u32;
        set_action_log(&mut c.action_log, ACTION_BUILD);
    }
    let incomplete = new_progress < total;
    w.push_event(
        c::EVENT_BUILD,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": effect,
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
        complete_construction(w, target_id, site);
    }
}

/// 官方 build.js 落成分支（124-230）：移除工地 + 新结构初值。
fn complete_construction(
    w: &mut Working,
    site_id: ObjId,
    site: kk_schema::room::ConstructionSiteObj,
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
        }),
        "extension" => RoomObject::Extension(kk_schema::room::ExtensionObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::EXTENSION_HITS as u32,
            off: false,
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
        }),
        "constructedWall" => RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::WALL_HITS as u32,
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
            })
        }
        "storage" => RoomObject::Storage(kk_schema::room::StorageObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::STORAGE_HITS as u32,
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
        }),
        other => {
            // 其余类型（tower/rampart/lab/…）：透传桶落成（官方全字段表 M2 对齐）。
            // owner 键名与放置路径（place 透传 spec）及读取侧（movement 障碍
            // 判定）统一为 "owner"
            let data = serde_json::json!({
                "type": "structure",
                "structureType": other,
                "owner": site.owner,
                "hits": 1,
            });
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

/// 官方 upgradeController.js：3 格、归属、8 级上限、升级进位、gcl 计入。
fn intent_upgrade_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, owner, energy, work_power) = match w.creep(id) {
        Some(c) if !c.spawning && c.store.energy > 0 => (
            (c.x, c.y),
            c.owner.clone(),
            c.store.energy as i64,
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::UPGRADE_CONTROLLER_POWER,
        ),
        _ => return,
    };
    let Some(RoomObject::Controller(_)) = w.room.objects.get(&target_id) else {
        return;
    };
    let (ctl_x, ctl_y, ctl_owner, level, progress) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.x, t.y, t.owner.clone(), t.level, t.progress),
        _ => return,
    };
    if (ctl_x as i32 - pos.0 as i32).abs() > 3 || (ctl_y as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    if level == 0 || ctl_owner.as_deref() != Some(owner.as_str()) {
        return;
    }
    let mut effect = work_power.min(energy);
    // 官方 _upgraded 累计（8 级每 tick 上限 15）
    let upgraded = *w.upgraded.get(&target_id).unwrap_or(&0);
    if level == 8 {
        let limit = c::CONTROLLER_MAX_UPGRADE_PER_TICK;
        if upgraded >= limit {
            return;
        }
        effect = effect.min(limit - upgraded);
    }
    if effect <= 0 {
        return;
    }
    let new_progress;
    if level < 8 {
        let next_level_progress = controller_levels(level);
        let downgrade_time = match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => t.downgrade_time,
            _ => None,
        };
        // 官方升级条件（upgradeController.js:63-66）：进位需 downgrade 窗口健康
        if progress + effect as u64 >= next_level_progress
            && downgrade_time.is_some_and(|dt| {
                dt + c::CONTROLLER_DOWNGRADE_RESTORE as u64 >= w.time + controller_downgrade(level)
            })
        {
            new_progress = progress + effect as u64 - next_level_progress;
            *w.upgraded.entry(target_id).or_insert(0) += effect;
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
                t.level = level + 1;
                t.progress = new_progress;
                if level + 1 == 8 {
                    t.progress = 0;
                }
                t.safe_mode_available += 1;
                t.downgrade_time = Some(w.time + controller_downgrade(level + 1) / 2);
            }
        } else {
            new_progress = progress + effect as u64;
            *w.upgraded.entry(target_id).or_insert(0) += effect;
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
                t.progress = new_progress;
            }
        }
    } else {
        *w.upgraded.entry(target_id).or_insert(0) += effect;
    }
    // gcl 计入玩家（官方 bulkUsers.inc(target.user, 'gcl', boostedEffect)）
    let player = w.players.entry(owner.clone()).or_default();
    player.gcl += effect;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy -= effect as u32;
        set_action_log(&mut c.action_log, ACTION_UPGRADE);
    }
    w.push_event(
        c::EVENT_UPGRADE_CONTROLLER,
        id,
        serde_json::json!({ "amount": effect, "energySpent": effect }),
    );
}

/// 官方 transfer.js：白名单资源、邻接、容量双形态（M1 单资源槽=总量一致）。
fn intent_transfer(w: &mut Working, id: ObjId, data: &IntentData) {
    let resource = data.resource_type.as_deref().unwrap_or("");
    if resource != "energy" {
        return; // M1 资源白名单仅 energy
    }
    let Some(amount_req) = data.amount else {
        return;
    };
    let (pos, held) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), c.store.energy as i64),
        _ => return,
    };
    if held < amount_req || amount_req < 0 {
        return;
    }
    let Some(target_id) = data.id else { return };
    // extension 容量按 RCL（官方 extensions/tick.js:10-17 归一
    // storeCapacityResource.energy，transfer.js:24-39 经 capacityForResource 钳制；
    // 无 controller ⇒ 官方 capacityForResource 得 undefined ⇒ `!targetCapacity` 拒）
    let extension_cap = || {
        w.controller()
            .map(|ctl| kk_world::types::extension_energy_capacity(ctl.level))
            .unwrap_or(0)
    };
    let (target_cap, t_pos, t_stored) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Creep(t)) if !t.spawning => {
            (store_capacity(t), (t.x, t.y), t.store.energy as i64)
        }
        Some(RoomObject::Spawn(t)) => (c::SPAWN_ENERGY_CAPACITY, (t.x, t.y), t.store.energy as i64),
        Some(RoomObject::Extension(t)) => (extension_cap(), (t.x, t.y), t.store.energy as i64),
        Some(RoomObject::Storage(t)) => (c::STORAGE_CAPACITY, (t.x, t.y), t.store.energy as i64),
        _ => return,
    };
    if (t_pos.0 as i32 - pos.0 as i32).abs() > 1 || (t_pos.1 as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    if t_stored >= target_cap {
        return;
    }
    let mut amount = amount_req.min(held);
    if t_stored + amount > target_cap {
        amount = target_cap - t_stored;
    }
    if amount == 0 {
        return;
    }
    match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Creep(t)) => t.store.energy += amount as u32,
        Some(RoomObject::Spawn(t)) => t.store.energy += amount as u32,
        Some(RoomObject::Extension(t)) => t.store.energy += amount as u32,
        Some(RoomObject::Storage(t)) => t.store.energy += amount as u32,
        _ => return,
    }
    if let Some(c) = w.creep_mut(id) {
        c.store.energy -= amount as u32;
    }
    w.push_event(
        c::EVENT_TRANSFER,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// 官方 withdraw.js：邻接、空容量、safeMode/rampart 保护（M1 无 rampart 变体）。
fn intent_withdraw(w: &mut Working, id: ObjId, data: &IntentData) {
    if data.resource_type.as_deref().is_some_and(|r| r != "energy") {
        return;
    }
    let Some(target_id) = data.id else { return };
    let (pos, cap, held) = match w.creep(id) {
        Some(c) if !c.spawning && store_capacity(c) > 0 => {
            ((c.x, c.y), store_capacity(c), c.store.energy as i64)
        }
        _ => return,
    };
    let empty_space = cap - held;
    let Some(amount_req) = data.amount else {
        return;
    };
    let mut amount = amount_req.min(empty_space);
    if amount < 0 {
        return;
    }
    // 可提取对象：有 store 的结构（spawn/extension/storage）；source 只能
    // harvest、掉落资源只能 pickup（官方引擎层无 source/energy 的 withdraw 路径）
    let (t_pos, t_energy) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Spawn(t)) => ((t.x, t.y), t.store.energy as i64),
        Some(RoomObject::Extension(t)) => ((t.x, t.y), t.store.energy as i64),
        Some(RoomObject::Storage(t)) => ((t.x, t.y), t.store.energy as i64),
        _ => return,
    };
    if (t_pos.0 as i32 - pos.0 as i32).abs() > 1 || (t_pos.1 as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    amount = amount.min(t_energy);
    if amount <= 0 {
        return;
    }
    match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Spawn(t)) => t.store.energy -= amount as u32,
        Some(RoomObject::Extension(t)) => t.store.energy -= amount as u32,
        Some(RoomObject::Storage(t)) => t.store.energy -= amount as u32,
        _ => return,
    }
    if let Some(c) = w.creep_mut(id) {
        c.store.energy += amount as u32;
    }
    // 官方 withdraw 的 EVENT_TRANSFER 反向（objectId=target, data.targetId=object）
    w.push_event(
        c::EVENT_TRANSFER,
        target_id,
        serde_json::json!({ "targetId": id.as_str(), "resourceType": "energy", "amount": amount }),
    );
}

/// 官方 drop.js：amount>0 落地为同格 energy；持有不足整条拒绝（:13
/// `!(store[resourceType] >= intent.amount)` ⇒ return，不钳制部分掉落）。
fn intent_drop(w: &mut Working, id: ObjId, data: &IntentData) {
    if data.resource_type.as_deref().is_some_and(|r| r != "energy") {
        return;
    }
    let Some(amount) = data.amount else { return };
    if amount <= 0 {
        return;
    }
    if w.creep(id)
        .is_none_or(|c| c.spawning || (c.store.energy as i64) < amount)
    {
        return;
    }
    intent_drop_amount(w, id, amount);
}

/// 掉落能量（drop.js 落地路径；dismantle 回流超容与 harvest 超容共用）。
pub(super) fn intent_drop_amount(w: &mut Working, id: ObjId, amount: i64) {
    let (pos, held) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), c.store.energy as i64),
        _ => return,
    };
    // 钳制仅服务内部调用方（harvest 超容路径已保证 held ≥ amount）
    let amount = amount.min(held);
    if amount <= 0 {
        return;
    }
    if let Some(c) = w.creep_mut(id) {
        c.store.energy -= amount as u32;
    }
    w.create_energy(pos.0, pos.1, amount);
}

/// 官方 pickup.js：邻接 energy、余量钳制、目标清零移除。
fn intent_pickup(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, cap, held) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), store_capacity(c), c.store.energy as i64),
        _ => return,
    };
    if held >= cap {
        return;
    }
    let Some(RoomObject::Energy(target)) = w.room.objects.get(&target_id) else {
        return;
    };
    let (tx, ty, amount) = (target.x, target.y, target.amount as i64);
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    let amount = amount.min(cap - held);
    if amount <= 0 {
        return;
    }
    if let Some(RoomObject::Energy(t)) = w.room.objects.get_mut(&target_id) {
        t.amount -= amount as u32;
    }
    let drained = w
        .room
        .objects
        .get(&target_id)
        .is_some_and(|o| matches!(o, RoomObject::Energy(e) if e.amount == 0));
    if drained {
        w.remove_obj(target_id);
    }
    if let Some(c) = w.creep_mut(id) {
        c.store.energy += amount as u32;
    }
}

/// 官方 attack.js（近战）：邻接、有 hits 的目标、他人 safeMode 拒、目标格
/// rampart 重定向、ATTACK 部件数 × ATTACK_POWER（无 boost）。
fn intent_attack(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // attack.js:13-15：spawning 拒；:38 功率 = 活跃 ATTACK × ATTACK_POWER
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            BodyKind::count_active(&c.body, BodyKind::Attack) as i64 * c::ATTACK_POWER,
        ),
        _ => return,
    };
    // attack.js:18：目标不存在或自指拒
    if target_id == id {
        return;
    }
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let (tx, ty) = target.pos();
    // attack.js:21-23：近战射程 1（Chebyshev）
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    // attack.js:24-26：spawning 中的 creep 目标拒
    if let RoomObject::Creep(t) = &target
        && t.spawning
    {
        return;
    }
    // attack.js:27-29：无 hits 的对象不可攻（source/controller/工地/掉落/旗）
    if damage::object_hits(&target) <= 0 {
        return;
    }
    // attack.js:30-32：他人（含无主房）safeMode 生效期拒
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    // attack.js:33-36：目标格有 rampart → 转打 rampart（任何主人/公开性）
    let target_id = damage::rampart_at(w, (tx, ty)).unwrap_or(target_id);
    // attack.js:40：统一伤害入口（0 功率也走——官方引擎层无部件预检）
    damage::apply_damage(w, id, target_id, power, c::EVENT_ATTACK_TYPE_MELEE);
    // attack.js:42 `object._attack = true`：官方全库只写不读的死标记，不移植
}

/// 官方 attack.js:30-32 / dismantle.js:24-26 的 safeMode 门：controller 归
/// 他人（undefined user ≠ object.user 同样成立）且 safeMode > gameTime。
fn blocked_by_safe_mode(w: &Working, actor: &str) -> bool {
    w.controller()
        .is_some_and(|ctl| ctl.owner.as_deref() != Some(actor) && ctl.safe_mode > w.time)
}

/// 官方 repair.js：射程 3、WORK × REPAIR_POWER、能量按 REPAIR_COST 折算消耗、
/// hitsMax 钳制（无 boost）。
fn intent_repair(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // repair.js:8-13：spawning 或空仓拒；:23 功率 = 活跃 WORK × REPAIR_POWER
    let (pos, energy, repair_power) = match w.creep(id) {
        Some(c) if !c.spawning && c.store.energy > 0 => (
            (c.x, c.y),
            c.store.energy as i64,
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::REPAIR_POWER,
        ),
        _ => return,
    };
    // repair.js:15-18：目标须为可修结构（CONSTRUCTION_COST 类型 + 有 hitsMax）
    // 且未满血——creep/source 等不在 CONSTRUCTION_COST 表，天然拒
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&movement::type_name_of(&target)).is_none() {
        return;
    }
    let Some(hits_max) = damage::structure_hits_max(w, &target) else {
        return;
    };
    let hits = damage::object_hits(&target);
    if hits >= hits_max {
        return;
    }
    let (tx, ty) = target.pos();
    // repair.js:19-21：射程 3（Chebyshev）
    if (tx as i32 - pos.0 as i32).abs() > 3 || (ty as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    // repair.js:23-27：effect = min(功率, 能量折算, 缺口)；cost = min(存量,
    // ceil(effect×REPAIR_COST))。JS 浮点（energy/0.01、ceil(e*0.01)）在此值域
    // 恒等于整数运算——值域自明：部件数 ≤ MAX_CREEP_SIZE(50) ⇒ effect ≤
    // 50×REPAIR_POWER = 5000；0.01 的 double 表示相对误差 ~2.1e-15，e×0.01 真值
    // 偏离精确整数至多 ~1e-11，而 5000 量级 double 的半 ULP 仅 ~4.6e-13——
    // 乘积必舍入到精确整数 ⇒ ceil == ceil(e/100) 整除（数值验证另覆盖至 6×10⁸
    // 无失配）
    let energy_remaining = energy * 100; // energy / REPAIR_COST
    let missing = hits_max - hits;
    let effect = repair_power.min(energy_remaining).min(missing);
    let cost = energy.min((effect + 99) / 100); // ceil(effect × REPAIR_COST)
    // repair.js:38-42：无 boost 时 boostedEffect = effect；零效应拒（无 WORK）
    if effect <= 0 {
        return;
    }
    // repair.js:44-51：加血（钳 hitsMax）、扣能
    damage::set_structure_hits(w, target_id, (hits + effect).min(hits_max));
    if let Some(creep) = w.creep_mut(id) {
        creep.store.energy -= cost as u32;
        set_action_log(&mut creep.action_log, ACTION_REPAIR);
    }
    // repair.js:58-60
    w.push_event(
        c::EVENT_REPAIR,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": effect,
            "energySpent": cost,
        }),
    );
}

/// 官方 dismantle.js：邻接、CONSTRUCTION_COST 结构、他人 safeMode 拒、目标格
/// rampart 重定向、WORK × DISMANTLE_POWER、按 DISMANTLE_COST 回流能量。
fn intent_dismantle(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // dismantle.js:10-12：spawning 拒；:33 功率 = 活跃 WORK × DISMANTLE_POWER
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::DISMANTLE_POWER,
        ),
        _ => return,
    };
    // dismantle.js:17-19：目标须为可建类型结构（creep/source 等拒）
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&movement::type_name_of(&target)).is_none() {
        return;
    }
    let (tx, ty) = target.pos();
    // dismantle.js:21-23：邻接
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    // dismantle.js:24-26：safeMode 门
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    // dismantle.js:27-30：rampart 重定向——amount 须按重定向后的目标 hits 取
    let target_id = damage::rampart_at(w, (tx, ty)).unwrap_or(target_id);
    let hits = w
        .room
        .objects
        .get(&target_id)
        .map(damage::object_hits)
        .unwrap_or(0);
    // dismantle.js:33-35：amount = min(功率, 目标 hits)；回流 = floor(amount ×
    // DISMANTLE_COST)——值域自明：amount ≤ 50×DISMANTLE_POWER = 2500，
    // 0.005 的 double 相对误差 ~2.1e-15 使乘积偏离整数至多 ~5e-12，远小于该
    // 量级半 ULP（~2.3e-13）⇒ JS floor == amount/200 整除（数值验证另覆盖至
    // 6×10⁸ 无失配）
    let amount = power.min(hits);
    let energy_gain = amount / 200;
    // dismantle.js:42-52：amount 为零不动；先回流能量、超容掉落、再施伤（可击毁）
    if amount <= 0 {
        return;
    }
    if let Some(creep) = w.creep_mut(id) {
        creep.store.energy = creep
            .store
            .energy
            .saturating_add(u32::try_from(energy_gain).unwrap_or(u32::MAX));
    }
    // dismantle.js:47-50：回流后超容立即掉落
    damage::drop_energy_over_capacity(w, id);
    damage::apply_damage(w, id, target_id, amount, c::EVENT_ATTACK_TYPE_DISMANTLE);
}

/// 官方 creeps/tick.js（M1 子集）：移动落格、TTL 死亡、疲劳恢复。
pub fn creep_tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    // 移动落格（movement.js execute）
    movement_execute(w, id)?;
    if !w.room.objects.contains_key(&id) {
        return Ok(()); // 结算中被移除（不会发生于 M1 路径，防御）
    }
    let (spawning, age_time) = match w.creep(id) {
        Some(c) => (c.spawning, c.age_time),
        None => return Ok(()),
    };
    if !spawning {
        // TTL 死亡（creeps/tick.js:93-95 gameTime >= ageTime-1）
        if age_time > 0 && w.time >= age_time.saturating_sub(1) {
            die_creep(w, id);
            return Ok(());
        }
    }
    // 疲劳恢复（creeps/tick.js:105-108）：-2 × 存活 MOVE 部件
    if let Some(c) = w.creep(id) {
        let moves = BodyKind::count_active(&c.body, BodyKind::Move) as i64;
        if moves > 0 {
            w.add_fatigue(id, -2 * moves);
        }
    }
    // 延迟伤害结算（creeps/tick.js:118-150 在疲劳恢复之后）：hits 归零 → 暴力
    // 死亡（与 TTL 死亡同路径），否则重排部件分布并处理缩容掉落
    damage::apply_pending_damage(w, id);
    Ok(())
}

/// 官方 movement.js execute（189-252）。
fn movement_execute(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    let Some(target) = w.movement.take_move(id) else {
        return Ok(());
    };
    // 对象可能在 check 与 execute 之间消亡（如 spawnstomp）——官方 objects 映射
    // 查不到即跳过；panic 会绕过"失败即回退"的 D4-3 语义，不设 expect
    let Some(creep) = w.creep(id) else {
        return Ok(());
    };
    let old_fatigue = w.old_fatigue.get(&id).copied().unwrap_or(0);
    if !movement::can_move(creep, old_fatigue) {
        return Ok(());
    }
    let (from, owner, body_len, weight) = (
        (creep.x, creep.y),
        creep.owner.clone(),
        creep.body.len(),
        movement::move_weight(creep),
    );
    // 官方疲劳率判定顺序（movement.js:204-222）：默认 2 → 沼泽 10 →
    // 路最后覆盖为 1（路上的沼泽按路走）；路磨损；进格踩掉敌方工地
    let mut fatigue_rate = 2i64;
    let mut road_id = None;
    let cell_ids: Vec<ObjId> = w.index.at(target.0 as usize, target.1 as usize).to_vec();
    for tid in cell_ids {
        match w.room.objects.get(&tid).cloned() {
            Some(RoomObject::Road(_)) => {
                road_id = Some(tid);
            }
            Some(RoomObject::ConstructionSite(site))
                // 官方 :224-233：非 safeMode 保护时，踩掉敌方工地（退一半工程为能量）
                if site.owner.as_deref() != Some(owner.as_str()) => {
                    let pos = (site.x, site.y);
                    w.remove_obj(tid);
                    if site.progress > 1 {
                        w.create_energy(pos.0, pos.1, (site.progress / 2) as i64);
                    }
                }
            _ => {}
        }
    }
    if w.room.terrain.check_mask(
        target.0 as usize,
        target.1 as usize,
        kk_core::TERRAIN_MASK_SWAMP,
    ) {
        fatigue_rate = 10;
    }
    if let Some(rid) = road_id {
        fatigue_rate = 1;
        if let Some(RoomObject::Road(r)) = w.room.objects.get_mut(&rid) {
            r.next_decay_time = r
                .next_decay_time
                .saturating_sub((c::ROAD_WEAROUT * body_len as i64).max(0) as u64);
        }
    }
    // 疲劳（movement.js:235-251）：体重 × 系数；边缘特例（从非边缘进边缘格 = 0，
    // 为跨房间预备——M1 单房间无落地，仍按官方给 0）
    let fatigue = weight as i64 * fatigue_rate;
    let to_edge = target.0 == 0 || target.0 == 49 || target.1 == 0 || target.1 == 49;
    let from_edge = from.0 == 0 || from.0 == 49 || from.1 == 0 || from.1 == 49;
    w.move_obj(id, target);
    if to_edge && !from_edge {
        if let Some(c) = w.creep_mut(id) {
            c.fatigue = 0;
        }
        w.tick_fatigue.insert(id, 0);
    } else {
        w.add_fatigue(id, fatigue);
    }
    Ok(())
}

/// creep 死亡（官方 _die.js 的 M1 形态：资源掉地、无墓碑——灰区登记）。
/// B1 起两条死因合流：TTL 到期（tick.js:93-95）与部件全灭的暴力死亡
/// （tick.js:134-136，damage::apply_pending_damage）。
pub fn die_creep(w: &mut Working, id: ObjId) {
    if let Some(c) = w.creep(id) {
        let (x, y, energy) = (c.x, c.y, c.store.energy as i64);
        if energy > 0 {
            w.create_energy(x, y, energy);
        }
    }
    w.remove_obj(id);
}

// ---- 公共小工具 ----

pub const ACTION_HARVEST: u8 = 1 << 0;
pub const ACTION_BUILD: u8 = 1 << 1;
pub const ACTION_UPGRADE: u8 = 1 << 2;
/// 官方 actionLog.attack：近战与 dismantle 共用此键（_damage.js:60-65）。
pub const ACTION_ATTACK: u8 = 1 << 3;
/// 官方 actionLog.attacked（_damage.js:73-75）。
pub const ACTION_ATTACKED: u8 = 1 << 4;
/// 官方 actionLog.repair（repair.js:53）。
pub const ACTION_REPAIR: u8 = 1 << 5;

pub fn set_action_log(bits: &mut u8, bit: u8) {
    *bits |= bit;
}

/// 官方 harvest 功率：WORK 部件数 × HARVEST_POWER（无 boost）。
pub fn harvest_power(creep: &kk_schema::room::CreepObj) -> i64 {
    BodyKind::count_active(&creep.body, BodyKind::Work) as i64 * c::HARVEST_POWER
}

/// 官方 storeCapacity（M1 无 boost）：CARRY 部件数 × CARRY_CAPACITY。
pub fn store_capacity(creep: &kk_schema::room::CreepObj) -> i64 {
    BodyKind::count_active(&creep.body, BodyKind::Carry) as i64 * c::CARRY_CAPACITY
}

fn controller_levels(level: u8) -> u64 {
    let key = level.to_string();
    c::CONTROLLER_LEVELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}

fn controller_downgrade(level: u8) -> u64 {
    let key = level.to_string();
    c::CONTROLLER_DOWNGRADE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}

pub fn body_parts_kinds(body: &[BodyPart]) -> Vec<BodyKind> {
    body.iter()
        .filter_map(|p| BodyKind::from_code(p.kind))
        .collect()
}
