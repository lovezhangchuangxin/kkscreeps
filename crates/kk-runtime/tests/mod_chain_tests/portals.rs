//! portal/房间状态/世界边缘域集成：放置归一→同 shard 传送→衰变→
//! 快照折叠（套件 13.6/21.1/7.13-7.14/26.0/inspection 同形），房间状态
//! 种子驱动 getRoomStatus 与 launchNuke 门，worldSize/连续距离环绕三处
//! 同源（map/market/terminal send），以及检查点 v9 冷恢复。
//! 行为锚点：engine processor/intents/portals/tick.js、creeps/tick.js:80-85、
//! processor/global.js:33-43、structures.js:1356-1389、common calcWorldSize、
//! utils.js calcRoomsDistance/calcTerminalEnergyCost。
use super::*;
use kk_core::constants as c;

const ROOM: &str = "W1N1";
const DEST: &str = "W2N1";

fn engine_on(binary: bool) -> EmbedEngine {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e
}

/// 双房世界：源房与目的房均为 p1 所辖 RCL1（传送后目的房有视野）。
fn two_room_world() -> Value {
    json!({ "players": ["p1"], "rooms": [
        { "name": ROOM, "rcl": 1, "owner": "p1" },
        { "name": DEST, "rcl": 1, "owner": "p1" },
    ]})
}

fn place_portal(
    e: &mut EmbedEngine,
    destination: Value,
    ticks_to_decay: Option<u64>,
) -> anyhow::Result<String> {
    let mut spec = json!({ "pos": [25, 25], "destination": destination });
    if let Some(t) = ticks_to_decay {
        spec["ticksToDecay"] = json!(t);
    }
    e.place_object(ROOM, "portal", &spec)
}

fn creeps_in(e: &EmbedEngine, room: &str) -> Vec<Value> {
    e.find_in_room(room, c::FIND_CREEPS)
}

/// PORTAL-001 同形：放置即站门格的 creep 两拍内被传到目的房精确坐标
///（官方 ageTime 真值门使首拍寿命未初始化的 creep 豁免——放置拍后
/// 寿命已初始化，下一拍传送）。
#[test]
fn standing_creep_transports_to_exact_destination() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&two_room_world())?;
        place_portal(&mut e, json!({ "room": DEST, "x": 10, "y": 10 }), None)?;
        e.place_object(
            ROOM,
            "creep",
            &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "PortalCreep" }),
        )?;
        e.tick(2)?;
        let teleported = creeps_in(&e, DEST)
            .into_iter()
            .find(|c| c["name"] == json!("PortalCreep"));
        let t = teleported.expect("两拍后 creep 出现在目的房");
        assert_eq!(t["pos"]["x"], json!(10));
        assert_eq!(t["pos"]["y"], json!(10));
        assert!(creeps_in(&e, ROOM).is_empty(), "源房无残留");
    }
    Ok(())
}

/// PORTAL-005 同形：相邻格 move 踩上 portal，随后无任何玩家代码的拍
/// 被动传走（同 tick 两段式：落格与传送同拍，官方 global 阶段同 tick 落格）。
#[test]
fn stepping_onto_portal_transports_without_further_intent() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&two_room_world())?;
        place_portal(&mut e, json!({ "room": DEST, "x": 10, "y": 10 }), None)?;
        let creep = e.place_object(
            ROOM,
            "creep",
            &json!({ "pos": [25, 26], "owner": "p1", "body": ["move"], "name": "PortalStepper" }),
        )?;
        e.tick(1)?;
        let run = e.run_player("p1", &format!("Game.getObjectById('{creep}').move(TOP)"))?;
        assert_eq!(run.expr["value"], json!(0), "move 返回 OK");
        // 落格与传送同拍完成（run_player 的结算段 = 官方 creep tick 标记
        // interRoom + global 阶段同 tick 落格），断言紧随其后钉住同拍性。
        let t = creeps_in(&e, DEST)
            .into_iter()
            .find(|c| c["name"] == json!("PortalStepper"))
            .expect("落格同拍即传送");
        assert_eq!(t["pos"]["x"], json!(10));
        assert_eq!(t["pos"]["y"], json!(10));
        assert!(creeps_in(&e, ROOM).is_empty());
        // 无任何玩家代码的后续拍不改变传送结果。
        e.tick(1)?;
        assert!(
            creeps_in(&e, DEST)
                .iter()
                .any(|c| c["name"] == json!("PortalStepper"))
        );
    }
    Ok(())
}

