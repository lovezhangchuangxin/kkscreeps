use super::*;

#[test]
fn signed_zero_is_not_collapsed_in_player_field_fingerprint() {
    let mut c = builtin::link();
    let field = c.object_types[0]
        .fields
        .iter_mut()
        .find(|f| f.key == key("builtin:store"))
        .unwrap();
    field.schema = ValueSchema {
        kind: ValueKind::Number(NumberDomain::Float {
            min: -1.0,
            max: 1.0,
        }),
        nullable: false,
    };
    field.default = FieldDefault::Value(json!(-0.0));
    let negative = plan(vec![c.clone()]).unwrap();
    c.object_types[0]
        .fields
        .iter_mut()
        .find(|f| f.key == key("builtin:store"))
        .unwrap()
        .default = FieldDefault::Value(json!(0.0));
    assert_ne!(negative.hash(), plan(vec![c]).unwrap().hash());
}

#[test]
fn multiple_type_catalogs_share_field_names_without_sharing_field_identity() {
    let mut c = module("counter");
    let binding = key("counter:fields");
    c.bindings.insert(binding.clone());
    let make_type = |name: &str| ObjectTypeDescriptor {
        key: key(name),
        schema_version: 1,
        owner_field: None,
        position_fields: None,
        fields: vec![FieldDescriptor {
            key: key("counter:value"),
            schema: ValueSchema {
                kind: ValueKind::Bool,
                nullable: false,
            },
            default: FieldDefault::Value(json!(true)),
            visibility: Visibility::Public,
            binding: binding.clone(),
        }],
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Counter".into(),
    };
    c.object_types = vec![make_type("counter:z"), make_type("counter:a")];
    let original = plan(vec![c.clone(), builtin::link()]).unwrap();
    c.object_types.reverse();
    let reordered = plan(vec![builtin::link(), c]).unwrap();
    assert_eq!(original.hash(), reordered.hash());
    assert_eq!(original.projection(), reordered.projection());
    let names: Vec<_> = original
        .projection()
        .types()
        .iter()
        .map(|t| t.descriptor.key.as_str())
        .collect();
    assert_eq!(names, vec!["builtin:link", "counter:a", "counter:z"]);
    let a = original.projection().resolve(&key("counter:a")).unwrap();
    let z = original.projection().resolve(&key("counter:z")).unwrap();
    assert_ne!(a, z);
    let field = original
        .projection()
        .field(a, &key("counter:value"))
        .unwrap();
    assert!(original.projection().get_field(a, field).is_some());
    assert!(
        original.projection().get_field(z, field).is_none(),
        "不能把 a 类型的字段句柄当成 z 类型的字段"
    );
    assert!(
        original
            .projection()
            .field(a, &key("counter:value"))
            .is_some()
    );
    assert!(
        original
            .projection()
            .field(z, &key("counter:value"))
            .is_some()
    );
}

