//! 入口返回形态与玩家面基础 API：toString/Buffer/require、loop 静默跳过、Memory 属性。
//!
//! 场景由 sandbox_tests 的唯一测试串行调用，避免进程级计数互相干扰；
//! 这里不添加独立 #[test]。

use super::*;

/// 官方形态回归锁定：toString 家族（bot 日志拼接的事实标准）、Buffer toJSON 与
/// 负 length 边缘（Node 对照）、require 错误消息。
pub(super) fn official_string_and_error_shapes() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    tick(&mut sb, &world())?;

    // toString 家族（官方 engine/src/game 各 prototype.toString；缺失时
    // String(obj) 得 '[object Object]'）
    let v = sb.run_expr("String(new RoomPosition(1,2,'W1N1'))")?;
    assert_eq!(
        v["value"], "[room W1N1 pos 1,2]",
        "RoomPosition（rooms.js:1358）"
    );
    let v = sb.run_expr("String(Game.creeps['w1'])")?;
    assert_eq!(v["value"], "[creep w1]", "Creep（creeps.js:122）");
    let v = sb.run_expr("String(Game.rooms['W1N1'])")?;
    assert_eq!(v["value"], "[room W1N1]", "Room（rooms.js:536）");
    let v = sb.run_expr("String(Game.spawns['Spawn1'])")?;
    assert_eq!(
        v["value"], "[spawn Spawn1]",
        "StructureSpawn（structures.js:893）"
    );
    let v = sb.run_expr("String(Game.getObjectById('src1'))")?;
    assert_eq!(v["value"], "[source #src1]", "Source（sources.js:41）");
    let v = sb.run_expr("String(Game.rooms['W1N1'].controller)")?;
    assert_eq!(
        v["value"], "[structure (controller) #k1]",
        "Structure 子类继承根格式（structures.js:68）"
    );

    // Buffer.toJSON（Node/官方：{"type":"Buffer","data":[...]}；缺失时 TypedArray 形态）
    let v = sb.run_expr(
        "const j = JSON.parse(JSON.stringify(Buffer.from('hi'))); j.type + ':' + j.data.join(',')",
    )?;
    assert_eq!(v["value"], "Buffer:104,105", "Buffer.toJSON");

    // Buffer.from 负整数 length 钳 0（Node 实测空 Buffer；>>>0 会 wrap 成 4GB 分配）
    let v = sb.run_expr("Buffer.from({length: -1}).length")?;
    assert_eq!(v["value"], 0, "负 length 钳 0");
    let v = sb.run_expr("Buffer.from({length: 2.7, 0: 1, 1: 2, 2: 3}).length")?;
    assert_eq!(v["value"], 2, "小数 length 截断（Node 同款）");

    // require 失败消息：官方形态 Unknown module（game.js:587）
    let v = sb.run_expr("(() => { try { require('nope_mod'); return 'no-throw'; } catch (e) { return e.message; } })()")?;
    assert_eq!(v["value"], "Unknown module 'nope_mod'", "官方错误消息形态");

    // toJSON 返回非字符串：失败在返回值序列化阶段 → serialization（回归锁定：
    // 曾漏到 JSON.parse 被 "not valid JSON" 误报 runtime）
    let v = sb.run_expr("({toJSON(){return undefined}})")?;
    assert_eq!(v["ok"], false, "toJSON 产物非字符串: {v:?}");
    assert_eq!(
        v["kind"], "serialization",
        "序列化阶段失败归 serialization: {v:?}"
    );

    // FIND_SOURCES_ACTIVE 官方条件 energy > 0（game.js:23），energy 缺省 3000
    let world_sources = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "ca0", "type": "creep", "room": "W1N1", "x": 7, "y": 7, "name": "anchor",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 } },
            { "id": "sa0", "type": "structure", "structureType": "source",
              "room": "W1N1", "x": 5, "y": 5, "energy": 0 },
            { "id": "sa1", "type": "structure", "structureType": "source",
              "room": "W1N1", "x": 6, "y": 6, "energy": 500 }
        ]
    });
    sb.begin_tick(&world_sources)?;
    sb.run_loop()?;
    sb.end_tick()?;
    let v = sb.run_expr("Game.rooms['W1N1'].find(FIND_SOURCES_ACTIVE).length")?;
    assert_eq!(v["value"], 1, "energy:0 的 source 不算 active");
    let v = sb.run_expr("Game.rooms['W1N1'].find(FIND_SOURCES).length")?;
    assert_eq!(v["value"], 2, "FIND_SOURCES 收全部 source");
    Ok(())
}