/// PORTAL-004/006 与 inspection 同形：永久门 ticksToDecay undefined、
/// 临时门倒数 [2,1,0,-1,null]（严格 time > decayTime 才移除——0 与 -1
/// 都可读出）、快照 destination 折叠 roomName。
#[test]
fn decay_countdown_reads_zero_and_minus_one_before_removal() -> anyhow::Result<()> {
    for binary in [false, true] {
        decay_countdown_impl(binary)?;
    }
    Ok(())
}

fn decay_countdown_impl(binary: bool) -> anyhow::Result<()> {
    let mut e = engine_on(binary);
    e.create_shard(&two_room_world())?;
    let permanent = place_portal(&mut e, json!({ "room": DEST, "x": 25, "y": 25 }), None)?;
    let temporary = place_portal(&mut e, json!({ "room": DEST, "x": 30, "y": 31 }), Some(3))?;
    // 保活 creep：官方房间休眠语义在 kk 不存在，此为套件注释同形锚。
    e.place_object(
        ROOM,
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["move"], "name": "Watcher" }),
    )?;
    e.tick(1)?;
    let read = |e: &mut EmbedEngine, id: &str| -> anyhow::Result<Value> {
        Ok(e.run_player(
            "p1",
            &format!("Game.getObjectById('{id}') ? Game.getObjectById('{id}').ticksToDecay : null"),
        )?
        .expr["value"]
            .clone())
    };
    // 读数时刻 t=2..6（初始 tick 后每轮 run_player = 执行+结算+推进一拍），
    // decayTime=1+3=4：4-2..4-5 = 2,1,0,-1，t=5 结算移除 → t=6 读 null。
    let mut readings = Vec::new();
    for _ in 0..5 {
        readings.push(read(&mut e, &temporary)?);
    }
    assert_eq!(
        readings,
        vec![json!(2), json!(1), json!(0), json!(-1), json!(null)]
    );
    // 永久门恒 undefined（序列化 null 且对象在场）。
    assert_eq!(read(&mut e, &permanent)?, json!(null));
    let snapshot = e.get_object(&permanent).expect("永久门在场");
    assert_eq!(
        snapshot["destination"],
        json!({"x": 25, "y": 25, "roomName": DEST})
    );
    // 管理快照通道按适配器 PortalSnapshot 契约恒暴露 ticksToDecay
    //（永久门 null）；玩家面仍为官方 getter 的 undefined（序列化 null）。
    assert_eq!(snapshot["ticksToDecay"], json!(null));
    assert_eq!(snapshot["structureType"], json!("portal"));
    Ok(())
}

/// 官方 creeps/tick.js:80-85：portal 标记覆盖同一 interRoom 字段的 edge
/// 标记——边缘格上的 portal 胜过环绕过渡，同拍直接传送到目的房。
#[test]
fn portal_on_edge_tile_beats_edge_wrap() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({ "players": ["p1"], "rooms": [
            { "name": ROOM, "rcl": 1, "owner": "p1" },
            { "name": "W0N1", "rcl": 1, "owner": "p1" },
            { "name": DEST, "rcl": 1, "owner": "p1" },
        ]}))?;
        // place_portal 固定 [25,25]，本例需要边缘格，直接走通用放置。
        e.place_object(
            ROOM,
            "portal",
            &json!({
                "pos": [0, 25],
                "destination": { "room": DEST, "x": 10, "y": 10 }
            }),
        )?;
        e.place_object(
            ROOM,
            "creep",
            &json!({ "pos": [0, 25], "owner": "p1", "body": ["move"], "name": "EdgeMover" }),
        )?;
        // 首拍即下 move(LEFT)：同一 tick 内移动执行产生边缘过渡标记、
        // 随后的 portal 检测覆盖它（放置即初始化寿命，无首拍豁免）。
        let run = e.run_player("p1", "Game.creeps.EdgeMover.move(LEFT)")?;
        assert_eq!(run.expr["value"], json!(0), "move 返回 OK");
        let t = creeps_in(&e, DEST)
            .into_iter()
            .find(|c| c["name"] == json!("EdgeMover"))
            .expect("portal 胜过边缘过渡：同拍传送到目的房");
        assert_eq!(t["pos"]["x"], json!(10));
        assert_eq!(t["pos"]["y"], json!(10));
        assert!(
            creeps_in(&e, "W0N1").is_empty(),
            "被 portal 覆盖的边缘过渡不生效"
        );
    }
    Ok(())
}

