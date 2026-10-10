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
            kk_mod_api::Key::new("hmod:spike.field.x").unwrap(),
            kk_mod_api::Key::new("hmod:spike.field.y").unwrap(),
            kk_mod_api::Key::new("hmod:mirror.field.x").unwrap(),
            kk_mod_api::Key::new("hmod:mirror.field.y").unwrap(),
            kk_mod_api::Key::new("hmod:dust.field.x").unwrap(),
            kk_mod_api::Key::new("hmod:dust.field.y").unwrap(),
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
                retaliation_milli: None,
            },
            marker_descriptor(),
            spike_descriptor(),
            mirror_descriptor(),
            dust_descriptor(),
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
        retaliation_milli: None,
    }
}

/// 反击型（G09.d）：耐久 300 + 反击比率 0.5（官方 powerBank 同构）。
fn spike_descriptor() -> kk_mod_api::ObjectTypeDescriptor {
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
        binding: kk_mod_api::Key::new(format!("hmod:spike.field.{name}")).unwrap(),
    };
    kk_mod_api::ObjectTypeDescriptor {
        key: kk_mod_api::Key::new("hmod:spike").unwrap(),
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
        player_class: "Spike".into(),
        store_capacity: None,
        hits_max: Some(300),
        retaliation_milli: Some(500),
    }
}

/// 全额反击型（G09.d）：耐久 300 + 反击比率 1.0。
fn mirror_descriptor() -> kk_mod_api::ObjectTypeDescriptor {
    let mut d = spike_descriptor();
    d.key = kk_mod_api::Key::new("hmod:mirror").unwrap();
    d.player_class = "Mirror".into();
    d.retaliation_milli = Some(1000);
    d.fields[0].binding = kk_mod_api::Key::new("hmod:mirror.field.x").unwrap();
    d.fields[1].binding = kk_mod_api::Key::new("hmod:mirror.field.y").unwrap();
    d
}

/// 极小比率型（G09.d）：0.001——近战 30×0.001 整除得 0，不触发。
fn dust_descriptor() -> kk_mod_api::ObjectTypeDescriptor {
    let mut d = spike_descriptor();
    d.key = kk_mod_api::Key::new("hmod:dust").unwrap();
    d.player_class = "Dust".into();
    d.retaliation_milli = Some(1);
    d.fields[0].binding = kk_mod_api::Key::new("hmod:dust.field.x").unwrap();
    d.fields[1].binding = kk_mod_api::Key::new("hmod:dust.field.y").unwrap();
    d
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
    let kk_engine::settle::SettledRoom { room, events, .. } = settle_room(
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
                .to_string()
                .into(),
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

/// 声明式反击（G09.d）：creep 攻击者走延迟队列（同轮 tick 消费），
/// HIT_BACK 事件先于主攻击事件。
#[test]
fn declared_retaliation_counters_creep_attacker_via_delayed_queue() {
    let mut f = Fixture::new(13, terrain_with(&[]));
    let spike = block(
        &mut f,
        10,
        11,
        "hmod:spike",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let attacker = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    let events = settle_events(&mut f, attack_intent(attacker, spike), hits_plane());
    // 目标照常扣 30；攻击者两部件 200 血，同轮 tick 吃到 30×0.5=15。
    assert_eq!(block_data(&f, spike)["hits"], serde_json::json!(270));
    match f.obj(attacker) {
        RoomObject::Creep(c) => assert_eq!(c.hits, 185, "反击经延迟队列同轮落账"),
        other => panic!("攻击者存活：{other:?}"),
    }
    let hit_back = events
        .iter()
        .position(|e| {
            e["event"] == serde_json::json!(kk_core::constants::EVENT_ATTACK)
                && e["data"]["attackType"]
                    == serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK)
        })
        .expect("HIT_BACK 事件在场");
    let main = events
        .iter()
        .position(|e| {
            e["event"] == serde_json::json!(kk_core::constants::EVENT_ATTACK)
                && e["data"]["attackType"]
                    == serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_MELEE)
        })
        .expect("主攻击事件在场");
    assert!(hit_back < main, "反击事件先于主攻击事件");
    assert_eq!(events[hit_back]["data"]["damage"], serde_json::json!(15));
}

/// 远程攻击同样触发（powerBank 语义：任意 attackType）。
#[test]
fn declared_retaliation_counters_ranged_attacker() {
    let mut f = Fixture::new(14, terrain_with(&[]));
    let spike = block(
        &mut f,
        12,
        12,
        "hmod:spike",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
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
        IntentData::target(spike),
    );
    settle_events(&mut f, batch, hits_plane());
    assert_eq!(
        block_data(&f, spike)["hits"],
        serde_json::json!(290),
        "远程每击 10"
    );
    match f.obj(shooter) {
        RoomObject::Creep(c) => assert_eq!(c.hits, 195, "反击 10×0.5=5"),
        other => panic!("攻击者存活：{other:?}"),
    }
}

/// 塔攻击者的反击立即扣减（队列不被消费）；击杀一击仍反击（powerBank
/// 语义），事件序摧毁→反击→主攻击。
#[test]
fn declared_retaliation_counters_tower_attacker_immediately() {
    let mut f = Fixture::new(15, terrain_with(&[]));
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
                .to_string()
                .into(),
            }),
        )
        .unwrap();
    let spike = block(
        &mut f,
        12,
        12,
        "hmod:spike",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let events = settle_events(&mut f, attack_intent(tower, spike), hits_plane());
    // 塔距 2 全功率 600 击毁 spike；反击 600×0.5=300 立即落塔（3000→2700）。
    assert!(f.world.get(spike).is_none(), "spike 被击毁");
    match f.world.get(tower).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => {
            let data: Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], serde_json::json!(2700), "塔立即吃到反击");
        }
        other => panic!("塔存活：{other:?}"),
    }
    let destroy = events
        .iter()
        .position(|e| e["event"] == serde_json::json!(kk_core::constants::EVENT_OBJECT_DESTROYED))
        .expect("击毁事件");
    let hit_back = events
        .iter()
        .position(|e| {
            e["data"]["attackType"]
                == serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK)
        })
        .expect("反击事件");
    let main = events
        .iter()
        .position(|e| {
            e["data"]["attackType"]
                == serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_RANGED)
        })
        .expect("主攻击事件");
    assert!(destroy < hit_back && hit_back < main, "摧毁→反击→主攻击");
    assert_eq!(events[hit_back]["data"]["damage"], serde_json::json!(300));
}

