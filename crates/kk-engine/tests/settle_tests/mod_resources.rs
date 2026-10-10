//! Mod 声明命名空间资源的结算面：搬运、掉落、拾取、死亡掉落与
//! 市场/跨房的显式拒绝。资源目录经冻结计划进入贡献平面。
use super::*;

/// 声明 resmod:crystal 与 resmod:dust 的贡献平面（纯数据，无处理器）。
fn resource_plane() -> kk_engine::settle::Plane {
    let mut contribution = kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "resmod".into(),
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
        resources: vec![
            kk_mod_api::resources::ResourceDescriptor {
                key: kk_mod_api::Key::new("resmod:crystal").unwrap(),
            },
            kk_mod_api::resources::ResourceDescriptor {
                key: kk_mod_api::Key::new("resmod:dust").unwrap(),
            },
        ],
        buildings: Vec::new(),
    };
    contribution.resources.sort_by(|a, b| a.key.cmp(&b.key));
    let plan = std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    );
    kk_engine::settle::Plane {
        world_span: kk_engine::settle::Plane::UNWRAPPED_SPAN,
        buildings: Default::default(),
        links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
        stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
        combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
        move_gate: Default::default(),
        quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
        static_passability: Default::default(),
        resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
        contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan,
    }
}

fn action(
    player: &str,
    actor: ObjId,
    target: ObjId,
    action: Action,
    resource: &str,
    amount: i64,
) -> IntentBatch {
    let mut data = IntentData::target(target);
    data.resource_type = Some(resource.into());
    data.amount = Some(amount);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(player, actor, action, data);
    batch
}

/// 放置 storage（Generic 表示）并写入 store 与共享容量字段。
fn storage_with(f: &mut Fixture, store: Value) -> ObjId {
    let id = generic_structure(f, 10, 11, "storage", "p1", 10_000);
    f.world
        .edit_object(id, |object| {
            if let Some(RoomObject::Generic(g)) = Some(object) {
                let mut data: Value = serde_json::from_str(&g.data).unwrap();
                data["store"] = store;
                data["storeCapacity"] = serde_json::json!(kk_core::constants::STORAGE_CAPACITY);
                g.data = data.to_string().into();
            }
            Ok(())
        })
        .unwrap();
    id
}

/// 读取 storage（Generic 表示）的 JSON store 中某资源数量。
fn storage_slot(f: &Fixture, id: ObjId, resource: &str) -> i64 {
    match f.obj(id) {
        RoomObject::Generic(g) => {
            let data: Value = serde_json::from_str(&g.data).unwrap();
            data["store"][resource].as_i64().unwrap_or(0)
        }
        _ => panic!("非 Generic 结构"),
    }
}

fn creep_store(f: &Fixture, id: ObjId, resource: &str) -> i64 {
    match f.obj(id) {
        RoomObject::Creep(c) => {
            kk_world::types::store_extra_slot(&c.store, &c.store_extra, resource)
        }
        _ => panic!("非 creep"),
    }
}

/// 声明资源的 transfer/withdraw 走既有资源槽链路；库存照常落账。
#[test]
fn declared_resources_flow_through_transfer_and_withdraw() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = resource_plane();
    let hauler = f.creep_with_mineral(
        "hauler",
        10,
        10,
        &[BodyKind::Carry],
        0,
        "resmod:crystal",
        30,
    );
    let store = storage_with(&mut f, serde_json::json!({"resmod:crystal": 5}));
    f.settle_with_plane(
        action("p1", hauler, store, Action::Transfer, "resmod:crystal", 20),
        plane.clone(),
    );
    assert_eq!(storage_slot(&f, store, "resmod:crystal"), 25);
    assert_eq!(creep_store(&f, hauler, "resmod:crystal"), 10);
    f.settle_with_plane(
        action("p1", hauler, store, Action::Withdraw, "resmod:crystal", 10),
        plane,
    );
    assert_eq!(storage_slot(&f, store, "resmod:crystal"), 15);
    assert_eq!(creep_store(&f, hauler, "resmod:crystal"), 20);
}

