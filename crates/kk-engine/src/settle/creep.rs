//! creep 指令执行与 creep tick（官方 creeps/intents.js 执行序 + tick.js）。
//! B1 增补近战 attack / repair / dismantle 执行器（伤害机制见 damage.rs）；
//! B2 增补 controller 结算族（claim/reserve/attack/sign/generateSafeMode）。

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
            Action::Heal => intent_heal(w, id, data, false),
            Action::RangedHeal => intent_heal(w, id, data, true),
            // B2 controller 结算族（claimController/reserveController/
            // attackController/signController/generateSafeMode）
            Action::ClaimController => intent_claim_controller(w, id, data),
            Action::ReserveController => intent_reserve_controller(w, id, data),
            Action::AttackController => intent_attack_controller(w, id, data),
            Action::SignController => intent_sign_controller(w, id, data),
            Action::GenerateSafeMode => intent_generate_safe_mode(w, id, data),
            // A4-iv 移动杂项族（say/suicide/pull——官方 processor/intents/creeps/
            // 同名文件）
            Action::Say => intent_say(w, id, data),
            Action::Suicide => intent_suicide(w, id),
            Action::Pull => intent_pull(w, id, data),
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

/// 官方 harvest.js：目标类型分派（source :25-65 / mineral :67-112——B4 补；
/// deposit :114-139 随对应能力批）。公共预检（:10-23）：spawning 拒、目标
/// 存在、Chebyshev 邻接（先于类型分支）。
fn intent_harvest(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let creep_pos = w.creep(id).map(|c| (c.x, c.y)).unwrap_or((0, 0));
    let (tx, ty) = target.pos();
    if (tx as i32 - creep_pos.0 as i32).abs() > 1 || (ty as i32 - creep_pos.1 as i32).abs() > 1 {
        return;
    }
    match target {
        RoomObject::Source(_) => intent_harvest_source(w, id, target_id),
        RoomObject::Mineral(_) => intent_harvest_mineral(w, id, target_id),
        _ => {}
    }
}

/// 官方 harvest.js source 分支（25-65）：controller 归属/预定门（B2 补
/// reservation 分支）、WORK×HARVEST_POWER、超容 drop energy。
fn intent_harvest_source(w: &mut Working, id: ObjId, target_id: ObjId) {
    let (owner, work_power, held_energy, held_total, store_cap) = match w.creep(id) {
        Some(c) => (
            c.owner.clone(),
            harvest_power(c),
            // energy 槽局部持有（写槽用——官方 :43 槽局部入仓）
            c.store.energy as i64,
            // 全槽和（官方 calcResources 对 store 全键求和——混合持有时
            // energy/G/命名矿物都计入超容判定，B4 评审 3 P1-1）
            kk_world::types::store_total(&c.store, &c.store_extra),
            store_capacity(c),
        ),
        None => return,
    };
    // 官方 harvest.js:31-34：controller 归他人**或被他人预定**拒——
    // `roomController.user && user != object.user ||
    //  roomController.reservation && reservation.user != object.user`
    if let Some(ctl) = w.controller()
        && (ctl.owner.as_deref().is_some_and(|u| u != owner)
            || ctl.reservation.as_ref().is_some_and(|r| r.user != owner))
    {
        return;
    }
    // 官方 harvest.js:37 `if (harvestAmount)`：零功率（无 WORK）整体跳过——
    // 不扣减、无 actionLog、无事件
    if work_power <= 0 {
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
    // 官方顺序（harvest.js:43-60）：energy **槽局部**入仓（:43
    // store.energy = (store.energy||0)+amount），超容判定用 calcResources
    // **全槽和**（:50），掉落量 = min(energy 槽持有, sum−cap)（:54）——只掉
    // 本 tick 产出的 energy，不动其他槽（P1-1 清偿 + 复审 F1 修正：原清偿
    // 误把全槽和写进 energy 槽 = 混持采 source 复制资源）
    let sum = held_total + amount;
    let energy_after = held_energy + amount;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy = energy_after.min(u32::MAX as i64) as u32;
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if sum > store_cap {
        let drop_amt = energy_after.min(sum - store_cap);
        if drop_amt > 0 {
            intent_drop_amount(w, id, drop_amt);
        }
    }
    w.push_event(
        c::EVENT_HARVEST,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "amount": amount }),
    );
}

/// 官方 harvest.js mineral 分支（67-112，B4）：矿量门（:69-71）、矿物同格
/// extractor 存在性（:73-77）、extractor 敌主拒/无主放行（:78-80）、
/// checkStructureAgainstController（:81-83——RCL 门，extractor 需 6 级）、
/// cooldown 拒（:84-86）、WORK×HARVEST_MINERAL_POWER（:88）、产出入矿种
/// 命名槽（:95）、超容按矿种 drop（:97-104）、extractor._cooldown=5（:108）。
/// 注意官方 mineral 分支**无** source 分支的 controller 归属/预定门（:31-34）。
fn intent_harvest_mineral(w: &mut Working, id: ObjId, target_id: ObjId) {
    let (owner, body, store_cap) = match w.creep(id) {
        Some(c) => (c.owner.clone(), c.body.clone(), store_capacity(c)),
        None => return,
    };
    let Some(RoomObject::Mineral(m)) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    // :69-71：矿量为 0 整条丢弃
    if m.mineral_amount == 0 {
        return;
    }
    // :73-77：矿物同格的 extractor（_.find 按坐标——index.at 同语义）
    let extractor_id = w
        .index
        .at(m.x as usize, m.y as usize)
        .iter()
        .copied()
        .find(|eid| matches!(w.room.objects.get(eid), Some(RoomObject::Extractor(_))));
    let Some(extractor_id) = extractor_id else {
        return;
    };
    let (ex_owner, ex_cooldown) = match w.room.objects.get(&extractor_id) {
        Some(RoomObject::Extractor(e)) => (e.owner.clone(), e.cooldown),
        _ => return,
    };
    // :78-80：敌主 extractor 拒（无主/自家放行）
    if ex_owner.as_deref().is_some_and(|u| u != owner) {
        return;
    }
    // :81-83：RCL 门（官方 utils.js:456-495 的 extractor 面——无主恒 active；
    // 有主须 controller level>=1 同主且配额非零；extractor 的 quota[8]===1
    // 走"唯一配额即本尊"分支，距离计数分支对该类型不可达 ⇒ RCL>=6 才 active）
    if !extractor_active(w, &ex_owner) {
        return;
    }
    // :84-86：冷却中拒
    if ex_cooldown != 0 {
        return;
    }
    // :88：HARVEST_MINERAL_POWER（1/WORK——boost 的 harvest 效果同一 BOOSTS 表）
    let work_power = kk_world::types::calc_body_effectiveness(
        &body,
        BodyKind::Work,
        "work",
        "harvest",
        c::HARVEST_MINERAL_POWER,
    );
    // :90 `if (harvestAmount)`：零功率整体跳过（不扣减、无 actionLog/事件）
    if work_power <= 0 {
        return;
    }
    let amount = (m.mineral_amount as i64).min(work_power);
    // :94：扣矿物本体
    if let Some(RoomObject::Mineral(t)) = w.room.objects.get_mut(&target_id) {
        t.mineral_amount -= amount as u32;
    }
    // :95：产出入矿种命名槽（bulk.update 的 _.merge 先改内存对象——:97 的
    // calcResources 读到含新产出的全槽和）；:97-104 超容按矿种 drop
    let held_total = match w.creep(id) {
        Some(c) => kk_world::types::store_total(&c.store, &c.store_extra),
        None => return,
    };
    // sum 全槽 + 新产出（官方 calcResources 含 _.merge 后的全部键——本矿种
    // 旧持有已在 store_total 内；混合持有超容按矿种 drop，P1-1 清偿）
    let sum = held_total + amount;
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, &m.mineral_type, amount);
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if sum > store_cap {
        intent_drop_amount_res(w, id, &m.mineral_type, sum - store_cap);
    }
    // :108：extractor._cooldown 瞬态（同 tick 的 extractor tick 持久化）
    w.extractor_cooldown_pending
        .insert(extractor_id, c::EXTRACTOR_COOLDOWN as u64);
    // :110：官方事件 amount 是**未钳制**的 harvestAmount（source 分支 :63 是
    // 钳后 amount——两分支的官方 quirk，原样保留）
    w.push_event(
        c::EVENT_HARVEST,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "amount": work_power }),
    );
}

