//! Mod VM 的分片生命周期：失败即销毁，按固定启动快照重建后才恢复执行。

use std::collections::{BTreeMap, BTreeSet};

use crate::mod_trace::{ModTraceEntry, TraceLog};
use kk_mod_api::{Key, PlanHash};
use kk_mod_host::{ModFaultKind, ModHostError, ModPackage, ModRealm, RegisterOptions, Registered};

/// 最近一次实际结算的 Mod 调用故障；不随 VM 销毁或重建丢失。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModSettlementFault {
    pub tick: u64,
    pub room: String,
    pub target: String,
    pub error: ModHostError,
}

/// 连续实际结算失败的同一 Mod 来源；成功或不可归责失败会中断序列。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModFaultStreak {
    pub mod_id: String,
    pub consecutive_faults: u32,
}

/// 最近一次自动停用决定；后续成功、重建或普通故障不清除该记录。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModAutoDisable {
    /// 停用来源（触发故障绑定的命名空间）；消费方不必从触发故障的
    /// 绑定重复归责。
    pub mod_id: String,
    pub trigger: ModSettlementFault,
    pub consecutive_faults: u32,
    pub disabled: BTreeSet<String>,
}

pub(crate) struct ModVm {
    realm: Option<ModRealm>,
    packages: BTreeMap<String, ModPackage>,
    options: RegisterOptions,
    plan_hash: PlanHash,
    executables: BTreeSet<Key>,
    disabled: BTreeSet<String>,
    composition: kk_mod_host::CompositionReport,
    fail_next_rebuild: bool,
    /// 可选运行 trace；None 即关闭（记录点先经 trace_enabled 判空，
    /// 关闭时无条目构造与缓冲）。
    pub(crate) trace: Option<TraceLog>,
    pub(crate) last_fault: Option<ModSettlementFault>,
    pub(crate) fault_streak: Option<ModFaultStreak>,
    pub(crate) last_auto_disable: Option<ModAutoDisable>,
}

impl ModVm {
    pub(crate) fn new(
        realm: ModRealm,
        packages: BTreeMap<String, ModPackage>,
        options: RegisterOptions,
        registered: &Registered,
    ) -> Self {
        Self {
            realm: Some(realm),
            packages,
            options,
            plan_hash: registered.plan.hash(),
            executables: registered.executables.clone(),
            disabled: BTreeSet::new(),
            composition: registered.composition.clone(),
            fail_next_rebuild: false,
            trace: None,
            last_fault: None,
            fault_streak: None,
            last_auto_disable: None,
        }
    }

    pub(crate) fn retire_previous_for_checkpoint(
        &mut self,
        previous: &mut Self,
    ) -> Result<(), String> {
        if let Some(old) = &previous.realm
            && !self
                .realm
                .as_ref()
                .expect("替代 VM 已恢复")
                .can_retire_previous_realm(old)
        {
            return Err(
                "检查点切换被拒：同线程存在其他活跃 Mod 分片或玩家 VM；须按进入栈顺序恢复".into(),
            );
        }
        if let Some(old) = previous.realm.take() {
            // SAFETY: restore_checkpoint 在同线程构造本替代 VM，所有 canary
            // 和临时 scope 已退出；旧 VM 未执行且紧邻替代 VM 下方。此前
            // 所有失败路径先销毁替代 VM；只有无剩余校验的切换点到达这里。
            unsafe {
                self.realm
                    .as_mut()
                    .expect("替代 VM 已恢复")
                    .retire_previous_realm(old);
            }
        }
        Ok(())
    }

    pub(crate) fn is_checkpoint_current(&self) -> bool {
        self.realm.as_ref().is_some_and(ModRealm::is_current_realm)
    }

    pub(crate) fn checkpoint_input(&self) -> (&BTreeMap<String, ModPackage>, &RegisterOptions) {
        (&self.packages, &self.options)
    }

    /// 开启运行 trace（幂等；只影响开启之后的记录点）。
    pub(crate) fn enable_trace(&mut self) {
        if self.trace.is_none() {
            self.trace = Some(TraceLog::default());
        }
    }

