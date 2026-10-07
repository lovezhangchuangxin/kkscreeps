use std::collections::{BTreeMap, BTreeSet};

use kk_mod_api::*;
use serde_json::json;

fn key(value: &str) -> Key {
    Key::new(value).unwrap()
}
fn plan(contributions: Vec<Contribution>) -> Result<ContributionPlan, FreezeError> {
    // 调用方传混合 vec 时：builtin 命名空间贡献分流到独立参数走
    // freeze_with_builtin，其余走 freeze——两个入口都经夹具覆盖。
    let mut builtin = None;
    let mut rest = Vec::new();
    for c in contributions {
        if c.source.namespace == "builtin" && builtin.is_none() {
            builtin = Some(c);
        } else {
            rest.push(c);
        }
    }
    match builtin {
        Some(b) => freeze_with_builtin(b, rest, FreezeOptions::default()),
        None => freeze(rest, FreezeOptions::default()),
    }
}
fn module(namespace: &str) -> Contribution {
    Contribution {
        source: Source {
            namespace: namespace.into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [1; 32],
            config_digest: [2; 32],
            player_api_digest: [3; 32],
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
    }
}
fn override_rule(namespace: &str, value: Number) -> Contribution {
    let mut c = module(namespace);
    c.overrides.push(RuleOverride {
        rule: key("builtin:link.capacity"),
        value,
    });
    c
}
fn handler(c: &mut Contribution, name: &str, kind: HandlerKind, dependencies: &[&str]) {
    let key = key(&format!("{}:{name}", c.source.namespace));
    c.bindings.insert(key.clone());
    c.handlers.push(HandlerDescriptor {
        key: key.clone(),
        slot: self::key("builtin:link.transfer"),
        kind,
        binding: key,
        depends_on: dependencies.iter().map(|v| self::key(v)).collect(),
    });
}
fn error(c: Contribution, expected: &str) {
    let error = plan(vec![c]).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

mod buildings;
mod catalog;
mod handlers;
mod projection;
mod resources;
mod rules;
mod store_rules;

#[test]
fn link_rules_extract_builtin_defaults() {
    let frozen = plan(vec![builtin::link()]).unwrap();
    let rules = builtin::LinkRules::from_plan(frozen.rules()).unwrap();
    assert_eq!(rules.hits, kk_core::constants::LINK_HITS);
    assert_eq!(rules.hits_max, kk_core::constants::LINK_HITS_MAX);
    assert_eq!(rules.capacity, kk_core::constants::LINK_CAPACITY);
    assert_eq!(rules.cooldown, kk_core::constants::LINK_COOLDOWN);
    assert_eq!(rules.loss_ratio, kk_core::constants::LINK_LOSS_RATIO);
    assert_eq!(rules.build_cost, 5000);
}

#[test]
fn structure_quota_snapshot_builtin_defaults() {
    // G11.a：配额族逐型逐级与官方表键集一致，默认值即官方值；
    // link 配额并入本族（原 link.rcl0..8 快照字段已删）。
    let frozen = plan(vec![builtin::link()]).unwrap();
    let snapshot = builtin::StructureQuotaSnapshot::from_plan(frozen.rules()).unwrap();
    let quotas: serde_json::Value =
        serde_json::from_str(kk_core::constants::CONTROLLER_STRUCTURES).unwrap();
    for (stype, levels) in quotas.as_object().unwrap() {
        for (level, value) in levels.as_object().unwrap() {
            let level_i: i64 = level.parse().unwrap();
            assert_eq!(
                snapshot.get(stype, level_i),
                Some(value.as_i64().unwrap()),
                "{stype} RCL {level} 配额应与官方表一致"
            );
        }
    }
    // 官方无的级别键不声明（link rcl0）：get 返回 None。
    assert_eq!(snapshot.get("link", 0), None);
    assert_eq!(snapshot.get("link", 9), None, "级别越界 None");
    // 折叠表：无覆盖时与官方表全表同值同键形（含键序保真，经
    // workspace 的 serde_json preserve_order）。
    let folded = snapshot.fold_controller_structures();
    assert_eq!(folded, quotas);
    assert!(folded["link"].as_object().unwrap().get("0").is_none());
}

#[test]
fn structure_quota_snapshot_rejects_incomplete_plan() {
    // 不含配额族的计划在装配期整体拒绝，不留半填充快照；错误指名
    // 首个缺失键（迭代序随 serde_json feature 图而定，只断言键前缀）。
    let empty = plan(vec![module("none")]).unwrap();
    let error = builtin::StructureQuotaSnapshot::from_plan(empty.rules()).unwrap_err();
    assert!(
        error.contains("builtin:structure.quota."),
        "错误应指名缺失的配额键：{error}"
    );
}

#[test]
fn structure_quota_snapshot_follows_override() {
    let mut contrib = override_rule("mods", Number::Signed(400));
    contrib.overrides.push(RuleOverride {
        rule: key("builtin:structure.quota.tower.rcl8"),
        value: Number::Signed(50),
    });
    contrib.overrides.push(RuleOverride {
        rule: key("builtin:structure.quota.link.rcl5"),
        value: Number::Signed(3),
    });
    let frozen = plan(vec![builtin::link(), contrib]).unwrap();
    let snapshot = builtin::StructureQuotaSnapshot::from_plan(frozen.rules()).unwrap();
    assert_eq!(snapshot.get("tower", 8), Some(50), "覆盖生效");
    assert_eq!(snapshot.get("tower", 7), Some(3), "未覆盖级保持官方值");
    assert_eq!(snapshot.get("link", 5), Some(3), "link 并入本族可覆盖");
    let folded = snapshot.fold_controller_structures();
    assert_eq!(folded["tower"]["8"].as_i64().unwrap(), 50);
    assert_eq!(folded["tower"]["7"].as_i64().unwrap(), 3);
    assert_eq!(folded["link"]["5"].as_i64().unwrap(), 3);
}

#[test]
fn link_rules_follow_override_and_reject_missing_plan() {
    let mut contrib = override_rule("mods", Number::Signed(400));
    contrib.overrides.push(RuleOverride {
        rule: key("builtin:link.lossRatio"),
        value: Number::Float(0.5),
    });
    let frozen = plan(vec![builtin::link(), contrib]).unwrap();
    let rules = builtin::LinkRules::from_plan(frozen.rules()).unwrap();
    assert_eq!(rules.capacity, 400);
    assert_eq!(rules.loss_ratio, 0.5);
    // 两个计划的指纹不同，规则快照互不可换。
    let plain = plan(vec![builtin::link()]).unwrap();
    assert_ne!(plain.hash(), frozen.hash());

    let empty = plan(vec![module("none")]).unwrap();
    let error = builtin::LinkRules::from_plan(empty.rules()).unwrap_err();
    assert!(error.contains("builtin:link.lossRatio"), "{error}");
}

fn mount_mod(field_key: &str, visibility: Visibility, schema: ValueSchema) -> Contribution {
    let mut c = module("extmod");
    c.bindings.insert(key(&format!("extmod:bind.{field_key}")));
    c.field_mounts.push(FieldMountDescriptor {
        target: TypeRef {
            key: key("builtin:link"),
            schema_version: 1,
        },
        field: FieldDescriptor {
            key: key(&format!("extmod:{field_key}")),
            schema,
            default: FieldDefault::Missing,
            visibility,
            binding: key(&format!("extmod:bind.{field_key}")),
        },
    });
    c
}

#[test]
fn field_mounts_extend_existing_type_into_catalog() {
    let mut contrib = mount_mod(
        "charge",
        Visibility::Owner,
        ValueSchema {
            kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 1000 }),
            nullable: false,
        },
    );
    contrib.field_mounts.push(FieldMountDescriptor {
        target: TypeRef {
            key: key("builtin:link"),
            schema_version: 1,
        },
        field: FieldDescriptor {
            key: key("extmod:label"),
            schema: ValueSchema {
                kind: ValueKind::String { max_bytes: 16 },
                nullable: false,
            },
            default: FieldDefault::Missing,
            visibility: Visibility::Public,
            binding: key("extmod:bind.label"),
        },
    });
    contrib.bindings.insert(key("extmod:bind.label"));
    let frozen = plan(vec![builtin::link(), contrib]).unwrap();
    let catalog = extension::ExtensionCatalog::from_plan(&frozen).unwrap();
    let link = key("builtin:link");
    let charge = catalog.field(&link, &key("extmod:charge")).unwrap();
    assert_eq!(charge.visibility, Visibility::Owner);
    let label = catalog.field(&link, &key("extmod:label")).unwrap();
    assert_eq!(label.visibility, Visibility::Public);
    // 内建字段经 builtin 绑定适配，不进扩展目录。
    assert!(
        catalog
            .field(&link, &key("builtin:link.capacity"))
            .is_none()
    );
    // 挂载改变投影目录，两个计划的指纹不同。
    let plain = plan(vec![builtin::link()]).unwrap();
    assert_ne!(plain.hash(), frozen.hash());
}

