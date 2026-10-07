//! 命名空间资源声明的冻结语义：合并、归属、重复与计划身份。
use super::*;
use kk_mod_api::resources::ResourceDescriptor;

fn resource(namespace: &str, name: &str) -> ResourceDescriptor {
    ResourceDescriptor {
        key: Key::new(format!("{namespace}:{name}")).unwrap(),
    }
}

#[test]
fn declared_resources_merge_into_sorted_plan_catalog() {
    let mut a = module("alpha");
    a.resources.push(resource("alpha", "zeta"));
    let mut b = module("beta");
    b.resources.push(resource("beta", "alpha"));
    let plan = plan(vec![builtin::link(), a, b]).unwrap();
    let keys: Vec<&str> = plan.resources().iter().map(Key::as_str).collect();
    assert_eq!(keys, ["alpha:zeta", "beta:alpha"]);
    // 内建资源不在声明目录中；目录由 ResourceCatalog 合并提供。
    assert!(!keys.contains(&"energy"));
}

#[test]
fn resource_key_must_be_owned_by_namespace() {
    let mut c = module("alpha");
    c.resources.push(resource("beta", "spark"));
    error(c, "不能定义其他命名空间的键 beta:spark");
}

#[test]
fn duplicate_resource_declaration_is_rejected() {
    let mut c = module("alpha");
    c.resources.push(resource("alpha", "spark"));
    c.resources.push(resource("alpha", "spark"));
    error(c, "重复声明的资源");
}

#[test]
fn builtin_resource_names_cannot_collide_with_declared_keys() {
    // 键要求 namespace:name（含冒号）且命名空间归本包；内建资源名均不
    // 含冒号，结构上不可能撞名。这里锁定该前提，防止键规则放宽后
    // 新资源悄悄覆盖内建资源身份。
    for name in kk_core::constants::RESOURCES_ALL {
        assert!(!name.contains(':'), "内建资源名 {name} 含冒号");
    }
    for name in kk_core::constants::INTERSHARD_RESOURCES {
        assert!(!name.contains(':'), "跨 shard 资源名 {name} 含冒号");
    }
}

#[test]
fn resource_catalog_participates_in_plan_hash() {
    let base = plan(vec![module("alpha")]).unwrap();
    let mut with_resource = module("alpha");
    with_resource.resources.push(resource("alpha", "spark"));
    let declared = plan(vec![with_resource]).unwrap();
    assert_ne!(base.hash(), declared.hash());
    // 资源名不同 → 计划身份不同。
    let mut other = module("alpha");
    other.resources.push(resource("alpha", "ember"));
    assert_ne!(
        declared.hash(),
        plan(vec![other]).unwrap().hash(),
        "不同资源名应改变计划指纹"
    );
}
