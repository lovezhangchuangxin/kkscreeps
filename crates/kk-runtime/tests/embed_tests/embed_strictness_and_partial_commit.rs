//! 验证输入校验、Memory 行为，以及失败轮的玩家状态和重试缓存。
//! 场景由 `embed_engine_suite` 顺序调用。

use super::*;

/// 验证 Memory、RawMemory、模块加载和 Game 的属性行为。
pub(super) fn a5_memory_module_forms_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;

    // ① Memory 描述符生命周期（MEMHACK-001/012）：tick 起 getter 无 setter 可
    // 配置；首访翻转为值属性
    let run = e.run_player(
        "p1",
        r#"(() => {
        const d0 = Object.getOwnPropertyDescriptor(globalThis, 'Memory');
        void Memory;
        const d1 = Object.getOwnPropertyDescriptor(globalThis, 'Memory');
        return {
            startGetter: typeof d0.get === 'function' && d0.set === undefined && d0.configurable,
            afterValue: 'value' in d1 && d1.get === undefined && d1.configurable && d1.enumerable,
        };
    })()"#,
    )?;
    assert_eq!(
        run.expr["value"]["startGetter"], true,
        "tick 起 getter 形态: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["afterValue"], true,
        "首访固化值属性: {:?}",
        run.expr
    );

    // ② RawMemory._parsed 身份与 set 失效（MEMHACK-004/007 尾拍）：首访后
    // _parsed === Memory 引用；set 即删 _parsed → 写回跳过、下一 tick 从新 raw 重解析
    let run = e.run_player(
        "p1",
        r#"(() => { Memory.keep = 1; return RawMemory._parsed === Memory; })()"#,
    )?;
    assert_eq!(
        run.expr["value"], true,
        "_parsed 与 Memory 同引用: {:?}",
        run.expr
    );
    let run = e.run_player("p1", r#"(() => { void Memory; Memory.mut = 'x'; RawMemory.set('{"fromSet":true}'); return 'ok'; })()"#)?;
    assert_eq!(run.expr["value"], "ok");
    let run = e.run_player(
        "p1",
        r#"({ raw: RawMemory.get(), mut: Memory.mut, fromSet: Memory.fromSet })"#,
    )?;
    assert_eq!(
        run.expr["value"]["raw"], r#"{"fromSet":true}"#,
        "set 后写回未覆写: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["mut"],
        serde_json::Value::Null,
        "陈旧 mutation 未落 raw"
    );
    assert_eq!(
        run.expr["value"]["fromSet"], true,
        "下一 tick 从新 raw 重解析"
    );

    // ③ 解析失败 → Memory null、raw 保留、写回跳过（官方 game.js:479-487）
    let run = e.run_player("p1", r#"RawMemory.set('{broken'); 'set-ok'"#)?;
    assert_eq!(run.expr["value"], "set-ok");
    let run = e.run_player("p1", r#"({ m: Memory === null, raw: RawMemory.get() })"#)?;
    assert_eq!(
        run.expr["value"]["m"], true,
        "解析失败 Memory=null: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["raw"], "{broken",
        "毒 raw 原样保留（不写回）"
    );

    // RawMemory.set 分别检查字符串类型和 2MB 长度上限。
    let run = e.run_player(
        "p1",
        r#"try { RawMemory.set(5); 'no-throw' } catch (e) { e.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Raw memory value is not a string",
        "非字符串门: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { RawMemory.set('x'.repeat(2*1024*1024+1)); 'no-throw' } catch (e) { e.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Raw memory length exceeded 2 MB limit",
        "2MB 门: {:?}",
        run.expr
    );

    // ⑤ delete+assign 绕过解析（MEMHACK-003）与 Game 可写性（官方平赋值）
    let run = e.run_player(
        "p1",
        r#"(() => {
        delete globalThis.Memory;
        globalThis.Memory = { injected: true };
        const before = Memory.injected;
        Game = { hacked: true };
        return { injected: before, gameHacked: Game.hacked === true };
    })()"#,
    )?;
    assert_eq!(
        run.expr["value"]["injected"], true,
        "delete+assign 绕过: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["gameHacked"], true,
        "Game 平赋值可写: {:?}",
        run.expr
    );
    let run = e.run_player("p1", r#"({ g: Game.hacked === undefined, t: Game.time })"#)?;
    assert_eq!(
        run.expr["value"]["g"], true,
        "下一 tick Game 重装: {:?}",
        run.expr
    );

    // spawn.memory 的写入应保存在 Memory.spawns 中。
    let spawn_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1" }),
    )?;
    let spawn_json = serde_json::to_string(&spawn_id)?;
    let run = e.run_player(
        "p1",
        &format!(
            r#"(() => {{
        RawMemory.set('{{}}'); // ③ 毒化后的 raw 在此洗净（同 tick 重解析生效）
        const s = Game.getObjectById({spawn_json});
        s.memory.job = 'harvest';
        return {{ job: s.memory.job, inMem: Memory.spawns[s.name].job }};
    }})()"#
        ),
    )?;
    assert_eq!(
        run.expr["value"]["job"], "harvest",
        "spawn.memory 读写: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["inMem"], "harvest",
        "spawn.memory 落 Memory.spawns: {:?}",
        run.expr
    );

    // 另外检查 RawMemory 对象每轮重建、模块名不自动补全，
    // 以及非对象 JSON、Memory 顶层原型和 String 包装对象的处理。
    let run = e.run_player("p1", r#"globalThis.__rm = RawMemory; 'captured'"#)?;
    assert_eq!(run.expr["value"], "captured");
    let run = e.run_player(
        "p1",
        r#"({ fresh: RawMemory !== globalThis.__rm, raw: RawMemory.get() })"#,
    )?;
    assert_eq!(
        run.expr["value"]["fresh"], true,
        "RawMemory 每 tick 新建: {:?}",
        run.expr
    );
    assert!(
        run.expr["value"]["raw"]
            .as_str()
            .is_some_and(|r| r.contains("spawns")),
        "raw 字符串本体跨 tick 持久（⑥ 的正常写回已洗净 ③ 的毒 raw）: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { require('nope.js'); 'no-throw' } catch (err) { err.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Unknown module 'nope.js'",
        "无 .js 补全（官方 :553）: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { require('../x'); 'no-throw' } catch (err) { err.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Unknown module '../x'",
        "无 ../ 解析: {:?}",
        run.expr
    );
    let run = e.run_player("p1", r#"RawMemory.set('"5"'); Memory === '5'"#)?;
    assert_eq!(
        run.expr["value"], true,
        "合法 JSON 非对象值原样保留: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"RawMemory.set('{"a":1}'); void Memory.a; try { Memory.hasOwnProperty('a'); 'callable' } catch (err) { 'throws' }"#,
    )?;
    assert_eq!(
        run.expr["value"], "throws",
        "顶层 __proto__=null（hasOwnProperty 不可用）: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { RawMemory.set(new String('x')); 'passed' } catch (err) { err.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "passed",
        "String 包装对象过 _.isString 门: {:?}",
        run.expr
    );
    Ok(())
}

/// 检查结构是否启用、默认耐久值，以及工地进度超限时的建造算术。
pub(super) fn a6_isactive_hits_family_impl() -> anyhow::Result<()> {
    // ---- isActive：RCL 配额 + Chebyshev 距离计数（controller 在 [1,1]）----
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 2, "owner": "p1" }],
    }))?;
    // 6 个 extension（RCL2 配额 5）沿 y=5 横排：x=2..5 距离同为 4（等距按
    // 放置序 tie-break），x=6 距离 5，x=7 距离 6——最后一个 inactive
    let mut ext_ids = Vec::new();
    for x in 2..=7u8 {
        ext_ids.push(e.place_object(
            "W1N1",
            "structure",
            &json!({ "pos": [x, 5], "structureType": "extension", "owner": "p1" }),
        )?);
    }
    // tower 在 RCL2（需 RCL3）→ quota[2]=0 → inactive
    let tower = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [20, 20], "structureType": "tower", "owner": "p1" }),
    )?;
    // 无主 road → gate① owner undefined → true
    let road = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [30, 30], "structureType": "road" }),
    )?;
    // owned spawn：CONTROLLER_STRUCTURES['spawn'][2]=1 → true
    let spawn = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "spawn", "owner": "p1",
            "name": "S1", "store": { "energy": 100 } }),
    )?;
    let ids_json = serde_json::to_string(&ext_ids)?;
    let expr = format!(
        "const ids = {ids_json};
         const act = ids.map((id) => Game.getObjectById(id).isActive());
         [act.join(','), Game.getObjectById({tower:?}).isActive(),
          Game.getObjectById({road:?}).isActive(), Game.getObjectById({spawn:?}).isActive()]"
    );
    let run = e.run_player("p1", &expr)?;
    let v = &run.expr["value"];
    assert_eq!(
        v[0], "true,true,true,true,true,false",
        "isActive 距离计数：前 5 extension active、最远 1 个 inactive: {v:?}"
    );
    assert_eq!(v[1], false, "RCL 不足的 tower inactive: {v:?}");
    assert_eq!(v[2], true, "无主 road 恒 active: {v:?}");
    assert_eq!(v[3], true, "owned spawn active: {v:?}");

    // 工地不计入距离配额（官方 compareObj.type='constructionSite' 不匹配
    // 结构类型）：更近的同类型工地不得挤掉既有结构的 active
    e.place_object(
        "W1N1",
        "constructionSite",
        &json!({ "pos": [2, 2], "structureType": "extension", "owner": "p1" }),
    )?;
    let ids_json2 = serde_json::to_string(&ext_ids)?;
    let run = e.run_player(
        "p1",
        &format!("{ids_json2}.map((id) => Game.getObjectById(id).isActive()).join(',')"),
    )?;
    assert_eq!(
        run.expr["value"], "true,true,true,true,true,false",
        "工地不参与 isActive 距离计数: {:?}",
        run.expr
    );

    // 检查通用结构默认的 hits 和 hitsMax。
    let mut e2 = EmbedEngine::new();
    e2.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    let tower2 = e2.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "tower", "owner": "p1" }),
    )?;
    let rampart = e2.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [11, 10], "structureType": "rampart", "owner": "p1" }),
    )?;
    let link = e2.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [12, 10], "structureType": "link", "owner": "p1" }),
    )?;
    let container = e2.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [13, 10], "structureType": "container" }),
    )?;
    let lab = e2.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [14, 10], "structureType": "lab", "owner": "p1" }),
    )?;
    // 显式 hits 覆盖默认耐久值，hitsMax 仍取结构常量。
    let tower_override = e2.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [15, 10], "structureType": "tower", "owner": "p1", "hits": 999 }),
    )?;
    for (id, hits, hits_max) in [
        (&tower2, 3000, 3000),
        (&rampart, 1, 300_000_000),
        (&link, 1000, 1000),
        (&container, 250_000, 250_000),
        (&lab, 500, 500),
        (&tower_override, 999, 3000),
    ] {
        let snap = e2.get_object(id).unwrap();
        assert_eq!(
            snap["hits"], hits,
            "generic 结构缺省 hits（{id}）: {snap:?}"
        );
        assert_eq!(
            snap["hitsMax"], hits_max,
            "generic 结构缺省 hitsMax（{id}）: {snap:?}"
        );
    }

    // ---- build 负 remaining 官方算术：progress 播种超 total → 一次 build 完成
    //（effect 为负：progress 回钳至 total、能量反增——utils 官方 quirk 直通）
    let mut e3 = EmbedEngine::new();
    e3.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 2, "owner": "p1" }],
    }))?;
    let site = e3.place_object(
        "W1N1",
        "constructionSite",
        &json!({ "pos": [25, 25], "structureType": "road", "owner": "p1", "progress": 1495 }),
    )?;
    let builder = e3.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 26], "owner": "p1", "body": ["work", "carry", "move"],
            "name": "b1", "store": { "energy": 5 } }),
    )?;
    let run = e3.run_player(
        "p1",
        &format!("Game.getObjectById({builder:?}).build(Game.getObjectById({site:?}))"),
    )?;
    assert_eq!(
        run.expr["value"], 0,
        "超限播种工地的 build 返回 OK: {:?}",
        run.expr
    );
    assert!(
        e3.get_object(&site).is_none(),
        "工地当 tick 落成移除（负 remaining 回钳完成）"
    );
    let structures = e3.find_in_room("W1N1", 107); // FIND_STRUCTURES
    let road_snap = structures
        .iter()
        .find(|s| s["pos"]["x"] == 25 && s["pos"]["y"] == 25)
        .expect("落成的 road 在 FIND_STRUCTURES");
    assert_eq!(
        road_snap["hits"], 5000,
        "plain 上 road 落成 hits=ROAD_HITS: {road_snap:?}"
    );
    let builder_snap = e3.get_object(&builder).unwrap();
    assert_eq!(
        builder_snap["store"]["energy"], 1200,
        "负 effect 能量反增 quirk（官方 5-(-1195)=1200）: {builder_snap:?}"
    );

    // progress 等于 total 时，建造效果为 0，仍应完成工地且不消耗能量。
    let mut e4 = EmbedEngine::new();
    e4.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 2, "owner": "p1" }],
    }))?;
    let site4 = e4.place_object(
        "W1N1",
        "constructionSite",
        &json!({ "pos": [20, 20], "structureType": "road", "owner": "p1", "progress": 300 }),
    )?;
    let builder4 = e4.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [20, 21], "owner": "p1", "body": ["work", "carry", "move"],
            "name": "b4", "store": { "energy": 50 } }),
    )?;
    let run = e4.run_player(
        "p1",
        &format!("Game.getObjectById({builder4:?}).build(Game.getObjectById({site4:?}))"),
    )?;
    assert_eq!(
        run.expr["value"], 0,
        "等值工地的 build 返回 OK: {:?}",
        run.expr
    );
    assert!(
        e4.get_object(&site4).is_none(),
        "等值工地一次 build 落成（effect=0 完成分支）"
    );
    let b4_snap = e4.get_object(&builder4).unwrap();
    assert_eq!(
        b4_snap["store"]["energy"], 50,
        "effect=0 能量不动: {b4_snap:?}"
    );

    // 直接注入没有 WORK 部件的建造意图，绕过玩家 API 的身体部件预检。
    // 结算仍应记录 actionLog 和 amount 为 0 的事件。
    let mut e5 = EmbedEngine::new();
    e5.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 2, "owner": "p1" }],
    }))?;
    let site5 = e5.place_object(
        "W1N1",
        "constructionSite",
        &json!({ "pos": [10, 10], "structureType": "road", "owner": "p1", "progress": 0 }),
    )?;
    let no_work = e5.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 11], "owner": "p1", "body": ["move"], "name": "nw",
            "store": { "energy": 10 } }),
    )?;
    let list = json!({ no_work.clone(): { "build": { "id": site5 } } });
    let batch = kk_engine::intent::IntentBatch::from_json("p1", &list);
    e5.settle_external(batch)?;
    // 建造效果为 0 时不改变工地或能量，但仍记录建造动作和事件。
    let site5_snap = e5.get_object(&site5).unwrap();
    assert_eq!(
        site5_snap["progress"], 0,
        "零 WORK build 进度不动: {site5_snap:?}"
    );
    let nw_snap = e5.get_object(&no_work).unwrap();
    assert_eq!(
        nw_snap["store"]["energy"], 10,
        "零 WORK build 能量不动: {nw_snap:?}"
    );
    let run = e5.run_player(
        "p1",
        "Game.rooms['W1N1'].getEventLog().filter((ev) => ev.event === 4).length",
    )?;
    // 当前玩家代码不发出建造意图；读取的是上一轮外部结算记录的 EVENT_BUILD。
    assert_eq!(
        run.expr["value"], 1,
        "零 WORK build 仍推 EVENT_BUILD(amount 0): {:?}",
        run.expr
    );
    Ok(())
}

