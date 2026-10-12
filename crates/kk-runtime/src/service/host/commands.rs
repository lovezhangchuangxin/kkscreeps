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
/// Deferred 是内部态：命令已挂起待拍内处理（表达式或 Mod 输入），drain 停在
/// FIFO 前缀，终态由拍成功后的屏障事务写回。
#[derive(Clone, Debug)]
pub enum AppliedCommand {
    Completed(Value),
    Rejected(String),
    Deferred,
}

/// 已挂起的受限 Mod 输入（S2.3）：包 namespace 每拍一条，下一拍以
/// 授予玩家身份注入结算批。命令终态只报告"已提交动作"（accepted）；
/// 动作实际拒绝/产出由结算事件面分别报告，不进命令终态。
#[derive(Clone, Debug)]
pub struct PendingModInput {
    pub stream_id: String,
    pub client_seq: u64,
    pub as_player: String,
    pub target: String,
    pub intent: String,
    pub args: Value,
}

/// 已挂起的控制台表达式（S2.3）：主体每拍一条，下一拍 loop 前求值。
#[derive(Clone, Debug)]
pub struct PendingExpr {
    pub stream_id: String,
    pub client_seq: u64,
    pub source: String,
    /// 求值结果（含软异常的有界错误形态）；None=尚未求值（停跑轮），
    /// 拍成功提交后经屏障终态化。
    pub result: Option<Value>,
}

