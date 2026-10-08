//! 能力携带唯一领域状态，避免布尔能力门与 Option 状态脱节。
use kk_engine::settle::ObjectJournal;
use kk_mod_api::builtin_capabilities::SlotCapability;
use std::cell::RefCell;
use std::rc::Rc;

/// 战斗链的运行时状态与首触日志。日志记录能力改向触达、且不在引擎
/// 预计算受影响集内的对象原始记录与延迟队列条目；拒绝时恢复。
pub(super) struct CombatChainState {
    pub(super) action: kk_engine::settle::creep::CombatAction,
    pub(super) consumed: bool,
    pub(super) retaliation: Option<kk_engine::settle::creep::RetaliationPolicy>,
    pub(super) journal: Rc<RefCell<ObjectJournal>>,
}

/// 建造族链的运行时状态与首触日志，与战斗链同型；触达对象不含延迟
/// 队列写入面（建造族目标均为工地/结构），条目仍按同构保存。
pub(super) struct BuildChainState {
    pub(super) action: kk_engine::settle::creep::BuildAction,
    pub(super) consumed: bool,
    pub(super) journal: Rc<RefCell<ObjectJournal>>,
}

pub(super) enum ChainCapability {
    Plain,
    Objects,
    Move,
    Combat(CombatChainState),
    Build(BuildChainState),
    Incoming(Rc<kk_engine::settle::incoming::IncomingDamage>),
    /// G09.c 结构受击候选（Mod 目标）。
    ModIncoming(Rc<kk_engine::settle::incoming::ModIncomingDamage>),
    Contest(Rc<kk_engine::settle::movement::contest::MoveContest>),
    Admission(Rc<kk_engine::settle::movement::admission::MoveAdmission>),
    Obstacle(Rc<kk_engine::settle::movement::obstacle::MoveObstacle>),
    Precheck(Rc<kk_engine::settle::movement::precheck::MovePrecheck>),
    PullRegistration(Rc<kk_engine::settle::movement::pull_registration::PullRegistration>),
    MoveCost(Rc<kk_engine::settle::movement::move_cost::MoveCost>),
    Resource(Rc<RefCell<ObjectJournal>>),
}

impl ChainCapability {
    /// 只有对象动作按槽键装配能力，其他调用变体携带自己的候选状态。
    pub(super) fn for_action(slot: &str) -> Self {
        match SlotCapability::for_key(slot) {
            Some(SlotCapability::Resource) => {
                Self::Resource(Rc::new(RefCell::new(ObjectJournal::default())))
            }
            Some(SlotCapability::Move) => Self::Move,
            Some(SlotCapability::Combat) => Self::Combat(CombatChainState {
                action: kk_engine::settle::creep::CombatAction::from_slot_key(slot)
                    .expect("战斗槽键已由 COMBAT_ACTION_SLOTS 判定"),
                consumed: false,
                retaliation: None,
                journal: Rc::new(RefCell::new(ObjectJournal::default())),
            }),
            Some(SlotCapability::Build) => Self::Build(BuildChainState {
                action: kk_engine::settle::creep::BuildAction::from_slot_key(slot)
                    .expect("建造族槽键已由 BUILD_ACTION_SLOTS 判定"),
                consumed: false,
                journal: Rc::new(RefCell::new(ObjectJournal::default())),
            }),
            _ => Self::Plain,
        }
    }

    pub(super) fn combat(&self) -> Option<&CombatChainState> {
        match self {
            Self::Combat(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn combat_mut(&mut self) -> Option<&mut CombatChainState> {
        match self {
            Self::Combat(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn build(&self) -> Option<&BuildChainState> {
        match self {
            Self::Build(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn build_mut(&mut self) -> Option<&mut BuildChainState> {
        match self {
            Self::Build(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn incoming(&self) -> Option<&Rc<kk_engine::settle::incoming::IncomingDamage>> {
        match self {
            Self::Incoming(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn mod_incoming(
        &self,
    ) -> Option<&Rc<kk_engine::settle::incoming::ModIncomingDamage>> {
        match self {
            Self::ModIncoming(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn contest(&self) -> Option<&Rc<kk_engine::settle::movement::contest::MoveContest>> {
        match self {
            Self::Contest(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn admission(
        &self,
    ) -> Option<&Rc<kk_engine::settle::movement::admission::MoveAdmission>> {
        match self {
            Self::Admission(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn obstacle(
        &self,
    ) -> Option<&Rc<kk_engine::settle::movement::obstacle::MoveObstacle>> {
        match self {
            Self::Obstacle(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn precheck(
        &self,
    ) -> Option<&Rc<kk_engine::settle::movement::precheck::MovePrecheck>> {
        match self {
            Self::Precheck(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn pull_registration(
        &self,
    ) -> Option<&Rc<kk_engine::settle::movement::pull_registration::PullRegistration>> {
        match self {
            Self::PullRegistration(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn move_cost(
        &self,
    ) -> Option<&Rc<kk_engine::settle::movement::move_cost::MoveCost>> {
        match self {
            Self::MoveCost(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn resource_journal(&self) -> Option<&Rc<RefCell<ObjectJournal>>> {
        match self {
            Self::Resource(state) => Some(state),
            _ => None,
        }
    }
}
