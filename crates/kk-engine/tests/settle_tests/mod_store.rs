//! G09.a 自有对象库存：store 声明类型的 Mod 对象作为 creep
//! transfer/withdraw 的接收/提取端点；无信封类型保持不可转移。
use super::*;

/// 声明 cmod:crate（store 容量 500，位置字段 x/y）的贡献平面。
fn store_plane() -> kk_engine::settle::Plane {
    let field = |name: &str| kk_mod_api::FieldDescriptor {
        key: kk_mod_api::Key::new(format!("cmod:{name}")).unwrap(),
        schema: kk_mod_api::ValueSchema {
            kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned {
                min: 0,
                max: 49,
            }),
            nullable: false,
        },
        default: kk_mod_api::FieldDefault::Missing,
        visibility: kk_mod_api::Visibility::Public,
        binding: kk_mod_api::Key::new(format!("cmod:crate.field.{name}")).unwrap(),
    };
    let contribution = kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "cmod".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::from([
            kk_mod_api::Key::new("cmod:crate.field.x").unwrap(),
            kk_mod_api::Key::new("cmod:crate.field.y").unwrap(),
        ]),
        field_mounts: Vec::new(),
        object_types: vec![kk_mod_api::ObjectTypeDescriptor {
            key: kk_mod_api::Key::new("cmod:crate").unwrap(),
            schema_version: 1,
            owner_field: None,
            position_fields: Some([
                kk_mod_api::Key::new("cmod:x").unwrap(),
                kk_mod_api::Key::new("cmod:y").unwrap(),
            ]),
            fields: vec![field("x"), field("y")],
            capabilities: Vec::new(),
            lifecycle_slots: BTreeSet::new(),
            visibility: kk_mod_api::ObjectVisibility::RoomVisible,
            player_class: "Crate".into(),
            store_capacity: Some(500),
            hits_max: None,
            retaliation_milli: None,
        }],
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
        static_passability: Default::default(),
        resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
        contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan,
    }
}

fn carrier(f: &mut Fixture, x: u8, y: u8, store: Value, capacity: i64) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x,
                y,
                type_key: "cmod:crate".into(),
                data:
                    serde_json::json!({"x": x, "y": y, "store": store, "storeCapacity": capacity})
                        .to_string(),
            }),
        )
        .unwrap()
}

fn carrier_data(f: &Fixture, id: ObjId) -> Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Mod(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("Mod 对象不存在：{other:?}"),
    }
}

fn transfer_intent(actor: ObjId, target: ObjId, resource: &str, amount: i64) -> IntentBatch {
    let mut data = IntentData::target(target);
    data.resource_type = Some(resource.into());
    data.amount = Some(amount);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", actor, Action::Transfer, data);
    batch
}

fn withdraw_intent(actor: ObjId, target: ObjId, resource: &str, amount: i64) -> IntentBatch {
    let mut data = IntentData::target(target);
    data.resource_type = Some(resource.into());
    data.amount = Some(amount);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", actor, Action::Withdraw, data);
    batch
}

#[test]
fn creep_transfers_energy_into_mod_carrier_up_to_capacity() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    let carrier = carrier(&mut f, 10, 11, serde_json::json!({"energy": 0}), 500);
    let creep = f.creep(
        "mule",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Carry],
        100,
    );
    f.settle_with_plane(transfer_intent(creep, carrier, "energy", 80), store_plane());
    assert_eq!(
        carrier_data(&f, carrier)["store"]["energy"],
        serde_json::json!(80),
        "creep 转入 Mod 载具"
    );
    assert_eq!(f.creep_state(creep).3, 20, "执行者扣除");
    // 持有不足整条拒绝（官方语义），载具与执行者都不动。
    f.settle_with_plane(
        transfer_intent(creep, carrier, "energy", 100),
        store_plane(),
    );
    assert_eq!(
        carrier_data(&f, carrier)["store"]["energy"],
        serde_json::json!(80)
    );
    assert_eq!(f.creep_state(creep).3, 20);
    // 补足后容量门生效：请求 450 只转剩余余量 420。
    f.world
        .edit_object(creep, |object| {
            if let Some(RoomObject::Creep(c)) = Some(object) {
                c.store.energy = 500;
            }
            Ok(())
        })
        .unwrap();
    f.settle_with_plane(
        transfer_intent(creep, carrier, "energy", 450),
        store_plane(),
    );
    assert_eq!(
        carrier_data(&f, carrier)["store"]["energy"],
        serde_json::json!(500),
        "容量 500 收满"
    );
    assert_eq!(f.creep_state(creep).3, 80, "只扣实际转入量 420（500−420）");
    // 满载后整条不转。
    f.settle_with_plane(transfer_intent(creep, carrier, "energy", 60), store_plane());
    assert_eq!(
        carrier_data(&f, carrier)["store"]["energy"],
        serde_json::json!(500)
    );
    assert_eq!(f.creep_state(creep).3, 80);
}

#[test]
fn creep_withdraws_energy_from_mod_carrier() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    let carrier = carrier(&mut f, 10, 11, serde_json::json!({"energy": 300}), 500);
    // 双 CARRY 容量 100：请求 120 按携带余量钳为 100。
    let creep = f.creep(
        "mule",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Carry, BodyKind::Carry],
        0,
    );
    f.settle_with_plane(
        withdraw_intent(creep, carrier, "energy", 120),
        store_plane(),
    );
    assert_eq!(
        carrier_data(&f, carrier)["store"]["energy"],
        serde_json::json!(200)
    );
    assert_eq!(f.creep_state(creep).3, 100, "提取者按携带余量入账");
}

#[test]
fn mod_object_without_envelope_is_ignored_by_transfer_and_withdraw() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    let bare = f
        .world
        .insert(
            "W1N1",
            RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x: 10,
                y: 11,
                type_key: "cmod:crate".into(),
                data: serde_json::json!({"x": 10, "y": 11}).to_string(),
            }),
        )
        .unwrap();
    let creep = f.creep(
        "mule",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Carry],
        100,
    );
    f.settle_with_plane(transfer_intent(creep, bare, "energy", 50), store_plane());
    assert_eq!(f.creep_state(creep).3, 100, "无信封目标整条忽略");
    // 反向：无信封源没有可提取库存。
    f.settle_with_plane(withdraw_intent(creep, bare, "energy", 10), store_plane());
    assert_eq!(f.creep_state(creep).3, 100, "无信封源提取为零");
}

#[test]
fn distance_gate_applies_to_mod_carrier() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    let carrier = carrier(&mut f, 30, 30, serde_json::json!({"energy": 0}), 500);
    let creep = f.creep(
        "mule",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Carry],
        100,
    );
    f.settle_with_plane(transfer_intent(creep, carrier, "energy", 50), store_plane());
    assert_eq!(
        carrier_data(&f, carrier)["store"]["energy"],
        serde_json::json!(0),
        "不相邻不转"
    );
    assert_eq!(f.creep_state(creep).3, 100);
}
