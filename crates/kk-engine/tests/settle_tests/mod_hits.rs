//! G09.b 自有对象耐久：hits 声明类型的 Mod 对象可作为 creep attack
//! 的目标（立即扣减、死亡移除+事件）；缺耐久信封的对象结算静默不动。
use super::*;

/// 声明 hmod:block（耐久 300，位置字段 x/y）的贡献平面；缺信封对照
/// 用同类型直接构造（发布前校验外的结算门行为）。
fn hits_plane() -> kk_engine::settle::Plane {
    let field = |name: &str| kk_mod_api::FieldDescriptor {
        key: kk_mod_api::Key::new(format!("hmod:{name}")).unwrap(),
        schema: kk_mod_api::ValueSchema {
            kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned {
                min: 0,
                max: 49,
            }),
            nullable: false,
        },
        default: kk_mod_api::FieldDefault::Missing,
        visibility: kk_mod_api::Visibility::Public,
        binding: kk_mod_api::Key::new(format!("hmod:block.field.{name}")).unwrap(),
    };
    let contribution = kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "hmod".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [11; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::from([
            kk_mod_api::Key::new("hmod:block.field.x").unwrap(),
            kk_mod_api::Key::new("hmod:block.field.y").unwrap(),
            kk_mod_api::Key::new("hmod:marker.field.x").unwrap(),
            kk_mod_api::Key::new("hmod:marker.field.y").unwrap(),
        ]),
        field_mounts: Vec::new(),
        object_types: vec![
            kk_mod_api::ObjectTypeDescriptor {
                key: kk_mod_api::Key::new("hmod:block").unwrap(),
                schema_version: 1,
                owner_field: None,
                position_fields: Some([
                    kk_mod_api::Key::new("hmod:x").unwrap(),
                    kk_mod_api::Key::new("hmod:y").unwrap(),
                ]),
                fields: vec![field("x"), field("y")],
                capabilities: Vec::new(),
                lifecycle_slots: BTreeSet::new(),
                visibility: kk_mod_api::ObjectVisibility::RoomVisible,
                player_class: "Block".into(),
                store_capacity: None,
                hits_max: Some(300),
            },
            marker_descriptor(),
        ],
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
    };
    let plan = std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    );
    kk_engine::settle::Plane {
        buildings: Default::default(),
        links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
        stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
        combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
        move_gate: Default::default(),
        quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
        terrain_passability: Default::default(),
        resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
        contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan,
    }
}

/// 计划内无耐久类型：与 block 同形字段、无 hitsMax 声明。
fn marker_descriptor() -> kk_mod_api::ObjectTypeDescriptor {
    let field = |name: &str| kk_mod_api::FieldDescriptor {
        key: kk_mod_api::Key::new(format!("hmod:{name}")).unwrap(),
        schema: kk_mod_api::ValueSchema {
            kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned {
                min: 0,
                max: 49,
            }),
            nullable: false,
        },
        default: kk_mod_api::FieldDefault::Missing,
        visibility: kk_mod_api::Visibility::Public,
        binding: kk_mod_api::Key::new(format!("hmod:marker.field.{name}")).unwrap(),
    };
    kk_mod_api::ObjectTypeDescriptor {
        key: kk_mod_api::Key::new("hmod:marker").unwrap(),
        schema_version: 1,
        owner_field: None,
        position_fields: Some([
            kk_mod_api::Key::new("hmod:x").unwrap(),
            kk_mod_api::Key::new("hmod:y").unwrap(),
        ]),
        fields: vec![field("x"), field("y")],
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: kk_mod_api::ObjectVisibility::RoomVisible,
        player_class: "Marker".into(),
        store_capacity: None,
        hits_max: None,
    }
}

fn block(f: &mut Fixture, x: u8, y: u8, type_key: &str, extra: Value) -> ObjId {
    let mut payload = serde_json::json!({"x": x, "y": y});
    if let (Some(map), Some(extra)) = (payload.as_object_mut(), extra.as_object()) {
        for (k, v) in extra {
            map.insert(k.clone(), v.clone());
        }
    }
    f.world
        .insert(
            "W1N1",
            RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x,
                y,
                type_key: type_key.into(),
                data: payload.to_string(),
            }),
        )
        .unwrap()
}

fn block_data(f: &Fixture, id: ObjId) -> Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Mod(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("Mod 对象不存在：{other:?}"),
    }
}

/// settle_with_plane 丢弃事件；耐久域需要事件断言，这里走同一结算入口
/// 并保留事件返回。
fn settle_events(
    f: &mut Fixture,
    batch: IntentBatch,
    plane: kk_engine::settle::Plane,
) -> Vec<Value> {
    let state = kk_engine::TickState::new(std::mem::take(&mut f.players), f.world.next_obj_id());
    let room = f.world.room("W1N1").unwrap().clone();
    let (room, events, _moves) = settle_room(
        room,
        f.world.time(),
        1_700_000_000_000,
        &state,
        &batch,
        &plane,
    )
    .unwrap();
    let (players, next, _) = state.into_parts().unwrap();
    f.players = players;
    let name = room.name.clone();
    f.world.replace_room(name, room).unwrap();
    f.world.bump_next_obj_id(next);
    f.world.advance_ticks(1).unwrap();
    events
}