#[test]
fn builtin_link_values_match_constant_dump_and_use_native_slots() {
    let plan = plan(vec![builtin::link()]).unwrap();
    let constants: serde_json::Value =
        serde_json::from_str(include_str!("../../../kk-runner/assets/constants.json")).unwrap();
    for (rule, constant) in [
        ("hits", "LINK_HITS"),
        ("hitsMax", "LINK_HITS_MAX"),
        ("capacity", "LINK_CAPACITY"),
        ("cooldown", "LINK_COOLDOWN"),
    ] {
        let handle = plan
            .rules()
            .resolve(&key(&format!("builtin:link.{rule}")))
            .unwrap();
        assert_eq!(
            plan.rules().get(handle),
            Some(&Number::Signed(constants[constant].as_i64().unwrap()))
        );
    }
    let get = |name: &str| {
        plan.rules()
            .get(plan.rules().resolve(&key(name)).unwrap())
            .unwrap()
    };
    assert_eq!(
        get("builtin:link.lossRatio"),
        &Number::Float(constants["LINK_LOSS_RATIO"].as_f64().unwrap())
    );
    assert_eq!(
        get("builtin:link.buildCost"),
        &Number::Signed(constants["CONSTRUCTION_COST"]["link"].as_i64().unwrap())
    );
    for level in 0..=8 {
        assert_eq!(
            get(&format!("builtin:link.rcl{level}")),
            &Number::Signed(
                constants["CONTROLLER_STRUCTURES"]["link"][level.to_string()]
                    .as_i64()
                    .unwrap_or(0)
            )
        );
    }
    assert_eq!(plan.rules().len(), 25);
    assert_eq!(plan.slots().len(), 16);
    for name in builtin::RESOURCE_ACTION_SLOTS
        .iter()
        .chain(builtin::MOVE_ACTION_SLOTS.iter())
        .chain(builtin::COMBAT_ACTION_SLOTS.iter())
        .chain(builtin::BUILD_ACTION_SLOTS.iter())
    {
        let slot = plan
            .get_slot(plan.resolve_slot(&key(name)).unwrap())
            .unwrap();
        assert_eq!(slot.stage, Stage::ObjectAction);
        assert_eq!(
            slot.dispatch,
            Dispatch::Native(key(&format!("{name}.native")))
        );
    }
    let slot = plan.resolve_slot(&key("builtin:link.transfer")).unwrap();
    assert_eq!(plan.get_slot(slot).unwrap().stage, Stage::ObjectAction);
    let intent = plan.resolve_intent(&key("builtin:link.transfer")).unwrap();
    assert_eq!(
        plan.get_intent(intent).unwrap().authority,
        IntentAuthority::Player
    );
    assert!(plan.resolve_intent(&key("builtin:missing")).is_none());
    assert!(
        plan.slots()
            .iter()
            .all(|s| matches!(s.dispatch, Dispatch::Native(_)))
    );
    assert_eq!(
        plan.intents()[0].receiver.as_ref().unwrap().key,
        key("builtin:link")
    );
    assert_eq!(
        plan.projection().types()[0].descriptor.player_class,
        "StructureLink"
    );
}

#[test]
fn registration_and_field_order_do_not_change_hash_handles_or_dispatch() {
    let mut c = module("test");
    handler(&mut c, "z", HandlerKind::Before, &[]);
    handler(&mut c, "a", HandlerKind::Before, &["test:z"]);
    let original = plan(vec![builtin::link(), c.clone()]).unwrap();
    let mut builtin = builtin::link();
    builtin.rules.reverse();
    builtin.object_types[0].fields.reverse();
    builtin.slots.reverse();
    c.handlers.reverse();
    let reversed = plan(vec![c, builtin]).unwrap();
    assert_eq!(original.hash(), reversed.hash());
    assert_eq!(original.rules(), reversed.rules());
    assert_eq!(original.projection(), reversed.projection());
    assert_eq!(original.slots(), reversed.slots());
    let ty = reversed.projection().resolve(&key("builtin:link")).unwrap();
    let field = reversed
        .projection()
        .field(ty, &key("builtin:cooldown"))
        .unwrap();
    assert!(
        reversed
            .projection()
            .get(ty)
            .unwrap()
            .fields
            .iter()
            .any(|f| f.handle == field)
    );
}

