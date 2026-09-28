//! 房间级指令：createConstructionSite / createFlag（官方 room/intents.js）。

use super::Working;
use super::movement::type_name_of;
use crate::intent::RoomIntent;
use kk_core::constants as c;
use kk_schema::room::{ConstructionSiteObj, FlagObj, RoomObject};

pub fn process_room_intents(w: &mut Working, player: &str, intents: &[RoomIntent]) {
    for intent in intents {
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
        }
    }
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
    // 对象面检查（checkConstructionSite objects 分支）
    for tid in w.index.at(ux as usize, uy as usize) {
        match w.room.objects.get(tid) {
            Some(RoomObject::ConstructionSite(_)) => return,
            Some(other) => {
                let tn = type_name_of(other);
                if tn == structure_type {
                    return;
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
    // 地形面检查（checkConstructionSite terrain 分支）：
    // 非 road/container 的 1/48 边界格要求相邻边界三格全墙；非 road 不可建墙上
    if structure_type != "road"
        && structure_type != "container"
        && (x == 1 || x == 48 || y == 1 || y == 48)
    {
        let border: [(usize, usize); 3] = if x == 1 {
            [
                (0, (y - 1) as usize),
                (0, y as usize),
                (0, (y + 1) as usize),
            ]
        } else if x == 48 {
            [
                (49, (y - 1) as usize),
                (49, y as usize),
                (49, (y + 1) as usize),
            ]
        } else if y == 1 {
            [
                ((x - 1) as usize, 0),
                (x as usize, 0),
                ((x + 1) as usize, 0),
            ]
        } else {
            [
                ((x - 1) as usize, 49),
                (x as usize, 49),
                ((x + 1) as usize, 49),
            ]
        };
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
    if structure_type != "road"
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
    if w.room.objects.values().any(|o| match o {
        RoomObject::Flag(f) => f.name == name,
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
    }));
    let _ = player; // M1 flag 无 owner 字段（官方 flags 集合有 user；灰区登记）
}
