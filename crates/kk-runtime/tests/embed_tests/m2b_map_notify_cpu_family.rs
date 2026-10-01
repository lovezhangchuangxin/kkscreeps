//! 验证地图路线与房间状态、通知的数量限制和持久化，以及 CPU 查询和 halt。

use super::*;

pub(super) fn m2b_map_family_e2e_impl() -> anyhow::Result<()> {
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

pub(super) fn m2b_map_multihop_e2e_impl() -> anyhow::Result<()> {
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

pub(super) fn m2b_notify_and_cpu_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;

    // notify：无参/带 groupInterval 均 OK；每 tick 20 帽（第 20 条 OK、21 起
    // ERR_FULL）——GAME-NOTIFY-001/002 同形
    let run = e.run_player(
        "p1",
        r#"[ typeof Game.notify,
             Game.notify('hello'),
             Game.notify('hello', 5),
             Game.notify() ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], "function");
    assert_eq!(v[1], 0);
    assert_eq!(v[2], 0);
    assert_eq!(v[3], 0, "无参亦 OK（message undefined 落库容空串）");

    let run = e.run_player(
        "p1",
        r#"(function () {
             const codes = [];
             for (let i = 0; i < 25; i++) codes.push(Game.notify('n' + i, 10));
             return [codes[19], codes[20], codes[24]];
           })()"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 0, "第 20 条（索引 19）OK");
    assert_eq!(v[1], -8, "第 21 条 ERR_FULL");
    assert_eq!(v[2], -8);
    assert_eq!(
        e.player_notifications_for_test("p1").len(),
        23,
        "tick1 的 3 条+tick2 帽内 20 条（i=0..19 全过）落账"
    );

    // 帽随 tick 重置（beginTick 重建 intents list）
    e.tick(1)?;
    let run = e.run_player("p1", r#"Game.notify('fresh tick')"#)?;
    assert_eq!(run.expr["value"], 0, "新 tick 帽重置");
    e.tick(1)?;
    let records = e.player_notifications_for_test("p1");
    assert_eq!(records.len(), 24);
    assert_eq!(records[23].message, "fresh tick");
    assert_eq!(records[23].group_interval, None);
    // tick1 三条：notify('hello')（无间隔）/notify('hello', 5)/notify()
    //——undefined 键 JSON 序列化自然丢弃
    assert_eq!(records[0].message, "hello");
    assert_eq!(records[0].group_interval, None);
    assert_eq!(records[1].message, "hello");
    assert_eq!(records[1].group_interval, Some(5));
    assert_eq!(records[2].message, "");

    // cpu 三面：cpuLimit=tickLimit；getHeapStatistics 真数值（limit>0 且
    // used∈[0,limit]——CPU-HEAP-001 同形）；halt 硬停（后续代码不可达、
    // 无 console 错误、tick 完成、下一 tick 可用）
    let run = e.run_player(
        "p1",
        r#"[ Game.cpuLimit === Game.cpu.tickLimit,
             Game.cpu.getHeapStatistics().heap_size_limit > 0,
             Game.cpu.getHeapStatistics().used_heap_size >= 0
               && Game.cpu.getHeapStatistics().used_heap_size
                 <= Game.cpu.getHeapStatistics().heap_size_limit,
             typeof Game.cpu.getHeapStatistics().externally_allocated_size ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(
        v[0], true,
        "cpuLimit=tickLimit（官方 runtimeData.cpu 同值）"
    );
    assert_eq!(v[1], true, "heap_size_limit 真数值");
    assert_eq!(v[2], true, "used 在界内");
    assert_eq!(
        v[3], "number",
        "snake_case 键面（overmind/tooangel 消费形）"
    );

    let run = e.run_player(
        "p1",
        r#"
        console.log('before-halt');
        Game.cpu.halt();
        console.log('after-halt');
        throw new Error('unreachable');
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("before-halt")),
        "{:?}",
        run.console
    );
    assert!(
        !run.console.iter().any(|l| l.contains("after-halt")),
        "halt 后代码不可达"
    );
    // halt 除了返回运行错误，还应输出 CPU halted 错误日志。
    assert!(
        run.console.iter().any(|l| l.contains("CPU halted")),
        "CPU halted 错误行：{:?}",
        run.console
    );
    assert!(
        !run.console.iter().any(|l| l.contains("unreachable")),
        "halt 后玩家代码不可达：{:?}",
        run.console
    );
    // halt 跳过本轮计费，提交前后的 CPU 桶净变动应为 0。
    let before = e.player_bucket_for_test("p1")?;
    let run = e.run_player("p1", r#"Game.cpu.halt(); console.log('nope');"#)?;
    assert!(run.console.iter().any(|l| l.contains("CPU halted")));
    let after = e.player_bucket_for_test("p1")?;
    assert_eq!(
        before, after,
        "halt tick 零计费——桶净变动 0（正常 tick 有 limit−used 回充差）"
    );
    // halt 后沙箱健康：下一 tick 正常执行
    e.tick(1)?;
    let run = e.run_player("p1", r#"1 + 1"#)?;
    assert_eq!(run.expr["value"], 2, "halt 后沙箱可复用");
    Ok(())
}