/// 官方 checkStructureAgainstController（utils.js:456-495）的 extractor 面
/// （harvest.js:81 的门）：无主恒 active（:457-460）；有主须 controller
/// level>=1 且同主（:463-465）且 CONTROLLER_STRUCTURES.extractor[level]
/// 配额非零（:467-471——RCL<6 为 0）。:474-476 的 quota[8]===1 提前返回
/// 分支使距离计数段（:478-495）对 extractor 不可达。
fn extractor_active(w: &Working, ex_owner: &Option<String>) -> bool {
    let Some(owner) = ex_owner.as_deref() else {
        return true;
    };
    let Some(ctl) = w.controller() else {
        return false;
    };
    if ctl.level < 1 || ctl.owner.as_deref() != Some(owner) {
        return false;
    }
    super::controller_allowed("extractor", ctl.level as i64) > 0
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
    // progressTotal 为创建时定格的存值（官方 build.js:68 读存的 progressTotal；
    // 放置路径=裸成本、intent 建工地=成本×地形系数，见 ConstructionSiteObj）
    let total = site.progress_total as i64;
    let remaining = total.saturating_sub(site.progress as i64);
    // 官方两步（build.js:67-84，GPT 二轮复审 P1-1）：基础量（无强化计数 ×
    // BUILD_POWER）定耗能与效果点数 min(power, remaining, energy)；强化效果
    // 按增量数组（降序取前 effect 个、整体 floor、钳 remaining）；能量扣基础
    // 量、进度加强化量
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
    if effect <= 0 {
        return;
    }
    let new_progress = site.progress as i64 + boosted;
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
            // 判定）统一为 "owner"
            let data = serde_json::json!({
                "type": "structure",
                "structureType": other,
                "owner": site.owner,
                "hits": 1,
                // build.js:116：出生即 true
                "notifyWhenAttacked": true,
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
    // 官方两步（upgradeController.js:51-58，GPT 二轮复审 P1-1）：基础量
    // （无强化计数 × POWER）定耗能与效果点数；强化效果按增量数组
    // （UPGRADE_CONTROLLER_POWER=1 ⇒ 增量=(m-1)）
    let (pos, owner, energy, base_power, boost_incs) = match w.creep(id) {
        Some(c) if !c.spawning && c.store.energy > 0 => (
            (c.x, c.y),
            c.owner.clone(),
            c.store.energy as i64,
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::UPGRADE_CONTROLLER_POWER,
            kk_world::types::boost_increments(
                &c.body,
                BodyKind::Work,
                "work",
                "upgradeController",
                c::UPGRADE_CONTROLLER_POWER,
            ),
        ),
        _ => return,
    };
    let Some(RoomObject::Controller(_)) = w.room.objects.get(&target_id) else {
        return;
    };
    let (ctl_x, ctl_y, ctl_owner, level, progress, upgrade_blocked) =
        match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => (
                t.x,
                t.y,
                t.owner.clone(),
                t.level,
                t.progress,
                t.upgrade_blocked,
            ),
            _ => return,
        };
    if (ctl_x as i32 - pos.0 as i32).abs() > 3 || (ctl_y as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    if level == 0 || ctl_owner.as_deref() != Some(owner.as_str()) {
        return;
    }
    // 官方 upgradeController.js:25-27：`target.upgradeBlocked &&
    // upgradeBlocked > gameTime` 拒——attackController 的 1000 tick 封锁期
    //（B2 起生效；null → 0 > t 恒假，放行）
    if upgrade_blocked.unwrap_or(0) > w.time {
        return;
    }
    let mut effect = base_power.min(energy);
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
    // 官方两步尾段（:51-58）：强化效果（boosted）用于 progress/_upgraded/
    // gcl/事件量；能量扣基础量（:58 store.energy -= buildEffect）
    let boosted = kk_world::types::apply_boost(&boost_incs, effect, effect);
    let new_progress;
    if level < 8 {
        let next_level_progress = controller_levels(level);
        let downgrade_time = match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => t.downgrade_time,
            _ => None,
        };
        // 官方升级条件（upgradeController.js:63-66）：进位需 downgrade 窗口健康
        if progress + boosted as u64 >= next_level_progress
            && downgrade_time.is_some_and(|dt| {
                dt + c::CONTROLLER_DOWNGRADE_RESTORE as u64 >= w.time + controller_downgrade(level)
            })
        {
            new_progress = progress + boosted as u64 - next_level_progress;
            *w.upgraded.entry(target_id).or_insert(0) += boosted;
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
            new_progress = progress + boosted as u64;
            *w.upgraded.entry(target_id).or_insert(0) += boosted;
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
                t.progress = new_progress;
            }
        }
    } else {
        *w.upgraded.entry(target_id).or_insert(0) += boosted;
    }
    // gcl 计入玩家（官方 bulkUsers.inc(target.user, 'gcl', boostedEffect)）
    let player = w.players.entry(owner.clone()).or_default();
    player.gcl += boosted;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy -= effect as u32;
        set_action_log(&mut c.action_log, ACTION_UPGRADE);
    }
    w.push_event(
        c::EVENT_UPGRADE_CONTROLLER,
        id,
        serde_json::json!({ "amount": boosted, "energySpent": effect }),
    );
}