/// 无反击声明类型不反伤（对照）。
#[test]
fn undeclared_types_do_not_retaliate() {
    let mut f = Fixture::new(16, terrain_with(&[]));
    let plain = block(
        &mut f,
        10,
        11,
        "hmod:block",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let attacker = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    let events = settle_events(&mut f, attack_intent(attacker, plain), hits_plane());
    match f.obj(attacker) {
        RoomObject::Creep(c) => assert_eq!(c.hits, 200, "无声明不反伤"),
        other => panic!("攻击者存活：{other:?}"),
    }
    assert!(
        events.iter().all(|e| e["data"]["attackType"]
            != serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK)),
        "无 HIT_BACK 事件"
    );
}

/// 房主安全模式抑制声明式反击（controller 主人==攻击者主人且激活时清零，
/// 与上游 _damage.js 的 powerBank 分支同款）。
#[test]
fn safe_mode_of_attacker_owner_suppresses_declared_retaliation() {
    let mut f = Fixture::new(17, terrain_with(&[]));
    f.controller_full(3, Some("p1"), |ctl| ctl.safe_mode = 1_000_000);
    let spike = block(
        &mut f,
        10,
        11,
        "hmod:spike",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let attacker = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    let events = settle_events(&mut f, attack_intent(attacker, spike), hits_plane());
    assert_eq!(
        block_data(&f, spike)["hits"],
        serde_json::json!(270),
        "目标照常扣减"
    );
    match f.obj(attacker) {
        RoomObject::Creep(c) => assert_eq!(c.hits, 200, "安全模式抑制声明反击"),
        other => panic!("攻击者存活：{other:?}"),
    }
    assert!(
        events.iter().all(|e| e["data"]["attackType"]
            != serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK)),
        "无 HIT_BACK 事件"
    );
}

