//! 收集玩家候选、缓存失败轮并协调 Memory/CPU/世界统一发布。
use super::*;
use crate::mod_trace::{ModTraceEntry, ModTraceEvent};
use anyhow::Context as _;

impl EmbedEngine {
    /// 请求一个玩家并推进一轮；存在失败缓存时，仍会提交整轮缓存的结果。
    /// 重试时忽略 `code`，且 `handle` 必须属于缓存轮。
    pub fn run_player(&mut self, handle: &str, code: &str) -> anyhow::Result<PlayerRun> {
        let mut codes = BTreeMap::new();
        codes.insert(handle.to_string(), code.to_string());
        let mut runs = self.run_players(&codes)?;
        Ok(runs.remove(handle).expect("请求的 handle 必在结果中"))
    }

    /// 让请求的玩家读取同一已提交世界，求值后合并意图并结算一个 tick。
    /// 空请求也会推进世界。玩家级错误保留在结果中，基础设施故障返回整轮错误。
    ///
    /// 世界结算失败后缓存整轮候选；再次调用忽略传入代码，只重试结算。
    /// 重试可请求缓存玩家的子集或空集，但始终提交并返回全部缓存玩家的结果；
    /// 请求缓存轮之外的玩家会返回错误并保留缓存。
    pub fn run_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, PlayerRun>> {
        self.no_execution_failure("run_players")?;
        let t0 = std::time::Instant::now();
        // 先校验再取走缓存，防止错误请求丢掉失败轮。子集请求仍重试整个缓存轮。
        if let Some(pending) = &self.pending {
            for handle in codes.keys() {
                if !pending.results.contains_key(handle) {
                    anyhow::bail!(
                        "上轮结算失败的重试轮不含玩家 {handle}（先经 run_players 重试缓存轮，或 discard_pending_round 丢弃后开新一轮）"
                    );
                }
            }
        }
        // 恢复失败不能取走候选或重新执行玩家；重建只读取原启动快照。
        self.rebuild_mod_realm()?;
        let (results, run_ns, project_ns, input) = match self.pending.take() {
            Some(pending) => (pending.results, 0u128, 0u128, pending.input),
            None => {
                let (results, run_ns, project_ns) = self.execute_players(codes)?;
                (results, run_ns, project_ns, TickInput::now())
            }
        };
        // 墙钟输入随候选一起缓存，重试不会重新采样签名时间等值。
        let t_settle = std::time::Instant::now();
        let writebacks = results
            .iter()
            .map(|(handle, result)| {
                Ok((
                    handle.clone(),
                    PlayerWriteback {
                        cpu: Some(
                            kk_core::cpu::RunBudget::new(&self.engine.players()[handle].cpu)
                                .finish(result.cpu_billed_ms)
                                .map_err(anyhow::Error::msg)?,
                        ),
                        raw_memory: result.raw_memory.clone(),
                        segments: result
                            .segments_out
                            .as_ref()
                            .map(SegmentWriteback::from_runner),
                    },
                ))
            })
            .collect::<anyhow::Result<BTreeMap<_, _>>>();
        let writebacks = match writebacks {
            Ok(writebacks) => writebacks,
            Err(error) => {
                self.execution_failure = Some(ExecutionFailure {
                    confirmed: results,
                    reason: error.to_string(),
                });
                return Err(error);
            }
        };
        // G25.a：日志载荷须在 results/writebacks 被结算消费前构造；轮身份
        // 取结算前的世界值（结算执行的是世界当前 tick），墙钟用缓存首次值。
        let (round_tick, round_generation) =
            (self.engine.world().time(), self.engine.world().generation());
        let log_input = self.input_log.is_some().then(|| {
            let intents = Value::Object(
                results
                    .iter()
                    .map(|(handle, result)| (handle.clone(), result.intents_raw.clone()))
                    .collect(),
            );
            let entries = results
                .iter()
                .map(|(handle, result)| {
                    // run_players 的写回路径恒带新桶；缺桶说明构造逻辑被改动。
                    let bucket = writebacks[handle]
                        .cpu
                        .as_ref()
                        .expect("run_players 写回必须携带新 CPU 桶");
                    (
                        handle.as_str(),
                        input_log::PlayerLogEntry {
                            cpu_bucket: bucket,
                            cpu_billed_ms: result.cpu_billed_ms,
                            raw_memory: result.raw_memory.as_deref(),
                            segments_out: result.segments_out.as_ref(),
                        },
                    )
                })
                .collect();
            (input.now_ms, intents, input_log::players_map(&entries))
        });
        let settled = self.settle_world_with_writebacks(
            Self::batch_from_results(&results),
            input,
            writebacks,
        );
        match settled {
            Ok(()) => {
                if let Some((now_ms, intents, players)) = log_input
                    && let Err(e) = self.log_round(
                        round_tick,
                        round_generation,
                        "players",
                        now_ms,
                        intents,
                        players,
                    )
                {
                    return Err(self.drop_log_for_io_failure(e));
                }
                let settle_ns = t_settle.elapsed().as_nanos();
                // 世界、玩家内存和 CPU 桶已经统一发布，无需线程计费回执。
                self.perf.record([
                    ns_ms(project_ns),
                    ns_ms(run_ns),
                    ns_ms(settle_ns),
                    t0.elapsed().as_secs_f64() * 1000.0,
                ]);
                let mut out = BTreeMap::new();
                for (handle, result) in results {
                    let run = self.finish_player_run(&handle, result);
                    out.insert(handle, run);
                }
                Ok(out)
            }
            Err(e) => {
                // 保留候选以重试结算；已提交世界、Memory 和 CPU 桶保持旧值。
                // V8 全局变量不在回滚范围内。
                self.pending = Some(PendingRound { results, input });
                Err(e)
            }
        }
    }

