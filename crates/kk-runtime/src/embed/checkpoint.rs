//! 冷恢复先在替代分片完成全部装配、身份核验和状态解码，再切换现行分片。
use super::*;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

mod identity;
mod migrate;

use identity::{ModIdentity, Worker};
use migrate::CURRENT_FORMAT_VERSION;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    /// JSON 载荷域布局版本（信封 version 是线格式；本字段由迁移框架
    /// 维护，见 migrate 模块）。必填——缺失即拒，v1 载荷经注册迁移升入。
    checkpoint_format_version: u32,
    engine_version: String,
    room_schema_version: u16,
    engine: kk_engine::settle::checkpoint::EngineCheckpoint,
    workers: BTreeMap<String, Worker>,
    mods: Option<ModIdentity>,
    /// 执行策略身份：冻结计划 + 停用闭包 + 每槽
    /// 派生执行面的域分隔摘要，保存时取自 VM 组合报告；恢复在重装配
    /// 与停用重放后重算等值核对。引用而非复制 engine.plan_hash。
    execution_strategy: Option<String>,
    disabled_mods: BTreeSet<String>,
    fault_streak: Option<crate::ModFaultStreak>,
    last_fault: Option<crate::ModSettlementFault>,
    last_auto_disable: Option<crate::ModAutoDisable>,
    concurrency: usize,
    kkpv: bool,
    last_console: BTreeMap<String, Vec<String>>,
    metadata: Value,
}

impl EmbedEngine {
    /// 导出完整已提交状态。metadata 供调用方保存场景身份、alias 等自身持久数据。
    /// VM 堆、模块缓存、失败轮、计时样本、地形编码缓存与运行 trace
    /// 缓冲不进入检查点（trace 开启意愿经引擎开启标志跨恢复保留）。
    pub fn checkpoint(&self, metadata: Value) -> anyhow::Result<Value> {
        self.no_open_round("checkpoint")?;
        let executor = self.players.as_ref().context("分片未创建")?;
        let workers = executor
            .registrations
            .iter()
            .map(|(name, registration)| {
                let worker = Worker::capture(&registration.config, &registration.modules);
                worker.config()?;
                Ok((name.clone(), worker))
            })
            .collect::<anyhow::Result<_>>()?;
        let (mods, execution_strategy, fault_streak, last_fault, last_auto_disable) =
            match &self.mod_realm {
                Some(vm) => {
                    let vm = vm.borrow();
                    let (packages, options) = vm.checkpoint_input();
                    (
                        Some(ModIdentity::capture(packages, options)?),
                        Some(
                            vm.composition()
                                .execution_strategy_digest()
                                .map_err(anyhow::Error::msg)?,
                        ),
                        vm.fault_streak.clone(),
                        vm.last_fault.clone(),
                        vm.last_auto_disable.clone(),
                    )
                }
                None => (None, None, None, None, None),
            };
        let saved = Checkpoint {
            checkpoint_format_version: CURRENT_FORMAT_VERSION,
            engine_version: env!("CARGO_PKG_VERSION").into(),
            room_schema_version: kk_schema::room::SCHEMA_VERSION,
            engine: self.engine.checkpoint().map_err(anyhow::Error::msg)?,
            workers,
            mods,
            execution_strategy,
            disabled_mods: self.disabled_mods(),
            fault_streak,
            last_fault,
            last_auto_disable,
            concurrency: self.player_concurrency,
            kkpv: self.kkpv_channel,
            last_console: self.last_console.clone(),
            metadata,
        };
        serde_json::to_value(saved).context("编码完整检查点")
    }

    pub fn save_checkpoint(&self, path: &Path, metadata: Value) -> anyhow::Result<()> {
        crate::checkpoint::write_checkpoint(path, &self.checkpoint(metadata)?)
    }

    /// 缺包、源码/配置变化或格式未知时拒绝恢复；调用方的旧分片保持可用。
    /// Mod 包须由宿主显式装载，检查点不会自行读取路径或执行其中携带的包源码。
    pub fn load_checkpoint(
        &mut self,
        path: &Path,
        mods: Option<(&BTreeMap<String, ModPackage>, &RegisterOptions)>,
    ) -> anyhow::Result<Value> {
        self.restore_checkpoint(crate::checkpoint::read_checkpoint(path)?, mods)
    }