/// 官方 transfer.js：RESOURCES_ALL 白名单（:8-11）、邻接、容量双形态
/// （B4 起命名矿物槽经 creep/storage 的 store_extra 承载）。
fn intent_transfer(w: &mut Working, id: ObjId, data: &IntentData) {
    // 官方 transfer.js:8-11：`_.contains(C.RESOURCES_ALL, resourceType)` 白名单
    // + 持有不足/负数拒（官方白名单含 power——power 无进 creep 的建模路径，
    // 放行面与官方逐字一致、不可达性由 pickup 门保证，见 intent_pickup）
    let resource = data.resource_type.as_deref().unwrap_or("");
    if !c::RESOURCES_ALL.contains(&resource) {
        return;
    }
    let Some(amount_req) = data.amount else {
        return;
    };
    let (pos, held) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), creep_slot(c, resource)),
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
    // 容量按资源分岔（官方 utils.capacityForResource，utils.js:671-675）：
    // 通用仓（creep/storage）数值容量全槽共用；per-resource 商店（spawn/
    // extension 的 scr.energy）对非 energy 得 0 ⇒ `!targetCapacity` 拒
    let per_resource_cap = |base: i64| if resource == "energy" { base } else { 0 };
    let (target_cap, t_pos, t_stored) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Creep(t)) if !t.spawning => (
            store_capacity(t),
            (t.x, t.y),
            // 官方 transfer.js:32：通用容量目标按 calcResources 全槽求和
            kk_world::types::store_total(&t.store, &t.store_extra),
        ),
        Some(RoomObject::Spawn(t)) => (
            per_resource_cap(c::SPAWN_ENERGY_CAPACITY),
            (t.x, t.y),
            store_slot(&t.store, resource),
        ),
        Some(RoomObject::Extension(t)) => (
            per_resource_cap(extension_cap()),
            (t.x, t.y),
            store_slot(&t.store, resource),
        ),
        Some(RoomObject::Storage(t)) => (
            c::STORAGE_CAPACITY,
            (t.x, t.y),
            // 同 creep：全槽求和（GPT 二轮复审 P1-2/评审交叉确认）
            kk_world::types::store_total(&t.store, &t.store_extra),
        ),
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
        Some(RoomObject::Creep(t)) => slots_add(&mut t.store, &mut t.store_extra, resource, amount),
        Some(RoomObject::Spawn(t)) => store_slot_add(&mut t.store, resource, amount),
        Some(RoomObject::Extension(t)) => store_slot_add(&mut t.store, resource, amount),
        Some(RoomObject::Storage(t)) => {
            slots_add(&mut t.store, &mut t.store_extra, resource, amount)
        }
        _ => return,
    }
    if let Some(c) = w.creep_mut(id) {
        slots_sub(&mut c.store, &mut c.store_extra, resource, amount);
    }
    w.push_event(
        c::EVENT_TRANSFER,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// 官方 withdraw.js：RESOURCES_ALL 白名单、空容量、safeMode/rampart 保护
/// （rampart 变体随对应批；目标白名单 spawn/extension/storage——B3 登记，
/// tombstone/ruin 目标随轨 B 后续）。
fn intent_withdraw(w: &mut Working, id: ObjId, data: &IntentData) {
    // 官方 withdraw.js:8-11：`_.contains(C.RESOURCES_ALL, resourceType)` 白名单
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !c::RESOURCES_ALL.contains(&resource) {
        return;
    }
    let Some(target_id) = data.id else { return };
    // actor 的空容量按 store 全槽求和（官方 calcResources）——含命名矿物槽
    let (pos, cap, held) = match w.creep(id) {
        Some(c) if !c.spawning && store_capacity(c) > 0 => (
            (c.x, c.y),
            store_capacity(c),
            kk_world::types::store_total(&c.store, &c.store_extra),
        ),
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
    // 可提取对象：有 store 的结构（spawn/extension/storage）+ Tombstone/Ruin
    //（官方预检 creeps.js:507-515 显式接受两类目标，withdraw.js:21 按目标
    // store 扣账——GPT 复审 P1-2 清偿：原仅三结构，tombstone/ruin 提取
    // API 返回 OK 结算静默丢弃）；source 只能 harvest、掉落资源只能 pickup
    //（官方引擎层无 source/energy 的 withdraw 路径）。命名矿物槽一并可提
    let (t_pos, t_amount) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Spawn(t)) => ((t.x, t.y), store_slot(&t.store, resource)),
        Some(RoomObject::Extension(t)) => ((t.x, t.y), store_slot(&t.store, resource)),
        Some(RoomObject::Storage(t)) => (
            (t.x, t.y),
            kk_world::types::store_extra_slot(&t.store, &t.store_extra, resource),
        ),
        Some(RoomObject::Tombstone(t)) => (
            (t.x, t.y),
            kk_world::types::store_extra_slot(&t.store, &t.store_extra, resource),
        ),
        Some(RoomObject::Ruin(t)) => (
            (t.x, t.y),
            kk_world::types::store_extra_slot(&t.store, &t.store_extra, resource),
        ),
        _ => return,
    };
    if (t_pos.0 as i32 - pos.0 as i32).abs() > 1 || (t_pos.1 as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    amount = amount.min(t_amount);
    if amount <= 0 {
        return;
    }
    match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Spawn(t)) => store_slot_sub(&mut t.store, resource, amount),
        Some(RoomObject::Extension(t)) => store_slot_sub(&mut t.store, resource, amount),
        Some(RoomObject::Storage(t)) => {
            slots_sub(&mut t.store, &mut t.store_extra, resource, amount)
        }
        Some(RoomObject::Tombstone(t)) => {
            slots_sub(&mut t.store, &mut t.store_extra, resource, amount)
        }
        Some(RoomObject::Ruin(t)) => slots_sub(&mut t.store, &mut t.store_extra, resource, amount),
        _ => return,
    }
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, resource, amount);
    }
    // 官方 withdraw 的 EVENT_TRANSFER 反向（objectId=target, data.targetId=object）
    w.push_event(
        c::EVENT_TRANSFER,
        target_id,
        serde_json::json!({ "targetId": id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// 官方 drop.js：amount>0 落地为同格资源堆；持有不足整条拒绝（:13
/// `!(store[resourceType] >= intent.amount)` ⇒ return，不钳制部分掉落）。
fn intent_drop(w: &mut Working, id: ObjId, data: &IntentData) {
    // 官方 drop.js:10：`_.contains(C.RESOURCES_ALL, resourceType)` 白名单
    //（B4 起命名矿物可掉——creep 的 store_extra 槽；掉落对象经命名槽承载）
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !c::RESOURCES_ALL.contains(&resource) {
        return;
    }
    let Some(amount) = data.amount else { return };
    if amount <= 0 {
        return;
    }
    if w.creep(id)
        .is_none_or(|c| c.spawning || creep_slot(c, resource) < amount)
    {
        return;
    }
    intent_drop_amount_res(w, id, resource, amount);
}

/// 掉落能量（drop.js 落地路径；dismantle 回流超容与 harvest 超容共用）。
pub(super) fn intent_drop_amount(w: &mut Working, id: ObjId, amount: i64) {
    // 内部调用方（dismantle 回流/harvest 超容）恒为 energy 槽
    intent_drop_amount_res(w, id, "energy", amount);
}

/// 官方 drop.js 落地路径（:24 `_create-energy(amount, resourceType)`——
/// 同格 container 先承接 + 同格同资源并堆，B3 的 create_resource 全量同式；
/// 衰减对全资源同式（官方 energy/tick.js:12 按 resourceType 键通用）。
/// B4 起资源参数化到任意 RESOURCES_ALL 名（命名矿物掉落为命名堆）。
pub(super) fn intent_drop_amount_res(w: &mut Working, id: ObjId, resource: &str, amount: i64) {
    let (pos, held) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), creep_slot(c, resource)),
        _ => return,
    };
    // 钳制仅服务内部调用方（harvest 超容路径已保证 held ≥ amount）
    let amount = amount.min(held);
    if amount <= 0 {
        return;
    }
    if let Some(c) = w.creep_mut(id) {
        slots_sub(&mut c.store, &mut c.store_extra, resource, amount);
    }
    w.create_resource(pos.0, pos.1, amount, resource);
}