#[test]
fn canonical_hash_is_independent_of_json_object_insertion_order() {
    let mut a = builtin::link();
    let field = a.object_types[0]
        .fields
        .iter_mut()
        .find(|f| f.key == key("builtin:store"))
        .unwrap();
    field.schema = ValueSchema {
        kind: ValueKind::Record {
            fields: BTreeMap::from([
                (
                    "energy".into(),
                    ValueSchema {
                        kind: ValueKind::Bool,
                        nullable: false,
                    },
                ),
                (
                    "other".into(),
                    ValueSchema {
                        kind: ValueKind::Bool,
                        nullable: false,
                    },
                ),
            ]),
            optional: BTreeSet::new(),
        },
        nullable: false,
    };
    field.default =
        FieldDefault::Value(serde_json::from_str("{\"energy\":true,\"other\":false}").unwrap());
    let mut b = a.clone();
    b.object_types[0]
        .fields
        .iter_mut()
        .find(|f| f.key == key("builtin:store"))
        .unwrap()
        .default =
        FieldDefault::Value(serde_json::from_str("{\"other\":false,\"energy\":true}").unwrap());
    assert_eq!(plan(vec![a]).unwrap().hash(), plan(vec![b]).unwrap().hash());
}

#[test]
fn fingerprint_covers_rules_permissions_layout_and_source_digests() {
    let base = plan(vec![builtin::link()]).unwrap().hash();
    for change in 0..7 {
        let mut c = builtin::link();
        match change {
            0 => c.rules[0].default = Number::Signed(999),
            1 => c.object_types[0].fields[0].visibility = Visibility::Owner,
            2 => c.object_types[0].schema_version += 1,
            3 => c.source.code_digest[0] ^= 1,
            4 => c.source.config_digest[0] ^= 1,
            5 => c.source.player_api_digest[0] ^= 1,
            _ => {
                c.object_types[0].fields[0].schema = ValueSchema {
                    kind: ValueKind::String { max_bytes: 129 },
                    nullable: false,
                }
            }
        }
        if change == 2 {
            c.intents[0].receiver.as_mut().unwrap().schema_version = 2;
            if let ValueKind::Record { fields: args, .. } = &mut c.intents[0].arguments.kind
                && let ValueKind::ObjectRef(r) = &mut args.get_mut("id").unwrap().kind
            {
                r.schema_version = 2;
            }
        }
        assert_ne!(base, plan(vec![c]).unwrap().hash(), "change {change}");
    }
}

#[test]
fn duplicate_sources_definitions_and_namespace_impersonation_are_rejected() {
    // plan 助手把首个 builtin 命名空间贡献分流到独立参数，第二个留在
    // mods 侧按保留名拒绝。
    assert!(
        plan(vec![builtin::link(), builtin::link()])
            .unwrap_err()
            .message
            .contains("builtin 命名空间保留")
    );
    // 通用重复来源（非 builtin 命名空间）仍由 freeze 内部拒绝。
    assert!(
        plan(vec![module("dup"), module("dup")])
            .unwrap_err()
            .message
            .contains("重复贡献来源")
    );
    // Mod 侧直接声明 builtin 命名空间：freeze 入口自身拒绝，不依赖
    // 同批真 builtin 贡献触发撞名。
    assert!(
        freeze(vec![module("builtin")], FreezeOptions::default())
            .unwrap_err()
            .message
            .contains("builtin 命名空间保留")
    );
    let mut c = builtin::link();
    c.object_types.push(c.object_types[0].clone());
    error(c, "重复定义");
    let mut c = builtin::link();
    c.source.namespace = "other".into();
    error(c, "不能定义其他命名空间");
    let mut c = builtin::link();
    c.source.api_version = 2;
    error(c, "API 版本");
    for name in [
        "",
        "link",
        "builtin:",
        "Builtin:link",
        "builtin:two:colons",
        "builtin:link/field",
        "builtin: 字段",
    ] {
        assert!(Key::new(name).is_err(), "{name}");
    }
}

#[test]
fn unsupported_projection_versions_and_schema_limits_are_rejected() {
    for options in [
        FreezeOptions {
            projection_version: 2,
            ..Default::default()
        },
        FreezeOptions {
            max_schema_depth: 0,
            ..Default::default()
        },
        FreezeOptions {
            max_schema_depth: 33,
            ..Default::default()
        },
    ] {
        assert!(freeze_with_builtin(builtin::link(), vec![], options).is_err());
    }
}
