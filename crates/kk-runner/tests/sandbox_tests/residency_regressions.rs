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