/// 官方 accessibleRooms 不含 status=closed 房：closed 目的房不传送，
/// 对象留原地逐拍重试（novice/respawn 的 db status 保持 normal 仍可达）。
#[test]
fn closed_destination_room_blocks_transport() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({ "players": ["p1"], "rooms": [
            { "name": ROOM, "rcl": 1, "owner": "p1" },
            { "name": DEST, "status": "closed" },
        ]}))?;
        place_portal(&mut e, json!({ "room": DEST, "x": 10, "y": 10 }), None)?;
        e.place_object(
            ROOM,
            "creep",
            &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "BlockedMover" }),
        )?;
        e.tick(3)?;
        let stayed = creeps_in(&e, ROOM)
            .into_iter()
            .find(|c| c["name"] == json!("BlockedMover"))
            .expect("closed 目的房使 creep 留在源房");
        assert_eq!(stayed["pos"]["x"], json!(25));
        assert_eq!(stayed["pos"]["y"], json!(25));
        assert!(creeps_in(&e, DEST).is_empty());
    }
    Ok(())
}

/// PORTAL-002/003 同形：同 shard destination 是真 RoomPosition 实例
///（方法可调用）、跨 shard destination 是 {shard, room} POJO（目的房
/// 无需在本世界注册）。
#[test]
fn destination_exposes_roomposition_or_cross_shard_pojo() -> anyhow::Result<()> {
    for binary in [false, true] {
        destination_exposes_impl(binary)?;
    }
    Ok(())
}

fn destination_exposes_impl(binary: bool) -> anyhow::Result<()> {
    let mut e = engine_on(binary);
    e.create_shard(&two_room_world())?;
    let same = place_portal(&mut e, json!({ "room": DEST, "x": 30, "y": 30 }), None)?;
    let cross = e.place_object(
        ROOM,
        "portal",
        &json!({ "pos": [26, 25], "destination": { "shard": "shard1", "room": "W5N5" } }),
    )?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!(
            "const s = Game.getObjectById('{same}').destination; \
             const x = Game.getObjectById('{cross}').destination; \
             ({{ \
                 isRoomPosition: s instanceof RoomPosition, \
                 equalToSelf: s.isEqualTo(s), \
                 room: s.roomName, x: s.x, y: s.y, \
                 crossShard: x.shard, crossRoom: x.room \
             }})"
        ),
    )?;
    assert_eq!(
        run.expr["value"],
        json!({
            "isRoomPosition": true, "equalToSelf": true,
            "room": DEST, "x": 30, "y": 30,
            "crossShard": "shard1", "crossRoom": "W5N5"
        })
    );
    // 跨 shard 门上的 creep 不传送（官方 OSS 无跨 shard 实现，kk 跳过）。
    e.place_object(
        ROOM,
        "creep",
        &json!({ "pos": [26, 25], "owner": "p1", "body": ["move"], "name": "CrossShardStand" }),
    )?;
    e.tick(2)?;
    assert!(
        creeps_in(&e, ROOM)
            .iter()
            .any(|c| c["name"] == json!("CrossShardStand")),
        "跨 shard 门上的 creep 原地保持"
    );
    // 跨 shard destination 的管理快照原样保留 {shard, room} 键。
    let snapshot = e.get_object(&cross).unwrap();
    assert_eq!(
        snapshot["destination"],
        json!({"shard": "shard1", "room": "W5N5"})
    );
    Ok(())
}

/// NUKE-IMPACT-008:portalAtBlastCenterSurvives 同形：核爆中心 portal
/// 免伤（无 hits 面，官方 `_damage` 的 !target.hits 早退同门）。
#[test]
fn portal_at_nuke_blast_center_survives() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({ "players": ["p1"], "rooms": [
        { "name": ROOM, "rcl": 8, "owner": "p1" },
    ]}))?;
    let portal = place_portal(&mut e, json!({ "room": DEST, "x": 25, "y": 25 }), None)?;
    e.place_object(
        ROOM,
        "nuke",
        &json!({ "pos": [25, 25], "launchRoomName": "W3N1", "timeToLand": 1 }),
    )?;
    e.tick(2)?;
    let run = e.run_player("p1", &format!("Game.getObjectById('{portal}') !== null"))?;
    assert_eq!(run.expr["value"], json!(true), "核爆后 portal 存活");
    Ok(())
}

