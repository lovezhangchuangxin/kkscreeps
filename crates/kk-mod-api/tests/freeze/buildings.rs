//! 建筑声明的冻结语义：归属、重复、配方校验与计划身份。
use super::*;
use kk_mod_api::buildings::{BuildingDescriptor, BuildingPathing, ProductionSpec};
use kk_mod_api::resources::ResourceDescriptor;
use kk_mod_api::{FieldDefault, FieldDescriptor, NumberDomain, ValueKind, ValueSchema, Visibility};

fn building(namespace: &str, name: &str) -> BuildingDescriptor {
    BuildingDescriptor {
        key: key(&format!("{namespace}:{name}")),
        player_class: "Foundry".into(),
        cost: vec![("energy".into(), 5000)],
        hits_max: 5000,
        pathing: BuildingPathing::Obstacle,
        store_capacity: Some(1000),
        production: None,
        fields: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
    }
}

#[test]
fn declared_buildings_merge_into_sorted_plan_catalog() {
    let mut a = module("alpha");
    a.buildings.push(building("alpha", "zeta"));
    let mut b = module("beta");
    b.buildings.push(building("beta", "alpha"));
    let plan = plan(vec![a, b]).unwrap();
    let keys: Vec<&str> = plan.buildings().iter().map(|b| b.key.as_str()).collect();
    // 键含冒号，结构上不可能与内建结构名撞车（断言列表本身即锁定该性质）。
    assert_eq!(keys, ["alpha:zeta", "beta:alpha"]);
}

#[test]
fn building_key_must_be_owned_by_namespace() {
    let mut c = module("alpha");
    c.buildings.push(building("beta", "spark"));
    error(c, "不能定义其他命名空间的键 beta:spark");
}

#[test]
fn duplicate_building_declaration_is_rejected() {
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    c.buildings.push(building("alpha", "foundry"));
    error(c, "重复定义");
}

#[test]
fn building_key_cannot_collide_with_object_type() {
    let mut c = module("alpha");
    c.object_types.push(ObjectTypeDescriptor {
        key: key("alpha:thing"),
        schema_version: 1,
        owner_field: None,
        position_fields: None,
        fields: Vec::new(),
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: ObjectVisibility::RoomVisible,
        player_class: "Thing".into(),
        store_capacity: None,
    });
    let mut b = building("alpha", "thing");
    b.player_class = "Foundry".into();
    c.buildings.push(b);
    error(c, "同一键不能既是对象类型又是建筑类型");
}

#[test]
fn building_cost_must_reference_known_resources() {
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.cost.push(("alpha:missing".into(), 10));
    c.buildings.push(b);
    error(c, "引用未知资源 \"alpha:missing\"");

    // 他包声明的资源在合并目录中可用：跨包组合按依赖闭包装载。
    let mut other = module("beta");
    other.resources.push(ResourceDescriptor {
        key: key("beta:ore"),
    });
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.cost = vec![("beta:ore".into(), 100), ("energy".into(), 5000)];
    c.buildings.push(b);
    plan(vec![other, c]).unwrap();
}

#[test]
fn building_amount_and_domain_bounds_are_enforced() {
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.cost = vec![("energy".into(), 0)];
    c.buildings.push(b);
    error(c, "数量须在 1..=1e9");

    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.cost = vec![("energy".into(), 2_000_000_000)];
    c.buildings.push(b);
    error(c, "数量须在 1..=1e9");

    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.cost = Vec::new();
    c.buildings.push(b);
    error(c, "至少声明一种资源");

    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.hits_max = 0;
    c.buildings.push(b);
    error(c, "耐久上限须在");

    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.store_capacity = Some(0);
    c.buildings.push(b);
    error(c, "仓库容量须在");
}

#[test]
fn production_requires_capacity_and_valid_interval() {
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.production = Some(ProductionSpec {
        interval: 0,
        inputs: Vec::new(),
        outputs: vec![("energy".into(), 1)],
    });
    c.buildings.push(b);
    error(c, "生产周期须在");

    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.store_capacity = None;
    b.production = Some(ProductionSpec {
        interval: 10,
        inputs: Vec::new(),
        outputs: vec![("energy".into(), 1)],
    });
    c.buildings.push(b);
    error(c, "必须声明仓库容量");

    // 有输入无产出同样需要容量：输入没有仓库就无处存放。
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.store_capacity = None;
    b.production = Some(ProductionSpec {
        interval: 10,
        inputs: vec![("energy".into(), 1)],
        outputs: Vec::new(),
    });
    c.buildings.push(b);
    error(c, "必须声明仓库容量");

    // 免费生产（无输入）允许；产物入自有仓库。
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.production = Some(ProductionSpec {
        interval: 20,
        inputs: Vec::new(),
        outputs: vec![("energy".into(), 40)],
    });
    c.buildings.push(b);
    plan(vec![c]).unwrap();
}

