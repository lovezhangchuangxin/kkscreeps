//! kk-runner 种子运行时的集成测试。
//!
//! 所有沙箱场景放在**一个测试函数**里顺序执行：V8 isolate 的 LIFO 销毁纪律在
//! 并行测试下会互相干扰（全局注册栈），串行既保证确定性也便于定位。

use std::path::PathBuf;

use serde_json::json;

use kk_runner::{ModuleTable, Sandbox, SandboxConfig, TickOutcome, TickPlan, live_sandbox_count};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-bot")
}

fn world() -> serde_json::Value {
    json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 50 } },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 300 } },
            { "id": "k1", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "progress": 100, "progressTotal": 1000 },
            { "id": "src1", "type": "structure", "structureType": "source", "room": "W1N1",
              "x": 15, "y": 15, "energy": 2500 }
        ]
    })
}

fn load_sample_bot(sb: &mut Sandbox) -> anyhow::Result<()> {
    let table = ModuleTable::from_directory(&fixtures_dir())?;
    assert!(table.len() >= 4, "样例 bot 至少 4 个模块");
    sb.set_modules(table)
}

fn tick(sb: &mut Sandbox, w: &serde_json::Value) -> anyhow::Result<TickOutcome> {
    match sb.begin_tick(w)? {
        TickPlan::SkipPenalty => Ok(TickOutcome::Terminated), // 调用方语境下都算"没跑成"
        TickPlan::Run { .. } => {
            let mut out = sb.run_loop()?;
            // end_tick 自带结算（含写回耗时，官方口径）；写回阶段被硬中断 → Terminated
            let wb = sb.end_tick()?;
            if wb.get("terminated").and_then(serde_json::Value::as_bool) == Some(true) {
                out = TickOutcome::Terminated;
            }
            Ok(out)
        }
    }
}

#[test]
fn m0_sandbox_suite() -> anyhow::Result<()> {
    prelude_basics()?;
    cjs_and_loop()?;
    console_capture()?;
    stub_game_projection()?;
    intent_channel_prelude_forms_impl()?;
    hostile_probes_impl()?;
    run_expr_channel()?;
    wasm_binary_module()?;
    watchdog_hard_terminate_and_reuse()?;
    watchdog_late_expiry_does_not_kill_next_tick()?;
    memory_gate_terminates_and_reuses()?;
    lifo_registry_drains()?;
    memory_alias_setters()?;
    roomposition_official_semantics()?;
    terrain_look_and_console_semantics()?;
    writeback_watchdog_and_contract()?;
    stub_value_shapes()?;
    official_string_and_error_shapes()?;
    run_loop_without_loop_silent()?;
    writeback_rearmed_after_cancel()?;
    // LIFO 零违规断言在**全部**场景之后——中段断言会让其后场景的违规漏网
    assert_eq!(kk_runner::lifo_violations(), 0, "全流程 LIFO 纪律零违规");
    Ok(())
}

/// 写回前交界竞态的确定性复现：run 成功后、写回前，terminate 已投递（看门狗
/// 同代只投一次，不会再 terminate）——end_tick 的 cancel 清掉这个唯一信号后，
/// 挂死的写回必须由重新武装的新代兜底掐断（否则线程永久挂死）。
fn writeback_rearmed_after_cancel() -> anyhow::Result<()> {
    let cfg = SandboxConfig {
        max_per_tick: 150.0,
        initial_bucket: 150.0,
        ..SandboxConfig::default()
    };
    let mut sb = Sandbox::new(cfg)?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { Memory.toJSON = function () { while (true) {} }; };",
    );
    sb.set_modules(table)?;
    sb.begin_tick(&world())?;
    let out = sb.run_loop()?; // loop 快速返回（只装挂死用的 toJSON）
    assert!(matches!(out, TickOutcome::Completed { .. }));
    // 模拟"看门狗恰在 run 成功后投递 terminate"：手动武装一个已过期的 deadline
    // 并等它 fire——此后该代已 fired，看门狗不会再投递（竞态窗口的前提）
    sb.watchdog().arm(std::time::Instant::now());
    std::thread::sleep(std::time::Duration::from_millis(50));
    let t0 = std::time::Instant::now();
    let wb = sb.end_tick()?;
    let elapsed = t0.elapsed().as_millis();
    assert_eq!(
        wb.get("terminated").and_then(serde_json::Value::as_bool),
        Some(true),
        "写回挂死被重新武装的新代兜底掐断: {wb:?}"
    );
    assert!(elapsed < 5_000, "不挂死线程（{elapsed}ms）");
    Ok(())
}

