//! 弃用告警域集成（套件 28.1-28.4 同形）：register.deprecated 逐字消息
//! 经 console.log 落进玩家捕获通道、per-tick 按文本去重（跨 API 同文本
//! 合并、下一拍重发）、avoid/ignore 选项丢弃后路径照常返回、renewCreep
//! 强化体告警不影响返回码。行为锚点：engine game.js:113-120
//!（register.deprecated 闭包去重）、map.js:199-207/235-258（告警先于
//! 参数校验）、path-finder.js:68-74（仅 use(false) 告警）、rooms.js:231-238
//! 与 :325-336（avoid/ignore 告警并丢弃选项）、structures.js:1257-1264
//!（强化体告警后照常设意图）。
use super::*;

const ROOM: &str = "W1N1";

fn engine_on(binary: bool) -> EmbedEngine {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e
}

/// p1 所辖 RCL 可调单房（renewCreep 夹具用 RCL 6，与套件同形）。
fn owned_world(rcl: i64) -> Value {
    json!({ "players": ["p1"], "rooms": [
        { "name": ROOM, "rcl": rcl, "owner": "p1" },
    ]})
}

/// 统计同时包含全部子串的捕获行数（套件断言的 filter 同形）。
fn count_notices(e: &EmbedEngine, substrings: &[&str]) -> usize {
    e.capture_console_logs("p1")
        .iter()
        .filter(|line| substrings.iter().all(|s| line.contains(s)))
        .count()
}

/// DEPRECATED-MAP-001/002 + DEDUP-001 前两拍同形：两种调用形态共享同一
/// 消息文本、每 tick 按文本去重为一条；下一拍重发。
#[test]
fn map_deprecation_notices_dedup_per_tick() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world(1))?;
        let run = e.run_player(
            "p1",
            "({ \
                available: Game.map.isRoomAvailable('W1N1'), \
                availableTwice: Game.map.isRoomAvailable('W1N1'), \
                positional: Game.map.getTerrainAt(25, 25, 'W1N1'), \
                object: Game.map.getTerrainAt(new RoomPosition(25, 25, 'W1N1')), \
            })",
        )?;
        assert_eq!(run.expr["value"]["available"], json!(true));
        assert_eq!(run.expr["value"]["availableTwice"], json!(true));
        assert_eq!(run.expr["value"]["positional"], json!("plain"));
        assert_eq!(run.expr["value"]["object"], json!("plain"));
        // 同 tick 两次调用 + 两种调用形态：各恰好一条。
        assert_eq!(
            count_notices(&e, &["Game.map.isRoomAvailable", "getRoomStatus"]),
            1,
            "isRoomAvailable 每 tick 一条"
        );
        assert_eq!(
            count_notices(&e, &["Game.map.getTerrainAt", "getRoomTerrain"]),
            1,
            "getTerrainAt 两种调用形态合并一条"
        );
        // 下一拍重发（去重域是 tick 不是运行期）。
        e.run_player("p1", "Game.map.isRoomAvailable('W1N1')")?;
        assert_eq!(
            count_notices(&e, &["Game.map.isRoomAvailable"]),
            1,
            "新的一拍重新告警，且只此一条"
        );
        // 告警先于房名校验：非法名返回 false 但告警照发（官方 :199-204）。
        let run2 = e.run_player("p1", "Game.map.isRoomAvailable('bogus')")?;
        assert_eq!(run2.expr["value"], json!(false));
        assert_eq!(count_notices(&e, &["Game.map.isRoomAvailable"]), 1);
    }
    Ok(())
}

/// DEPRECATED-PATH-001 同形：use(false) 告警、use(true) 静默。
#[test]
fn pathfinder_use_false_notices_true_silent() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world(1))?;
        e.run_player("p1", "PathFinder.use(false); PathFinder.use(true); null")?;
        assert_eq!(count_notices(&e, &["PathFinder.use"]), 1);
        // 消息正文逐字（官方 path-finder.js:71）。
        assert!(e.capture_console_logs("p1").iter().any(|l| {
            l.contains("`PathFinder.use` is considered deprecated and will be removed soon.")
        }));
        // 新拍只调 use(true)：capture 是最近一次运行，若 use(true) 也
        // 告警（同拍去重会遮蔽），此处计数为 1 即可区分。
        e.run_player("p1", "PathFinder.use(true)")?;
        assert_eq!(
            count_notices(&e, &["PathFinder.use"]),
            0,
            "use(true) 静默（官方仅 !isActive 告警）"
        );
    }
    Ok(())
}

