//! 无开放轮次的包/API 切换（G24.a）：世界与账户保持已提交状态，原地
//! 换装 Mod 执行层（包集合、冻结计划、玩家面与工作者配置）。
//!
//! 语义边界：
//! - 允许包集合增减与宿主选项（selection/configs/预算/停用阈值）调整：
//!   交集包的 manifest 与文件摘要必须逐项一致；同包内容变更属内容级
//!   迁移（G23 后续），本入口明确拒绝、不修补。
//! - 去包残留硬门：已移除类型的 Mod 对象、已移除包的扩展字段与
//!   namespace state 会把世界钉死（每轮结算与冷恢复都拒绝），由换计划
//!   收养的内容校验前置拒绝并指路 G23；已移除建筑退化为惰性 Generic
//!   对象（沿用既有的静默跳过语义）。
//! - 停用集合投影到新包集后重放并核对闭包；故障诊断三域指向已移除包
//!   的记录丢弃（诊断是瞬态信号，不是数据）。
//! - 玩家工作者重建：预算与模块表（玩家代码）保留；常量、Mod 玩家面
//!   与地形通行旗标按新计划重注入——计划派生常量键是封闭集
//!   （LINK_CAPACITY、仓库十键、战斗射程四键、移动门两键、
//!   RESOURCES_ALL），create 路径确定性覆盖或移除；通行旗标是
//!   覆盖式字段（terrain_walls_passable），非封闭集成员也随重建
//!   归位，旧计划值不会残留。V8 堆与模块级状态不保留（与冷恢复同口径）。
//! - 运行 trace：开启意愿与既有缓冲都转移到新 VM——切换是继续运行而
//!   非冷恢复，诊断连续性不中断。
//! - 与冷恢复同口径丢弃：perf 样本、已装的测试故障钩子与 kkpv 公开束
//!   （后者必须重建——束头嵌 plan_hash）。
//!
//! 实现走替代分片模式（与 `restore_checkpoint` 同骨架）：全部可失败
//! 步骤发生在候选分片上，`*self = replacement` 是唯一的原位变更点；
//! 任一步失败旧分片原样继续运行。与冷恢复的区别在于身份门的方向：
//! 冷恢复要求新旧同身份（ModIdentity/plan_hash/执行策略逐项等值），
//! 切换恰好要换身份——这三道等值门不适用，取而代之的是交集内容等值、
//! 残留校验与停用投影。
use super::*;
use anyhow::Context;

#[cfg(test)]
mod tests;