/// 官方形态回归锁定：toString 家族（bot 日志拼接的事实标准）、Buffer toJSON 与
/// 负 length 边缘（Node 对照）、require 错误消息。
fn official_string_and_error_shapes() -> anyhow::Result<()> {
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
fn run_loop_without_loop_silent() -> anyhow::Result<()> {
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

/// 桩返回值形态抽查：
/// RoomVisual 链式返回 this、Memory 首访后可整体赋值（官方 game.js:492-495）、
/// getTerrainAt 官方词汇表与边界语义（map.js:235-258）。
fn stub_value_shapes() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    tick(&mut sb, &world())?;

    // RoomVisual 绘制方法返回 this（链式调用是官方承诺行为）
    let v = sb.run_expr("(() => { const rv = new RoomVisual('W1N1'); return rv.circle(1,2).line(0,0,2,2) === rv; })()")?;
    assert_eq!(v["value"], true, "RoomVisual 链式调用");

    // Memory 整体赋值：官方描述符无 writable（game.js:492-495）——首访后只读，
    // sloppy 下静默失败、Memory 保持原对象（跨 tick 也不变）
    let v = sb
        .run_expr("(() => { void Memory.fresh; Memory = { fresh: 1 }; return Memory.fresh; })()")?;
    assert_eq!(
        v["value"],
        serde_json::Value::Null,
        "整体赋值被官方语义忽略"
    );
    let v = sb.run_expr("Memory.fresh")?;
    assert_eq!(v["value"], serde_json::Value::Null, "Memory 保持原对象");

    // 无地形数据的世界：getTerrainAt → undefined（官方对缺失地形的行为）
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
fn memory_alias_setters() -> anyhow::Result<()> {
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

/// RoomPosition 官方语义抽查：isEqualTo 房间判定、getDirectionTo 阈值算法。
fn roomposition_official_semantics() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    tick(&mut sb, &world())?;

    // isEqualTo：目标带 roomName 时房间必须相等（官方 rooms.js:1579）
    let v = sb.run_expr("new RoomPosition(5,5,'W1N1').isEqualTo(new RoomPosition(5,5,'W2N1'))")?;
    assert_eq!(v["value"], false, "跨房间同坐标不相等");
    let v = sb.run_expr("new RoomPosition(5,5,'W1N1').isEqualTo(new RoomPosition(5,5,'W1N1'))")?;
    assert_eq!(v["value"], true);
    let v = sb.run_expr("new RoomPosition(5,5,'W1N1').isEqualTo(5,5)")?;
    assert_eq!(v["value"], true, "数值形态视为同房间");

    // getDirectionTo：官方阈值算法（|dx| > 2|dy| → 左右；(7,3) 应为 RIGHT 而非对角）
    let v = sb.run_expr("new RoomPosition(0,0,'W1N1').getDirectionTo(7,3)")?;
    assert_eq!(v["value"], 3, "(7,3) → RIGHT（官方 utils.js 阈值）");
    let v = sb.run_expr("new RoomPosition(0,0,'W1N1').getDirectionTo(3,7)")?;
    assert_eq!(v["value"], 5, "(3,7) → BOTTOM");
    let v = sb.run_expr("new RoomPosition(0,0,'W1N1').getDirectionTo(3,3)")?;
    assert_eq!(v["value"], 4, "(3,3) → BOTTOM_RIGHT");

    // 跨房间 getDirectionTo（官方 rooms.js:1383-1393）：世界坐标换算后再阈值判定。
    // roomNameToXY：W1N1=[-2,-2]、E1N1=[1,-2] ⇒ dx=150, dy=0 → RIGHT
    let v = sb.run_expr(
        "new RoomPosition(25,25,'W1N1').getDirectionTo(new RoomPosition(25,25,'E1N1'))",
    )?;
    assert_eq!(v["value"], 3, "跨房间东邻 → RIGHT");
    // W1N2=[-2,-3] ⇒ dx=0, dy=-50 → TOP
    let v = sb.run_expr(
        "new RoomPosition(25,25,'W1N1').getDirectionTo(new RoomPosition(25,25,'W1N2'))",
    )?;
    assert_eq!(v["value"], 1, "跨房间北邻 → TOP");
    // W2N1=[-3,-2] ⇒ dx=-50, dy=0 → LEFT
    let v = sb.run_expr(
        "new RoomPosition(25,25,'W1N1').getDirectionTo(new RoomPosition(25,25,'W2N1'))",
    )?;
    assert_eq!(v["value"], 7, "跨房间西邻 → LEFT");
    // E0S0=[0,0]、对角世界坐标 (80,80) → BOTTOM_RIGHT
    let v =
        sb.run_expr("new RoomPosition(25,25,'W1N1').getDirectionTo(new RoomPosition(5,5,'E0S0'))")?;
    assert_eq!(v["value"], 4, "跨房间对角 → BOTTOM_RIGHT");
    // 数值形态（无 roomName）视为同房间——跨房间换算不触发
    let v = sb.run_expr("new RoomPosition(0,0,'W1N1').getDirectionTo(3,3)")?;
    assert_eq!(v["value"], 4, "数值形态同房间（不换算）");
    Ok(())
}

/// M1 步 5 的 prelude 配套：Terrain 对象（rooms.js:1223-1241）、look 面 terrain
/// 项、room.controller undefined、console 官方格式化（console.js:16-22）。
fn terrain_look_and_console_semantics() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    let mut terrain = vec![0u32; 2500];
    terrain[0] = 1; // (0,0) wall
    terrain[10] = 2; // (10,0) swamp
    let w = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            // 视野锚点：官方无对象玩家看不到任何房（runtimeData.rooms 为空）
            { "id": "c9", "type": "creep", "room": "W1N1", "x": 30, "y": 30, "name": "anchor",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 } }
        ]
    });
    tick(&mut sb, &w)?;

    // room.controller：无 controller 房间投影 undefined（非 null）
    let v = sb.run_expr("typeof Game.rooms['W1N1'].controller")?;
    assert_eq!(v["value"], "undefined", "无 controller → undefined");

    // room.find(type, {filter})：官方经 _.filter 应用（rooms.js:648-654）——
    // 函数谓词与 lodash 对象谓词两形态（换带对象的世界验证）
    let w2 = serde_json::json!({
        "time": 2,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            { "id": "k1", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "owner": "p1" },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 300 } },
        ]
    });
    sb.begin_tick(&w2)?;
    sb.run_loop()?;
    sb.end_tick()?;
    let v = sb.run_expr("Game.rooms['W1N1'].find(FIND_STRUCTURES).length")?;
    assert_eq!(v["value"], 2, "无 filter 返回全部结构");
    let v = sb.run_expr(
        "Game.rooms['W1N1'].find(FIND_STRUCTURES, {filter: (s) => s.structureType === 'spawn'}).length",
    )?;
    assert_eq!(v["value"], 1, "函数谓词过滤生效");
    let v = sb.run_expr(
        "Game.rooms['W1N1'].find(FIND_STRUCTURES, {filter: {structureType: 'spawn'}})[0].name",
    )?;
    assert_eq!(v["value"], "Spawn1", "lodash 对象谓词过滤生效");

    // RoomPosition.findInRange（官方 rooms.js:1518-1543）：数字 type + Chebyshev
    // 半径过滤；数组 type 直传；不可达房抛错
    let v = sb.run_expr(
        "Game.rooms['W1N1'].getPositionAt(10,10).findInRange(FIND_STRUCTURES, 1).length",
    )?;
    assert_eq!(v["value"], 1, "半径 1 内只有 controller");
    let v = sb.run_expr(
        "Game.rooms['W1N1'].getPositionAt(10,10).findInRange(FIND_STRUCTURES, 15).map((s) => s.structureType)",
    )?;
    assert_eq!(
        v["value"],
        serde_json::json!(["controller", "spawn"]),
        "半径 15 覆盖两结构（find 序）"
    );
    let v = sb.run_expr(
        "(() => { try { Game.rooms['W1N1'].getPositionAt(1,1).findInRange.call({roomName: 'E9S9', x: 1, y: 1}, FIND_SOURCES, 5); return 'no'; } catch (e) { return e.message; } })()",
    )?;
    assert_eq!(v["value"], "Could not access room E9S9", "不可达房抛错");

    // Room.Terrain 静态挂法（rooms.js:1135）
    let v = sb.run_expr("typeof Room.Terrain")?;
    assert_eq!(v["value"], "function", "Room.Terrain 是静态属性");

    // Terrain.get：位码语义 wall=1/swamp=2/plain=0（rooms.js:1230-1233）
    let v = sb.run_expr("[Game.rooms['W1N1'].getTerrain().get(0,0), Game.rooms['W1N1'].getTerrain().get(10,0), Game.rooms['W1N1'].getTerrain().get(5,5)]")?;
    assert_eq!(v["value"], serde_json::json!([1, 2, 0]), "get 位码");

    // getRawBuffer：无参新建、有参填充返回原数组（rooms.js:1235-1239）
    let v = sb.run_expr("Game.rooms['W1N1'].getTerrain().getRawBuffer().length")?;
    assert_eq!(v["value"], 2500, "getRawBuffer 2500 字节");
    let v = sb.run_expr("(() => { const u = new Uint8Array(2500); const t = Game.map.getRoomTerrain('W1N1'); return [t.getRawBuffer(u) === u, u[0], u[10]]; })()")?;
    assert_eq!(
        v["value"],
        serde_json::json!([true, 1, 2]),
        "getRawBuffer 填充给定数组（map.js:262-264 getRoomTerrain 同一 Terrain）"
    );

    // 无地形数据的房间：构造即抛错（rooms.js:1226-1228 官方错误文案）
    let v = sb.run_expr("(() => { try { new Room.Terrain('E9S9'); return 'no'; } catch (e) { return e.message; } })()")?;
    assert_eq!(v["value"], "Could not access room E9S9", "未知房间报错");

    // RoomPosition.lookFor(LOOK_TERRAIN)：单词数组（rooms.js:1601-1610）
    let v = sb.run_expr("JSON.stringify([new RoomPosition(0,0,'W1N1').lookFor(LOOK_TERRAIN), new RoomPosition(10,0,'W1N1').lookFor(LOOK_TERRAIN), new RoomPosition(5,5,'W1N1').lookFor(LOOK_TERRAIN)])")?;
    assert_eq!(
        v["value"], r#"[["wall"],["swamp"],["plain"]]"#,
        "lookFor(LOOK_TERRAIN) 词汇表"
    );

    // room.lookForAt(LOOK_TERRAIN) 是单词数组（rooms.js:798-806 → :681 return [result]）；
    // room.lookAt 的 terrain 项是对象形态（rooms.js:663-681 带 outArray 分支）
    let v = sb.run_expr("JSON.stringify(Game.rooms['W1N1'].lookForAt(LOOK_TERRAIN, 0, 0))")?;
    assert_eq!(v["value"], r#"["wall"]"#);
    let v =
        sb.run_expr("JSON.stringify(Game.rooms['W1N1'].lookAt(new RoomPosition(10,0,'W1N1')))")?;
    assert_eq!(v["value"], r#"[{"type":"terrain","terrain":"swamp"}]"#);

    // console 官方格式化（console.js:16-22 逐参映射）：真值走 toString、
    // undefined → 'undefined'、falsy 走 JSON.stringify（NaN → 'null'）
    sb.run_expr(
        "console.log(null, 0, undefined, NaN, '', 'x', [1,2], new RoomPosition(1,2,'W1N1'))",
    )?;
    let lines = sb.drain_console();
    assert_eq!(
        lines[0].text, "null 0 undefined null \"\" x 1,2 [room W1N1 pos 1,2]",
        "官方 console 格式化: {:?}",
        lines
    );
    Ok(())
}

/// 写回阶段的看门狗覆盖与契约分类：
/// ① 玩家经 Memory.toJSON 在写回阶段挂死 → 被硬中断（不挂死进程）；
/// ② Memory 不可序列化（1n）→ tick 末失败按契约归玩家 runtime 而非成功；
/// ③ 已开始执行后硬中断 → runtime（executed 语义）；
/// ④ 活对象返回值 → serialization（对齐 vanilla 的 constructor 检测）。
fn writeback_watchdog_and_contract() -> anyhow::Result<()> {
    // ① 写回挂死被看门狗掐断
    let cfg = SandboxConfig {
        max_per_tick: 150.0,
        initial_bucket: 150.0,
        ..SandboxConfig::default()
    };
    let mut sb = Sandbox::new(cfg)?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { Memory.toJSON = function () { while (true) {} }; };",
    );
    sb.set_modules(table)?;
    let t0 = std::time::Instant::now();
    let out = tick(&mut sb, &world())?;
    let elapsed = t0.elapsed().as_millis();
    assert!(matches!(out, TickOutcome::Terminated), "写回挂死被硬中断");
    assert!(elapsed < 5_000, "不挂死进程（{elapsed}ms）");

    Ok(())
}