/// NUKE-LAUNCH-014..017 同形：源/目标房 novice/respawn 时预检返回
/// ERR_INVALID_TARGET 且不消耗库存冷却；同时钉住 getRoomStatus 的
/// 玩家面四态与 closed 的时间戳。
#[test]
fn launch_nuke_room_status_gates_reject_without_consuming() -> anyhow::Result<()> {
    for (source_status, dest_status, expected) in [
        (Some("novice"), None, "novice"),
        (Some("respawn"), None, "respawn"),
        (None, Some("novice"), "novice"),
        (None, Some("respawn"), "respawn"),
    ] {
        let mut room_src = json!({ "name": ROOM, "rcl": 8, "owner": "p1" });
        if let Some(s) = source_status {
            room_src["status"] = json!(s);
        }
        let mut room_dst = json!({ "name": DEST, "rcl": 1, "owner": "p1" });
        if let Some(s) = dest_status {
            room_dst["status"] = json!(s);
        }
        let mut e = EmbedEngine::new();
        e.create_shard(&json!({ "players": ["p1"], "rooms": [room_src, room_dst] }))?;
        let nuker = e.place_object(
            ROOM,
            "structure",
            &json!({
                "pos": [25, 25], "structureType": "nuker", "owner": "p1",
                "store": { "energy": c::NUKER_ENERGY_CAPACITY, "G": c::NUKER_GHODIUM_CAPACITY }
            }),
        )?;
        e.tick(1)?;
        let status_room = if source_status.is_some() { ROOM } else { DEST };
        let run = e.run_player(
            "p1",
            &format!(
                "const s = Game.map.getRoomStatus('{status_room}'); \
                 const rc = Game.getObjectById('{nuker}').launchNuke(new RoomPosition(25, 25, '{DEST}')); \
                 [s.status, typeof s.timestamp, s.timestamp > 0, rc]"
            ),
        )?;
        assert_eq!(
            run.expr["value"],
            json!([expected, "number", true, c::ERR_INVALID_TARGET]),
            "源状态 {source_status:?} 目标状态 {dest_status:?}"
        );
        e.tick(1)?;
        let snapshot = e.get_object(&nuker).unwrap();
        assert_eq!(snapshot["store"]["energy"], json!(c::NUKER_ENERGY_CAPACITY));
        assert_eq!(snapshot["store"]["G"], json!(c::NUKER_GHODIUM_CAPACITY));
        assert_eq!(snapshot["cooldown"], json!(0));
    }
    // 反向：closed 源房不拦发射（官方门只含 novice/respawnArea——
    // closed 经 openTime 不参与 launchNuke 门序）。
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({ "players": ["p1"], "rooms": [
        { "name": ROOM, "rcl": 8, "owner": "p1", "status": "closed" },
        { "name": DEST, "rcl": 1, "owner": "p1" },
    ]}))?;
    let nuker = e.place_object(
        ROOM,
        "structure",
        &json!({
            "pos": [25, 25], "structureType": "nuker", "owner": "p1",
            "store": { "energy": c::NUKER_ENERGY_CAPACITY, "G": c::NUKER_GHODIUM_CAPACITY }
        }),
    )?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!("Game.getObjectById('{nuker}').launchNuke(new RoomPosition(25, 25, '{DEST}'))"),
    )?;
    assert_eq!(run.expr["value"], json!(0), "closed 源房照常发射");
    Ok(())
}
/// 目的房未注册（合法房名不在世界）：对象保持原地逐 tick 重试等价
///（官方 accessibleRooms 未含即跳过、interRoom 保留）。
#[test]
fn portal_to_unregistered_destination_keeps_creep_in_place() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({ "players": ["p1"], "rooms": [
            { "name": ROOM, "rcl": 1, "owner": "p1" },
        ]}))?;
        place_portal(&mut e, json!({ "room": "W9N9", "x": 10, "y": 10 }), None)?;
        e.place_object(
            ROOM,
            "creep",
            &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "Stranded" }),
        )?;
        e.tick(3)?;
        let stayed = creeps_in(&e, ROOM);
        assert_eq!(stayed.len(), 1, "未注册目的房不传送");
        assert_eq!(stayed[0]["pos"]["x"], json!(25));
        assert_eq!(stayed[0]["pos"]["y"], json!(25));
    }
    Ok(())
}