    pub fn restore_checkpoint(
        &mut self,
        mut value: Value,
        mods: Option<(&BTreeMap<String, ModPackage>, &RegisterOptions)>,
    ) -> anyhow::Result<Value> {
        self.no_open_round("restore_checkpoint")?;
        if self
            .mod_realm
            .as_ref()
            .is_some_and(|vm| !vm.borrow().is_checkpoint_current())
        {
            anyhow::bail!(
                "检查点切换被拒：同线程存在其他活跃 Mod 分片或玩家 VM；须按进入栈顺序恢复"
            );
        }
        // 迁移先于 deny_unknown_fields 解码（对原始 Value 路由版本）；
        // 返回的原始版本供语义门决策——v2 起保存的检查点必须携带
        // 执行策略身份，v1 存量迁移后仍按 legacy 宽容（当次恢复有效，
        // 再保存即当前格式）。
        let source_format = migrate::migrate_to_current(&mut value).map_err(anyhow::Error::msg)?;
        let saved: Checkpoint = serde_json::from_value(value).context("解码完整检查点")?;
        if source_format >= 2 {
            anyhow::ensure!(
                saved.mods.is_none() || saved.execution_strategy.is_some(),
                "检查点格式 {source_format} 起必须携带执行策略身份（execution_strategy）；尚未提供迁移补全"
            );
        }
        anyhow::ensure!(
            saved.engine_version == env!("CARGO_PKG_VERSION")
                && saved.room_schema_version == kk_schema::room::SCHEMA_VERSION,
            "检查点引擎/房间 schema 版本不匹配；尚未提供迁移"
        );
        match (&saved.mods, mods) {
            (Some(identity), Some((packages, options))) => identity.verify(packages, options)?,
            (None, None) => anyhow::ensure!(
                saved.disabled_mods.is_empty()
                    && saved.execution_strategy.is_none()
                    && saved.fault_streak.is_none()
                    && saved.last_fault.is_none()
                    && saved.last_auto_disable.is_none(),
                "原生检查点含 Mod 执行状态"
            ),
            _ => anyhow::bail!("检查点 Mod 包缺失或装配方式不匹配"),
        }
        // 引擎账户可能包含尚无玩家线程的市场/NPC 账户；保存原有工作者子集。
        anyhow::ensure!(
            saved
                .workers
                .keys()
                .all(|name| saved.engine.players.contains_key(name)),
            "检查点工作者缺少账户状态"
        );
        let mut replacement = Self::new();
        // 恢复装配期间还未载入保存的世界时间；此时不记录重建事件。
        // 成功切换前再应用原引擎的开启意愿，缓冲从空状态续跑。
        replacement.set_player_concurrency(saved.concurrency)?;
        replacement.set_kkpv_channel(saved.kkpv);
        for (name, worker) in &saved.workers {
            replacement
                .sandbox_configs_for_test
                .insert(name.clone(), worker.config()?);
        }
        let shard = json!({"players": saved.workers.keys().collect::<Vec<_>>(), "rooms": []});
        if let Some((packages, options)) = mods {
            replacement.create_shard_with_mods(&shard, packages, options.clone())?;
            for id in &saved.disabled_mods {
                replacement.disable_mod(id)?;
            }
            anyhow::ensure!(
                replacement.disabled_mods() == saved.disabled_mods,
                "检查点停用集合未满足依赖闭包"
            );
            replacement.rebuild_mod_realm()?;
            // 执行策略身份等值核对：包/配置身份、停用闭包与 PlanHash
            // 逐项通过后，此处把「组合派生一致性」也钉成机器核验。
            // v2 起缺键已在上方硬门拒绝；能到这里仍缺键/null 的只有
            // v1 存量（显式降级 legacy 语义）——plan_hash/ModIdentity/
            // 停用闭包三道门仍然逐项核验，本项是增强而非唯一门。
            if let Some(expected) = &saved.execution_strategy {
                let actual = replacement
                    .mod_realm
                    .as_ref()
                    .expect("重装配后 Mod VM 在位")
                    .borrow()
                    .composition()
                    .execution_strategy_digest()
                    .map_err(anyhow::Error::msg)?;
                anyhow::ensure!(
                    &actual == expected,
                    "检查点执行策略身份不匹配（预期 {expected}，实际 {actual}）；尚未提供迁移"
                );
            }
        } else {
            replacement.create_shard(&shard)?;
        }
        replacement
            .engine
            .restore_checkpoint(saved.engine)
            .map_err(anyhow::Error::msg)?;
        for (name, worker) in &saved.workers {
            replacement.set_player_modules(name, worker.modules())?;
        }
        if let Some(vm) = &replacement.mod_realm {
            let mut vm = vm.borrow_mut();
            if let Some(streak) = &saved.fault_streak {
                anyhow::ensure!(
                    streak.consecutive_faults > 0
                        && saved
                            .mods
                            .as_ref()
                            .is_some_and(|identity| identity.packages.contains_key(&streak.mod_id)),
                    "检查点 Mod 故障计数身份非法"
                );
            }
            vm.fault_streak = saved.fault_streak;
            vm.last_fault = saved.last_fault;
            vm.last_auto_disable = saved.last_auto_disable;
        }
        replacement.last_console = saved.last_console;
        if let (Some(new), Some(old)) = (&replacement.mod_realm, &self.mod_realm) {
            new.borrow_mut()
                .retire_previous_for_checkpoint(&mut old.borrow_mut())
                .map_err(anyhow::Error::msg)?;
        }
        if self.mod_trace_enabled {
            replacement.enable_mod_trace();
        }
        *self = replacement;
        Ok(saved.metadata)
    }
}

#[cfg(test)]
mod tests;
