use std::collections::{BTreeMap, BTreeSet};

use crate::{validate, *};

pub(crate) fn handler_plans(
    slots: &BTreeMap<Key, SlotDescriptor>,
    handlers: &BTreeMap<Key, HandlerDescriptor>,
    bindings: &BTreeSet<Key>,
) -> Result<Vec<SlotPlan>, FreezeError> {
    for h in handlers.values() {
        validate::binding(&h.binding, bindings, h.key.as_str())?;
        if !slots.contains_key(&h.slot) {
            return Err(FreezeError::new(
                h.key.to_string(),
                format!("未知处理器入口 {}", h.slot),
            ));
        }
        for key in &h.depends_on {
            let dependency = handlers.get(key).ok_or_else(|| {
                FreezeError::new(h.key.to_string(), format!("未知处理器依赖 {key}"))
            })?;
            if dependency.slot != h.slot || dependency.kind != h.kind {
                return Err(FreezeError::new(
                    h.key.to_string(),
                    format!(
                        "处理器依赖 {key} 指向 {} / {:?}，须在同一入口和同一种 kind 内",
                        dependency.slot, dependency.kind
                    ),
                ));
            }
        }
    }
    let mut result = Vec::new();
    for slot in slots.values() {
        let mut ordered: BTreeMap<HandlerKind, Vec<Key>> = BTreeMap::new();
        for kind in [
            HandlerKind::Before,
            HandlerKind::Wrap,
            HandlerKind::Replace,
            HandlerKind::After,
        ] {
            let mut pending: BTreeSet<Key> = handlers
                .values()
                .filter(|h| h.slot == slot.key && h.kind == kind)
                .map(|h| h.key.clone())
                .collect();
            let mut done = BTreeSet::new();
            let mut sequence = Vec::new();
            while !pending.is_empty() {
                let next = pending
                    .iter()
                    .find(|k| handlers[*k].depends_on.is_subset(&done))
                    .cloned()
                    .ok_or_else(|| {
                        FreezeError::new(
                            slot.key.to_string(),
                            format!(
                                "{kind:?} 处理器依赖有环（含下游）：{}",
                                pending
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        )
                    })?;
                pending.remove(&next);
                done.insert(next.clone());
                sequence.push(handlers[&next].binding.clone());
            }
            ordered.insert(kind, sequence);
        }
        let mut replacements = ordered
            .remove(&HandlerKind::Replace)
            .expect("已建立 kind 目录");
        if replacements.len() > 1 {
            return Err(FreezeError::new(
                slot.key.to_string(),
                format!(
                    "多个 replace 处理器冲突：{}",
                    replacements
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        let has_handlers = handlers.values().any(|h| h.slot == slot.key);
        let dispatch = if !has_handlers {
            Dispatch::Native(slot.native_binding.clone())
        } else {
            let mut after = ordered
                .remove(&HandlerKind::After)
                .expect("已建立 kind 目录");
            after.reverse();
            Dispatch::Chained {
                before: ordered
                    .remove(&HandlerKind::Before)
                    .expect("已建立 kind 目录"),
                wraps: ordered
                    .remove(&HandlerKind::Wrap)
                    .expect("已建立 kind 目录"),
                center: replacements
                    .pop()
                    .unwrap_or_else(|| slot.native_binding.clone()),
                after,
            }
        };
        result.push(SlotPlan {
            key: slot.key.clone(),
            stage: slot.stage,
            dispatch,
        });
    }
    Ok(result)
}
