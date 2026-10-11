//! 常驻优化的保存语义与可信解码边界回归；由主套件串行执行。
use super::*;

pub(super) fn memory_residency_regressions() -> anyhow::Result<()> {
    let cases = [
        (
            "Memory.a = Object.freeze({v: undefined});",
            "{\"a\":{}}",
            "Memory.a.v === undefined",
        ),
        (
            "Memory.temp = undefined; Memory.toJSON = function() { return {hasTemp: 'temp' in this}; };",
            "{\"hasTemp\":true}",
            "Memory.hasTemp === true",
        ),
        (
            "Object.defineProperty(Memory, 'bad', {enumerable: true, get() { throw Error('not serializable by walking'); }}); Memory.toJSON = () => ({ok: 1});",
            "{\"ok\":1}",
            "Memory.ok === 1",
        ),
        (
            "globalThis.reads = 0; Object.defineProperty(Memory, 'n', {enumerable: true, get() { return ++reads; }});",
            "{\"n\":1}",
            "Memory.n === 1 && reads === 1",
        ),
        (
            "Memory.a = new Array(3); Memory.a[0] = 5; Memory.a.extra = 7;",
            "{\"a\":[5,null,null]}",
            "Memory.a.extra === undefined && Memory.a[1] === null && 1 in Memory.a",
        ),
        (
            "Memory.a = [1, 2]; globalThis.previousMemory = Memory;",
            "{\"a\":[1,2]}",
            "Memory === previousMemory && Memory.a[1] === 2",
        ),
        (
            "Memory.a = [1]; Memory.a[Symbol('hidden')] = 7; globalThis.previousMemory = Memory;",
            "{\"a\":[1]}",
            "Memory !== previousMemory && Object.getOwnPropertySymbols(Memory.a).length === 0",
        ),
        (
            "Memory.a = {v: undefined};",
            "{\"a\":{}}",
            "!('v' in Memory.a)",
        ),
    ];
    for (setup, expected, next) in cases {
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        sb.set_raw_memory("{}")?;
        assert!(matches!(sb.begin_tick(&world())?, TickPlan::Run { .. }));
        assert_eq!(
            sb.run_expr(&format!("(() => {{ {setup} return true; }})()"))?["ok"],
            true,
            "{setup}"
        );
        let out = sb.end_tick()?;
        assert_eq!(out["ok"], true, "{setup}: {out}");
        assert_eq!(out["rawMemory"], expected, "{setup}: {out}");
        sb.settle_bill();
        sb.set_raw_memory(expected)?;
        assert!(matches!(sb.begin_tick(&world())?, TickPlan::Run { .. }));
        assert_eq!(sb.run_expr(next)?["value"], true, "{setup}: {next}");
        assert_eq!(sb.end_tick()?["ok"], true);
        sb.settle_bill();
    }
    Ok(())
}

pub(super) fn kkpv_residency_prototype_guard() -> anyhow::Result<()> {
    // 两个对象触发新增、命中更新与删除；原型钩子若收到内部 Map 或记录即失败。
    let mut w = json!({
        "time": 1, "user": "p1", "cpu": {"limit": 20, "tickLimit": 500, "bucket": 10000},
        "rooms": {"W1N1": {"name": "W1N1", "terrain": vec![0; 2500]}},
        "objects": [
            {"id": "kk0000000000000001", "type": "creep", "room": "W1N1", "x": 10, "y": 10,
             "name": "anchor", "owner": "p1", "body": ["move"], "store": {"energy": 0}, "hits": 100},
            {"id": "kk0000000000000002", "type": "resource", "room": "W1N1", "x": 11, "y": 11,
             "resourceType": "energy", "amount": 10}
        ]
    });
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    sb.begin_tick(&w)?;
    assert_eq!(sb.run_expr(r#"(() => {
        globalThis.hookCalls = 0;
        globalThis.savedMethods = [];
        for (const [proto, names] of [[Map.prototype, ['get', 'set', 'delete', 'forEach']],
                                     [Set.prototype, ['add', 'has']]]) {
            for (const name of names) {
                const original = proto[name];
                savedMethods.push([proto, name, original]);
                proto[name] = function(...args) { hookCalls++; return Reflect.apply(original, this, args); };
            }
        }
        return true;
    })()"#)?["value"], true);
    sb.end_tick()?;
    sb.settle_bill();
    for time in 2..=4 {
        w["time"] = json!(time);
        w["objects"][0]["hits"] = json!(100 - time);
        if time == 4 {
            w["objects"].as_array_mut().unwrap().pop();
        }
        let private = kk_world::kkpv::kkpv_bundle_from_world_json(&w, [0; 32], [0; 32])
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let public = kk_world::kkpv::kkpv_public_bundle_from_world_json(&w, [0; 32], [0; 32])
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        sb.begin_tick_kkpv(
            &kk_world::kkpv::kkpv_envelope_from_world_json(&w),
            &kk_runner::SharedKkpvBundle::from_vec(public),
            &kk_runner::SharedKkpvBundle::from_vec(private),
        )?;
        let result = sb.run_expr("Game.creeps.anchor.hits")?;
        assert_eq!(result["value"], 100 - time);
        sb.end_tick()?;
        sb.settle_bill();
    }
    let out = sb.run_expr(
        r#"(() => {
        for (const [proto, name, original] of savedMethods) proto[name] = original;
        return hookCalls;
    })()"#,
    )?;
    assert_eq!(
        out["value"], 0,
        "可信生命周期不能进入玩家 Map/Set 原型钩子: {out}"
    );
    Ok(())
}