    /// 记录点门控：关闭时调用方不构造条目（无分配、无格式化）。
    pub(crate) fn trace_enabled(&self) -> bool {
        self.trace.is_some()
    }

    /// 槽是否属停用命名空间（realm 对这类槽直接丢弃动作）。
    pub(crate) fn slot_inactive(&self, slot: &str) -> bool {
        self.realm
            .as_ref()
            .is_some_and(|realm| realm.is_inactive_slot(slot))
    }

    /// 记录一条 trace 事件；调用方须先经 trace_enabled() 门控。
    pub(crate) fn record_trace(&mut self, entry: ModTraceEntry) {
        if let Some(log) = &mut self.trace {
            log.push(entry);
        }
    }

    /// 当前快照与累计丢弃数；未开启返回 None。
    pub(crate) fn trace_snapshot(&self) -> Option<(Vec<ModTraceEntry>, usize)> {
        self.trace.as_ref().map(TraceLog::snapshot)
    }

    pub(crate) fn composition(&self) -> kk_mod_host::CompositionReport {
        self.composition.clone()
    }

    pub(crate) fn disabled(&self) -> &BTreeSet<String> {
        &self.disabled
    }

    pub(crate) fn disable(&mut self, id: &str) -> Result<BTreeSet<String>, String> {
        let composition = self
            .composition
            .with_disabled(&BTreeSet::from([id.into()]))?;
        let disabled = composition.disabled_mods.clone();
        let added = disabled.difference(&self.disabled).cloned().collect();
        if disabled != self.disabled {
            self.disabled = disabled;
            self.composition = composition;
            // 在新掩码下重建后才能重试；不改变玩家缓存与失败诊断。
            self.invalidate();
        }
        Ok(added)
    }

    /// 只在引擎实际结算返回且预算结束后调用；失败候选尚未发布。
    /// 不能在 JS/候选 op 内销毁 realm，也不能因重建失败重复计数旧诊断。
    pub(crate) fn finish_settlement(&mut self, succeeded: bool) -> Option<ModAutoDisable> {
        let accountable = if succeeded {
            None
        } else {
            self.last_fault.as_ref().and_then(|record| {
                let fault = record.error.fault.as_ref()?;
                // 来源建立在可信 Mod 遵守 SDK 的约定上，并非不可伪造的安全身份。
                // 共享资源、候选读取与 native 基础设施故障不自动归责。
                if !matches!(
                    fault.kind,
                    ModFaultKind::Handler
                        | ModFaultKind::Control
                        | ModFaultKind::Context
                        | ModFaultKind::WriteConversion
                        | ModFaultKind::Write
                ) {
                    return None;
                }
                let binding = fault.binding.as_ref()?;
                let id = binding.namespace();
                (self.executables.contains(binding)
                    && self.packages.contains_key(id)
                    && !self.disabled.contains(id))
                .then(|| id.to_string())
            })
        };
        let Some(id) = accountable else {
            self.fault_streak = None;
            return None;
        };
        let count = self
            .fault_streak
            .as_ref()
            .filter(|streak| streak.mod_id == id)
            .map_or(1, |streak| streak.consecutive_faults.saturating_add(1));
        self.fault_streak = Some(ModFaultStreak {
            mod_id: id.clone(),
            consecutive_faults: count,
        });
        let threshold = self.options.fault_disable_threshold?;
        if count < threshold.get() {
            return None;
        }
        // 已核对包身份；依赖闭包与显式停用复用同一机制。下一次重试才执行新链。
        let disabled = self.disable(&id).expect("已核对来源 Mod 存在于固定包快照");
        let decision = ModAutoDisable {
            mod_id: id,
            trigger: self.last_fault.clone().expect("仅已留存故障可以归责"),
            consecutive_faults: count,
            disabled,
        };
        self.last_auto_disable = Some(decision.clone());
        Some(decision)
    }

    pub(crate) fn needs_recovery(&self) -> bool {
        self.realm.is_none()
    }

    pub(crate) fn invalidate(&mut self) {
        // 不保留污染 realm；引擎持有的是本容器，不能通过另一份 Rc 继续调用旧 VM。
        self.realm = None;
    }