/// powerCreep 经同 shard portal 传送（官方 power-creeps/tick.js:44-47
/// 无 ageTime 门）；跨 shard portal 对 powerCreep 不传送。
#[test]
fn power_creep_transports_through_same_shard_portal_only() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&two_room_world())?;
        place_portal(&mut e, json!({ "room": DEST, "x": 40, "y": 40 }), None)?;
        e.place_object(
            ROOM,
            "powerCreep",
            &json!({
                "pos": [25, 25], "owner": "p1", "name": "PortedPC",
                "className": "operator", "level": 1
            }),
        )?;
        e.tick(1)?;
        assert!(
            e.find_in_room(DEST, c::FIND_POWER_CREEPS)
                .into_iter()
                .any(|pc| pc["name"] == json!("PortedPC")),
            "powerCreep 同 shard 传送"
        );
    }
    Ok(())
}

/// 传送不清疲劳、不发 EVENT_EXIT（官方 portal 分支与房间出口分支的
/// 两处差异；房间出口的 EXIT 事件由边缘过渡路径产生）。
#[test]
fn portal_transport_keeps_fatigue_and_emits_no_exit_event() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&two_room_world())?;
    place_portal(&mut e, json!({ "room": DEST, "x": 10, "y": 10 }), None)?;
    let creep = e.place_object(
        ROOM,
        "creep",
        &json!({
            "pos": [25, 26], "owner": "p1", "body": ["move", "work", "work"],
            "name": "Weary"
        }),
    )?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!(
            "[Game.getObjectById('{creep}').move(TOP), \
             Game.getObjectById('{creep}').fatigue]"
        ),
    )?;
    // move+work+work：出发拍疲劳 0，移动后按 2×work 重量累计 2。
    assert_eq!(run.expr["value"], json!([0, 0]), "出发拍疲劳为 0");
    // 紧随传送拍读数：后续空拍会按 MOVE 部件恢复疲劳（官方同形）。
    let run = e.run_player(
        "p1",
        &format!(
            "const c = Game.getObjectById('{creep}'); \
             c && c.pos.roomName === '{DEST}' ? [c.fatigue, \
             Game.rooms['{ROOM}'].getEventLog().filter(ev => ev.event === EVENT_EXIT).length] : null"
        ),
    )?;
    assert_eq!(
        run.expr["value"],
        json!([2, 0]),
        "疲劳随传送保持（不清），源房无 EXIT 事件"
    );
    Ok(())
}

/// MAP-ROOM-003/004/005 同形：normal/null、off-world closed/null、
/// in-world closed 带正时间戳；worldSize=包含跨度（E0/S0 锚 0——
/// 纯 W/N 世界量到 E0/S0）；连续距离逐轴环绕。
#[test]
fn room_status_surface_and_wrapped_distance() -> anyhow::Result<()> {
    for binary in [false, true] {
        room_status_surface_impl(binary)?;
    }
    Ok(())
}

