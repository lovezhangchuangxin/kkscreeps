//! 构造资源存储的 JSON 字段，只加入数量大于 0 的资源。
//! 沙箱投影另行补充 energy 字段，即使其值为 0；查询快照不补。

use serde_json::{Value, json};

/// 将 energy 和 ghodium 中的正量资源写入 JSON 对象。
/// 查询快照的过滤依据：screeps-ok adapters/vanilla/snapshots.ts 的 snapStore。
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

/// 合并额外资源中的正量条目，同名键覆盖现有值。
/// 调用方应保证额外资源名唯一，且不与 energy、G 专用字段重复。
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