/// lodash 预置、官方常量平铺、Deno 遮蔽、全局面白名单、Buffer 垫片语义、运行时健康。
fn prelude_basics() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;

    let v = sb.run_expr("_.VERSION")?;
    assert_eq!(v["value"], "3.10.1", "lodash 3.10.1 预置");

    let v = sb.run_expr("_.map([1,2,3], x => x * 2)")?;
    assert_eq!(v["value"], json!([2, 4, 6]));

    let v = sb.run_expr("[OK, FIND_CREEPS, BODYPARTS_ALL.length].join(',')")?;
    assert_eq!(v["value"], "0,101,8", "官方常量平铺为裸全局");

    let v = sb.run_expr("typeof Deno")?;
    assert_eq!(v["value"], "undefined", "Deno.core 遮蔽");

    let v = sb.run_expr("typeof __bootstrap")?;
    assert_eq!(
        v["value"], "undefined",
        "deno_core 的 __bootstrap 命名空间（挂 core/internals，104 个内部 op 含 op_panic）遮蔽"
    );

    // 全局面白名单比对：deno_core 升级若残留新的内部全局（如 __bootstrap 同类），
    // 此处显形（ADR-002：内部函数集合启动时必须遮蔽）
    let v = sb.run_expr("Object.getOwnPropertyNames(globalThis)")?;
    let names: Vec<String> = serde_json::from_value(v["value"].clone())?;
    let unexpected: Vec<&String> = names.iter().filter(|n| !is_allowed_global(n)).collect();
    assert!(
        unexpected.is_empty(),
        "遮蔽后仍有非白名单全局: {unexpected:?}（全部 {names:?}）"
    );

    let v = sb.run_expr("require('lodash') === _")?;
    assert_eq!(v["value"], true, "require('lodash') 命中预置");
    // require('lodash') 在缓存查询前短路，唯一可观察形态是 require.cache——
    // 该形状同时是 resetModules 重建产物的直接校验面
    let v = sb.run_expr("require.cache.lodash && require.cache.lodash.exports === _")?;
    assert_eq!(
        v["value"], true,
        "require.cache 的 lodash 预置存在且为同一对象"
    );

    // ---- Buffer 垫片语义（对齐官方 Node 行为） ----
    let v = sb.run_expr("typeof Buffer !== 'undefined' && Buffer.from('hi').length")?;
    assert_eq!(v["value"], 2, "Buffer 垫片可用");

    // utf8Decode：非法/截断序列产出 U+FFFD 而非抛 RangeError（官方行为）
    let v = sb.run_expr("Buffer.from([255]).toString()")?;
    assert_eq!(v["value"], "\u{fffd}", "非法 UTF-8 解码为替换字符");
    let v = sb.run_expr("Buffer.from([0xf4,0x90,0x80,0x80]).toString()")?;
    assert!(v["ok"] == true, "码点超界序列不抛: {v:?}");

    // from 的 encoding 分派（与 toString 对称；此前被静默忽略）
    let v = sb.run_expr("JSON.stringify(Array.from(Buffer.from('aGk=','base64')))")?;
    assert_eq!(v["value"], "[104,105]", "base64 解码");
    let v = sb.run_expr("JSON.stringify(Array.from(Buffer.from('414243','hex')))")?;
    assert_eq!(v["value"], "[65,66,67]", "hex 解码");
    let v = sb
        .run_expr("try { Buffer.from('x','nope'); 'no-throw' } catch (e) { e.constructor.name }")?;
    assert_eq!(v["value"], "TypeError", "未支持 encoding 显式抛错");

    // alloc 的字符串 fill 按字节循环填充（官方：alloc(8,'x') → 8 个 120）
    let v = sb.run_expr("JSON.stringify(Array.from(Buffer.alloc(8,'x')))")?;
    assert_eq!(
        v["value"], "[120,120,120,120,120,120,120,120]",
        "单字符 fill"
    );
    let v = sb.run_expr("JSON.stringify(Array.from(Buffer.alloc(2,'65')))")?;
    assert_eq!(
        v["value"], "[54,53]",
        "多字符 fill 按字符字节（非数值解析）"
    );

    // console.log 任意值不抛（DataView 无 slice，官方不崩）
    let v = sb.run_expr("(function(){ try { console.log(new DataView(new ArrayBuffer(8))); return 'ok'; } catch (e) { return e.message; } })()")?;
    assert_eq!(v["value"], "ok", "console.log(DataView) 不抛");

    // 官方别名：同一引用（Spawn=StructureSpawn、Energy=Resource）
    let v = sb.run_expr("Spawn === StructureSpawn && Energy === Resource")?;
    assert_eq!(v["value"], true, "官方类别名指向同一引用");

    // runExpr 间接 eval：全局作用域求值，prelude 闭包（含捕获的 ops 引用）不可达
    let v = sb.run_expr("ops")?;
    assert_eq!(v["ok"], false, "runExpr 求值在全局作用域: {v:?}");
    assert!(
        v["error"]
            .as_str()
            .is_some_and(|e| e.contains("ops is not defined")),
        "ops 未定义（prelude 闭包不可达）: {v:?}"
    );

    // __kk 整体冻结：玩家不可覆盖验收仪表/宿主 tick 通道
    let v = sb.run_expr("Object.isFrozen(globalThis.__kk)")?;
    assert_eq!(v["value"], true, "__kk 冻结（防仪表造假/通道劫持）");

    let v = sb.run_expr("6 * 7")?;
    assert_eq!(v["value"], 42, "遮蔽后运行时健康");
    Ok(())
}

