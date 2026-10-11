//! 两种安装入口的完整发布与末步失败回归，验证实际引擎状态。
use crate::settle::{
    Engine, RoomCtx,
    contrib::{SlotCall, SlotExecutor},
};
use kk_mod_api::buildings::{BuildingDescriptor, BuildingPathing};
use kk_mod_api::resources::ResourceDescriptor;
use kk_mod_api::{
    Contribution, ContributionPlan, FreezeOptions, Key, Number, RuleOverride, Source,
};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

struct UncalledExecutor;
impl SlotExecutor for UncalledExecutor {
    fn run(
        &self,
        _: &kk_mod_api::SlotPlan,
        _: &RoomCtx,
        _: SlotCall<'_>,
    ) -> Result<kk_mod_api::SlotOutcome, String> {
        panic!("安装只装配执行表，不应执行宿主槽")
    }
}

fn plan(unwired: bool) -> Arc<ContributionPlan> {
    let contribution = Contribution {
        source: Source {
            namespace: "installation".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: if unwired {
            BTreeSet::from([Key::new("installation:unwired").unwrap()])
        } else {
            BTreeSet::new()
        },
        object_types: vec![],
        field_mounts: vec![],
        intents: vec![],
        rules: vec![],
        overrides: [
            ("link.cooldown", 3),
            ("store.container.capacity", 3333),
            ("creep.attack.range", 2),
            ("creep.move.fatigueLimit", 4),
        ]
        .into_iter()
        .map(|(name, value)| RuleOverride {
            rule: Key::new(format!("builtin:{name}")).unwrap(),
            value: Number::Signed(value),
        })
        .collect(),
        constraints: BTreeSet::new(),
        slots: vec![],
        handlers: vec![],
        resources: vec![ResourceDescriptor {
            key: Key::new("installation:crystal").unwrap(),
        }],
        buildings: vec![BuildingDescriptor {
            key: Key::new("installation:gauge").unwrap(),
            player_class: "Gauge".into(),
            cost: vec![("energy".into(), 100)],
            hits_max: 500,
            pathing: BuildingPathing::Obstacle,
            store_capacity: Some(200),
            production: None,
            fields: vec![],
            lifecycle_slots: BTreeSet::new(),
        }],
    };
    Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            FreezeOptions::default(),
        )
        .unwrap(),
    )
}

fn install(engine: &mut Engine, plan: Arc<ContributionPlan>, hosted: bool) -> Result<(), String> {
    if hosted {
        engine.install_hosted_plan(
            plan,
            Rc::new(UncalledExecutor),
            BTreeSet::new(),
            BTreeSet::new(),
        )
    } else {
        engine.install_plan(plan, BTreeMap::new(), BTreeMap::new())
    }
}

#[test]
fn late_binding_failure_preserves_every_installed_domain_and_committed_state() {
    for hosted in [false, true] {
        let mut engine = Engine::default();
        engine
            .register_room("W1N1", kk_core::Terrain::plains())
            .unwrap();
        let original = serde_json::to_value(engine.checkpoint().unwrap()).unwrap();
        let rules = (engine.links, engine.stores, engine.combat, engine.move_gate);
        let old_plan = engine.plan.clone();
        let resources = engine.resources.clone();
        let buildings = engine.buildings.clone();
        let extensions = engine.extensions.clone();
        let dispatch = engine.contrib.clone();
        let error = install(&mut engine, plan(true), hosted).unwrap_err();
        assert_eq!(error, "计划声明了未安装的绑定 installation:unwired");
        assert_eq!(
            (engine.links, engine.stores, engine.combat, engine.move_gate),
            rules
        );
        assert!(Arc::ptr_eq(&engine.plan, &old_plan));
        assert!(std::sync::Arc::ptr_eq(&engine.resources, &resources));
        assert!(std::sync::Arc::ptr_eq(&engine.buildings, &buildings));
        assert!(std::sync::Arc::ptr_eq(&engine.extensions, &extensions));
        assert!(std::sync::Arc::ptr_eq(&engine.contrib, &dispatch));
        assert_eq!(
            serde_json::to_value(engine.checkpoint().unwrap()).unwrap(),
            original
        );
        // 拒绝新计划之后旧引擎仍可继续结算。
        engine.tick_with_intents(Default::default()).unwrap();
        assert_eq!(engine.world().time(), 1);
    }
}

#[test]
fn both_installers_publish_rules_catalogs_dispatch_and_plan_together() {
    for hosted in [false, true] {
        let mut engine = Engine::default();
        engine
            .register_room("W1N1", kk_core::Terrain::plains())
            .unwrap();
        let mut before = serde_json::to_value(engine.checkpoint().unwrap()).unwrap();
        let resources = engine.resources.clone();
        let buildings = engine.buildings.clone();
        let extensions = engine.extensions.clone();
        let dispatch = engine.contrib.clone();
        let candidate = plan(false);
        install(&mut engine, candidate.clone(), hosted).unwrap();
        assert!(Arc::ptr_eq(&engine.plan, &candidate));
        assert_eq!(engine.links.cooldown, 3);
        assert_eq!(engine.stores.container, 3333);
        assert_eq!(engine.combat.attack, 2);
        assert_eq!(engine.move_gate.fatigue_limit, 4);
        assert!(engine.resources.contains("installation:crystal"));
        assert_eq!(
            engine.buildings.get("installation:gauge").unwrap().hits_max,
            500
        );
        assert!(!std::sync::Arc::ptr_eq(&engine.resources, &resources));
        assert!(!std::sync::Arc::ptr_eq(&engine.buildings, &buildings));
        assert!(!std::sync::Arc::ptr_eq(&engine.extensions, &extensions));
        assert!(!std::sync::Arc::ptr_eq(&engine.contrib, &dispatch));
        before["plan_hash"] = serde_json::json!(engine.plan_hash().hex());
        assert_eq!(
            serde_json::to_value(engine.checkpoint().unwrap()).unwrap(),
            before
        );
    }
}
