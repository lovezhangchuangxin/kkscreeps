//! tick 候选拥有数据；房间规则只在操作玩家状态或 ID 时短暂借用共享部分。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use kk_world::RoomState;
use serde_json::Value;

use super::{NOTIFY_RECORD_CAP, NotifyRecord, PlayerState, SettleError, TickInput, market};
use crate::intent::IntentBatch;

/// 候选发布时的拥有数据：玩家候选、ID 游标与 Mod 状态候选。
pub type PublishedCore = (BTreeMap<String, PlayerState>, u64, BTreeMap<String, Value>);

pub(crate) struct SharedState {
    pub(crate) next_id: u64,
    pub(crate) players: BTreeMap<String, PlayerState>,
    /// 各 Mod 命名空间的候选状态；随候选一起发布或丢弃。服务端私有，
    /// 不进玩家投影。
    pub(crate) mod_states: BTreeMap<String, Value>,
}

/// 本轮共享的玩家候选和 ID 分配器，不引用引擎中的已提交数据。
/// 克隆仅共享同一候选；所有操作在返回前释放内部借用。
/// 发布前必须释放房间规则持有的克隆，才能取回拥有的数据。
#[derive(Clone)]
pub struct TickState {
    pub(crate) inner: Rc<RefCell<SharedState>>,
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

    pub(crate) fn allocate_id(&self) -> u64 {
        let mut state = self.inner.borrow_mut();
        let id = state.next_id;
        state.next_id = id.saturating_add(1);
        id
    }

    pub(crate) fn add_gcl(&self, player: &str, amount: i64) {
        self.inner
            .borrow_mut()
            .players
            .entry(player.into())
            .or_default()
            .gcl += amount;
    }

    pub(crate) fn claim_state(&self, player: &str) -> (usize, i64) {
        self.inner
            .borrow()
            .players
            .get(player)
            .map(|p| (p.rooms.len(), p.gcl))
            .unwrap_or_default()
    }

    /// 消费唯一持有者。仍有规则持有克隆时返回错误，不复制或发布共享候选。
    pub fn into_parts(self) -> Result<PublishedCore, SettleError> {
        let state = Rc::try_unwrap(self.inner)
            .map_err(|_| SettleError("候选仍被房间规则持有，不能发布".into()))?
            .into_inner();
        Ok((state.players, state.next_id, state.mod_states))
    }
}

/// 引擎本轮的全部结算候选与固定输入；失败时直接丢弃。
/// 协调器提供的 RawMemory、segments 和 CPU 桶值在校验阶段合并。
pub(crate) struct OwnedTickTxn {
    pub(crate) time: u64,
    pub(crate) input: TickInput,
    pub(crate) room_names: Vec<String>,
    pub(crate) state: TickState,
    pub(crate) batch: IntentBatch,
    pub(crate) rooms: Vec<(String, RoomState)>,
    pub(crate) events: Vec<Value>,
    pub(crate) market: market::MarketState,
}

/// 已完成派生状态处理且不再共享的发布数据。
pub(crate) struct PreparedTick {
    pub(crate) rooms: Vec<(String, RoomState)>,
    pub(crate) players: BTreeMap<String, PlayerState>,
    pub(crate) next_id: u64,
    pub(crate) events: Vec<Value>,
    pub(crate) market: market::MarketState,
    pub(crate) mod_states: BTreeMap<String, Value>,
}

impl OwnedTickTxn {
    pub(crate) fn into_prepared(self) -> Result<PreparedTick, SettleError> {
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
    fn room_handles_share_ids_and_player_updates() {
        let state = TickState::new(BTreeMap::new(), 10);
        let first_room = state.clone();
        let second_room = state.clone();
        assert_eq!(first_room.allocate_id(), 10);
        first_room.add_gcl("p1", 7);
        assert_eq!(second_room.allocate_id(), 11);
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
        assert_eq!(retained.allocate_id(), 1);
    }
}