/// 全局面白名单：ECMA 内建 + 平铺常量（全大写下划线）+ 官方游戏类 + 环境注入物。
/// deno_core 升级新增的全局若不在其中，遮蔽复查测试即失败（人工复核后增删）。
fn is_allowed_global(name: &str) -> bool {
    const ECMA_BUILTINS: &[&str] = &[
        "globalThis",
        "Infinity",
        "NaN",
        "undefined",
        "eval",
        "isFinite",
        "isNaN",
        "parseFloat",
        "parseInt",
        "decodeURI",
        "decodeURIComponent",
        "encodeURI",
        "encodeURIComponent",
        "escape",
        "unescape",
        "Object",
        "Function",
        "Array",
        "String",
        "Number",
        "Boolean",
        "Symbol",
        "BigInt",
        "Error",
        "EvalError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "TypeError",
        "URIError",
        "AggregateError",
        "SuppressedError",
        "JSON",
        "Math",
        "Date",
        "RegExp",
        "ArrayBuffer",
        "SharedArrayBuffer",
        "DataView",
        "Map",
        "Set",
        "Iterator",
        "WeakMap",
        "WeakSet",
        "WeakRef",
        "FinalizationRegistry",
        "Promise",
        "Reflect",
        "Proxy",
        "Intl",
        "Temporal",
        "Atomics",
        "DisposableStack",
        "AsyncDisposableStack",
        "Uint8Array",
        "Int8Array",
        "Uint8ClampedArray",
        "Uint16Array",
        "Int16Array",
        "Uint32Array",
        "Int32Array",
        "BigUint64Array",
        "BigInt64Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "WebAssembly",
        "console",
    ];
    const ENV_NAMES: &[&str] = &[
        // prelude 注入
        "_",
        "Buffer",
        "require",
        "Game",
        "Memory",
        "RawMemory",
        "__kk",
        // deno_core 注入（V8 内建微任务调度，无内部面可达）
        "queueMicrotask",
        // 官方游戏类（GAME_CLASS_NAMES + 独立类 + 两个别名）
        "ConstructionSite",
        "Creep",
        "Deposit",
        "Flag",
        "Mineral",
        "Nuke",
        "OwnedStructure",
        "PowerCreep",
        "Resource",
        "Ruin",
        "Room",
        "RoomObject",
        "Source",
        "Store",
        "Structure",
        "StructureContainer",
        "StructureController",
        "StructureExtension",
        "StructureExtractor",
        "StructureFactory",
        "StructureInvaderCore",
        "StructureKeeperLair",
        "StructureLab",
        "StructureLink",
        "StructureNuker",
        "StructureObserver",
        "StructurePortal",
        "StructurePowerBank",
        "StructurePowerSpawn",
        "StructureRampart",
        "StructureRoad",
        "StructureSpawn",
        "StructureStorage",
        "StructureTerminal",
        "StructureTower",
        "StructureWall",
        "Tombstone",
        "Spawn",
        "Energy",
        "RoomPosition",
        "RoomVisual",
        "PathFinder",
    ];
    if ECMA_BUILTINS.contains(&name) || ENV_NAMES.contains(&name) {
        return true;
    }
    // 官方常量平铺（443 个，全大写 + 下划线，如 OK/FIND_CREEPS/BODYPARTS_ALL）。
    // 排除 __ 前缀：deno_core 的内部全局惯例是双下划线开头（如 __bootstrap），
    // 不能被全大写兜底放行
    name.len() > 1
        && !name.starts_with("__")
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// 样例 bot：模块加载、循环依赖语义、失败缓存删除、子目录模块、global/Memory 跨 tick。
fn cjs_and_loop() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    load_sample_bot(&mut sb)?;

    let w = world();
    for expected_tick in 1..=3 {
        let out = tick(&mut sb, &w)?;
        assert!(matches!(out, TickOutcome::Completed { .. }));
        let v = sb.run_expr("globalThis.__tickCount")?;
        assert_eq!(v["value"], expected_tick, "loop 每 tick 执行一次");
    }

    // ---- 循环依赖半成品 exports 语义（官方 game.js / Node 同款核心语义） ----
    // 断言一：循环点读半成品属性 → undefined（role_harvester 的 role 赋值在 require('utils') 之后）
    let v = sb.run_expr("require('utils').roleSeenDuringCycle === undefined")?;
    assert_eq!(
        v["value"], true,
        "循环点读到的是半成品 exports（属性尚未赋值）"
    );
    // 断言二：循环点持有的对象引用在模块完成后可见后续赋值——若加载器在循环点
    // 返回新空对象（另一种破坏形态），lateProbe 不是同一引用，此断言能抓住
    let v = sb.run_expr("require('utils').lateProbe.role")?;
    assert_eq!(
        v["value"], "harvester",
        "循环点持有的引用在模块完成后可见后续赋值（同一对象）"
    );

    let v = sb.run_expr("JSON.stringify(require('role_harvester'))")?;
    let role = v["value"].as_str().unwrap_or_default();
    assert!(
        role.contains("\"role\":\"harvester\""),
        "模块导出正确: {role}"
    );

    // ---- 模块执行失败删缓存（官方 game.js:578-581 / Node 原生语义） ----
    // 瞬时初始化错误后，下一 tick 重新执行而非永久复用半成品 exports
    let mut table = ModuleTable::new();
    table.insert_js(
        "flaky",
        "if (!globalThis.__flakyOk) throw new Error('transient'); module.exports.ran = true;",
    );
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    let v = sb.run_expr("require('flaky')")?;
    assert_eq!(v["ok"], false, "首次 require 抛错: {v:?}");
    let v = sb.run_expr("'flaky' in require.cache")?;
    assert_eq!(
        v["value"], false,
        "失败的模块不留在缓存（否则永远复用半成品 exports）"
    );
    sb.run_expr("globalThis.__flakyOk = true")?;
    let v = sb.run_expr("require('flaky').ran")?;
    assert_eq!(v["value"], true, "条件恢复后重新执行模块");

    // 恢复样例 bot，继续后续场景
    load_sample_bot(&mut sb)?;

    // 子目录模块
    let v = sb.run_expr("require('sub/mod').name")?;
    assert_eq!(v["value"], "sub-module");

    // 相对路径解析
    let v = sb.run_expr("require('./utils').identity(7)")?;
    assert_eq!(v["value"], 7);

    // global 跨 tick 持久
    sb.run_expr("globalThis.__marker = 123")?;
    let out = tick(&mut sb, &w)?;
    assert!(matches!(out, TickOutcome::Completed { .. }));
    let v = sb.run_expr("globalThis.__marker")?;
    assert_eq!(v["value"], 123, "global 跨 tick 持久（官方语义）");

    // ---- Memory 跨 tick 写回-重解析生命周期（bot 状态生命线） ----
    tick(&mut sb, &w)?; // 世界已注入（Game.creeps 可用）
    sb.run_expr("Memory.crossTick = 42; Game.creeps['w1'].memory.job = 'mine'")?;
    sb.end_tick()?; // endTick 写回 RawMemory（含结算）
    let out = tick(&mut sb, &w)?; // 下一 tick 懒重解析
    assert!(matches!(out, TickOutcome::Completed { .. }));
    let v = sb.run_expr("Memory.crossTick")?;
    assert_eq!(v["value"], 42, "Memory 跨 tick 写回-重解析一致");
    let v = sb.run_expr("Game.creeps['w1'].memory.job")?;
    assert_eq!(v["value"], "mine", "creep.memory 懒别名跨 tick 一致");

    // ---- Memory 写回失败保护：玩家一行代码（BigInt）不得使 endTick 升级为 Err ----
    // 失败以 {ok:false} 回传（而非基础设施 Err），由 runPlayer 通道归为玩家 runtime 错误
    sb.run_expr("Memory.big = 1n")?;
    let wb = sb.end_tick()?;
    assert_eq!(wb["ok"], false, "写回失败按玩家侧错误回传而非 Err: {wb}");
    let lines = sb.drain_console();
    assert!(
        lines
            .iter()
            .any(|l| l.level == "error" && l.text.contains("Memory 写回失败")),
        "写回失败打 console: {lines:?}"
    );
    let out = tick(&mut sb, &w)?;
    assert!(matches!(out, TickOutcome::Completed { .. }));
    let v = sb.run_expr("Memory.big")?;
    assert_eq!(
        v["value"],
        serde_json::Value::Null,
        "写回被跳过：保留上一 tick 存档，非法值不跨 tick"
    );
    Ok(())
}