/// 未声明的命名空间资源（不在计划目录中）与未知资源同样被结算拒绝：
/// 无库存变化、不产生掉落堆（该路径不可能有事件而无库存变化）。
#[test]
fn undeclared_namespaced_resources_are_rejected() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = resource_plane();
    // 夹具直接写入的未声明资源（真实世界经结算面无法产生，这里验证拒绝门）。
    let hauler = f.creep_with_mineral("hauler", 10, 10, &[BodyKind::Carry], 0, "resmod:ember", 30);
    let store = storage_with(&mut f, serde_json::json!({"resmod:crystal": 5}));
    f.settle_with_plane(
        action("p1", hauler, store, Action::Transfer, "resmod:ember", 20),
        plane.clone(),
    );
    assert_eq!(
        creep_store(&f, hauler, "resmod:ember"),
        30,
        "未声明资源不动"
    );
    assert_eq!(storage_slot(&f, store, "resmod:ember"), 0);
    // 掉落同样拒绝：不产生掉落堆。
    f.settle_with_plane(
        {
            let mut data = IntentData::bare();
            data.resource_type = Some("resmod:ember".into());
            data.amount = Some(20);
            let mut batch = IntentBatch::default();
            batch.add_object_intent("p1", hauler, Action::Drop, data);
            batch
        },
        plane,
    );
    assert_eq!(creep_store(&f, hauler, "resmod:ember"), 30);
    let piles = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter(|o| matches!(o, RoomObject::Energy(_)))
        .count();
    assert_eq!(piles, 0);
}

#[test]
fn pickup_rejects_resources_missing_from_the_current_catalog() {
    for (resource, with_mods) in [
        ("resmod:ember", true),
        ("unknownResource", true),
        ("resmod:crystal", false),
    ] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let hauler = f.creep("hauler", "p1", 10, 10, &[BodyKind::Carry], 0);
        // 快照/宿主可以携带目录外掉落物；pickup 必须在两端写入前拒绝。
        let pile = f
            .world
            .insert(
                "W1N1",
                RoomObject::Energy(kk_schema::room::EnergyObj {
                    id: 0,
                    x: 10,
                    y: 11,
                    resource_type: 0,
                    amount: 12,
                    resource: Some(resource.into()),
                }),
            )
            .unwrap();
        let mut batch = IntentBatch::default();
        batch.add_object_intent("p1", hauler, Action::Pickup, IntentData::target(pile));
        if with_mods {
            f.settle_with_plane(batch, resource_plane());
        } else {
            f.settle(batch);
        }
        assert_eq!(creep_store(&f, hauler, resource), 0, "{resource}");
        let RoomObject::Energy(pile) = f.obj(pile) else {
            panic!("拒绝拾取不得移除掉落堆");
        };
        // 正常对象阶段仍衰减 1；其余 11 不得被 pickup 转入库存。
        assert_eq!(pile.amount, 11, "{resource}");
    }
}

/// 声明资源的主动掉落进入命名掉落堆；拾取按既有 pickup 链路入槽。
#[test]
fn declared_resource_drop_creates_named_pile_and_pickup_restores() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = resource_plane();
    let hauler = f.creep_with_mineral("hauler", 10, 10, &[BodyKind::Carry], 0, "resmod:dust", 15);
    f.settle_with_plane(
        {
            let mut data = IntentData::bare();
            data.resource_type = Some("resmod:dust".into());
            data.amount = Some(15);
            let mut batch = IntentBatch::default();
            batch.add_object_intent("p1", hauler, Action::Drop, data);
            batch
        },
        plane.clone(),
    );
    assert_eq!(creep_store(&f, hauler, "resmod:dust"), 0);
    let pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e),
            _ => None,
        })
        .unwrap();
    assert_eq!(pile.resource.as_deref(), Some("resmod:dust"));
    assert_eq!(
        pile.amount, 14,
        "drop 当 tick 的对象阶段已衰减 ceil(15/1000)=1"
    );
    // 掉落堆上拾取：同格、非 power，按资源名入槽。
    f.settle_with_plane(
        {
            let data = IntentData::target(ObjId(pile.id));
            let mut batch = IntentBatch::default();
            batch.add_object_intent("p1", hauler, Action::Pickup, data);
            batch
        },
        plane,
    );
    // 掉落堆每 tick 衰减 ceil(15/ENERGY_DECAY)=1；拾取 tick 先衰减再拾取，
    // 与 vanilla 掉落物衰减语义一致。
    assert_eq!(
        creep_store(&f, hauler, "resmod:dust"),
        14,
        "拾取 14 后堆已清空"
    );
}