fn room_status_surface_impl(binary: bool) -> anyhow::Result<()> {
    let mut e = engine_on(binary);
    e.create_shard(&json!({ "players": ["p1"], "rooms": [
        { "name": ROOM, "rcl": 1, "owner": "p1" },
        { "name": "W2N1", "status": "closed" },
        { "name": "E1S1" },
    ]}))?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        "({ \
            normal: Game.map.getRoomStatus('W1N1'), \
            closedSeed: Game.map.getRoomStatus('W2N1'), \
            closedAvailable: Game.map.isRoomAvailable('W2N1'), \
            offWorld: Game.map.getRoomStatus('W99N99'), \
            invalid: Game.map.getRoomStatus('junk'), \
            unseen: Game.map.getRoomStatus('E1S1').status, \
            worldSize: Game.map.getWorldSize(), \
            across: [false, true].map(c => Game.map.getRoomLinearDistance('W1N1', 'E1S1', c)), \
            within: Game.map.getRoomLinearDistance('W1N1', 'W2N1', true) \
        })",
    )?;
    let v = &run.expr["value"];
    assert_eq!(v["normal"], json!({"status": "normal", "timestamp": null}));
    assert_eq!(v["closedSeed"]["status"], json!("closed"));
    assert_eq!(v["closedAvailable"], json!(false));
    assert!(v["closedSeed"]["timestamp"].as_u64().is_some_and(|t| t > 0));
    assert_eq!(
        v["offWorld"],
        json!({"status": "closed", "timestamp": null})
    );
    assert_eq!(v["invalid"], json!(null));
    // 无视野房（E1S1 上无 p1 对象）仍是公共地图数据。
    assert_eq!(v["unseen"], json!("normal"));
    // 房集 W1N1(-2,-2)、W2N1(-3,-2)、E1S1(1,1)：x 跨度 1-(-3)+1=5、
    // y 跨度 4 → worldSize 5。W1N1↔E1S1 dx=3, dy=3；连续形
    // min(5-3,3)=2 两轴 → 2（环绕生效）；W1N1↔W2N1 dx=1 → min(4,1)=1。
    assert_eq!(v["worldSize"], json!(5));
    assert_eq!(v["across"], json!([3, 2]));
    assert_eq!(v["within"], json!(1));
    let mut expired = e.checkpoint(Value::Null)?;
    expired["engine"]["world"]["rooms"]["W2N1"]["room_status"]["timestamp"] = json!(1);
    drop(e);
    let mut resumed = engine_on(binary);
    resumed.restore_checkpoint(expired, None)?;
    let after = resumed.run_player(
        "p1",
        "({status: Game.map.getRoomStatus('W2N1'), available: Game.map.isRoomAvailable('W2N1')})",
    )?;
    assert_eq!(
        after.expr["value"]["status"],
        json!({"status":"normal","timestamp":null})
    );
    assert_eq!(after.expr["value"]["available"], json!(true));
    Ok(())
}

/// TERMINAL-SEND-015 同形：跨边缘 send 的能量费按环绕距离计费——
/// W0N1↔W10N1 直线 10 格、世界跨度 12（0 锚点），环绕后 min(12-10,10)=2；
/// 玩家面 calcTransactionCost 与结算侧扣费同源。
#[test]
fn terminal_send_charges_wrapped_distance_across_edges() -> anyhow::Result<()> {
    for binary in [false, true] {
        terminal_send_wrapped_impl(binary)?;
    }
    Ok(())
}

fn terminal_send_wrapped_impl(binary: bool) -> anyhow::Result<()> {
    let mut e = engine_on(binary);
    e.create_shard(&json!({ "players": ["p1"], "rooms": [
        { "name": "W0N1", "rcl": 6, "owner": "p1" },
        { "name": "W10N1", "rcl": 6, "owner": "p1" },
    ]}))?;
    let src = e.place_object(
        "W0N1",
        "structure",
        &json!({
            "pos": [25, 25], "structureType": "terminal", "owner": "p1",
            "store": { "energy": 100000 }
        }),
    )?;
    e.place_object(
        "W10N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "terminal", "owner": "p1", "store": {} }),
    )?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!(
            "({{ \
                wrapped: Game.map.getRoomLinearDistance('W0N1', 'W10N1', true), \
                cost: Game.market.calcTransactionCost(1000, 'W0N1', 'W10N1'), \
                rc: Game.getObjectById('{src}').send(RESOURCE_ENERGY, 1000, 'W10N1') \
            }})"
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v["wrapped"], json!(2), "房集跨度 12，直线 10 → 环绕 2");
    assert_eq!(
        v["cost"],
        json!((1000.0 * (1.0 - (-2.0f64 / 30.0).exp())).ceil() as i64)
    );
    assert_eq!(v["rc"], json!(0));
    e.tick(1)?;
    let src_snapshot = e.get_object(&src).unwrap();
    let cost = v["cost"].as_i64().unwrap();
    assert_eq!(src_snapshot["store"]["energy"], json!(100000 - 1000 - cost));
    Ok(())
}

