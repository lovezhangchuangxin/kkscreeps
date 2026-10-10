//! portal 结算：衰变移除与同 shard 传送。
//!
//! 官方语义（来源：上游 engine/src/processor/intents/portals/tick.js 与
//! creeps/tick.js:80-85、power-creeps/tick.js:44-47、processor/global.js:33-43）：
//! 传送是同 tick 两段式——房间处理阶段的 creep tick（移动执行后）以
//! `if(object.ageTime)` 真值门检测同格 portal 并标记 `interRoom`；同 tick
//! 靠后的 global 阶段把对象直写为 destination 的精确坐标（无占位检查、
//! 可叠格），不发 EVENT_EXIT、不清疲劳。目的房不可达（官方
//! accessibleRooms 未含）时保留标记逐 tick 重试——本仓无标记态，每 tick
//! 重扫同格等价。powerCreep 仅同 shard portal 传送；跨 shard 传送官方 OSS
//! 无实现（专有后端），本仓跳过（登记分歧）。
//!
//! 衰变移除门为严格 `gameTime > decayTime`（玩家可读出 0 与 -1——套件
//! PORTAL-006 钉住 [2,1,0,-1,null]；xxscreeps 提前到 decayTime-1 移除是
//! 分歧，不采），直接删除无 ruin 无泼洒，并连带移除 (x+1,y+1) 处的
//! constructedWall（官方 worldgen 的 portal 配墙产物，在场才移除）。

use kk_core::RoomName;
use kk_schema::room::RoomObject;
use kk_world::domain::room_status::RoomStatusKind;
use kk_world::{ObjId, RoomState};

use super::Working;
use super::inter_room::InterRoomMove;

/// portal 对象 tick：到期移除（含配墙）。其它结构类型静默跳过。
pub fn portal_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let data: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
    if data.get("structureType").and_then(|v| v.as_str()) != Some("portal") {
        return;
    }
    // 官方 portals/tick.js 的 `decayTime &&` 真值门：decayTime=0 与缺省
    // 同义（永不移除），且严格 time > decayTime 才到期。
    let expired = data
        .get("decayTime")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|d| d > 0 && w.time > d);
    if !expired {
        return;
    }
    let (x, y) = (g.x, g.y);
    w.remove_obj(id);
    // 官方 portals/tick.js:18-21：连带移除 (x+1,y+1) 的 constructedWall。
    // 越界（49 边）静默跳过；找不到墙（管理放置通常无配墙）同样跳过。
    if x < 49 && y < 49 {
        let wall = w.room.objects.iter().find_map(|(wid, obj)| match obj {
            RoomObject::ConstructedWall(wall) if wall.x == x + 1 && wall.y == y + 1 => Some(*wid),
            _ => None,
        });
        if let Some(wall) = wall {
            w.remove_obj(wall);
        }
    }
}

/// 解析 portal 数据的 destination；非 portal 对象返回 None。
fn portal_destination(data: &serde_json::Value) -> Option<serde_json::Value> {
    if data.get("structureType").and_then(|v| v.as_str()) != Some("portal") {
        return None;
    }
    data.get("destination").cloned()
}

