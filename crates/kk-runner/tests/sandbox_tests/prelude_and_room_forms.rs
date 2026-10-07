//! 验证入口返回形态、Memory 属性、RoomPosition 和房间查询。
//!
//! 场景由 sandbox_tests.rs 的唯一测试串行调用，避免进程级计数互相干扰；
//! 这里不添加独立 #[test]。

use super::*;

/// 写回前交界竞态的确定性复现：run 成功后、写回前，terminate 已投递（看门狗
/// 同代只投一次，不会再 terminate）——end_tick 的 cancel 清掉这个唯一信号后，
/// 挂死的写回必须由重新武装的新代兜底掐断（否则线程永久挂死）。
pub(super) fn writeback_rearmed_after_cancel() -> anyhow::Result<()> {
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

/// RoomPosition 官方语义抽查：isEqualTo 房间判定、getDirectionTo 阈值算法。
pub(super) fn roomposition_official_semantics() -> anyhow::Result<()> {
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

/// A5-core 片 2：RoomPosition packedPos 形态族官方同形（rooms.js:1276-1348/1350-1356/
/// 1578-1585/1594-1600 + utils.js:34-63 fetchXYArguments）。
pub(super) fn roomposition_packedpos_forms() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    // terrain 布局同 terrain_look_and_console_semantics：(0,0) wall、(10,0) swamp
    let mut terrain = vec![0u32; 2500];
    terrain[0] = 1;
    terrain[10] = 2;
    let w = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 50 } },
        ]
    });
    tick(&mut sb, &w)?;

    // 构造器唯一 own 字段 __packedPos：非枚举/可写/非配置（官方 :1288-1292）；
    // 编码 xx<<24|yy<<16|x<<8|y（房间坐标 +128 偏移：W1N1=[-2,-2]→[126,126]）
    let v = sb.run_expr("new RoomPosition(25,37,'W1N1').__packedPos")?;
    assert_eq!(
        v["value"],
        126i64 << 24 | 126i64 << 16 | 25i64 << 8 | 37,
        "packed 编码位形"
    );
    let v = sb.run_expr(
        r#"(() => {
        const p = new RoomPosition(25,37,'W1N1');
        const d = Object.getOwnPropertyDescriptor(p, '__packedPos');
        return JSON.stringify({
            keys: Object.keys(p),
            enum: d.enumerable, writable: d.writable, configurable: d.configurable,
        });
    })()"#,
    )?;
    assert_eq!(
        v["value"], r#"{"keys":[],"enum":false,"writable":true,"configurable":false}"#,
        "own 形态：仅非枚举可写 __packedPos，无 own x/y/roomName"
    );
    // x/y/roomName 是原型 accessor（enumerable、非配置，官方 :1295-1348 defineProperties 缺省）；
    // 方法挂载序与可枚举性同官方（`=` 赋值，for-in 按定义序全量可见）
    let v = sb.run_expr(
        r#"(() => {
        const d = Object.getOwnPropertyDescriptor(RoomPosition.prototype, 'x');
        const dy = Object.getOwnPropertyDescriptor(RoomPosition.prototype, 'y');
        const dr = Object.getOwnPropertyDescriptor(RoomPosition.prototype, 'roomName');
        const forIn = []; for (const k in new RoomPosition(1,2,'W1N1')) forIn.push(k);
        return JSON.stringify({
            xEnum: d.enumerable, xGet: typeof d.get, xSet: typeof d.set, xConf: d.configurable,
            yEnum: dy.enumerable, rEnum: dr.enumerable,
            forIn,
        });
    })()"#,
    )?;
    assert_eq!(
        v["value"],
        r#"{"xEnum":true,"xGet":"function","xSet":"function","xConf":false,"yEnum":true,"rEnum":true,"forIn":["x","y","roomName","toJSON","toString","inRangeTo","isNearTo","getDirectionTo","findPathTo","findClosestByPath","findInRange","findClosestByRange","isEqualTo","getRangeTo","look","lookFor","createFlag","createConstructionSite"]}"#,
        "原型 accessor 形态与 for-in 可见性（官方挂载序）"
    );

    // 非法构造参：官方文案（:1280-1287）——坐标越界/NaN/非法房间名
    let v = sb.run_expr(r#"(() => { try { new RoomPosition(50,10,'W1N1'); return 'no'; } catch (e) { return e.message; } })()"#)?;
    assert_eq!(
        v["value"], "Invalid arguments in RoomPosition constructor",
        "x=50 抛官方文案"
    );
    let v = sb.run_expr(r#"(() => { try { new RoomPosition(10,NaN,'W1N1'); return 'no'; } catch (e) { return e.message; } })()"#)?;
    assert_eq!(
        v["value"], "Invalid arguments in RoomPosition constructor",
        "NaN 坐标"
    );
    let v = sb.run_expr(r#"(() => { try { new RoomPosition(10,10,'E999S9'); return 'no'; } catch (e) { return e.message; } })()"#)?;
    assert_eq!(
        v["value"], "Invalid arguments in RoomPosition constructor",
        "世界界外房间"
    );

    // sim 特判（:1277 + roomNames[0] 槽）：roomName 往返命中缓存槽
    let v = sb.run_expr(r#"JSON.stringify([new RoomPosition(25,25,'sim').roomName, new RoomPosition(25,25,'sim').x])"#)?;
    assert_eq!(v["value"], r#"["sim",25]"#, "sim 特判与缓存槽命中");

    // setter 通道（:1297-1348）：改 x 走字节改写、越界抛 'Invalid coordinate'；
    // roomName setter 换房不动 x/y、界外抛 'Invalid roomName'
    let v = sb.run_expr(r#"(() => {
        const p = new RoomPosition(25,37,'W1N1');
        p.x = 30;
        const yErr = (() => { try { p.y = -1; return 'no'; } catch (e) { return e.message; } })();
        p.roomName = 'E5S5';
        const rErr = (() => { try { p.roomName = 'E999S9'; return 'no'; } catch (e) { return e.message; } })();
        return JSON.stringify({ x: p.x, y: p.y, roomName: p.roomName, yErr, rErr });
    })()"#)?;
    assert_eq!(
        v["value"],
        r#"{"x":30,"y":37,"roomName":"E5S5","yErr":"Invalid coordinate","rErr":"Invalid roomName"}"#,
        "setter 校验与字节改写"
    );

    // __packedPos 可写搬运（UNDOC-PACKEDPOS-003 同形）：整体换位后三 getter 随动
    let v = sb.run_expr(
        r#"(() => {
        const src = new RoomPosition(13,41,'E5S5');
        const dst = new RoomPosition(0,0,'W1N1');
        dst.__packedPos = src.__packedPos;
        return JSON.stringify({ x: dst.x, y: dst.y, roomName: dst.roomName });
    })()"#,
    )?;
    assert_eq!(
        v["value"], r#"{"x":13,"y":41,"roomName":"E5S5"}"#,
        "packedPos 搬运"
    );

    // isEqualTo 快路径（:1578-1585）：同 packed 即相等；数值形态兜底仍同房间
    let v = sb.run_expr(
        r#"(() => {
        const a = new RoomPosition(25,37,'W1N1');
        return JSON.stringify([
            a.isEqualTo(new RoomPosition(25,37,'W1N1')),
            a.isEqualTo(new RoomPosition(25,37,'W2N1')),
            a.isEqualTo(25, 37),
        ]);
    })()"#,
    )?;
    assert_eq!(v["value"], r#"[true,false,true]"#, "isEqualTo 快路径三形态");

    // toJSON（:1350-1356）：Object.assign 字面量 + this（own 无枚举字段）
    let v = sb.run_expr(r#"JSON.stringify(new RoomPosition(25,37,'W1N1'))"#)?;
    assert_eq!(
        v["value"], r#"{"x":25,"y":37,"roomName":"W1N1"}"#,
        "toJSON 三键形态"
    );

    // look() 委托（:1594-1600）：可见房走 room.lookAt(this)（片 3a 起真化，本格仅 terrain）；不可见房抛官方文案
    let v = sb.run_expr(r#"JSON.stringify(new RoomPosition(10,0,'W1N1').look())"#)?;
    assert_eq!(
        v["value"], r#"[{"type":"terrain","terrain":"swamp"}]"#,
        "look 委托 lookAt（该格仅 terrain）"
    );
    let v = sb.run_expr(r#"(() => { try { new RoomPosition(5,5,'W2N1').look(); return 'no'; } catch (e) { return e.message; } })()"#)?;
    assert_eq!(
        v["value"], "Could not access room W2N1",
        "look 不可见房抛错"
    );
    // lookFor 非 terrain 分支同样在委托前抛（:1613-1616）
    let v = sb.run_expr(r#"(() => { try { new RoomPosition(5,5,'W2N1').lookFor(LOOK_CREEPS); return 'no'; } catch (e) { return e.message; } })()"#)?;
    assert_eq!(
        v["value"], "Could not access room W2N1",
        "lookFor 不可见房抛错"
    );

    // fetchXYArguments 官方语义（utils.js:34-63）：instanceof 判定——裸 {x,y}
    // 对象非实例 → undefined 三元组（getRangeTo 得 NaN → JSON null、inRangeTo 恒
    // false、isEqualTo false）；.pos 为实例的 RoomObject 正常解包
    let v = sb.run_expr(
        r#"(() => {
        const p = new RoomPosition(10,10,'W1N1');
        return JSON.stringify({
            rangePlain: p.getRangeTo({x: 12, y: 12}),
            inRangePlain: p.inRangeTo({x: 11, y: 11}, 2),
            equalPlain: p.isEqualTo({x: 10, y: 10}),
            viaPosObj: p.getRangeTo({pos: new RoomPosition(12,12,'W1N1')}),
        });
    })()"#,
    )?;
    assert_eq!(
        v["value"], r#"{"rangePlain":null,"inRangePlain":false,"equalPlain":false,"viaPosObj":2}"#,
        "fetchXYArguments instanceof 判定族"
    );

    // getPositionAt 官方越界返 null（rooms.js:971-976）
    let v = sb.run_expr(
        r#"(() => {
        const oob = Game.rooms['W1N1'].getPositionAt(50,10);
        const ok = Game.rooms['W1N1'].getPositionAt(10,10);
        return JSON.stringify([oob, ok instanceof RoomPosition]);
    })()"#,
    )?;
    assert_eq!(v["value"], r#"[null,true]"#, "getPositionAt 越界 null");

    Ok(())
}

/// A5-core 片 3a：Room look 家族真化（lookAt 固定序 14 类型 / lookForAt 双形态与
/// ERR_INVALID_ARGS / lookAtArea·lookForAtArea asArray 两形态 / energy·resource
/// 双键同指 / 越界官方怪癖——rooms.js:659-860 逐字）。
pub(super) fn roomlook_family_forms() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    let mut terrain = vec![0u32; 2500];
    terrain[0] = 1; // (0,0) wall
    let w = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "looker",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 } },
            { "id": "c2", "type": "creep", "room": "W1N1", "x": 24, "y": 24, "name": "neighbor",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 } },
            { "id": "r1", "type": "resource", "room": "W1N1", "x": 25, "y": 25,
              "resourceType": "energy", "amount": 100 },
            { "id": "rd1", "type": "structure", "structureType": "road", "room": "W1N1",
              "x": 25, "y": 25 },
        ]
    });
    tick(&mut sb, &w)?;

    // lookAt 固定序：creep→energy→resource→structure→terrain（本格对象集）
    let v = sb.run_expr("JSON.stringify(Game.rooms['W1N1'].lookAt(25, 25).map(i => i.type))")?;
    assert_eq!(
        v["value"], r#"["creep","energy","resource","structure","terrain"]"#,
        "lookAt 官方固定序"
    );
    // 包裹形态恰 {type,[type]:obj}（无 x/y——那是 lookAtArea asArray 专属）
    let v = sb.run_expr(
        r#"JSON.stringify(Game.rooms['W1N1'].lookAt(25, 25).map(i => Object.keys(i).sort()))"#,
    )?;
    assert_eq!(
        v["value"],
        r#"[["creep","type"],["energy","type"],["resource","type"],["structure","type"],["terrain","type"]]"#,
        "lookAt 包裹键形态"
    );

    // 双键同指：energy/resource 同一 Resource 实例（rooms.js:487-488）
    let v = sb.run_expr(
        r#"(() => {
            const room = Game.rooms['W1N1'];
            const e = room.lookForAt(LOOK_ENERGY, 25, 25);
            const r = room.lookForAt(LOOK_RESOURCES, 25, 25);
            return JSON.stringify([e.length, r.length, e[0].id === r[0].id, e[0].resourceType]);
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[1,1,true,"energy"]"#,
        "energy/resource 双键同指"
    );

    // lookForAt：未知类型 ERR_INVALID_ARGS；空寄存器 []；terrain 单词数组
    let v = sb.run_expr("Game.rooms['W1N1'].lookForAt('CREEPS', 25, 25)")?;
    assert_eq!(v["value"], -10, "未知 look 类型 ERR_INVALID_ARGS");
    let v = sb.run_expr("Game.rooms['W1N1'].lookForAt(LOOK_NUKES, 25, 25).length")?;
    assert_eq!(v["value"], 0, "空寄存器返 []");
    let v = sb.run_expr("JSON.stringify(Game.rooms['W1N1'].lookForAt(LOOK_TERRAIN, 0, 0))")?;
    assert_eq!(v["value"], r#"["wall"]"#, "terrain 单词数组");

    // 越界：非 terrain 抛官方文案（lookAt 首臂即 creep——同样抛）；terrain 分支
    // 读 flat 数组越界索引得 undefined → 'plain'（官方怪癖保留）
    let v = sb.run_expr(
        r#"(() => { try { Game.rooms['W1N1'].lookForAt('creep', 60, 60); return 'no'; } catch (e) { return e.message; } })()"#,
    )?;
    assert_eq!(
        v["value"], "look coords are out of bounds",
        "非 terrain 越界抛错"
    );
    let v = sb.run_expr(
        r#"(() => { try { Game.rooms['W1N1'].lookAt(60, 60); return 'no'; } catch (e) { return e.message; } })()"#,
    )?;
    assert_eq!(
        v["value"], "look coords are out of bounds",
        "lookAt 越界同抛"
    );
    let v = sb.run_expr("JSON.stringify(Game.rooms['W1N1'].lookForAt(LOOK_TERRAIN, 60, 60))")?;
    assert_eq!(
        v["value"], r#"["plain"]"#,
        "terrain 越界落 flat 数组外 → plain（官方怪癖）"
    );

    // lookAtArea asArray=false：稠密格阵，每格含 terrain 项；坐标系 [y][x]
    let v = sb.run_expr(
        r#"(() => {
            const area = Game.rooms['W1N1'].lookAtArea(23, 23, 26, 26, false);
            return JSON.stringify({
                dense: [23,24,25,26].every(y => [23,24,25,26].every(x => Array.isArray(area[y] && area[y][x]))),
                cell2323: area[23][23].map(i => i.type),
                cell2424: area[24][24].map(i => i.type).sort(),
                cell2525: area[25][25].map(i => i.type).sort(),
            });
        })()"#,
    )?;
    assert_eq!(
        v["value"],
        r#"{"dense":true,"cell2323":["terrain"],"cell2424":["creep","terrain"],"cell2525":["creep","energy","resource","structure","terrain"]}"#,
        "lookAtArea 稠密形态（含逐格 terrain）"
    );
    // lookAtArea asArray=true：条目带 x/y
    let v = sb.run_expr(
        r#"(() => {
            const arr = Game.rooms['W1N1'].lookAtArea(23, 23, 26, 26, true);
            const creepEntry = arr.find(i => i.type === 'creep');
            return JSON.stringify([creepEntry.x, creepEntry.y, Object.keys(creepEntry).sort()]);
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[25,25,["creep","type","x","y"]]"#,
        "lookAtArea asArray 条目带坐标"
    );

    // lookForAtArea asArray=false：行对象惰性格（稀疏）、裸对象无 type 包裹
    let v = sb.run_expr(
        r#"(() => {
            const area = Game.rooms['W1N1'].lookForAtArea(LOOK_CREEPS, 23, 23, 26, 26, false);
            return JSON.stringify({
                row23type: typeof area[23],
                cell2323: typeof area[23][24],
                cell2424: Array.isArray(area[24] && area[24][24]) && area[24][24].length,
                rawName: area[24] && area[24][24] && area[24][24][0] && area[24][24][0].name,
                wrapped: area[24] && area[24][24] && area[24][24][0] && ('type' in area[24][24][0]),
            });
        })()"#,
    )?;
    assert_eq!(
        v["value"],
        r#"{"row23type":"object","cell2323":"undefined","cell2424":1,"rawName":"neighbor","wrapped":false}"#,
        "lookForAtArea 稀疏裸对象形态"
    );
    // lookForAtArea asArray=true：{x,y,[type]} 无 type 键
    let v = sb.run_expr(
        r#"(() => {
            const arr = Game.rooms['W1N1'].lookForAtArea(LOOK_CREEPS, 23, 23, 26, 26, true);
            return JSON.stringify(arr.map(i => [i.x, i.y, Object.keys(i).sort()]));
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[[25,25,["creep","x","y"]],[24,24,["creep","x","y"]]]"#,
        "lookForAtArea asArray 条目形态"
    );
    // lookForAtArea 未知类型：官方无 ERR 门，keys.length 自然 TypeError
    let v = sb.run_expr("Game.rooms['W1N1'].lookForAtArea('bogus', 23, 23, 26, 26, true)")?;
    assert_eq!(v["ok"], false, "未知类型 TypeError（官方同形）: {v:?}");

    // RoomPosition.lookFor 委托：非 terrain 走 room.lookForAt（片 3a 真化）
    let v = sb.run_expr(
        r#"(() => {
            const a = new RoomPosition(25, 25, 'W1N1').lookFor(LOOK_CREEPS);
            const b = new RoomPosition(24, 24, 'W1N1').lookFor('creep');
            return JSON.stringify([a.length, a[0].name, b[0].name]);
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[1,"looker","neighbor"]"#,
        "RoomPosition.lookFor 委托真化"
    );

    Ok(())
}

