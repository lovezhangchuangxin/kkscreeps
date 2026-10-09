//! 世界候选准备、分阶段结算与统一发布。
use super::{stages, txn};
use crate::intent::IntentBatch;
use crate::settle::{
    Engine, Plane, PlayerWriteback, RunState, SettleError, TickInput, TickStage, TickState,
    WritebackStep, inter_room, market, nukes, powers, settle_room,
};
use kk_schema::room::RoomObject;
use std::collections::BTreeMap;

impl Engine {
    /// 按准备、房间、市场、校验、发布的顺序处理候选。
    pub(in crate::settle) fn staged_body(
        &mut self,
        cursor: &mut stages::StageCursor,
        batch: IntentBatch,
        input: &TickInput,
        writebacks: BTreeMap<String, PlayerWriteback>,
    ) -> Result<(), SettleError> {
        cursor.enter(TickStage::Prepare)?;
        if let Some(handle) = writebacks
            .keys()
            .find(|handle| !self.players.contains_key(*handle))
        {
            return Err(SettleError(format!(
                "玩家写回候选不属于已注册玩家：{handle}"
            )));
        }
        let mut txn = self.prepare_tick(batch, *input)?;
        cursor.enter(TickStage::Settle)?;
        self.settle_rooms_stage(&mut txn)?;
        cursor.enter(TickStage::Market)?;
        {
            // 核弹发射在全部房间结算后跨房落位（terminal send 同款标记模式）；
            // 先于市场阶段，nuker 的库与冷却在提交前一并定型。
            nukes::settle_launches(&mut txn.rooms, &txn.state, txn.time)?;
            let mut state = txn.state.inner.borrow_mut();
            market::settle_market(
                &mut txn.rooms,
                &mut state.players,
                &mut txn.market,
                &txn.batch,
                txn.time,
                txn.input.now_ms,
            );
        }
        {
            // power 账号域全局意图与已生成侧死亡扫描（墙钟冷却用同源
            // now_ms）；players 经共享 TickState 内短借用访问，独立于
            // 市场阶段的借用块。
            powers::settle_power(
                &mut txn.rooms,
                &txn.batch,
                txn.time,
                txn.input.now_ms,
                &txn.state,
            )?;
        }
        self.writeback_checkpoint(WritebackStep::Market)?;
        cursor.enter(TickStage::Validate)?;
        {
            let mut state = txn.state.inner.borrow_mut();
            for (handle, patch) in writebacks {
                let player = state.players.get_mut(&handle).ok_or_else(|| {
                    SettleError(format!("玩家写回候选不属于已注册玩家：{handle}"))
                })?;
                patch.apply_with_checkpoints(player, |step| self.writeback_checkpoint(step))?;
            }
        }
        self.validate_stage(&mut txn)?;
        let time = txn.time;
        let prepared = txn.into_prepared()?;
        self.run_state = RunState::Prepared { tick: time };
        cursor.enter(TickStage::Publish)?;
        self.publish_tick(prepared, input.warmup)?;
        Ok(())
    }

    /// 构造玩家副本和 ID 起点，并复核安全模式指令。
    pub(in crate::settle) fn prepare_tick(
        &self,
        batch: IntentBatch,
        input: TickInput,
    ) -> Result<txn::OwnedTickTxn, SettleError> {
        let next = self
            .next_id
            .max(self.world.next_available_obj_id().map_err(SettleError)?);
        let players = self.players.clone();
        let mut batch = batch;
        // 按已提交世界复核：玩家已有生效的安全模式时，丢弃其新的激活指令。
        // 不依赖 JavaScript 的扫描结果，也不计入本轮尚未生效的激活。
        // 来源：上游 engine/src/game/structures.js 的 StructureController.activateSafeMode。
        let time = self.world.time();
        let mut safemode_holders: Vec<String> = self
            .world
            .rooms()
            .values()
            .flat_map(|r| r.objects.values())
            .filter_map(|o| match o {
                RoomObject::Controller(c) if c.safe_mode > time => c.owner.clone(),
                _ => None,
            })
            .collect();
        safemode_holders.sort_unstable();
        safemode_holders.dedup();
        if !safemode_holders.is_empty() {
            for holder in &safemode_holders {
                if let Some(intents) = batch.users.get_mut(holder) {
                    for actions in intents.objects.values_mut() {
                        actions.remove(&crate::intent::Action::ActivateSafeMode);
                    }
                }
            }
        }
        let room_names: Vec<String> = self.world.rooms().keys().cloned().collect();
        Ok(txn::OwnedTickTxn {
            time,
            input,
            room_names,
            state: TickState::with_mod_states(players, next, self.mod_states.clone()),
            batch,
            rooms: Vec::new(),
            events: Vec::new(),
            market: self.market.clone(),
        })
    }

    /// 按房名结算副本，共用候选玩家与 ID 游标；任一房间失败均不写回。
    pub(super) fn settle_rooms_stage(
        &self,
        txn: &mut txn::OwnedTickTxn,
    ) -> Result<(), SettleError> {
        let mut inter_room_moves = Vec::new();
        for name in &txn.room_names {
            if self.fail_room.as_deref() == Some(name.as_str()) {
                return Err(SettleError(format!("故障注入：房间 {name} 结算失败")));
            }
            let mut room = self.world.room(name).expect("房间名来自键集").clone();
            // 二进制房间快照不保存房名；结算指令按世界目录中的名称匹配。
            room.name.clone_from(name);
            let plane = Plane {
                links: self.links,
                stores: self.stores,
                combat: self.combat,
                move_gate: self.move_gate,
                static_passability: self.static_passability,
                quotas: self.quotas.clone(),
                resources: self.resources.clone(),
                buildings: self.buildings.clone(),
                contrib: self.contrib.clone(),
                extensions: self.extensions.clone(),
                plan: self.plan.clone(),
            };
            let (room, mut room_events, mut moves) = settle_room(
                room,
                txn.time,
                txn.input.now_ms,
                &txn.state,
                &txn.batch,
                &plane,
            )?;
            inter_room_moves.append(&mut moves);
            txn.rooms.push((name.clone(), room));
            txn.events.append(&mut room_events);
        }
        // 跨房过渡在全部房间结算完成后应用到候选：到达对象不参与目的房
        // 本 tick 的结算（其对象 tick 已在来源房执行），发布后对下一轮可见。
        inter_room::apply_inter_room_moves(&mut txn.rooms, &inter_room_moves);
        Ok(())
    }

    /// 将已校验候选发布到世界；World 在写入前检查所有可失败条件，
    /// 一次推进时间与版本并使目录/投影缓存失效，随后换入账户等候选。
    pub(super) fn publish_tick(
        &mut self,
        prepared: txn::PreparedTick,
        warmup: bool,
    ) -> Result<(), SettleError> {
        self.world
            .commit_tick_rooms(prepared.rooms, prepared.next_id, !warmup)
            .map_err(SettleError)?;
        self.market = prepared.market;
        self.players = prepared.players;
        self.mod_states = prepared.mod_states;
        self.next_id = prepared.next_id;
        self.last_events = prepared.events;
        Ok(())
    }
}
