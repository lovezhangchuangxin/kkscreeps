//! 常驻宿主：引擎的单一所有者、tick 调度、周期保存、暂停/重试与停止。
//!
//! 宿主不接触网络与信号——驱动方（CLI）负责控制流（stdio 命令、
//! SIGTERM）与时钟，宿主按调用推进。调度策略以纯函数 [`next_deadline`]
//! 表达：未超时保持原截止点（快速 tick 不累积漂移），已超时从当前时刻
//! 延后一个间隔（过载降低实际速率，不补拍、不突发追赶）；进程重启后
//! 无历史截止点，停机期间不补算。
use super::checkpoints::{self, KeepReport};
use super::dir::ServiceDir;
use super::identity::{self, ServiceIdentity};
use crate::EmbedEngine;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// 周期保存与保留参数（操作配置的宿主视图）。
#[derive(Clone, Copy)]
pub struct HostParameters {
    pub auto_save_ticks: u32,
    pub auto_save_wall_secs: u64,
    pub keep_checkpoints: u32,
}

/// 计算下一次 tick 截止时间。`previous_target` 是**上一拍已消费**的截止
/// 点：下一网格点 = previous + interval，仍在未来则锁定网格（拍执行耗时
/// 不累积进周期，快速拍零漂移）；已超时（拍用时超过间隔）→ now +
/// interval（过载降低实际速率，不补拍、不突发追赶）。首轮/重启无历史
/// 截止点 → now + interval，停机期间的墙钟积压不产生待补拍数。
pub fn next_deadline(
    previous_target: Option<Instant>,
    now: Instant,
    interval: Duration,
) -> Instant {
    match previous_target.and_then(|previous| previous.checked_add(interval)) {
        Some(grid) if grid > now => grid,
        // 兜底臂同样防溢出：与网格臂一致，极端时刻饱和到 now，调度器
        // 不因时钟算术 panic（实践不可达，仅保持两臂一致）。
        _ => now.checked_add(interval).unwrap_or(now),
    }
}

/// v2 目录的保存上下文：普通保存继承恢复版本的屏障身份与水位
/// （新屏障由命令完成事务建立，S2.2 起）；锚点序号供保留策略保护。
#[derive(Clone, Debug)]
pub struct V2SaveContext {
    pub barrier_id: String,
    pub inbox_applied: std::collections::BTreeMap<String, u64>,
    pub code_versions: std::collections::BTreeMap<String, u64>,
    pub outbox_cursor: u64,
    pub anchor_seq: u64,
}

/// 宿主生命周期；故障不单独成态，挂在 `fault` 上并冻结推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Running,
    /// 写入准入已关闭：不再开始新拍，等待收尾保存。
    Draining,
    Stopped,
}

/// 暂停类别（决定 retry 的处置路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseKind {
    /// 结算失败：重试只重结算，复用缓存候选。
    Settle,
    /// 玩家线程/协议故障：禁止自动重派，重试先显式重建工作者。
    Execution,
    /// 其余基础设施故障（如 Mod realm 待恢复）：无缓存轮，重试直接
    /// 重新进入拍入口（run_players 自带 realm 重建）。
    Infrastructure,
}

#[derive(Clone, Debug)]
pub enum Fault {
    /// 结算或基础设施故障：保持 tick、保留已确认结果，等待显式重试。
    Paused { kind: PauseKind, reason: String },
    /// 发布耐久性不确定：停止推进，retry 不可用，只能停止。
    Persistence { reason: String },
}

/// 单次 tick 的结果；玩家级错误不构成故障（沿现行玩家结局语义结算）。
#[derive(Debug)]
pub struct TickReport {
    pub player_errors: Vec<(String, Value)>,
}

/// 恢复摘要（run 启动时的选择结果，随状态报告）。
#[derive(Clone)]
pub struct RecoverySummary {
    pub selected: String,
    pub publish_seq: u64,
    pub tick: u64,
    pub skipped_broken: usize,
}

/// 一次保存尝试的结果。
#[derive(Debug)]
pub enum SaveOutcome {
    Saved { seq: u64, tick: u64 },
    Failed { reason: String },
}

/// 停止流程的结局；含失败轮时不擅自保存/丢弃，交由操作者处置。
#[derive(Debug)]
pub enum StopOutcome {
    /// 收尾保存成功（或无需保存）并已完成关闭。
    Completed { saved: Option<(u64, u64)> },
    /// 存在未处置失败轮：不保存、不丢弃，最后耐久状态仍在此前版本。
    BlockedByFailedRound { last_durable_tick: u64 },
    /// 收尾保存失败：不把未耐久状态当作已保存。
    SaveFailed { reason: String },
}

