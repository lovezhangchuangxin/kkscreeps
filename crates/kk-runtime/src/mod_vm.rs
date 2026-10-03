//! Mod VM 的分片生命周期：失败即销毁，按固定启动快照重建后才恢复执行。

use std::collections::{BTreeMap, BTreeSet};

use kk_mod_api::{Key, PlanHash};
use kk_mod_host::{ModPackage, ModRealm, RegisterOptions};

pub(crate) struct ModVm {
    realm: Option<ModRealm>,
    packages: BTreeMap<String, ModPackage>,
    options: RegisterOptions,
    plan_hash: PlanHash,
    executables: BTreeSet<Key>,
    fail_next_rebuild: bool,
}

impl ModVm {
    pub(crate) fn new(
        realm: ModRealm,
        packages: BTreeMap<String, ModPackage>,
        options: RegisterOptions,
        plan_hash: PlanHash,
        executables: BTreeSet<Key>,
    ) -> Self {
        Self {
            realm: Some(realm),
            packages,
            options,
            plan_hash,
            executables,
            fail_next_rebuild: false,
        }
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
        let mut replacement = ModRealm::new().map_err(|e| format!("Mod VM 重建：{e}"))?;
        let registered = replacement
            .register(&self.packages, self.options.clone())
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
        let mut vm = ModVm::new(
            realm,
            packages,
            options,
            registered.plan.hash(),
            registered.executables,
        );
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
}