/// 字段级复用（FLAG_FIELD_DIRTY）：双拍手搓束精确断言 KKDIAG remat——
/// 定宽变化拍变长字段零重物化（旧实现等于该行全部变长字段数），单个
/// 变长字段变格拍恰 1 次重物化。
pub(super) fn field_dirty_reuse_counters() -> anyhow::Result<()> {
    use kk_schema::kkpv::{KkpvBlockRef, KkpvLimits, KkpvRow, KkpvVal, encode_with_dirty};
    let limits = KkpvLimits::default();
    let block = |tick: u64, hits: i64, store_energy: i64| KkpvBlockRef {
        private: true,
        tick,
        generation: 0,
        plan_hash: [0; 32],
        audience_hash: [0; 32],
        room_name: "W1N1",
        terrain: None,
        rows: vec![KkpvRow {
            id: 1,
            type_key: std::borrow::Cow::Borrowed("creep"),
            fields: vec![
                ("type", KkpvVal::Str("creep")),
                ("room", KkpvVal::Str("W1N1")),
                ("x", KkpvVal::Int(10)),
                ("y", KkpvVal::Int(10)),
                ("hits", KkpvVal::Int(hits)),
                ("owner", KkpvVal::Str("p1")),
                ("name", KkpvVal::Str("m1")),
                ("body", KkpvVal::Array(vec![KkpvVal::Str("move")])),
                (
                    "store",
                    KkpvVal::Object(vec![("energy", KkpvVal::Int(store_energy))]),
                ),
            ],
        }],
    };
    // 束框架：u32 块数 + 逐块 u32 长度 + 块字节（与 forEachBlock 同源）。
    fn frame(block_bytes: &[u8]) -> Vec<u8> {
        let mut out = 1u32.to_le_bytes().to_vec();
        out.extend((block_bytes.len() as u32).to_le_bytes());
        out.extend(block_bytes);
        out
    }
    let world = |time: u64| {
        serde_json::json!({
            "time": time, "user": "p1",
            "cpu": {"limit": 20, "tickLimit": 500, "bucket": 10000},
            "rooms": {"W1N1": {"name": "W1N1", "terrain": vec![0u8; 2500]}},
            "objects": [],
        })
    };
    let mut sb = Sandbox::new(SandboxConfig {
        run_diag: true,
        ..SandboxConfig::default()
    })?;
    let begin = |sb: &mut Sandbox, time: u64, bundle: Vec<u8>| -> anyhow::Result<()> {
        // 公开束块头带 tick，须与信封同拍重建。
        let public =
            kk_world::kkpv::kkpv_public_bundle_from_world_json(&world(time), [0; 32], [0; 32])
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        sb.begin_tick_kkpv(
            &kk_world::kkpv::kkpv_envelope_from_world_json(&world(time)),
            &kk_runner::SharedKkpvBundle::from_vec(public),
            &kk_runner::SharedKkpvBundle::from_vec(bundle),
        )?;
        Ok(())
    };
    // 首拍：物化全部变长字段（读取触发惰性首解码）。
    let first =
        encode_with_dirty(&block(1, 100, 5), &limits, None).map_err(|e| anyhow::anyhow!("{e}"))?;
    begin(&mut sb, 1, frame(first.bytes()))?;
    let r = sb.run_expr("Game.getObjectById('kk0000000000000001') !== null")?;
    assert_eq!(r["value"], true, "对象在首拍可见");
    sb.end_tick()?;
    sb.settle_bill();
    // 次拍：仅定宽 hits 变化（行脏、全部变长字段位清零）——remat 必须为 0。
    let second = encode_with_dirty(&block(2, 99, 5), &limits, Some(&first))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    begin(&mut sb, 2, frame(second.bytes()))?;
    let r = sb.run_expr("Game.getObjectById('kk0000000000000001').hits")?;
    assert_eq!(r["value"], 99, "定宽变化读到新值: {r}");
    sb.end_tick()?;
    sb.settle_bill();
    let remat = sb
        .drain_console()
        .iter()
        .filter_map(|line| line.text.strip_prefix("KKDIAG "))
        .next_back()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|d| d["remat"].as_i64());
    assert_eq!(remat, Some(0), "定宽变化拍的变长字段零重物化");
    // 第三拍：仅 store 变格（其余变长字段仍清零）——remat 恰 1。
    let third = encode_with_dirty(&block(3, 99, 6), &limits, Some(&second))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    begin(&mut sb, 3, frame(third.bytes()))?;
    let r = sb.run_expr("Game.getObjectById('kk0000000000000001').store.energy")?;
    assert_eq!(r["value"], 6, "变格字段读到新值");
    sb.end_tick()?;
    sb.settle_bill();
    let remat = sb
        .drain_console()
        .iter()
        .filter_map(|line| line.text.strip_prefix("KKDIAG "))
        .next_back()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|d| d["remat"].as_i64());
    assert_eq!(remat, Some(1), "单个变长字段变格恰 1 次重物化");
    Ok(())
}
