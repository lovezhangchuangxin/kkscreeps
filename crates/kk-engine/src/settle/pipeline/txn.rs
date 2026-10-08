//! tick 候选拥有数据；房间规则只在操作玩家状态或 ID 时短暂借用共享部分。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use kk_world::RoomState;
use serde_json::Value;

use crate::intent::IntentBatch;
use crate::settle::{NOTIFY_RECORD_CAP, NotifyRecord, PlayerState, SettleError, TickInput, market};

/// 候选发布时的拥有数据：玩家候选、ID 游标与 Mod 状态候选。
pub type PublishedCore = (BTreeMap<String, PlayerState>, u64, BTreeMap<String, Value>);

pub(in crate::settle) struct SharedState {
    /// 宿主不变式故障不能被 JS 捕获或链内 decline/游标回滚清除。
    fatal_error: Option<SettleError>,
    pub(in crate::settle) next_id: u64,
    pub(in crate::settle) players: BTreeMap<String, PlayerState>,
    /// 各 Mod 命名空间的候选状态；随候选一起发布或丢弃。服务端私有，
    /// 不进玩家投影。
    pub(in crate::settle) mod_states: BTreeMap<String, Value>,
}

/// 本轮共享的玩家候选和 ID 分配器，不引用引擎中的已提交数据。
/// 克隆仅共享同一候选；所有操作在返回前释放内部借用。
/// 发布前必须释放房间规则持有的克隆，才能取回拥有的数据。
#[derive(Clone)]
pub struct TickState {
    pub(in crate::settle) inner: Rc<RefCell<SharedState>>,
}

impl TickState {
    pub fn new(players: BTreeMap<String, PlayerState>, next_id: u64) -> Self {
        Self::with_mod_states(players, next_id, BTreeMap::new())
    }

