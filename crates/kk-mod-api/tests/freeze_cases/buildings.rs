//! 建筑声明的冻结语义：归属、重复、配方校验与计划身份。
use super::*;
use kk_mod_api::buildings::{BuildingDescriptor, BuildingPathing, ProductionSpec};
use kk_mod_api::resources::ResourceDescriptor;

fn building(namespace: &str, name: &str) -> BuildingDescriptor {
    BuildingDescriptor {
        key: key(&format!("{namespace}:{name}")),
        player_class: "Foundry".into(),
        cost: vec![("energy".into(), 5000)],
        hits_max: 5000,
        pathing: BuildingPathing::Obstacle,
        store_capacity: Some(1000),
        production: None,
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
    assert_eq!(keys, ["alpha:zeta", "beta:alpha"]);
    // 键含冒号，结构上不可能与内建结构名撞车。
    assert!(!keys.iter().any(|k| !k.contains(':')));
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