#[test]
fn production_capacity_must_hold_a_full_cycle() {
    // 容量装不下单周期产出：运行期 used+produced>capacity 恒真，生产
    // 永久 no-op——冻结期拒绝。
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.store_capacity = Some(4);
    b.production = Some(ProductionSpec {
        interval: 10,
        inputs: Vec::new(),
        outputs: vec![("energy".into(), 5)],
    });
    c.buildings.push(b);
    error(c, "须能容纳单周期的全部输入");

    // 容量装不下单周期全部输入：输入永远攒不齐，同样拒绝。
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.store_capacity = Some(50);
    b.production = Some(ProductionSpec {
        interval: 10,
        inputs: vec![("energy".into(), 30), ("alpha:dust".into(), 30)],
        outputs: vec![("alpha:spark".into(), 1)],
    });
    c.resources.push(ResourceDescriptor {
        key: key("alpha:dust"),
    });
    c.resources.push(ResourceDescriptor {
        key: key("alpha:spark"),
    });
    c.buildings.push(b);
    error(c, "须能容纳单周期的全部输入");
}

#[test]
fn building_catalog_participates_in_plan_hash() {
    let base = plan(vec![module("alpha")]).unwrap();
    let mut with_building = module("alpha");
    with_building.buildings.push(building("alpha", "foundry"));
    let declared = plan(vec![with_building]).unwrap();
    assert_ne!(base.hash(), declared.hash());
    // 配方变化 → 计划身份变化（成本/耐久/寻路/生产均参与身份）。
    let mut other_hits = module("alpha");
    let mut b = building("alpha", "foundry");
    b.hits_max = 6000;
    other_hits.buildings.push(b);
    assert_ne!(
        declared.hash(),
        plan(vec![other_hits]).unwrap().hash(),
        "耐久变化应改变计划身份"
    );
}

#[test]
fn duplicate_resources_in_each_building_vector_are_rejected() {
    for vector in ["成本", "生产输入", "生产输出"] {
        let mut b = building("alpha", "foundry");
        b.production = Some(ProductionSpec {
            interval: 1,
            inputs: Vec::new(),
            outputs: vec![("H".into(), 1)],
        });
        // 重复项不相邻；生产输入的逐项预检会把同一份库存消费两次。
        let repeated = vec![
            ("energy".into(), 50),
            ("H".into(), 1),
            ("energy".into(), 50),
        ];
        match vector {
            "成本" => b.cost = repeated,
            "生产输入" => b.production.as_mut().unwrap().inputs = repeated,
            "生产输出" => b.production.as_mut().unwrap().outputs = repeated,
            _ => unreachable!(),
        }
        let mut c = module("alpha");
        c.buildings.push(b);
        error(c, &format!("建筑 {vector} 重复声明资源 \"energy\""));
    }
}

#[test]
fn building_resource_order_is_canonical_but_amounts_remain_identity() {
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.cost = vec![("energy".into(), 50), ("H".into(), 10)];
    b.production = Some(ProductionSpec {
        interval: 1,
        inputs: vec![("energy".into(), 50), ("H".into(), 10)],
        outputs: vec![("energy".into(), 20), ("H".into(), 5)],
    });
    c.buildings.push(b);
    let original = plan(vec![c.clone()]).unwrap();
    for vector in ["成本", "生产输入", "生产输出"] {
        let mut reordered = c.clone();
        let b = &mut reordered.buildings[0];
        let entries = match vector {
            "成本" => &mut b.cost,
            "生产输入" => &mut b.production.as_mut().unwrap().inputs,
            "生产输出" => &mut b.production.as_mut().unwrap().outputs,
            _ => unreachable!(),
        };
        entries.reverse();
        let frozen = plan(vec![reordered.clone()]).unwrap();
        assert_eq!(original.hash(), frozen.hash(), "{vector} 排列不改变身份");
        assert_eq!(original.buildings(), frozen.buildings());
        assert_eq!(frozen.buildings()[0].cost[0].0, "H");
        assert_eq!(
            frozen.buildings()[0].production.as_ref().unwrap().inputs[0].0,
            "H"
        );
        assert_eq!(
            frozen.buildings()[0].production.as_ref().unwrap().outputs[0].0,
            "H"
        );
        let b = &mut reordered.buildings[0];
        match vector {
            "成本" => b.cost[0].1 += 1,
            "生产输入" => b.production.as_mut().unwrap().inputs[0].1 += 1,
            "生产输出" => b.production.as_mut().unwrap().outputs[0].1 += 1,
            _ => unreachable!(),
        }
        assert_ne!(
            original.hash(),
            plan(vec![reordered]).unwrap().hash(),
            "{vector} 数量改变身份"
        );
    }
}

