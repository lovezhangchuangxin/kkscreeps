//! link 的描述与默认数值，不安装引擎处理器，也不替换现有玩家 API。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::*;

fn key(name: &str) -> Key {
    Key::new(format!("builtin:{name}")).expect("内建键合法")
}
fn keys(names: &[&str]) -> BTreeSet<Key> {
    names.iter().map(|name| key(name)).collect()
}
fn signed(min: i64, max: i64) -> ValueSchema {
    ValueSchema {
        kind: ValueKind::Number(NumberDomain::Signed { min, max }),
        nullable: false,
    }
}
fn string(max_bytes: u32) -> ValueSchema {
    ValueSchema {
        kind: ValueKind::String { max_bytes },
        nullable: false,
    }
}

pub fn link() -> Contribution {
    let consumers = keys(&[
        "link.init",
        "link.build",
        "link.capacity",
        "link.active",
        "link.precheck",
        "link.transfer.native",
        "link.tick.native",
        "link.constants",
        "link.project",
    ]);
    let mut bindings = consumers.clone();
    bindings.insert(key("link.sanitize"));
    let mut fields = Vec::new();
    let mut field = |name: &str, schema: ValueSchema, default: FieldDefault| {
        let binding = key(&format!("link.field.{name}"));
        bindings.insert(binding.clone());
        fields.push(FieldDescriptor {
            key: key(name),
            schema,
            default,
            visibility: Visibility::Public,
            binding,
        });
    };
    field("id", string(128), FieldDefault::Missing);
    field("room", string(128), FieldDefault::Missing);
    let coordinate = ValueSchema {
        kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
        nullable: false,
    };
    field("x", coordinate.clone(), FieldDefault::Missing);
    field("y", coordinate, FieldDefault::Missing);
    field(
        "owner",
        ValueSchema {
            nullable: true,
            ..string(128)
        },
        FieldDefault::Missing,
    );
    field(
        "structureType",
        ValueSchema {
            kind: ValueKind::Enum(["link".into()].into()),
            nullable: false,
        },
        FieldDefault::Value(json!("link")),
    );
    field(
        "hits",
        signed(0, i64::from(u32::MAX)),
        FieldDefault::Missing,
    );
    field(
        "hitsMax",
        signed(0, i64::from(u32::MAX)),
        FieldDefault::Missing,
    );
    field("cooldown", signed(0, i64::MAX), FieldDefault::Missing);
    field(
        "store",
        ValueSchema {
            kind: ValueKind::Map {
                value: Box::new(signed(0, i64::MAX)),
                max_len: 4096,
                max_key_bytes: 128,
            },
            nullable: false,
        },
        FieldDefault::Missing,
    );
    field("storeCapacity", signed(0, i64::MAX), FieldDefault::Missing);
    field(
        "storeCapacityResource",
        ValueSchema {
            kind: ValueKind::Map {
                value: Box::new(signed(0, i64::MAX)),
                max_len: 4096,
                max_key_bytes: 128,
            },
            nullable: false,
        },
        FieldDefault::Missing,
    );

    let mut rules = Vec::new();
    for (name, value, max) in [
        ("link.hits", c::LINK_HITS, i64::from(u32::MAX)),
        ("link.hitsMax", c::LINK_HITS_MAX, i64::from(u32::MAX)),
        ("link.capacity", c::LINK_CAPACITY, i64::from(u32::MAX)),
        ("link.cooldown", c::LINK_COOLDOWN, i64::MAX / 49),
        (
            "link.buildCost",
            c::CONSTRUCTION_COST
                .iter()
                .find(|(name, _)| *name == "link")
                .expect("link 建造成本")
                .1,
            i64::from(u32::MAX),
        ),
    ] {
        rules.push(RuleDescriptor {
            key: key(name),
            domain: NumberDomain::Signed { min: 0, max },
            default: Number::Signed(value),
            consumers: consumers.clone(),
        });
    }
    rules.push(RuleDescriptor {
        key: key("link.lossRatio"),
        domain: NumberDomain::Float { min: 0.0, max: 1.0 },
        default: Number::Float(c::LINK_LOSS_RATIO),
        consumers: consumers.clone(),
    });
    let quotas: serde_json::Value =
        serde_json::from_str(c::CONTROLLER_STRUCTURES).expect("内建 RCL 表合法");
    for level in 0..=8 {
        rules.push(RuleDescriptor {
            key: key(&format!("link.rcl{level}")),
            domain: NumberDomain::Signed {
                min: 0,
                max: i64::from(u32::MAX),
            },
            default: Number::Signed(quotas["link"][level.to_string()].as_i64().unwrap_or(0)),
            consumers: consumers.clone(),
        });
    }
    let slots = vec![
        SlotDescriptor {
            key: key("link.transfer"),
            stage: Stage::ObjectAction,
            native_binding: key("link.transfer.native"),
        },
        SlotDescriptor {
            key: key("link.tick"),
            stage: Stage::ObjectTick,
            native_binding: key("link.tick.native"),
        },
    ];
    Contribution {
        source: Source {
            namespace: "builtin".into(),
            api_version: 1,
            revision: "link-descriptors-v1".into(),
            code_digest: Sha256::digest(include_bytes!("builtin.rs")).into(),
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings,
        object_types: vec![ObjectTypeDescriptor {
            key: key("link"),
            schema_version: 1,
            owner_field: Some(key("owner")),
            position_fields: Some([key("x"), key("y")]),
            fields,
            capabilities: vec![Capability {
                key: key("store"),
                consumers,
            }],
            lifecycle_slots: keys(&["link.tick"]),
            visibility: ObjectVisibility::RoomVisible,
            player_class: "StructureLink".into(),
        }],
        intents: vec![IntentDescriptor {
            key: key("link.transfer"),
            scope: IntentScope::Object,
            receiver: Some(TypeRef {
                key: key("link"),
                schema_version: 1,
            }),
            arguments: ValueSchema {
                kind: ValueKind::Record {
                    fields: BTreeMap::from([
                        (
                            "id".into(),
                            ValueSchema {
                                kind: ValueKind::ObjectRef(TypeRef {
                                    key: key("link"),
                                    schema_version: 1,
                                }),
                                nullable: false,
                            },
                        ),
                        ("amount".into(), signed(0, i64::MAX)),
                        (
                            "resourceType".into(),
                            ValueSchema {
                                nullable: true,
                                ..string(128)
                            },
                        ),
                    ]),
                    optional: ["resourceType".into()].into(),
                },
                nullable: false,
            },
            conversion: ArgumentConversion::Native(key("link.sanitize")),
            authority: IntentAuthority::Player,
            merge: Merge::Last,
            slot: key("link.transfer"),
            suppressed_by: BTreeSet::new(),
        }],
        rules,
        overrides: Vec::new(),
        constraints: [RuleConstraint::LessOrEqual {
            left: key("link.hits"),
            right: key("link.hitsMax"),
        }]
        .into(),
        slots,
        handlers: Vec::new(),
    }
}
