//! link 默认贡献：有效规则贯穿放置、建造、结算与配额，
//! 处理链（before/wrap/replace/after）的执行序与 next 语义，
//! 以及装配对缺失实现的拒绝。

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use kk_engine::settle::contrib::{ActionHandler, ContributionDispatch, TickHandler};
use kk_mod_api::builtin::{self, LinkRules};
use kk_mod_api::{Contribution, FreezeOptions, HandlerDescriptor, Key, RuleOverride, Source};
use serde_json::json;

fn place_generic(f: &mut Fixture, x: u8, y: u8, body: serde_json::Value) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: body.to_string(),
            }),
        )
        .unwrap()
}

fn tower(f: &mut Fixture, x: u8, y: u8, energy: i64) -> ObjId {
    place_generic(
        f,
        x,
        y,
        json!({
            "type": "structure", "structureType": "tower", "owner": "p1",
            "hits": 3000, "hitsMax": 3000, "nextDecayTime": 1_000_000,
            "store": {"energy": energy}, "storeCapacity": 1000,
            "storeCapacityResource": {"energy": 1000},
        }),
    )
}

fn data_of(f: &Fixture, id: ObjId) -> serde_json::Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("Generic 不存在：{other:?}"),
    }
}

/// 构造只带规则覆盖的测试贡献；namespace 须为小写合法命名空间。
fn overrides_mod(rules: &[(&str, kk_mod_api::Number)]) -> Contribution {
    let mut c = Contribution {
        source: Source {
            namespace: "testmod".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::new(),
        field_mounts: Vec::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
    };
    for (rule, value) in rules {
        c.overrides.push(RuleOverride {
            rule: Key::new(format!("builtin:{rule}")).unwrap(),
            value: value.clone(),
        });
    }
    c
}

/// 冻结 builtin link + 覆盖，并提取有效规则。
fn frozen_plan(rules: &[(&str, kk_mod_api::Number)]) -> kk_mod_api::ContributionPlan {
    kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![overrides_mod(rules)],
        FreezeOptions::default(),
    )
    .unwrap()
}

fn rules_of(plan: &kk_mod_api::ContributionPlan) -> LinkRules {
    LinkRules::from_plan(plan.rules()).unwrap()
}

/// 显式规则的放置上下文；控制器可为 None。
fn place_ctx(links: LinkRules) -> kk_world::place::PlaceCtx<'static> {
    kk_world::place::PlaceCtx {
        time: 50,
        links,
        stores: kk_world::place::builtin_store_rules(),
        controller: None,
        name_seq: 0,
    }
}

/// 槽执行链的开销基准：Native 直连与 Chained（无操作 before/wrap/after
/// 各一）在等量 link tick 上的耗时对照。手动运行（须 --nocapture 才有
/// 计时输出）：
/// `cargo test -p kk-engine --release --test settle_tests \
///  contrib_dispatch_native_vs_chained_bench -- --ignored --nocapture`
/// 输出每次 link tick 的纳秒数，数字记录在
/// docs/research/notes/b5-kkpv-wiring-2026-10-03.md；两臂夹具相同
/// （每臂 50 link、cooldown 起点相同且全程为正），差值即链分派开销。
/// 无日志回调——闭包体留空，避免把记录成本计入链开销。
#[test]
#[ignore = "手动基准（计时输出，非回归断言）"]
fn contrib_dispatch_native_vs_chained_bench() {
    use std::time::Instant;
    const LINKS: usize = 50;
    const ITERATIONS: u32 = 2000;

    let mut contribution = overrides_mod(&[]);
    let mut hook = |name: &str| {
        let key = Key::new(format!("testmod:{name}")).unwrap();
        contribution.bindings.insert(key.clone());
        key
    };
    let before = hook("bench_before");
    let wrap = hook("bench_wrap");
    let after = hook("bench_after");
    for (key, kind) in [
        (&before, kk_mod_api::HandlerKind::Before),
        (&wrap, kk_mod_api::HandlerKind::Wrap),
        (&after, kk_mod_api::HandlerKind::After),
    ] {
        contribution.handlers.push(HandlerDescriptor {
            key: key.clone(),
            slot: Key::new("builtin:link.tick").unwrap(),
            kind,
            binding: key.clone(),
            depends_on: BTreeSet::new(),
        });
    }
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
        FreezeOptions::default(),
    )
    .unwrap();
    let mut tick_handlers = BTreeMap::new();
    tick_handlers.insert(before, TickHandler::Before(Box::new(|_, _| Ok(()))));
    tick_handlers.insert(
        wrap,
        TickHandler::Wrap(Box::new(|_ctx, id, next| next.call(id))),
    );
    tick_handlers.insert(after, TickHandler::After(Box::new(|_, _| Ok(()))));
    let chained = std::rc::Rc::new(
        ContributionDispatch::assemble(&plan, BTreeMap::new(), tick_handlers).unwrap(),
    );
    let native = std::rc::Rc::new(ContributionDispatch::default());

    let bench = |label: &str, dispatch: &std::rc::Rc<ContributionDispatch>| {
        let mut f = Fixture::new(7, terrain_with(&[]));
        f.controller(5, Some("p1"));
        for i in 0..LINKS {
            place_generic(
                &mut f,
                (i % 40 + 2) as u8,
                (i / 40 * 5 + 2) as u8,
                json!({
                    "type": "structure", "structureType": "link", "owner": "p1",
                    "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
                    "store": {"energy": 0}, "storeCapacity": 800,
                    "storeCapacityResource": {"energy": 800},
                    "cooldown": 1_000_000,
                }),
            );
        }
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            f.settle_with_dispatch(IntentBatch::default(), dispatch.clone());
        }
        let per_link_ns =
            start.elapsed().as_nanos() as f64 / (f64::from(ITERATIONS) * LINKS as f64);
        println!("{label}: {per_link_ns:.1} ns/link-tick（{LINKS} link × {ITERATIONS} 轮）");
        per_link_ns
    };

    let native_ns = bench("native", &native);
    let chained_ns = bench("chained（before+wrap+after 无操作）", &chained);
    println!(
        "链开销：{:+.1} ns/link-tick（{:+.0}%）",
        chained_ns - native_ns,
        (chained_ns / native_ns - 1.0) * 100.0
    );
}

#[path = "m2b_contrib_link_family/rules.rs"]
mod rules;

#[path = "m2b_contrib_link_family/chains.rs"]
mod chains;