/// 声明一个 gauge 字段并把它（或指定绑定）登记进贡献 bindings 集
///（装载器对 JS 声明自动插入；直构贡献须自带，freeze 校验其存在）。
fn add_gauge_field(c: &mut Contribution, name: &str, binding: Option<&str>, default: FieldDefault) {
    let descriptor = gauge_field(name, binding, default);
    c.bindings.insert(descriptor.binding.clone());
    let b = c.buildings.last_mut().expect("先推入建筑再挂字段");
    b.fields.push(descriptor);
}

fn gauge_field(name: &str, binding: Option<&str>, default: FieldDefault) -> FieldDescriptor {
    FieldDescriptor {
        key: key(&format!("alpha:{name}")),
        schema: ValueSchema {
            kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 100 }),
            nullable: false,
        },
        default,
        visibility: Visibility::Public,
        binding: key(binding.unwrap_or(&format!("alpha:foundry.field.{name}"))),
    }
}

/// 建筑字段的冻结校验：键/绑定归属、默认值 schema、槽归属与
/// 嵌套限额。错误上下文以「建筑键 / 字段键」点名。
#[test]
fn building_fields_validate_ownership_defaults_and_slots() {
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(&mut c, "heat", None, FieldDefault::Value(json!(200)));
    error(c, "字段默认值不满足 schema");

    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(
        &mut c,
        "heat",
        Some("beta:foundry.field.heat"),
        FieldDefault::Missing,
    );
    error(c, "beta:foundry.field.heat");

    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(&mut c, "heat", None, FieldDefault::Missing);
    let b = c.buildings.last_mut().unwrap();
    b.fields.last_mut().unwrap().key = key("beta:heat");
    error(c, "beta:heat");

    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.lifecycle_slots.insert(key("beta:foundry.tick"));
    c.buildings.push(b);
    error(c, "beta:foundry.tick");

    // 可见性门在冻结层设防：JS 转换层之外直构 Rust 贡献同样拒绝
    //（记录直供无字段级裁切，owner/server-only 首片不开放）。
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(&mut c, "secret", None, FieldDefault::Missing);
    let b = c.buildings.last_mut().unwrap();
    b.fields.last_mut().unwrap().visibility = Visibility::Owner;
    error(c, "建筑字段可见性首片只支持 public");

    // 超深 schema：17 层嵌套数组越过默认限额 16。
    let mut kind = ValueKind::Bool;
    for _ in 0..17 {
        kind = ValueKind::Array {
            element: Box::new(ValueSchema {
                kind,
                nullable: false,
            }),
            max_len: 4,
        };
    }
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(&mut c, "deep", None, FieldDefault::Missing);
    let b = c.buildings.last_mut().unwrap();
    b.fields.last_mut().unwrap().schema = ValueSchema {
        kind,
        nullable: false,
    };
    error(c, "schema 嵌套超过限额");
}

