//! Game.map 域：describeExits/线性距离与多跳路线 e2e。

use super::*;

pub(super) fn map_family_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" },
                  { "name": "W1N2" }],
    }))?;

    // describeExits：在场房四向全开（邻房可不在世界——官方 mapGrid 语义，
    // MAP-ROOM-001 同形）；非法名 null
    let run = e.run_player(
        "p1",
        r#"[ Game.map.describeExits('W1N1'),
             Game.map.describeExits('not_a_room'),
             Game.map.getRoomLinearDistance('W1N1', 'W5N5'),
             Game.map.getRoomLinearDistance('W1N1', 'W1N1') ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(
        v[0],
        // W 数字向中心递减：W1N1 东邻 W0N1、西邻 W2N1（与官方套件
        // MAP-ROOM-001 期望逐键一致）
        json!({"1": "W1N2", "3": "W0N1", "5": "W1N0", "7": "W2N1"}),
        "四向出口（TOP=1/RIGHT=3/BOTTOM=5/LEFT=7 键）"
    );
    assert_eq!(v[1], serde_json::Value::Null, "非法名 null");
    assert_eq!(v[2], 4, "Chebyshev（W1N1→W5N5 对角 4）");
    assert_eq!(v[3], 0, "同房 0");

    // getRoomStatus：在场 normal（视野无关）/合法名 off-world closed/非法名
    // 与非字符串 undefined（正则 .test 强转语义）
    let run = e.run_player(
        "p1",
        r#"[ Game.map.getRoomStatus('W1N2'),
             Game.map.getRoomStatus('W1N2').status,
             Game.map.getRoomStatus('E10N10'),
             Game.map.getRoomStatus('bogus'),
             Game.map.getRoomStatus(123),
             Game.map.isRoomAvailable('W1N2'),
             Game.map.isRoomAvailable('E10N10'),
             Game.map.isRoomAvailable('bogus') ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!({"status": "normal", "timestamp": null}));
    assert_eq!(v[1], "normal", "未见房同 normal（视野无关）");
    assert_eq!(
        v[2],
        json!({"status": "closed", "timestamp": null}),
        "off-world closed"
    );
    assert_eq!(v[3], serde_json::Value::Null, "非法名 undefined");
    assert_eq!(v[4], serde_json::Value::Null, "非字符串 undefined");
    assert_eq!(v[5], true, "在场房 available");
    assert_eq!(v[6], false);
    assert_eq!(v[7], false);

    // findRoute/findExit：邻房路由一步（W1N2 在 W1N1 北——FIND_EXIT_TOP=1）；
    // 对象参形态；非法名 ERR_NO_PATH；同房 findExit=ERR_INVALID_ARGS；
    // routeCallback Infinity 剪枝
    let run = e.run_player(
        "p1",
        r#"[ Game.map.findRoute('W1N1', 'W1N2'),
             Game.map.findRoute({name: 'W1N1'}, {name: 'W1N2'}),
             Game.map.findRoute('W1N1', 'not_a_room'),
             Game.map.findExit('W1N1', 'W1N2'),
             Game.map.findExit('W1N1', 'W1N1'),
             Game.map.findRoute('W1N1', 'W1N2', { routeCallback: (r) => r === 'W1N2' ? Infinity : 1 }),
             Game.rooms['W1N1'].findExitTo('W1N2') ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!([{"exit": 1, "room": "W1N2"}]), "一步北行");
    assert_eq!(v[1], v[0], "对象参取 .name");
    assert_eq!(v[2], -2, "非法名 ERR_NO_PATH");
    assert_eq!(v[3], 1, "findExit=FIND_EXIT_TOP");
    assert_eq!(v[4], -10, "同房 ERR_INVALID_ARGS");
    assert_eq!(v[5], -2, "Infinity 剪枝无路");
    assert_eq!(v[6], 1, "findExitTo 与 map 一致（ROOM-API-002 同形）");
    Ok(())
}

pub(super) fn map_multihop_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" },
                  { "name": "W1N2" }, { "name": "W2N1" }, { "name": "W2N2" }],
    }))?;

    // 两步 L 形路线有两条等成本路径，用来检查并列节点的处理顺序。
    let run = e.run_player(
        "p1",
        r#"[ Game.map.findRoute('W1N1', 'W2N2'),
             Game.map.findRoute('W1N1', 'W2N2', { routeCallback: (r) => r === 'W2N1' ? Infinity : 1 }) ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0].as_array().map(Vec::len), Some(2), "两步路由");
    let route = v[0].as_array().unwrap();
    // 等成本时应先向西再向北（LEFT 7 → TOP 1）；只检查步数不能发现并列顺序变化。
    assert_eq!(
        (route[0]["room"].clone(), route[1]["room"].clone()),
        (json!("W2N1"), json!("W2N2")),
        "房序列（官方 Heap 并列序——锁定逐字移植形态）"
    );
    assert_eq!(
        (route[0]["exit"].clone(), route[1]["exit"].clone()),
        (json!(7), json!(1))
    );
    // 封锁先向西的 W2N1，路线应改为向北经 W1N2、W2N2。
    let detour = v[1].as_array().unwrap();
    assert_eq!(detour.len(), 2, "绕行两步");
    assert_eq!(
        (detour[0]["room"].clone(), detour[1]["room"].clone()),
        (json!("W1N2"), json!("W2N2")),
        "Infinity 剪枝后北路绕行"
    );

    // kRouteGrid=30 搜索界：距起点 >30 房不可达（官方 xyToIndex 界外哨兵）
    let run = e.run_player("p1", r#"Game.map.findRoute('W1N1', 'W40N40')"#)?;
    assert_eq!(run.expr["value"], -2, "超 kRouteGrid=30 界 → ERR_NO_PATH");
    Ok(())
}
