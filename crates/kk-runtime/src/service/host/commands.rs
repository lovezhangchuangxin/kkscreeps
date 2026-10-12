//! 命令消费面（S2.2）：inbox FIFO 前缀消费、代码边界事务与即时屏障。
//!
//! 消费只发生在无开放轮次时（同步执行、拍间处理）；每笔命令（完成或
//! 验证拒绝）以携带新屏障身份与推进水位的检查点耐久确认，锚点随后
//! 单调前进。崩溃窗口语义：屏障发布后、inbox 存档前中断——恢复选择
//! 落在新屏障链（meta 水位已含该命令），drain 对"水位已含仍 Received"
//! 的条目按可推导结果幂等补全（code.replace 的结果=codeVersion，由
//! 当前版本号重构，不重新执行）；effects 未更新则新屏障候选被恢复
//! 下限过滤、世界回到旧链重新消费——两段确认不产生死锁。
use super::{Fault, ServiceHost};
use crate::service::{checkpoints, effects, inbox};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// 单笔命令的应用结局；对外状态机 received → completed/rejected。
#[derive(Clone, Debug)]
pub enum AppliedCommand {
    Completed(Value),
    Rejected(String),
}

/// 玩家模块包总字节上限（方案 §4.1 拟议 2MiB；服务配置化留给 S3）。
const MODULE_BUDGET_BYTES: usize = 2 * 1024 * 1024;
/// 每轮 drain 消费的命令上限（固定有限全局 FIFO 前缀，防单轮垄断）。
pub const DRAIN_BATCH: usize = 4;

impl ServiceHost {
    /// v2 命令面是否启用（serve run 据此决定是否消费 inbox）。
    pub fn commands_enabled(&self) -> bool {
        self.v2.is_some()
    }

    /// 命令面的存储/基础设施错误统一冻结推进（reason 如实记来源）：
    /// 命令消费不能与拍推进交错重试，重启按最后完整版本恢复。
    fn freeze_persistence(&mut self, reason: String) -> anyhow::Error {
        self.last_error = Some(reason.clone());
        self.fault = Some(Fault::Persistence {
            reason: reason.clone(),
        });
        anyhow::anyhow!(reason)
    }

    /// 消费 inbox 中最老的未终态命令前缀（按接收序全局 FIFO），逐笔
    /// 应用、建立即时屏障并把终态/水位存回 inbox。返回处理的命令数。
    /// 持久化故障按 S1.2 语义冻结推进并中止（已处理且存档的不回退）。
    pub fn drain_command_prefix(&mut self, max_commands: usize) -> anyhow::Result<usize> {
        anyhow::ensure!(self.phase == super::Phase::Running, "命令消费只在运行态");
        anyhow::ensure!(self.fault.is_none(), "存在未处置故障，命令消费不可用");
        if self.v2.is_none() {
            return Ok(0);
        }
        let world_id = self.identity.world_id.clone();
        let mut processed = 0usize;
        while processed < max_commands {
            let mut state = inbox::load(&self.dir.inbox_file(), &world_id)
                .map_err(|error| self.freeze_persistence(format!("读取命令收件箱失败: {error}")))?;
            // 全局最老未终态命令（接收序即 server_seq）。
            let next = state
                .streams
                .iter()
                .filter(|(_, stream)| !stream.retired)
                .flat_map(|(stream_id, stream)| {
                    stream.entries.iter().filter_map(move |(seq, entry)| {
                        (!entry.status.is_settled()).then_some((
                            entry.server_seq,
                            stream_id.clone(),
                            *seq,
                        ))
                    })
                })
                .min_by_key(|(server_seq, _, _)| *server_seq);
            let Some((_, stream_id, client_seq)) = next else {
                break;
            };
            let stream = state.streams.get(&stream_id).expect("定位的流必在");
            // 主体以流绑定为准（权限来源不信 payload 自报），复用本轮
            // 已加载状态，不再整读一次收件箱。
            let principal = stream.principal.clone();
            let entry = stream
                .entries
                .get(&client_seq)
                .expect("定位的条目必在")
                .clone();
            let applied = match self.apply_command(&stream_id, client_seq, &principal, &entry) {
                Ok(applied) => applied,
                Err(error) => {
                    // 应用阶段的 Err 必须伴随冻结（drive 契约只认 fault，
                    // 不设 fault 会形成无故障态的活锁）；已自行冻结的
                    // 原样上抛。
                    if self.fault.is_none() {
                        return Err(self.freeze_persistence(format!("命令应用失败: {error:#}")));
                    }
                    return Err(error);
                }
            };
            // 终态与已消费水位存回 inbox；这之前崩溃=命令重新消费
            // （幂等补全/重放语义覆盖）。
            let stream = state.streams.get_mut(&stream_id).expect("定位的流必在");
            stream.applied_seq = stream.applied_seq.max(client_seq);
            stream
                .entries
                .get_mut(&client_seq)
                .expect("条目必在")
                .status = match &applied {
                AppliedCommand::Completed(result) => inbox::EntryStatus::Completed {
                    result: result.clone(),
                },
                AppliedCommand::Rejected(reason) => inbox::EntryStatus::Rejected {
                    reason: reason.clone(),
                },
            };
            inbox::store(&state, &self.dir.inbox_file())
                .map_err(|error| self.freeze_persistence(format!("命令终态存档失败: {error}")))?;
            processed += 1;
        }
        Ok(processed)
    }

