//! 世界候选准备、分阶段结算与统一发布。
use super::{stages, txn};
use crate::intent::IntentBatch;
use crate::settle::{
    Engine, Plane, PlayerWriteback, RunState, SettleError, SettledRoom, TickInput, TickStage,
    TickState, WritebackStep, diag::TickDiag, inter_room, market, nukes, portals, powers,
    settle_room,
};
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::RoomState;
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
            let mut state = txn.state.inner.lock().expect("共享候选锁");
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
            let mut state = txn.state.inner.lock().expect("共享候选锁");
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
        self.last_rooms_parallel = diag.parallel;
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
    /// 满足资格（工数 ≥2、随机源为派生、无 claim 意图）时先尝试逐房并行；
    /// 并行段触碰依赖房序的共享路径（ID 分配/claim 读候选）即恢复共享态
    /// 并走串行重做，语义与纯串行完全一致（R09-lite，B0 形态先行）。
    pub(super) fn settle_rooms_stage(
        &self,
        txn: &mut txn::OwnedTickTxn,
        diag: &mut TickDiag,
    ) -> Result<(), SettleError> {
        let workers = self.settle_workers_effective(txn.room_names.len());
        let (inter_room_moves, fresh_age) = if workers >= 2
            && matches!(txn.state.random, crate::settle::ProcessorRandom::Derived)
            && !self.contrib.any_hosted()
            && !batch_has_claim_intents(&txn.batch)
            && let Some(collections) = self.settle_rooms_parallel(txn, diag, workers)?
        {
            collections
        } else {
            diag.parallel = false;
            self.settle_rooms_serial(txn, diag)?
        };
        self.apply_inter_room_tail(txn, diag, inter_room_moves, fresh_age)
    }

    /// 逐房并行结算的房间循环尾部之外的全局段：portal 收集与跨房移动
    /// 应用（两条路径共用）。
    fn apply_inter_room_tail(
        &self,
        txn: &mut txn::OwnedTickTxn,
        diag: &mut TickDiag,
        mut inter_room_moves: Vec<inter_room::InterRoomMove>,
        fresh_age: std::collections::BTreeSet<ObjId>,
    ) -> Result<(), SettleError> {
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

    /// 原串行逐房结算：按世界目录房序克隆、结算并收集跨房移动；也是
    /// 并行尝试触发串行回退时的重做路径。Plane 在循环外构建一次（房集
    /// 在整轮结算内不变）。
    fn settle_rooms_serial(
        &self,
        txn: &mut txn::OwnedTickTxn,
        diag: &mut TickDiag,
    ) -> Result<
        (
            Vec<inter_room::InterRoomMove>,
            std::collections::BTreeSet<ObjId>,
        ),
        SettleError,
    > {
        let mut inter_room_moves = Vec::new();
        let mut fresh_age = std::collections::BTreeSet::new();
        let world_span = market::world_span(self.world.rooms().keys().map(String::as_str));
        let plane = self.tick_plane(world_span);
        for name in &txn.room_names {
            if self.fail_room.as_deref() == Some(name.as_str()) {
                return Err(SettleError(format!("故障注入：房间 {name} 结算失败")));
            }
            let t_room = std::time::Instant::now();
            let mut room = self.world.room(name).expect("房间名来自键集").clone();
            // 二进制房间快照不保存房名；结算指令按世界目录中的名称匹配。
            room.name.clone_from(name);
            diag.clone_ns += t_room.elapsed().as_nanos();
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
        Ok((inter_room_moves, fresh_age))
    }

    /// 并行逐房结算尝试。Some 表示产物已按房序写入 txn 并返回跨房收集；
    /// None 表示触发串行回退（共享态耦合量已恢复、房间产物已丢弃，调用
    /// 方走串行重做）。Err 为确定性失败：取最低房序错误，与串行首失败
    /// 同消息（候选由调用方整体丢弃）。
    ///
    /// 确定性依据（取证见 50x6-settle-phase-diag）：房间循环的跨房耦合仅
    /// ID 分配、claim 读候选与 gcl/power 交换律累加——前两者触发串行
    /// 回退，后者求和与顺序无关；事件按房序归并、跨房移动后置应用。
    ///
    /// 克隆与结算流水重叠：主线程克隆一间即交付共享队列一间（World 非
    /// Sync，克隆留在主线程，计时归 clone 段），工作者取房即结算。完成
    /// 序由调度决定，不影响产物——合并严格按房序，工作者计时经完成通道
    /// 回传累加。
    fn settle_rooms_parallel(
        &self,
        txn: &mut txn::OwnedTickTxn,
        diag: &mut TickDiag,
        workers: usize,
    ) -> Result<Option<RoomCollections>, SettleError> {
        let diag_entry = (diag.clone_ns, diag.settle_ns, diag.hook_ns);
        let snapshot = txn.state.coupling_snapshot();
        let hook = self.room_settled_hook.clone();
        let fail_room = self.fail_room.clone();
        let time = txn.time;
        let now_ms = txn.input.now_ms;
        let plane = self.tick_plane(market::world_span(
            self.world.rooms().keys().map(String::as_str),
        ));
        struct Outcome {
            settled: Result<SettledRoom, SettleError>,
        }
        // 单队列多消费者：Receiver 非 Sync，包 Mutex 共享等待。等待者持锁
        // 阻塞于 recv，工作者只在取房瞬间竞争；结算期间不持锁，主线程
        // send 不经过该锁。
        let (work_tx, work_rx) = std::sync::mpsc::channel::<(usize, RoomState)>();
        let work_rx = std::sync::Mutex::new(work_rx);
        let (done_tx, done_rx) = std::sync::mpsc::channel::<(usize, Outcome, u128, u128)>();
        let state = txn.state.clone();
        let batch = &txn.batch;
        let total = txn.room_names.len();
        let mut injected: Option<String> = None;
        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(workers.min(total));
            for _ in 0..workers.min(total) {
                let state = &state;
                let batch = &*batch;
                let plane = &plane;
                let hook = hook.as_deref();
                let work_rx = &work_rx;
                let done_tx = done_tx.clone();
                handles.push(scope.spawn(move || {
                    loop {
                        let room = {
                            let guard = work_rx.lock().expect("结算取房锁");
                            guard.recv()
                        };
                        let (idx, room) = match room {
                            Ok(pair) => pair,
                            Err(_) => break, // 队列关闭：克隆已全部交付
                        };
                        let t = std::time::Instant::now();
                        let settled = settle_room(room, time, now_ms, state, batch, plane);
                        let settle_ns = t.elapsed().as_nanos();
                        let mut hook_ns = 0;
                        if let Ok(sr) = &settled
                            && let Some(hook) = hook
                        {
                            let t = std::time::Instant::now();
                            hook(&sr.room.name, &sr.room);
                            hook_ns = t.elapsed().as_nanos();
                        }
                        if done_tx
                            .send((idx, Outcome { settled }, settle_ns, hook_ns))
                            .is_err()
                        {
                            break; // 接收端已丢弃：整轮已失败
                        }
                    }
                }));
            }
            // 主线程克隆房候选并即时交付：克隆与工作者结算重叠。故障注入
            // 点之后不再交付，注入错误按房序进入合并（与串行首失败同位）。
            for (idx, name) in txn.room_names.iter().enumerate() {
                if fail_room.as_deref() == Some(name.as_str()) {
                    injected = Some(name.clone());
                    break;
                }
                let t_room = std::time::Instant::now();
                let mut room = self.world.room(name).expect("房间名来自键集").clone();
                room.name.clone_from(name);
                diag.clone_ns += t_room.elapsed().as_nanos();
                if work_tx.send((idx, room)).is_err() {
                    break; // 工作者 panic：join 处统一报错
                }
            }
            // 显式关闭队列并释放本线程的完成通道端：工作者随 recv Err 退
            // 出，scope 退出前的 join 才能返回；不显式 drop 会互相等待。
            drop(work_tx);
            drop(done_tx);
            for handle in handles {
                handle.join().expect("结算工作线程 panic");
            }
        });
        // scope 已 join 全部工作者：完成消息齐备，逐条收集后按房序合并。
        let mut outcomes: Vec<Option<Outcome>> = (0..total).map(|_| None).collect();
        while let Ok((idx, outcome, settle_ns, hook_ns)) = done_rx.recv() {
            diag.settle_ns += settle_ns;
            diag.hook_ns += hook_ns;
            debug_assert!(outcomes[idx].is_none(), "每房恰一条完成消息");
            outcomes[idx] = Some(outcome);
        }
        if let Some(name) = &injected {
            let idx = txn
                .room_names
                .iter()
                .position(|n| n == name)
                .expect("注入房必在房序");
            if outcomes[idx].is_none() {
                outcomes[idx] = Some(Outcome {
                    settled: Err(SettleError(format!("故障注入：房间 {name} 结算失败"))),
                });
            }
        }
        let mut inter_room_moves = Vec::new();
        let mut fresh_age = std::collections::BTreeSet::new();
        for (name, outcome) in txn.room_names.iter().zip(outcomes) {
            let Some(outcome) = outcome else {
                break; // 注入点之后未交付
            };
            let SettledRoom {
                room,
                events: mut room_events,
                inter_room: mut moves,
                fresh_age: mut fresh,
                action_log,
            } = outcome.settled?;
            inter_room_moves.append(&mut moves);
            fresh_age.append(&mut fresh);
            txn.rooms.push((name.clone(), room));
            txn.events.append(&mut room_events);
            if !action_log.is_empty() {
                txn.action_log.insert(name.clone(), action_log);
            }
        }
        // 串行回退检查：并行段触碰了依赖房序的共享路径。恢复共享态耦合量
        // 并丢弃房间产物；本拍分段计时回退到并行尝试前（串行重做重新累计）。
        if txn.state.take_serial_only() {
            diag.parallel = false;
            txn.state.restore_coupling(snapshot);
            txn.rooms.clear();
            txn.events.clear();
            txn.action_log.clear();
            (diag.clone_ns, diag.settle_ns, diag.hook_ns) = diag_entry;
            return Ok(None);
        }
        diag.parallel = true;
        Ok(Some((inter_room_moves, fresh_age)))
    }

    /// 本轮逐房结算的规则平面（房集在整轮结算内不变）。
    fn tick_plane(&self, world_span: i64) -> Plane {
        Plane {
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
        }
    }

    /// 逐房并行结算的有效工数：环境覆盖（KK_SETTLE_WORKERS，0/1=强制串行）
    /// > 引擎覆盖（set_settle_workers）> 按房数自适应 min(4, rooms/16)。
    fn settle_workers_effective(&self, rooms: usize) -> usize {
        static ENV: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
        if let Some(n) = ENV.get_or_init(|| {
            std::env::var("KK_SETTLE_WORKERS")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok())
        }) {
            return *n;
        }
        if let Some(n) = self.settle_workers_override {
            return n;
        }
        // 核数感知：并行结算以 CPU 换墙钟，小核机器（如 2 核轻量服务器）
        // 不值得——为预编码线程与主线程保留产能后取可用工数。
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        std::cmp::min(4, std::cmp::min(rooms / 16, cores.saturating_sub(3).max(1)))
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
        let old_rooms = self
            .world
            .commit_tick_rooms_prevalidated(prepared.rooms, prepared.next_id, !warmup)
            .map_err(SettleError)?;
        diag.commit_ns = t_commit.elapsed().as_nanos();
        // 旧房释放在结算关键路径之外（RoomState 无 Drop 副作用且 Send）：
        // 后台线程吸收 16 万级对象的 dealloc，与下一拍的投影/执行窗重叠。
        if !old_rooms.is_empty() {
            std::thread::spawn(move || drop(old_rooms));
        }
        self.market = prepared.market;
        self.players = prepared.players;
        self.mod_states = prepared.mod_states;
        self.next_id = prepared.next_id;
        self.last_events = prepared.events;
        self.last_action_log = prepared.action_log;
        Ok(())
    }
}

/// 逐房结算的跨房收集（串行/并行两路径共用的返回形态）。
type RoomCollections = (
    Vec<inter_room::InterRoomMove>,
    std::collections::BTreeSet<ObjId>,
);

/// 批内是否携带 claimController 意图：claim 读候选 gcl 的语义依赖房序
///（同拍他房累加后占领门槛），携带即整拍走串行。频率上接近零
///（仅在占领新房时出现）。
fn batch_has_claim_intents(batch: &IntentBatch) -> bool {
    batch.users.values().any(|user| {
        user.objects
            .values()
            .any(|actions| actions.contains_key(&crate::intent::Action::ClaimController))
    })
}