const RECENT_ERRORS_BOUND: usize = 20;

/// 已恢复世界的常驻宿主。持有引擎与目录的写权（调用方须持有目录租约）；
/// `Drop` 不做保存——停止语义只经 [`ServiceHost::request_stop`] /
/// [`ServiceHost::finish_stop`] 显式驱动。
pub struct ServiceHost {
    engine: EmbedEngine,
    dir: ServiceDir,
    identity: ServiceIdentity,
    codes: BTreeMap<String, String>,
    entries: BTreeMap<String, String>,
    params: HostParameters,
    phase: Phase,
    fault: Option<Fault>,
    next_seq: u64,
    last_durable_tick: u64,
    last_durable_seq: u64,
    dirty_ticks: u64,
    ticks_total: u64,
    last_save_ms: Option<f64>,
    last_error: Option<String>,
    run_epoch: String,
    recovery: RecoverySummary,
    recent_player_errors: VecDeque<Value>,
    /// 停止收尾时冻结的活 tick：teardown 重置引擎后 status 仍如实报告
    /// 停止时刻的世界时间（而不是误导性的 0）。
    frozen_live_tick: Option<u64>,
    /// v2 目录的保存上下文；None=v1 目录按 S1 语义保存。
    v2: Option<V2SaveContext>,
}