impl EmbedEngine {
    /// 在已提交边界把 Mod 包集合切换为 `packages`（世界与账户不动）。
    /// 成功返回切换摘要：前后 plan_hash 与执行策略身份、增删包清单。
    pub fn switch_mod_packages(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
        options: &RegisterOptions,
    ) -> anyhow::Result<Value> {
        self.no_open_round("switch_mod_packages")?;
        let old_vm = self
            .mod_realm
            .as_ref()
            .context("原生分片没有可切换的 Mod 执行层；重建分片请用 create_shard_with_mods")?;
        if !old_vm.borrow().is_checkpoint_current() {
            anyhow::bail!("包切换被拒：同线程存在其他活跃 Mod 分片或玩家 VM；须按进入栈顺序切换");
        }
        // 从活状态捕获提交快照（不经 JSON 往返，引擎身份门不适用）。
        let mut engine_saved = self.engine.checkpoint().map_err(anyhow::Error::msg)?;
        let executor = self.players.as_ref().context("分片未创建")?;
        let workers: BTreeMap<String, checkpoint::Worker> = executor
            .registrations
            .iter()
            .map(|(name, registration)| {
                let worker =
                    checkpoint::Worker::capture(&registration.config, &registration.modules);
                worker.config()?;
                Ok((name.clone(), worker))
            })
            .collect::<anyhow::Result<_>>()?;
        let old_identity = {
            let vm = old_vm.borrow();
            let (old_packages, old_options) = vm.checkpoint_input();
            checkpoint::ModIdentity::capture(old_packages, old_options)?
        };
        let new_identity = checkpoint::ModIdentity::capture(packages, options)?;
        // 交集包逐项等值：同包内容变更 = 内容级迁移，拒绝。
        for (id, old_entry) in &old_identity.packages {
            if let Some(new_entry) = new_identity.packages.get(id)
                && old_entry != new_entry
            {
                anyhow::bail!(
                    "包切换被拒：包 {id} 的 manifest/文件摘要与当前不一致；\
                     同包内容变更属内容级迁移（G23 后续），本入口不修补"
                );
            }
        }
        let old_disabled = self.disabled_mods();
        let (old_installed, plan_hash_before) = {
            let vm = old_vm.borrow();
            (
                vm.composition()
                    .selection
                    .selected
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<String>>(),
                self.engine.plan_hash().hex(),
            )
        };
        let strategy_before = old_vm
            .borrow()
            .composition()
            .execution_strategy_digest()
            .map_err(anyhow::Error::msg)?;
        let (fault_streak, last_fault, last_auto_disable) = {
            let vm = old_vm.borrow();
            (
                vm.fault_streak.clone(),
                vm.last_fault.clone(),
                vm.last_auto_disable.clone(),
            )
        };

        // 候选分片：注册 → canary 预检 → install_hosted_plan → 玩家
        // spawn 全链。注入的工作者配置保留预算与模块表来源；计划派生
        // 字段（常量封闭集、mod_api、扩展意图）由装配路径按新计划覆盖。
        // 分片以当前世界种子构造：realm 装配期就要注入种子，口径与
        // restore_checkpoint 的载荷种子一致（G22.b）。
        let mut replacement = Self::with_world_seed(self.engine.world().seed());
        replacement.set_player_concurrency(self.player_concurrency)?;
        replacement.set_kkpv_channel(self.kkpv_channel);
        for (name, worker) in &workers {
            replacement
                .sandbox_configs_for_test
                .insert(name.clone(), worker.config()?);
        }
        let shard = json!({"players": workers.keys().collect::<Vec<_>>(), "rooms": []});
        replacement.create_shard_with_mods(&shard, packages, options.clone())?;
        // 停用投影按新计划的**装配选择集**求交：发现集包含未选择包，
        // 对其求交会把未安装 ID 喂给停用闭包（按未知 Mod 拒绝）。
        let new_installed: BTreeSet<String> = replacement
            .mod_realm
            .as_ref()
            .context("新分片须装配 Mod VM")?
            .borrow()
            .composition()
            .selection
            .selected
            .iter()
            .cloned()
            .collect();
        let projected: BTreeSet<String> =
            old_disabled.intersection(&new_installed).cloned().collect();
        // 孤立 namespace state 显式分诊：链式提交对每个执行过的绑定都落
        // 键，未写 state 的处理器是 null 占位（无信息量，随包移除丢弃）；
        // 非 null 是 Mod 提交过的实数据，静默删除等于丢数据——拒绝并
        // 指路内容级迁移。
        let orphan_data: Vec<&String> = engine_saved
            .mod_states
            .iter()
            .filter(|(namespace, state)| !new_installed.contains(*namespace) && !state.is_null())
            .map(|(namespace, _)| namespace)
            .collect();
        anyhow::ensure!(
            orphan_data.is_empty(),
            "包切换被拒：已移除包存在非空 namespace state 残留 {orphan_data:?}；\
             先在包内清理 state 或等待 G23 内容级迁移，本入口不删除数据"
        );
        engine_saved
            .mod_states
            .retain(|namespace, _| new_installed.contains(namespace));
        for id in &projected {
            replacement.disable_mod(id)?;
        }
        anyhow::ensure!(
            replacement.disabled_mods() == projected,
            "包切换被拒：停用集合投影到新包集后不满足依赖闭包"
        );
        replacement.rebuild_mod_realm()?;
        // 换计划收养：对象/扩展/namespace state 的内容校验对新计划执行，
        // 命中残留即拒绝——候选分片报废，旧分片不动。
        replacement
            .engine
            .restore_checkpoint_replanned(engine_saved)
            .map_err(|error| {
                anyhow::anyhow!(
                    "包切换被拒：新计划下存在已移除包的残留状态（Mod 对象/扩展字段/\
                     namespace state）；先清理残留或等待 G23 内容级迁移：{error}"
                )
            })?;
        for (name, worker) in &workers {
            replacement.set_player_modules(name, worker.modules())?;
        }
        // 故障诊断三域：指向已移除包的记录丢弃，其余原样转移。
        if let Some(vm) = &replacement.mod_realm {
            let mut vm = vm.borrow_mut();
            vm.fault_streak = fault_streak.filter(|streak| new_installed.contains(&streak.mod_id));
            vm.last_fault = last_fault.filter(|fault| new_installed.contains(&fault.error.context));
            vm.last_auto_disable =
                last_auto_disable.filter(|record| new_installed.contains(&record.mod_id));
        }
        // 剩余可失败读取（摘要）与注入点都在 retire 之前：此后的路径
        // 只做不可失败的转移，`*self = replacement` 是唯一的原位变更。
        let plan_hash_after = replacement.engine.plan_hash().hex();
        let strategy_after = replacement
            .mod_realm
            .as_ref()
            .context("切换后 Mod VM 在位")?
            .borrow()
            .composition()
            .execution_strategy_digest()
            .map_err(anyhow::Error::msg)?;
        // 增删按装配选择集报告：发现集含未选择包，不是语义变化面。
        let added: Vec<&String> = new_installed
            .iter()
            .filter(|id| !old_installed.contains(*id))
            .collect();
        let removed: Vec<&String> = old_installed
            .iter()
            .filter(|id| !new_installed.contains(*id))
            .collect();
        // 注入点在候选全部验证之后、retire 之前：旧分片（含 realm、
        // trace 与 console 历史）保持完整，替代分片整体报废。
        if self.fail_next_switch_for_test {
            self.fail_next_switch_for_test = false;
            anyhow::bail!("包切换失败（测试注入）");
        }
        if let (Some(new), Some(old)) = (&replacement.mod_realm, &self.mod_realm) {
            new.borrow_mut()
                .retire_previous_for_checkpoint(&mut old.borrow_mut())
                .map_err(anyhow::Error::msg)?;
            // retire 只取走旧 realm；ModVm 结构仍在，trace 缓冲可以转移。
            if let Some(log) = old.borrow_mut().trace.take() {
                new.borrow_mut().trace = Some(log);
            }
        }
        if self.mod_trace_enabled {
            replacement.enable_mod_trace();
        }
        // console 历史最后转移（clone）：此后无可失败步骤，旧分片在切换
        // 前的每个失败阶段都保持完整。
        replacement.last_console = self.last_console.clone();
        *self = replacement;
        Ok(json!({
            "plan_hash_before": plan_hash_before,
            "plan_hash_after": plan_hash_after,
            "execution_strategy_before": strategy_before,
            "execution_strategy_after": strategy_after,
            "added": added,
            "removed": removed,
        }))
    }
}