/// 官方 game.js:555-563：main 未导出 loop() 时静默跳过（无 else 分支）——
/// 不是玩家错误，不得进 player_error 统计。
pub(super) fn run_loop_without_loop_silent() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.other = 1;"); // 无 loop
    sb.set_modules(table)?;
    let w = world();
    sb.begin_tick(&w)?;
    let out = sb.run_loop()?;
    assert!(
        matches!(out, TickOutcome::Completed { .. }),
        "无 loop 的 main 静默跳过（官方语义，非玩家错误）"
    );
    assert!(
        sb.take_last_loop_error().is_none(),
        "未导出 loop() 不产生玩家错误"
    );
    sb.end_tick()?;
    Ok(())
}

/// 检查 RoomVisual 链式调用、首访后的 Memory 只读属性，以及地形返回值。
/// 也验证直线 PathFinder 的路径形态，不验证避障或搜索成本。
pub(super) fn stub_value_shapes() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    tick(&mut sb, &world())?;

    // RoomVisual 绘制方法返回 this（链式调用是官方承诺行为）
    let v = sb.run_expr("(() => { const rv = new RoomVisual('W1N1'); return rv.circle(1,2).line(0,0,2,2) === rv; })()")?;
    assert_eq!(v["value"], true, "RoomVisual 链式调用");

    // 首访后的 Memory 值属性不可写；非严格代码整体赋值静默失败，保留原对象。
    // 此场景在同一 tick 内读取两次，没有验证跨 tick 对象身份。
    let v = sb
        .run_expr("(() => { void Memory.fresh; Memory = { fresh: 1 }; return Memory.fresh; })()")?;
    assert_eq!(
        v["value"],
        serde_json::Value::Null,
        "整体赋值被官方语义忽略"
    );
    let v = sb.run_expr("Memory.fresh")?;
    assert_eq!(v["value"], serde_json::Value::Null, "Memory 保持原对象");

    // 无地形数据的世界：getTerrainAt → undefined（官方对缺失地形的行为）。
    // 共享 world() 夹具已带全平原地形（寻路族需要），此处用无地形的局部世界。
    let no_terrain = serde_json::json!({
        "time": 2,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": []
    });
    sb.begin_tick(&no_terrain)?;
    let v = sb.run_expr("typeof Game.map.getTerrainAt(5,5,'W1N1')")?;
    assert_eq!(v["value"], "undefined", "无地形数据 → undefined");
    let v = sb.run_expr("typeof Game.map.getTerrainAt(-1,5,'W1N1')")?;
    assert_eq!(v["value"], "undefined", "越界 → undefined");

    // 有地形数据：官方词汇表 wall/swamp/plain（bit 语义 TERRAIN_MASK_*）
    let mut terrain = vec![0u32; 2500];
    terrain[0] = 1; // (0,0) wall
    terrain[10] = 2; // (10,0) swamp
    let world_with_terrain = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": []
    });
    sb.begin_tick(&world_with_terrain)?;
    sb.run_loop()?;
    sb.end_tick()?;
    let v = sb.run_expr("Game.map.getTerrainAt(0,0,'W1N1')")?;
    assert_eq!(v["value"], "wall");
    let v = sb.run_expr("Game.map.getTerrainAt(10,0,'W1N1')")?;
    assert_eq!(v["value"], "swamp");
    let v = sb.run_expr("Game.map.getTerrainAt(5,5,'W1N1')")?;
    assert_eq!(v["value"], "plain");

    // PathFinder：官方 path 不含起点（pf.cc:574 回溯在 origin 停止）；同点 → 空
    let v = sb.run_expr("JSON.stringify(PathFinder.search(new RoomPosition(1,1,'W1N1'), new RoomPosition(3,3,'W1N1')).path.map(p => [p.x, p.y]))")?;
    assert_eq!(v["value"], "[[2,2],[3,3]]", "path 不含起点: {:?}", v);
    let v = sb.run_expr(
        "PathFinder.search(new RoomPosition(5,5,'W1N1'), new RoomPosition(5,5,'W1N1')).path.length",
    )?;
    assert_eq!(v["value"], 0, "同起终点 → 空路径");
    let v = sb.run_expr(
        "PathFinder.search(new RoomPosition(1,1,'W1N1'), new RoomPosition(2,1,'W1N1')).path.length",
    )?;
    assert_eq!(v["value"], 1, "一步路径恰一个位置");

    // goal {pos, range}：进入 range 邻域即停（官方语义；bot 生态据此自拼端点）；
    // 裸 goal 即 range:0 走到目标格；origin 已在邻域内 → 空路径
    let v = sb.run_expr(
        "JSON.stringify(PathFinder.search(new RoomPosition(1,1,'W1N1'), {pos: new RoomPosition(5,5,'W1N1'), range: 1}).path.map(p => [p.x, p.y]))",
    )?;
    assert_eq!(v["value"], "[[2,2],[3,3],[4,4]]", "range:1 止于邻格: {v:?}");
    let v = sb.run_expr(
        "PathFinder.search(new RoomPosition(1,1,'W1N1'), {pos: new RoomPosition(3,3,'W1N1'), range: 2}).path.length",
    )?;
    assert_eq!(v["value"], 0, "origin 已在 range 内 → 空路径");

    // 负 range 夹 0（官方 driver/lib/path-finder.js:106 Math.max(0, range|0)）；
    // 非空路径的 ops ≥ 1（官方 ops 是搜索消耗，pf.cc:594）
    let v = sb.run_expr(
        "(() => { const r = PathFinder.search(new RoomPosition(1,1,'W1N1'), {pos: new RoomPosition(3,3,'W1N1'), range: -5}); return [r.path.length, r.ops > 0]; })()",
    )?;
    assert_eq!(
        v["value"],
        serde_json::json!([2, true]),
        "负 range 夹 0 走到目标格；ops 非零"
    );

    // runExpr 返回值检测遇病态对象（constructor:null）不崩、不误判硬中断：
    // 按规范字面归 serialization（constructor ∉ {Object, undefined}），且绝不触发
    // 整份 tickLimit 计费（回归锁定：病态对象曾被误判硬中断并按整份计费）
    let v = sb.run_expr("({constructor: null, x: 1})")?;
    assert_eq!(v["ok"], false, "病态对象: {v:?}");
    assert_eq!(
        v["kind"], "serialization",
        "检测防护生效（不逃逸、不误判硬中断）"
    );
    assert!(
        sb.bucket() > 9_600.0,
        "未被错误按整份 tickLimit 计费: {}",
        sb.bucket()
    );

    // Memory.toJSON 返回非字符串 → 玩家侧写回失败（非"写回阶段硬中断"）
    sb.run_expr("Memory.toJSON = () => undefined; 1")?;
    let wb = sb.end_tick()?;
    assert_eq!(wb["ok"], false, "写回失败: {wb}");
    assert_eq!(
        wb.get("terminated").and_then(serde_json::Value::as_bool),
        None,
        "不是硬中断: {wb}"
    );
    Ok(())
}

/// 官方 memory 别名是双向的（get+set）——tooangel 直接对 room.memory 赋值。
pub(super) fn memory_alias_setters() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    tick(&mut sb, &world())?;

    // room.memory setter（官方 rooms.js:551）
    let v = sb.run_expr("(() => { Game.rooms['W1N1'].memory = { role: 'home' }; return Memory.rooms['W1N1'].role; })()")?;
    assert_eq!(v["value"], "home", "room.memory 赋值落入 Memory.rooms");
    let v = sb.run_expr("Game.rooms['W1N1'].memory.role")?;
    assert_eq!(v["value"], "home", "room.memory 读别名一致");

    // creep.memory / flag.memory setter（严格模式下赋值不得抛 getter-only 错）
    let v = sb.run_expr(
        "(() => { Game.creeps['w1'].memory = { job: 1 }; return Game.creeps['w1'].memory.job; })()",
    )?;
    assert_eq!(v["value"], 1, "creep.memory setter");
    let v = sb.run_expr("(() => { 'use strict'; Memory.flags = {}; Game.flags['f1'] = undefined; return 'strict-ok'; })()")?;
    assert_eq!(v["value"], "strict-ok");
    Ok(())
}
