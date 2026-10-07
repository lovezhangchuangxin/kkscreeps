//! 按 CPU 窗口注入 JSON/KKPV 世界，重置每轮计费与看门狗状态。
use super::*;

impl Sandbox {
    /// 制定预算计划并注入世界数据，不提交桶余额。
    /// SkipPenalty 表示负余额停跑；回充已暂存，仍需 settle_bill 或 discard_bill。
    pub fn begin_tick(&mut self, world: &serde_json::Value) -> anyhow::Result<TickPlan> {
        let plan = self.budget.begin_tick();
        self.begin_tick_plan(world, None, None, plan)
    }

    /// [`Sandbox::begin_tick`] 的 KKPV 通道形态：`world` 是不含对象数组与
    /// 地形的世界信封，对象记录由沙箱从 `private_bundle` 私有块束解码，
    /// 地形由 `public_bundle` 公开块束直读（共享内存零拷贝挂载）。两个
    /// 束须同时提供——只给其一说明通道装配有误，直接失败。
    pub fn begin_tick_kkpv(
        &mut self,
        world: &serde_json::Value,
        public_bundle: &crate::shared_bundle::SharedKkpvBundle,
        private_bundle: &crate::shared_bundle::SharedKkpvBundle,
    ) -> anyhow::Result<TickPlan> {
        let plan = self.budget.begin_tick();
        self.begin_tick_plan(world, Some(public_bundle), Some(private_bundle), plan)
    }

    /// 使用宿主制定的预算；本地桶只作为本次执行与计费的工作副本。
    pub fn begin_tick_with_budget(
        &mut self,
        world: &serde_json::Value,
        budget: &kk_core::cpu::RunBudget,
    ) -> anyhow::Result<TickPlan> {
        self.budget = budget.bucket().clone();
        self.begin_tick_plan(world, None, None, budget.plan().clone())
    }

    /// [`Sandbox::begin_tick_with_budget`] 的 KKPV 通道形态。
    pub fn begin_tick_kkpv_with_budget(
        &mut self,
        world: &serde_json::Value,
        public_bundle: &crate::shared_bundle::SharedKkpvBundle,
        private_bundle: &crate::shared_bundle::SharedKkpvBundle,
        budget: &kk_core::cpu::RunBudget,
    ) -> anyhow::Result<TickPlan> {
        self.budget = budget.bucket().clone();
        self.begin_tick_plan(
            world,
            Some(public_bundle),
            Some(private_bundle),
            budget.plan().clone(),
        )
    }

    /// 无可信身份的原始 JSON 地形按内容核对，不能仅用 rev 判断换世界。
    pub(super) fn json_terrain_pack(
        &mut self,
        world: &serde_json::Value,
    ) -> anyhow::Result<std::sync::Arc<crate::pathfinder::TerrainPack>> {
        // 原始 JSON 不带可信世界身份：rev 相同也须按内容核对，防止换世界串缓存。
        let pack =
            crate::pathfinder::TerrainPack::from_world_json(world).map_err(anyhow::Error::msg)?;
        if let Some(cached) = &self.terrain_pack_cache
            && **cached == pack
        {
            return Ok(cached.clone());
        }
        let pack = std::sync::Arc::new(pack);
        self.terrain_pack_cache = Some(pack.clone());
        Ok(pack)
    }

    /// 宿主为下一次注入提供世界独立快照；消费一次后恢复原始输入校验。
    pub fn set_host_terrain(&mut self, terrain: Option<std::sync::Arc<kk_pathfinder::TerrainSet>>) {
        self.host_terrain = terrain;
    }