/// CARRY 缩容后的超容掉落按目录顺序处理声明资源（内建序在前）。
#[test]
fn overflow_drop_covers_declared_resources_after_builtin_order() {
    use kk_engine::settle::damage::drop_energy_over_capacity;
    // 直接构造 Working 走掉落入口（与 settle/resources/tests.rs 同法）。
    let room = kk_world::RoomState::new("W1N1", kk_core::Terrain::plains());
    let plane = resource_plane();
    let mut w = kk_engine::settle::Working::new(
        room,
        0,
        0,
        kk_engine::TickState::new(BTreeMap::new(), 100),
        plane,
    );
    let creep = kk_schema::room::CreepObj {
        id: 1,
        x: 10,
        y: 10,
        fatigue: 0,
        age_time: u64::MAX / 2,
        hits: 100,
        spawning: false,
        action_log: 0,
        name: "hauler".into(),
        owner: "p1".into(),
        // 单 CARRY 容量 50；energy 50 + crystal 20 + dust 10 超容 30。
        body: vec![kk_schema::room::BodyPart {
            kind: kk_world::types::BodyKind::Carry.code(),
            hits: 100,
            boost: None,
        }],
        store: kk_schema::room::Store {
            energy: 50,
            ghodium: 0,
        },
        store_extra: vec![
            kk_schema::room::ResourceStack {
                resource: "resmod:crystal".into(),
                amount: 20,
            },
            kk_schema::room::ResourceStack {
                resource: "resmod:dust".into(),
                amount: 10,
            },
        ],
        say_message: None,
        say_public: None,
        notify_when_attacked: None,
    };
    let id = ObjId(1);
    w.room.objects.insert(id, RoomObject::Creep(creep));
    w.index.insert(id, (10, 10));
    drop_energy_over_capacity(&mut w, id);
    let creep = match w.room.objects.get(&id).unwrap() {
        RoomObject::Creep(c) => c,
        _ => panic!(),
    };
    // 内建序在前：超容 30 全部由 energy 承担（持有 50），声明资源不动。
    assert_eq!(creep.store.energy, 20, "超容 30 先扣 energy");
    assert_eq!(
        kk_world::types::store_extra_slot(&creep.store, &creep.store_extra, "resmod:crystal"),
        20
    );
    assert_eq!(
        kk_world::types::store_extra_slot(&creep.store, &creep.store_extra, "resmod:dust"),
        10
    );
    let piles: Vec<(Option<&str>, u32)> = w
        .room
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) => Some((e.resource.as_deref(), e.amount)),
            _ => None,
        })
        .collect();
    assert_eq!(piles, vec![(Some("energy"), 30)]);

    // 内建资源耗尽后，超容由声明资源按目录序承担：energy=0 + crystal 40
    // + dust 20 超 cap 50 共 10，掉 crystal 10（crystal 先于 dust）。
    let mut w2 = w;
    if let Some(RoomObject::Creep(creep2)) = w2.room.objects.get_mut(&id) {
        creep2.store.energy = 0;
        for slot in &mut creep2.store_extra {
            if slot.resource == "resmod:crystal" {
                slot.amount = 40;
            }
            if slot.resource == "resmod:dust" {
                slot.amount = 20;
            }
        }
    }
    drop_energy_over_capacity(&mut w2, id);
    let creep2 = match w2.room.objects.get(&id).unwrap() {
        RoomObject::Creep(c) => c,
        _ => panic!(),
    };
    assert_eq!(creep2.store.energy, 0);
    assert_eq!(
        kk_world::types::store_extra_slot(&creep2.store, &creep2.store_extra, "resmod:crystal"),
        30,
        "energy 耗尽后按目录序扣 crystal"
    );
    assert_eq!(
        kk_world::types::store_extra_slot(&creep2.store, &creep2.store_extra, "resmod:dust"),
        20
    );
}

