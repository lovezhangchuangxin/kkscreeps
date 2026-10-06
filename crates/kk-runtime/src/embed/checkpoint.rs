//! 冷恢复先在替代分片完成全部装配、身份核验和状态解码，再切换现行分片。
use super::*;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

mod identity;
use identity::{ModIdentity, Worker};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    engine_version: String,
    room_schema_version: u16,
    engine: kk_engine::settle::checkpoint::EngineCheckpoint,
    workers: BTreeMap<String, Worker>,
    mods: Option<ModIdentity>,
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
    /// 缓冲不进入检查点（trace 开启意愿经引擎挂起标志跨恢复保留）。
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
        let (mods, fault_streak, last_fault, last_auto_disable) = match &self.mod_realm {
            Some(vm) => {
                let vm = vm.borrow();
                let (packages, options) = vm.checkpoint_input();
                (
                    Some(ModIdentity::capture(packages, options)?),
                    vm.fault_streak.clone(),
                    vm.last_fault.clone(),
                    vm.last_auto_disable.clone(),
                )
            }
            None => (None, None, None, None),
        };
        let saved = Checkpoint {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            room_schema_version: kk_schema::room::SCHEMA_VERSION,
            engine: self.engine.checkpoint().map_err(anyhow::Error::msg)?,
            workers,
            mods,
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
        value: Value,
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
        let saved: Checkpoint = serde_json::from_value(value).context("解码完整检查点")?;
        anyhow::ensure!(
            saved.engine_version == env!("CARGO_PKG_VERSION")
                && saved.room_schema_version == kk_schema::room::SCHEMA_VERSION,
            "检查点引擎/房间 schema 版本不匹配；尚未提供迁移"
        );
        match (&saved.mods, mods) {
            (Some(identity), Some((packages, options))) => identity.verify(packages, options)?,
            (None, None) => anyhow::ensure!(
                saved.disabled_mods.is_empty()
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
        // 恢复用替代引擎整体替换 self；调用方在本引擎上请求过的
        // trace 开启意愿必须在替代引擎装配 Mod 之前转移过去（挂起标志
        // 在 create_shard_with_mods 内消费），否则续跑静默失去 trace。
        replacement.mod_trace_pending = self.mod_trace_pending;
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
        *self = replacement;
        Ok(saved.metadata)
    }
}

#[cfg(test)]
mod tests;