/// 全房结算后收集同格传送：返回待应用的跨房移动（复用边缘过渡的
/// 应用机制——effects/extensions/记忆随对象迁移）。
///
/// 官方对 creep 的 `ageTime` 真值门：出生拍尚未初始化寿命的 creep
///（kk 的 age_time=0）不传送，下一拍起传送——放置即站门格的对象首拍
/// 豁免与官方 NPC 首拍同义。powerCreep（Generic 承载）无此门，仅同
/// shard destination 传送。跨 shard destination 跳过（官方 OSS 无实现）。
pub fn collect_portal_transports(
    rooms: &[(String, RoomState)],
    fresh_age: &std::collections::BTreeSet<ObjId>,
    now_ms: u64,
) -> Vec<InterRoomMove> {
    let mut moves = Vec::new();
    // 官方 `_.find` 对同格多 portal 只取对象表序（_id 升序）首个——每对象
    // 至多一条移动，避免同拍被第二个 portal 再次搬走。
    let mut transported = std::collections::BTreeSet::new();
    for (_, room) in rooms {
        for obj in room.objects.values() {
            let RoomObject::Generic(g) = obj else {
                continue;
            };
            // 前置字面量门（含 \u 转义藏匿兜底，见
            // kk_world::domain::literal_gate）：structureType 值为 portal
            // 的载荷必命中；误命中只多一次解析，结果不变。
            if !kk_world::domain::literal_gate(&g.data, "\"portal\"") {
                continue;
            }
            let Ok(data) = serde_json::from_str::<serde_json::Value>(&g.data) else {
                continue;
            };
            let Some(destination) = portal_destination(&data) else {
                continue;
            };
            // 跨 shard destination 不传送（登记分歧：官方专有后端实现）。
            if destination.get("shard").is_some() {
                continue;
            }
            let (Some(dx), Some(dy), Some(droom)) = (
                destination.get("x").and_then(|v| v.as_u64()),
                destination.get("y").and_then(|v| v.as_u64()),
                destination.get("room").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let (Some(dx), Some(dy)) = (u8::try_from(dx).ok(), u8::try_from(dy).ok()) else {
                continue;
            };
            if RoomName::parse(droom).is_err() {
                continue;
            }
            // 官方 accessibleRooms 不含 status=closed 房（novice/respawn
            // 的 db status 保持 normal，仍可达）：closed 目的房不传送，
            // 对象留原地逐拍重试（目的房未注册同理由应用层保持原位）。
            let dest_closed = rooms.iter().any(|(name, room)| {
                name == droom
                    && room
                        .room_status
                        .as_ref()
                        .is_some_and(|s| s.active_kind(now_ms) == Some(RoomStatusKind::Closed))
            });
            if dest_closed {
                continue;
            }
            for (id, other) in &room.objects {
                if transported.contains(id) {
                    continue;
                }
                let transportable = match other {
                    // 官方门 `if(object.ageTime)` 在寿命补设之前求值：
                    // 结算中刚从 0 初始化的对象（fresh_age）当拍豁免。
                    RoomObject::Creep(c) => c.age_time != 0 && !fresh_age.contains(id),
                    RoomObject::Generic(g2) => {
                        kk_world::domain::generic::type_name(&g2.data).as_deref()
                            == Some("powerCreep")
                    }
                    _ => false,
                };
                if transportable && other.pos() == (g.x, g.y) {
                    transported.insert(*id);
                    moves.push(InterRoomMove {
                        id: *id,
                        to_room: droom.to_string(),
                        x: dx,
                        y: dy,
                    });
                }
            }
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_schema::room::{ConstructedWallObj, GenericObj};

    fn portal_room(portal_data: serde_json::Value) -> (String, RoomState) {
        let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
        room.insert_with_id(
            1,
            RoomObject::Generic(GenericObj {
                id: 1,
                x: 25,
                y: 25,
                data: portal_data.to_string(),
            }),
        )
        .unwrap();
        ("W1N1".into(), room)
    }

    fn creep(id: u64, x: u8, y: u8, age_time: u64) -> RoomObject {
        RoomObject::Creep(kk_schema::room::CreepObj {
            id,
            x,
            y,
            fatigue: 0,
            age_time,
            hits: 100,
            spawning: false,
            action_log: 0,
            name: format!("c{id}"),
            owner: "p1".to_string(),
            body: vec![],
            store: kk_schema::room::Store::default(),
            store_extra: Vec::new(),
            say_message: None,
            say_public: None,
            notify_when_attacked: None,
        })
    }

    #[test]
    fn collect_transports_same_tile_creep_and_power_creep() {
        let portal = serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "room": "W2N1", "x": 10, "y": 10 }
        });
        let (name, mut room) = portal_room(portal);
        room.insert_with_id(2, creep(2, 25, 25, 100)).unwrap();
        // 出生拍寿命未初始化的 creep 首拍豁免（官方 ageTime 真值门）。
        room.insert_with_id(3, creep(3, 25, 25, 0)).unwrap();
        // 同格第二只活 creep 同拍各自传送（每对象恰一条移动）。
        room.insert_with_id(5, creep(5, 25, 25, 100)).unwrap();
        let pc = RoomObject::Generic(GenericObj {
            id: 4,
            x: 25,
            y: 25,
            data: r#"{"type":"powerCreep"}"#.into(),
        });
        room.insert_with_id(4, pc).unwrap();
        let rooms = vec![(name, room)];
        let moves = collect_portal_transports(&rooms, &Default::default(), 0);
        assert_eq!(moves.len(), 3, "两只活 creep 与 powerCreep 各一条");
        assert!(moves.contains(&InterRoomMove {
            id: ObjId(2),
            to_room: "W2N1".into(),
            x: 10,
            y: 10
        }));
        assert!(moves.contains(&InterRoomMove {
            id: ObjId(4),
            to_room: "W2N1".into(),
            x: 10,
            y: 10
        }));
    }

    #[test]
    fn collect_picks_first_portal_for_stacked_portals() {
        // 同格两 portal：对象表序（ID 升序）首个生效，creep 不被二次搬走。
        let (_, mut room) = portal_room(serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "room": "W3N1", "x": 1, "y": 1 }
        }));
        room.insert_with_id(
            9,
            RoomObject::Generic(GenericObj {
                id: 9,
                x: 25,
                y: 25,
                data: r#"{"type":"structure","structureType":"portal","destination":{"room":"W4N1","x":2,"y":2}}"#.into(),
            }),
        )
        .unwrap();
        room.insert_with_id(2, creep(2, 25, 25, 100)).unwrap();
        let rooms = vec![("W1N1".into(), room)];
        let moves = collect_portal_transports(&rooms, &Default::default(), 0);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to_room, "W3N1", "ID 序首个 portal（1 < 9）生效");
    }

    #[test]
    fn collect_skips_cross_shard_and_off_tile() {
        let cross = serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "shard": "shard1", "room": "W5N5" }
        });
        let (name, mut room) = portal_room(cross);
        room.insert_with_id(2, creep(2, 25, 25, 100)).unwrap();
        room.insert_with_id(3, creep(3, 10, 10, 100)).unwrap();
        let rooms = vec![(name, room)];
        assert!(collect_portal_transports(&rooms, &Default::default(), 0).is_empty());
    }

    #[test]
    fn collect_exempts_fresh_age_creeps() {
        // 本拍刚从 0 初始化寿命的 creep：官方门在补设前求值，当拍豁免。
        let (name, mut room) = portal_room(serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "room": "W2N1", "x": 10, "y": 10 }
        }));
        room.insert_with_id(2, creep(2, 25, 25, 100)).unwrap();
        room.insert_with_id(3, creep(3, 25, 25, 1100)).unwrap();
        let rooms = vec![(name, room)];
        let fresh = std::collections::BTreeSet::from_iter([ObjId(3)]);
        let moves = collect_portal_transports(&rooms, &fresh, 0);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].id, ObjId(2), "fresh creep 当拍不传送");
    }

    #[test]
    fn collect_skips_closed_destination_room() {
        // 目的房 status=closed 不传送（novice/respawn 保持可达）。
        let (name, mut room) = portal_room(serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "room": "W2N1", "x": 10, "y": 10 }
        }));
        room.insert_with_id(2, creep(2, 25, 25, 100)).unwrap();
        let mut closed_room = RoomState::new("W2N1", kk_core::Terrain::plains());
        closed_room.room_status = Some(kk_world::domain::room_status::RoomStatus {
            status: kk_world::domain::room_status::RoomStatusKind::Closed,
            timestamp: 100,
        });
        let rooms = vec![(name, room), ("W2N1".into(), closed_room)];
        assert!(collect_portal_transports(&rooms, &Default::default(), 99).is_empty());
        assert_eq!(
            collect_portal_transports(&rooms, &Default::default(), 100).len(),
            1
        );
    }

    #[test]
    fn portal_tick_decay_time_zero_never_removes() {
        // 官方 `decayTime &&` 真值门：decayTime=0 与缺省同义（永久门）。
        let portal = serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "room": "W2N1", "x": 10, "y": 10 },
            "decayTime": 0
        });
        let (_, room) = portal_room(portal);
        let mut w = test_working(room, 9);
        portal_tick(&mut w, ObjId(1));
        assert!(
            w.room.objects.contains_key(&ObjId(1)),
            "decayTime=0 永不移除"
        );
    }

    #[test]
    fn portal_tick_removes_at_strict_gate_and_companion_wall() {
        let portal = serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "room": "W2N1", "x": 10, "y": 10 },
            "decayTime": 4
        });
        let (_, mut room) = portal_room(portal);
        room.insert_with_id(
            5,
            RoomObject::ConstructedWall(ConstructedWallObj {
                id: 5,
                x: 26,
                y: 26,
                hits: 1000,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
        let mut w = test_working(room, 4);
        portal_tick(&mut w, ObjId(1));
        // 等号拍（time == decayTime）不移除；严格大于才移除。
        assert!(w.room.objects.contains_key(&ObjId(1)));
        assert!(w.room.objects.contains_key(&ObjId(5)));
        let mut w = test_working(w.room, 5);
        portal_tick(&mut w, ObjId(1));
        assert!(!w.room.objects.contains_key(&ObjId(1)));
        assert!(!w.room.objects.contains_key(&ObjId(5)), "配墙连带移除");
    }

    #[test]
    fn portal_tick_at_49_edge_skips_companion_wall_without_panicking() {
        let portal = serde_json::json!({
            "type": "structure", "structureType": "portal",
            "destination": { "room": "W2N1", "x": 10, "y": 10 },
            "decayTime": 4
        });
        let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
        room.insert_with_id(
            1,
            RoomObject::Generic(GenericObj {
                id: 1,
                x: 49,
                y: 25,
                data: portal.to_string(),
            }),
        )
        .unwrap();
        let mut wall = ConstructedWallObj {
            id: 5,
            x: 10,
            y: 10,
            hits: 1000,
            notify_when_attacked: None,
        };
        wall.x = 10;
        room.insert_with_id(5, RoomObject::ConstructedWall(wall))
            .unwrap();
        let mut w = test_working(room, 5);
        portal_tick(&mut w, ObjId(1));
        assert!(
            !w.room.objects.contains_key(&ObjId(1)),
            "49 边 portal 到期移除"
        );
        assert!(
            w.room.objects.contains_key(&ObjId(5)),
            "越界无配墙可移除，墙保持"
        );
    }

    fn test_working(room: RoomState, time: u64) -> Working {
        Working::new(
            room,
            time,
            0,
            super::super::TickState::new(std::collections::BTreeMap::new(), 100),
            test_plane(),
        )
    }

    fn test_plane() -> super::super::Plane {
        let plan = std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap(),
        );
        super::super::Plane {
            world_span: super::super::Plane::UNWRAPPED_SPAN,
            buildings: Default::default(),
            links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
            stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
            combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            static_passability: Default::default(),
            resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
            contrib: std::rc::Rc::new(super::super::contrib::ContributionDispatch::default()),
            extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            plan,
        }
    }
}