    pub(super) fn begin_tick_plan(
        &mut self,
        world: &serde_json::Value,
        kkpv_public: Option<&crate::shared_bundle::SharedKkpvBundle>,
        kkpv_bundle: Option<&crate::shared_bundle::SharedKkpvBundle>,
        plan: TickPlan,
    ) -> anyhow::Result<TickPlan> {
        // 调用方应已提交或丢弃上一轮候选；清除残留，避免它混入当前 tick。
        let host_terrain = self.host_terrain.take();
        self.pending_bill = None;
        self.pending_recharge = None;
        match plan {
            // 停跑分支不注入世界或执行玩家代码；清除上一轮指令，只暂存本轮回充。
            TickPlan::SkipPenalty => {
                self.pending_intents = None;
                self.pending_recharge = Some(self.budget.skip_recharge());
                Ok(TickPlan::SkipPenalty)
            }
            TickPlan::Run { .. } => {
                // 清除可能遗留的终止标志与上一 tick 未消费的 intents
                //（消费方漏取不得把旧 intent 泄漏进新 tick），然后武装看门狗
                self.terminated_this_tick = false;
                self.pending_intents = None;
                self.runtime.v8_isolate().cancel_terminate_execution();
                // 寻路地形随世界注入 OpState（停跑分支不注入：不执行玩家
                // 代码，op 不会被调；下一 Run tick 整体替换旧值）。
                // 可信宿主的快照直接复用；原始 JSON 按内容核对，
                // KKPV 通道持束句柄、首次搜索懒解码（见 pathfinder.rs）。
                let tick_terrain = if let Some(terrain) = host_terrain {
                    crate::pathfinder::TickTerrain::frozen(terrain)
                } else {
                    match kkpv_public {
                        Some(public) => crate::pathfinder::TickTerrain::kkpv(
                            public.clone(),
                            self.terrain_walls_passable,
                        ),
                        None => {
                            let pack = self.json_terrain_pack(world)?;
                            crate::pathfinder::TickTerrain::json(pack, self.terrain_walls_passable)
                        }
                    }
                };
                self.runtime.op_state().borrow_mut().put(tick_terrain);
                // 将沙箱预算作为独立参数交给 beginTick，替换世界数据中的 CPU 占位值，
                // 避免为了更新 CPU 字段克隆整个世界数据。
                let cpu_json = {
                    // 在 begin_tick 采样 V8 堆统计，字段名使用 snake_case。
                    // JS getHeapStatistics 返回此快照；externally_allocated_size 无对应统计，固定为 0。
                    let hs = self.runtime.v8_isolate().get_heap_statistics();
                    serde_json::json!({
                        "limit": self.budget.limit,
                        "tickLimit": self.budget.tick_limit(),
                        "bucket": self.budget.bucket(),
                        "heap": {
                            "total_heap_size": hs.total_heap_size(),
                            "total_heap_size_executable": hs.total_heap_size_executable(),
                            "total_physical_size": hs.total_physical_size(),
                            "total_available_size": hs.total_available_size(),
                            "used_heap_size": hs.used_heap_size(),
                            "heap_size_limit": hs.heap_size_limit(),
                            "malloced_memory": hs.malloced_memory(),
                            "peak_malloced_memory": hs.peak_malloced_memory(),
                            "does_zap_garbage": hs.does_zap_garbage() as u32,
                            "externally_allocated_size": 0,
                        },
                    })
                    .to_string()
                };
                // 先序列化世界数据再启动看门狗；启动后的失败路径必须解除期限。
                let world_json = serde_json::to_string(world)?;
                let deadline = Instant::now()
                    + Duration::from_millis(self.budget.tick_limit().max(1.0) as u64);
                *self.shared.thread_cpu_start.lock().unwrap() =
                    Some(crate::cpu_clock::thread_cpu_time()?);
                let deadline = self
                    .command_deadline
                    .map_or(deadline, |command| command.min(deadline));
                self.watchdog.arm(deadline);
                *self.shared.tick_start.lock().unwrap() = Some(Instant::now());
                self.shared.reset_console_gate();
                // 先恢复堆上限再清诊断标志，否则会丢失近堆回调曾扩大上限的信息。
                self.restore_heap_limit();
                self.shared.near_heap_fired.store(false, Ordering::Relaxed);
                self.shared.halt_requested.store(false, Ordering::Relaxed);
                // 世界注入也受本 tick 期限约束；失败时生成完整预算费用并清理执行状态。
                // 参数通过 V8 值传递（字符串或 ArrayBuffer），不拼接进 JavaScript 源码。
                // KKPV 通道：公开束是跨玩家共享的只读内存（零拷贝挂载），
                // 私有束是本玩家独占的束（Vec 已移入 backing store，同样零拷贝）。
                let channel_args_ok = kkpv_public.is_some() == kkpv_bundle.is_some();
                match (channel_args_ok, kkpv_public, kkpv_bundle) {
                    (true, Some(public), Some(private)) => call_host_kkpv_begin(
                        &self.tokio,
                        &mut self.runtime,
                        &self.host.begin_tick_kkpv,
                        &world_json,
                        public,
                        private,
                        &cpu_json,
                    ),
                    (false, _, _) => {
                        // 看门狗已武装；失败清理统一由下方 inspect_err 完成。
                        anyhow::bail!("KKPV 通道的公开束与私有束须同时提供或同时为空");
                    }
                    (true, _, _) => call_host_unit(
                        &self.tokio,
                        &mut self.runtime,
                        &self.host.begin_tick,
                        &[world_json, cpu_json],
                    ),
                }
                .inspect_err(|_| self.after_termination())?;
                Ok(TickPlan::Run {
                    tick_limit: self.budget.tick_limit(),
                })
            }
        }
    }
}