    /// 应用单笔命令：先看恢复水位是否已含该身份（屏障后存档前崩溃的
    /// 幂等补全），再按 kind 分派。验证拒绝同样持久化终态（防队首毒
    /// 消息阻塞）；基础设施错误（发布/存档失败）以 Err 上抛冻结推进。
    fn apply_command(
        &mut self,
        stream_id: &str,
        client_seq: u64,
        principal: &str,
        entry: &inbox::CommandEntry,
    ) -> anyhow::Result<AppliedCommand> {
        let confirmed = self
            .v2
            .as_ref()
            .and_then(|context| context.inbox_applied.get(stream_id).copied())
            .unwrap_or(0);
        if confirmed >= client_seq {
            // 屏障水位已含该命令但 inbox 仍是 Received：结果按可推导
            // 规则重构（code.replace 的完成结果=当前 codeVersion），
            // 不重新执行、不补造不可知结果。
            // S2.3 约束登记：新增会建屏障的命令 kind 时，其结果必须
            // 可由屏障 meta 重构（或随 meta 持久化），否则此处的
            // "不可推导"拒绝会把崩溃前已对外确认的 completed 改写。
            return Ok(match entry.kind.as_str() {
                "code.replace" => {
                    let version = self
                        .v2
                        .as_ref()
                        .and_then(|context| context.code_versions.get(principal))
                        .copied()
                        .unwrap_or(0);
                    AppliedCommand::Completed(json!({"codeVersion": version}))
                }
                other => AppliedCommand::Rejected(format!(
                    "命令已在屏障确认但结果不可推导（kind {other}）；不重新执行"
                )),
            });
        }
        match entry.kind.as_str() {
            "code.replace" => {
                self.apply_code_replace(stream_id, client_seq, principal, &entry.payload)
            }
            other => Ok(AppliedCommand::Rejected(format!(
                "未知命令 kind {other:?}（S2.2 可用：code.replace）"
            ))),
        }
    }