/// 拒绝非法地形和分片输入，接受整数值的浮点 rcl；校验失败保留现有世界。
pub(super) fn embed_shard_input_strictness_impl() -> anyhow::Result<()> {
    use kk_runtime::embed::terrain_codes_from_json;
    use serde_json::json;
    // 元素级严格性：非整数（小数/字符串/null）、负数、超 255 一律拒绝
    assert_eq!(
        terrain_codes_from_json(&json!([0, 1, 2, 3])).unwrap(),
        vec![0, 1, 2, 3]
    );
    for bad in [json!(1.5), json!("1"), json!(null), json!(-1), json!(256)] {
        assert!(
            terrain_codes_from_json(&json!([bad])).is_err(),
            "元素 {bad} 必须被拒绝"
        );
    }
    assert!(terrain_codes_from_json(&json!("not-array")).is_err());

    // JSON 3.0 的 rcl 应接受为整数 3，并能通过玩家 API 读回。
    let mut engine = EmbedEngine::new();
    engine.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3.0, "owner": "p1" }],
    }))?;
    let run = engine.run_player("p1", "Game.rooms['W1N1'].controller.level")?;
    assert_eq!(run.expr["value"], 3, "浮点整数 rcl: {:?}", run.expr);

    // 非法 spec 保留既有世界（上一行 run_player 已把 time 推到 1）
    for invalid in [
        json!({ "players": [], "rooms": [{ "name": "W2N2", "rcl": "3" }] }),
        json!({ "players": [], "rooms": [{ "name": "W2N2", "rcl": 3.5 }] }),
        json!({ "players": [], "rooms": [{ "name": "W2N2" }, { "name": "W2N2" }] }),
        json!({ "players": ["alice", { "name": "alice" }], "rooms": [] }),
        json!({}),
        json!({ "players": [], "rooms": "W2N2" }),
        json!({ "players": "alice", "rooms": [] }),
        json!({ "players": [], "rooms": [{}] }),
        json!({ "players": [{}], "rooms": [] }),
        json!({ "players": [], "rooms": [{ "name": "E130N0" }] }),
        json!({ "players": [], "rooms": [{ "name": "E128N0" }] }),
        json!({ "players": [], "rooms": [{ "name": "middle0" }] }),
    ] {
        assert!(engine.create_shard(&invalid).is_err());
        assert_eq!(engine.get_game_time(), 2, "创建热身拍 + 失败创建不推进");
    }
    Ok(())
}

