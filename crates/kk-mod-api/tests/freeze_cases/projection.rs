use super::*;

#[test]
fn optional_record_members_do_not_implicitly_accept_null_or_unknown_names() {
    for (optional, nullable, default, valid) in [
        ("value", false, json!({}), true),
        ("value", false, json!({"value": null}), false),
        ("value", true, json!({"value": null}), true),
        ("value", false, json!({"unknown": true}), false),
        ("missing", false, json!({"value":true}), false),
    ] {
        let mut c = builtin::link();
        let field = c.object_types[0]
            .fields
            .iter_mut()
            .find(|f| f.key == key("builtin:store"))
            .unwrap();
        field.schema = ValueSchema {
            kind: ValueKind::Record {
                fields: BTreeMap::from([(
                    "value".into(),
                    ValueSchema {
                        kind: ValueKind::Bool,
                        nullable,
                    },
                )]),
                optional: [optional.into()].into(),
            },
            nullable: false,
        };
        field.default = FieldDefault::Value(default);
        assert_eq!(plan(vec![c]).is_ok(), valid);
    }
}

#[test]
fn bounded_maps_allow_empty_store_but_reject_large_keys_and_bad_members() {
    let schema = ValueSchema {
        kind: ValueKind::Map {
            value: Box::new(ValueSchema {
                kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 100 }),
                nullable: false,
            }),
            max_len: 2,
            max_key_bytes: 6,
        },
        nullable: false,
    };
    for (value, valid) in [
        (json!({}), true),
        (json!({"energy":0}), true),
        (json!({"a":1,"b":2,"c":3}), false),
        (json!({"longkey":1}), false),
        (json!({"energy":-1}), false),
    ] {
        let mut c = builtin::link();
        let field = c.object_types[0]
            .fields
            .iter_mut()
            .find(|f| f.key == key("builtin:store"))
            .unwrap();
        field.schema = schema.clone();
        field.default = FieldDefault::Value(value);
        assert_eq!(plan(vec![c]).is_ok(), valid);
    }
}

#[test]
fn field_defaults_preserve_missing_null_zero_and_enforce_bounds() {
    for (schema, value, valid) in [
        (
            ValueSchema {
                kind: ValueKind::Bool,
                nullable: false,
            },
            FieldDefault::Missing,
            true,
        ),
        (
            ValueSchema {
                kind: ValueKind::Bool,
                nullable: true,
            },
            FieldDefault::Value(json!(null)),
            true,
        ),
        (
            ValueSchema {
                kind: ValueKind::Bool,
                nullable: false,
            },
            FieldDefault::Value(json!(null)),
            false,
        ),
        (
            ValueSchema {
                kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 2 }),
                nullable: false,
            },
            FieldDefault::Value(json!(0)),
            true,
        ),
        (
            ValueSchema {
                kind: ValueKind::String { max_bytes: 2 },
                nullable: false,
            },
            FieldDefault::Value(json!("中")),
            false,
        ),
        (
            ValueSchema {
                kind: ValueKind::Array {
                    element: Box::new(ValueSchema {
                        kind: ValueKind::Bool,
                        nullable: false,
                    }),
                    max_len: 1,
                },
                nullable: false,
            },
            FieldDefault::Value(json!([true, false])),
            false,
        ),
    ] {
        let mut c = builtin::link();
        let field = c.object_types[0]
            .fields
            .iter_mut()
            .find(|f| f.key == key("builtin:store"))
            .unwrap();
        field.schema = schema;
        field.default = value;
        assert_eq!(plan(vec![c]).is_ok(), valid);
    }
}

#[test]
fn nested_schema_and_type_reference_versions_are_checked() {
    let mut schema = ValueSchema {
        kind: ValueKind::Bool,
        nullable: false,
    };
    for _ in 0..16 {
        schema = ValueSchema {
            kind: ValueKind::Array {
                element: Box::new(schema),
                max_len: 1,
            },
            nullable: false,
        };
    }
    let mut c = builtin::link();
    c.object_types[0].fields[0].schema = schema;
    error(c, "嵌套超过");
    let mut c = builtin::link();
    c.intents[0].receiver.as_mut().unwrap().schema_version = 2;
    error(c, "schema_version 不匹配");
    let mut c = builtin::link();
    c.intents[0].receiver.as_mut().unwrap().key = key("builtin:missing");
    error(c, "未知类型");
}

#[test]
fn fields_ownership_capabilities_and_bindings_are_validated() {
    let mut c = builtin::link();
    let duplicate = c.object_types[0].fields[0].clone();
    c.object_types[0].fields.push(duplicate);
    error(c, "重复字段");
    let mut c = builtin::link();
    c.object_types[0].owner_field = None;
    c.object_types[0].fields[0].visibility = Visibility::Owner;
    error(c, "归属字段");
    let mut c = builtin::link();
    c.object_types[0].owner_field = Some(key("builtin:missing"));
    error(c, "未知归属字段");
    let mut c = builtin::link();
    c.object_types[0].position_fields = Some([key("builtin:x"), key("builtin:x")]);
    error(c, "位置字段不能相同");
    let mut c = builtin::link();
    c.object_types[0].fields[0].visibility = Visibility::Conditional(key("builtin:missing"));
    error(c, "未知绑定");
    let mut c = builtin::link();
    c.object_types[0].capabilities[0].consumers.clear();
    error(c, "未声明消费者");
    let mut c = builtin::link();
    c.object_types[0]
        .lifecycle_slots
        .insert(key("builtin:missing"));
    error(c, "未知生命周期入口");
}