fn console_capture() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { console.log('hello', { a: 1 }); console.error('bad'); };",
    );
    sb.set_modules(table)?;
    tick(&mut sb, &world())?;
    let lines = sb.drain_console();
    let log_text = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // 官方 console 格式化（console.js:16-22）：对象参数走 toString → '[object Object]'
    assert!(
        log_text.contains("hello [object Object]"),
        "log 捕获: {log_text}"
    );
    assert!(log_text.contains("bad"), "error 捕获");

    // 每 tick 行数闸门：高频输出不越限（防跨边界放大约 OOM 宿主），超限出告知行
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { for (let i = 0; i < 5000; i++) console.log('spam'); };",
    );
    sb.set_modules(table)?;
    tick(&mut sb, &world())?;
    let lines = sb.drain_console();
    assert!(
        lines.len() <= 1_001,
        "console 行数闸门（1000 行 + 截断告知）: 实收 {}",
        lines.len()
    );
    assert!(
        lines.last().is_some_and(|l| l.text.contains("已丢弃")),
        "截断有告知行: {:?}",
        lines.last()
    );
    Ok(())
}

/// stub Game 投影：对象可见、find 可用、缺失属性被探测。
fn stub_game_projection() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    load_sample_bot(&mut sb)?;
    let w = world();

    tick(&mut sb, &w)?;
    let v = sb.run_expr("Game.time")?;
    assert_eq!(v["value"], 1);

    let v = sb.run_expr("Game.creeps['w1'].pos.x + ',' + Game.creeps['w1'].pos.y")?;
    assert_eq!(v["value"], "25,25", "creep 投影");

    let v = sb.run_expr("Game.getObjectById('c1').body.length")?;
    assert_eq!(v["value"], 2);

    let v = sb.run_expr("Game.spawns['Spawn1'].store.energy")?;
    assert_eq!(v["value"], 300);

    let v = sb.run_expr("Game.rooms['W1N1'].controller.level")?;
    assert_eq!(v["value"], 3);

    let v = sb.run_expr("Game.rooms['W1N1'].find(FIND_CREEPS).length")?;
    assert_eq!(v["value"], 1, "room.find 投影");

    // 桩方法：返回 OK 且被计数
    let v = sb.run_expr("Game.creeps['w1'].move(TOP)")?;
    assert_eq!(v["value"], 0, "桩方法返回 OK");

    // 缺失属性探测：访问 Game 上不存在的属性会被登记（missing 桶 = get-trap 确定缺失）
    sb.run_expr("void Game.totallyMissing")?;
    let missing = sb.drain_missing_api()?;
    assert_eq!(
        missing["missing"]["Game.totallyMissing"], 1,
        "缺失 API 探测: {missing}"
    );

    // `in` 检查（has trap）进独立低置信桶 hasProbes——否定式能力探测与真实缺失
    // 在 trap 内不可区分，不计入缺失清单；drain 后两桶都清空
    sb.run_expr("'notThere' in Game")?;
    let missing = sb.drain_missing_api()?;
    assert_eq!(
        missing["hasProbes"]["Game.notThere"], 1,
        "in 检查进 hasProbes 桶: {missing}"
    );
    assert!(
        missing["missing"].as_object().is_some_and(|m| m.is_empty()),
        "has 探测不计入缺失: {missing}"
    );
    let again = sb.drain_missing_api()?;
    assert_eq!(
        again,
        serde_json::json!({ "missing": {}, "hasProbes": {} }),
        "drain 后两桶清空: {again}"
    );

    // pos 实例面（RoomPosition）与 Buffer 静态面纳入探测（findInRange 已实装，
    // 用真缺失的方法名验证探针）
    sb.run_expr("void Game.creeps['w1'].pos.findPathToDestination")?;
    sb.run_expr("void Buffer.poolAllocThings")?;
    let missing = sb.drain_missing_api()?;
    assert_eq!(
        missing["missing"]["RoomPosition.findPathToDestination"], 1,
        "pos 实例面探测: {missing}"
    );
    assert_eq!(
        missing["missing"]["Buffer.poolAllocThings"], 1,
        "Buffer 静态面探测: {missing}"
    );

    // spawn 对象的官方别名 instanceof（Spawn=StructureSpawn 同一引用）
    let v = sb.run_expr("Game.spawns['Spawn1'] instanceof Spawn")?;
    assert_eq!(v["value"], true, "instanceof 别名（官方语义）");

    // ---- RoomPosition 数值坐标形态（x,y 双参视为同房间坐标，官方 utils.js/rooms.js） ----
    let v = sb.run_expr("(new RoomPosition(10,10,'W1N1')).getRangeTo(12,12)")?;
    assert_eq!(
        v["value"], 2,
        "数值形态 getRangeTo（官方 2，曾恒 Infinity）"
    );
    let v = sb.run_expr("(new RoomPosition(10,10,'W1N1')).isNearTo(11,11)")?;
    assert_eq!(v["value"], true, "数值形态 isNearTo");
    let v = sb.run_expr("(new RoomPosition(10,10,'W1N1')).isInRangeTo(12,20,1)")?;
    assert_eq!(
        v["value"], false,
        "三参形态 range 取第三参（曾错绑到 y，恒 true）"
    );
    let v = sb.run_expr("(new RoomPosition(10,10,'W1N1')).isInRangeTo(11,11,1)")?;
    assert_eq!(v["value"], true, "三参形态在范围内");
    let v = sb.run_expr("(new RoomPosition(10,10,'W1N1')).inRangeTo(11,11,1)")?;
    assert_eq!(v["value"], true, "inRangeTo 别名受益");
    let v =
        sb.run_expr("(new RoomPosition(10,10,'W1N1')).getRangeTo(new RoomPosition(10,10,'W2N1'))")?;
    assert_eq!(
        v["value"],
        serde_json::Value::Null,
        "跨房间（带 roomName 且不等）仍为 Infinity → JSON null"
    );

    // 桩调用计数（A2 起 move/harvest 已从桩换真——真方法不再计数，改锁
    // 仪表总体工作：sample bot 的 room.find 仍在桩面）
    let stubs = sb.drain_stub_calls()?;
    assert!(
        stubs["room.find:101"].as_i64().unwrap_or(0) >= 1,
        "桩方法调用计数: {stubs}"
    );
    Ok(())
}