/// A5-core 片 3b：find 出口臂（rooms.js:592-644——地形 wall 位计算）+
/// serializePath/deserializePath Room 静态化（:528-534 + utils.js:555-609）。
pub(super) fn roomfind_exits_and_path_codec_forms() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    // 地形：四角 wall + top 边 (10,0) 墙 + left 边 (0,10) 墙，其余 plain——
    // 覆盖「角格恒墙、边中段可走」的出口计算面
    let mut terrain = vec![0u32; 2500];
    terrain[0] = 1; // (0,0) wall
    terrain[49] = 1; // (49,0) wall
    terrain[2450] = 1; // (0,49) wall
    terrain[2499] = 1; // (49,49) wall
    terrain[10] = 1; // (10,0) wall——top 边中段墙
    terrain[10 * 50] = 1; // (0,10) wall——left 边中段墙
    let w = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 50 } },
        ]
    });
    tick(&mut sb, &w)?;

    // 四向臂：位置实例/房名/坐标形态——top 边 = y0 且跳过 (0,0)/(49,0)/(10,0) 三墙
    let v = sb.run_expr(
        r#"(() => {
            const top = Game.rooms['W1N1'].find(FIND_EXIT_TOP);
            return JSON.stringify([
                top.length,
                top.every(p => p instanceof RoomPosition && p.roomName === 'W1N1' && p.y === 0),
                top.some(p => p.x === 10),
                top[0].x, top[1].x,
            ]);
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[47,true,false,1,2]"#,
        "FIND_EXIT_TOP 墙位过滤与形态"
    );
    let v = sb.run_expr(
        r#"(() => {
            const left = Game.rooms['W1N1'].find(FIND_EXIT_LEFT);
            return JSON.stringify([
                left.length,
                left.every(p => p.x === 0),
                left.some(p => p.y === 10),
            ]);
        })()"#,
    )?;
    assert_eq!(v["value"], r#"[47,true,false]"#, "FIND_EXIT_LEFT 同构");
    // right/bottom 边各含两角墙（角墙属其所贴的每条边——四角各计入两边）
    let v = sb.run_expr(
        r#"(() => {
            const right = Game.rooms['W1N1'].find(FIND_EXIT_RIGHT);
            const bottom = Game.rooms['W1N1'].find(FIND_EXIT_BOTTOM);
            return JSON.stringify([right.length, bottom.length,
                right.every(p => p.x === 49), bottom.every(p => p.y === 49)]);
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[48,48,true,true]"#,
        "right/bottom 各含两角墙"
    );
    // FIND_EXIT = 四向并集（官方 concat 序 TOP/BOTTOM/RIGHT/LEFT），无重复
    let v = sb.run_expr(
        r#"(() => {
            const all = Game.rooms['W1N1'].find(FIND_EXIT);
            const keys = all.map(p => p.x + ',' + p.y);
            return JSON.stringify([all.length, new Set(keys).size,
                all.every(p => p.x === 0 || p.x === 49 || p.y === 0 || p.y === 49)]);
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[190,190,true]"#,
        "FIND_EXIT 并集 47+48+48+47（角墙只在其各自边排除）"
    );
    // opts.filter 应用（官方出口臂同样过 filter 尾）
    let v =
        sb.run_expr("Game.rooms['W1N1'].find(FIND_EXIT_TOP, { filter: (p) => p.x < 5 }).length")?;
    assert_eq!(
        v["value"], 4,
        "出口臂 opts.filter（x<5 恰 1..4 四格——0 是墙）"
    );

    // serializePath/deserializePath：Room 静态（非实例方法）+ 往返
    let v = sb.run_expr(
        r#"(() => {
            const path = Room.deserializePath('10253347');
            const round = Room.serializePath(path);
            return JSON.stringify([
                typeof Room.serializePath, typeof Room.deserializePath,
                typeof Game.rooms['W1N1'].serializePath,
                JSON.stringify(path),
                round,
            ]);
        })()"#,
    )?;
    assert_eq!(
        v["value"],
        r#"["function","function","undefined","[{\"x\":10,\"y\":25,\"dx\":1,\"dy\":0,\"direction\":3},{\"x\":11,\"y\":25,\"dx\":1,\"dy\":0,\"direction\":3},{\"x\":12,\"y\":26,\"dx\":1,\"dy\":1,\"direction\":4},{\"x\":11,\"y\":26,\"dx\":-1,\"dy\":0,\"direction\":7}]","10253347"]"#,
        "路径编解码往返 + 静态形态（实例无此方法）"
    );
    // serializePath 官方校验文案
    let v = sb.run_expr(
        r#"(() => { try { Room.serializePath('not-array'); return 'no'; } catch (e) { return e.message; } })()"#,
    )?;
    assert_eq!(
        v["value"], "path is not an array",
        "serializePath 非数组文案"
    );
    let v = sb.run_expr("Room.serializePath([])")?;
    assert_eq!(v["value"], "", "空路径 ''");

    // findCache 官方怪癖（rooms.js:585-598/:641-654，评审 P2/P3 清偿后锁定）：
    // 新 tick 隔离缓存——① 同 tick 重复调用返回同一缓存数组引用；② FIND_EXIT
    // 首调携带 filter 时烘焙进缓存，后继无 filter 调用返回陈旧滤过集
    let w2 = serde_json::json!({
        "time": 2,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 50 } },
        ]
    });
    tick(&mut sb, &w2)?;
    let v = sb.run_expr(
        r#"(() => {
            const room = Game.rooms['W1N1'];
            const a = room.find(FIND_EXIT_TOP);
            const b = room.find(FIND_EXIT_TOP);
            const filtered = room.find(FIND_EXIT, { filter: (p) => p.y === 0 });
            const after = room.find(FIND_EXIT);
            const refiltered = room.find(FIND_EXIT, { filter: (p) => p.x === 1 });
            return JSON.stringify([a === b, a[0] === b[0], filtered.length, after.length, refiltered.length]);
        })()"#,
    )?;
    assert_eq!(
        v["value"], r#"[false,true,47,47,1]"#,
        "findCache 官方形态：数组每次新拷（a!==b）、元素同实例（a[0]===b[0]）、首调 filter 烘焙进缓存（after=47 陈旧集）、命中再滤取交集（x===1 恰 1 条）"
    );

    Ok(())
}
