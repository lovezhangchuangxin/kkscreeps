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
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "body": ["move", "work"], "store": { "energy": 50 } },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "store": { "energy": 300 } },
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
    run_expr_channel()?;
    wasm_binary_module()?;
    watchdog_hard_terminate_and_reuse()?;
    watchdog_late_expiry_does_not_kill_next_tick()?;
    memory_gate_terminates_and_reuses()?;
    lifo_registry_drains()?;
    memory_alias_setters()?;
    roomposition_official_semantics()?;
    writeback_watchdog_and_contract()?;
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
    // （第四轮修复后：失败以 {ok:false} 回传，由 runPlayer 通道归为玩家 runtime 错误）
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
    assert!(log_text.contains("hello { a: 1 }"), "log 捕获: {log_text}");
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

    // 缺失属性探测：访问 Game 上不存在的属性会被登记
    sb.run_expr("void Game.totallyMissing")?;
    let missing = sb.drain_missing_api()?;
    assert_eq!(
        missing["Game.totallyMissing"], 1,
        "缺失 API 探测: {missing}"
    );

    // `in` 检查同样触发探测（has trap）——此前静默漏报
    sb.run_expr("'notThere' in Game")?;
    let missing = sb.drain_missing_api()?;
    assert_eq!(
        missing["Game.notThere"], 1,
        "in 检查触发缺失 API 探测: {missing}"
    );

    // pos 实例面（RoomPosition）与 Buffer 静态面纳入探测
    sb.run_expr("void Game.creeps['w1'].pos.findInRange")?;
    sb.run_expr("void Buffer.poolAllocThings")?;
    let missing = sb.drain_missing_api()?;
    assert_eq!(
        missing["RoomPosition.findInRange"], 1,
        "pos 实例面探测: {missing}"
    );
    assert_eq!(
        missing["Buffer.poolAllocThings"], 1,
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

    // 桩调用计数
    let stubs = sb.drain_stub_calls()?;
    assert!(
        stubs["creep.move"].as_i64().unwrap_or(0) >= 1,
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