    /// 执行世界结算并消费测试故障钩子。重试的 `input` 来自失败轮缓存。
    pub(super) fn settle_world(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
    ) -> anyhow::Result<()> {
        self.settle_world_with_writebacks(batch, input, BTreeMap::new())
    }

    pub(super) fn settle_world_with_writebacks(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
        writebacks: BTreeMap<String, PlayerWriteback>,
    ) -> anyhow::Result<()> {
        self.rebuild_mod_realm()?;
        if self.fail_next_settle {
            self.fail_next_settle = false;
            anyhow::bail!("故障注入：世界结算失败（测试钩子）");
        }
        // 引擎钩子持续生效，这里只借用一次，并在结算返回后解除。
        let fail_room = self.fail_next_settle_room.take();
        if fail_room.is_some() {
            self.engine.set_fail_room_for_test(fail_room.clone());
        }
        let fail_stage = self.fail_next_settle_stage.take();
        if fail_stage.is_some() {
            self.engine.set_fail_stage_for_test(fail_stage);
        }
        let fail_writeback = self.fail_next_writeback.take();
        self.engine.set_fail_writeback_for_test(fail_writeback);
        if let Some(vm) = &self.mod_realm {
            let mut vm = vm.borrow_mut();
            // 新的实际结算清除上一轮诊断；恢复和结算前的拒绝不清除。
            vm.last_fault = None;
            vm.realm_mut()
                .map_err(anyhow::Error::msg)?
                .begin_settlement()
                .map_err(anyhow::Error::msg)?;
        }
        let mut settled = self
            .engine
            .tick_with_writebacks(batch, input, writebacks)
            .map_err(|e| anyhow::anyhow!("{e}"));
        if fail_room.is_some() {
            self.engine.set_fail_room_for_test(None);
        }
        if fail_stage.is_some() {
            self.engine.set_fail_stage_for_test(None);
        }
        self.engine.set_fail_writeback_for_test(None);
        if let Some(vm) = &self.mod_realm {
            vm.borrow_mut()
                .realm_mut()
                .map_err(anyhow::Error::msg)?
                .end_settlement();
        }
        if let Some(vm) = &self.mod_realm {
            let mut vm = vm.borrow_mut();
            let tracing = vm.trace_enabled();
            if let Some(decision) = vm.finish_settlement(settled.is_ok()) {
                if tracing {
                    // mod_id 来自 finish_settlement 的同一归责口径；
                    // disabled 含依赖闭包，按序保留。
                    vm.record_trace(ModTraceEntry {
                        tick: self.engine.world().time(),
                        room: String::new(),
                        target: String::new(),
                        slot: String::new(),
                        stage: "",
                        event: ModTraceEvent::AutoDisabled {
                            mod_id: decision.mod_id.clone(),
                            consecutive: decision.consecutive_faults,
                            disabled: decision.disabled.iter().cloned().collect(),
                        },
                    });
                }
                let error = settled.expect_err("自动停用只能发生在结算失败后");
                settled = Err(anyhow::anyhow!(
                    "{error}；连续 {} 次可归责故障，已自动停用 Mod [{}]；本轮未发布，下次重试使用新链",
                    decision.consecutive_faults,
                    decision
                        .disabled
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Err(error) = &settled {
                // 整轮失败：候选缓存待重试（不重跑玩家代码）；trace 记录
                // 回退事实，作者可见"结果被丢弃"而不只是最终错误。
                if tracing {
                    vm.record_trace(ModTraceEntry {
                        tick: self.engine.world().time(),
                        room: String::new(),
                        target: String::new(),
                        slot: String::new(),
                        stage: "",
                        event: ModTraceEvent::SettlementRetry {
                            error: crate::mod_trace::truncate(&error.to_string()),
                        },
                    });
                }
                vm.invalidate();
            }
        }
        settled
    }

    /// 从同一已提交状态注入 Memory 和世界，有界派发并回收请求玩家的候选。
    /// 注入通信失败或 `InfraFailure` 中止整轮，保留已确认结果并阻止自动重跑；
    /// 玩家的语法错误、运行错误和跳帧惩罚则由后续结算统一处理。
    #[allow(clippy::type_complexity)]
    pub(super) fn execute_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<(BTreeMap<String, RunResult>, u128, u128)> {
        // 未知 handle 显式报错：静默跳过会让结果映射缺项，调用方无从得知玩家没跑
        let registered = self
            .players
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?
            .players();
        for handle in codes.keys() {
            if !registered.iter().any(|p| p == handle) {
                anyhow::bail!("未知玩家 handle: {handle}");
            }
        }
        let mut results = BTreeMap::new();
        let mut project_ns = 0u128;
        let handles = registered.clone();
        // 本轮所有玩家读取同一代世界，玩家执行结束后才开始结算。
        let bound_generation = self.engine.world().generation();
        // 回执必须匹配派发时的 tick 和 generation，防止接受另一轮的结果。
        let epoch = RunEpoch {
            tick: self.engine.world().time(),
            generation: bound_generation,
        };
        let identity = self.engine.world().instance();
        let terrain_epoch = self.engine.world().terrain_epoch();
        // 计划级墙通行声明参与缓存键：切包改变声明时，同世界代际也须重建。
        let walls_passable = self.engine.terrain_passability().wall_passable;
        let terrain = match &self.pathfinder_terrain {
            Some((cached_identity, cached_epoch, cached_walls, terrain))
                if std::sync::Arc::ptr_eq(cached_identity, &identity)
                    && *cached_epoch == terrain_epoch
                    && *cached_walls == walls_passable =>
            {
                terrain.clone()
            }
            _ => {
                let pack =
                    kk_runner::pathfinder::TerrainPack::from_room_rows(
                        self.engine.world().rooms().iter().map(|(name, room)| {
                            (name.to_string(), room.terrain().as_slice().to_vec())
                        }),
                    );
                let terrain = pack
                    .native_with_walls(walls_passable)
                    .map_err(anyhow::Error::msg)?;
                self.pathfinder_terrain =
                    Some((identity, terrain_epoch, walls_passable, terrain.clone()));
                terrain
            }
        };
        let source: &dyn WorldSource = self.engine.world();
        // 包括提前返回路径在内，检查玩家执行期间世界未改变。
        let assert_bound = || {
            debug_assert_eq!(
                source.generation(),
                bound_generation,
                "玩家执行期间世界代际不得变动（同一 tick 绑定同一版本）"
            );
        };
        // 覆盖顺序必须包含全部注册玩家且无重复，否则可能漏跑或重复运行。
        let handles = match &self.execution_order_for_test {
            Some(order) => {
                let mut sorted = order.clone();
                sorted.sort();
                assert_eq!(sorted, registered, "执行序覆盖必须是注册玩家集的排列");
                // 只在启用测试覆盖时保存排列，默认执行不额外复制观测记录。
                self.last_execution_order = Some(order.clone());
                order.clone()
            }
            None => handles,
        };
        let mut requested = handles
            .into_iter()
            .filter(|handle| codes.contains_key(handle));
        let mut pending = BTreeMap::<String, crate::PendingExecution>::new();
        let (notify, completion) = std::sync::mpsc::channel();
        let mut failure: Option<String> = None;
        let started = std::time::Instant::now();
        // KKPV 通道的公开地形束按代际发布一次、整轮共享：acquire 命中
        // （同代际重入或重试轮复投影）不重编码；miss 才投影+编码+发布。
        // 耗时归入投影相。失败即整轮失败——没有公开束任何玩家都无法装配。
        let mut kkpv_public: Option<kk_runner::SharedKkpvBundle> = None;
        if self.kkpv_channel {
            let t_public = std::time::Instant::now();
            match self.public_bundle_for_epoch(source, epoch) {
                Ok(bundle) => kkpv_public = Some(bundle),
                Err(e) => failure = Some(e.to_string()),
            }
            project_ns += t_public.elapsed().as_nanos();
        }
        loop {
            while failure.is_none() && pending.len() < self.player_concurrency {
                let Some(handle) = requested.next() else {
                    break;
                };
                let code = &codes[&handle];
                let state = self.engine.players().get(&handle);
                let raw_mem = state.map(|p| p.raw_memory.clone()).unwrap_or_default();
                let segments_json = match state {
                    Some(p) if !p.active_request.is_empty() => serde_json::to_string(
                        &p.active_request
                            .iter()
                            .map(|id| (*id, p.segments.get(id).cloned().unwrap_or_default()))
                            .collect::<Vec<(u8, String)>>(),
                    )
                    .unwrap_or_else(|_| "[]".into()),
                    _ => "[]".into(),
                };
                let foreign_json = self
                    .resolve_foreign_segment(&handle)
                    .unwrap_or(serde_json::Value::Null);
                let gcl = self
                    .engine
                    .players()
                    .get(&handle)
                    .map(|p| p.gcl)
                    .unwrap_or(0);
                let t_project = std::time::Instant::now();
                // KKPV 通道：对象记录走二进制块束，world 是不含 objects 的信封；
                // JSON 参考通道：整世界一个 Value。市场都注入 world 顶层。
                let (world, kkpv_bundle) = if self.kkpv_channel {
                    match source.kkpv_projection_for_viewer(
                        &handle,
                        gcl,
                        self.engine.extensions(),
                        self.player_surface.as_deref(),
                        self.engine.plan_hash().bytes(),
                        kkpv_audience_hash(&handle),
                    ) {
                        Some(projection) => match projection
                            .map_err(|e| anyhow::anyhow!("KKPV 投影失败（玩家 {handle}）: {e}"))
                            .and_then(|(mut envelope, blocks)| {
                                if let Some(obj) = envelope.as_object_mut() {
                                    obj.insert("market".into(), market_view(&self.engine, &handle));
                                }
                                kk_schema::kkpv::encode_bundle(
                                    &blocks,
                                    &kk_schema::kkpv::KkpvLimits::default(),
                                )
                                .map_err(|e| {
                                    anyhow::anyhow!("KKPV 块束编码失败（玩家 {handle}）: {e}")
                                })
                                .map(|bundle| (envelope, Some(bundle)))
                            }) {
                            Ok(pair) => pair,
                            Err(e) => {
                                // 投影耗时归入投影相，不混入执行相。
                                project_ns += t_project.elapsed().as_nanos();
                                failure = Some(e.to_string());
                                continue;
                            }
                        },
                        None => {
                            project_ns += t_project.elapsed().as_nanos();
                            failure = Some(format!("世界源未提供 KKPV 投影（玩家 {handle}）"));
                            continue;
                        }
                    }
                } else {
                    let mut world = source.world_json_for_viewer_with_extensions(
                        &handle,
                        gcl,
                        self.engine.extensions(),
                        self.player_surface.as_deref(),
                    );
                    // 市场读取已提交的引擎状态，随世界 JSON 注入，不存放在 V8 持久闭包中。
                    if let Some(obj) = world.as_object_mut() {
                        obj.insert("market".into(), market_view(&self.engine, &handle));
                    }
                    (world, None)
                };
                project_ns += t_project.elapsed().as_nanos();
                let budget = kk_core::cpu::RunBudget::new(&self.engine.players()[&handle].cpu);
                match self.players.as_mut().expect("已检查分片创建").dispatch_job(
                    &handle,
                    kk_runner::RunJob {
                        terrain: Some(terrain.clone()),
                        world,
                        kkpv_bundle,
                        kkpv_public: kkpv_public.clone(),
                        code: code.clone(),
                        epoch,
                        attempt: 0,
                        deadline: None,
                        raw_memory: raw_mem,
                        segments_json,
                        foreign_json: foreign_json.to_string(),
                        budget,
                    },
                    Some(notify.clone()),
                ) {
                    Ok(job) => {
                        pending.insert(handle, job);
                    }
                    Err(e) => {
                        failure = Some(format!("基础设施故障（玩家 {handle}）: {e}"));
                    }
                }
            }
            if pending.is_empty() {
                break;
            }
            let completed: Vec<_> = pending
                .iter()
                .filter_map(|(handle, job)| job.try_take().map(|result| (handle.clone(), result)))
                .collect();
            if completed.is_empty() {
                let deadline = pending.values().map(|job| job.deadline()).min().unwrap();
                let _ = completion
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()));
                continue;
            }
            for (handle, result) in completed {
                pending.remove(&handle);
                if let RunOutcome::InfraFailure(err) = &result.outcome {
                    failure.get_or_insert_with(|| format!("基础设施故障（玩家 {handle}）: {err}"));
                } else {
                    results.insert(handle.clone(), result);
                    if self.fail_after_player_for_test.as_deref() == Some(handle.as_str()) {
                        self.fail_after_player_for_test = None;
                        failure.get_or_insert_with(|| {
                            format!("故障注入：玩家 {handle} 回执后执行轮中止")
                        });
                    }
                }
            }
        }
        let run_ns = started.elapsed().as_nanos().saturating_sub(project_ns);
        assert_bound();
        if let Some(reason) = failure {
            self.execution_failure = Some(ExecutionFailure {
                confirmed: results,
                reason: reason.clone(),
            });
            anyhow::bail!(reason);
        }
        Ok((results, run_ns, project_ns))
    }

    /// 合并已清洗并绑定玩家身份的意图，不使用 JavaScript 自报的身份。
    pub(super) fn batch_from_results(results: &BTreeMap<String, RunResult>) -> IntentBatch {
        let mut batch = IntentBatch::default();
        for (registered_player, result) in results {
            // 注册表键是身份真相源；自报 player 和其他玩家键均不能越过此边界。
            if let Some(intents) = result.intents.users.get(registered_player) {
                batch
                    .users
                    .insert(registered_player.clone(), intents.clone());
            }
        }
        batch
    }

    /// 结算成功后向输入日志追加一段（未启用时无操作）。调用点必须在引擎
    /// 提交之后；失败轮/被丢弃轮不得调用。返回 Err 表示日志 I/O 故障。
    fn log_round(
        &mut self,
        tick: u64,
        generation: u64,
        source: &str,
        now_ms: u64,
        intents: Value,
        players: Value,
    ) -> Result<(), String> {
        if self.input_log.is_none() {
            return Ok(());
        }
        if self.fail_next_log_write {
            self.fail_next_log_write = false;
            return Err("故障注入：输入日志写入失败（测试钩子）".into());
        }
        // 身份先取后借：input_log 的可变借用不能与 self 的方法调用共存。
        let (plan_hash, strategy, disabled) = self.log_identity_fields();
        let log = self.input_log.as_mut().expect("上方已判在位");
        log.append_round(
            tick,
            generation,
            source,
            now_ms,
            intents,
            players,
            &plan_hash,
            strategy.as_deref(),
            &disabled,
        )
        .map_err(|e| e.to_string())
    }

    /// 日志写入失败的统一处置：日志链已与世界脱节，摘除停用；本轮世界
    /// 事实上已提交，错误文案必须如实说明不得重试结算。
    fn drop_log_for_io_failure(&mut self, error: String) -> anyhow::Error {
        self.input_log = None;
        anyhow::Error::new(input_log::CommittedLogError {
            committed_time: self.get_game_time(),
            reason: error,
        })
    }

    /// 按发布者当前公开列表解析外部段；未指定 ID 时使用其默认公开段。
    /// 默认段也必须仍在公开列表中。无请求、无发布者或无授权时返回 None；
    /// 已授权但未写入的段返回 `data: null`，区别于活动段的空字符串。
    pub(super) fn resolve_foreign_segment(&self, reader: &str) -> Option<serde_json::Value> {
        let req = self.engine.players().get(reader)?.foreign_request.clone()?;
        let (username, id_opt) = req;
        let publisher = self.engine.players().get(&username)?;
        let id = match id_opt {
            Some(id) => {
                if !publisher.public_ids.contains(&id) {
                    return None;
                }
                id
            }
            // 默认公开段仍须通过当前公开列表检查，残留的默认段 ID 不授予访问权限。
            None => {
                let id = publisher.default_public?;
                if !publisher.public_ids.contains(&id) {
                    return None;
                }
                id
            }
        };
        Some(serde_json::json!({
            "username": username,
            "id": id,
            "data": publisher
                .segments
                .get(&id)
                .map(|s| serde_json::Value::String(s.clone()))
                .unwrap_or(serde_json::Value::Null),
        }))
    }

    /// 直接结算调用方构造的意图，不执行玩家代码或提交玩家 Memory、CPU 桶。
    /// 有失败轮缓存时拒绝调用；本入口失败不会缓存意图，重试须重新提供。
    pub fn settle_external(&mut self, batch: IntentBatch) -> anyhow::Result<()> {
        self.no_pending_round("settle_external")?;
        let (tick, generation) = (self.engine.world().time(), self.engine.world().generation());
        // 外部轮以线格式为权威：结算消费的批必须与日志记录、回放重建的
        // 是同一规范化形态——直接构造的批先过 to_json→from_json 规范化
        // （清洗限长/钳制在此生效，原样结算超规范载荷会让回放重放出
        // 偏离原轮的历史）。
        let intents = batch.to_json();
        let mut batch = IntentBatch::default();
        for (player, wire) in intents.as_object().expect("to_json 恒产出玩家键对象") {
            batch.extend_from(IntentBatch::from_json(player, wire));
        }
        let input = TickInput::now();
        self.settle_world(batch, input)?;
        if self.input_log.is_some()
            && let Err(e) = self.log_round(
                tick,
                generation,
                "external",
                input.now_ms,
                intents,
                json!({}),
            )
        {
            return Err(self.drop_log_for_io_failure(e));
        }
        Ok(())
    }

    /// 执行结果不完整时禁止开始新轮，等待调用方显式处置。
    pub(super) fn no_execution_failure(&self, op: &str) -> anyhow::Result<()> {
        if let Some(failure) = &self.execution_failure {
            anyhow::bail!(
                "{op} 被拒：执行轮存在基础设施故障，已确认 {} 个玩家结果；先 discard_pending_round 显式放弃或重建分片。原因：{}",
                failure.confirmed.len(),
                failure.reason
            );
        }
        Ok(())
    }

    /// 防止缓存玩家结果与新世界结算混用。
    pub(super) fn no_pending_round(&self, op: &str) -> anyhow::Result<()> {
        self.no_execution_failure(op)?;
        if self.pending.is_some() {
            anyhow::bail!(
                "{op} 被拒：存在未处置的失败轮候选（先 run_players 重试或 discard_pending_round）"
            );
        }
        Ok(())
    }

    /// 修改世界或市场前，同时检查玩家失败缓存与引擎恢复状态。
    /// `tick`、`settle_external` 失败可能只有恢复状态而没有缓存；市场修改
    /// 又不会改变世界 generation，因此不能仅依赖引擎的世界版本检查。
    pub(super) fn no_open_round(&self, op: &str) -> anyhow::Result<()> {
        self.no_pending_round(op)?;
        if self.needs_mod_recovery() {
            anyhow::bail!("{op} 被拒：Mod VM 待恢复；先 rebuild_mod_realm 或重试结算");
        }
        if matches!(self.engine.run_state(), RunState::NeedsRecovery { .. }) {
            anyhow::bail!(
                "{op} 被拒：引擎处于恢复态（上次结算段失败、世界保持上一 tick）——先重试结算或 discard_pending_round 放弃恢复"
            );
        }
        Ok(())
    }

    /// 保存最近一次完成结算的 console，并转换适配器结果。
    /// `executed` 表示进入求值流程，语法错误也属于该流程。
    pub(super) fn finish_player_run(&mut self, handle: &str, result: RunResult) -> PlayerRun {
        let executed = !matches!(result.outcome, RunOutcome::Terminated)
            && result.expr.get("kind").and_then(Value::as_str) != Some("inject_failed");
        let console = result.console.clone();
        self.last_console
            .insert(handle.to_string(), console.clone());
        PlayerRun {
            executed,
            expr: result.expr,
            console,
        }
    }

    /// 不运行玩家代码，逐轮结算 `count` 个 tick。
    /// 途中失败仅保留失败 tick 之前的状态，已经成功的 tick 不会撤销。
    pub fn tick(&mut self, count: u64) -> anyhow::Result<()> {
        self.no_pending_round("tick")?;
        for _ in 0..count {
            let (tick, generation) = (self.engine.world().time(), self.engine.world().generation());
            let input = TickInput::now();
            self.settle_world(IntentBatch::default(), input)?;
            if self.input_log.is_some()
                && let Err(e) = self.log_round(
                    tick,
                    generation,
                    "advance",
                    input.now_ms,
                    json!({}),
                    json!({}),
                )
            {
                return Err(self.drop_log_for_io_failure(e));
            }
        }
        Ok(())
    }

    /// 回放一轮已记录的结算输入（G25.b）：注入记录的墙钟与写回，**不写
    /// 新日志段**（链只属于产生它的那轮）。原轮必然成功才记段，此处的
    /// 失败即实现缺陷或载荷损坏——引擎会进入恢复态，不可原地续放。
    pub fn settle_replayed_round(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
        writebacks: BTreeMap<String, PlayerWriteback>,
    ) -> anyhow::Result<()> {
        self.no_pending_round("settle_replayed_round")?;
        self.settle_world_with_writebacks(batch, input, writebacks)
    }

    /// 从当前已恢复状态起逐段重放（G25.b）。头段先与引擎配对（种子/
    /// 引擎版本——随机流派生根不在逐段身份里）；段集须已经过配对选择
    /// （`segments_from_checkpoint`）。逐段校验身份快照与轮身份，任何
    /// 失败立即终止并携带段 seq；不写新日志段。
    pub fn replay_input_rounds(
        &mut self,
        header: &Value,
        segments: &[Value],
    ) -> anyhow::Result<()> {
        replay::assert_header_paired(header, self.world_seed())
            .context("回放入口：头段身份配对失败")?;
        for segment in segments {
            let seq = segment["seq"].as_u64().unwrap_or(0);
            let at = || {
                format!(
                    "段 seq={seq}（tick={} source={}）：",
                    segment["tick"].as_u64().unwrap_or(0),
                    segment["source"].as_str().unwrap_or("?")
                )
            };
            let (plan_hash, strategy, disabled) = self.log_identity_fields();
            replay::assert_segment_identity(segment, &plan_hash, strategy.as_deref(), &disabled)
                .with_context(|| format!("{}身份快照与引擎不符", at()))?;
            let tick = segment["tick"].as_u64().context("段缺少 tick")?;
            let generation = segment["generation"]
                .as_u64()
                .context("段缺少 generation")?;
            // tick 硬门：段记录的是结算前世界时间，逐段必须与恢复后的
            // 世界时间轴咬合（缺口/乱序/宿主未记录变异在此暴露）。
            anyhow::ensure!(
                tick == self.engine.world().time(),
                "{}tick={tick} 与世界时间 {} 不符（配对缺口、宿主变异或日志损坏）",
                at(),
                self.engine.world().time()
            );
            // generation 强校验：结算前对结算前。对象放置等合法宿主变异
            // 也会 bump generation，故以独立文案区分而非并入硬门。
            anyhow::ensure!(
                generation == self.engine.world().generation(),
                "{}generation={generation} 与世界 {} 不符（轮间存在未记录的宿主世界变异或日志损坏）",
                at(),
                self.engine.world().generation()
            );
            let round = replay::rebuild_round(segment).with_context(at)?;
            self.settle_replayed_round(round.batch, round.input, round.writebacks)
                .with_context(|| {
                    format!("{}回放结算失败（原轮必然成功——实现缺陷或载荷损坏）", at())
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