/// 官方 pickup.js：邻接 energy、余量钳制、目标清零移除。
fn intent_pickup(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // 官方 pickup.js:9：carry = calcResources(object)（全槽求和）——含命名
    // 矿物槽（GPT 二轮复审 P1-3 的全资源口径，B4 落地）
    let (pos, cap, held) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            store_capacity(c),
            kk_world::types::store_total(&c.store, &c.store_extra),
        ),
        _ => return,
    };
    if held >= cap {
        return;
    }
    let Some(RoomObject::Energy(target)) = w.room.objects.get(&target_id) else {
        return;
    };
    // 官方 pickup.js:23-30：按堆的 resourceType 入开放 store 对应键——B4 起
    // 命名矿物堆放行入槽（B3 的"矿物堆拒拾"登记随本批清偿）；power 堆维持
    // 拒绝（power 无建模槽位，随 power 批放开——B3 登记口径不变）。判定走
    // energy_resource_name 单源——code 与命名槽双态都折算后再放行
    let slot_resource = match super::energy_resource_name(target) {
        "power" => return,
        name => name.to_string(),
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
        slots_add(&mut c.store, &mut c.store_extra, &slot_resource, amount);
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
            kk_world::types::calc_body_effectiveness(
                &c.body,
                BodyKind::Attack,
                "attack",
                "attack",
                c::ATTACK_POWER,
            ),
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
    // 官方两步（repair.js:23-38，GPT 二轮复审 P1-1）：基础功率定 effect 与
    // cost；强化增量数组定 boostedEffect（加血量）
    let (pos, energy, repair_power, boost_incs) = match w.creep(id) {
        Some(c) if !c.spawning && c.store.energy > 0 => (
            (c.x, c.y),
            c.store.energy as i64,
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::REPAIR_POWER,
            kk_world::types::boost_increments(
                &c.body,
                BodyKind::Work,
                "work",
                "repair",
                c::REPAIR_POWER,
            ),
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
    // repair.js:28-38：增量数组按 effect 点数取、floor、钳缺口——零效应拒
    let boosted = kk_world::types::apply_boost(&boost_incs, effect, missing);
    if effect <= 0 {
        return;
    }
    // repair.js:44-51：加血（boosted 亦钳 hitsMax）、扣能按基础 cost
    damage::set_structure_hits(w, target_id, (hits + boosted).min(hits_max));
    if let Some(creep) = w.creep_mut(id) {
        creep.store.energy -= cost as u32;
        set_action_log(&mut creep.action_log, ACTION_REPAIR);
    }
    // repair.js:58-60（事件 amount = boostedEffect）
    w.push_event(
        c::EVENT_REPAIR,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": boosted,
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
            kk_world::types::calc_body_effectiveness(
                &c.body,
                BodyKind::Work,
                "work",
                "dismantle",
                c::DISMANTLE_POWER,
            ),
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

/// 官方 heal.js（ranged=false）/ rangedHeal.js（ranged=true）：射程 1/3、
/// creep 目标（自愈合法——无自指拒，对照 attack.js:18）、他人 safeMode 拒、
/// HEAL × HEAL_POWER(12)/RANGED_HEAL_POWER(4) 延迟施加（无能量消耗、无 boost）。
fn intent_heal(w: &mut Working, id: ObjId, data: &IntentData, ranged: bool) {
    let Some(target_id) = data.id else { return };
    // heal.js:10-15 / rangedHeal.js:10-15：spawning 拒；功率 = 活跃 HEAL ×
    // HEAL_POWER / RANGED_HEAL_POWER
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            // heal.js:26 / rangedHeal.js:26——效应键随分支（BOOSTS.heal 表的
            // heal/rangedHeal 两键乘数同值，官方两调用各自查键）
            if ranged {
                kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Heal,
                    "heal",
                    "rangedHeal",
                    c::RANGED_HEAL_POWER,
                )
            } else {
                kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Heal,
                    "heal",
                    "heal",
                    c::HEAL_POWER,
                )
            },
        ),
        _ => return,
    };
    // heal.js:17-19：目标必须是已出生的 creep（powerCreep 属 M4）；spawning 拒
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let RoomObject::Creep(t) = &target else {
        return;
    };
    if t.spawning {
        return;
    }
    let (tx, ty) = (t.x, t.y);
    // heal.js:21-23 射程 1 / rangedHeal.js:21-23 射程 3（Chebyshev）
    let range = if ranged { 3 } else { 1 };
    if (tx as i32 - pos.0 as i32).abs() > range || (ty as i32 - pos.1 as i32).abs() > range {
        return;
    }
    // heal.js:25-27：他人 safeMode 拒（与 attack 同形）
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    // heal.js:29：延迟施加（_healToApply——伤害通道的对称面，tick.js:126-128
    // 在死亡判定前回补，同 tick 与伤害互相抵消）
    *w.heal_to_apply.entry(target_id).or_insert(0) += power;
    // heal.js:31-32：actionLog heal/rangedHeal（官方两键分立，u8 位图并入
    // 同一位——见 ACTION_HEAL 注释）+ healed（自愈同体）
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_HEAL);
    }
    if let Some(t) = w.creep_mut(target_id) {
        set_action_log(&mut t.action_log, ACTION_HEALED);
    }
    // heal.js:34：事件无条件（0 治疗量也发——引擎层不做部件预检，与 attack 同形）
    w.push_event(
        c::EVENT_HEAL,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": power,
            "healType": if ranged {
                c::EVENT_HEAL_TYPE_RANGED
            } else {
                c::EVENT_HEAL_TYPE_MELEE
            },
        }),
    );
}

// ---- B2：controller 结算族（官方 processor/intents/creeps/ 同名文件） ----

/// controller 族公共预检：目标存在、是 controller、Chebyshev 邻接 ≤ 1
/// （claimController.js:15-21 / reserveController.js:19-24 /
/// attackController.js:15-20 / signController.js:14-18 /
/// generateSafeMode.js:11-16 五处同形）。返回目标 id。
fn controller_target(w: &Working, id: ObjId, data: &IntentData) -> Option<ObjId> {
    let target_id = data.id?;
    let (x, y) = w.creep(id).map(|c| (c.x as i32, c.y as i32))?;
    let target = w.room.objects.get(&target_id)?;
    if !matches!(target, RoomObject::Controller(_)) {
        return None;
    }
    let (tx, ty) = target.pos();
    if (tx as i32 - x).abs() > 1 || (ty as i32 - y).abs() > 1 {
        return None;
    }
    Some(target_id)
}

/// 官方 claimController.js：夺主。0 级无主（或仅被自己预定）的 controller，
/// 带存活 CLAIM 部件的 creep 邻接claim → level=1、owner 换人、progress 清零、
/// downgradeTime/reservation 清空。
fn intent_claim_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // claimController.js:8-13：非 creep 拒（分派层保证）、spawning 拒
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :15-21：目标 + 邻接（公共预检）
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (level, reservation) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.level, t.reservation.clone()),
        _ => return,
    };
    // :22-24：`target.bindUser && object.user != target.bindUser` 拒——bindUser
    // 在官方 OSS 引擎全库无写入点（仅 hosted 后端 backend-npm 读取），私有服
    // 世界不可达，不建模
    // :25-27：level > 0 拒（已占房不可夺，unclaim/降级才回 0 级）
    if level > 0 {
        return;
    }
    // :28-30：存活 CLAIM 部件数为零拒
    let claim_parts = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim))
        .unwrap_or(0);
    if claim_parts == 0 {
        return;
    }
    // :31-33：他人预定拒（自己的预定不拦，:52 落地时清掉）
    if reservation.is_some_and(|r| r.user != owner) {
        return;
    }
    // :34-43：GCL 门。shardAccess（:37-39）是 intershard 门槛，单分片恒有——
    // N/A。claimedRooms 读 user.rooms（官方 bulk addToSet 不改内存对象
    // （bulk.js:101-107）——读到上一 tick 定集，我方 PlayerState.rooms 同形）；
    // calcNeededGcl（utils.js:661-663）= GCL_MULTIPLY × (n-1)^GCL_POW
    let claimed_rooms = w.players.get(&owner).map(|p| p.rooms.len()).unwrap_or(0);
    let needed_gcl = c::GCL_MULTIPLY as f64 * (claimed_rooms as f64).powf(c::GCL_POW);
    let gcl = w.players.get(&owner).map(|p| p.gcl).unwrap_or(0);
    if (gcl as f64) < needed_gcl {
        return;
    }
    // :45-53：落地。addRoomToUser（:55）经 tick 末 rooms 重建承载
    // （settle/mod.rs PlayerState 文档）
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.level = 1;
        t.owner = Some(owner);
        // 重新占领清除"曾拥有"哨兵（claimController.js:45-53 只写 user）
        t.owner_cleared = false;
        t.progress = 0;
        t.downgrade_time = None;
        t.reservation = None;
    }
}