/// 一个玩家把 Game 设为不可重定义后，其下一轮世界注入失败；
/// 该失败不阻止世界推进，也不阻止另一玩家提交 Memory。
pub(super) fn b05_partial_commit_inject_failure_impl() -> anyhow::Result<()> {
    use serde_json::json;
    use std::collections::BTreeMap;
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    // 本轮世界已注入，再把 Game 设为不可重定义；下一轮重装 Game 时才会失败。
    let mut codes = BTreeMap::new();
    codes.insert(
        "p1".to_string(),
        "Memory.n = (Memory.n || 0) + 1; 'ok'".to_string(),
    );
    codes.insert(
        "p2".to_string(),
        r#"(() => { Object.defineProperty(globalThis, 'Game', { configurable: false, writable: false }); return 'armed'; })()"#
            .to_string(),
    );
    let runs = e.run_players(&codes)?;
    assert!(runs["p1"].executed, "武装轮 p1 正常");
    assert!(runs["p2"].executed, "武装轮 p2 正常（毒化下一轮生效）");
    // p2 世界注入失败只影响自身；世界照常推进，p1 的 Memory 仍应提交。
    let mut codes = BTreeMap::new();
    codes.insert(
        "p1".to_string(),
        "Memory.n = (Memory.n || 0) + 1; 'ok'".to_string(),
    );
    codes.insert("p2".to_string(), "'innocent'".to_string());
    let runs = e.run_players(&codes)?;
    assert!(runs["p1"].executed, "p1 不受邻居注入失败影响");
    assert!(!runs["p2"].executed, "注入失败 executed=false");
    assert_eq!(
        runs["p2"].expr["kind"], "inject_failed",
        "注入失败 kind: {:?}",
        runs["p2"].expr
    );
    // 世界照常推进 + p1 Memory 已提交（下一轮读回验证：两轮 +1 = 2）
    let time_after = e.get_game_time();
    let run = e.run_player("p1", "Memory.n")?;
    assert_eq!(run.expr["value"], 2, "p1 Memory 候选已提交且可读回");
    assert_eq!(e.get_game_time(), time_after + 1, "毒化轮与验证轮各推进 1");
    Ok(())
}