    pub(crate) fn realm_mut(&mut self) -> Result<&mut ModRealm, String> {
        self.realm.as_mut().ok_or_else(|| "Mod VM 待恢复".into())
    }

    pub(crate) fn recover(&mut self) -> Result<(), String> {
        if !self.needs_recovery() {
            return Ok(());
        }
        let mut replacement =
            ModRealm::with_options(&self.options).map_err(|e| format!("Mod VM 重建：{e}"))?;
        let registered = replacement
            .register_with_disabled(&self.packages, self.options.clone(), &self.disabled)
            .map_err(|e| format!("Mod VM 重新注册：{e}"))?;
        if registered.plan.hash() != self.plan_hash || registered.executables != self.executables {
            return Err(format!(
                "Mod VM 恢复计划不匹配（预期 {}，实际 {}）；分片继续停顿",
                self.plan_hash.hex(),
                registered.plan.hash().hex()
            ));
        }
        if std::mem::take(&mut self.fail_next_rebuild) {
            return Err("故障注入：Mod VM 重新注册后恢复失败".into());
        }
        // 全部注册与身份校验成功后才发布替代 VM；失败时保持空槽，允许再次恢复。
        // 组合同步为新 realm 的产物：正常恢复与原值逐项一致（身份已核验），
        // 避免诊断面滞留重建前的旧副本。
        self.composition = registered.composition;
        self.realm = Some(replacement);
        Ok(())
    }

