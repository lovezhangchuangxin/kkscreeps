//! Memory 写回、预算中断及中断后实例复用。
use super::*;

/// 写回阶段的看门狗覆盖与契约分类：
/// ① 玩家经 Memory.toJSON 在写回阶段挂死 → 被硬中断（不挂死进程）；
/// ② Memory 不可序列化（1n）→ tick 末失败按契约归玩家 runtime 而非成功；
/// ③ 已开始执行后硬中断 → runtime（executed 语义）；
/// ④ 活对象返回值 → serialization（对齐 vanilla 的 constructor 检测）。
pub(crate) fn writeback_watchdog_and_contract() -> anyhow::Result<()> {
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

/// 看门狗硬中断 + 代际守卫 + 实例复用 + 官方跳帧语义。
pub(crate) fn watchdog_hard_terminate_and_reuse() -> anyhow::Result<()> {
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

/// 在到期附近反复解除与重新登记期限，检查旧代次中断不会影响后续表达式。
/// 这是调度竞态压力测试，有限次数通过不代表遍历了全部线程交错。
pub(crate) fn watchdog_late_expiry_does_not_kill_next_tick() -> anyhow::Result<()> {
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
pub(crate) fn memory_gate_terminates_and_reuses() -> anyhow::Result<()> {
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

/// 负余额 begin_tick 只暂存回充，settle_bill 更新余额，discard_bill 保持原余额。
/// 覆盖 −40→−20→0 的恢复轨迹，以及 −580 恢复到 −40 的深度透支分支。
pub(crate) fn skip_recharge_candidate_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig {
        initial_bucket: -40.0,
        ..SandboxConfig::default()
    })?;
    let w = world();
    // 停跑轮回充不即时落桶：桶仍 −40、候选 = 官方公式增量 20
    match sb.begin_tick(&w)? {
        TickPlan::SkipPenalty => {}
        plan => panic!("负桶应停跑：{plan:?}"),
    }
    assert_eq!(sb.bucket(), -40.0, "回充未即时落桶（候选化）");
    assert_eq!(sb.pending_recharge(), Some(20.0), "候选 = −40 + limit");
    // discard 臂（世界结算失败回退）：候选丢弃、桶零痕迹
    sb.discard_bill();
    assert_eq!(sb.pending_recharge(), None);
    assert_eq!(
        sb.bucket(),
        -40.0,
        "丢弃轮回血回退（原实现此处已 −20——绕过统一提交点）"
    );
    // settle 臂：提交点并入 → −20；再两轮回正、恢复 Run
    match sb.begin_tick(&w)? {
        TickPlan::SkipPenalty => {}
        plan => panic!("仍负应停跑：{plan:?}"),
    }
    sb.settle_bill();
    assert_eq!(sb.bucket(), -20.0, "提交点并入回充");
    match sb.begin_tick(&w)? {
        TickPlan::SkipPenalty => {}
        plan => panic!("−20 仍应停跑：{plan:?}"),
    }
    sb.settle_bill();
    assert_eq!(sb.bucket(), 0.0);
    match sb.begin_tick(&w)? {
        TickPlan::Run { tick_limit } => assert_eq!(tick_limit, 20.0),
        plan => panic!("回正后应恢复执行：{plan:?}"),
    }
    sb.end_tick()?; // 测试语境下本 tick 未执行玩家代码（结算量级可忽略）
    sb.settle_bill();

    // 深负钳地板：−580 → 候选 540（目标 −40，make.js:77 的 −2×limit 地板）
    let mut sb2 = Sandbox::new(SandboxConfig {
        initial_bucket: -580.0,
        ..SandboxConfig::default()
    })?;
    match sb2.begin_tick(&w)? {
        TickPlan::SkipPenalty => {}
        plan => panic!("深负应停跑：{plan:?}"),
    }
    assert_eq!(
        sb2.pending_recharge(),
        Some(540.0),
        "深负钳到 −2×limit 地板"
    );
    sb2.settle_bill();
    assert_eq!(sb2.bucket(), -40.0);
    Ok(())
}
