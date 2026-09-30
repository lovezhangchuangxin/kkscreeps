//! B1 结构治理批自 embed_tests.rs 连续切块（串行链 impl；顶层唯一 #[test] 不变量
//! 在 crate 根——本文件全部 fn 由根链按原调用序调用，不得加 #[test]）。

use super::*;

/// A5-core 片 1 行为探针：Memory/RawMemory/模块系统/Game 可写性的官方形态
/// （对照 screeps-ok UNDOC-MEMHACK 族与官方 game.js/runtime.js 逐条）。
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

    // ④ RawMemory.set 双门消息形态（官方 runtime.js:110-114）
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

    // ⑥ spawn.memory 官方形态（structures.js:865-885，本批新增）
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

    // ⑦ 盲区补锁（评审 3 清单）：RawMemory 每 tick 新建（对象身份）；require
    // 负向（.js 补全/ ../ 均不存在→Unknown module 原名）；非对象 JSON 原样
    // 保留（Memory === '5'）；顶层 __proto__=null（hasOwnProperty 不可用）；
    // String 包装对象过 _.isString 门
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

/// A6 结构读取面：isActive 官方四门 + 距离计数（utils.js:456-505）、place
/// 通道 generic 结构 hits/hitsMax 缺省表、build 负 remaining 官方算术
/// （ROAD-HITS-001 swamp/wall 的 quirk 通道：播种超限 → 负 effect 回钳完成）。
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

    // ---- place 通道 generic 结构 hits/hitsMax 缺省（读取面）----
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
    // 显式 hits 覆盖缺省（vanilla placeStructure 同款）；hitsMax 恒表值
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

    // ---- build effect==0 两分支（评审 3 P2-2：摘除早退后的行为翻转面锁）----
    // ① progress==total 精确相等：effect=0 → 一次 build 落成、能量不变
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

    // ② 零 WORK creep 的 build intent（API 层 ERR_NO_BODYPART 拦、结算侧仍
    // 记 actionLog + 事件 amount 0——官方 build.js 无 WORK 门直通算术）：
    // 经 settle_external 绕过 API 预检注入，锁结算侧行为
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
    // 零 WORK：effect=0——工地不动、能量不动（economy 门 energy>0 已过），
    // 但 actionLog + EVENT_BUILD(amount 0) 照记（官方无早退门；旧代码静默
    // return——正向对照防空批假绿：事件必须在场）
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
    // EVENT_BUILD=4；零 WORK 路径的 amount 0 事件照记（官方无早退门的行为
    // 翻转面锁——本 tick 的 run_player 自身不带 build intent，读的是上轮
    // settle_external 落库的事件流）
    assert_eq!(
        run.expr["value"], 1,
        "零 WORK build 仍推 EVENT_BUILD(amount 0): {:?}",
        run.expr
    );
    Ok(())
}

/// 原 kk-engine embed.rs mod tests 随迁（B0-5 移库）：元素级严格性、浮点
/// 整数 rcl、非法 spec 保世界。
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

    // JSON `3.0` 在 JS 语义里就是整数 3（vanilla 接受）——经玩家 API 面读回
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
        assert_eq!(engine.get_game_time(), 1);
    }
    Ok(())
}

