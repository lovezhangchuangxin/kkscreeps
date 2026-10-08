//! 声明生成的映射与确定性测试；构造手法与 player_surface 测试同源。
use super::*;
use crate::*;
use crate::{Contribution, FreezeOptions, Source, freeze_with_builtin};
use std::collections::{BTreeMap, BTreeSet};

fn key(value: &str) -> Key {
    Key::new(value).unwrap()
}

fn module(namespace: &str) -> Contribution {
    Contribution {
        source: Source {
            namespace: namespace.into(),
            api_version: 1,
            revision: "7".into(),
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

fn field(name: &str, kind: ValueKind, visibility: Visibility) -> FieldDescriptor {
    FieldDescriptor {
        key: key(&format!("alpha:{name}")),
        schema: ValueSchema {
            kind,
            nullable: false,
        },
        default: FieldDefault::Missing,
        visibility,
        binding: key(&format!("alpha:t.field.{name}")),
    }
}

fn signed(min: i64, max: i64) -> ValueKind {
    ValueKind::Number(NumberDomain::Signed { min, max })
}

fn rich_plan() -> crate::ContributionPlan {
    let mut c = module("alpha");
    let register = |c: &mut Contribution, fields: &[FieldDescriptor]| {
        for field in fields {
            c.bindings.insert(field.binding.clone());
        }
    };
    let x = key("alpha:x");
    let y = key("alpha:y");
    let beacon_fields = vec![
        field(
            "owner",
            ValueKind::String { max_bytes: 128 },
            Visibility::Public,
        ),
        field(
            "x",
            ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
            Visibility::Public,
        ),
        field(
            "y",
            ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
            Visibility::Public,
        ),
        field("level", signed(-5, 10), Visibility::Public),
        field(
            "state",
            ValueKind::Enum(BTreeSet::from(["on".into(), "off".into()])),
            Visibility::Public,
        ),
        field("armed", ValueKind::Bool, Visibility::Public),
        field(
            "weight",
            ValueKind::Number(NumberDomain::Float { min: 0.5, max: 9.5 }),
            Visibility::Public,
        ),
        field(
            "mentor",
            ValueKind::ObjectRef(TypeRef {
                key: key("alpha:beacon"),
                schema_version: 1,
            }),
            Visibility::Public,
        ),
        // 合法但非 TS 标识符的字段名：声明须退化为引号键。
        field("hit-points", signed(0, 999), Visibility::Public),
        // 含引号的枚举值：字面量须转义，否则产出未闭合字符串。
        field(
            "mood",
            ValueKind::Enum(BTreeSet::from(["it's".into(), "calm".into()])),
            Visibility::Public,
        ),
        field(
            "visitors",
            ValueKind::Array {
                element: Box::new(ValueSchema {
                    kind: ValueKind::String { max_bytes: 24 },
                    nullable: true,
                }),
                max_len: 8,
            },
            Visibility::Public,
        ),
        field(
            "tally",
            ValueKind::Map {
                value: Box::new(ValueSchema {
                    kind: signed(0, 100),
                    nullable: false,
                }),
                max_len: 16,
                max_key_bytes: 24,
            },
            Visibility::Public,
        ),
        field(
            "note",
            ValueKind::String { max_bytes: 64 },
            Visibility::Owner,
        ),
        field("secret", ValueKind::Bool, Visibility::ServerOnly),
    ];
    register(&mut c, &beacon_fields);
    c.object_types.push(ObjectTypeDescriptor {
        key: key("alpha:beacon"),
        schema_version: 1,
        owner_field: Some(key("alpha:owner")),
        position_fields: Some([x, y]),
        fields: beacon_fields,
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Beacon".into(),
        store_capacity: None,
        hits_max: None,
        retaliation_milli: None,
    });
    c.intents.push(IntentDescriptor {
        key: key("alpha:salute"),
        scope: IntentScope::Object,
        receiver: Some(TypeRef {
            key: key("alpha:beacon"),
            schema_version: 1,
        }),
        arguments: ValueSchema {
            kind: ValueKind::Record {
                fields: BTreeMap::from([
                    (
                        "amount".to_string(),
                        ValueSchema {
                            kind: signed(1, 50),
                            nullable: false,
                        },
                    ),
                    (
                        "tag".to_string(),
                        ValueSchema {
                            kind: ValueKind::String { max_bytes: 16 },
                            nullable: true,
                        },
                    ),
                ]),
                optional: BTreeSet::from(["tag".to_string()]),
            },
            nullable: false,
        },
        conversion: ArgumentConversion::Strict,
        authority: IntentAuthority::Player,
        merge: Merge::Last,
        slot: key("alpha:salute.slot"),
        suppressed_by: BTreeSet::new(),
    });
    c.slots.push(SlotDescriptor {
        key: key("alpha:salute.slot"),
        stage: Stage::ObjectAction,
        native_binding: key("alpha:salute.process"),
    });
    c.bindings.insert(key("alpha:salute.process"));
    c.resources.push(crate::resources::ResourceDescriptor {
        key: key("alpha:fuel"),
    });
    // 字段绑定按装载器口径入 bindings 集（freeze 校验其存在）。
    c.bindings.insert(key("alpha:gate.field.charge"));
    c.buildings.push(crate::buildings::BuildingDescriptor {
        key: key("alpha:gate"),
        player_class: "Gate".into(),
        cost: vec![("energy".into(), 500)],
        hits_max: 1000,
        pathing: crate::buildings::BuildingPathing::Obstacle,
        store_capacity: None,
        production: None,
        fields: vec![crate::FieldDescriptor {
            key: key("alpha:charge"),
            schema: ValueSchema {
                kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 100 }),
                nullable: false,
            },
            default: crate::FieldDefault::Value(serde_json::json!(0)),
            visibility: Visibility::Public,
            binding: key("alpha:gate.field.charge"),
        }],
        lifecycle_slots: BTreeSet::new(),
    });
    freeze_with_builtin(crate::builtin::link(), vec![c], FreezeOptions::default()).unwrap()
}

#[test]
fn declarations_map_every_value_kind_and_envelope() {
    let text = player_declarations(&rich_plan()).unwrap();
    // 信封与类型键字面量。声明了位置字段 x/y 时信封键不重复出现
    // （投影最后写入信封，同名声明字段被等值覆写）。
    assert!(text.contains("export interface Beacon {"), "{text}");
    assert!(text.contains("modType: 'alpha:beacon';"));
    assert!(text.contains("type: 'mod';"));
    assert!(text.contains("pos: { x: number; y: number; roomName: string };"));
    assert_eq!(
        text.match_indices("\n  x: number;").count(),
        1,
        "信封 x 只声明一次：{text}"
    );
    assert!(
        text.contains("\n  /** "),
        "interface 内字段注释须缩进：{text}"
    );
    // 数值域入注释；枚举成联合；数组 readonly 且元素可空；Map 成 Record。
    assert!(text.contains("level: number;"));
    assert!(text.contains("整数 -5..=10"));
    assert!(text.contains("浮点 0.5..=9.5"), "Float 域须入注释：{text}");
    assert!(text.contains("weight: number;"));
    assert!(text.contains("state: 'off' | 'on';"), "{text}");
    assert!(
        text.contains("armed: boolean;"),
        "Bool 须映射 boolean：{text}"
    );
    // ObjectRef 投影为对象 id 字符串，注释标注指向类型。
    assert!(
        text.contains("mentor: string /* alpha:beacon */;"),
        "{text}"
    );
    assert!(text.contains("对象 alpha:beacon 的 id"));
    assert!(text.contains("visitors: ReadonlyArray<string | null>;"));
    assert!(text.contains("tally: Record<string, number>;"));
    // 非 TS 标识符合法字段名退化为引号键；枚举值中的引号被转义。
    assert!(
        text.contains("'hit-points': number;"),
        "kebab 字段名须引号键：{text}"
    );
    assert!(
        text.contains("mood: 'calm' | 'it\\'s';"),
        "枚举字面量须转义：{text}"
    );
    // 归属者字段可选；ServerOnly 字段不出现。
    assert!(text.contains("note?: string;"), "{text}");
    assert!(!text.contains("secret"), "ServerOnly 字段不得进入玩家声明");
    // 构造器形态：运行时仅作原型载体（setPrototypeOf 从不构造），
    // 实例约束不进类型（extends 内建类的玩家类无法满足投影 interface）。
    assert!(
        text.contains("export type BeaconConstructor = new (...args: never[]) => unknown;"),
        "{text}"
    );
    // 内建类基类形态须可被 extends（实例类型为对象类型）。
    assert!(
        text.contains("export type ModBuiltinClass = abstract new (...args: never[]) => object;"),
        "{text}"
    );
}

#[test]
fn intents_resources_and_buildings_are_typed() {
    let text = player_declarations(&rich_plan()).unwrap();
    // 意图参数 interface 与 emit 窄化签名。
    assert!(
        text.contains("export interface AlphaSaluteArgs {"),
        "{text}"
    );
    assert!(text.contains("amount: number;"));
    assert!(text.contains("tag?: string | null;"), "{text}");
    assert!(text.contains(
        "emit(intentKey: 'alpha:salute', target: { id: string }, args: AlphaSaluteArgs): number;"
    ));
    assert!(text.contains("接收者 alpha:beacon"));
    // 资源联合与建筑绑定。
    assert!(text.contains("export type ModResourceName = 'alpha:fuel';"));
    assert!(text.contains("bind(typeKey: 'alpha:gate', cls: GateConstructor): void;"));
    // 建筑自定义字段接口：存储键成员直供玩家交叉类型。
    assert!(text.contains("export interface GateConstructorFields {"));
    assert!(text.contains("'alpha:charge': number"), "{text}");
    // 查询常量为数字字面量类型。
    assert!(
        text.contains("install(typeKey: 'alpha:beacon'): { find: 1000; look: 'alpha:beacon' };")
    );
    // 内建类清单。
    assert!(text.contains("'Creep'"));
    assert!(text.contains("export type ModBuiltinClass"));
}

#[test]
fn generation_is_deterministic_and_empty_plan_degrades() {
    let plan = rich_plan();
    assert_eq!(
        player_declarations(&plan).unwrap(),
        player_declarations(&plan).unwrap()
    );
    // 无 Mod 声明的计划：只剩内建类清单与空目录段，不产出 interface。
    let empty =
        freeze_with_builtin(crate::builtin::link(), Vec::new(), FreezeOptions::default()).unwrap();
    let text = player_declarations(&empty).unwrap();
    // 安装 API interface 恒在；对象类型缺席以构造器形态判别。
    assert!(!text.contains("Constructor"), "{text}");
    assert!(text.contains("objects: {};"));
    assert!(text.contains("queries: {};"));
    assert!(text.contains("intents: {};"));
    assert!(text.contains("buildings: {};"));
    assert!(!text.contains("ModResourceName"));
    assert!(text.contains("ModBuiltinClassName"));
}

#[test]
fn non_object_intent_scope_is_rejected_consistently_with_install() {
    let mut c = module("alpha");
    c.object_types.push(ObjectTypeDescriptor {
        key: key("alpha:beacon"),
        schema_version: 1,
        owner_field: None,
        position_fields: None,
        fields: Vec::new(),
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Beacon".into(),
        store_capacity: None,
        hits_max: None,
        retaliation_milli: None,
    });
    c.intents.push(IntentDescriptor {
        key: key("alpha:shout"),
        scope: IntentScope::Room,
        receiver: None,
        arguments: ValueSchema {
            kind: ValueKind::Bool,
            nullable: false,
        },
        conversion: ArgumentConversion::Strict,
        authority: IntentAuthority::Player,
        merge: Merge::Last,
        slot: key("alpha:shout.slot"),
        suppressed_by: BTreeSet::new(),
    });
    c.slots.push(SlotDescriptor {
        key: key("alpha:shout.slot"),
        stage: Stage::ObjectAction,
        native_binding: key("alpha:shout.process"),
    });
    c.bindings.insert(key("alpha:shout.process"));
    let plan =
        freeze_with_builtin(crate::builtin::link(), vec![c], FreezeOptions::default()).unwrap();
    let error = player_declarations(&plan).unwrap_err();
    assert!(error.contains("执行通道未接线"), "{error}");
    // 共用门覆盖全部未接线形态：队列合并计划同样拒绝，不会产出
    // "能生成声明但装不上"的计划。
    let mut c = module("alpha");
    c.object_types.push(ObjectTypeDescriptor {
        key: key("alpha:beacon"),
        schema_version: 1,
        owner_field: None,
        position_fields: None,
        fields: Vec::new(),
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Beacon".into(),
        store_capacity: None,
        hits_max: None,
        retaliation_milli: None,
    });
    c.intents.push(IntentDescriptor {
        key: key("alpha:queued"),
        scope: IntentScope::Object,
        receiver: Some(TypeRef {
            key: key("alpha:beacon"),
            schema_version: 1,
        }),
        arguments: ValueSchema {
            kind: ValueKind::Bool,
            nullable: false,
        },
        conversion: ArgumentConversion::Strict,
        authority: IntentAuthority::Player,
        merge: Merge::Queue { max_len: 8 },
        slot: key("alpha:queued.slot"),
        suppressed_by: BTreeSet::new(),
    });
    c.slots.push(SlotDescriptor {
        key: key("alpha:queued.slot"),
        stage: Stage::ObjectAction,
        native_binding: key("alpha:queued.process"),
    });
    c.bindings.insert(key("alpha:queued.process"));
    let plan =
        freeze_with_builtin(crate::builtin::link(), vec![c], FreezeOptions::default()).unwrap();
    let error = player_declarations(&plan).unwrap_err();
    assert!(error.contains("队列合并模式"), "{error}");
}

#[test]
fn args_type_names_survive_hostile_keys_and_collisions() {
    // `a-b:c` 与 `a:b-c` 段化后同为 ABCArgs：撞名须追加序号消歧，
    // 两个产出段（参数 interface 与 emit 签名）引用同名。
    let intents = |namespace: &str, name: &str| {
        let mut c = module(namespace);
        // 对象域动作须声明接收者：每个命名空间给一个最小对象类型。
        c.object_types.push(ObjectTypeDescriptor {
            key: key(&format!("{namespace}:node")),
            schema_version: 1,
            owner_field: None,
            position_fields: None,
            fields: Vec::new(),
            capabilities: Vec::new(),
            lifecycle_slots: BTreeSet::new(),
            visibility: ObjectVisibility::RoomVisible,
            player_class: format!("{}Node", namespace.replace(['-', '.'], "")),
            store_capacity: None,
            hits_max: None,
            retaliation_milli: None,
        });
        c.intents.push(IntentDescriptor {
            key: key(&format!("{namespace}:{name}")),
            scope: IntentScope::Object,
            receiver: Some(TypeRef {
                key: key(&format!("{namespace}:node")),
                schema_version: 1,
            }),
            arguments: ValueSchema {
                kind: ValueKind::Bool,
                nullable: false,
            },
            conversion: ArgumentConversion::Strict,
            authority: IntentAuthority::Player,
            merge: Merge::Last,
            slot: key(&format!("{namespace}:{name}.slot")),
            suppressed_by: BTreeSet::new(),
        });
        c.slots.push(SlotDescriptor {
            key: key(&format!("{namespace}:{name}.slot")),
            stage: Stage::ObjectAction,
            native_binding: key(&format!("{namespace}:{name}.process")),
        });
        c.bindings
            .insert(key(&format!("{namespace}:{name}.process")));
        c
    };
    let plan = freeze_with_builtin(
        crate::builtin::link(),
        vec![
            intents("a-b", "c"),
            intents("a", "b-c"),
            intents("a-c", "sa.lute"),
        ],
        FreezeOptions::default(),
    )
    .unwrap();
    let text = player_declarations(&plan).unwrap();
    // 撞名消歧后首名保留、次名追加序号；参数形态与 emit 签名引用一致。
    assert!(text.contains("export type ABCArgs = boolean;"), "{text}");
    assert!(text.contains("export type ABCArgs2 = boolean;"), "{text}");
    assert!(
        text.contains("export type ACSaLuteArgs = boolean;"),
        "段化须吞掉非字母数字：{text}"
    );
    assert!(text.contains("args: ABCArgs): number;"), "{text}");
    assert!(text.contains("args: ABCArgs2): number;"), "{text}");
    assert!(text.contains("args: ACSaLuteArgs): number;"), "{text}");
}

#[test]
fn store_declaring_types_carry_envelope_members() {
    // G09.a：capacity 声明类型的投影携带 store/storeCapacity 信封键，
    // 声明面与投影面同形；无声明类型不出现两键。
    let mut c = module("alpha");
    let x = key("alpha:x");
    let y = key("alpha:y");
    let fields = vec![
        field(
            "x",
            ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
            Visibility::Public,
        ),
        field(
            "y",
            ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
            Visibility::Public,
        ),
    ];
    for f in &fields {
        c.bindings.insert(f.binding.clone());
    }
    c.object_types.push(ObjectTypeDescriptor {
        key: key("alpha:crate"),
        schema_version: 1,
        owner_field: None,
        position_fields: Some([x, y]),
        fields,
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Crate".into(),
        store_capacity: Some(300),
        hits_max: None,
        retaliation_milli: None,
    });
    let plan =
        crate::freeze_with_builtin(crate::builtin::link(), vec![c], Default::default()).unwrap();
    let text = player_declarations(&plan).unwrap();
    assert!(text.contains("export interface Crate {"), "{text}");
    assert!(text.contains("store: Record<string, number>;"), "{text}");
    assert!(text.contains("storeCapacity: number;"), "{text}");
    // 无声明类型不携带两键（rich_plan 的 Beacon 断言反向覆盖）。
}

#[test]
fn hits_declaring_types_carry_envelope_members() {
    // G09.b：hitsMax 声明类型的投影携带 hits/hitsMax 信封键。
    let mut c = module("alpha");
    let x = key("alpha:x");
    let y = key("alpha:y");
    let fields = vec![
        field(
            "x",
            ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
            Visibility::Public,
        ),
        field(
            "y",
            ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
            Visibility::Public,
        ),
    ];
    for f in &fields {
        c.bindings.insert(f.binding.clone());
    }
    c.object_types.push(ObjectTypeDescriptor {
        key: key("alpha:bunker"),
        schema_version: 1,
        owner_field: None,
        position_fields: Some([x, y]),
        fields,
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Bunker".into(),
        store_capacity: None,
        hits_max: Some(300),
        retaliation_milli: None,
    });
    let plan =
        crate::freeze_with_builtin(crate::builtin::link(), vec![c], Default::default()).unwrap();
    let text = player_declarations(&plan).unwrap();
    assert!(text.contains("export interface Bunker {"), "{text}");
    assert!(text.contains("hits: number;"), "{text}");
    assert!(text.contains("hitsMax: number;"), "{text}");
}
