//! 实际链安装和诊断共用的停用过滤与原始中心回退。
use kk_mod_api::{ContributionPlan, Dispatch, Key, Stage};
use serde::Serialize;
use std::collections::BTreeSet;

/// 启动时交给 JS 的固定链描述，后续调用只传槽键与本次能力标识。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExecutionChain {
    pub key: String,
    pub before: Vec<String>,
    pub wraps: Vec<String>,
    pub center: String,
    pub after: Vec<String>,
    pub native_center: bool,
    pub decline_allowed: bool,
}

impl ExecutionChain {
    pub(crate) fn from_plan(
        plan: &ContributionPlan,
        contributions: &[kk_mod_api::Contribution],
        disabled: &BTreeSet<String>,
    ) -> Result<Vec<Self>, String> {
        // 原始中心来自声明，不从 replace 后的冻结 dispatch 反推。
        let builtin = kk_mod_api::builtin::link();
        let centers: std::collections::BTreeMap<_, _> = std::iter::once(&builtin)
            .chain(contributions)
            .flat_map(|contribution| &contribution.slots)
            .map(|slot| (&slot.key, &slot.native_binding))
            .collect();
        plan.slots()
            .iter()
            .filter_map(|slot| {
                let original = centers.get(&slot.key).copied();
                match original {
                    Some(original) => Self::for_slot(slot, original, disabled).transpose(),
                    None => Some(Err(format!("槽 {} 缺少原始中心", slot.key))),
                }
            })
            .collect()
    }

    pub(crate) fn for_slot(
        slot: &kk_mod_api::SlotPlan,
        original_center: &Key,
        disabled: &BTreeSet<String>,
    ) -> Result<Option<Self>, String> {
        let enabled = |key: &Key| !disabled.contains(key.namespace());
        if !enabled(&slot.key) {
            return Ok(None);
        }
        let (before, wraps, center, after) = match &slot.dispatch {
            Dispatch::Native(center) => (&[][..], &[][..], center, &[][..]),
            Dispatch::Chained {
                before,
                wraps,
                center,
                after,
            } => (
                before.as_slice(),
                wraps.as_slice(),
                center,
                after.as_slice(),
            ),
        };
        // 每层保留 JS 栈帧，显式限制防止依赖包数变成运行期栈溢出。
        if wraps.len() > 64 {
            return Err(format!("槽 {} 的 wrap 层数超过 64", slot.key));
        }
        let center = if enabled(center) {
            center
        } else {
            original_center
        };
        if !enabled(center) {
            return Err(format!("槽 {} 的原始中心已经停用", slot.key));
        }
        let names = |keys: &[Key]| {
            keys.iter()
                .filter(|key| enabled(key))
                .map(ToString::to_string)
                .collect()
        };
        Ok(Some(Self {
            key: slot.key.to_string(),
            before: names(before),
            wraps: names(wraps),
            center: center.to_string(),
            after: names(after),
            native_center: center.namespace() == "builtin",
            decline_allowed: slot.key.as_str() == kk_mod_api::builtin::INCOMING_DAMAGE_SLOT
                || slot.key.as_str() == kk_mod_api::builtin::MOVE_CONTEST_SLOT
                || slot.stage == Stage::ObjectAction
                    && (slot.key.namespace() != "builtin"
                        || kk_mod_api::builtin::RESOURCE_ACTION_SLOTS.contains(&slot.key.as_str())
                        || kk_mod_api::builtin::MOVE_ACTION_SLOTS.contains(&slot.key.as_str())
                        || kk_mod_api::builtin::COMBAT_ACTION_SLOTS.contains(&slot.key.as_str())
                        || kk_mod_api::builtin::BUILD_ACTION_SLOTS.contains(&slot.key.as_str())),
        }))
    }

    pub(crate) fn bindings(&self) -> BTreeSet<String> {
        self.before
            .iter()
            .chain(&self.wraps)
            .chain(&self.after)
            .chain((!self.native_center).then_some(&self.center))
            .cloned()
            .collect()
    }
}
