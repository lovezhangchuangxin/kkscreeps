//! 结算房间级工地、旗帜创建和移除，以及房主主动销毁结构。

use super::Working;
use super::movement::type_name_of;
use crate::intent::RoomIntent;
use kk_core::constants as c;
use kk_schema::room::{ConstructionSiteObj, FlagObj, RoomObject};
use kk_world::ObjId;

/// 按 removeFlag、createFlag、createConstructionSite、removeConstructionSite、destroyStructure
/// 的顺序执行；同种动作保持提交顺序，只处理当前房间的意图。
pub fn process_room_intents(w: &mut Working, player: &str, intents: &[RoomIntent]) {
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

fn room_intent_rank(i: &RoomIntent) -> u8 {
    match i {
        RoomIntent::RemoveFlag { .. } => 0,
        RoomIntent::CreateFlag { .. } => 1,
        RoomIntent::CreateConstructionSite { .. } => 2,
        RoomIntent::RemoveConstructionSite { .. } => 3,
        RoomIntent::DestroyStructure { .. } => 4,
    }
}

/// 房主可销毁可建造结构，但房内有敌方 creep 时拒绝，包括孵化中的 creep。
/// 销毁入口负责废墟和资源转移，此动作不发摧毁事件。
/// 来源：上游 engine/src/processor/intents/room/destroy-structure.js。
fn destroy_structure_intent(w: &mut Working, player: &str, id: kk_world::ObjId) {
    let Some(obj) = w.room.objects.get(&id).cloned() else {
        return;
    };
    let type_name = type_name_of(&obj);
    if kk_world::types::construction_cost(&type_name).is_none() {
        return;
    }
    let Some(ctl) = w.controller() else {
        return;
    };
    if ctl.owner.as_deref() != Some(player) {
        return;
    }
    let enemy_creep = w
        .room
        .objects
        .values()
        .any(|o| matches!(o, RoomObject::Creep(creep) if creep.owner != player));
    if enemy_creep {
        return;
    }
    super::damage::destroy_structure(w, id);
}

/// 检查类型、坐标、RCL 配额和格内兼容性，再创建进度为 0 的工地。
/// 当前数量上限只统计本房间中该玩家的工地；此处没有预定房间的归属检查。
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
    // link 的建造成本与配额取有效规则；其余类型查 CONSTRUCTION_COST
    // 与 CONTROLLER_STRUCTURES 官方表。
    let cost = if structure_type == "link" {
        w.plane.links.build_cost
    } else {
        match kk_world::types::construction_cost(structure_type) {
            Some(cost) => cost,
            None => return,
        }
    };
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
    if let Some(ctl) = w.controller() {
        if ctl.owner.as_deref().is_some_and(|u| u != player) {
            return;
        }
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
        let allowed = super::structures::allowed_for(w, structure_type, rcl);
        if existing >= allowed {
            return;
        }
    }
    let (ux, uy) = (x as u8, y as u8);
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
    if is_extractor && !(has_mineral && !has_extractor) {
        return;
    }
    if structure_type != "road"
        && structure_type != "container"
        && (x == 1 || x == 48 || y == 1 || y == 48)
    {
        // 四角同时命中 x/y 条件时，后面的 y 检查覆盖 x，只检查对应横边三格。
        // 不改成两边都检查；来源：上游 utils.js::checkConstructionSite。
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
    // 道路地形倍率在创建工地时计入总进度，后续建造读取保存值。
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

/// 保存本房间的玩家旗帜，名称中的 | 和 ~ 分别编码为 $VLINE$ 和 $TILDE$。
/// 重复名称检查限于当前房间，不是全世界。
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
        owner: Some(player.to_string()),
    }));
}

/// 按同一名称编码移除当前玩家的旗帜。
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

/// 工地主人或房主可移除工地；进度大于 1 时返还一半进度对应的能量，
/// 同格容器优先接收，剩余量进入掉落堆。
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