/// 玩家模块包总字节上限（方案 §4.1 拟议 2MiB；服务配置化留给 S3）。
const MODULE_BUDGET_BYTES: usize = 2 * 1024 * 1024;
/// 控制台表达式源码上限（方案 §4.1 拟议 8KiB；结果上限在 runner 层
/// 的 CONSOLE_EXPR_MAX_BYTES）。
pub const EXPR_MAX_BYTES: usize = 8 * 1024;
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
            if matches!(applied, AppliedCommand::Deferred) {
                // 拍内命令（console.expr）挂起即停：不越过 FIFO 前缀，
                // 其余命令（含同主体第二条）等本条终态化后的下一轮。
                break;
            }
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
                // Deferred 已在上游 break，不落终态写路径。
                AppliedCommand::Deferred => unreachable!("Deferred 不写终态"),
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
            // 表达式结果与世界同份持久化：补全 inbox 缺口时直接返回，
            // 不能因为客户端尚未看到 completed 再次施加耐久副作用。
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
                "console.expr" => match self.command_results.streams.get(stream_id) {
                    Some(saved) if saved.client_seq == client_seq => {
                        AppliedCommand::Completed(saved.result.clone())
                    }
                    // 旧试验版本可能已确认效果却没有结果；拒绝启动推进，
                    // 不能以结果未知为由再次施加耐久副作用。
                    _ => anyhow::bail!("已确认表达式缺耐久结果，须人工核验；不能重新执行"),
                },
                // mod.input 的完成结果是常量（已提交动作），直接重构终态——
                // 意图效果已随屏障链耐久（或已被四门丢弃），重新注入会在
                // 新拍对非幂等玩法动作双重施加。
                "mod.input" => AppliedCommand::Completed(json!({"accepted": true})),
                other => AppliedCommand::Rejected(format!(
                    "命令已在屏障确认但结果不可推导（kind {other}）；不重新执行"
                )),
            });
        }
        match entry.kind.as_str() {
            "code.replace" => {
                self.apply_code_replace(stream_id, client_seq, principal, &entry.payload)
            }
            "console.expr" => {
                Ok(self.defer_console_expr(stream_id, client_seq, principal, &entry.payload))
            }
            "mod.input" => {
                Ok(self.defer_mod_input(stream_id, client_seq, principal, &entry.payload))
            }
            other => Ok(AppliedCommand::Rejected(format!(
                "未知命令 kind {other:?}（S2.2/S2.3 可用：code.replace/console.expr/mod.input）"
            ))),
        }
    }

    /// 挂起控制台表达式（S2.3）：校验（主体注册、字符串源码、≤8KiB）
    /// 失败走明确拒绝终态；通过则挂到下一拍 loop 前求值（每主体每拍
    /// 一条——已挂起时不再挂，drain 停在前缀等当前完成）。Deferred
    /// 不是终态：不写 inbox，返回后本轮 drain 结束。
    fn defer_console_expr(
        &mut self,
        stream_id: &str,
        client_seq: u64,
        principal: &str,
        payload: &Value,
    ) -> AppliedCommand {
        if self.pending_expr.contains_key(principal) {
            return AppliedCommand::Deferred;
        }
        let reject = |reason: String| AppliedCommand::Rejected(reason);
        if !self.entries.contains_key(principal) {
            return reject(format!(
                "流主体 {principal} 不是注册玩家；console.expr 只对注册玩家生效"
            ));
        }
        let Some(source) = payload["expression"].as_str() else {
            return reject("payload 缺 expression 字符串字段".into());
        };
        if source.len() > EXPR_MAX_BYTES {
            return reject(format!(
                "表达式 {source_len} 字节超上限 {EXPR_MAX_BYTES}",
                source_len = source.len()
            ));
        }
        self.pending_expr.insert(
            principal.to_string(),
            PendingExpr {
                stream_id: stream_id.to_string(),
                client_seq,
                source: source.to_string(),
                result: None,
            },
        );
        AppliedCommand::Deferred
    }

    /// 挂起受限 Mod 输入（S2.3）：校验（流主体=包 namespace、意图键
    /// namespace 与主体一致、asPlayer 必填且在该包授予表内、目标 ID 为
    /// kk 前缀十六进制、参数为 JSON 对象）失败走明确拒绝终态；通过则挂
    /// 到下一拍注入结算批（每包每拍一条——已挂起时不再挂，drain 停在
    /// 前缀）。结算层的四门核验（计划目录/schema/接收者/归属）原样
    /// 生效；消费前先核验当前计划与归属，结算候选再次核验。
    fn defer_mod_input(
        &mut self,
        stream_id: &str,
        client_seq: u64,
        principal: &str,
        payload: &Value,
    ) -> AppliedCommand {
        if self.pending_mod_input.contains_key(principal) {
            return AppliedCommand::Deferred;
        }
        let reject = |reason: String| AppliedCommand::Rejected(reason);
        // 意图键与主体同 namespace：后台主体固定为对应包，不从 payload
        // 自报取得权限（asPlayer 须经宿主授予表）。
        let Ok(key) = kk_mod_api::Key::new(payload["intent"].as_str().unwrap_or_default()) else {
            return reject("payload 缺 intent 意图键（namespace:name）".into());
        };
        // Key::new 已保证 namespace:name 形（含 namespace），直接取。
        let namespace = key.namespace();
        if namespace != principal {
            return reject(format!(
                "意图键 namespace {namespace} 与流主体（包）{principal} 不一致"
            ));
        }
        let Some(as_player) = payload["asPlayer"].as_str() else {
            return reject("payload 缺 asPlayer（首片要求代表玩家；无授予即拒绝）".into());
        };
        let granted = self
            .mod_grants
            .get(principal)
            .is_some_and(|players| players.iter().any(|p| p == as_player));
        if !granted {
            return reject(format!(
                "包 {principal} 未被授予代表玩家 {as_player}（serve config mod_grants）"
            ));
        }
        let Some(target) = payload["targetId"].as_str() else {
            return reject("payload 缺 targetId 字符串字段".into());
        };
        if kk_world::ObjId::parse(target).is_none() {
            return reject(format!("targetId {target:?} 不是 kk 前缀十六进制对象 ID"));
        }
        if !payload["args"].is_object() {
            return reject("payload 缺 args JSON 对象（参数经冻结 schema 严格转换）".into());
        }
        if let Err(reason) = self.engine.validate_host_intent(&crate::HostIntent {
            player: as_player.to_string(),
            target: target.to_string(),
            intent: key.to_string(),
            args: payload["args"].clone(),
        }) {
            return reject(reason);
        }
        self.pending_mod_input.insert(
            principal.to_string(),
            PendingModInput {
                stream_id: stream_id.to_string(),
                client_seq,
                as_player: as_player.to_string(),
                target: target.to_string(),
                intent: key.to_string(),
                args: payload["args"].clone(),
            },
        );
        AppliedCommand::Deferred
    }

    /// 拍成功提交后终态化已求值的表达式命令：一条即时屏障覆盖同拍全部
    /// （消费水位逐流推进、codeVersion 不变），屏障耐久后写回 inbox
    /// 终态。发布失败按冻结处置——命令保持 Received，重启重新消费
    /// 确认后的窗口由结果树补全，未确认才重跑。由 tick() 在世界提交后调用。
    pub(crate) fn finish_pending_commands(&mut self) -> anyhow::Result<()> {
        // 待终态化集合：已求值的表达式（completed 携带求值结果）与全部
        // 挂起的 Mod 输入（completed 只报告"已提交动作"——动作实际
        // 拒绝/产出由结算事件面分别报告）。
        // 第五元素标记挂起域（表达式=玩家主体表 / Mod 输入=包主体表）：
        // 键空间无机制隔离（包 namespace 可与玩家名同名），按域移除防止
        // 同名时误删另一表的未求值挂起项。
        let mut done: Vec<(String, String, u64, Value, bool)> = self
            .pending_expr
            .iter()
            .filter_map(|(principal, pending)| {
                pending.result.clone().map(|result| {
                    (
                        principal.clone(),
                        pending.stream_id.clone(),
                        pending.client_seq,
                        result,
                        true,
                    )
                })
            })
            .collect();
        done.extend(self.pending_mod_input.iter().map(|(namespace, pending)| {
            (
                namespace.clone(),
                pending.stream_id.clone(),
                pending.client_seq,
                json!({"accepted": true}),
                false,
            )
        }));
        if done.is_empty() {
            return Ok(());
        }
        let mut applied = self
            .v2
            .as_ref()
            .map(|context| context.inbox_applied.clone())
            .unwrap_or_default();
        for (_, stream_id, client_seq, _, _) in &done {
            let entry = applied.entry(stream_id.clone()).or_insert(0);
            *entry = (*entry).max(*client_seq);
        }
        let versions = self
            .v2
            .as_ref()
            .map(|context| context.code_versions.clone())
            .unwrap_or_default();
        let barrier = format!("b-{}", crate::service::identity::generate_world_id());
        let state = inbox::load(&self.dir.inbox_file(), &self.identity.world_id)
            .map_err(|error| self.freeze_persistence(format!("读取命令收件箱失败: {error}")))?;
        let mut results = self.command_results.clone();
        results.streams.retain(|stream, saved| {
            state
                .streams
                .get(stream)
                .and_then(|s| s.entries.get(&saved.client_seq))
                .is_some_and(|entry| !entry.status.is_settled())
        });
        for (_, stream, seq, result, _) in &done {
            results.streams.insert(
                stream.clone(),
                crate::service::command_results::CommandResult {
                    client_seq: *seq,
                    result: result.clone(),
                },
            );
        }
        self.publish_barrier_with_results(&barrier, applied, versions, results)
            .map_err(|error| self.freeze_persistence(format!("命令屏障发布失败: {error}")))?;
        // 屏障已耐久：终态与水位写回 inbox（这之前崩溃=幂等补全的
        // 结果补全路径，见 apply_command）。
        let world_id = self.identity.world_id.clone();
        let mut state = inbox::load(&self.dir.inbox_file(), &world_id)
            .map_err(|error| self.freeze_persistence(format!("读取命令收件箱失败: {error}")))?;
        for (key, stream_id, client_seq, result, from_expr) in &done {
            // 按域移除：表达式查挂起表、Mod 输入查包表（同名键不互删）。
            if *from_expr {
                self.pending_expr.remove(key);
            } else {
                self.pending_mod_input.remove(key);
            }
            let stream = state.streams.get_mut(stream_id).expect("定位的流必在");
            stream.applied_seq = stream.applied_seq.max(*client_seq);
            stream.entries.get_mut(client_seq).expect("条目必在").status =
                inbox::EntryStatus::Completed {
                    result: result.clone(),
                };
        }
        inbox::store(&state, &self.dir.inbox_file())
            .map_err(|error| self.freeze_persistence(format!("命令终态存档失败: {error}")))?;
        Ok(())
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
        self.publish_barrier_with_results(barrier, applied, versions, self.command_results.clone())
    }

    fn publish_barrier_with_results(
        &mut self,
        barrier: &str,
        applied: BTreeMap<String, u64>,
        versions: BTreeMap<String, u64>,
        results: crate::service::command_results::CommandResults,
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
        checkpoints::publish_with_results(
            &self.engine,
            &self.dir.checkpoints_dir(),
            &meta,
            &results,
        )
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
        self.command_results = results;
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
mod tests;
