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
    move_misc_family_prelude_forms_impl()?;
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
    store_class_semantics_impl()?;
    spawn_family_api_impl()?;
    resource_family_prelude_matrix_impl()?;
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

/// A4-i Store 类官方全语义（engine/src/game/store.js:19-101 逐条）：
/// 四形态（per-resource 商店 spawn/extension、数值容量 storage/creep）的
/// getCapacity/getUsedCapacity/getFreeCapacity 全分岔 + Proxy 缺省 0 +
/// toString/instanceof/stringify 形态。世界 fixture 手写 world_json 的投影
/// 键（storeCapacityResource/storeCapacity——typed 投影同形）。
fn store_class_semantics_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 5,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "energyAvailable": 200, "energyCapacityAvailable": 250 } },
        "objects": [
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 150 },
              "storeCapacityResource": { "energy": 300 }, "off": false, "spawning": null },
            { "id": "e1", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 21, "y": 20, "owner": "p1", "store": { "energy": 50 },
              "storeCapacityResource": { "energy": 50 }, "off": false },
            { "id": "st1", "type": "structure", "structureType": "storage", "room": "W1N1",
              "x": 22, "y": 20, "owner": "p1", "store": { "energy": 500 }, "storeCapacity": 1000000 },
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 20, "y": 21, "name": "carrier",
              "owner": "p1", "body": ["carry", "carry", "move"], "store": { "energy": 30 },
              "storeCapacity": 100, "ticksToLive": 1000, "spawning": false }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // spawn（per-resource 商店）：store.js 全分岔——getCapacity() null、
            // getCapacity('energy') 300、非 energy 资源 null；getUsedCapacity()
            // null（capacityForResource(o,undefined)===0）、getUsedCapacity('H')
            // null（容量为 0 的资源）、getFreeCapacity() null、
            // getFreeCapacity('energy') = 300−150（无 storeCapacity 分支）
            let v = sb.run_expr(
                r#"(()=>{
                const s = Game.spawns['Spawn1'].store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getCapacity('H'),
                        s.getUsedCapacity(), s.getUsedCapacity('energy'), s.getUsedCapacity('H'),
                        s.getFreeCapacity(), s.getFreeCapacity('energy')];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], serde_json::Value::Null, "spawn getCapacity() → null");
            assert_eq!(v[1], 300, "spawn getCapacity('energy') → 300");
            assert_eq!(
                v[2],
                serde_json::Value::Null,
                "spawn getCapacity('H') → null"
            );
            assert_eq!(
                v[3],
                serde_json::Value::Null,
                "spawn getUsedCapacity() → null"
            );
            assert_eq!(v[4], 150, "spawn getUsedCapacity('energy')");
            assert_eq!(
                v[5],
                serde_json::Value::Null,
                "spawn getUsedCapacity('H') → null（容量 0 资源）"
            );
            assert_eq!(
                v[6],
                serde_json::Value::Null,
                "spawn getFreeCapacity() → null"
            );
            assert_eq!(v[7], 150, "spawn getFreeCapacity('energy') = 300−150");

            // extension（RCL 归一的 per-resource 容量 50）
            let v = sb.run_expr(
                r#"(()=>{
                const s = Game.getObjectById('e1').store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getFreeCapacity('energy')];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(
                v[0],
                serde_json::Value::Null,
                "extension getCapacity() → null"
            );
            assert_eq!(v[1], 50, "extension getCapacity('energy') → 50");
            assert_eq!(v[2], 0, "extension getFreeCapacity('energy') = 50−50");

            // storage（数值容量）：_sum 惰性缓存路径
            let v = sb.run_expr(r#"(()=>{
                const s = Game.getObjectById('st1').store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getUsedCapacity(),
                        s.getUsedCapacity('energy'), s.getFreeCapacity(), s.getFreeCapacity('energy')];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 1000000, "storage getCapacity()");
            assert_eq!(v[1], 1000000, "storage getCapacity('energy')（容量共享）");
            assert_eq!(v[2], 500, "storage getUsedCapacity()（_sum）");
            assert_eq!(v[3], 500, "storage getUsedCapacity('energy')");
            assert_eq!(v[4], 999500, "storage getFreeCapacity() = 1000000−500");
            assert_eq!(v[5], 999500, "storage getFreeCapacity('energy')");

            // creep（carry 派生容量 100）
            let v = sb.run_expr(
                r#"(()=>{
                const c = Game.creeps['carrier'];
                const s = c.store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getUsedCapacity(),
                        s.getFreeCapacity(), c.carryCapacity, c.carry.getCapacity()];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 100, "creep store.getCapacity()");
            assert_eq!(v[1], 100, "creep store.getCapacity('energy')");
            assert_eq!(v[2], 30, "creep store.getUsedCapacity()");
            assert_eq!(v[3], 70, "creep store.getFreeCapacity()");
            assert_eq!(
                v[4], 100,
                "creep carryCapacity（官方 o.storeCapacity 直读）"
            );
            assert_eq!(v[5], 100, "creep carry 同 store 形态");

            // Proxy 语义（store.js:87-96）：RESOURCES_ALL 缺省 0、未知名
            // undefined、toString '[store]'、instanceof、可枚举面只有正量资源
            let v = sb.run_expr(
                r#"(()=>{
                const s = Game.spawns['Spawn1'].store;
                return [s.energy, s.H, s.void_unknown_name === undefined, String(s),
                        s instanceof Store, JSON.stringify(s),
                        JSON.stringify(Object.keys(s))];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 150, "正量资源直读");
            assert_eq!(v[1], 0, "RESOURCES_ALL 资源缺省 0（非 undefined）");
            assert_eq!(v[2], true, "未知属性 undefined");
            assert_eq!(v[3], "[store]", "toString（官方 store.js:80-84）");
            assert_eq!(v[4], true, "instanceof Store（构造器挂 globals）");
            assert_eq!(
                v[5], r#"{"energy":150}"#,
                "stringify 只含正量资源（三函数非 enumerable）"
            );
            assert_eq!(v[6], r#"["energy"]"#, "Object.keys 只有正量资源");
            let _ = sb.end_tick()?;
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A4-i spawn 方法族（structures.js:1063-1334）：spawnCreep 全预检序 + 同 tick
/// 幽灵壳 + Memory 初始化 + intent 载荷；Spawning getter/setDirections/cancel；
/// renewCreep/recycleCreep 预检码与 intent。
fn spawn_family_api_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 5,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "energyAvailable": 200, "energyCapacityAvailable": 250 } },
        "objects": [
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 150 },
              "storeCapacityResource": { "energy": 300 }, "off": false, "spawning": null },
            { "id": "s2", "type": "spawn", "room": "W1N1", "x": 23, "y": 20, "name": "Spawn2",
              "owner": "p1", "store": { "energy": 300 },
              "storeCapacityResource": { "energy": 300 }, "off": false,
              "spawning": { "name": "child", "needTime": 6, "spawnTime": 11, "directions": [3, 5] } },
            { "id": "s3", "type": "spawn", "room": "W1N1", "x": 24, "y": 20, "name": "EnemySpawn",
              "owner": "p2", "store": { "energy": 300 },
              "storeCapacityResource": { "energy": 300 }, "off": false, "spawning": null },
            { "id": "s4", "type": "spawn", "room": "W1N1", "x": 25, "y": 20, "name": "OffSpawn",
              "owner": "p1", "store": { "energy": 300 },
              "storeCapacityResource": { "energy": 300 }, "off": true, "spawning": null },
            { "id": "e1", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 21, "y": 20, "owner": "p1", "store": { "energy": 50 },
              "storeCapacityResource": { "energy": 50 }, "off": false },
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 20, "y": 21, "name": "carrier",
              "owner": "p1", "body": ["carry", "carry", "move"], "store": { "energy": 30 },
              "storeCapacity": 100, "ticksToLive": 1000, "spawning": false },
            { "id": "c2", "type": "creep", "room": "W1N1", "x": 40, "y": 40, "name": "far",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 },
              "storeCapacity": 50, "ticksToLive": 1000, "spawning": false },
            { "id": "c3", "type": "creep", "room": "W1N1", "x": 20, "y": 19, "name": "claimer",
              "owner": "p1", "body": ["claim"], "store": { "energy": 0 },
              "storeCapacity": 0, "ticksToLive": 500, "spawning": false }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // ---- spawnCreep 预检族（错误码逐条对照 :1065-1116）----
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                return [
                    sp.spawnCreep([WORK, WORK, WORK], 'W1'),                       // -6 能量不足（300>200）
                    sp.spawnCreep(['fly'], 'W1'),                                   // -10 非法部件
                    sp.spawnCrerep_typo_guard === undefined ? sp.spawnCreep([MOVE], 'W1', null) : -99, // -10 options 非 object
                    sp.spawnCreep([MOVE], 'W1', { directions: [0] }),               // -10 越界方向
                    sp.spawnCreep([MOVE], 'W1', { directions: [] }),                // -10 空方向
                    sp.spawnCreep([MOVE], 'W1', { directions: 5 }),                 // -10 非数组方向
                    sp.spawnCreep([MOVE], 'x'.repeat(101)),                         // -10 名字超 100
                    sp.spawnCreep([MOVE], 'W1', { dryRun: true }),                  // 0 dryRun（不消费名字）
                    sp.spawnCreep([MOVE], 'W1')                                     // 0 正常（同 tick 前面全拒）
                ];
            })()"#)?;
            // 上面的 typo 守卫恒 false → 走 spawnCreep([MOVE], 'W1', null) 分支
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], -6, "ERR_NOT_ENOUGH_ENERGY");
            assert_eq!(v[1], -10, "非法部件 ERR_INVALID_ARGS");
            assert_eq!(
                v[2], -10,
                "options=null ERR_INVALID_ARGS（默认参只兜 undefined）"
            );
            assert_eq!(v[3], -10, "directions 越界");
            assert_eq!(v[4], -10, "directions 空");
            assert_eq!(v[5], -10, "directions 非数组");
            assert_eq!(v[6], -10, "名字 > 100 字符");
            assert_eq!(v[7], 0, "dryRun 返回 OK");
            assert_eq!(v[8], 0, "dryRun 后同名可用（名单未消费）");

            // 同 tick 幽灵壳（官方 :1132-1213 十三字段逐项）+ Memory 初始化
            let v = sb.run_expr(
                r#"(()=>{
                const g = Game.creeps['W1'];
                const m = Memory.creeps['W1'];
                return [g instanceof Creep, g.spawning, g.my, g.name, g.ticksToLive,
                        g.body.length, g.body[0].type, g.body[0].hits, g.carryCapacity,
                        g.carry.energy, g.store.energy, g.store.getCapacity(),
                        g.hits, g.hitsMax, g.saying === undefined, g.id === undefined,
                        g.pos.x, g.pos.y, g.pos.roomName, g.room === Game.rooms['W1N1'],
                        JSON.stringify(m)];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], true, "instanceof Creep");
            assert_eq!(v[1], true, "spawning: true");
            assert_eq!(v[2], true, "my: true");
            assert_eq!(v[3], "W1");
            assert_eq!(v[4], 1500, "ticksToLive = CREEP_LIFE_TIME");
            assert_eq!(v[5], 1);
            assert_eq!(v[6], "move", "body 映射 {{type, hits:100}}");
            assert_eq!(v[7], 100);
            assert_eq!(v[8], 0, "carryCapacity：无 CARRY");
            assert_eq!(v[9], 0, "carry.energy");
            assert_eq!(v[10], 0, "store.energy（Proxy 缺省 0）");
            assert_eq!(v[11], serde_json::Value::Null, "幽灵壳 store 无容量 → null");
            assert_eq!(v[12], 100, "hits = body.length×100");
            assert_eq!(v[13], 100, "hitsMax");
            assert_eq!(v[14], true, "saying undefined");
            assert_eq!(v[15], true, "官方幽灵壳无 id");
            assert_eq!(
                v[16..19],
                [
                    serde_json::json!(20),
                    serde_json::json!(20),
                    serde_json::json!("W1N1")
                ]
            );
            assert_eq!(v[19], true, "room 是当前 Game.rooms 的实例");
            assert_eq!(v[20], "{}", "Memory.creeps['W1'] 初始化为空对象");

            // 同 tick 同名拒（createdCreepNames + Game.creeps 双通道）
            let v = sb.run_expr(r#"Game.spawns['Spawn1'].spawnCreep([MOVE], 'W1')"#)?;
            assert_eq!(v["value"], -3, "ERR_NAME_EXISTS");

            // options.memory 落 Memory.creeps
            let v = sb.run_expr(
                r#"Game.spawns['Spawn1'].spawnCreep([MOVE], 'Mem1', { memory: { role: 'harvester' } })"#,
            )?;
            assert_eq!(v["value"], 0);
            let v = sb.run_expr(r#"JSON.stringify(Memory.creeps['Mem1'])"#)?;
            assert_eq!(v["value"], r#"{"role":"harvester"}"#);

            // energyStructures 显式模式（:1053-1061 无 owner 过滤 + 空列表零能量）
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                const ext = Game.getObjectById('e1');
                return [
                    sp.spawnCreep([MOVE], 'W3', { energyStructures: [ext] }),   // 0（ext 50 ≥ 50）
                    sp.spawnCreep([MOVE], 'W4', { energyStructures: [] }),      // -6（显式空列表 = 零可用）
                    sp.spawnCreep([MOVE], 'W5', { energyStructures: [Game.creeps['carrier']] }) // -6（非 spawn/extension 计 0）
                ];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 0, "显式列表按列表计能量");
            assert_eq!(v[1], -6, "空列表是显式零能量模式（非缺省全房）");
            assert_eq!(v[2], -6, "非 spawn/extension 的列表元素计 0");

            // 敌方 spawn：NOT_OWNER（预检 ⑥ 在 energy 之前）
            let v = sb.run_expr(r#"Game.getObjectById('s3').spawnCreep([MOVE], 'Enemy1')"#)?;
            assert_eq!(v["value"], -1, "ERR_NOT_OWNER");
            // off 的 spawn：ERR_RCL_NOT_ENOUGH（⑧ 显式回查）
            let v = sb.run_expr(r#"Game.spawns['OffSpawn'].spawnCreep([MOVE], 'Off1')"#)?;
            assert_eq!(v["value"], -14, "ERR_RCL_NOT_ENOUGH（off）");
            // 孵化中的 spawn：ERR_BUSY（⑦ 显式回查）
            let v = sb.run_expr(r#"Game.spawns['Spawn2'].spawnCreep([MOVE], 'Busy1')"#)?;
            assert_eq!(v["value"], -4, "ERR_BUSY（spawning）");

            // ---- Spawning（structures.js:1301-1334）----
            let v = sb.run_expr(
                r#"(()=>{
                const sp = Game.spawns['Spawn2'].spawning;
                return [sp instanceof StructureSpawn.Spawning, sp.name, sp.needTime,
                        sp.remainingTime, sp.directions[0], sp.directions[1],
                        sp.spawn === Game.spawns['Spawn2'],
                        JSON.stringify(sp)];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], true, "instanceof StructureSpawn.Spawning");
            assert_eq!(v[1], "child");
            assert_eq!(v[2], 6, "needTime");
            assert_eq!(v[3], 6, "remainingTime = spawnTime(11) − time(5)");
            assert_eq!(v[4], 3);
            assert_eq!(v[5], 5);
            assert_eq!(v[6], true, "spawn 引用是当前壳（非 enumerable 定格）");
            assert_eq!(
                v[7],
                r#"{"name":"child","needTime":6,"remainingTime":6,"directions":[3,5]}"#
            );
            let v = sb.run_expr(r#"Game.spawns['Spawn1'].spawning === null"#)?;
            assert_eq!(v["value"], true, "无任务读 null");

            // setDirections / cancel（含数字化去重与越界拒）
            let v = sb.run_expr(
                r#"(()=>{
                const sp = Game.spawns['Spawn2'].spawning;
                return [sp.setDirections([2, '2', 3]), sp.setDirections([0]), sp.setDirections([]),
                        sp.setDirections(5), sp.cancel(),
                        Game.getObjectById('s3').spawning === null];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 0, "合法组（数字化去重 [2,3]）");
            assert_eq!(v[1], -10, "越界拒");
            assert_eq!(v[2], -10, "空拒");
            assert_eq!(v[3], -10, "非数组拒");
            assert_eq!(v[4], 0, "cancel OK");
            assert_eq!(v[5], true);

            // ---- renewCreep 预检码（:1237-1267）----
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                const near = Game.creeps['carrier'];
                return [
                    Game.spawns['Spawn2'].renewCreep(near),   // -4 busy（spawning 最先）
                    sp.renewCreep(near),                       // 0（cost=ceil(1.2·150/3/3)=20 ≤ 200；1000+200 ≤ 1500）
                    sp.renewCreep(Game.creeps['claimer']),    // -7 CLAIM 部件
                    sp.renewCreep(Game.creeps['far']),        // -9 非邻接
                    sp.renewCreep(null),                      // -7 空 target
                    Game.getObjectById('s3').renewCreep(near),// -1 敌方 spawn
                    Game.spawns['OffSpawn'].renewCreep(near)  // -14 off
                ];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], -4, "ERR_BUSY");
            assert_eq!(v[1], 0, "正常续命（intent 记录）");
            assert_eq!(v[2], -7, "CLAIM 部件 ERR_INVALID_TARGET");
            assert_eq!(v[3], -9, "ERR_NOT_IN_RANGE");
            assert_eq!(v[4], -7, "空 target ERR_INVALID_TARGET");
            assert_eq!(v[5], -1, "敌方 spawn ERR_NOT_OWNER");
            assert_eq!(v[6], -14, "off spawn ERR_RCL_NOT_ENOUGH");

            // ---- recycleCreep 预检码（:1269-1290——my 最先、无 CLAIM 条件）----
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                const near = Game.creeps['claimer'];
                return [
                    Game.getObjectById('s3').recycleCreep(near), // -1 my 最先
                    sp.recycleCreep(Game.creeps['far']),          // -9 非邻接
                    sp.recycleCreep(near),                        // 0（CLAIM 可回收——与 renew 的差异面）
                    sp.recycleCreep(null)                         // -7
                ];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], -1, "ERR_NOT_OWNER 最先");
            assert_eq!(v[1], -9, "ERR_NOT_IN_RANGE");
            assert_eq!(v[2], 0, "CLAIM creep 可回收");
            assert_eq!(v[3], -7, "ERR_INVALID_TARGET");

            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            // intent 载荷形态：同名覆盖 last-wins（W1 系全拒后只有最后成功的
            // createCreep 留存）+ renewCreep/recycleCreep/setSpawnDirections/
            // cancelSpawning 白名单字段
            let intents = sb.take_intents().expect("intentsList 带出");
            let create = &intents["s1"]["createCreep"];
            assert_eq!(
                create["name"], "W3",
                "last-wins：W1→Mem1→W3 逐次覆盖（错误分支不记录）"
            );
            assert_eq!(create["body"], serde_json::json!(["move"]));
            assert_eq!(
                create["energyStructures"],
                serde_json::json!(["e1"]),
                "显式列表的 uniq pluck 形态"
            );
            assert!(
                create.get("directions").is_none(),
                "未传 directions 缺键（undefined 不入 stringify）"
            );
            assert_eq!(intents["s1"]["renewCreep"]["id"], "c1");
            assert_eq!(intents["s1"]["recycleCreep"]["id"], "c3");
            assert_eq!(
                intents["s2"]["setSpawnDirections"]["directions"],
                serde_json::json!([2, 3])
            );
            assert_eq!(intents["s2"]["cancelSpawning"], serde_json::json!({}));
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A4-ii 资源族四方法（transfer/withdraw/drop/pickup）prelude 换真的预检返回码
/// 矩阵——官方 creeps.js:404-593 的预检序逐条锁值（预检顺序即断言顺序）。
/// 世界：rcl3 房（extension 容量 50）；w1 两 carry（容量 100 存 80）为 actor。
fn resource_family_prelude_matrix_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" }, "W2N1": { "name": "W2N1" } },
        "objects": [
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["carry", "carry"], "store": { "energy": 80 } },
            { "id": "kk2", "type": "creep", "room": "W1N1", "x": 25, "y": 26, "name": "w2",
              "owner": "p1", "body": ["carry"], "store": { "energy": 10 } },
            { "id": "kk3", "type": "creep", "room": "W1N1", "x": 26, "y": 26, "name": "w3",
              "owner": "p2", "body": ["carry"], "store": { "energy": 5 } },
            { "id": "kk4", "type": "creep", "room": "W1N1", "x": 27, "y": 27, "name": "w4",
              "owner": "p1", "body": ["carry"], "store": { "energy": 50 }, "spawning": true },
            { "id": "kk5", "type": "creep", "room": "W1N1", "x": 28, "y": 28, "name": "w5",
              "owner": "p1", "body": ["carry"], "store": { "energy": 0 } },
            { "id": "kk6", "type": "creep", "room": "W1N1", "x": 24, "y": 24, "name": "w6",
              "owner": "p1", "body": ["carry", "carry"], "store": { "energy": 100 } },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 24, "y": 25, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 300 }, "energyCapacity": 300 },
            { "id": "e1", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 26, "y": 24, "owner": "p1", "store": { "energy": 0 } },
            { "id": "e2", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 24, "y": 26, "owner": "p1", "store": { "energy": 50 } },
            { "id": "k1", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "owner": "p1" },
            { "id": "src1", "type": "structure", "structureType": "source", "room": "W1N1",
              "x": 40, "y": 40, "energy": 2500 },
            { "id": "st1", "type": "structure", "structureType": "storage", "room": "W1N1",
              "x": 45, "y": 45, "owner": "p1", "store": { "energy": 500 } },
            { "id": "r1", "type": "resource", "room": "W1N1", "x": 25, "y": 24,
              "resourceType": "energy", "amount": 30 },
            { "id": "r2", "type": "resource", "room": "W1N1", "x": 45, "y": 44,
              "resourceType": "energy", "amount": 30 },
            // A4-ii 评审 3 P1 补测面：nuker/powerBank/terminal（W1N1，kk2 邻接
            // (25,27)/(26,27)/(24,27)；powerBank 手写世界造 store 使 :532 分支
            // 可达——官方 powerBank 无 store 在目标门即拒）与 p2 safeMode 房
            { "id": "nk1", "type": "structure", "structureType": "nuker", "room": "W1N1",
              "x": 25, "y": 27, "owner": "p2", "store": { "energy": 100 } },
            { "id": "pb1", "type": "structure", "structureType": "powerBank", "room": "W1N1",
              "x": 26, "y": 27, "owner": "p2", "store": { "power": 100 } },
            { "id": "tm1", "type": "structure", "structureType": "terminal", "room": "W1N1",
              "x": 24, "y": 27, "owner": "p2", "store": { "energy": 500 },
              "effects": [{ "power": 15, "ticksRemaining": 5 }] },
            { "id": "tm2", "type": "structure", "structureType": "terminal", "room": "W1N1",
              "x": 26, "y": 25, "owner": "p1", "store": { "energy": 500 } },
            { "id": "w7", "type": "creep", "room": "W2N1", "x": 25, "y": 25, "name": "w7",
              "owner": "p1", "body": ["carry"], "store": { "energy": 0 } },
            { "id": "k2", "type": "structure", "structureType": "controller", "room": "W2N1",
              "x": 10, "y": 10, "level": 3, "owner": "p2", "safeMode": 100 },
            { "id": "sp2", "type": "spawn", "room": "W2N1", "x": 26, "y": 25, "name": "Spawn2",
              "owner": "p2", "store": { "energy": 300 }, "energyCapacity": 300 }
        ]
    });
    // 预检返回码断言宏（直持 sb——闭包形式与穿插的裸 run_expr 抢借用）
    macro_rules! ck {
        ($expr:expr, $want:expr) => {{
            let v = sb.run_expr($expr)?;
            assert_eq!(v["ok"], true, "矩阵求值须成功: {} → {:?}", $expr, v);
            assert_eq!(v["value"].as_i64(), Some($want), "预检返回码: {}", $expr);
        }};
    }

    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // ---- drop（官方 creeps.js:404-427：my/spawning → 白名单 → 保有 → 缺省）----
            ck!("Game.creeps['w1'].drop(RESOURCE_ENERGY, 30)", 0);
            // 官方无 amount<0 预检：负数不落 INVALID_ARGS（全段无 amount<0 检查，
            // store 80 ≥ -5 放行——执行器 amount>0 才落地，官方同形 no-op）
            ck!("Game.creeps['w1'].drop(RESOURCE_ENERGY, -5)", 0);
            ck!("Game.creeps['w1'].drop(RESOURCE_ENERGY, 999)", -6);
            ck!("Game.creeps['w1'].drop('notaresource')", -10);
            ck!("Game.creeps['w5'].drop(RESOURCE_ENERGY)", -6);
            ck!("Game.getObjectById('kk3').drop(RESOURCE_ENERGY)", -1);
            ck!("Game.creeps['w4'].drop(RESOURCE_ENERGY)", -4);

            // ---- transfer（官方 :428-492：amount<0 → 白名单 → 目标族 → 容量 →
            // 邻接 → 保有 → 缺省钳制/显式超容拒）----
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('e1'), RESOURCE_ENERGY)",
                0
            );
            // e2 已满（rcl3 档 50——容量折算的投影锁值）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('e2'), RESOURCE_ENERGY)",
                -8
            );
            // spawn 满仓 300/300
            ck!(
                "Game.creeps['w1'].transfer(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -8
            );
            // creep 目标：显式超容拒（60+10 > 50），部分量放行（20+10 ≤ 50）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), RESOURCE_ENERGY, 60)",
                -8
            );
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), RESOURCE_ENERGY, 20)",
                0
            );
            // 邻接后置：远 storage 过容量门后在 isNearTo 拒
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('st1'), RESOURCE_ENERGY)",
                -9
            );
            // source 不在 register.structures（game.js:224-247 键集无 source）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('src1'), RESOURCE_ENERGY)",
                -7
            );
            // spawning 目标拒（:445）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk4'), RESOURCE_ENERGY)",
                -7
            );
            // 非能量资源进 perResource 结构：容量 0 → INVALID_TARGET（:459）
            ck!(
                "Game.creeps['w1'].transfer(Game.spawns['Spawn1'], RESOURCE_POWER)",
                -7
            );
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('e1'), RESOURCE_POWER)",
                -7
            );
            // 参数门在目标门之前（:435/:438）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), RESOURCE_ENERGY, -5)",
                -10
            );
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), 'bogus')",
                -10
            );
            ck!("Game.creeps['w1'].transfer(null, RESOURCE_ENERGY)", -7);
            ck!(
                "Game.creeps['w1'].transfer({id: 'short'}, RESOURCE_ENERGY)",
                -7
            );
            // controller+能量重定向 upgradeController（:449-451；A4-iii 换真前经
            // 桩返回 OK——本断言锁重定向形态而非 upgrade 语义）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('k1'), RESOURCE_ENERGY)",
                0
            );
            // 非能量进 controller：容量 0 → INVALID_TARGET
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('k1'), RESOURCE_POWER)",
                -7
            );
            // 序列化对象拒绝（game.js:122-126 assertTargetObject 抛错点）
            let v = sb.run_expr(
                "Game.creeps['w1'].transfer({id: '123456789012345678901234'}, RESOURCE_ENERGY)",
            )?;
            assert_eq!(v["ok"], false, "plain object + 24 位 id 须抛序列化拒绝");
            assert!(
                v["error"]
                    .as_str()
                    .unwrap_or("")
                    .contains("serialized game object"),
                "官方文案: {v:?}"
            );

            // ---- withdraw（官方 :493-565：参数门 → store 面/目标族 → terminal →
            // rampart（桩面恒放行）→ safeMode → nuker/powerBank → 容量/保有 →
            // 邻接 → 空容量 → 缺省/超容拒）----
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY, 10)",
                0
            );
            // 超过自身空容量（100-80=20）
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY, 30)",
                -8
            );
            // 满仓 creep
            ck!(
                "Game.creeps['w6'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -8
            );
            // 空目标结构：过容量门后缺省 amount=0 → NOT_ENOUGH_RESOURCES
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('e1'), RESOURCE_ENERGY)",
                -6
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('st1'), RESOURCE_ENERGY)",
                -9
            );
            // source/creep 目标族拒（:507-516）
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('src1'), RESOURCE_ENERGY)",
                -7
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('kk2'), RESOURCE_ENERGY)",
                -7
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY, -1)",
                -10
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], 'bogus')",
                -10
            );
            ck!(
                "Game.getObjectById('kk3').withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -1
            );
            ck!(
                "Game.creeps['w4'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -4
            );
            // 未知 id 在 store 面（data() 求值点）抛官方同文案——早于 assertTargetObject
            let v = sb.run_expr(
                "Game.creeps['w1'].withdraw({id: 'aaaaaaaaaaaaaaaaaaaaaaaa'}, RESOURCE_ENERGY)",
            )?;
            assert_eq!(v["ok"], false, "未知 id 须抛 Could not find");
            assert!(
                v["error"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Could not find an object with ID"),
                "官方文案: {v:?}"
            );

            // ---- withdraw 补测（A4-ii 评审 3 P1：已实现零覆盖分支）----
            // safeMode 门（:528-530）：W2N1 是 p2 的 safeMode 生效房（k2
            // safeMode=100 绝对挂钟 > time=1），p1 creep 提取该房任何 store 即拒
            ck!(
                "Game.getObjectById('w7').withdraw(Game.getObjectById('sp2'), RESOURCE_ENERGY)",
                -1
            );
            // nuker/powerBank 不可提取（:532-534）——powerBank 经手写 store 造
            // 到该分支（官方无 store 在目标门即拒）
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('nk1'), RESOURCE_ENERGY)",
                -7
            );
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('pb1'), RESOURCE_ENERGY)",
                -7
            );
            // terminal 被 PWR_DISRUPT_TERMINAL(15) 封锁（:518-521）与无 effects 放行
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('tm1'), RESOURCE_ENERGY)",
                -7
            );
            // actor 用 w2（w1 的 withdraw intent 已被矩阵主链 last-wins 锁 s1——
            // 不覆盖；w2 之前只有被拒的 nk1 调用无 intent）
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('tm2'), RESOURCE_ENERGY, 5)",
                0
            );
            // transfer 的 owner/busy 门（评审 3：同形门在 drop/withdraw 有覆盖
            // 而 transfer 缺席）
            ck!(
                "Game.getObjectById('kk3').transfer(Game.creeps['w1'], RESOURCE_ENERGY, 1)",
                -1
            );
            ck!(
                "Game.creeps['w4'].transfer(Game.creeps['w1'], RESOURCE_ENERGY, 1)",
                -4
            );

            // ---- pickup（官方 :566-593：my/spawning → Resource 目标 → 满仓 →
            // isNearTo ≤1 格）----
            ck!("Game.creeps['w1'].pickup(Game.getObjectById('r1'))", 0);
            ck!("Game.creeps['w1'].pickup(Game.getObjectById('r2'))", -9);
            ck!("Game.creeps['w6'].pickup(Game.getObjectById('r1'))", -8);
            ck!("Game.creeps['w1'].pickup(Game.spawns['Spawn1'])", -7);
            ck!(
                "Game.getObjectById('kk3').pickup(Game.getObjectById('r1'))",
                -1
            );
            ck!("Game.creeps['w4'].pickup(Game.getObjectById('r1'))", -4);
            let v = sb.run_expr("Game.creeps['w1'].pickup({id: '123456789012345678901234'})")?;
            assert_eq!(v["ok"], false, "pickup 序列化对象同抛");
            assert!(
                v["error"]
                    .as_str()
                    .unwrap_or("")
                    .contains("serialized game object"),
                "官方文案: {v:?}"
            );

            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            // 矩阵 tick 的 last-wins 锁值：drop 负 amount 官方无门原样进 intent
            //（执行器 amount>0 才落地——no-op）；transfer 到 kk2 20
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(
                intents["kk1"]["drop"]["amount"], -5,
                "负 amount 原样记录（官方无门）"
            );
            assert_eq!(intents["kk1"]["drop"]["resourceType"], "energy");
            assert_eq!(intents["kk1"]["transfer"]["id"], "kk2");
            assert_eq!(intents["kk1"]["transfer"]["amount"], 20, "显式部分量 20");
            // 矩阵内唯一放行的 withdraw（10）last-wins 存留；被拒的调用不得记 intent
            assert_eq!(intents["kk1"]["withdraw"]["id"], "s1");
            assert_eq!(intents["kk1"]["withdraw"]["amount"], 10);
            assert_eq!(
                intents["kk1"]["pickup"]["id"], "r1",
                "矩阵内唯一放行的 pickup"
            );
        }
        _ => panic!("应正常执行"),
    }

    // ---- 第二 tick：四方法缺省量/钳制的 intent 载荷锁值 ----
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                const w1 = Game.creeps['w1'];
                return [
                    w1.drop(RESOURCE_ENERGY, 30),
                    w1.transfer(Game.getObjectById('e1'), RESOURCE_ENERGY),
                    w1.withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY),
                    w1.pickup(Game.getObjectById('r1')),
                ].join(',');
            })()"#,
            )?;
            assert_eq!(v["value"], "0,0,0,0", "四方法缺省形态全部放行");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            let intents = sb.take_intents().expect("intentsList 带出");
            // drop 显式 30
            assert_eq!(intents["kk1"]["drop"]["amount"], 30);
            // transfer 缺省 = min(保有 80, 目标容量 50 − 已存 0) = 50——
            // extension 容量按房 rcl3 折算（官方 extensions/tick.js:10-17 的
            // storeCapacityResource.energy 投影等价锁值）
            assert_eq!(intents["kk1"]["transfer"]["id"], "e1");
            assert_eq!(
                intents["kk1"]["transfer"]["amount"], 50,
                "缺省钳到 rcl3 extension 容量"
            );
            assert_eq!(intents["kk1"]["transfer"]["resourceType"], "energy");
            // withdraw 缺省 = min(空容量 20, spawn 存量 300) = 20——
            // actor 容量按 body CARRY×50 折算（_recalc-body.js:23 投影等价锁值）
            assert_eq!(intents["kk1"]["withdraw"]["id"], "s1");
            assert_eq!(
                intents["kk1"]["withdraw"]["amount"], 20,
                "缺省钳到自身空容量"
            );
            assert_eq!(intents["kk1"]["withdraw"]["resourceType"], "energy");
            // pickup 载荷只有 id（官方 :586）
            assert_eq!(intents["kk1"]["pickup"]["id"], "r1");
            assert_eq!(
                serde_json::to_string(&intents["kk1"]["pickup"])?,
                r#"{"id":"r1"}"#,
                "pickup 载荷仅 id（无 amount/resourceType）"
            );
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A4-iv 移动杂项族 prelude 预检矩阵（moveTo 直线桩/moveByPath/say/suicide/
/// pull/notifyWhenAttacked）：逐分支对照官方 creeps.js:158-257/305-327/826-838/
/// 813-825/1101-1126 与 structures.js:89-101/1220-1233 的返回码与 intent 形态。
fn move_misc_family_prelude_forms_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            // 主体：p1 的 move+work @ (25,25)
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"] },
            // 敌方 creep 邻接 @ (26,25)（pull 可拉敌 creep——官方无目标归属门）
            { "id": "kk2", "type": "creep", "room": "W1N1", "x": 26, "y": 25, "name": "e1",
              "owner": "p2", "body": ["move"] },
            // 远处敌方 creep @ (40,40)（NOT_IN_RANGE 分支）
            { "id": "kk3", "type": "creep", "room": "W1N1", "x": 40, "y": 40, "name": "e2",
              "owner": "p2", "body": ["move"] },
            // 孵化中自家 creep（BUSY 分支）
            { "id": "kk4", "type": "creep", "room": "W1N1", "x": 25, "y": 26, "name": "b1",
              "owner": "p1", "body": ["move"], "spawning": true },
            // 疲劳 creep（moveTo 的 TIRED 门——官方 fatigue 显式回查）
            { "id": "kk5", "type": "creep", "room": "W1N1", "x": 25, "y": 24, "name": "t1",
              "owner": "p1", "body": ["move", "work"], "fatigue": 2 },
            // 无 MOVE creep（NO_BODYPART 门）
            { "id": "kk6", "type": "creep", "room": "W1N1", "x": 25, "y": 23, "name": "nm1",
              "owner": "p1", "body": ["work", "carry"] },
            // 自家 spawn（notifyWhenAttacked 覆盖版 + 已存 true 的差分门）
            { "id": "kk7", "type": "spawn", "structureType": "spawn", "room": "W1N1",
              "x": 24, "y": 25, "name": "S1", "owner": "p1", "store": { "energy": 300 },
              "notifyWhenAttacked": true },
            // 中性路（基类版：my===undefined 放行——房间归 p1 controller）
            { "id": "kk8", "type": "structure", "structureType": "road", "room": "W1N1",
              "x": 25, "y": 27 },
            // 敌方 tower（基类版 my===false → NOT_OWNER）
            { "id": "kk9", "type": "structure", "structureType": "tower", "room": "W1N1",
              "x": 23, "y": 25, "owner": "p2", "hits": 1 },
            // controller 归 p1（基类版的房归属分支基准）
            { "id": "kk10", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "owner": "p1" }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(r#"(()=>{
                const g = (i) => Game.getObjectById(i);
                const RP = RoomPosition;
                return {
                  say: [g('kk1').say('hi', true), g('kk2').say('x'), g('kk4').say('x'), g('kk5').say(42)],
                  suicide: [g('kk1').suicide(), g('kk4').suicide()],
                  pull: [g('kk1').pull(g('kk2')), g('kk1').pull(g('kk1')), g('kk1').pull(g('kk7')),
                         g('kk1').pull(g('kk3')), g('kk4').pull(g('kk1')), g('kk1').pull(null)],
                  moveTo: [g('kk2').moveTo(30, 25),
                           g('kk1').moveTo(new RP(25, 25, 'W1N1')), g('kk1').moveTo(25, 25),
                           g('kk1').moveTo('abc'), g('kk1').moveTo({x: 30}),
                           g('kk1').moveTo(30, 25), g('kk1').moveTo(g('kk2')),
                           g('kk5').moveTo(30, 25), g('kk6').moveTo(30, 25),
                           g('kk1').moveTo(30, 25, {noPathFinding: true}) ],
                  moveByPath: [g('kk1').moveByPath([new RP(25, 25, 'W1N1'), new RP(26, 25, 'W1N1')]),
                               g('kk1').moveByPath([new RP(40, 40, 'W1N1')]),
                               g('kk1').moveByPath('26253'),
                               g('kk1').moveByPath(5),
                               g('kk1').moveByPath([]),
                               (()=>{ try { g('kk1').moveByPath('abc'); return 'no-throw'; } catch (e) { return 'threw'; } })()],
                  nwa: [g('kk7').notifyWhenAttacked(true), g('kk7').notifyWhenAttacked(false),
                        g('kk7').notifyWhenAttacked(42), g('kk8').notifyWhenAttacked(true),
                        g('kk9').notifyWhenAttacked(true)],
                };
            })()"#)?;
            let r = &v["value"];
            let expect = |arr: serde_json::Value, want: &[i64], label: &str| {
                let got: Vec<i64> = arr
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_i64().unwrap())
                    .collect();
                assert_eq!(&got, want, "{label}");
            };
            // say：OK / NOT_OWNER / BUSY(spawning) / OK（疲劳 creep 可 say——
            // 官方无疲劳门；数字字符串化在 intent 面）
            expect(
                r["say"].clone(),
                &[0, -1, -4, 0],
                "say 预检序（creeps.js:826-838）",
            );
            expect(
                r["suicide"].clone(),
                &[0, -4],
                "suicide（creeps.js:813-825）",
            );
            // pull：邻接敌方 OK / 自身 INVALID_TARGET / spawn 非 creep INVALID_TARGET /
            // 远处 NOT_IN_RANGE / spawning BUSY / null INVALID_TARGET
            expect(
                r["pull"].clone(),
                &[0, -7, -7, -9, -4, -7],
                "pull（creeps.js:1101-1126）",
            );
            // moveTo：敌方 NOT_OWNER / 同格 RoomPosition OK / 同格数值 OK /
            // 非对象目标 INVALID_TARGET / 无 pos 对象 INVALID_TARGET / 数值 OK /
            // 对象形态（creep 目标解包 .pos）OK / 疲劳 TIRED（显式回查）/
            // 无 MOVE NO_BODYPART / noPathFinding 且无缓存 → NOT_FOUND
            expect(
                r["moveTo"].clone(),
                &[-1, 0, 0, -7, -7, 0, 0, -11, -12, -5],
                "moveTo 预检序（creeps.js:158-257 直线桩形态）",
            );
            // moveByPath：RoomPosition 数组（路径首项即当前位 → 下一步方向 3）OK /
            // 不在路径且不邻首项 NOT_FOUND / 序列化串（首落点 (26,25) 的起步格
            // (25,25) 命中）OK / 数字 INVALID_ARGS / 空数组 NOT_FOUND /
            // 非法串抛官方同文案错误（第 6 项是字符串哨兵，单独断言）
            let mbp: Vec<i64> = r["moveByPath"].as_array().unwrap()[..5]
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect();
            assert_eq!(
                &mbp,
                &[0, -5, 0, -10, -5],
                "moveByPath（creeps.js:305-327）"
            );
            assert_eq!(
                r["moveByPath"][5], "threw",
                "非法序列化串抛错（utils.js:584）"
            );
            // notifyWhenAttacked：spawn 覆盖版已存 true → 同值不记（OK）/
            // false 记 intent / 非布尔 INVALID_ARGS / 中性路（本房归 p1）OK /
            // 敌方 tower NOT_OWNER
            expect(
                r["nwa"].clone(),
                &[0, 0, -10, 0, -1],
                "notifyWhenAttacked 两版预检",
            );
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            // 免费表：say/pull 免费（runtime.js:61）——计费面只有 move/suicide/
            // notifyWhenAttacked(false)/中性路共 4×0.2
            let cpu = end["intentsCpu"].as_f64().unwrap();
            assert!(
                (cpu - 0.8).abs() < 1e-9,
                "免费表生效（say/pull 免费）：{cpu}"
            );
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(intents["kk1"]["say"]["message"], "hi");
            assert_eq!(intents["kk1"]["say"]["isPublic"], true);
            assert_eq!(
                intents["kk5"]["say"]["message"], "42",
                "数字字符串化（空串拼接 message）"
            );
            assert_eq!(intents["kk1"]["suicide"], json!({}), "空载荷");
            assert_eq!(intents["kk1"]["pull"]["id"], "kk2");
            assert_eq!(
                intents["kk1"]["move"]["direction"], 3,
                "直线方向（getDirectionTo 阈值算法）"
            );
            // 同值差分门：spawn 已存 true，notifyWhenAttacked(true) 不记
            assert!(intents["kk7"].get("notifyWhenAttacked").is_some());
            assert_eq!(
                intents["kk7"]["notifyWhenAttacked"],
                json!({"enabled": false}),
                "只有 false 调用记录（true 与存值相同被差分门拦）"
            );
            assert_eq!(
                intents["kk8"]["notifyWhenAttacked"],
                json!({"enabled": true}),
                "中性路首调恒记（undefined != true）"
            );
            assert!(intents.get("kk9").is_none(), "敌方结构无 intent");
            // 桩清偿面：本族方法不再进 stub 计数
            let stubs = sb.drain_stub_calls()?;
            for name in [
                "creep.moveTo",
                "creep.moveByPath",
                "creep.say",
                "creep.suicide",
                "creep.pull",
                "spawn.notifyWhenAttacked",
                "structure.notifyWhenAttacked",
            ] {
                assert!(stubs.get(name).is_none(), "{name} 已换真，不应再有桩计数");
            }
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}