    /// 代码边界事务：校验与 CAS（失败=验证拒绝，保旧版现役不动）→
    /// 切换工作者模块表（Memory/segments/CPU 不动）→ 即时屏障检查点
    /// （新屏障身份、codeVersion+1、消费水位）耐久发布 → 锚点推进。
    /// 不推进世界时间；模块顶层仍在下一正常玩家执行阶段运行。
    fn apply_code_replace(
        &mut self,
        stream_id: &str,
        client_seq: u64,
        principal: &str,
        payload: &Value,
    ) -> anyhow::Result<AppliedCommand> {
        // 载荷与授权校验一律验证拒绝（终态化防队首毒消息）；基础设施
        // 故障才走 Err 冻结。拒因载荷缺失/畸形也在拒绝之列——走 Err
        // 会让驱动落入无故障态的活锁（历史上确为此修过）。
        let reject = |reason: String| Ok(AppliedCommand::Rejected(reason));
        if !self.entries.contains_key(principal) {
            return reject(format!(
                "流主体 {principal} 不是注册玩家；code.replace 只对注册玩家生效"
            ));
        }
        let Some(expected) = payload["expectedCodeVersion"].as_u64() else {
            return reject("payload 缺 expectedCodeVersion 数值字段（u64）".into());
        };
        let current = self
            .v2
            .as_ref()
            .and_then(|context| context.code_versions.get(principal))
            .copied()
            .unwrap_or(0);
        if expected != current {
            return Ok(AppliedCommand::Rejected(format!(
                "codeVersion CAS 失败：期望 {expected} 实际 {current}（过期版本不静默覆盖）"
            )));
        }
        // 模块表校验：非空、模块名/源码均为字符串、总字节预算、入口
        // 存在且为 JS。失败一律验证拒绝（保旧版）。
        let modules = match payload["modules"].as_object() {
            Some(modules) if !modules.is_empty() => modules,
            _ => return reject("payload 缺非空 modules 对象".into()),
        };
        let mut table = kk_runner::ModuleTable::new();
        let mut total = 0usize;
        for (name, code) in modules {
            let Some(source) = code.as_str() else {
                return reject(format!("模块 {name} 的源码必须是字符串"));
            };
            total = total.saturating_add(source.len());
            if total > MODULE_BUDGET_BYTES {
                return reject(format!("模块包总字节 {total} 超上限 {MODULE_BUDGET_BYTES}"));
            }
            table.insert_js(name, source);
        }
        let entry_name = match payload["entry"].as_str() {
            Some(name) => name.to_string(),
            None => match self.entries.get(principal).cloned() {
                Some(name) => name,
                None => return reject(format!("玩家 {principal} 无既有入口")),
            },
        };
        if table.kind(&entry_name) != Some(kk_runner::ModuleKind::Js) {
            return reject(format!("入口模块 {entry_name} 缺失或非 JS"));
        }

        // 校验全部通过后切换现役模块表（Memory/CPU 不动）；切换或发布
        // 失败按冻结处置（重启从最后完整版本恢复，版本明确）。
        self.engine
            .set_player_modules(principal, table)
            .map_err(|error| self.freeze_persistence(format!("切换工作者模块失败: {error:#}")))?;
        self.entries
            .insert(principal.to_string(), entry_name.clone());
        // 宿主拍的执行代码由入口派生：入口变更须同步，否则替换后宿主
        // 仍 require 旧入口（模块表已换会直接报错）。
        self.codes
            .insert(principal.to_string(), Self::derive_player_code(&entry_name));
        let mut versions = self
            .v2
            .as_ref()
            .map(|context| context.code_versions.clone())
            .unwrap_or_default();
        let new_version = expected + 1;
        versions.insert(principal.to_string(), new_version);
        let mut applied = self
            .v2
            .as_ref()
            .map(|context| context.inbox_applied.clone())
            .unwrap_or_default();
        applied.insert(stream_id.to_string(), client_seq);

        let barrier = format!("b-{}", crate::service::identity::generate_world_id());
        self.publish_barrier(&barrier, applied, versions)
            .map_err(|error| {
                // 屏障或锚点写失败：冻结（内存引擎已持新模块表但 v2 上下
                // 文停在旧版本）。此时停止的收尾保存可能发布"引擎新表 +
                // meta 旧版本"的分歧检查点——恢复下限按同屏障同水位放
                // 行，重启后 CAS 基准随 meta 回滚、drain 先于任何 tick
                // 重新消费收敛（已实证）；若收尾保存也未发生，世界回
                // 旧链重新消费，同样收敛。
                self.freeze_persistence(format!("代码替换屏障发布失败: {error}"))
            })?;
        // 结果形状与幂等补全的重构体保持一致（{"codeVersion":N}）；
        // "模块顶层在下一玩家执行阶段运行"属契约文档，不入结果体。
        Ok(AppliedCommand::Completed(
            json!({"codeVersion": new_version}),
        ))
    }