    /// 携带已提交 Mod 状态构造本轮候选；发布时整体换回。
    pub fn with_mod_states(
        players: BTreeMap<String, PlayerState>,
        next_id: u64,
        mod_states: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(SharedState {
                fatal_error: None,
                next_id,
                players,
                mod_states,
            })),
        }
    }

    /// 读取某命名空间的候选状态；未初始化的命名空间返回 null。
    pub fn mod_state(&self, namespace: &str) -> Value {
        self.inner
            .borrow()
            .mod_states
            .get(namespace)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// 写回某命名空间的候选状态；发布前对失败轮不可见。
    pub fn set_mod_state(&self, namespace: &str, value: Value) {
        self.inner
            .borrow_mut()
            .mod_states
            .insert(namespace.to_string(), value);
    }

    pub(in crate::settle) fn allocate_id(&self) -> Result<u64, SettleError> {
        let mut state = self.inner.borrow_mut();
        if let Some(error) = &state.fatal_error {
            return Err(error.clone());
        }
        let id = state.next_id;
        if id == 0 || id == u64::MAX {
            let error = SettleError("对象 ID 游标非法或已耗尽".into());
            state.fatal_error = Some(error.clone());
            return Err(error);
        }
        state.next_id = id + 1;
        Ok(id)
    }

    pub(in crate::settle) fn record_fatal(&self, error: SettleError) -> SettleError {
        self.inner
            .borrow_mut()
            .fatal_error
            .get_or_insert(error)
            .clone()
    }

    pub(in crate::settle) fn check_fatal(&self) -> Result<(), SettleError> {
        match &self.inner.borrow().fatal_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// 当前 ID 游标；供链内快照记录，拒绝回滚插入对象时一并恢复。
    pub(in crate::settle) fn peek_next_id(&self) -> u64 {
        self.inner.borrow().next_id
    }

    /// 回滚 ID 游标；仅允许在已删除本链全部新插入对象后调用，
    /// 否则会出现游标复用。游标只能回退到快照值，不得前进。
    pub(in crate::settle) fn restore_next_id(&self, snapshot: u64) {
        let mut state = self.inner.borrow_mut();
        if state.next_id > snapshot {
            state.next_id = snapshot;
        }
    }

    pub(in crate::settle) fn add_gcl(&self, player: &str, amount: i64) {
        self.inner
            .borrow_mut()
            .players
            .entry(player.into())
            .or_default()
            .gcl += amount;
    }

    /// powerSpawn.processPower 的账号 power 累计；房间相位直写共享候选，
    /// 与市场阶段消费同一份 players（发布或丢弃随整轮候选）。
    pub(in crate::settle) fn add_power(&self, player: &str, amount: i64) {
        self.inner
            .borrow_mut()
            .players
            .entry(player.into())
            .or_default()
            .power += amount;
    }

    pub(in crate::settle) fn claim_state(&self, player: &str) -> (usize, i64) {
        self.inner
            .borrow()
            .players
            .get(player)
            .map(|p| (p.rooms.len(), p.gcl))
            .unwrap_or_default()
    }

    /// 消费唯一持有者。仍有规则持有克隆时返回错误，不复制或发布共享候选。
    pub fn into_parts(self) -> Result<PublishedCore, SettleError> {
        self.check_fatal()?;
        let state = Rc::try_unwrap(self.inner)
            .map_err(|_| SettleError("候选仍被房间规则持有，不能发布".into()))?
            .into_inner();
        Ok((state.players, state.next_id, state.mod_states))
    }
}

/// 引擎本轮的全部结算候选与固定输入；失败时直接丢弃。
/// 协调器提供的 RawMemory、segments 和 CPU 桶值在校验阶段合并。
pub(in crate::settle) struct OwnedTickTxn {
    pub(in crate::settle) time: u64,
    pub(in crate::settle) input: TickInput,
    pub(in crate::settle) room_names: Vec<String>,
    pub(in crate::settle) state: TickState,
    pub(in crate::settle) batch: IntentBatch,
    pub(in crate::settle) rooms: Vec<(String, RoomState)>,
    pub(in crate::settle) events: Vec<Value>,
    pub(in crate::settle) market: market::MarketState,
}

/// 已完成派生状态处理且不再共享的发布数据。
pub(in crate::settle) struct PreparedTick {
    pub(in crate::settle) rooms: Vec<(String, RoomState)>,
    pub(in crate::settle) players: BTreeMap<String, PlayerState>,
    pub(in crate::settle) next_id: u64,
    pub(in crate::settle) events: Vec<Value>,
    pub(in crate::settle) market: market::MarketState,
    pub(in crate::settle) mod_states: BTreeMap<String, Value>,
}

impl OwnedTickTxn {
    pub(in crate::settle) fn into_prepared(self) -> Result<PreparedTick, SettleError> {
        let (mut players, next_id, mod_states) = self.state.into_parts()?;
        for (player, intents) in &self.batch.users {
            if intents.notify.is_empty() {
                continue;
            }
            let state = players.entry(player.clone()).or_default();
            for n in &intents.notify {
                state.notifications.push(NotifyRecord {
                    message: n.message.clone(),
                    group_interval: n.group_interval,
                    tick: self.time,
                });
            }
            let len = state.notifications.len();
            if len > NOTIFY_RECORD_CAP {
                state.notifications.drain(0..len - NOTIFY_RECORD_CAP);
            }
        }
        // 结算期间保持上轮的 rooms；所有 claim 完成后才按房间候选重建。
        for state in players.values_mut() {
            state.rooms.clear();
        }
        for (_, room) in &self.rooms {
            if let Some(owner) = room.objects.values().find_map(|o| match o {
                kk_schema::room::RoomObject::Controller(c) => c.owner.as_ref(),
                _ => None,
            }) && let Some(state) = players.get_mut(owner)
            {
                state.rooms.insert(room.name.clone());
            }
        }
        Ok(PreparedTick {
            rooms: self.rooms,
            players,
            next_id,
            events: self.events,
            market: self.market,
            mod_states,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_error_survives_caught_error_and_chain_cursor_rollback() {
        let state = TickState::new(BTreeMap::new(), u64::MAX - 1);
        assert_eq!(state.allocate_id().unwrap(), u64::MAX - 1);
        assert!(state.allocate_id().is_err());
        state.restore_next_id(1);
        assert!(state.allocate_id().is_err());
        assert!(state.into_parts().is_err());
        let zero = TickState::new(BTreeMap::new(), 0);
        assert!(zero.allocate_id().is_err());
    }

    #[test]
    fn room_handles_share_ids_and_player_updates() {
        let state = TickState::new(BTreeMap::new(), 10);
        let first_room = state.clone();
        let second_room = state.clone();
        assert_eq!(first_room.allocate_id().unwrap(), 10);
        first_room.add_gcl("p1", 7);
        assert_eq!(second_room.allocate_id().unwrap(), 11);
        assert_eq!(second_room.claim_state("p1"), (0, 7));
        second_room.add_gcl("p1", 3);
        drop((first_room, second_room));
        let (players, next, mod_states) = state.into_parts().unwrap();
        assert_eq!(players["p1"].gcl, 10);
        assert_eq!(next, 12);
        assert!(mod_states.is_empty());
    }

    #[test]
    fn retained_handle_prevents_publication() {
        let state = TickState::new(BTreeMap::new(), 1);
        let retained = state.clone();
        assert!(state.into_parts().is_err());
        assert_eq!(retained.allocate_id().unwrap(), 1);
    }
}