/// runExpr 通道（screeps-ok runPlayer 的基础）：末表达式值、undefined 归一、错误三分类。
fn run_expr_channel() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;

    let v = sb.run_expr("1 + 1")?;
    assert_eq!(v["ok"], true);
    assert_eq!(v["value"], 2);

    let v = sb.run_expr("undefined")?;
    assert_eq!(v["value"], serde_json::Value::Null, "undefined 归一为 null");

    let v = sb.run_expr("(function(){ var x = 1; x++; return x; })()")?;
    assert_eq!(v["value"], 2);

    // 语法错误分类
    let v = sb.run_expr("var a = ;")?;
    assert_eq!(v["ok"], false);
    assert_eq!(v["kind"], "syntax", "语法错误分类: {v}");

    // 运行期错误分类
    let v = sb.run_expr("null.crash")?;
    assert_eq!(v["ok"], false);
    assert_eq!(v["kind"], "runtime");

    // 序列化错误分类（活对象/循环引用不可 JSON）
    let v = sb.run_expr("(function(){ const a = {}; a.self = a; return a; })()")?;
    assert_eq!(v["ok"], false);
    assert_eq!(v["kind"], "serialization");
    Ok(())
}

/// 看门狗硬中断 + 代际守卫 + 实例复用 + 官方跳帧语义。
fn watchdog_hard_terminate_and_reuse() -> anyhow::Result<()> {
    let cfg = SandboxConfig {
        max_per_tick: 100.0,
        initial_bucket: 100.0, // tickLimit = 100
        ..SandboxConfig::default()
    };
    let mut sb = Sandbox::new(cfg)?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { const t = Date.now(); while (true) {} };",
    );
    sb.set_modules(table)?;
    let w = world();

    let t0 = std::time::Instant::now();
    let out = tick(&mut sb, &w)?;
    let elapsed = t0.elapsed().as_millis();
    assert!(matches!(out, TickOutcome::Terminated), "死循环被硬中断");
    assert!(elapsed < 2_000, "硬中断及时生效（{elapsed}ms）");
    // 初始 100 + 回充 20 = 120，扣整份 tickLimit 100 → 剩 20（bucket_sim 同款语义）
    assert!(
        (sb.bucket() - 20.0).abs() < 0.01,
        "按整份 tickLimit 计费: {}",
        sb.bucket()
    );

    // 官方语义（make.js:206-212 + 72-83）：桶充足（扣后 ≥ 0）时硬中断**不**跳帧，
    // 下一 tick 照常执行；只有桶被扣成负数才停跑
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {}
        TickPlan::SkipPenalty => panic!("桶充足时硬中断不应触发跳帧（官方 cpuAvailable 语义）"),
    }
    sb.end_tick()?; // 测试语境下本 tick 未执行玩家代码（结算量级可忽略）

    // 实例复用：换正常代码后继续可跑
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { globalThis.__revived = 1; };",
    );
    sb.set_modules(table)?;
    let out = tick(&mut sb, &world())?;
    assert!(
        matches!(out, TickOutcome::Completed { .. }),
        "runtime 复用成功"
    );
    let v = sb.run_expr("globalThis.__revived")?;
    assert_eq!(v["value"], 1);
    Ok(())
}

