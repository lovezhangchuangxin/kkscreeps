//! 验证旗帜和工地的创建、修改、删除、可见性，以及房间查询辅助方法。
//! 每次 `run_players` 都会结算一个 tick。

use std::collections::BTreeMap;

use super::*;

pub(super) fn create_flag_and_site_e2e_impl() -> anyhow::Result<()> {
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
    // 创建旗帜后，同 tick 的 Game.flags 应立即可读，其 room 指向当前 Room 实例。
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

    // 旗换色与改位置分两轮调用，避免同 tick 的同名创建意图处理顺序影响本场景。
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
    // 自家旗帜即使在不可见房仍出现在 Game.flags；旗帜不授予视野，房间不进入 Game.rooms。
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

pub(super) fn creation_gates_and_helpers_e2e_impl() -> anyhow::Result<()> {
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
        // findPath 真路径：range 1 目标环 + 终点回填共两步；首步可直行或
        // 对角（等代价路径由原生 JPS 平局规则决定），断言长度与 dx 族。
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
    let path_line = run
        .console
        .iter()
        .find(|l| l.starts_with("path="))
        .ok_or_else(|| anyhow::anyhow!("缺少 path 行: {:?}", run.console))?;
    assert!(
        path_line.starts_with("path=2:1:"),
        "两步且首步向东（dx=1），实际 {path_line}"
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

pub(super) fn storage_terminal_backfill_e2e_impl() -> anyhow::Result<()> {
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