impl ServiceHost {
    /// 装配已恢复的引擎；`next_seq` 取自目录实况，此后每次发布推进。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: EmbedEngine,
        dir: ServiceDir,
        identity: ServiceIdentity,
        entries: BTreeMap<String, String>,
        params: HostParameters,
        recovery: RecoverySummary,
    ) -> anyhow::Result<Self> {
        let mut codes = BTreeMap::new();
        for (player, entry) in &entries {
            let entry = serde_json::to_string(entry)?;
            codes.insert(
                player.clone(),
                format!("(() => {{ const bot = require({entry}); if (!bot || typeof bot.loop !== 'function') throw new Error('bot entry must export loop'); bot.loop(); return true; }})()"),
            );
        }
        let last_durable_tick = engine.get_game_time();
        Ok(Self {
            next_seq: checkpoints::next_publish_seq(&dir.checkpoints_dir())?,
            last_durable_tick,
            last_durable_seq: recovery.publish_seq,
            engine,
            dir,
            identity,
            codes,
            entries,
            params,
            phase: Phase::Running,
            fault: None,
            dirty_ticks: 0,
            ticks_total: 0,
            last_save_ms: None,
            last_error: None,
            run_epoch: identity::generate_world_id()[..16].to_string(),
            recovery,
            recent_player_errors: VecDeque::new(),
            frozen_live_tick: None,
            v2: None,
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn fault(&self) -> Option<&Fault> {
        self.fault.as_ref()
    }

    pub fn engine(&mut self) -> &mut EmbedEngine {
        &mut self.engine
    }

    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.entries
    }

    /// 是否允许推进 tick：Running 且无任何故障。Draining/Persistence 下
    /// 新拍不得开始（写入准入已关）；Paused 下等待显式重试。
    pub fn can_tick(&self) -> bool {
        self.phase == Phase::Running && self.fault.is_none()
    }

    /// 执行一拍：全部注册玩家运行一次并结算。玩家级错误计入统计继续
    /// 运行；基础设施故障按类别转入 Paused 并冻结推进。
    pub fn tick(&mut self) -> anyhow::Result<Option<TickReport>> {
        anyhow::ensure!(
            self.phase == Phase::Running,
            "阶段 {:?} 拒绝开始新拍",
            self.phase
        );
        anyhow::ensure!(self.fault.is_none(), "存在未处置故障，先重试或停止");
        let runs = match self.engine.run_players(&self.codes) {
            Ok(runs) => runs,
            Err(error) => {
                let reason = format!("{error:#}");
                let kind = match self.engine.pending_round_kind() {
                    Some(crate::embed::PendingRoundKind::Settle) => PauseKind::Settle,
                    Some(crate::embed::PendingRoundKind::Execution) => PauseKind::Execution,
                    // 无缓存轮的基础设施失败（如 Mod realm 恢复门）：
                    // 世界仍处上一已提交拍，重试直接重进拍入口。
                    None => PauseKind::Infrastructure,
                };
                self.last_error = Some(reason.clone());
                self.fault = Some(Fault::Paused { kind, reason });
                return Ok(None);
            }
        };
        self.ticks_total += 1;
        self.dirty_ticks += 1;
        let mut player_errors = Vec::new();
        for (player, run) in runs {
            let ok = run.executed && run.expr["ok"] == true;
            if !ok {
                let record = json!({
                    "tick": self.engine.get_game_time() - 1,
                    "player": player,
                    "executed": run.executed,
                    "result": run.expr,
                });
                if self.recent_player_errors.len() == RECENT_ERRORS_BOUND {
                    self.recent_player_errors.pop_front();
                }
                self.recent_player_errors.push_back(record.clone());
                player_errors.push((player, run.expr));
            }
        }
        Ok(Some(TickReport { player_errors }))
    }

    /// 周期保存判定：距上次耐久保存 `secs_since_reference` 秒（基准=
    /// 上次保存或本进程启动，由驱动方供给）；任一条件到点且确有脏拍
    /// 才写盘——无状态变化不重复发布相同版本。
    pub fn should_save(&self, secs_since_reference: f64) -> bool {
        self.dirty_ticks > 0
            && (self.dirty_ticks >= u64::from(self.params.auto_save_ticks)
                || secs_since_reference >= self.params.auto_save_wall_secs as f64)
    }

    /// 装配 v2 保存上下文（run setup 在 v2 目录恢复成功后调用一次）；
    /// 普通保存自此携带继承的屏障身份与水位，保留策略保护锚点。
    pub fn set_v2_context(&mut self, context: V2SaveContext) {
        self.v2 = Some(context);
    }

    /// 立即发布当前已提交状态为新检查点并执行保留策略。序号在每次尝试
    /// 时按目录实况重新分配（不缓存：发布失败后目录环境可能变化，缓存
    /// 序号会与“紧接已发布版本”的不变式脱节）。失败按持久化故障处置：
    /// 冻结推进（retry 不可用），消息附带目标文件的实证状态——rename 后
    /// 父目录同步失败属耐久未知，调用方不能按“旧文件一定还在”重试。
    pub fn save_now(&mut self) -> SaveOutcome {
        let dir = self.dir.checkpoints_dir();
        let seq = match checkpoints::next_publish_seq(&dir) {
            Ok(seq) => seq,
            Err(error) => {
                let reason = format!("分配发布序号失败: {error:#}");
                self.last_error = Some(reason.clone());
                self.fault = Some(Fault::Persistence {
                    reason: reason.clone(),
                });
                return SaveOutcome::Failed { reason };
            }
        };
        let tick = self.engine.get_game_time();
        let started = Instant::now();
        let outcome = match &self.v2 {
            Some(context) => {
                let meta = crate::service::metadata::new_meta_v2(
                    &self.identity.world_id,
                    seq,
                    self.engine.get_game_time(),
                    self.engine.world_seed(),
                    &self.identity.scenario_sha256,
                    &self.entries,
                    &context.barrier_id,
                    &context.inbox_applied,
                    &context.code_versions,
                    context.outbox_cursor,
                );
                checkpoints::publish_with_meta(&self.engine, &dir, &meta)
            }
            None => checkpoints::publish(&self.engine, &dir, seq, &self.identity, &self.entries),
        };
        match outcome {
            Ok(()) => {
                self.last_save_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
                self.last_durable_tick = tick;
                self.last_durable_seq = seq;
                self.next_seq = seq + 1;
                self.dirty_ticks = 0;
                if let Err(error) = self.enforce_keep() {
                    // 保留失败不否定已耐久的新版本；作为非致命问题记录，
                    // 不构成持久化故障（旧文件残留不影响正确性）。
                    self.last_error = Some(format!("检查点保留策略失败（不致命）: {error:#}"));
                }
                SaveOutcome::Saved { seq, tick }
            }
            Err(error) => {
                let reason = format!(
                    "{error:#}；目标文件实证：{}",
                    self.probe_checkpoint(seq, tick)
                );
                self.last_error = Some(reason.clone());
                self.fault = Some(Fault::Persistence {
                    reason: reason.clone(),
                });
                SaveOutcome::Failed { reason }
            }
        }
    }

    fn enforce_keep(&self) -> anyhow::Result<KeepReport> {
        // v2 目录的恢复锚点恒受保留：窗口前移不得释放锚点引用。
        let protected: Vec<u64> = self
            .v2
            .as_ref()
            .map(|context| vec![context.anchor_seq])
            .unwrap_or_default();
        checkpoints::enforce_keep(
            &self.dir.checkpoints_dir(),
            self.params.keep_checkpoints,
            &protected,
        )
    }

    /// 发布失败后的目标文件实证：区分“旧版本未受影响”与“目标已是新
    /// 版本（耐久未知）”，为操作者与复核提供可判别形态。
    fn probe_checkpoint(&self, seq: u64, tick: u64) -> String {
        let path = self
            .dir
            .checkpoints_dir()
            .join(checkpoints::checkpoint_file_name(seq));
        match kk_storage::envelope::read_json(&path, crate::checkpoint::SPEC) {
            Ok(value) => {
                let meta_tick = value
                    .pointer("/metadata/service/tick")
                    .and_then(|v| v.as_u64());
                match meta_tick {
                    Some(found) if found == tick => {
                        "目标已是本次新版本（rename 已发生、目录同步未确认）——耐久未知，不能按旧版本仍在重试".into()
                    }
                    other => format!("目标存在但内容不符（tick {other:?} ≠ {tick}），视为发布失败"),
                }
            }
            Err(_) => format!("目标不可读（{}）——按旧版本未受影响处理", path.display()),
        }
    }

    /// 显式重试：只消费现有缓存（结算失败）或先重建工作者（执行故障），
    /// 都不重新运行已确认的玩家代码。基础设施类直接重进拍入口。成功后
    /// 解除暂停、恢复推进；失败时重新置故障并如实上报。
    pub fn retry(&mut self) -> anyhow::Result<Option<TickReport>> {
        anyhow::ensure!(
            self.phase == Phase::Running,
            "阶段 {:?} 拒绝重试（先完成停止流程）",
            self.phase
        );
        let kind = match &self.fault {
            Some(Fault::Paused { kind, .. }) => *kind,
            Some(Fault::Persistence { .. }) => {
                anyhow::bail!("持久化故障不可通过 retry 处置；只能停止进程（状态未损）")
            }
            None => anyhow::bail!("没有待重试的暂停故障"),
        };
        if kind == PauseKind::Execution {
            self.engine.rebuild_player_workers().map_err(|error| {
                anyhow::anyhow!("重建玩家工作者失败（故障保持，未重派）: {error:#}")
            })?;
        }
        self.fault = None;
        match self.tick() {
            Ok(report) => Ok(report),
            Err(error) => Err(error),
        }
    }

    /// 关闭写入准入：不再开始新拍；在途轮次由驱动方等其同步结束后再
    /// 调用 [`ServiceHost::finish_stop`]。
    pub fn request_stop(&mut self) {
        if self.phase == Phase::Running {
            self.phase = Phase::Draining;
        }
    }

    /// 停止收尾：无开放/失败轮时按需保存并关闭引擎；存在失败轮时不
    /// 擅自保存或丢弃（交由操作者处置），返回最后耐久 tick。
    pub fn finish_stop(&mut self) -> StopOutcome {
        // 仅首次冻结：二次收尾时引擎已 teardown 重置为全新世界（time=0），
        // 无条件覆盖会把停止时刻值冲掉。
        if self.frozen_live_tick.is_none() {
            self.frozen_live_tick = Some(self.engine.get_game_time());
        }
        if self.engine.has_pending_round() {
            self.phase = Phase::Stopped;
            return StopOutcome::BlockedByFailedRound {
                last_durable_tick: self.last_durable_tick,
            };
        }
        let saved = if self.dirty_ticks > 0 {
            match self.save_now() {
                SaveOutcome::Saved { seq, tick } => Some((seq, tick)),
                SaveOutcome::Failed { reason } => {
                    self.phase = Phase::Stopped;
                    return StopOutcome::SaveFailed { reason };
                }
            }
        } else {
            None
        };
        self.phase = Phase::Stopped;
        self.engine.teardown();
        StopOutcome::Completed { saved }
    }

    /// 状态摘要（诊断读面）：世界/运行身份、活/耐久 tick、序号、脏拍、
    /// 保存耗时、最近错误、恢复选择与跳过坏份数。
    pub fn status(&self) -> Value {
        json!({
            "world_id": self.identity.world_id,
            "run_epoch": self.run_epoch,
            "phase": format!("{:?}", self.phase),
            "fault": self.fault.as_ref().map(|fault| match fault {
                Fault::Paused { kind, reason } => json!({
                    "kind": format!("{kind:?}"), "class": "paused", "reason": reason,
                }),
                Fault::Persistence { reason } => json!({
                    "class": "persistence", "reason": reason,
                }),
            }),
            "live_tick": self.frozen_live_tick.unwrap_or_else(|| self.engine.get_game_time()),
            "durable_tick": self.last_durable_tick,
            "publish_seq": self.last_durable_seq,
            "next_publish_seq": self.next_seq,
            "dirty_ticks": self.dirty_ticks,
            "ticks_total": self.ticks_total,
            "last_save_ms": self.last_save_ms,
            "last_error": self.last_error,
            "recovery_selected": self.recovery.selected,
            "skipped_broken": self.recovery.skipped_broken,
            "players": self.codes.keys().collect::<Vec<_>>(),
            "recent_player_errors": self.recent_player_errors,
        })
    }
}

#[cfg(test)]
mod tests;