/// 官方 reserveController.js：无主房预定。CONTROLLER_RESERVE(1)/CLAIM 部件/
/// tick，上限 CONTROLLER_RESERVE_MAX(5000)（相对 gameTime 的滑动窗）。
fn intent_reserve_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // reserveController.js:8-13：spawning 拒；:15-17 shardAccess 单分片 N/A
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :19-24：目标 + 邻接
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (ctl_owner, reservation) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.owner.clone(), t.reservation.clone()),
        _ => return,
    };
    // :26-28：`target.user || (target.reservation && reservation.user !=
    // object.user)` 拒——已占房不可预定（房主自己也不行）；他人预定不可覆盖
    if ctl_owner.is_some() || reservation.as_ref().is_some_and(|r| r.user != owner) {
        return;
    }
    // :30-33：effect = 存活 CLAIM × CONTROLLER_RESERVE；零功率拒（无事件）
    let effect = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim) as i64 * c::CONTROLLER_RESERVE)
        .unwrap_or(0);
    if effect == 0 {
        return;
    }
    // :35-40：无预约先建 {user, endTime: gameTime+1}（仅内存值，未持久化）
    let base_end = match &reservation {
        Some(r) => r.end_time,
        None => w.time + 1,
    };
    // :43-45：滑窗上限：endTime + effect > gameTime + CONTROLLER_RESERVE_MAX 拒。
    // 官方在内存对象上先建了预约再判上限——被拒的"幻影预约"仅本 tick 后续
    // 处理器可见、永不入库；我方工作副本即持久层（D4-2 灰区），按整票拒绝
    // （不落任何预约状态）
    if base_end + effect as u64 > w.time + c::CONTROLLER_RESERVE_MAX as u64 {
        return;
    }
    // :47：actionLog.reserveController（官方独立键，位图 bit8）
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_RESERVE);
    }
    // :49-50：endTime += effect 并持久化
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.reservation = Some(kk_schema::room::Reservation {
            user: owner,
            end_time: base_end + effect as u64,
        });
    }
    // :52：EVENT_RESERVE_CONTROLLER {amount: effect}
    w.push_event(
        c::EVENT_RESERVE_CONTROLLER,
        id,
        serde_json::json!({ "amount": effect }),
    );
}

/// 官方 attackController.js：预定削减 + 占房 downgrade 挂钟后移 +
/// 升级封锁（CONTROLLER_ATTACK_BLOCKED_UPGRADE=1000 tick）。
fn intent_attack_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // attackController.js:8-13：spawning 拒
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :15-20：目标 + 邻接
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (ctl_owner, reservation, safe_mode, upgrade_blocked) = match w.room.objects.get(&target_id)
    {
        Some(RoomObject::Controller(t)) => (
            t.owner.clone(),
            t.reservation.clone(),
            t.safe_mode,
            t.upgrade_blocked,
        ),
        _ => return,
    };
    // :22-24：原始房（无主无预定）不可攻
    if ctl_owner.is_none() && reservation.is_none() {
        return;
    }
    // :25-28：`(roomController.user != object.user && safeMode > gameTime) ||
    // upgradeBlocked > gameTime` 拒——他人 safeMode 生效期不可攻；
    // upgradeBlocked 对任何攻击者（含房主自己）生效。roomController 即目标
    // （同房唯一 controller）
    if (ctl_owner.as_deref() != Some(owner.as_str()) && safe_mode > w.time)
        || upgrade_blocked.unwrap_or(0) > w.time
    {
        return;
    }
    // :29-31：EFFECT_INVULNERABILITY 免疫属 power 体系（M4）——N/A
    // 存活 CLAIM 部件（:34/:42 两分支共用）
    let claim_parts = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim) as i64)
        .unwrap_or(0);
    // :33-40：预定削减分支——effect = floor(CLAIM × CONTROLLER_RESERVE)
    //（整数常量下 floor 恒等）；endTime -= effect（官方 bulk update 为
    // merge，user 保留）
    if let Some(r) = reservation {
        let effect = claim_parts * c::CONTROLLER_RESERVE;
        if effect == 0 {
            return;
        }
        let new_end = r.end_time.saturating_sub(effect as u64);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
            t.reservation = Some(kk_schema::room::Reservation {
                user: r.user,
                end_time: new_end,
            });
        }
    }
    // :41-49：占房分支——effect = floor(CLAIM × CONTROLLER_CLAIM_DOWNGRADE)；
    // downgradeTime -= effect；_upgradeBlocked = gameTime + 1000（瞬态，
    // controller tick 持久化）。downgradeTime 未挂钟（新 claim 的房）时官方
    // null − effect 得负数 → 下 tick 降级链即刻触发；u64 饱和到 0 同 observable
    //（两者都 ≤ gameTime−1，触发条件同真）
    if ctl_owner.is_some() {
        let effect = claim_parts * c::CONTROLLER_CLAIM_DOWNGRADE;
        if effect == 0 {
            return;
        }
        let new_dt = match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => {
                t.downgrade_time.unwrap_or(0).saturating_sub(effect as u64)
            }
            _ => return,
        };
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
            t.downgrade_time = Some(new_dt);
        }
        w.upgrade_block_pending.insert(
            target_id,
            w.time + c::CONTROLLER_ATTACK_BLOCKED_UPGRADE as u64,
        );
    }
    // :50：actionLog.attack（与近战/dismantle 共用官方键）
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_ATTACK);
    }
    // :52：EVENT_ATTACK_CONTROLLER——官方事件无 data 键；我方事件封套恒带
    // data（push_event 形态），空对象差异属投影层可见的既有形态（登记报告）
    w.push_event(c::EVENT_ATTACK_CONTROLLER, id, serde_json::json!({}));
}

/// 官方 signController.js：签名落库 / 空签名清除。
fn intent_sign_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // signController.js:8-13：spawning 拒
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :14-18：目标 + 邻接
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    // :23-28：`intent.sign ? {user, text: intent.sign, time: gameTime,
    // datetime: Date.now()} : null`——userString 清洗下缺省与空串同为 ""
    // （falsy），都落清除。官方第四字段 datetime 为墙钟，确定性引擎不落盘
    //（schema Sign 文档；投影层合成——轨 A 接线）
    let sign = data
        .sign
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|text| kk_schema::room::Sign {
            user: owner,
            text: text.clone(),
            time: w.time,
            // 官方 signController.js:23：datetime 落一次 Date.now() 毫秒墙钟
            //（确定性引擎的墙钟面与官方同源——replay 不确定性官方同样存在）
            datetime: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            ),
        });
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.sign = sign;
    }
}

