//! 房间级指令：createConstructionSite / createFlag / destroyStructure
//! （官方 room/intents.js 的分派序：removeFlag → createFlag →
//! createConstructionSite → removeConstructionSite → destroyStructure）。

use super::Working;
use super::movement::type_name_of;
use crate::intent::RoomIntent;
use kk_core::constants as c;
use kk_schema::room::{ConstructionSiteObj, FlagObj, RoomObject};
use kk_world::ObjId;

pub fn process_room_intents(w: &mut Working, player: &str, intents: &[RoomIntent]) {
    // 官方 room/intents.js 按名分组固定序处理（头注）；我方 Vec 为 push 序，
    // 稳定排序对齐组序（同组内保持 push 序——官方 forEach 数组序同构）
    let mut ordered: Vec<&RoomIntent> = intents.iter().collect();
    ordered.sort_by_key(|i| room_intent_rank(i));
    for intent in ordered {
        match intent {
            RoomIntent::CreateConstructionSite {
                room,
                x,
                y,
                structure_type,
                name,
            } => {
                if room == &w.room.name {
                    create_construction_site(w, player, *x, *y, structure_type, name.clone());
                }
            }
            RoomIntent::CreateFlag {
                room,
                x,
                y,
                name,
                color,
                secondary_color,
            } => {
                if room == &w.room.name {
                    create_flag(w, player, *x, *y, name, *color, *secondary_color);
                }
            }
            RoomIntent::DestroyStructure { room, id } => {
                if room == &w.room.name {
                    destroy_structure_intent(w, player, *id);
                }
            }
            RoomIntent::RemoveFlag { room, name } => {
                if room == &w.room.name {
                    remove_flag(w, player, name);
                }
            }
            RoomIntent::RemoveConstructionSite { room, id } => {
                if room == &w.room.name {
                    remove_construction_site(w, player, *id);
                }
            }
        }
    }
}

/// 官方 room/intents.js 的组处理序（头注）：removeFlag → createFlag →
/// createConstructionSite → removeConstructionSite → destroyStructure
/// （genEnergy 不在我方面）。
fn room_intent_rank(i: &RoomIntent) -> u8 {
    match i {
        RoomIntent::RemoveFlag { .. } => 0,
        RoomIntent::CreateFlag { .. } => 1,
        RoomIntent::CreateConstructionSite { .. } => 2,
        RoomIntent::RemoveConstructionSite { .. } => 3,
        RoomIntent::DestroyStructure { .. } => 4,
    }
}

/// 官方 room/destroy-structure.js（structure.destroy() 的结算侧，B3）：
/// 可建类型、controller 归本人、房内无敌方 creep/powerCreep → _destroy
/// （建废墟）。新手墙分支（:16 `type == WALL && decayTime && !user`）——
/// 新手墙体系不建模，N/A。
///
/// 事件（OSS/套件分歧，登记报告）：OSS 4.3.2 的 destroy-structure.js 与
/// _destroy.js 均不 push EVENT_OBJECT_DESTROYED（事件只在 _damage.js:51）；
/// vanilla 与 screeps-ok ROOM-EVENTLOG-027 期望 destroy() 产事件——以套件
/// 为准，在销毁后补 push（objectId = 被毁结构、data.type = structureType）。
fn destroy_structure_intent(w: &mut Working, player: &str, id: kk_world::ObjId) {
    let Some(obj) = w.room.objects.get(&id).cloned() else {
        return;
    };
    // :12：`!object || !C.CONSTRUCTION_COST[object.type]` 拒
    let type_name = type_name_of(&obj);
    if kk_world::types::construction_cost(&type_name).is_none() {
        return;
    }
    // :14：`!roomController || roomController.user != userId` 拒
    let Some(ctl) = w.controller() else {
        return;
    };
    if ctl.owner.as_deref() != Some(player) {
        return;
    }
    // :18：房内存在他人 creep/powerCreep 拒（powerCreep M4 不建模）
    let enemy_creep = w
        .room
        .objects
        .values()
        .any(|o| matches!(o, RoomObject::Creep(creep) if creep.owner != player));
    if enemy_creep {
        return;
    }
    // 事件（GPT 复审 P1-4 清偿——B3 曾按套件口径补 push，新证据推翻）：
    // OSS destroy-structure.js 全文无 eventLog.push（事件只在 _damage.js:51
    // 与 _die.js:97）；**套件自己的 vanilla parity.json:10-12 承认 stable
    // vanilla（官方引擎）不产事件**，其 expected 是从 API 文档语义推断的
    // canonical claim——按硬约束①对齐官方移除。ROOM-EVENTLOG-027 转 red
    // 经 ratchet-allow.json 显式豁免留痕
    super::damage::destroy_structure(w, id);
}

