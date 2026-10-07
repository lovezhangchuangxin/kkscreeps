//! 从同一冻结计划安装规则和活动链；停用不改变规则目录。
use super::*;

/// 装入 JS 的有效规则条目。整数按双精度转换：超过 ±(2^53−1) 时取最近
/// 双精度值，与处理器 state/args 写回候选所走的 serde_json Value → JS
/// number 转换同一语义，不引入 BigInt。
#[derive(Serialize)]
struct RuleEntry {
    key: String,
    value: f64,
}

impl ModRealm {
    pub(super) fn install_chains(
        &mut self,
        plan: &ContributionPlan,
        contributions: &[Contribution],
        disabled: &BTreeSet<String>,
    ) -> ModHostResult<()> {
        let programs = ChainProgram::from_plan(plan, contributions, disabled)
            .map_err(|error| ModHostError::new("chain", error))?;
        {
            let runtime_ref = &mut self.runtime;
            deno_core::scope!(scope, runtime_ref);
            v8::tc_scope!(let tc, scope);
            let install = v8::Local::new(tc, self.sdk_install_chains.clone());
            let value = deno_core::serde_v8::to_v8(tc, &programs)
                .map_err(|error| ModHostError::new("chain", error.to_string()))?;
            let receiver = v8::undefined(tc).into();
            install
                .call(tc, receiver, &[value])
                .ok_or_else(|| scope_error!(tc, "chain", "链安装失败"))?;
        }
        // 有效规则与执行链同源同批安装：值直接来自冻结计划（含默认、
        // 单方覆盖与显式选择的多方覆盖），恢复重建沿同一入口重装并受
        // PlanHash 核验；停用掩码不影响这里装入的数值。
        let entries: Vec<RuleEntry> = plan
            .rules()
            .entries()
            .map(|(key, value)| RuleEntry {
                key: key.to_string(),
                value: match value {
                    Number::Signed(v) => *v as f64,
                    Number::Unsigned(v) => *v as f64,
                    Number::Float(v) => *v,
                },
            })
            .collect();
        {
            let runtime_ref = &mut self.runtime;
            deno_core::scope!(scope, runtime_ref);
            v8::tc_scope!(let tc, scope);
            let install = v8::Local::new(tc, self.sdk_install_rules.clone());
            let value = deno_core::serde_v8::to_v8(tc, &entries)
                .map_err(|error| ModHostError::new("register", error.to_string()))?;
            let receiver = v8::undefined(tc).into();
            install
                .call(tc, receiver, &[value])
                .ok_or_else(|| scope_error!(tc, "register", "有效规则安装失败"))?;
        }
        self.inactive_slots = plan
            .slots()
            .iter()
            .filter(|slot| disabled.contains(slot.key.namespace()))
            .map(|slot| slot.key.to_string())
            .collect();
        self.chains = programs
            .into_iter()
            .map(|program| {
                (
                    program.key.clone(),
                    InstalledChain {
                        native_center: program.native_center,
                        decline_allowed: program.decline_allowed,
                        bindings: Rc::new(program.bindings()),
                    },
                )
            })
            .collect();
        Ok(())
    }
}