/// 世界结算失败不提交 Memory；重试消费缓存，只执行玩家代码一次。
/// 显式丢弃后也不得保留失败轮的 Memory 候选。
pub(super) fn b05_partial_commit_settle_failure_impl() -> anyhow::Result<()> {
    use serde_json::json;
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    // 建立基线：n=1 已提交
    e.run_player("p1", "Memory.n = (Memory.n || 0) + 1; 'ok'")?;
    let time_before = e.get_game_time();
    assert_eq!(time_before, 2, "热身拍 + 一轮提交");

    // 注入结算失败 + 增量（若玩家重跑会变成 3——执行次数取证）
    e.fail_next_settle_for_test();
    let err = e
        .run_player(
            "p1",
            "Memory.n = Memory.n + 1; Memory.exec = (Memory.exec || 0) + 1; console.log('RUN'); 'ok'",
        )
        .unwrap_err();
    assert!(err.to_string().contains("故障注入"), "结算失败: {err}");
    assert!(e.has_pending_round(), "候选已缓存");

    // ①失败轮不动：世界时间不变、Memory 候选未提交（下一轮注入的仍是旧值——
    // 但注意下一轮就是重试轮，不重跑玩家；用 discard 后的新轮验证旧值）
    assert_eq!(e.get_game_time(), time_before, "失败轮世界时间不动");

    // ③重试：同参数再调——**不重跑玩家**（RUN 行恰好一次：缓存投影原样返回，
    // 执行次数=1），结算消费缓存候选成功、n 提交为 2
    let run = e.run_player("p1", "Memory.n")?;
    let run_count = run.console.iter().filter(|l| l.contains("RUN")).count();
    assert_eq!(run_count, 1, "重试不重跑玩家（RUN 恰一次）");
    assert_eq!(e.get_game_time(), time_before + 1, "重试轮成功推进");
    assert!(!e.has_pending_round(), "提交后缓存清空");
    // 下一轮读回 n=2，exec=1；若重试重新运行原代码，exec 会增加，能检出重复执行。
    let run = e.run_player("p1", "({ n: Memory.n, exec: Memory.exec })")?;
    assert_eq!(run.expr["value"]["n"], 2, "缓存候选（n=2）在重试轮提交");
    assert_eq!(
        run.expr["value"]["exec"], 1,
        "玩家代码执行次数恰一次（exec 计数）"
    );

    // 显式丢弃后，世界停在失败前且没有缓存。新一轮只提交自己的 Memory 修改；
    // 若失败轮 Memory 被提前写入，最后的计数会从 101 变成 102。
    e.fail_next_settle_for_test();
    assert!(
        e.run_player("p1", "Memory.exec = Memory.exec + 1; 'x'")
            .is_err()
    );
    assert!(e.has_pending_round());
    // 缓存失败轮存在时，世界修改和推进操作都应被拒。
    assert!(
        e.place_object("W1N1", "flag", &json!({ "pos": [10, 10], "name": "f" }))
            .is_err()
    );
    assert!(e.tick(1).is_err(), "pending 轮 tick 被拒");
    e.discard_pending_round();
    assert!(!e.has_pending_round(), "丢弃后无 pending");
    assert_eq!(e.get_game_time(), time_before + 2, "丢弃不推进世界");
    let run = e.run_player("p1", "Memory.exec = Memory.exec + 100; Memory.exec")?;
    assert_eq!(
        run.expr["value"], 101,
        "丢弃后新一轮全量重跑（B 轮的 +1 已被丢弃，新一轮 +100 提交）"
    );
    Ok(())
}