/// DEPRECATED-PATH-002/003 + DEDUP-001 第三拍同形：avoid/ignore 各自告警
/// 一次（不同文本互不去重、findPath/findPathTo/findClosestByPath 同文本
/// 合并），选项被丢弃、路径照常返回 10 步对角。
#[test]
fn avoid_ignore_notices_and_options_dropped() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world(1))?;
        let run = e.run_player(
            "p1",
            "const room = Game.rooms.W1N1; \
             const from = new RoomPosition(10, 10, 'W1N1'); \
             const to = new RoomPosition(20, 20, 'W1N1'); \
             const tgt = new RoomPosition(15, 15, 'W1N1'); \
             ({ \
                findPath: room.findPath(from, to, { avoid: [{ x: 15, y: 15 }] }).length, \
                findPathTo: from.findPathTo(to, { avoid: [{ x: 15, y: 15 }] }).length, \
                closestPos: from.findClosestByPath([tgt], { avoid: [{ x: 15, y: 15 }] }) ? 1 : 0, \
                ignorePath: room.findPath(from, to, { ignore: [{ x: 15, y: 15 }] }).length, \
                ignorePathTo: from.findPathTo(to, { ignore: [{ x: 15, y: 15 }] }).length, \
                ignoreClosest: from.findClosestByPath([tgt], { ignore: [{ x: 15, y: 15 }] }) ? 1 : 0, \
             })",
        )?;
        // 选项丢弃：10 步对角路径不受 avoid/ignore 影响。
        assert_eq!(run.expr["value"]["findPath"], json!(10));
        assert_eq!(run.expr["value"]["findPathTo"], json!(10));
        assert_eq!(run.expr["value"]["closestPos"], json!(1));
        assert_eq!(run.expr["value"]["ignorePath"], json!(10));
        assert_eq!(run.expr["value"]["ignorePathTo"], json!(10));
        assert_eq!(run.expr["value"]["ignoreClosest"], json!(1));
        // 三个 avoid 表面合并一条（同文本跨 API 去重）；ignore 是另一条
        //（filter 子串集与套件 PATH-003 同形，选中文本同时含三子串）。
        assert_eq!(
            count_notices(&e, &["avoid", "costCallback", "PathFinder.use"]),
            1
        );
        assert_eq!(
            count_notices(&e, &["ignore", "costCallback", "PathFinder.use"]),
            1
        );
    }
    Ok(())
}

/// DEPRECATED-SPAWN-001 同形：强化体 renewCreep 告警推荐 unboostCreep，
/// 返回码仍 OK、意图照常生效（告警在全部拒绝门之后）。
#[test]
fn renew_creep_boosted_notice_keeps_ok() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world(6))?;
        e.place_object(
            ROOM,
            "spawn",
            &json!({"pos": [25, 25], "owner": "p1", "store": {"energy": 300}}),
        )?;
        e.place_object(
            ROOM,
            "creep",
            &json!({
                "pos": [25, 26], "owner": "p1",
                "body": ["work", "carry", "move"], "ticksToLive": 100,
                "boosts": {"0": "UH"},
            }),
        )?;
        e.tick(1)?;
        let run = e.run_player(
            "p1",
            "const s = Game.rooms.W1N1.find(FIND_MY_SPAWNS)[0]; \
             const c = Game.rooms.W1N1.find(FIND_MY_CREEPS)[0]; \
             s.renewCreep(c)",
        )?;
        assert_eq!(run.expr["value"], json!(0), "renewCreep 返回 OK");
        assert_eq!(
            count_notices(
                &e,
                &[
                    "StructureSpawn.renewCreep",
                    "boosted",
                    "StructureLab.unboostCreep"
                ]
            ),
            1
        );
        // 对照：无强化的 creep 不告警（capture_console_logs 是最近一次
        // 完成结算的运行，新拍只含本拍输出，不受上一拍影响）。
        e.place_object(
            ROOM,
            "creep",
            &json!({
                "pos": [26, 26], "owner": "p1",
                "body": ["work", "carry", "move"], "ticksToLive": 100,
            }),
        )?;
        let run2 = e.run_player(
            "p1",
            "const s = Game.rooms.W1N1.find(FIND_MY_SPAWNS)[0]; \
             const cs = Game.rooms.W1N1.find(FIND_MY_CREEPS); \
             s.renewCreep(cs[cs.length - 1])",
        )?;
        assert_eq!(run2.expr["value"], json!(0), "无强化 renew 仍返回 OK");
        assert_eq!(
            count_notices(&e, &["StructureSpawn.renewCreep"]),
            0,
            "无强化的 renewCreep 不告警"
        );
    }
    Ok(())
}
