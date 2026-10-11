//! 世界候选准备、分阶段结算与统一发布。
use super::{stages, txn};
use crate::intent::IntentBatch;
use crate::settle::{
    Engine, Plane, PlayerWriteback, RunState, SettleError, SettledRoom, TickInput, TickStage,
    TickState, WritebackStep, diag::TickDiag, inter_room, market, nukes, portals, powers,
    settle_room,
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
        let mut diag = TickDiag::default();
        let t_stage = std::time::Instant::now();
        let mut txn = self.prepare_tick(batch, *input)?;
        diag.prepare_ns = t_stage.elapsed().as_nanos();
        cursor.enter(TickStage::Settle)?;
        self.settle_rooms_stage(&mut txn, &mut diag)?;
        cursor.enter(TickStage::Market)?;
        {
            // 核弹发射在全部房间结算后跨房落位（terminal send 同款标记模式）；
            // 先于市场阶段，nuker 的库与冷却在提交前一并定型。
            let t_stage = std::time::Instant::now();
            nukes::settle_launches(&mut txn.rooms, &txn.state, txn.time, txn.input.now_ms)?;
            diag.nukes_ns = t_stage.elapsed().as_nanos();
            let t_stage = std::time::Instant::now();
            let mut state = txn.state.inner.borrow_mut();
            market::settle_market(
                &mut txn.rooms,
                &mut state.players,
                &mut txn.market,
                &txn.batch,
                txn.time,
                txn.input.now_ms,
            );
            diag.market_ns = t_stage.elapsed().as_nanos();
        }
        {
            // power 账号域全局意图与已生成侧死亡扫描（墙钟冷却用同源
            // now_ms）；players 经共享 TickState 内短借用访问，独立于
            // 市场阶段的借用块。
            let t_stage = std::time::Instant::now();
            powers::settle_power(
                &mut txn.rooms,
                &txn.batch,
                txn.time,
                txn.input.now_ms,
                &txn.state,
            )?;
            diag.power_ns = t_stage.elapsed().as_nanos();
        }
        self.writeback_checkpoint(WritebackStep::Market)?;
        cursor.enter(TickStage::Validate)?;
        {
            let t_stage = std::time::Instant::now();
            let mut state = txn.state.inner.borrow_mut();
            for (handle, patch) in writebacks {
                let player = state.players.get_mut(&handle).ok_or_else(|| {
                    SettleError(format!("玩家写回候选不属于已注册玩家：{handle}"))
                })?;
                patch.apply_with_checkpoints(player, |step| self.writeback_checkpoint(step))?;
            }
            diag.writeback_ns = t_stage.elapsed().as_nanos();
        }
        let t_stage = std::time::Instant::now();
        self.validate_stage(&mut txn)?;
        diag.validate_ns = t_stage.elapsed().as_nanos();
        // 耦合计数在共享候选被消费前拷走（含结算期间的 allocate/claim/
        // gcl/power 触碰频次）。
        diag.counters = txn.state.diag_counters();
        diag.events = txn.events.len() as u64;
        diag.rooms = txn.rooms.len() as u64;
        let time = txn.time;
        let t_stage = std::time::Instant::now();
        let prepared = txn.into_prepared()?;
        diag.into_prepared_ns = t_stage.elapsed().as_nanos();
        self.run_state = RunState::Prepared { tick: time };
        cursor.enter(TickStage::Publish)?;
        self.publish_tick(prepared, input.warmup, &mut diag)?;
        self.record_settle_diag(diag);
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
            state: TickState::with_mod_states(players, next, self.mod_states.clone())
                .with_random(self.processor_random.clone()),
            batch,
            rooms: Vec::new(),
            events: Vec::new(),
            market: self.market.clone(),
            action_log: std::collections::BTreeMap::new(),
        })
    }

    /// 按房名结算副本，共用候选玩家与 ID 游标；任一房间失败均不写回。
    pub(super) fn settle_rooms_stage(
        &self,
        txn: &mut txn::OwnedTickTxn,
        diag: &mut TickDiag,
    ) -> Result<(), SettleError> {
        let mut inter_room_moves = Vec::new();
        let mut fresh_age = std::collections::BTreeSet::new();
        // 世界房集在整轮结算内不变：span 只求一次，逐房 Plane 复用。
        let world_span = market::world_span(self.world.rooms().keys().map(String::as_str));
        for name in &txn.room_names {
            if self.fail_room.as_deref() == Some(name.as_str()) {
                return Err(SettleError(format!("故障注入：房间 {name} 结算失败")));
            }
            let t_room = std::time::Instant::now();
            let mut room = self.world.room(name).expect("房间名来自键集").clone();
            // 二进制房间快照不保存房名；结算指令按世界目录中的名称匹配。
            room.name.clone_from(name);
            diag.clone_ns += t_room.elapsed().as_nanos();
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
                world_span,
                plan: self.plan.clone(),
            };
            let t_settle = std::time::Instant::now();
            let SettledRoom {
                room,
                events: mut room_events,
                inter_room: mut moves,
                fresh_age: mut fresh,
                action_log,
            } = settle_room(
                room,
                txn.time,
                txn.input.now_ms,
                &txn.state,
                &txn.batch,
                &plane,
            )?;
            diag.settle_ns += t_settle.elapsed().as_nanos();
            let t_settle = std::time::Instant::now();
            if let Some(hook) = &self.room_settled_hook {
                hook(name, &room);
            }
            diag.hook_ns += t_settle.elapsed().as_nanos();
            inter_room_moves.append(&mut moves);
            fresh_age.append(&mut fresh);
            txn.rooms.push((name.clone(), room));
            txn.events.append(&mut room_events);
            if !action_log.is_empty() {
                txn.action_log.insert(name.clone(), action_log);
            }
        }
        // portal 传送先于边缘过渡收集应用（官方两段式同 tick：creep tick
        // 的 portal 标记覆盖 edge 标记——既在边缘格又在 portal 格的对象只
        // 走传送；目的房未注册或 closed 时对象保持原位逐拍重试）。fresh_age
        // 是本拍刚初始化寿命的 creep，官方门在其补设前求值故当拍豁免。
        let t_tail = std::time::Instant::now();
        let portal_moves =
            portals::collect_portal_transports(&txn.rooms, &fresh_age, txn.input.now_ms);
        let portal_ids: std::collections::BTreeSet<_> =
            portal_moves.iter().map(|mv| mv.id).collect();
        inter_room_moves.retain(|mv| !portal_ids.contains(&mv.id));
        diag.portal_moves = portal_moves.len() as u64;
        diag.interroom_moves = inter_room_moves.len() as u64;
        inter_room::apply_inter_room_moves(&mut txn.rooms, &portal_moves, txn.input.now_ms);
        // 跨房过渡随后应用到候选：到达对象不参与目的房本 tick 的结算
        //（其对象 tick 已在来源房执行），发布后对下一轮可见。
        inter_room::apply_inter_room_moves(&mut txn.rooms, &inter_room_moves, txn.input.now_ms);
        diag.interroom_ns += t_tail.elapsed().as_nanos();
        Ok(())
    }

    /// 将已校验候选发布到世界；World 在写入前检查所有可失败条件，
    /// 一次推进时间与版本并使目录/投影缓存失效，随后换入账户等候选。
    pub(super) fn publish_tick(
        &mut self,
        prepared: txn::PreparedTick,
        warmup: bool,
        diag: &mut TickDiag,
    ) -> Result<(), SettleError> {
        let t_commit = std::time::Instant::now();
        self.world
            .commit_tick_rooms(prepared.rooms, prepared.next_id, !warmup)
            .map_err(SettleError)?;
        diag.commit_ns = t_commit.elapsed().as_nanos();
        self.market = prepared.market;
        self.players = prepared.players;
        self.mod_states = prepared.mod_states;
        self.next_id = prepared.next_id;
        self.last_events = prepared.events;
        self.last_action_log = prepared.action_log;
        Ok(())
    }
}