/// 请求缓存之外的玩家应返回错误，但保留失败轮供原请求重试。
/// 全局计数器区分缓存重试和重复执行：成功重试后执行次数仍为 1。
pub(super) fn b06_rejected_retry_preserves_pending_impl() -> anyhow::Result<()> {
    use serde_json::json;
    use std::collections::BTreeMap;
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    e.run_player("p1", "'warmup'")?;
    assert_eq!(e.get_game_time(), 2, "热身拍 + warmup 轮");

    // 轮 A：结算失败 + 计数代码（globalThis 跨 tick 持久——V8 全局不随世界
    // 回滚，执行次数的取证通道；b05 注入失败场景的 defineProperty 已证该通道）
    let counter = "globalThis.n = (globalThis.n || 0) + 1; globalThis.n";
    e.fail_next_settle_for_test();
    let err = e.run_player("p1", counter).unwrap_err();
    assert!(err.to_string().contains("故障注入"), "{err}");
    assert!(e.has_pending_round(), "失败轮候选已缓存");
    assert_eq!(e.get_game_time(), 2, "失败轮世界不动");

    // 轮 B：重试请求携带缓存轮之外的玩家 → 拒绝，但缓存必须幸存
    let mut wrong: BTreeMap<String, String> = BTreeMap::new();
    wrong.insert("p1".to_string(), counter.to_string());
    wrong.insert("ghost".to_string(), "'x'".to_string());
    let err2 = e.run_players(&wrong).unwrap_err();
    assert!(err2.to_string().contains("不含玩家 ghost"), "{err2}");
    assert!(
        e.has_pending_round(),
        "被拒的请求不得销毁缓存（B0-6：校验先于 take）"
    );

    // 重试仍传入计数代码；若错误请求丢掉了缓存，这里会重新执行并返回 2。
    let run = e.run_player("p1", counter)?;
    assert_eq!(
        run.expr["value"], 1,
        "重试轮返回缓存候选的 expr（执行 1 的 n=1）；旧 bug 形态下重跑计数得 2: {:?}",
        run.expr
    );
    assert!(!e.has_pending_round(), "重试成功后缓存清空");
    assert_eq!(e.get_game_time(), 3, "warmup 轮 2 + 重试轮推进 1");
    // V8 全局计数器不随世界回滚；值仍为 1 才能证明重试没有重复执行。
    let run = e.run_player("p1", "globalThis.n")?;
    assert_eq!(
        run.expr["value"], 1,
        "玩家代码执行次数恰一次（tick 契约验收场景 3；旧 bug 形态下 n=2）"
    );
    Ok(())
}