    pub(crate) fn fail_next_rebuild_for_test(&mut self) {
        self.fail_next_rebuild = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatched_plan_never_installs_replacement() -> Result<(), String> {
        let packages = BTreeMap::new();
        let options = RegisterOptions::default();
        let mut realm = ModRealm::new().map_err(|e| e.to_string())?;
        let registered = realm
            .register(&packages, options.clone())
            .map_err(|e| e.to_string())?;
        let mut vm = ModVm::new(realm, packages, options, &registered);
        vm.invalidate();
        // 冻结选项属于计划指纹；模拟启动身份与原计划不一致，走真实注册与核验路径。
        vm.options.freeze.max_schema_depth += 1;
        assert!(vm.recover().unwrap_err().contains("计划不匹配"));
        assert!(vm.needs_recovery());
        assert!(vm.realm_mut().is_err());
        vm.options.freeze.max_schema_depth -= 1;
        vm.recover()?;
        assert!(!vm.needs_recovery());
        vm.invalidate();
        // 模拟重新装载/注册阶段故障；既不发布部分 VM，也不解除恢复门。
        vm.packages.insert(
            "broken".into(),
            ModPackage {
                manifest: serde_json::from_value(
                    serde_json::json!({"id":"broken", "version":"1.0.0", "apiVersion":"^1.0.0"}),
                )
                .map_err(|e| e.to_string())?,
                root: std::env::temp_dir().join("kk-mod-recovery-broken"),
                files: BTreeMap::from([(
                    "server.js".into(),
                    b"throw Error('register failed')".to_vec(),
                )]),
            },
        );
        assert!(vm.recover().unwrap_err().contains("重新注册"));
        assert!(vm.needs_recovery());
        vm.packages.clear();
        vm.recover()?;
        Ok(())
    }

    #[test]
    fn selection_drift_is_rejected_by_recovery_identity() -> Result<(), String> {
        // 两个可注册包；启动时只选择 first。恢复必须用分片私有的同一
        // 选择快照：漂移的选择会改变实际安装内容，被 PlanHash 核验拒绝。
        // 同一线程同一时刻只保留一个 realm（V8 isolate 进入栈纪律）。
        let make = |id: &str| -> Result<ModPackage, String> {
            Ok(ModPackage {
                manifest: serde_json::from_value(
                    serde_json::json!({"id":id,"version":"1.0.0","apiVersion":"^1.0.0"}),
                )
                .map_err(|e| e.to_string())?,
                root: std::env::temp_dir().join(format!("kk-mod-vm-selection-{id}")),
                files: BTreeMap::from([(
                    "server.js".into(),
                    b"export default function register() {}".to_vec(),
                )]),
            })
        };
        let packages = BTreeMap::from([
            ("first".to_string(), make("first")?),
            ("second".to_string(), make("second")?),
        ]);
        let options = RegisterOptions {
            selection: kk_mod_host::ContentSelection {
                include: BTreeSet::from(["first".to_string()]),
                exclude: BTreeSet::new(),
            },
            ..Default::default()
        };
        let mut realm = ModRealm::with_options(&options).map_err(|e| e.to_string())?;
        let registered = realm
            .register(&packages, options.clone())
            .map_err(|e| e.to_string())?;
        let selected_only = registered.plan.hash();
        drop(realm);
        // 全量注册（不做选择）是不同计划：证明选择确实改变实际内容身份。
        let hash_all = {
            let mut all = ModRealm::new().map_err(|e| e.to_string())?;
            let all_registered = all
                .register(&packages, RegisterOptions::default())
                .map_err(|e| e.to_string())?;
            all_registered.plan.hash()
        };
        assert_ne!(hash_all, selected_only);
        let mut realm = ModRealm::with_options(&options).map_err(|e| e.to_string())?;
        let registered = realm
            .register(&packages, options.clone())
            .map_err(|e| e.to_string())?;
        let mut vm = ModVm::new(realm, packages, options, &registered);
        vm.invalidate();
        vm.recover()?;
        assert_eq!(vm.composition().selection.selected, vec!["first"]);
        // 启动快照被漂移修改时，恢复拒绝发布替代 VM，保持空槽。
        vm.options.selection.include.insert("second".into());
        vm.invalidate();
        assert!(vm.recover().unwrap_err().contains("计划不匹配"));
        assert!(vm.needs_recovery());
        vm.options.selection.include.remove("second");
        vm.recover()?;
        assert!(!vm.needs_recovery());
        Ok(())
    }

    #[test]
    fn policy_rejects_unattributed_faults_even_with_threshold_one() -> Result<(), String> {
        let options = RegisterOptions {
            fault_disable_threshold: std::num::NonZeroU32::new(1),
            ..Default::default()
        };
        let packages = BTreeMap::from([("test".into(), ModPackage {
            manifest: serde_json::from_value(serde_json::json!({"id":"test","version":"1.0.0","apiVersion":"^1.0.0"})).map_err(|e| e.to_string())?,
            root: std::env::temp_dir().join("kk-mod-auto-policy-test"),
            files: BTreeMap::from([("server.js".into(), b"export default function register(api){api.handlers.before('builtin:link.tick',{id:'test'},()=>{});}".to_vec())]),
        })]);
        let mut realm = ModRealm::with_options(&options).map_err(|e| e.to_string())?;
        let registered = realm
            .register(&packages, options.clone())
            .map_err(|e| e.to_string())?;
        let mut vm = ModVm::new(realm, packages, options, &registered);
        let valid = Key::new("test:handler.test")?;
        assert!(vm.executables.contains(&valid));
        for (kind, binding) in [
            (ModFaultKind::Resource, Some(valid.clone())),
            (ModFaultKind::Native, Some(valid.clone())),
            (ModFaultKind::Read, Some(valid.clone())),
            (ModFaultKind::Capability, Some(valid.clone())),
            (ModFaultKind::Chain, Some(valid)),
            (ModFaultKind::Handler, None),
            (
                ModFaultKind::Handler,
                Some(Key::new("test:handler.unknown")?),
            ),
            (
                ModFaultKind::Handler,
                Some(Key::new("unknown:handler.test")?),
            ),
        ] {
            vm.last_fault = Some(ModSettlementFault {
                tick: 0,
                room: "W1N1".into(),
                target: "target".into(),
                error: ModHostError {
                    context: "test".into(),
                    message: "不能根据文本归责 test:handler.test".into(),
                    fault: Some(kk_mod_host::ModFault {
                        slot: Some("builtin:link.tick".into()),
                        binding,
                        kind,
                    }),
                },
            });
            vm.fault_streak = Some(ModFaultStreak {
                mod_id: "test".into(),
                consecutive_faults: 1,
            });
            assert!(vm.finish_settlement(false).is_none(), "{kind:?}");
            assert!(vm.fault_streak.is_none());
            assert!(vm.disabled().is_empty());
            assert!(vm.last_auto_disable.is_none());
        }
        Ok(())
    }
}
