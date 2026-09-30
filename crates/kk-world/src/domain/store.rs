//! store 领域语义：正量键集共核（两投影的契约折叠各自定型）。

use serde_json::{Value, json};

/// store 双槽的正量键集（官方 store getter 只暴露正量资源——vanilla
/// snapStore amount>0 才入表；store.js:19-25 按资源名键构造）。两投影共此
/// 核心（M2B P4 单源）；世界投影再无条件补 energy 键（M0 契约恒投影）。
pub(crate) fn store_positive_entries(
    store: &kk_schema::room::Store,
) -> serde_json::Map<String, Value> {
    let mut v = serde_json::Map::new();
    if store.energy > 0 {
        v.insert("energy".into(), json!(store.energy));
    }
    if store.ghodium > 0 {
        v.insert(
            kk_core::constants::RESOURCE_GHODIUM.into(),
            json!(store.ghodium),
        );
    }
    v
}

/// 额外资源栈的正量并入（B3：tombstone/ruin/creep 的 store_extra——boost
/// 矿物等命名槽，amount>0 才入表）。两投影同 loop 共源（M2B P4）。
pub(crate) fn apply_store_extra(
    v: &mut serde_json::Map<String, Value>,
    extra: &[kk_schema::room::ResourceStack],
) {
    for stack in extra {
        if stack.amount > 0 {
            v.insert(stack.resource.clone(), json!(stack.amount));
        }
    }
}