/// 市场订单与跨房发送对声明资源显式拒绝：订单不产生、send 请求不保存，
/// 全局阶段的 stowed send 也不转移（双门）。
#[test]
fn market_and_terminal_send_reject_declared_resources() {
    use kk_engine::intent::MarketIntent;
    let mut world = kk_world::World::new(7);
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    world
        .register_room("W5N1", kk_core::Terrain::plains())
        .unwrap();
    let mut engine = kk_engine::Engine::new(world);
    // 装配声明了资源的计划，证明拒绝是显式的目录边界而不是未知字符串。
    let mut contribution = kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "resmod".into(),
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
        resources: vec![kk_mod_api::resources::ResourceDescriptor {
            key: kk_mod_api::Key::new("resmod:crystal").unwrap(),
        }],
        buildings: Vec::new(),
    };
    contribution.resources.sort_by(|a, b| a.key.cmp(&b.key));
    let plan = std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    );
    engine
        .install_plan(plan, BTreeMap::new(), BTreeMap::new())
        .unwrap();
    assert!(
        engine.resources().contains("resmod:crystal"),
        "目录已含声明资源"
    );
    // 双房各一个 terminal，来源房带声明资源库存。
    for (room, store) in [
        (
            "W1N1",
            serde_json::json!({"resmod:crystal": 100, "energy": 100_000}),
        ),
        ("W5N1", serde_json::json!({"energy": 100_000})),
    ] {
        let data = serde_json::json!({
            "type": "structure", "structureType": "terminal", "owner": "p1",
            "hits": 3000, "hitsMax": 3000,
            "store": store, "storeCapacity": 300000,
        });
        engine
            .insert_object(
                room,
                RoomObject::Generic(kk_schema::room::GenericObj {
                    id: 0,
                    x: 25,
                    y: 25,
                    data: data.to_string().into(),
                }),
            )
            .unwrap();
    }
    // 房间相：send 请求不保存（terminal_send 只认内建资源）。
    let terminals: Vec<ObjId> = ["W1N1", "W5N1"]
        .iter()
        .flat_map(|room| engine.world().room(room).unwrap().objects.keys().copied())
        .collect();
    let from = terminals[0];
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        from,
        Action::Send,
        IntentData {
            target_room_name: Some("W5N1".into()),
            resource_type: Some("resmod:crystal".into()),
            amount: Some(50),
            description: None,
            ..IntentData::bare()
        },
    );
    batch
        .users
        .entry("p1".into())
        .or_default()
        .market
        .push(MarketIntent::CreateOrder {
            order_type: Some("sell".into()),
            resource_type: Some("resmod:crystal".into()),
            price_milli: Some(1000),
            total_amount: Some(100),
            room_name: Some("W1N1".into()),
        });
    engine
        .tick_with_intents_with_input(
            batch,
            kk_engine::TickInput {
                now_ms: 1_700_000_000_000,
                warmup: false,
            },
        )
        .unwrap();
    assert!(
        engine
            .market()
            .orders
            .values()
            .all(|o| o.resource_type != "resmod:crystal"),
        "声明资源不产生市场订单"
    );
    let data: Value = match engine.world().get(from).map(|(_, o)| o.clone()) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        _ => panic!(),
    };
    assert_eq!(data["store"]["resmod:crystal"], 100, "库存不动");
    // 全局相双门：直接 stow 一条 send（绕过房间相）也不转移。
    engine
        .edit_object(from, |object| {
            if let Some(RoomObject::Generic(g)) = Some(object) {
                let mut v: Value = serde_json::from_str(&g.data).unwrap();
                v["send"] = serde_json::json!({
                    "targetRoomName": "W5N1", "resourceType": "resmod:crystal", "amount": 50,
                });
                g.data = v.to_string().into();
            }
            Ok(())
        })
        .unwrap();
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    let data: Value = match engine.world().get(from).map(|(_, o)| o.clone()) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        _ => panic!(),
    };
    assert_eq!(data["store"]["resmod:crystal"], 100, "stowed send 同样拒绝");
}