/// 全额反击的消耗战语义：反伤沿延迟队列按部件扣血（先耗攻击部件），
/// 部件 hits 归零后 calc_body_effectiveness 的 count_active 口径使输出
/// 归零——单挑结构性停滞（攻击者存活但打不动），攻击者死亡须经由
/// 其他伤害来源。此处钉住停滞形态本身。
#[test]
fn full_retaliation_decays_attacker_parts_and_stalls_duel() {
    let mut f = Fixture::new(18, terrain_with(&[]));
    let mirror = block(
        &mut f,
        10,
        11,
        "hmod:mirror",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let attacker = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    // 30↔30 四轮后攻击部件（100 血）耗尽：输出归零，双方读值冻结。
    for _ in 0..4 {
        settle_events(&mut f, attack_intent(attacker, mirror), hits_plane());
    }
    assert_eq!(block_data(&f, mirror)["hits"], serde_json::json!(180));
    match f.obj(attacker) {
        RoomObject::Creep(c) => assert_eq!(c.hits, 80, "攻击部件耗尽、残余部件存活"),
        other => panic!("攻击者存活：{other:?}"),
    }
    // 追加轮不再产生任何伤害（部件级输出归零，非 creep 死亡）。
    for _ in 0..3 {
        settle_events(&mut f, attack_intent(attacker, mirror), hits_plane());
    }
    assert_eq!(
        block_data(&f, mirror)["hits"],
        serde_json::json!(180),
        "决斗停滞"
    );
    match f.obj(attacker) {
        RoomObject::Creep(c) => assert_eq!(c.hits, 80),
        other => panic!("攻击者存活：{other:?}"),
    }
}

/// 塔被反击击毁：反击量 ≥ 塔剩余耐久时走结构销毁（废墟/事件），
/// 与 creep 攻击者的分流互补。
#[test]
fn retaliation_destroys_tower_attacker() {
    let mut f = Fixture::new(19, terrain_with(&[]));
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
                    "hits": 150, "hitsMax": 3000, "nextDecayTime": 1_000_000,
                    "store": {"energy": 100}, "storeCapacity": 1000,
                    "storeCapacityResource": {"energy": 1000},
                })
                .to_string()
                .into(),
            }),
        )
        .unwrap();
    let spike = block(
        &mut f,
        12,
        12,
        "hmod:spike",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let events = settle_events(&mut f, attack_intent(tower, spike), hits_plane());
    // 塔满功率 600 击毁 spike；反击 300 ≥ 塔剩余 150 → 塔亦毁（结构销毁）。
    assert!(f.world.get(spike).is_none(), "spike 被击毁");
    assert!(f.world.get(tower).is_none(), "塔被反击击毁");
    let destroys: Vec<_> = events
        .iter()
        .filter(|e| e["event"] == serde_json::json!(kk_core::constants::EVENT_OBJECT_DESTROYED))
        .collect();
    assert_eq!(destroys.len(), 2, "双方击毁事件各一：{events:?}");
}

/// 多攻击者同轮各吃反击（按攻击者分键累计）。
#[test]
fn multiple_attackers_each_take_their_own_retaliation() {
    let mut f = Fixture::new(20, terrain_with(&[]));
    let spike = block(
        &mut f,
        10,
        10,
        "hmod:spike",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let a1 = f.creep("a1", "p1", 10, 11, &[BodyKind::Attack, BodyKind::Move], 0);
    let a2 = f.creep("a2", "p1", 11, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a1, Action::Attack, IntentData::target(spike));
    batch.add_object_intent("p1", a2, Action::Attack, IntentData::target(spike));
    settle_events(&mut f, batch, hits_plane());
    for id in [a1, a2] {
        match f.obj(id) {
            RoomObject::Creep(c) => assert_eq!(c.hits, 185, "各自独立吃 15 反击"),
            other => panic!("攻击者存活：{other:?}"),
        }
    }
    assert_eq!(
        block_data(&f, spike)["hits"],
        serde_json::json!(240),
        "目标吃两击"
    );
}

/// 极小比率的整除截断：30×0.001=0.03 → 0，不触发不反击。
#[test]
fn fractional_retaliation_below_one_does_not_trigger() {
    let mut f = Fixture::new(21, terrain_with(&[]));
    let dust = block(
        &mut f,
        10,
        11,
        "hmod:dust",
        serde_json::json!({"hits": 300, "hitsMax": 300}),
    );
    let attacker = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Move], 0);
    let events = settle_events(&mut f, attack_intent(attacker, dust), hits_plane());
    assert_eq!(
        block_data(&f, dust)["hits"],
        serde_json::json!(270),
        "目标照常扣减"
    );
    match f.obj(attacker) {
        RoomObject::Creep(c) => assert_eq!(c.hits, 200, "整除截断为 0 不反击"),
        other => panic!("攻击者存活：{other:?}"),
    }
    assert!(events.iter().all(|e| e["data"]["attackType"]
        != serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK)));
}