/// 官方 generateSafeMode.js：消耗 1000 ghodium 为 controller 充一枚
/// safeModeAvailable。激活（消耗 available 设 safeMode 时钟）是另一半——
/// controller 对象的 activateSafeMode intent（settle/controller.rs）。
fn intent_generate_safe_mode(w: &mut Working, id: ObjId, data: &IntentData) {
    // generateSafeMode.js:8-10：spawning 拒（本文件无 type 检查——分派层保证）
    let spawning = w.creep(id).is_some_and(|c| c.spawning);
    if spawning {
        return;
    }
    // :11-16：目标 + 邻接
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    // :19-21：`!object.store || !(store[RESOURCE_GHODIUM] >= SAFE_MODE_COST)`
    // 拒——store 缺失/ghodium 不足皆拒（无部分消耗）
    let ghodium = w.creep(id).map(|c| c.store.ghodium as i64).unwrap_or(0);
    if ghodium < c::SAFE_MODE_COST {
        return;
    }
    // :23-24：available += 1（官方 `(target.safeModeAvailable || 0) + 1`）；
    // store.G -= SAFE_MODE_COST
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.safe_mode_available = t.safe_mode_available.saturating_add(1);
    }
    if let Some(c) = w.creep_mut(id) {
        c.store.ghodium -= c::SAFE_MODE_COST as u32;
    }
}

// ---- A4-iv：移动杂项族执行器（官方 processor/intents/creeps/ 同名文件） ----

/// 官方 say.js：spawning 拒后落 actionLog.say。官方键携带 message（截 10）与
/// isPublic——u16 位图只落事件位（ACTION_SAY），文本载荷不可承载：prelude 的
/// `saying` getter 读不到消息文本（恒 undefined），与 healType 的 B1.1 登记
/// 同口径（schema 换正式 actionLog 形态时恢复）。message 的 isString 门
///（say.js:14-16）：清洗层 userString 后官方恒字符串（缺省折叠 ""），我方
/// None 按 "" 对齐。
fn intent_say(w: &mut Working, id: ObjId, data: &IntentData) {
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    let _message = data.message.as_deref().unwrap_or("");
    let _is_public = data.is_public.unwrap_or(false);
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_SAY);
    }
}

/// 官方 suicide.js：intent 相即死（_die.js，dropRate=CREEP_CORPSE_RATE、
/// violentDeath=false——B3 起 die_creep_with_rate 即官方 _die.js 全量，
/// TTL/暴亡/suicide 0.2、recycle 1.0 四路 dropRate 对位）。user=='2'
///（Source Keeper dropRate=0）不建模——NPC 不入 M2 世界。
/// 死亡发生在 intent 相（官方同位）；已入矩阵的 move 不再落格（官方 execute
/// 经 roomObjects forEach，死对象不被遍历——同形）。
/// **有意分歧（A4-iv 评审 1/3 双确认登记）**：官方后序处理器（claim/
/// upgrade/.../pull，creepActions 序 suicide 之后）持有死引用快照继续执行
/// 且对**其他对象**的效果落地（processor.js:241 先取引用、_die.js:12-13 只
/// delete 集合不清引用、各处理器无 actor 存活复检）——`suicide();
/// upgradeController(c)` 同 tick 官方升级照常生效。我方对象消亡后
/// w.creep(id)→None 全早退（对其他对象的效果也跳过）。实装需处理器持
/// pre-death 快照（侵入面大），真实 bot 罕见该组合——登记有意分歧。
fn intent_suicide(w: &mut Working, id: ObjId) {
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    die_creep(w, id);
}

/// 官方 pull.js：目标 creep 存在、已出生、邻接 → movement.addPulling（配对
/// _pull/_pulled，环链拒绝）。位移的另一半是被拉 creep 自己的 move(puller)
/// intent（move 的朝相邻 creep 形态，intent_move 的 id 分支）——仲裁期
/// canMove 对被拉者放行（免疲劳/MOVE）、其移动疲劳沿链记在拉者头上
///（Working::add_fatigue 的归链）。
fn intent_pull(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (x, y) = match w.creep(id) {
        Some(c) if !c.spawning => (c.x as i32, c.y as i32),
        _ => return,
    };
    let (tx, ty) = match w.creep(target_id) {
        Some(t) if !t.spawning => (t.x as i32, t.y as i32),
        _ => return,
    };
    // pull.js:16-18：邻接复核（API 层 isNearTo 的结算侧对照）
    if (tx - x).abs() > 1 || (ty - y).abs() > 1 {
        return;
    }
    w.movement.add_pulling(w.room, id, target_id);
}