/// 字段表参与计划身份：排列归一化不改变身份，默认值/可见性/槽集
/// 变化改变身份——冷恢复按计划哈希分派，身份漂移必须可检。
#[test]
fn building_fields_are_canonicalized_and_participate_in_identity() {
    let base = |default: FieldDefault, with_slot: bool| {
        let mut c = module("alpha");
        let mut b = building("alpha", "foundry");
        b.fields = vec![
            gauge_field("heat", None, default.clone()),
            gauge_field("cycles", None, FieldDefault::Missing),
        ];
        for field in &b.fields {
            c.bindings.insert(field.binding.clone());
        }
        if with_slot {
            let slot = key("alpha:foundry.tick");
            let process = key("alpha:foundry.tick.process");
            c.bindings.insert(process.clone());
            c.slots.push(SlotDescriptor {
                key: slot.clone(),
                stage: Stage::ObjectTick,
                native_binding: process,
            });
            b.lifecycle_slots.insert(slot);
        }
        c.buildings.push(b);
        c
    };
    let original = plan(vec![base(FieldDefault::Value(json!(7)), true)]).unwrap();
    let mut reordered = base(FieldDefault::Value(json!(7)), true);
    reordered.buildings[0].fields.reverse();
    assert_eq!(
        original.hash(),
        plan(vec![reordered]).unwrap().hash(),
        "字段排列不改变身份"
    );
    assert_eq!(
        original.buildings()[0].fields[0].key.as_str(),
        "alpha:cycles",
        "字段表按键稳定排序"
    );
    assert_ne!(
        original.hash(),
        plan(vec![base(FieldDefault::Value(json!(8)), false)])
            .unwrap()
            .hash(),
        "默认值变化改变身份"
    );
    let wider = |default: FieldDefault, with_slot: bool| {
        let mut c = base(default, with_slot);
        c.buildings[0].fields[0].schema = ValueSchema {
            kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 2000 }),
            nullable: false,
        };
        c
    };
    assert_ne!(
        original.hash(),
        plan(vec![wider(FieldDefault::Value(json!(7)), false)])
            .unwrap()
            .hash(),
        "schema 变化改变身份"
    );
    assert_ne!(
        original.hash(),
        plan(vec![base(FieldDefault::Value(json!(7)), false)])
            .unwrap()
            .hash(),
        "生命周期槽变化改变身份"
    );
    // 冻结期规范化：Float 域的整数默认落为 1.0 backing。
    let float_field = FieldDescriptor {
        schema: ValueSchema {
            kind: ValueKind::Number(NumberDomain::Float {
                min: 0.0,
                max: 10.0,
            }),
            nullable: false,
        },
        ..gauge_field("gauge", None, FieldDefault::Value(json!(1)))
    };
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.fields = vec![float_field];
    c.bindings.insert(b.fields[0].binding.clone());
    c.buildings.push(b);
    let frozen = plan(vec![c]).unwrap();
    assert_eq!(
        frozen.buildings()[0].fields[0].default,
        FieldDefault::Value(json!(1.0)),
        "整数字段默认经冻结规范化为浮点 backing"
    );
}

/// 冻结层第二遍目录门：完整 schema、重复字段键、生命周期槽声明与
/// 形状、字段绑定入集——Rust 直构贡献与 JS 装载同门。
#[test]
fn building_fields_second_pass_gates() {
    // 空枚举漏到运行期会把房间钉死在永久故障（任何写入恒拒）。
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(&mut c, "mode", None, FieldDefault::Missing);
    let b = c.buildings.last_mut().unwrap();
    b.fields.last_mut().unwrap().schema = ValueSchema {
        kind: ValueKind::Enum(BTreeSet::new()),
        nullable: false,
    };
    error(c, "枚举为空或成员长度无效");

    // ref 指向未知类型同样在第二遍拒绝。
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(&mut c, "link", None, FieldDefault::Missing);
    let b = c.buildings.last_mut().unwrap();
    b.fields.last_mut().unwrap().schema = ValueSchema {
        kind: ValueKind::ObjectRef(TypeRef {
            key: key("alpha:ghost"),
            schema_version: 1,
        }),
        nullable: false,
    };
    error(c, "alpha:ghost");

    // 同建筑重复字段键（目录层门之外的 freeze 权威层）。
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    add_gauge_field(&mut c, "heat", None, FieldDefault::Missing);
    add_gauge_field(&mut c, "heat", None, FieldDefault::Missing);
    error(c, "建筑字段重复声明 alpha:heat");

    // 字段绑定未入贡献 bindings 集。
    let mut c = module("alpha");
    c.buildings.push(building("alpha", "foundry"));
    let orphan = gauge_field("heat", None, FieldDefault::Missing);
    let b = c.buildings.last_mut().unwrap();
    b.fields.push(orphan);
    error(c, "未知绑定 alpha:foundry.field.heat");

    // 生命周期槽未在 slots 声明。
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    b.lifecycle_slots.insert(key("alpha:foundry.tick"));
    c.buildings.push(b);
    error(c, "未在 slots 中声明");

    // 槽键不从建筑键派生（即便已声明且阶段正确）。
    let mut c = module("alpha");
    let mut b = building("alpha", "foundry");
    let process = key("alpha:tick.process");
    c.bindings.insert(process.clone());
    c.slots.push(SlotDescriptor {
        key: key("alpha:other.tick"),
        stage: Stage::ObjectTick,
        native_binding: process,
    });
    b.lifecycle_slots.insert(key("alpha:other.tick"));
    c.buildings.push(b);
    error(c, "须为 alpha:foundry.tick");
}