    /// 发布即时屏障：携带新屏障身份、命令消费水位与 codeVersion 的
    /// 完整检查点耐久发布，随后推进 effects 锚点（单调）并更新保存
    /// 上下文。effects 写失败时新屏障未确认——返回 Err 由调用方冻结。
    fn publish_barrier(
        &mut self,
        barrier: &str,
        applied: BTreeMap<String, u64>,
        versions: BTreeMap<String, u64>,
    ) -> anyhow::Result<u64> {
        let context = self.v2.as_ref().expect("屏障发布只用于 v2 目录").clone();
        let seq = checkpoints::next_publish_seq(&self.dir.checkpoints_dir())?;
        let meta = crate::service::metadata::new_meta_v2(
            &self.identity.world_id,
            seq,
            self.engine.get_game_time(),
            self.engine.world_seed(),
            &self.identity.scenario_sha256,
            &self.entries,
            barrier,
            &applied,
            &versions,
            context.outbox_cursor,
        );
        checkpoints::publish_with_meta(&self.engine, &self.dir.checkpoints_dir(), &meta)
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        // 锚点推进：沿用既有 sinks 水位（S2.4 起由投递确认维护）。
        // effects 损坏/身份不符是停服条件——不得按空 sinks 重建覆写
        // （那会静默清零 S2.4 的投递确认水位）。
        let mut anchor = effects::EffectsState::initial(&self.identity.world_id, seq, barrier);
        anchor.anchor.inbox_applied = applied.clone();
        anchor.sinks = effects::load(&self.dir.effects_file(), &self.identity.world_id)
            .map_err(|error| anyhow::anyhow!("屏障锚点推进前读取 effects 失败: {error}"))?
            .sinks;
        effects::write(&self.dir.effects_file(), &anchor)
            .map_err(|error| anyhow::anyhow!("恢复锚点更新失败: {error}"))?;
        // 保存上下文与耐久游标随屏障推进；保留策略保护新锚点。
        self.last_durable_tick = self.engine.get_game_time();
        self.last_durable_seq = seq;
        self.next_seq = seq + 1;
        self.dirty_ticks = 0;
        if let Some(context) = self.v2.as_mut() {
            context.barrier_id = barrier.to_string();
            context.inbox_applied = applied;
            context.code_versions = versions;
            context.anchor_seq = seq;
        }
        if let Err(error) = self.enforce_keep() {
            // 屏障后的保留清理失败不否定已确认命令；下轮保存重试。
            self.last_error = Some(format!("屏障后保留清理失败（不阻塞）: {error}"));
        }
        Ok(seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EmbedEngine;
    use crate::service::host::{HostParameters, RecoverySummary, ServiceHost, V2SaveContext};
    use crate::service::{ServiceDir, identity, inbox};
    use identity::ServiceIdentity;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kk-cmds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// v2 宿主：单玩家引擎（Memory.n 计数）+ 初始屏障 + 空 inbox/effects。
    fn host_v2(tag: &str, code: &str) -> (ServiceHost, ServiceIdentity) {
        let root = temp_dir(tag);
        let ident = identity::ServiceIdentity {
            service_format_version: identity::SERVICE_FORMAT_V2,
            world_id: identity::generate_world_id(),
            world_seed: 5,
            scenario_sha256: "e".repeat(64),
            initialized: true,
        };
        let mut engine = EmbedEngine::with_world_seed(ident.world_seed);
        engine
            .create_shard(&json!({
                "players": ["p1"],
                "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}],
            }))
            .unwrap();
        let mut table = kk_runner::ModuleTable::new();
        table.insert_js("main", code);
        engine.set_player_modules("p1", table).unwrap();
        let dir = ServiceDir::new(&root);
        dir.ensure_layout().unwrap();
        let mut host = ServiceHost::new(
            engine,
            dir,
            ident.clone(),
            BTreeMap::from([("p1".to_string(), "main".to_string())]),
            HostParameters {
                auto_save_ticks: u64::MAX as u32,
                auto_save_wall_secs: u64::MAX,
                keep_checkpoints: 5,
            },
            RecoverySummary {
                selected: String::new(),
                publish_seq: 0,
                tick: 0,
                skipped_broken: 0,
            },
        )
        .unwrap();
        host.set_v2_context(V2SaveContext {
            barrier_id: "b-init".into(),
            inbox_applied: BTreeMap::new(),
            code_versions: BTreeMap::new(),
            outbox_cursor: 0,
            anchor_seq: 0,
        });
        // 初始 effects 先行：不变量是 effects 在首次 publish_barrier 前
        // 存在（它读该文件沿用 sinks，缺失/损坏是停服条件）——enable
        // 事务在启用时写 effects，此后任何屏障发布都满足此前置。
        effects::write(
            &host.dir.effects_file(),
            &effects::EffectsState::initial(&ident.world_id, 0, "b-init"),
        )
        .unwrap();
        host.publish_barrier("b-init", BTreeMap::new(), BTreeMap::new())
            .unwrap();
        inbox::store(
            &inbox::InboxState::empty(&ident.world_id),
            &host.dir.inbox_file(),
        )
        .unwrap();
        (host, ident)
    }

    /// 注入一条 code.replace 命令（stream s1 主体 p1）。
    fn inject_replace(host: &ServiceHost, ident: &identity::ServiceIdentity, payload: Value) {
        let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id).unwrap();
        if !state.streams.contains_key("s1") {
            inbox::accept::open_stream(&mut state, "p1", "s1").unwrap();
        }
        let envelope = json!({
            "worldId": ident.world_id,
            "principal": "p1",
            "streamId": "s1",
            "clientSeq": state.streams["s1"].next_client_seq,
            "kind": "code.replace",
            "schemaVersion": 1,
            "payload": payload,
        });
        match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
            inbox::accept::AcceptOutcome::Received { .. } => {}
            other => panic!("注入应接收: {other:?}"),
        }
        inbox::store(&state, &host.dir.inbox_file()).unwrap();
    }