/// 官方 creeps/tick.js（M1 子集）：移动落格、TTL 死亡、疲劳恢复。
/// 官方 creeps/tick.js（M1 子集）：孵化中的雏形自愈/出生（spawn 侧的对偶
/// 路径）、移动落格、ageTime 首触赋值、TTL 死亡、疲劳恢复。
pub fn creep_tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    // 移动落格（movement.js execute）
    movement_execute(w, id)?;
    if !w.room.objects.contains_key(&id) {
        return Ok(()); // 结算中被移除（不会发生于 M1 路径，防御）
    }
    let (spawning, age_time, x, y, name) = match w.creep(id) {
        Some(c) => (c.spawning, c.age_time, c.x, c.y, c.name.clone()),
        None => return Ok(()),
    };
    if spawning {
        // 官方 tick.js:10-27：孵化中的雏形——同格无 spawn（M2 无 invaderCore
        // 孵化）→ 移除；spawn 任务已清/换名 → 雏形侧出生（cancelSpawning 移除
        // 雏形、spawn 侧 spawn_tick 出生并清任务，正常路径不可达——防御对偶）。
        // 出生后**不**走 else 的 ageTime/TTL 分支（官方 if/else 互斥——出生
        // 雏形的寿命在下一 tick 赋值），但共用尾部的疲劳/伤害照常
        let spawn_id = w
            .index
            .at(x as usize, y as usize)
            .iter()
            .find(|tid| matches!(w.room.objects.get(tid), Some(RoomObject::Spawn(_))))
            .copied();
        match spawn_id {
            None => {
                w.remove_obj(id);
                return Ok(());
            }
            Some(sid) => {
                let mismatch = match w.room.objects.get(&sid) {
                    Some(RoomObject::Spawn(s)) => {
                        s.spawning.as_ref().map(|j| j.name.as_str()) != Some(name.as_str())
                    }
                    _ => true,
                };
                if mismatch {
                    let job_directions = match w.room.objects.get(&sid) {
                        Some(RoomObject::Spawn(s)) => s
                            .spawning
                            .as_ref()
                            .map(|j| j.directions.clone())
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    super::spawn::born_creep(w, sid, &name, &job_directions);
                }
            }
        }
    } else {
        // ageTime 首触赋值（官方 tick.js:88-91：孵化出生的雏形不带 ageTime——
        // CLAIM 部件 600、其余 1500；放置路径 place.rs 已显式赋值不走此分支）
        if age_time == 0 {
            let has_claim = w.creep(id).is_some_and(|c| {
                c.body
                    .iter()
                    .any(|p| BodyKind::from_code(p.kind) == Some(BodyKind::Claim))
            });
            let life = if has_claim {
                c::CREEP_CLAIM_LIFE_TIME
            } else {
                c::CREEP_LIFE_TIME
            } as u64;
            let born_at = w.time + life;
            if let Some(creep) = w.creep_mut(id) {
                creep.age_time = born_at;
            }
        }
        // TTL 死亡（creeps/tick.js:93-95 gameTime >= ageTime-1）
        if let Some(age_time) = w.creep(id).map(|c| c.age_time)
            && w.time >= age_time.saturating_sub(1)
        {
            die_creep(w, id);
            return Ok(());
        }
    }
    // 疲劳恢复（creeps/tick.js:105-108，两分支共用尾部）：-2 × 存活 MOVE 部件
    if let Some(c) = w.creep(id) {
        let moves =
            kk_world::types::calc_body_effectiveness(&c.body, BodyKind::Move, "move", "fatigue", 1);
        if moves > 0 {
            w.add_fatigue(id, -2 * moves);
        }
    }
    // 延迟伤害/治疗结算（creeps/tick.js:118-150 在疲劳恢复之后）：伤害先于
    // 治疗施加、死亡判定在两者之后——同 tick 等量互相抵消；归零 → 暴力死亡
    // （与 TTL 死亡同路径），否则重排部件分布并处理缩容掉落。官方对孵化中
    // 的雏形同样施加（攻击 intent 期照常入 _damageToApply）
    damage::apply_pending_hits(w, id);
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
    if !movement::can_move(creep, old_fatigue, w.movement.is_pulled(id)) {
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
    //（:224-233 的 safeMode 门：`!roomController || controller.user ===
    // object.user || !(safeMode > gameTime)` 才允许踩——他人 safeMode 生效期
    // 敌方工地受保护；M1 只做了工地归属半边，B2 补齐 safeMode 门）
    let stomp_allowed = w
        .controller()
        .is_none_or(|ctl| ctl.owner.as_deref() == Some(owner.as_str()) || ctl.safe_mode <= w.time);
    let mut fatigue_rate = 2i64;
    let mut road_id = None;
    let cell_ids: Vec<ObjId> = w.index.at(target.0 as usize, target.1 as usize).to_vec();
    for tid in cell_ids {
        match w.room.objects.get(&tid).cloned() {
            Some(RoomObject::Road(_)) => {
                road_id = Some(tid);
            }
            Some(RoomObject::ConstructionSite(site))
                // 官方 :224-233：safeMode 门放行时踩掉敌方工地（退一半工程为能量）
                if stomp_allowed && site.owner.as_deref() != Some(owner.as_str()) => {
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

/// creep 死亡（官方 _die.js 全量，B3）：移除本体 → 墓碑承接全部资源
/// （部件造价回收 + 携带 store，同格 container 先收）→ EVENT_OBJECT_DESTROYED。
/// 四条 M2 死因合流：TTL 到期（tick.js:93-95）、部件全灭的暴亡
/// （tick.js:134-136）、suicide（suicide.js:14）——官方均传 dropRate=undefined
/// （落 CREEP_CORPSE_RATE=0.2，走 [`die_creep`]）；recycleCreep
/// （recycle-creep.js:23）传 1.0（走 [`die_creep_with_rate`]）。
/// attackType 恒 undefined（墓碑抑制分支是核弹专属，M2 无核弹路径）。
///
/// 不建模面（官方 NPC/宿主体系，登记报告）：userSummoned/strongholdId 跳过
/// 回收的门（:38）；tombstoneDecay 覆盖键（:18——OSS 引擎全库无写入点）；
/// Source Keeper 的 dropRate=0（suicide.js:14 的 user=='2'）；stats.creepsLost
/// （:99-101 宿主统计面）。
pub fn die_creep(w: &mut Working, id: ObjId) {
    die_creep_with_rate(w, id, c::CREEP_CORPSE_RATE);
}

pub fn die_creep_with_rate(w: &mut Working, id: ObjId, drop_rate: f64) {
    let Some(c) = w.creep(id) else { return };
    let (x, y, owner, name, age_time, body, store, store_extra) = (
        c.x,
        c.y,
        c.owner.clone(),
        c.name.clone(),
        c.age_time,
        c.body.clone(),
        c.store.clone(),
        c.store_extra.clone(),
    );
    let time = w.time;
    // _die.js:10-11：先移除本体
    w.remove_obj(id);

    // _die.js:16-19：decayTime = body.length × TOMBSTONE_DECAY_PER_PART
    let decay_time = time + body.len() as u64 * c::TOMBSTONE_DECAY_PER_PART as u64;
    // processor.js:50：_ticksToLive = ageTime − gameTime（tick 首拍维护，死亡
    // 时点读到的即本值）
    let ticks_to_live = age_time.saturating_sub(time);

    // ---- 资源承接（_die.js:37-91；:38 `dropRate > 0` 门包住两循环——0 时
    // 跳过回收与携带承接，墓碑仍建（空 store）） ----
    // 墓碑 store 累计器（官方 tombstone.store 的内存对象）：energy/G 双槽 +
    // 额外资源列表；每次入账先经同格 container 承接（:45-59/:79-90 的两段
    // 同形分支）
    let mut tomb = TombStore::default();
    if drop_rate > 0.0 {
        // :40-41 lifeTime 按 body 是否含 CLAIM 部件（_.any 不看 hits——含损毁件）
        let life_time = if body.iter().any(|p| p.kind == BodyKind::Claim.code()) {
            c::CREEP_CLAIM_LIFE_TIME
        } else {
            c::CREEP_LIFE_TIME
        } as f64;
        // :42 lifeRate = dropRate × _ticksToLive / lifeTime
        let life_rate = drop_rate * ticks_to_live as f64 / life_time;

        // :43-53 部件造价回收：energy 起步累加；boosted 件额外产出 boost 矿物
        //（LAB_BOOST_MINERAL × lifeRate）与能量（LAB_BOOST_ENERGY × lifeRate）；
        // 每件能量 = min(CREEP_PART_MAX_ENERGY, BODYPART_COST × lifeRate)。
        // 官方键序 = bodyResources 的插入序（energy 先、矿物按 body 首现序）——
        // 仅 container 余量竞争时可观察，按同序遍历
        let mut energy: f64 = 0.0;
        let mut minerals: Vec<(String, f64)> = Vec::new();
        for part in &body {
            if let Some(boost) = &part.boost {
                match minerals.iter_mut().find(|(res, _)| res == boost) {
                    Some((_, amount)) => *amount += c::LAB_BOOST_MINERAL as f64 * life_rate,
                    None => minerals.push((boost.clone(), c::LAB_BOOST_MINERAL as f64 * life_rate)),
                }
                energy += c::LAB_BOOST_ENERGY as f64 * life_rate;
            }
            let part_cost = BodyKind::from_code(part.kind)
                .and_then(|k| {
                    c::BODYPART_COST
                        .iter()
                        .find(|(name, _)| *name == k.name())
                        .map(|(_, v)| *v)
                })
                .unwrap_or(0) as f64;
            energy += (c::CREEP_PART_MAX_ENERGY as f64).min(part_cost * life_rate);
        }
        // :61-76 第一循环（bodyResources：资源总额 floor 后逐键入账）
        tomb.add(w, x, y, "energy", energy.floor() as i64);
        for (res, amount) in minerals {
            tomb.add(w, x, y, &res, amount.floor() as i64);
        }
        // :78-91 第二循环（object.store 携带资源——官方开放 store 逐键循环；
        // B4 起含命名矿物槽的全集）
        tomb.add(w, x, y, "energy", store.energy as i64);
        tomb.add(
            w,
            x,
            y,
            kk_core::constants::RESOURCE_GHODIUM,
            store.ghodium as i64,
        );
        for stack in &store_extra {
            tomb.add(w, x, y, &stack.resource, stack.amount as i64);
        }
    }

    // _die.js:22-34/93：墓碑落库。creepSaying 官方取 actionLog.say 的公开
    // 遗言——位图 action_log 无文本载荷，恒 None（灰区表既有 say 登记的延伸）
    w.insert_new(RoomObject::Tombstone(kk_schema::room::TombstoneObj {
        id: 0,
        x,
        y,
        death_time: time,
        decay_time,
        creep_id: Some(id.0),
        creep_name: Some(name),
        creep_ticks_to_live: ticks_to_live.min(u32::MAX as u64) as u32,
        creep_body: body.iter().map(|p| p.kind).collect(),
        creep_saying: None,
        owner: Some(owner),
        store: tomb.store,
        store_extra: tomb.extra,
    }));

    // _die.js:97：死亡事件（墓碑先落、事件在后——官方同序）
    w.push_event(
        c::EVENT_OBJECT_DESTROYED,
        id,
        serde_json::json!({ "type": "creep" }),
    );
}

/// 墓碑 store 的入账累计器（官方 _die.js 的 tombstone.store 内存对象 +
/// 两段 container 承接分支的 Rust 形态）。
#[derive(Default)]
struct TombStore {
    store: kk_schema::room::Store,
    extra: Vec<kk_schema::room::ResourceStack>,
}

impl TombStore {
    /// 一次资源入账：同格 container 先承接（余量竞争按入账序——官方两循环
    /// 的 per-key 时序），余量进墓碑槽。amount ≤ 0 无事发生（官方 floor 后
    /// `if(amount > 0)` 门）。
    fn add(&mut self, w: &mut Working, x: u8, y: u8, resource: &str, amount: i64) {
        if amount <= 0 {
            return;
        }
        let leftover = w.deposit_into_container(x, y, resource, amount);
        if leftover <= 0 {
            return;
        }
        match resource {
            "energy" => {
                self.store.energy = self
                    .store
                    .energy
                    .saturating_add(u32::try_from(leftover).unwrap_or(u32::MAX))
            }
            kk_core::constants::RESOURCE_GHODIUM => {
                self.store.ghodium = self
                    .store
                    .ghodium
                    .saturating_add(u32::try_from(leftover).unwrap_or(u32::MAX))
            }
            other => {
                if let Some(slot) = self.extra.iter_mut().find(|s| s.resource == other) {
                    slot.amount = slot
                        .amount
                        .saturating_add(u32::try_from(leftover).unwrap_or(u32::MAX));
                } else {
                    self.extra.push(kk_schema::room::ResourceStack {
                        resource: other.to_string(),
                        amount: u32::try_from(leftover).unwrap_or(u32::MAX),
                    });
                }
            }
        }
    }
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
/// 官方 actionLog.heal（heal.js:31）。rangedHeal.js:31 的官方独立键
/// `rangedHeal` 在 u8 位图形态下并入本位（位图已满 8 位；远近程的玩家可见
/// 区分由 EVENT_HEAL 的 healType 字段承载——schema 换正式 actionLog 形态时
/// 恢复官方双键）。
pub const ACTION_HEAL: u8 = 1 << 6;
/// 官方 actionLog.healed（heal.js:32；renew-creep.js:42 也置此键）。
pub const ACTION_HEALED: u8 = 1 << 7;
/// 官方 actionLog.reserveController（reserveController.js:47）。u8 位图在 B1
/// 已满 8 位，B2 起 action_log 拓宽为 u16，本键落在 bit8。
pub const ACTION_RESERVE: u16 = 1 << 8;
/// 官方 actionLog.say（say.js:18-21）。官方键是带 message(截 10)/isPublic 的
/// 对象——位图只落事件位，文本载荷不可承载（intent_say 注释的灰区口径）。
pub const ACTION_SAY: u16 = 1 << 9;

pub fn set_action_log(bits: &mut u16, bit: impl Into<u16>) {
    *bits |= bit.into();
}

/// 官方 harvest 功率：WORK 部件数 × HARVEST_POWER（无 boost）。
pub fn harvest_power(creep: &kk_schema::room::CreepObj) -> i64 {
    kk_world::types::calc_body_effectiveness(
        &creep.body,
        BodyKind::Work,
        "work",
        "harvest",
        c::HARVEST_POWER,
    )
}

/// 官方 storeCapacity（M1 无 boost）：CARRY 部件数 × CARRY_CAPACITY。
/// Store 两槽（energy/G）的资源键访问——官方 store 对象按资源名为键
/// （GPT 复审 P1-1 清偿：transfer/withdraw/drop 参数化，schema 现两资源槽）。
fn store_slot(store: &kk_schema::room::Store, resource: &str) -> i64 {
    match resource {
        "energy" => store.energy as i64,
        "G" => store.ghodium as i64,
        _ => 0,
    }
}

fn store_slot_sub(store: &mut kk_schema::room::Store, resource: &str, amount: i64) {
    match resource {
        "energy" => store.energy = store.energy.saturating_sub(amount as u32),
        "G" => store.ghodium = store.ghodium.saturating_sub(amount as u32),
        _ => {}
    }
}

fn store_slot_add(store: &mut kk_schema::room::Store, resource: &str, amount: i64) {
    match resource {
        "energy" => store.energy = store.energy.saturating_add(amount as u32),
        "G" => store.ghodium = store.ghodium.saturating_add(amount as u32),
        _ => {}
    }
}

// ---- B4 多资源槽（creep/storage 的命名资源面：双槽 + store_extra 列表）----
// 官方 store 是按资源名键的开放对象（harvest.js:95/transfer.js:45 的
// `store[resourceType]` 任意键读写）；energy/G 走双槽字段，其余并入列表。

/// 读槽（无键 = 0——官方 store[res] 的 undefined 参与数值运算同值）。
fn creep_slot(c: &kk_schema::room::CreepObj, resource: &str) -> i64 {
    kk_world::types::store_extra_slot(&c.store, &c.store_extra, resource)
}

/// 写槽（加）：无键现场建（官方 `(store[res] || 0) + amount` 同式；饱和加
/// 是放置面可造超域堆的既有口径）。
fn slots_add(
    store: &mut kk_schema::room::Store,
    extra: &mut Vec<kk_schema::room::ResourceStack>,
    resource: &str,
    amount: i64,
) {
    if amount <= 0 {
        return;
    }
    match resource {
        "energy" => store.energy = store.energy.saturating_add(amount as u32),
        kk_core::constants::RESOURCE_GHODIUM => {
            store.ghodium = store.ghodium.saturating_add(amount as u32)
        }
        other => {
            if let Some(slot) = extra.iter_mut().find(|s| s.resource == other) {
                slot.amount = slot
                    .amount
                    .saturating_add(u32::try_from(amount).unwrap_or(u32::MAX));
            } else {
                extra.push(kk_schema::room::ResourceStack {
                    resource: other.to_string(),
                    amount: u32::try_from(amount).unwrap_or(u32::MAX),
                });
            }
        }
    }
}

/// 写槽（减）：饱和减（调用方已保证持有足额——官方直接减）。
fn slots_sub(
    store: &mut kk_schema::room::Store,
    extra: &mut [kk_schema::room::ResourceStack],
    resource: &str,
    amount: i64,
) {
    if amount <= 0 {
        return;
    }
    match resource {
        "energy" => store.energy = store.energy.saturating_sub(amount as u32),
        kk_core::constants::RESOURCE_GHODIUM => {
            store.ghodium = store.ghodium.saturating_sub(amount as u32)
        }
        other => {
            if let Some(slot) = extra.iter_mut().find(|s| s.resource == other) {
                slot.amount = slot
                    .amount
                    .saturating_sub(u32::try_from(amount).unwrap_or(u32::MAX));
            }
        }
    }
}

pub fn store_capacity(creep: &kk_schema::room::CreepObj) -> i64 {
    kk_world::types::calc_body_effectiveness(
        &creep.body,
        BodyKind::Carry,
        "carry",
        "capacity",
        c::CARRY_CAPACITY,
    )
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
