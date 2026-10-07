//! 能力携带唯一领域状态，避免布尔能力门与 Option 状态脱节。
use super::*;

pub(super) enum ChainCapability {
    Plain,
    Objects,
    Move,
    Combat(CombatChainState),
    Build(BuildChainState),
    Incoming(Rc<kk_engine::settle::incoming::IncomingDamage>),
    Contest(Rc<kk_engine::settle::movement::contest::MoveContest>),
    Admission(Rc<kk_engine::settle::movement::admission::MoveAdmission>),
    Obstacle(Rc<kk_engine::settle::movement::obstacle::MoveObstacle>),
    Precheck(Rc<kk_engine::settle::movement::precheck::MovePrecheck>),
    PullRegistration(Rc<kk_engine::settle::movement::pull_registration::PullRegistration>),
    MoveCost(Rc<kk_engine::settle::movement::move_cost::MoveCost>),
    Resource(Rc<RefCell<ObjectJournal>>),
}

impl ChainCapability {
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
