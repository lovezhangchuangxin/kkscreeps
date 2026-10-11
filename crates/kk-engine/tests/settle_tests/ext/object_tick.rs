//! 自有生命周期的装配门与纯 Rust 扩展中心替换。
use super::*;

fn lifecycle() -> Contribution {
    let mut c = counter_contribution(&[]);
    c.intents.clear();
    c.bindings
        .remove(&Key::new("extmod:increment.process").unwrap());
    c.object_types[0].lifecycle_slots = BTreeSet::from([Key::new("extmod:counter.tick").unwrap()]);
    c.slots = vec![SlotDescriptor {
        key: Key::new("extmod:counter.tick").unwrap(),
        stage: kk_mod_api::Stage::ObjectTick,
        native_binding: Key::new("extmod:counter.tick.process").unwrap(),
    }];
    c.bindings.insert(c.slots[0].native_binding.clone());
    c
}

#[test]
fn installation_rejects_wrong_lifecycle_name_stage_and_center_without_changing_plan() {
    for (case, needle) in [
        (0, "自身的 object.tick"),
        (1, "ObjectTick"),
        (2, "自身 process"),
    ] {
        let mut c = lifecycle();
        match case {
            0 => {
                c.slots[0].key = Key::new("extmod:counter.preTick").unwrap();
                c.object_types[0].lifecycle_slots = BTreeSet::from([c.slots[0].key.clone()]);
            }
            1 => c.slots[0].stage = kk_mod_api::Stage::Market,
            _ => {
                c.slots[0].native_binding = Key::new("extmod:unwired.process").unwrap();
                c.bindings.insert(c.slots[0].native_binding.clone());
            }
        }
        let mut e = Engine::new(kk_world::World::new(2));
        let hash = e.plan_hash();
        let error = e
            .install_plan_with_ext(
                frozen(c),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap_err();
        assert!(error.contains(needle), "{error}");
        assert_eq!(e.plan_hash(), hash);
    }
}

#[test]
fn rust_lifecycle_replacement_installs_original_binding_and_ticks_without_intents() {
    for replaced in [false, true] {
        let mut c = lifecycle();
        let mut handlers = BTreeMap::new();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        if replaced {
            let binding = Key::new("extmod:handler.replace").unwrap();
            c.bindings.insert(binding.clone());
            c.handlers.push(HandlerDescriptor {
                key: binding.clone(),
                slot: c.slots[0].key.clone(),
                kind: HandlerKind::Replace,
                binding: binding.clone(),
                depends_on: BTreeSet::new(),
            });
            handlers.insert(
                binding,
                ExtHandler::Replace(observer(log.clone(), "replace")),
            );
        }
        let processes = BTreeMap::from([(
            c.slots[0].native_binding.clone(),
            observer(log.clone(), "tick"),
        )]);
        let mut e = Engine::new(kk_world::World::new(2));
        e.install_plan_with_ext(
            frozen(c),
            BTreeMap::new(),
            BTreeMap::new(),
            handlers,
            processes,
        )
        .unwrap();
        e.register_room("W1N1", kk_core::Terrain::plains()).unwrap();
        e.insert_object(
            "W1N1",
            RoomObject::Mod(ModObj {
                id: 0,
                x: 10,
                y: 12,
                type_key: "extmod:counter".into(),
                data: json!({"owner":"p1","x":10,"y":12,"value":0}).to_string(),
            }),
        )
        .unwrap();
        e.tick_with_intents(IntentBatch::default()).unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            vec![if replaced { "replace" } else { "tick" }]
        );
        assert_eq!(e.world().time(), 1);
    }
}
