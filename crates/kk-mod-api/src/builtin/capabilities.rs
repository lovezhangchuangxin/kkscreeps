//! 槽能力分类只由内建元数据表导出；不参与计划序列化或 PlanHash。
use crate::builtin::*;
use crate::{SlotPlan, Stage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotCapability {
    Resource,
    Move,
    Combat,
    Build,
    Incoming,
    Contest,
    Admission,
    Obstacle,
    Precheck,
    PullRegistration,
    MoveCost,
}
impl SlotCapability {
    pub fn for_key(key: &str) -> Option<Self> {
        if RESOURCE_ACTION_SLOTS.contains(&key) {
            Some(Self::Resource)
        } else if MOVE_ACTION_SLOTS.contains(&key) {
            Some(Self::Move)
        } else if COMBAT_ACTION_SLOTS.contains(&key) {
            Some(Self::Combat)
        } else if BUILD_ACTION_SLOTS.contains(&key) {
            Some(Self::Build)
        } else {
            match key {
                INCOMING_DAMAGE_SLOT => Some(Self::Incoming),
                MOVE_CONTEST_SLOT => Some(Self::Contest),
                MOVE_ADMISSION_SLOT => Some(Self::Admission),
                MOVE_OBSTACLE_SLOT => Some(Self::Obstacle),
                MOVE_PRECHECK_SLOT => Some(Self::Precheck),
                MOVE_PULL_REGISTRATION_SLOT => Some(Self::PullRegistration),
                MOVE_COST_SLOT => Some(Self::MoveCost),
                _ => None,
            }
        }
    }
}

pub fn decline_allowed(slot: &SlotPlan) -> bool {
    SlotCapability::for_key(slot.key.as_str()).is_some()
        || slot.stage == Stage::ObjectAction && slot.key.namespace() != "builtin"
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capability_catalog_covers_every_rejectable_builtin_slot_once() {
        let mut classified = std::collections::BTreeSet::new();
        for (keys, capability) in [
            (RESOURCE_ACTION_SLOTS.as_slice(), SlotCapability::Resource),
            (MOVE_ACTION_SLOTS.as_slice(), SlotCapability::Move),
            (COMBAT_ACTION_SLOTS.as_slice(), SlotCapability::Combat),
            (BUILD_ACTION_SLOTS.as_slice(), SlotCapability::Build),
        ] {
            for key in keys {
                assert!(classified.insert(*key), "能力组不得重叠：{key}");
                assert_eq!(SlotCapability::for_key(key), Some(capability));
            }
        }
        let typed = SLOT_KEYS
            .iter()
            .filter(|key| SlotCapability::for_key(key).is_some())
            .count();
        assert_eq!(typed, 19);
        assert_eq!(SlotCapability::for_key("builtin:link.tick"), None);
        assert_eq!(SlotCapability::for_key("foreign:action"), None);
    }
}