#[test]
fn attack_decrements_mod_hits_and_death_removes_with_type_key_event() {
    let mut f = Fixture::new(9, terrain_with(&[]));
    let block = block(
        &mut f,
        10,
        11,
        "hmod:block",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let attacker = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    // 单 ATTACK 部件每次近战 30 伤害。
    let events = settle_events(&mut f, attack_intent(attacker, block), hits_plane());
    assert_eq!(
        block_data(&f, block)["hits"],
        serde_json::json!(270),
        "耐久立即扣减（不经受击槽延迟）"
    );
    assert!(
        events
            .iter()
            .any(|e| e["event"] == serde_json::json!(kk_core::constants::EVENT_ATTACK)),
        "攻击事件在场"
    );
    assert!(
        events
            .iter()
            .all(|e| e["event"] != serde_json::json!(kk_core::constants::EVENT_OBJECT_DESTROYED)),
        "未击毁不发销毁事件"
    );
    // 连击至第 10 次击毁：对象移除，事件 type 为类型键（非空串）。
    for _ in 0..8 {
        settle_events(&mut f, attack_intent(attacker, block), hits_plane());
    }
    let events = settle_events(&mut f, attack_intent(attacker, block), hits_plane());
    assert!(f.world.get(block).is_none(), "扣至 0 即移除");
    let destroyed = events
        .iter()
        .find(|e| e["event"] == serde_json::json!(kk_core::constants::EVENT_OBJECT_DESTROYED))
        .expect("击毁事件在场");
    assert_eq!(destroyed["data"]["type"], serde_json::json!("hmod:block"));
    // 死亡不产废墟、不掉落（Mod 对象不经工地成本模型，store 信封随对象
    // 消失）——误走 destroy_structure 会插入 Ruin。
    let room = f.world.room("W1N1").unwrap();
    assert!(
        room.objects
            .values()
            .all(|o| !matches!(o, RoomObject::Ruin(_) | RoomObject::Energy(_))),
        "不产废墟与地面掉落"
    );
}

#[test]
fn no_durability_object_attack_is_silently_ignored() {
    let mut f = Fixture::new(10, terrain_with(&[]));
    let attacker = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    // 计划内无耐久类型（hmod:marker）：结算静默不动——object_hits=0 门。
    let marker = block(&mut f, 10, 11, "hmod:marker", serde_json::json!({}));
    let events = settle_events(&mut f, attack_intent(attacker, marker), hits_plane());
    assert!(
        events
            .iter()
            .all(|e| e["event"] != serde_json::json!(kk_core::constants::EVENT_OBJECT_DESTROYED)),
        "无耐久对象不可击毁"
    );
    assert!(f.world.get(marker).is_some(), "对象保持原样");
    // 纵深防御臂：声明耐久却缺信封（快照篡改态，真实管线被发布前校验
    // 拒绝）同样落 object_hits=0 门。
    let bare = block(&mut f, 10, 12, "hmod:block", serde_json::json!({}));
    let events = settle_events(&mut f, attack_intent(attacker, bare), hits_plane());
    assert!(
        events
            .iter()
            .all(|e| e["event"] != serde_json::json!(kk_core::constants::EVENT_OBJECT_DESTROYED)),
        "缺信封对象不可击毁"
    );
    assert!(f.world.get(bare).is_some());
}

/// 远程攻击共用同一 object_hits 门（RANGED_ATTACK_POWER=10/击）。
#[test]
fn ranged_attack_decrements_mod_hits() {
    let mut f = Fixture::new(11, terrain_with(&[]));
    let block = block(
        &mut f,
        12,
        12,
        "hmod:block",
        serde_json::json!({"hits": 200, "hitsMax": 200}),
    );
    let shooter = f.creep(
        "bow",
        "p1",
        10,
        10,
        &[BodyKind::RangedAttack, BodyKind::Move],
        0,
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        shooter,
        Action::RangedAttack,
        IntentData::target(block),
    );
    settle_events(&mut f, batch, hits_plane());
    assert_eq!(
        block_data(&f, block)["hits"],
        serde_json::json!(190),
        "远程每击 10"
    );
}

/// 塔攻击（Generic 攻击者）经同一 object_hits 门命中耐久对象。
#[test]
fn tower_attack_hits_mod_durability_target() {
    let mut f = Fixture::new(12, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let tower = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "tower", "owner": "p1",
                    "hits": 3000, "hitsMax": 3000, "nextDecayTime": 1_000_000,
                    "store": {"energy": 100}, "storeCapacity": 1000,
                    "storeCapacityResource": {"energy": 1000},
                })
                .to_string(),
            }),
        )
        .unwrap();
    let block = block(
        &mut f,
        12,
        12,
        "hmod:block",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let events = settle_events(&mut f, attack_intent(tower, block), hits_plane());
    // 塔距 2 全功率 600 → 300 一次性击毁。
    assert!(f.world.get(block).is_none(), "塔伤害即死");
    assert!(
        events.iter().any(|e| e["event"]
            == serde_json::json!(kk_core::constants::EVENT_OBJECT_DESTROYED)
            && e["data"]["type"] == serde_json::json!("hmod:block")),
        "击毁事件 type 为类型键"
    );
}