/// 官方 room/create-construction-site.js：边界、成本存在性、控制器归属、
/// RCL 配额、对象/地形双重检查；路费按地形系数。
fn create_construction_site(
    w: &mut Working,
    player: &str,
    x: i64,
    y: i64,
    structure_type: &str,
    name: Option<String>,
) {
    if x <= 0 || x >= 49 || y <= 0 || y >= 49 {
        return;
    }
    let Some(cost) = kk_world::types::construction_cost(structure_type) else {
        return;
    };
    // 总量帽（纵深，非官方结算语义）：官方 MAX_CONSTRUCTION_SITES=100 的检查
    // 在 game 层（rooms.js:1071-1073，按玩家全对象计数 + 本 tick 已建数——官方
    // 分开数是因其快照不随建增长）；我们的工作副本即时插入，现存数已覆盖
    // 本 tick 新建。按（玩家,房间）近似（跨房总量随多房引擎，灰区登记）
    let existing_sites = w
        .room
        .objects
        .values()
        .filter(
            |o| matches!(o, RoomObject::ConstructionSite(s) if s.owner.as_deref() == Some(player)),
        )
        .count();
    if existing_sites >= c::MAX_CONSTRUCTION_SITES as usize {
        return;
    }
    // 官方 W/E 房间分支：控制器非本人/非本预约拒 + RCL 配额
    if let Some(ctl) = w.controller() {
        if ctl.owner.as_deref().is_some_and(|u| u != player) {
            return;
        }
        // checkControllerAvailability（utils.js:340-354）：现存同类型（含工地）< 配额
        let rcl = ctl.level as i64;
        let existing = w
            .room
            .objects
            .values()
            .filter(|o| match o {
                RoomObject::ConstructionSite(s) => s.structure_type == structure_type,
                other => type_name_of(other) == structure_type,
            })
            .count() as i64;
        let allowed = super::controller_allowed(structure_type, rcl);
        if existing >= allowed {
            return;
        }
    }
    let (ux, uy) = (x as u8, y as u8);
    // 对象面检查（checkConstructionSite objects 分支）：extractor 在互斥
    // 检查前提前返回（官方 utils.js:178-181——需同格矿物且无既有 extractor，
    // 不受造价对象互斥约束；M2B P2 清偿补齐矿物门）
    let is_extractor = structure_type == "extractor";
    let (mut has_mineral, mut has_extractor) = (false, false);
    for tid in w.index.at(ux as usize, uy as usize) {
        match w.room.objects.get(tid) {
            Some(RoomObject::ConstructionSite(_)) => return,
            Some(other) => {
                let tn = type_name_of(other);
                if tn == structure_type {
                    return;
                }
                if tn == "mineral" {
                    has_mineral = true;
                }
                if tn == "extractor" {
                    has_extractor = true;
                }
                if is_extractor {
                    continue;
                }
                // 非 rampart/road 的成本对象互斥
                if structure_type != "rampart"
                    && structure_type != "road"
                    && tn != "rampart"
                    && tn != "road"
                    && kk_world::types::construction_cost(&tn).is_some()
                {
                    return;
                }
            }
            None => {}
        }
    }
    // 官方 :178-181：extractor → 矿物在场且无既有 extractor（has_extractor
    // 与同类型拦截语义重复，官方两查并存——逐字保留）
    if is_extractor && !(has_mineral && !has_extractor) {
        return;
    }
    // 地形面检查（checkConstructionSite terrain 分支）：
    // 非 road/container 的 1/48 边界格要求相邻边界三格全墙（官方 :132-135
    // 顺序赋值——角格 y 边后写胜出，M2B P2 清偿对齐）；extractor 恒过
    //（官方 :149-150 免墙检）；非 road 不可建墙上
    if structure_type != "road"
        && structure_type != "container"
        && (x == 1 || x == 48 || y == 1 || y == 48)
    {
        let mut border: Option<[(usize, usize); 3]> = None;
        if x == 1 {
            border = Some([
                (0, (y - 1) as usize),
                (0, y as usize),
                (0, (y + 1) as usize),
            ]);
        }
        if x == 48 {
            border = Some([
                (49, (y - 1) as usize),
                (49, y as usize),
                (49, (y + 1) as usize),
            ]);
        }
        if y == 1 {
            border = Some([
                ((x - 1) as usize, 0),
                (x as usize, 0),
                ((x + 1) as usize, 0),
            ]);
        }
        if y == 48 {
            border = Some([
                ((x - 1) as usize, 49),
                (x as usize, 49),
                ((x + 1) as usize, 49),
            ]);
        }
        if let Some(border) = border {
            for (bx, by) in border {
                if !w
                    .room
                    .terrain
                    .check_mask(bx, by, kk_core::TERRAIN_MASK_WALL)
                {
                    return;
                }
            }
        }
    }
    if structure_type != "road"
        && structure_type != "extractor"
        && w.room
            .terrain
            .check_mask(ux as usize, uy as usize, kk_core::TERRAIN_MASK_WALL)
    {
        return;
    }
    // progressTotal 定格存值（官方 create-construction-site.js:38-48：road 在
    // 沼泽 ×5、墙 ×150；其余类型裸成本）——工地创建后地形再变不改变完成线
    let mut progress_total = cost as u64;
    if structure_type == "road" {
        if w.room
            .terrain
            .check_mask(ux as usize, uy as usize, kk_core::TERRAIN_MASK_SWAMP)
        {
            progress_total *= c::CONSTRUCTION_COST_ROAD_SWAMP_RATIO as u64;
        }
        if w.room
            .terrain
            .check_mask(ux as usize, uy as usize, kk_core::TERRAIN_MASK_WALL)
        {
            progress_total *= c::CONSTRUCTION_COST_ROAD_WALL_RATIO as u64;
        }
    }
    w.insert_new(RoomObject::ConstructionSite(ConstructionSiteObj {
        id: 0,
        x: ux,
        y: uy,
        structure_type: structure_type.to_string(),
        progress: 0,
        progress_total: progress_total.min(u32::MAX as u64) as u32,
        owner: Some(player.to_string()),
        name: if structure_type == "spawn" {
            name
        } else {
            None
        },
    }));
}