#[test]
fn field_mounts_reject_bad_targets_and_unsupported_visibility() {
    // 条件权限没有投影判定入口，目录提取时拒绝（声明本身合法，
    // 装配扩展写入入口时失败——与处理器缺失同一层）。
    let mut conditional = mount_mod(
        "secret",
        Visibility::Conditional(key("extmod:judge")),
        ValueSchema {
            kind: ValueKind::Bool,
            nullable: false,
        },
    );
    // 判定绑定先行声明，使冻结通过、目录提取层给出"条件权限不支持"。
    conditional.bindings.insert(key("extmod:judge"));
    let frozen = plan(vec![builtin::link(), conditional]).unwrap();
    let err =
        extension::ExtensionCatalog::from_plan(&frozen).expect_err("条件权限扩展字段拒绝进目录");
    assert!(err.contains("条件权限"), "{err}");

    // 扩展字段不允许声明默认值：目录不自动落值，声明即误导。
    let mut defaulted = mount_mod(
        "charge",
        Visibility::Owner,
        ValueSchema {
            kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 10 }),
            nullable: false,
        },
    );
    defaulted.field_mounts[0].field.default = FieldDefault::Value(serde_json::json!(3));
    let frozen = plan(vec![builtin::link(), defaulted]).unwrap();
    let err =
        extension::ExtensionCatalog::from_plan(&frozen).expect_err("带默认值的扩展字段拒绝进目录");
    assert!(err.contains("默认值"), "{err}");

    // 目标类型不存在。
    let mut stray = mount_mod(
        "charge",
        Visibility::Public,
        ValueSchema {
            kind: ValueKind::Bool,
            nullable: false,
        },
    );
    stray.field_mounts[0].target.key = key("builtin:nosuch");
    let err = plan(vec![builtin::link(), stray]).unwrap_err().to_string();
    assert!(err.contains("目标类型不存在"), "{err}");

    // 目标版本不匹配。
    let mut versioned = mount_mod(
        "charge",
        Visibility::Public,
        ValueSchema {
            kind: ValueKind::Bool,
            nullable: false,
        },
    );
    versioned.field_mounts[0].target.schema_version = 2;
    let err = plan(vec![builtin::link(), versioned])
        .unwrap_err()
        .to_string();
    assert!(err.contains("版本不匹配"), "{err}");
}