/// 真实看门狗线程的代际守卫：deadline 边缘的 disarm/arm 交界处，迟到的旧到期
/// 不得把 terminate 投递到下一个 tick 的执行上（并发-1 的窗口）。
/// 竞态本质非确定，靠高频贴近 deadline 的 arm/disarm 逼近窗口；守卫失效时
/// 本测试会间歇性失败（runExpr 被 terminate 打死），锁原子化后必然通过。
fn watchdog_late_expiry_does_not_kill_next_tick() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = world();
    for _ in 0..300 {
        // arm 一个 1ms 的短 deadline，在到期边缘 disarm，随后立刻模拟下一 tick
        //（cancel → arm → 执行）——迟到的 terminate 若越过交界会打死这次执行
        sb.watchdog()
            .arm(std::time::Instant::now() + std::time::Duration::from_millis(1));
        std::thread::sleep(std::time::Duration::from_micros(800));
        sb.watchdog().disarm();
        sb.begin_tick(&w)?;
        let v = sb.run_expr("1+1")?;
        assert_eq!(v["ok"], true, "迟到的到期不得误杀下一个 tick 的执行: {v:?}");
        sb.end_tick()?;
    }
    Ok(())
}

/// 内存双闸门：接近堆上限优雅终止（进程不崩），实例复用；近堆回调确实先于
/// 看门狗兜底触发（near_heap_fired 诊断 + 墙钟远小于 tickLimit），且善后恢复堆上限
/// （否则 V8 把回调返回的放大上限持久化，反复触发可指数级架空配置上限）。
fn memory_gate_terminates_and_reuses() -> anyhow::Result<()> {
    let cfg = SandboxConfig {
        heap_limit_bytes: 48 * 1024 * 1024,
        // 看门狗兜底线拉远，确保本测试走的是 near-heap 路径而非超时路径
        max_per_tick: 10_000.0,
        initial_bucket: 10_000.0,
        ..SandboxConfig::default()
    };
    let mut sb = Sandbox::new(cfg)?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { const a = []; while (true) a.push(new Array(10000).fill('xxxxxxxxxx')); };",
    );
    sb.set_modules(table)?;

    let t0 = std::time::Instant::now();
    let out = tick(&mut sb, &world())?;
    let elapsed_ms = t0.elapsed().as_millis();
    assert!(
        matches!(out, TickOutcome::Terminated),
        "内存逼近上限被优雅终止（非进程崩溃）"
    );
    assert!(
        sb.near_heap_fired(),
        "近堆回调确实触发（区分内存闸门与看门狗兜底路径）"
    );
    assert!(
        elapsed_ms < 10_000,
        "近堆触发远快于看门狗兜底线（{elapsed_ms}ms < 10000ms）"
    );
    assert!(
        sb.heap_size_limit_bytes() <= 60 * 1024 * 1024,
        "善后已把被回调抬高的堆上限恢复到配置值 48MB 附近（实测 {} 字节；\
         若 restore 失效会停留在 96MB）",
        sb.heap_size_limit_bytes()
    );

    // 复用（善后已 remove+re-add 回调并恢复堆上限；诊断标志在下一 begin_tick 复位）
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { globalThis.__afterOom = 1; };",
    );
    sb.set_modules(table)?;
    sb.begin_tick(&world())?;
    assert!(!sb.near_heap_fired(), "新 tick 开始时近堆诊断标志复位");
    sb.end_tick()?;
    let out = tick(&mut sb, &world())?;
    assert!(
        matches!(out, TickOutcome::Completed { .. }),
        "OOM 后实例复用成功"
    );
    Ok(())
}

