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
            let out = sb.run_loop()?;
            let used = match &out {
                TickOutcome::Completed { .. } => sb.cpu_used_ms(),
                TickOutcome::Terminated => 0.0,
            };
            sb.end_tick(used)?;
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
    watchdog_hard_terminate_and_reuse()?;
    memory_gate_terminates_and_reuses()?;
    lifo_registry_drains()?;
    Ok(())
}

/// lodash 预置、官方常量平铺、Deno 遮蔽、运行时健康。
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

    let v = sb.run_expr("require('lodash') === _")?;
    assert_eq!(v["value"], true, "require('lodash') 命中预置");

    let v = sb.run_expr("typeof Buffer !== 'undefined' && Buffer.from('hi').length")?;
    assert_eq!(v["value"], 2, "Buffer 垫片可用");

    let v = sb.run_expr("6 * 7")?;
    assert_eq!(v["value"], 42, "遮蔽后运行时健康");
    Ok(())
}

/// 样例 bot：模块加载、循环依赖、子目录模块、global 跨 tick 持久、模块缓存。
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

    // 循环依赖语义：role_harvester 加载时 utils 尚未完成初始化
    let v = sb.run_expr("JSON.stringify(require('role_harvester'))")?;
    let role = v["value"].as_str().unwrap_or_default();
    assert!(
        role.contains("\"role\":\"harvester\""),
        "模块导出正确: {role}"
    );

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

/// 看门狗硬中断 + 代际守卫 + 实例复用 + 跳帧惩罚。
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

    let t0 = std::time::Instant::now();
    let out = tick(&mut sb, &world())?;
    let elapsed = t0.elapsed().as_millis();
    assert!(matches!(out, TickOutcome::Terminated), "死循环被硬中断");
    assert!(elapsed < 2_000, "硬中断及时生效（{elapsed}ms）");
    // 初始 100 + 回充 20 = 120，扣整份 tickLimit 100 → 剩 20（bucket_sim 同款语义）
    assert!(
        (sb.bucket() - 20.0).abs() < 0.01,
        "按整份 tickLimit 计费: {}",
        sb.bucket()
    );

    // 跳帧惩罚
    match sb.begin_tick(&world())? {
        TickPlan::SkipPenalty => {}
        _ => panic!("硬中断后下一 tick 应被跳过（skipTicksPenalty）"),
    }

    // 实例复用：换正常代码后继续可跑
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { globalThis.__revived = 1; };",
    );
    sb.set_modules(table)?;
    sb.begin_tick(&world())?;
    sb.end_tick(0.0)?; // 跳帧 tick 不执行代码，直接结算
    let out = tick(&mut sb, &world())?;
    assert!(
        matches!(out, TickOutcome::Completed { .. }),
        "runtime 复用成功"
    );
    let v = sb.run_expr("globalThis.__revived")?;
    assert_eq!(v["value"], 1);
    Ok(())
}

/// 内存双闸门：接近堆上限优雅终止（进程不崩），实例复用。
fn memory_gate_terminates_and_reuses() -> anyhow::Result<()> {
    let cfg = SandboxConfig {
        heap_limit_bytes: 48 * 1024 * 1024,
        ..SandboxConfig::default()
    };
    let mut sb = Sandbox::new(cfg)?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { const a = []; while (true) a.push(new Array(10000).fill('xxxxxxxxxx')); };",
    );
    sb.set_modules(table)?;

    let out = tick(&mut sb, &world())?;
    assert!(
        matches!(out, TickOutcome::Terminated),
        "内存逼近上限被优雅终止（非进程崩溃）"
    );

    // 复用
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "module.exports.loop = function () { globalThis.__afterOom = 1; };",
    );
    sb.set_modules(table)?;
    sb.begin_tick(&world())?; // 惩罚跳帧 tick
    sb.end_tick(0.0)?;
    let out = tick(&mut sb, &world())?;
    assert!(
        matches!(out, TickOutcome::Completed { .. }),
        "OOM 后实例复用成功"
    );
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
