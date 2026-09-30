//! M2B 扩面 P2 全链 e2e：创建族（createFlag/createConstructionSite 四 API 面
//! 与旗按玩家私有可见性）、Flag/Site 增删改、findClosestByRange、
//! room.storage/terminal 回填、Room.findPath 直线桩、Room.toJSON。
//! 官方对照：rooms.js:978-1097/1546-1576/540-549、flags.js:58-91、
//! construction-sites.js:50-62。run_players 一次调用即结算一 tick。

use std::collections::BTreeMap;

use super::*;

pub(super) fn m2b_create_flag_and_site_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" },
                  { "name": "W2N2" }],
    }))?;

    // p2 在 W1N1 摆 creep 授房视野（官方：无对象的房不进 Game.rooms——
    // createFlag 面对不可见房无从发起）
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [30, 31], "owner": "p2", "body": ["move"], "name": "p2eye" }),
    )?;
    // tick1：p1 建旗 + 建工地；p2 同名建旗（per-user 判重不冲突）
    let run = e.run_player(
        "p1",
        r#"
        console.log('f1=' + Game.rooms['W1N1'].createFlag(10, 10, 'home'));
        console.log('sametick=' + (Game.flags['home'] !== undefined && Game.flags['home'].name));
        console.log('shellroom=' + (Game.flags['home'].room && Game.flags['home'].room.name));
        console.log('dup=' + Game.rooms['W1N1'].createFlag(11, 11, 'home'));
        console.log('s1=' + Game.rooms['W1N1'].createConstructionSite(20, 20, 'extension'));
        console.log('pos=' + Game.rooms['W1N1'].createFlag(new RoomPosition(12, 12, 'W1N1'), 'posf', COLOR_RED));
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("f1=home")),
        "{:?}",
        run.console
    );
    // 本地 Game.flags 装配（官方 rooms.js:1022——同 tick 立即可读；壳 .room
    // 回填 Room 实例——官方 RoomObject.call 同形，清偿复审 P3 锁）
    assert!(
        run.console.iter().any(|l| l.contains("sametick=home")),
        "{:?}",
        run.console
    );
    assert!(
        run.console.iter().any(|l| l.contains("shellroom=W1N1")),
        "{:?}",
        run.console
    );
    assert!(
        run.console.iter().any(|l| l.contains("dup=-3")),
        "同 tick 同名 → ERR_NAME_EXISTS"
    );
    assert!(run.console.iter().any(|l| l.contains("s1=0")), "工地 OK");
    assert!(
        run.console.iter().any(|l| l.contains("pos=posf")),
        "RoomPosition 形态转发"
    );
    let _ = e.run_player(
        "p2",
        r#"Game.rooms['W1N1'].createFlag(30, 30, 'home', COLOR_BLUE);"#,
    )?;

    // tick2：双观察者同 tick——p1 见自家旗不见 p2 旗；工地进全集
    let mut codes = BTreeMap::new();
    codes.insert(
        "p1".into(),
        r#"[ !!Game.flags['home'], !!Game.flags['posf'],
             Game.rooms['W1N1'].find(FIND_FLAGS).length,
             Game.rooms['W1N1'].find(FIND_CONSTRUCTION_SITES).length ]"#
            .into(),
    );
    codes.insert(
        "p2".into(),
        r#"[ !!Game.flags['home'], Game.rooms['W1N1'].find(FIND_FLAGS).length ]"#.into(),
    );
    let runs = e.run_players(&codes)?;
    let v1 = &runs["p1"].expr["value"];
    assert_eq!(v1[0], true, "p1 见自家 home 旗");
    assert_eq!(v1[1], true, "p1 见 posf 旗");
    assert_eq!(v1[2], 2, "FIND_FLAGS 只含自家（p2 的 home 不可见）");
    assert_eq!(v1[3], 1, "工地入 FIND 集");
    let v2 = &runs["p2"].expr["value"];
    assert_eq!(v2[0], true, "p2 见自家同名 home 旗（per-user 集合）");
    assert_eq!(v2[1], 1, "p2 的 FIND_FLAGS 只见自家");

    // tick3：旗换色 + 工地 API 删除（setColor 与 setPosition 不同 tick——
    // 同 tick 时组序处理下第二个同名 createFlag 输给第一个，官方同形；
    // 该语义由引擎级测试的组序用例另行锁定）
    let run = e.run_player(
        "p1",
        r#"
        console.log('sc=' + Game.flags['home'].setColor(COLOR_GREEN));
        const sites = Game.rooms['W1N1'].find(FIND_CONSTRUCTION_SITES);
        console.log('rm=' + sites[0].remove());
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("sc=0")),
        "{:?}",
        run.console
    );
    assert!(run.console.iter().any(|l| l.contains("rm=0")));

    // tick4：换色已生效；移位
    let run = e.run_player(
        "p1",
        r#"[ Game.flags['home'].color,
             Game.rooms['W1N1'].find(FIND_CONSTRUCTION_SITES).length,
             Game.flags['home'].setPosition(40, 40) ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 5, "换色生效（COLOR_GREEN=5）");
    assert_eq!(v[1], 0, "工地经 API 删除");
    assert_eq!(v[2], 0, "setPosition OK");
    // tick5：移位生效 + 跨房移位（RoomPosition 形态带目标房名）
    let run = e.run_player(
        "p1",
        r#"[ Game.flags['home'].pos.x, Game.flags['home'].pos.y, Game.flags['home'].setPosition(new RoomPosition(5, 5, 'W2N2')) ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 40, "移位生效 x");
    assert_eq!(v[1], 40, "移位生效 y");
    assert_eq!(v[2], 0);
    // tick6：不可见房自家旗双断言（三路评审 P2 清偿）：官方 flags 按 user
    // 全局查询（data.js:140）——旗在 Game.flags 存活；旗不授房视野
    //（data.js:113-116 跳过表）——W2N2 不进 Game.rooms
    let run = e.run_player(
        "p1",
        r#"[ !!Game.flags['home'], Game.rooms['W2N2'] === undefined, Game.flags['home'].remove() ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], true, "不可见房自家旗仍在 Game.flags");
    assert_eq!(v[1], true, "旗不授房视野");
    assert_eq!(v[2], 0);
    // tick7：删除生效
    let run = e.run_player("p1", r#"!!Game.flags['home']"#)?;
    assert_eq!(run.expr["value"], false, "remove 生效");
    Ok(())
}

pub(super) fn m2b_creation_gates_and_helpers_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "c" }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [27, 25], "owner": "p1", "body": ["move"], "name": "far" }),
    )?;

    let run = e.run_player(
        "p1",
        r#"
        // RCL 配额：rcl1 的 extension 配额 0 → 第二个工地 ERR_RCL_NOT_ENOUGH
        console.log('q=' + Game.rooms['W1N1'].createConstructionSite(20, 20, 'extension'));
        // 同格同类互斥（上一 tick 已建的工地进占位检查——同 tick 双建官方
        // API 双 OK、结算静默去重，不在此断言）
        console.log('t1=' + Game.rooms['W1N1'].createConstructionSite(5, 5, 'road'));
        // findClosestByRange：c(25,25) 距离 0 优于 far(27,25)
        console.log('near=' + Game.creeps.c.pos.findClosestByRange(FIND_CREEPS).name);
        // findPath 直线桩：形态与可调用性
        const path = Game.rooms['W1N1'].findPath(Game.creeps.c.pos, new RoomPosition(27, 25, 'W1N1'));
        console.log('path=' + path.length + ':' + path[0].dx + ':' + path[0].direction);
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("q=-14")),
        "{:?}",
        run.console
    );
    assert!(run.console.iter().any(|l| l.contains("t1=0")));
    assert!(run.console.iter().any(|l| l.contains("near=c")));
    assert!(
        run.console.iter().any(|l| l.contains("path=2:1:3")),
        "直线两步向右（RIGHT=3）"
    );

    // 下一 tick：同格同类 → ERR_INVALID_TARGET（工地已进投影）
    let run = e.run_player(
        "p1",
        r#"Game.rooms['W1N1'].createConstructionSite(5, 5, 'road')"#,
    )?;
    assert_eq!(run.expr["value"], -7, "同格同类 ERR_INVALID_TARGET");

    // toJSON：stringify 不含 controller/storage/terminal 键
    let run = e.run_player(
        "p1",
        r#"(s => [ 'controller' in s, 'storage' in s, 'toJSON' in s, s.mode ])(JSON.parse(JSON.stringify(Game.rooms['W1N1'])))"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], false, "controller 键不入 toJSON");
    assert_eq!(v[1], false);
    assert_eq!(v[2], false);
    assert_eq!(v[3], "world");
    Ok(())
}

pub(super) fn m2b_storage_terminal_backfill_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 4, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "storage", "owner": "p1", "store": { "energy": 100 } }),
    )?;

    let run = e.run_player(
        "p1",
        r#"[ !!Game.rooms['W1N1'].storage, Game.rooms['W1N1'].storage.structureType,
             Game.rooms['W1N1'].storage.store.energy, Game.rooms['W1N1'].terminal === undefined ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], true, "storage 回填为结构实例");
    assert_eq!(v[1], "storage");
    assert_eq!(v[2], 100);
    assert_eq!(v[3], true, "无 terminal 为 undefined");
    Ok(())
}