#[test]
fn field_mounts_reject_duplicates_builtin_binding_and_bad_schemas() {
    // 同键重复挂载。
    let mut dup = mount_mod(
        "charge",
        Visibility::Public,
        ValueSchema {
            kind: ValueKind::Bool,
            nullable: false,
        },
    );
    dup.field_mounts.push(dup.field_mounts[0].clone());
    let err = plan(vec![builtin::link(), dup]).unwrap_err().to_string();
    assert!(err.contains("字段挂载重复"), "{err}");

    // builtin 绑定冒充内建适配字段。
    let mut fake = mount_mod(
        "charge",
        Visibility::Public,
        ValueSchema {
            kind: ValueKind::Bool,
            nullable: false,
        },
    );
    let stray = key("builtin:link.field.fake");
    let original = fake.bindings.insert(stray.clone());
    fake.field_mounts[0].field.binding = stray;
    let _ = original;
    // 贡献不能声明 builtin 命名空间的绑定——首遍归属检查先行拒绝；
    // 挂载通道的 builtin 检查是同语义的纵深防御。
    let err = plan(vec![builtin::link(), fake]).unwrap_err().to_string();
    assert!(err.contains("不能定义其他命名空间"), "{err}");

    // 挂载字段引用不存在的类型：挂载并入后与自带字段同一套校验。
    let mut bad_ref = mount_mod(
        "ref",
        Visibility::Public,
        ValueSchema {
            kind: ValueKind::ObjectRef(TypeRef {
                key: key("extmod:ghost"),
                schema_version: 1,
            }),
            nullable: false,
        },
    );
    bad_ref.field_mounts[0].field.key = key("extmod:ref");
    bad_ref.field_mounts[0].field.binding = key("extmod:bind.ref");
    bad_ref.bindings.insert(key("extmod:bind.ref"));
    let err = plan(vec![builtin::link(), bad_ref])
        .unwrap_err()
        .to_string();
    assert!(err.contains("extmod:ghost"), "{err}");

    // 无效数值域（min>max）在冻结期拒绝，不留永久不可写字段。
    let mut bad_domain = mount_mod(
        "charge",
        Visibility::Public,
        ValueSchema {
            kind: ValueKind::Number(NumberDomain::Signed { min: 10, max: 5 }),
            nullable: false,
        },
    );
    bad_domain.field_mounts[0].field.key = key("extmod:charge2");
    bad_domain.field_mounts[0].field.binding = key("extmod:bind.charge2");
    bad_domain.bindings.insert(key("extmod:bind.charge2"));
    let err = plan(vec![builtin::link(), bad_domain])
        .unwrap_err()
        .to_string();
    assert!(err.contains("数值范围"), "{err}");
}