/// 传送与房间状态种子随完整检查点冷恢复（v9）：恢复世界的 portal 继续
/// 传送与衰变，状态种子读回同值。
#[test]
fn portals_and_room_status_survive_cold_restore() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({ "players": ["p1"], "rooms": [
            { "name": ROOM, "rcl": 1, "owner": "p1" },
            { "name": DEST, "rcl": 1, "owner": "p1" },
            { "name": "W3N1", "status": "novice" },
        ]}))?;
        place_portal(&mut e, json!({ "room": DEST, "x": 10, "y": 10 }), Some(500))?;
        let saved = e.checkpoint(Value::Null)?;
        assert_eq!(saved["checkpoint_format_version"], json!(9));
        let seed = &saved["engine"]["world"]["rooms"]["W3N1"]["room_status"];
        assert_eq!(seed["status"], json!("novice"));
        assert!(seed["timestamp"].as_u64().is_some_and(|t| t > 0));
        assert_eq!(
            saved["engine"]["world"]["rooms"][ROOM]["room_status"],
            json!(null),
            "normal 房为 null"
        );

        // 完整往返等值：恢复即保存逐字相等（种子非 null 的全量断言）。
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved.clone(), None)?;
        assert_eq!(restored.checkpoint(Value::Null)?, saved);
        // 迁移接受腿：键齐全的 v8 载荷降版本 → migrate → 恢复 → 读回种子。
        let mut v8 = saved.clone();
        v8["checkpoint_format_version"] = json!(8);
        let mut legacy_engine = EmbedEngine::new();
        legacy_engine.set_kkpv_channel(binary);
        legacy_engine.restore_checkpoint(v8, None)?;
        let run = legacy_engine.run_player("p1", "Game.map.getRoomStatus('W3N1').status")?;
        assert_eq!(run.expr["value"], json!("novice"), "v8 载荷迁移后种子可读");
        restored.place_object(
            ROOM,
            "creep",
            &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "Restored" }),
        )?;
        restored.tick(2)?;
        assert!(
            creeps_in(&restored, DEST)
                .iter()
                .any(|c| c["name"] == json!("Restored")),
            "恢复的 portal 继续传送"
        );
        let run = restored.run_player("p1", "Game.map.getRoomStatus('W3N1').status")?;
        assert_eq!(run.expr["value"], json!("novice"));
        // 缺 room_status 键的 v8 载荷显式拒绝（v8_to_v9 拒缺迁移）。
        let mut legacy = restored.checkpoint(Value::Null)?;
        legacy["checkpoint_format_version"] = json!(8);
        if let Some(rooms) = legacy["engine"]["world"]["rooms"].as_object_mut() {
            for room in rooms.values_mut() {
                room.as_object_mut().unwrap().remove("room_status");
            }
        }
        let before = restored.checkpoint(Value::Null)?;
        assert!(restored.restore_checkpoint(legacy, None).is_err());
        assert_eq!(
            restored.checkpoint(Value::Null)?,
            before,
            "拒绝后旧分片完整"
        );
    }
    Ok(())
}

/// 放置契约：destination 缺失/混合形态/越界拒绝且候选原子（同拍前缀
/// 对象不落库）；createShard 非法 status 拒绝。
#[test]
fn portal_placement_and_status_seed_rejections() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&two_room_world())?;
    for bad in [
        json!({ "pos": [25, 25] }),
        json!({ "pos": [25, 25], "destination": { "shard": "shard1", "room": "W2N1", "x": 1, "y": 1 } }),
        json!({ "pos": [25, 25], "destination": { "room": "W2N1", "x": 50, "y": 1 } }),
        json!({ "pos": [25, 25], "destination": { "room": "not a room", "x": 1, "y": 1 } }),
        json!({ "pos": [25, 25], "destination": { "room": "W2N1", "x": 1, "y": 1 }, "owner": "p1" }),
        json!({ "pos": [25, 25], "destination": { "room": "W2N1", "x": 1, "y": 1 }, "ticksToDecay": -1 }),
    ] {
        assert!(
            e.place_object(ROOM, "portal", &bad).is_err(),
            "非法 spec 须拒绝：{bad}"
        );
    }
    assert!(
        e.find_in_room(ROOM, c::FIND_STRUCTURES)
            .into_iter()
            .all(|s| s["structureType"] != json!("portal")),
        "失败候选不落库"
    );
    assert!(
        EmbedEngine::new()
            .create_shard(&json!({ "players": ["p1"], "rooms": [
                { "name": ROOM, "status": "closedForever" }
            ]}))
            .is_err(),
        "非法 status 拒绝"
    );
    Ok(())
}
