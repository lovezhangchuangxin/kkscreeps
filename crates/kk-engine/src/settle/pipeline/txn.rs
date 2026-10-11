//! tick 候选拥有数据；房间规则只在操作玩家状态或 ID 时短暂借用共享部分。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

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
    /// 跨房耦合计数（KK_SETTLE_DIAG 取证；随既有借用累加，不另加锁）。
    pub(in crate::settle) diag: crate::settle::diag::DiagCounters,
    /// 逐房并行段的串行回退旗标：ID 分配/回退、claim 读候选任一触碰即
    /// 置位（这些操作的语义依赖房序，见 settle_rooms_stage）。逐拍生命周期。
    pub(in crate::settle) serial_only: bool,
}

/// 共享态耦合快照（并行逐房结算的串行回退用，见 TickState 方法）。
pub(in crate::settle) struct CouplingSnapshot {
    gcl_power: BTreeMap<String, (i64, i64)>,
    next_id: u64,
    fatal: Option<SettleError>,
    diag: crate::settle::diag::DiagCounters,
}

/// 本轮共享的玩家候选和 ID 分配器，不引用引擎中的已提交数据。
/// 克隆仅共享同一候选；所有操作在返回前释放内部借用。
/// 发布前必须释放房间规则持有的克隆，才能取回拥有的数据。
#[derive(Clone)]
pub struct TickState {
    /// 共享候选以 Arc<Mutex> 承载：逐房并行结算时跨工作线程共享（add_gcl/
    /// add_power 等短临界区，无嵌套加锁路径）。单线程串行路径同款语义。
    pub(in crate::settle) inner: Arc<Mutex<SharedState>>,
    /// 本拍处理器随机源（Engine 在 prepare 时带入，mineral_tick 消费）。
    pub(in crate::settle) random: crate::settle::ProcessorRandom,
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
            inner: Arc::new(Mutex::new(SharedState {
                fatal_error: None,
                next_id,
                players,
                mod_states,
                diag: crate::settle::diag::DiagCounters::default(),
                serial_only: false,
            })),
            random: crate::settle::ProcessorRandom::default(),
        }
    }

    /// 附加处理器随机源（Engine prepare 时调用；默认构造为 Derived）。
    pub(in crate::settle) fn with_random(mut self, random: crate::settle::ProcessorRandom) -> Self {
        self.random = random;
        self
    }

    /// 读取某命名空间的候选状态；未初始化的命名空间返回 null。
    pub fn mod_state(&self, namespace: &str) -> Value {
        self.inner
            .lock()
            .expect("共享候选锁")
            .mod_states
            .get(namespace)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// 写回某命名空间的候选状态；发布前对失败轮不可见。
    pub fn set_mod_state(&self, namespace: &str, value: Value) {
        self.inner
            .lock()
            .expect("共享候选锁")
            .mod_states
            .insert(namespace.to_string(), value);
    }

    pub(in crate::settle) fn allocate_id(&self) -> Result<u64, SettleError> {
        let mut state = self.inner.lock().expect("共享候选锁");
        if let Some(error) = &state.fatal_error {
            return Err(error.clone());
        }
        let id = state.next_id;
        state.diag.id_allocs += 1;
        state.serial_only = true;
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
            .lock()
            .expect("共享候选锁")
            .fatal_error
            .get_or_insert(error)
            .clone()
    }

    pub(in crate::settle) fn check_fatal(&self) -> Result<(), SettleError> {
        match &self.inner.lock().expect("共享候选锁").fatal_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// 当前 ID 游标；供链内快照记录，拒绝回滚插入对象时一并恢复。
    pub(in crate::settle) fn peek_next_id(&self) -> u64 {
        self.inner.lock().expect("共享候选锁").next_id
    }

    /// 回滚 ID 游标；仅允许在已删除本链全部新插入对象后调用，
    /// 否则会出现游标复用。游标只能回退到快照值，不得前进。
    pub(in crate::settle) fn restore_next_id(&self, snapshot: u64) {
        let mut state = self.inner.lock().expect("共享候选锁");
        state.diag.id_restores += 1;
        state.serial_only = true;
        if state.next_id > snapshot {
            state.next_id = snapshot;
        }
    }

    pub(in crate::settle) fn add_gcl(&self, player: &str, amount: i64) {
        let mut state = self.inner.lock().expect("共享候选锁");
        state.diag.gcl_adds += 1;
        state.players.entry(player.into()).or_default().gcl += amount;
    }

    /// powerSpawn.processPower 的账号 power 累计；房间相位直写共享候选，
    /// 与市场阶段消费同一份 players（发布或丢弃随整轮候选）。
    pub(in crate::settle) fn add_power(&self, player: &str, amount: i64) {
        let mut state = self.inner.lock().expect("共享候选锁");
        state.diag.power_adds += 1;
        state.players.entry(player.into()).or_default().power += amount;
    }

    pub(in crate::settle) fn claim_state(&self, player: &str) -> (usize, i64) {
        let mut state = self.inner.lock().expect("共享候选锁");
        state.diag.claim_calls += 1;
        state.serial_only = true;
        state
            .players
            .get(player)
            .map(|p| (p.rooms.len(), p.gcl))
            .unwrap_or_default()
    }

    /// 并行逐房结算前的共享态耦合快照：并行段只可能触碰 gcl/power 累加、
    /// ID 游标、fatal 与诊断计数（其余字段晚期阶段才动）；触发串行回退时
    /// 恢复后重做。
    pub(in crate::settle) fn coupling_snapshot(&self) -> CouplingSnapshot {
        let s = self.inner.lock().expect("共享候选锁");
        CouplingSnapshot {
            gcl_power: s
                .players
                .iter()
                .map(|(k, p)| (k.clone(), (p.gcl, p.power)))
                .collect(),
            next_id: s.next_id,
            fatal: s.fatal_error.clone(),
            diag: s.diag,
        }
    }

    /// 恢复共享态耦合量到快照（串行回退用；并行段新建的玩家条目只可能
    /// 来自 gcl/power 累加，整体移除）。
    pub(in crate::settle) fn restore_coupling(&self, snapshot: CouplingSnapshot) {
        let mut s = self.inner.lock().expect("共享候选锁");
        s.players.retain(|k, _| snapshot.gcl_power.contains_key(k));
        for (k, (gcl, power)) in snapshot.gcl_power {
            if let Some(p) = s.players.get_mut(&k) {
                p.gcl = gcl;
                p.power = power;
            }
        }
        s.next_id = snapshot.next_id;
        s.fatal_error = snapshot.fatal;
        s.diag = snapshot.diag;
    }

    /// 读串行回退旗标并复位（每次并行尝试后调用一次）。
    pub(in crate::settle) fn take_serial_only(&self) -> bool {
        let mut s = self.inner.lock().expect("共享候选锁");
        std::mem::replace(&mut s.serial_only, false)
    }

    /// 拷贝本拍耦合计数（结算收尾时取走；共享候选本身随后被消费）。
    pub(in crate::settle) fn diag_counters(&self) -> crate::settle::diag::DiagCounters {
        self.inner.lock().expect("共享候选锁").diag
    }

    /// 消费唯一持有者。仍有规则持有克隆时返回错误，不复制或发布共享候选。
    pub fn into_parts(self) -> Result<PublishedCore, SettleError> {
        self.check_fatal()?;
        let state = Arc::try_unwrap(self.inner)
            .map_err(|_| SettleError("候选仍被房间规则持有，不能发布".into()))?
            .into_inner()
            .expect("共享候选锁");
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
    /// 各房本拍动作记录导出（captureActionLog 读面），随候选发布/丢弃。
    pub(in crate::settle) action_log:
        BTreeMap<String, Vec<crate::settle::action_log::ActionLogEntry>>,
}

/// 已完成派生状态处理且不再共享的发布数据。
pub(in crate::settle) struct PreparedTick {
    pub(in crate::settle) rooms: Vec<(String, RoomState)>,
    pub(in crate::settle) players: BTreeMap<String, PlayerState>,
    pub(in crate::settle) next_id: u64,
    pub(in crate::settle) events: Vec<Value>,
    pub(in crate::settle) market: market::MarketState,
    pub(in crate::settle) mod_states: BTreeMap<String, Value>,
    pub(in crate::settle) action_log:
        BTreeMap<String, Vec<crate::settle::action_log::ActionLogEntry>>,
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
            action_log: self.action_log,
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
