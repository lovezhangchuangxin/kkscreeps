//! 一次正常槽调用的提交/拒绝边界；故障仍由引擎丢弃整轮候选。
use super::capability::ChainCapability;
use kk_engine::settle::{ChainSnapshot, ObjectJournal, RoomCtx};
use kk_mod_api::SlotOutcome;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub(super) type StagedStates = Rc<RefCell<BTreeMap<String, Value>>>;

enum Rollback {
    Unavailable,
    /// 数值/移动候选与内建动作的局部拒绝由调用方引擎恢复。
    EngineOwned,
    Journal(Rc<RefCell<ObjectJournal>>),
    Objects(ChainSnapshot),
}

pub(super) struct Completion {
    rollback: Rollback,
    staged_states: Option<StagedStates>,
}

impl Completion {
    pub(super) fn prepare(ctx: &RoomCtx, capability: &ChainCapability) -> Self {
        let rollback = match capability {
            ChainCapability::Plain => Rollback::Unavailable,
            ChainCapability::Objects => Rollback::Objects(ctx.borrow_mut().snapshot_mod_objects()),
            ChainCapability::Resource(journal) => Rollback::Journal(journal.clone()),
            ChainCapability::Combat(state) => Rollback::Journal(state.journal.clone()),
            ChainCapability::Build(state) => Rollback::Journal(state.journal.clone()),
            ChainCapability::Move
            | ChainCapability::Incoming(_)
            | ChainCapability::ModIncoming(_)
            | ChainCapability::Contest(_)
            | ChainCapability::Admission(_)
            | ChainCapability::Obstacle(_)
            | ChainCapability::Precheck(_)
            | ChainCapability::PullRegistration(_)
            | ChainCapability::MoveCost(_) => Rollback::EngineOwned,
        };
        let staged_states = (!matches!(&rollback, Rollback::Unavailable))
            .then(|| Rc::new(RefCell::new(BTreeMap::new())));
        Self {
            rollback,
            staged_states,
        }
    }

    pub(super) fn staged_states(&self) -> Option<StagedStates> {
        self.staged_states.clone()
    }

    /// 不在 Drop 中恢复：异常、超限或基础设施失败须保持整轮故障边界。
    pub(super) fn finish(self, ctx: &RoomCtx, outcome: &SlotOutcome) -> Result<(), String> {
        match outcome {
            SlotOutcome::Completed => {
                if matches!(&self.rollback, Rollback::Objects(_)) {
                    ctx.borrow_mut().finish_chain_snapshot();
                }
                if let Some(states) = self.staged_states {
                    let states = std::mem::take(&mut *states.borrow_mut());
                    for (namespace, value) in states {
                        ctx.borrow().state.set_mod_state(&namespace, value);
                    }
                }
            }
            SlotOutcome::Declined(_) => match self.rollback {
                Rollback::Unavailable => return Err("该槽没有正常拒绝的局部恢复边界".into()),
                Rollback::EngineOwned => {}
                Rollback::Journal(journal) => journal.borrow_mut().restore(&mut ctx.borrow_mut()),
                Rollback::Objects(snapshot) => ctx.borrow_mut().restore_chain_snapshot(snapshot),
            },
        }
        Ok(())
    }
}