/// 官方 room/create-flag.js：重名拒、颜色白名单、边界。
fn create_flag(
    w: &mut Working,
    player: &str,
    x: i64,
    y: i64,
    name: &str,
    color: i64,
    secondary_color: i64,
) {
    let name = name.replace('|', "$VLINE$").replace('~', "$TILDE$");
    // 官方 create-flag.js:11-14：重名判**按玩家**（flags 私有集合 per-user——
    // 两玩家可各有同名旗）
    if w.room.objects.values().any(|o| match o {
        RoomObject::Flag(f) => f.name == name && f.owner.as_deref() == Some(player),
        _ => false,
    }) {
        return;
    }
    let colors_ok = (1..=10).contains(&color) && (1..=10).contains(&secondary_color);
    if !colors_ok {
        return;
    }
    if !(0..=49).contains(&x) || !(0..=49).contains(&y) {
        return;
    }
    w.insert_new(RoomObject::Flag(FlagObj {
        id: 0,
        x: x as u8,
        y: y as u8,
        name,
        color: color as u8,
        secondary_color: secondary_color as u8,
        // M2B 扩面 P2：官方 flags 集合带 user（旗按玩家私有）
        owner: Some(player.to_string()),
    }));
}

/// 官方 room/remove-flag.js：按**玩家**旗名删（flags 集合按 user 定位、名不
/// 在其集合内静默返回——他人同名旗不可删、不存在不可删）。
fn remove_flag(w: &mut Working, player: &str, name: &str) {
    let name = name.replace('|', "$VLINE$").replace('~', "$TILDE$");
    let target = w.room.objects.values().find_map(|o| match o {
        RoomObject::Flag(f) if f.name == name && f.owner.as_deref() == Some(player) => Some(f.id),
        _ => None,
    });
    if let Some(id) = target {
        w.remove_obj(ObjId(id));
    }
}

/// 官方 room/remove-construction-site.js：对象存在且为工地；所有权门
///（site.user == player ∨ 房 controller.user == player，:6）；移除 +
/// progress>1 时 `floor(progress/2)` 能量掉落在原格（:8-10）。
fn remove_construction_site(w: &mut Working, player: &str, id: ObjId) {
    let Some(obj) = w.room.objects.get(&id).cloned() else {
        return;
    };
    let RoomObject::ConstructionSite(site) = &obj else {
        return;
    };
    let controller_owner = w.controller().and_then(|ctl| ctl.owner.clone());
    if site.owner.as_deref() != Some(player) && controller_owner.as_deref() != Some(player) {
        return;
    }
    let (x, y, progress) = (site.x, site.y, site.progress);
    w.remove_obj(id);
    if progress > 1 {
        w.create_energy(x, y, (progress / 2) as i64);
    }
}