    fn entry_status(
        host: &ServiceHost,
        ident: &identity::ServiceIdentity,
        seq: u64,
    ) -> inbox::EntryStatus {
        inbox::load(&host.dir.inbox_file(), &ident.world_id)
            .unwrap()
            .streams["s1"]
            .entries[&seq]
            .status
            .clone()
    }

    /// 走宿主拍执行玩家（require 入口+loop），与 serve run 驱动同路径。
    fn run_one_player(host: &mut ServiceHost) {
        host.tick().expect("拍应成功").expect("无故障应推进");
    }

    /// 主链路：替换消费即屏障——codeVersion 推进、新模块生效、Memory
    /// 保留、inbox 终态与水位存档、effects 锚点换新屏障、后续保存继承。
    #[test]
    fn code_replace_consumes_command_with_immediate_barrier() -> anyhow::Result<()> {
        let code = "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };";
        let (mut host, ident) = host_v2("main-link", code);
        // 先跑两拍积累 Memory，证明替换不重置。
        run_one_player(&mut host);
        run_one_player(&mut host);
        let tick_before = host.engine.get_game_time();
        let new_code = "exports.loop = () => { Memory.m = (Memory.n || 0) + 100; };";
        inject_replace(
            &host,
            &ident,
            json!({"modules": {"main": new_code}, "expectedCodeVersion": 0}),
        );

        let processed = host.drain_command_prefix(4)?;
        assert_eq!(processed, 1);
        assert_eq!(host.engine.get_game_time(), tick_before, "替换不推进世界");
        match entry_status(&host, &ident, 1) {
            inbox::EntryStatus::Completed { result } => {
                assert_eq!(result["codeVersion"], json!(1))
            }
            other => panic!("应完成：{other:?}"),
        }
        // 新模块生效且 Memory.n 延续（m = n+100 而非 100）。
        run_one_player(&mut host);
        let raw = host.engine.player_state("p1").unwrap().raw_memory.clone();
        let memory: Value = serde_json::from_str(&raw)?;
        assert_eq!(memory["n"], json!(2), "旧 Memory 不被初始化覆盖");
        assert_eq!(memory["m"], json!(102), "新代码在正常执行阶段运行");
        // 屏障与水位：effects 锚点指向新屏障；后续普通保存继承。
        let effects = effects::load(&host.dir.effects_file(), &ident.world_id)?;
        assert_ne!(effects.anchor.barrier_id, "b-init");
        assert_eq!(effects.anchor.inbox_applied.get("s1"), Some(&1));
        run_one_player(&mut host);
        host.save_now();
        let selected = checkpoints::select_latest(
            &host.dir.checkpoints_dir(),
            &ident,
            Some(&checkpoints::RecoveryFloor {
                checkpoint_seq: effects.anchor.checkpoint_seq,
                barrier_id: effects.anchor.barrier_id.clone(),
                inbox_applied: effects.anchor.inbox_applied.clone(),
            }),
        )?;
        assert_eq!(selected.meta.code_versions.unwrap()["p1"], 1);
        assert_eq!(selected.publish_seq, effects.anchor.checkpoint_seq + 1);
        Ok(())
    }