/// Mod 类型 payload 以裸名存取：跨命名空间挂载引入同名裸字段（全键
/// 不同）会使同一 payload 键被两个 schema 约束，冻结期拒绝；builtin
/// 类型的扩展字段按全键存储，不受该检查约束（对照）。
#[test]
fn field_bare_name_collision_rejected_on_mod_types_only() {
    let field = |ns: &str, name: &str| FieldDescriptor {
        key: key(&format!("{ns}:{name}")),
        schema: ValueSchema {
            kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 99 }),
            nullable: false,
        },
        default: FieldDefault::Missing,
        visibility: Visibility::Public,
        binding: key(&format!("{ns}:bind.{name}")),
    };
    // 目标：nsA 声明的 Mod 类型，自带 nsA:charge。
    let mut owner = module("nsa");
    let mut bindings = BTreeSet::new();
    bindings.insert(key("nsa:bind.charge"));
    let mut ty = ObjectTypeDescriptor {
        key: key("nsa:thing"),
        schema_version: 1,
        owner_field: None,
        position_fields: None,
        fields: vec![field("nsa", "charge")],
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Thing".into(),
        store_capacity: None,
    };
    ty.fields.sort_by(|a, b| a.key.cmp(&b.key));
    owner.object_types.push(ty);
    owner.bindings = bindings;
    // nsB 挂载 nsB:charge 到 nsA:thing——全键不同、裸名相同。
    let mut mounter = module("nsb");
    mounter.bindings.insert(key("nsb:bind.charge"));
    mounter.field_mounts.push(FieldMountDescriptor {
        target: TypeRef {
            key: key("nsa:thing"),
            schema_version: 1,
        },
        field: field("nsb", "charge"),
    });
    let err = plan(vec![builtin::link(), owner.clone(), mounter]).unwrap_err();
    assert!(
        err.to_string().contains("字段裸名 charge 冲突"),
        "实得 {err}"
    );
    // 对照：同名挂载到 builtin:link（builtin:link 自带 builtin:charge
    // 全键不同、扩展字段按全键存储）不触发裸名检查。
    let mut builtin_mount = module("nsb");
    builtin_mount.bindings.insert(key("nsb:bind.charge"));
    builtin_mount.field_mounts.push(FieldMountDescriptor {
        target: TypeRef {
            key: key("builtin:link"),
            schema_version: 1,
        },
        field: field("nsb", "charge"),
    });
    plan(vec![builtin::link(), owner, builtin_mount]).unwrap();
}