/// .wasm 二进制模块语义：require 返回 Buffer → WebAssembly 编译成功 + 内容一致
/// （官方 game.js 的 binary 模块分支；进 CI 的合成夹具，此前只有手动 bot-check 覆盖）。
fn wasm_binary_module() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    // 合法最小 wasm：\0asm 魔数 + version 1（空模块，WebAssembly 接受）
    let mut table = ModuleTable::new();
    table.insert_binary("mod.wasm", [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
    table.insert_js(
        "main",
        "module.exports.loop = function () {\n\
             if (globalThis.__wasmChecked) return;\n\
             const b = require('mod.wasm');\n\
             globalThis.__wasmBytes = (b instanceof Buffer && b.length === 8 && b[0] === 0 && b[1] === 97);\n\
             globalThis.__wasmCompiled = (function () { try { new WebAssembly.Module(b); return true; } catch (e) { return false; } })();\n\
             globalThis.__wasmChecked = true;\n\
         };",
    );
    sb.set_modules(table)?;
    let out = tick(&mut sb, &world())?;
    assert!(matches!(out, TickOutcome::Completed { .. }));
    let v = sb.run_expr("globalThis.__wasmBytes")?;
    assert_eq!(
        v["value"], true,
        "require 二进制模块返回 Buffer（内容一致）"
    );
    let v = sb.run_expr("globalThis.__wasmCompiled")?;
    assert_eq!(v["value"], true, "Buffer 可直接编译为 WebAssembly.Module");
    Ok(())
}

/// LIFO 注册栈：正常逆序销毁后全局栈清空。
fn lifo_registry_drains() -> anyhow::Result<()> {
    let before = live_sandbox_count();
    {
        let _a = Sandbox::new(SandboxConfig::default())?;
        let _b = Sandbox::new(SandboxConfig::default())?;
        // 作用域结束按声明逆序 drop（_b 先于 _a）——恰好是 LIFO
    }
    assert_eq!(live_sandbox_count(), before, "销毁后注册栈回到基线");
    Ok(())
}

/// M2A-A2：intentsList 形态（对象id→动作名→参数）、同名覆盖 last-wins、
/// 覆盖只计一次费、getUsed 含 intents.cpu、软异常照收、硬中断丢弃且整份计费。
/// （并入串行套件——LIFO 纪律，不得作独立 #[test] 与长套件并行）
fn intent_channel_prelude_forms_impl() -> anyhow::Result<()> {
    // M2A-A2：intentsList 形态（对象id→动作名→参数）、同名覆盖 last-wins、
    // 覆盖只计一次费、getUsed 含 intents.cpu、软异常照收、硬中断丢弃且整份计费
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 0 } },
            { "id": "kk2", "type": "structure", "structureType": "source", "room": "W1N1",
              "x": 26, "y": 25, "energy": 2500, "energyCapacity": 3000 }
        ]
    });
    // 计费双轨的沙箱侧：getUsed 差值含虚拟 0.2（官方 runtime.js:202-204）
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                const b = Game.cpu.getUsed();
                Game.creeps['w1'].move(3);
                const d1 = Game.cpu.getUsed() - b;
                Game.creeps['w1'].move(5);
                const d2 = Game.cpu.getUsed() - b;
                return [d1, d2];
            })()"#,
            )?;
            let d1 = v["value"][0].as_f64().expect("d1 数值");
            let d2 = v["value"][1].as_f64().expect("d2 数值");
            assert!(d1 >= 0.19, "首推计费 +0.2（实际 {d1}）");
            assert!(d2 - d1 < 0.19, "同名覆盖只计一次（d1={d1} d2={d2}）");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            assert_eq!(end["intentsCpu"], 0.2);
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(intents["kk1"]["move"]["direction"], 5, "last-wins 覆盖");
            assert!(sb.take_intents().is_none(), "take 后置空");
        }
        _ => panic!("应正常执行"),
    }
    // 软异常：throw 前已 push 的 intent 照收（官方 runtime.js:241-244 catch 后
    // 照常组装 outMessage.intentsList）
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr("Game.creeps['w1'].move(3); (()=>{ throw new Error('x') })()")?;
            assert_eq!(v["ok"], false, "软异常归 runtime");
            let _ = sb.end_tick()?;
            assert!(sb.take_intents().is_some(), "软异常 intents 照收");
        }
        _ => panic!("应正常执行"),
    }
    // 硬中断：本轮 intents 丢弃（官方 isolate 级故障整轮丢，make.js:279-294）
    // 且按整份 tickLimit 计费
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let bucket_before = sb.bucket();
            let _ = sb.run_expr("Game.creeps['w1'].move(3); while (true) {}")?;
            let end = sb.end_tick()?;
            assert_eq!(end["terminated"], true, "硬中断");
            assert!(sb.take_intents().is_none(), "硬中断 intents 丢弃");
            assert!(
                bucket_before - sb.bucket() >= 470.0,
                "整份计费（{} → {}）",
                bucket_before,
                sb.bucket()
            );
        }
        _ => panic!("应正常执行"),
    }
    // 写回软失败（ok:false）→ intents 同步丢弃（三路径丢 intents 的第三环，
    // UNDOC-MEMJSON-005 锁定的官方语义；自审补：此前该路径无 take 断言）
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // Memory.big = 1n 使写回 stringify 抛（BigInt 不可序列化）→ ok:false
            let v = sb.run_expr("Game.creeps['w1'].move(3); Memory.big = 1n; 'set'")?;
            assert_eq!(v["value"], "set");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], false, "写回失败按软错误回传");
            assert!(
                sb.take_intents().is_none(),
                "写回失败 intents 同步丢弃（MEMJSON-005）"
            );
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// 对抗性探针（外部复审 P1-1/P2-5 修复的回归锁）：覆写全局 JSON.stringify
/// 不得影响宿主消息（预置函数）；原型污染不得使计费免费化（null 原型免费表）。
fn hostile_probes_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 0 } }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // P1-1 探针：覆写 stringify 试图伪造 room intent——宿主用预置函数，
            // intentsList 只含真实 move，伪造内容不进通道
            let v = sb.run_expr(r#"(()=>{
                const orig = JSON.stringify;
                JSON.stringify = function(o) {
                    if (o && o.intentsList !== undefined) {
                        return orig({ ok: true, intentsList: { room: { createConstructionSite:
                            [{ roomName: 'W1N1', x: 10, y: 10, structureType: 'road' }] } }, intentsCpu: 0 });
                    }
                    return orig(o);
                };
                Game.creeps['w1'].move(3);
                return 'done';
            })()"#)?;
            assert_eq!(v["value"], "done");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(intents["kk1"]["move"]["direction"], 3, "真实 move 存活");
            assert!(
                intents.get("room").is_none(),
                "伪造的 room intent 不得进入通道（P1-1）: {intents}"
            );
        }
        _ => panic!("应正常执行"),
    }
    // P2-5 探针：原型污染免费表——计费不免费化
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                Object.prototype.move = true;
                Game.creeps['w1'].move(5);
                return 'done';
            })()"#,
            )?;
            assert_eq!(v["value"], "done");
            let end = sb.end_tick()?;
            assert_eq!(end["intentsCpu"], 0.2, "原型污染不得使 move 免费（P2-5）");
        }
        _ => panic!("应正常执行"),
    }
    // 自审 P1 探针：Object.prototype.toJSON 原型污染——预置 stringify 只防"换函数"
    // 不防"换数据"，宿主信封与 intent data 的 null 原型（官方 runtime.js:214
    // outMessage / :56 setPrototypeOf(data) 同款结构位）是免疫关键：继承的
    // toJSON 不得在序列化时改写载荷（曾可一条 defineProperty 完整重放"无 API
    // 调用造工地"原探针）
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                Object.defineProperty(Object.prototype, 'toJSON', { value: function() {
                    if (this && this.ok !== undefined && this.intentsList === undefined) {
                        return { ok: true, intentsList: { room: { createConstructionSite:
                            [{ roomName: 'W1N1', x: 11, y: 11, structureType: 'road' }] } },
                            intentsCpu: 0 };
                    }
                    return 'x';
                }, configurable: true });
                Game.creeps['w1'].move(7);
                return ('toJSON' in {}) ? 'done' : 'probe-broken';
            })()"#,
            )?;
            assert_eq!(v["value"], "done", "污染须存活到编码时刻（探针自检）");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true, "信封形状不得被继承 toJSON 改写（自审 P1）");
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(
                intents["kk1"]["move"]["direction"], 7,
                "真实 move 存活（intent data 剥原型后不被 toJSON 改写）"
            );
            assert!(
                intents.get("room").is_none(),
                "toJSON 污染不得伪造 room intent（自审 P1）: {intents}"
            );
        }
        _ => panic!("应正常执行"),
    }
    // 对抗评审 P1 探针：Object/Array 绑定交换 + toJSON 污染组合——nullEnv/
    // 收集器若经可交换的全局 Object 构造，null-proto 防线整体失效（V8 实证
    // 可完整重放伪造链）；出口净化窗口（sanitizedStringify 摘除根原型 toJSON）
    // 是根本闸——交换后信封仍不可伪造
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                Object.defineProperty(Object.prototype, 'toJSON', { value: function() {
                    if (this && this.ok !== undefined && this.intentsList === undefined) {
                        return { ok: true, intentsList: { room: { createConstructionSite:
                            [{ roomName: 'W1N1', x: 13, y: 13, structureType: 'road' }] } },
                            intentsCpu: 0 };
                    }
                    return 'x';
                }, configurable: true });
                globalThis.Object = { assign: (t, s) => ({ ...s }), create: () => ({}),
                    setPrototypeOf: () => {}, defineProperty: () => {}, entries: (o) => [],
                    keys: () => [], prototype: Object.prototype };
                Game.creeps['w1'].move(8);
                return 'done';
            })()"#,
            )?;
            assert_eq!(v["value"], "done");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true, "Object 交换不得破坏宿主信封（对抗 P1）");
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(intents["kk1"]["move"]["direction"], 8, "真实 move 存活");
            assert!(
                intents.get("room").is_none(),
                "Object 交换 + toJSON 污染不得伪造 room intent（对抗 P1）: {intents}"
            );
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}