/// ADR-013 复现场景①（部分提交三连之一）：p2 把 Game 搞成不可重定义 →
/// p2 世界注入失败——**玩家级失败玩家级恢复**（不冒泡整轮 Err）；轮次照常
/// 推进，p1 的 Memory 候选照常提交。
pub(super) fn b05_partial_commit_inject_failure_impl() -> anyhow::Result<()> {
    use serde_json::json;
    use std::collections::BTreeMap;
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    // 轮 A（武装轮）：p2 冻结 Game 的可重定义性——毒化当轮不生效（装配已过），
    // 影响下一轮 beginTick 的 Game 重装（GPT 复现序列的两 tick 形态）
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
    // 轮 B（毒化生效轮）：p2 世界注入失败——**玩家级失败玩家级恢复**（不冒泡
    // 整轮 Err）；轮次照常推进，p1 的 Memory 候选照常提交
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

/// ADR-013 复现场景②③（部分提交三连之二/三）：世界结算失败轮——先执行
/// 玩家的 Memory/世界版本全部不变；重试**不重跑玩家代码**（执行次数=1），
/// 只消费已缓存候选重试结算。
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
    assert_eq!(time_before, 1);

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
    // 提交验证：新一轮读回 n=2（缓存候选 1+1）；exec 计数=1 锁死「执行次数
    // 恰一次」——即使假想的「协调器缓存 codes 重跑原代码」回归也会推高 exec
    //（B0-5 评审 P2 清偿：console 计数与 Memory 计数双取证）
    let run = e.run_player("p1", "({ n: Memory.n, exec: Memory.exec })")?;
    assert_eq!(run.expr["value"]["n"], 2, "缓存候选（n=2）在重试轮提交");
    assert_eq!(
        run.expr["value"]["exec"], 1,
        "玩家代码执行次数恰一次（exec 计数）"
    );

    // ④discard 臂（B0-5 评审 P2 清偿）：失败轮可显式丢弃——丢弃后世界仍停
    // 在失败前、无 pending；新一轮全量重跑、pending 闸门解除。取证：失败轮 B
    // 的 exec+1 随候选**被丢弃**（不提交），新一轮 code 重跑（exec+100 提交）
    // → 读回 101（若 B 轮候选被误提交则是 102——两臂可区分）
    e.fail_next_settle_for_test();
    assert!(
        e.run_player("p1", "Memory.exec = Memory.exec + 1; 'x'")
            .is_err()
    );
    assert!(e.has_pending_round());
    // pending 闸门：变异/推进被拒（B0-5 评审 P1 清偿的锁定）
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

/// B0-6（外部评审清偿·重试缓存销毁）：被拒的重试请求不得销毁失败轮缓存——
/// 键集校验先于 take。原实现 take 后 bail，缓存随之中毁：错误请求 → pending
/// 丢失 → 原请求重试**重跑玩家代码**（探针：globalThis.n=2 而世界仅 +1 tick），
/// 违 tick 契约「不得为重试再次运行玩家代码」+ 验收场景 3「执行次数=1」。
pub(super) fn b06_rejected_retry_preserves_pending_impl() -> anyhow::Result<()> {
    use serde_json::json;
    use std::collections::BTreeMap;
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    e.run_player("p1", "'warmup'")?;
    assert_eq!(e.get_game_time(), 1);

    // 轮 A：结算失败 + 计数代码（globalThis 跨 tick 持久——V8 全局不随世界
    // 回滚，执行次数的取证通道；b05 注入失败场景的 defineProperty 已证该通道）
    let counter = "globalThis.n = (globalThis.n || 0) + 1; globalThis.n";
    e.fail_next_settle_for_test();
    let err = e.run_player("p1", counter).unwrap_err();
    assert!(err.to_string().contains("故障注入"), "{err}");
    assert!(e.has_pending_round(), "失败轮候选已缓存");
    assert_eq!(e.get_game_time(), 1, "失败轮世界不动");

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

    // 轮 C：原请求重试 → 消费缓存（返回执行 1 的 expr），玩家代码不再执行。
    // 请求 code 故意用计数代码本身：旧 bug 形态（缓存被销毁）下此处会**重跑
    // 计数**——expr=2、终检 n=2，双断言都是杀伤线（评审 3 P2-1 清偿）
    let run = e.run_player("p1", counter)?;
    assert_eq!(
        run.expr["value"], 1,
        "重试轮返回缓存候选的 expr（执行 1 的 n=1）；旧 bug 形态下重跑计数得 2: {:?}",
        run.expr
    );
    assert!(!e.has_pending_round(), "重试成功后缓存清空");
    assert_eq!(e.get_game_time(), 2, "warmup 1 + 重试轮推进 1");
    // 执行次数终检：globalThis.n 仍为 1（V8 全局跨执行持久、不随世界回滚——
    // b05 注入失败场景的 defineProperty 已锁该通道）
    let run = e.run_player("p1", "globalThis.n")?;
    assert_eq!(
        run.expr["value"], 1,
        "玩家代码执行次数恰一次（tick 契约验收场景 3；旧 bug 形态下 n=2）"
    );
    Ok(())
}

/// B0-6（外部评审清偿·TickInput 钉子）：失败轮缓存的结算输入在重试复用——
/// signController 的 datetime 落**首次尝试**的墙钟（ADR-013 决策 5「重放用
/// 记录值」），不再随重试重采样（原实现 settle_world 每次取 TickInput::now）。
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
    // signer creep 贴着 controller(1,1)（signController 邻接门，creeps.js:1087）
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 1], "name": "signer", "owner": "p1",
            "body": ["move"], "store": { "energy": 0 } }),
    )?;
    let sign_code = "Game.creeps['signer'].signController(Game.rooms['W1N1'].controller, 'b06')";
    // 预热一轮：把首跑的沙箱装配耗时挡在 datetime 判窗外（评审 3 P2-2 清偿
    // ——first_attempt_ms 与 TickInput::now() 的间隙=首轮执行耗时）
    e.run_player("p1", "'warmup'")?;

    // 轮 A：签名 intent + 结算失败 → 缓存轮记录首次尝试的 TickInput
    let first_attempt_ms = now_ms();
    e.fail_next_settle_for_test();
    let err = e.run_player("p1", sign_code).unwrap_err();
    assert!(err.to_string().contains("故障注入"), "{err}");
    // 隔开两批墙钟：若重试重采样，datetime ≥ 重试起点（与记录值差 ≥ sleep 间隔）
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