/// 失败轮重试应使用首次尝试的墙钟输入，使控制器签名时间早于重试开始时间。
pub(super) fn b06_retry_reuses_recorded_tick_input_impl() -> anyhow::Result<()> {
    use serde_json::json;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let now_ms = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    // signer 与控制器(1,1) 相邻，满足签名的距离要求。
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 1], "name": "signer", "owner": "p1",
            "body": ["move"], "store": { "energy": 0 } }),
    )?;
    let sign_code = "Game.creeps['signer'].signController(Game.rooms['W1N1'].controller, 'b06')";
    // 先预热，缩短首次计时起点到 TickInput::now() 之间的沙箱执行时间。
    e.run_player("p1", "'warmup'")?;

    // 轮 A：签名 intent + 结算失败 → 缓存轮记录首次尝试的 TickInput
    let first_attempt_ms = now_ms();
    e.fail_next_settle_for_test();
    let err = e.run_player("p1", sign_code).unwrap_err();
    assert!(err.to_string().contains("故障注入"), "{err}");
    // 间隔两次尝试的墙钟；若重试重新采样，签名时间将不早于重试起点。
    std::thread::sleep(Duration::from_millis(150));
    let retry_started_ms = now_ms();
    let run = e.run_player("p1", "'retry'")?;
    assert_eq!(
        run.expr["value"], 0,
        "重试轮消费缓存候选（signController 返回 OK）: {:?}",
        run.expr
    );

    // datetime 读回（prelude 按官方形态转 ISO 串——new Date().getTime() 还原
    // 毫秒）：应为首次尝试的记录值（显著早于重试起点），而非重试时刻
    let run = e.run_player(
        "p1",
        "new Date(Game.rooms['W1N1'].controller.sign.datetime).getTime()",
    )?;
    let dt = run.expr["value"].as_u64().expect("datetime 为毫秒整数");
    assert!(
        dt < retry_started_ms.saturating_sub(100),
        "datetime 用首次尝试的记录值（dt={dt} 应早于重试起点 {retry_started_ms}；重采样则不早于它）"
    );
    assert!(
        dt >= first_attempt_ms.saturating_sub(1_000),
        "datetime 合理性：不早于首轮起点附近（{first_attempt_ms}）"
    );
    Ok(())
}