    /// CAS 过期：验证拒绝持久化终态并推进水位，不阻塞后续命令。
    #[test]
    fn stale_code_version_is_rejected_without_blocking() -> anyhow::Result<()> {
        let (mut host, ident) = host_v2("cas", "exports.loop = () => {};");
        inject_replace(
            &host,
            &ident,
            json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 7}),
        );
        let processed = host.drain_command_prefix(4)?;
        assert_eq!(processed, 1);
        match entry_status(&host, &ident, 1) {
            inbox::EntryStatus::Rejected { reason } => {
                assert!(reason.contains("CAS"), "{reason}")
            }
            other => panic!("应拒绝：{other:?}"),
        }
        // 毒消息不阻塞：正确版本的第二条命令照常完成。
        inject_replace(
            &host,
            &ident,
            json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0}),
        );
        let processed = host.drain_command_prefix(4)?;
        assert_eq!(processed, 1);
        match entry_status(&host, &ident, 2) {
            inbox::EntryStatus::Completed { result } => assert_eq!(result["codeVersion"], json!(1)),
            other => panic!("第二条应完成：{other:?}"),
        }
        Ok(())
    }

    /// 非法载荷：验证拒绝且现役模块不动（旧代码继续跑）。含畸形
    /// expectedCodeVersion（缺失/字符串/负数）——载荷校验走 Rejected
    /// 终态而非 Err，队首毒消息被消费不形成活锁（三路 review 一致抓出
    /// 的 P1 回归锁定）。
    #[test]
    fn invalid_payload_keeps_current_modules() -> anyhow::Result<()> {
        let code = "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };";
        let (mut host, ident) = host_v2("invalid", code);
        for payload in [
            json!({"modules": {}, "expectedCodeVersion": 0}),
            json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0, "entry": "missing"}),
            json!({"expectedCodeVersion": 0}),
            json!({"modules": {"main": "exports.loop = () => {};"}}),
            json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": "0"}),
            json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": -1}),
        ] {
            inject_replace(&host, &ident, payload.clone());
            assert_eq!(
                host.drain_command_prefix(4)?,
                1,
                "每条都应被消费为终态（载荷 {payload}）"
            );
            assert!(
                host.fault().is_none(),
                "载荷校验拒绝不得设置故障态（活锁回归）"
            );
        }
        // 旧模块仍生效。
        run_one_player(&mut host);
        let raw = host.engine.player_state("p1").unwrap().raw_memory.clone();
        assert_eq!(serde_json::from_str::<Value>(&raw)?["n"], json!(1));
        Ok(())
    }

    /// 流绑定主体不是注册玩家：明确验证拒绝（授权类问题不升级为整服
    /// 持久化冻结），毒消息不阻塞。
    #[test]
    fn unknown_principal_is_rejected_without_freezing() -> anyhow::Result<()> {
        let (mut host, ident) = host_v2("ghost", "exports.loop = () => {};");
        let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
        inbox::accept::open_stream(&mut state, "ghost", "s1").unwrap();
        let envelope = json!({
            "worldId": ident.world_id,
            "principal": "ghost",
            "streamId": "s1",
            "clientSeq": 1,
            "kind": "code.replace",
            "schemaVersion": 1,
            "payload": {"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0},
        });
        match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
            inbox::accept::AcceptOutcome::Received { .. } => {}
            other => panic!("注入应接收: {other:?}"),
        }
        inbox::store(&state, &host.dir.inbox_file())?;
        assert_eq!(host.drain_command_prefix(4)?, 1);
        match entry_status(&host, &ident, 1) {
            inbox::EntryStatus::Rejected { reason } => {
                assert!(reason.contains("不是注册玩家"), "{reason}")
            }
            other => panic!("应拒绝：{other:?}"),
        }
        assert!(host.fault().is_none(), "授权类拒绝不得冻结");
        Ok(())
    }

    /// 屏障确认后、inbox 存档前崩溃：恢复水位已含该身份时 drain 幂等
    /// 补全（结果由当前版本号重构），不重新执行玩家模块切换。
    #[test]
    fn confirmed_barrier_before_archive_recovers_idempotently() -> anyhow::Result<()> {
        let (mut host, ident) = host_v2("idem", "exports.loop = () => {};");
        inject_replace(
            &host,
            &ident,
            json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0}),
        );
        assert_eq!(host.drain_command_prefix(4)?, 1);
        // 模拟存档前崩溃：把 inbox 的终态回退为 Received、水位归零
        // （屏障检查点与 effects 已含水位=1）。
        let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
        state.streams.get_mut("s1").unwrap().applied_seq = 0;
        state
            .streams
            .get_mut("s1")
            .unwrap()
            .entries
            .get_mut(&1)
            .unwrap()
            .status = inbox::EntryStatus::Received;
        inbox::store(&state, &host.dir.inbox_file())?;

        // 用同一宿主实例模拟恢复后的宿主：drain 已把 v2 上下文更新为
        // 屏障值（水位=1），与真实恢复装配（run 启动自检查点 meta 重建
        // V2SaveContext）数据同源等价；端到端恢复链路由集成用例覆盖。
        // 幂等补全 Completed，不重新走 CAS/切换（expected 0 ≠ 当前 1，
        // 若误重执行将 CAS 拒绝——补全路径不触发）。
        assert_eq!(host.drain_command_prefix(4)?, 1);
        match entry_status(&host, &ident, 1) {
            inbox::EntryStatus::Completed { result } => {
                assert_eq!(result["codeVersion"], json!(1), "结果由当前版本重构")
            }
            other => panic!("应幂等补全：{other:?}"),
        }
        Ok(())
    }

    /// 屏障发布失败：持久化故障冻结推进（保留内存版本、不发布完成）。
    #[test]
    fn publish_failure_freezes_as_persistence_fault() -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let (mut host, ident) = host_v2("freeze", "exports.loop = () => {};");
            struct Restore(PathBuf);
            impl Drop for Restore {
                fn drop(&mut self) {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
                }
            }
            let checkpoints = host.dir.checkpoints_dir();
            let _restore = Restore(checkpoints.clone());
            std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o500))?;
            inject_replace(
                &host,
                &ident,
                json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0}),
            );
            assert!(host.drain_command_prefix(4).is_err(), "发布失败应上抛");
            assert!(matches!(
                host.fault(),
                Some(crate::service::Fault::Persistence { .. })
            ));
            assert!(matches!(
                entry_status(&host, &ident, 1),
                inbox::EntryStatus::Received
            ));
        }
        Ok(())
    }
}