#[test]
fn object_store_declaration_freezes_and_reserved_keys_reject() {
    // G09.a：capacity 声明进入冻结描述符，参与计划身份。
    let mut c = module("stmod");
    c.bindings.insert(key("stmod:crate.field.x"));
    c.object_types.push(ObjectTypeDescriptor {
        key: key("stmod:crate"),
        schema_version: 1,
        owner_field: None,
        position_fields: None,
        fields: vec![FieldDescriptor {
            key: key("stmod:x"),
            schema: ValueSchema {
                kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
                nullable: false,
            },
            default: FieldDefault::Missing,
            visibility: Visibility::Public,
            binding: key("stmod:crate.field.x"),
        }],
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Crate".into(),
        store_capacity: Some(500),
    });
    let frozen = plan(vec![builtin::link(), c.clone()]).unwrap();
    let ty = frozen
        .projection()
        .types()
        .iter()
        .find(|ty| ty.descriptor.key.as_str() == "stmod:crate")
        .expect("类型已冻结");
    assert_eq!(ty.descriptor.store_capacity, Some(500));
    // 同一类型仅撤下 store 声明即改变计划身份（排除类型集合差异的干扰）。
    c.object_types[0].store_capacity = None;
    let plain = plan(vec![builtin::link(), c]).unwrap();
    assert_ne!(plain.hash(), frozen.hash(), "store 声明参与计划身份");
    let plain_ty = plain
        .projection()
        .types()
        .iter()
        .find(|ty| ty.descriptor.key.as_str() == "stmod:crate")
        .expect("类型仍在计划中");
    assert_eq!(plain_ty.descriptor.store_capacity, None);

    // 保留键：字段裸名占用 store/storeCapacity 在玩家面导出时拒绝。
    for reserved in ["store", "storeCapacity"] {
        let mut bad = module("stmod");
        let binding = key(&format!("stmod:crate.field.{reserved}"));
        bad.bindings.insert(binding.clone());
        bad.object_types.push(ObjectTypeDescriptor {
            key: key("stmod:crate"),
            schema_version: 1,
            owner_field: None,
            position_fields: None,
            fields: vec![FieldDescriptor {
                key: key(&format!("stmod:{reserved}")),
                schema: ValueSchema {
                    kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 9 }),
                    nullable: false,
                },
                default: FieldDefault::Missing,
                visibility: Visibility::Public,
                binding,
            }],
            capabilities: Vec::new(),
            lifecycle_slots: BTreeSet::new(),
            visibility: ObjectVisibility::RoomVisible,
            player_class: "Crate".into(),
            store_capacity: None,
        });
        // 保留键在冻结期拒绝（含 ServerOnly 可见性；builtin:link 的
        // 官方投影字段形状豁免）。
        let err = plan(vec![builtin::link(), bad]).unwrap_err().to_string();
        assert!(
            err.contains("占用投影保留键"),
            "字段 {reserved} 应拒绝：{err}"
        );
    }
}
