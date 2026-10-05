//! 冻结输入的组合诊断；不执行 JS，也不改变规则或已提交数据。
use std::collections::{BTreeMap, BTreeSet};

use kk_mod_api::{
    Contribution, ContributionPlan, Dispatch, HandlerDescriptor, Key, Number, SlotPlan,
};
use serde::Serialize;

use crate::{ExecutionChain, ModPackage};

#[derive(Clone, Debug, Serialize)]
pub struct RuleSelection {
    pub key: Key,
    pub default: Number,
    pub value: Number,
    pub definition_source: String,
    /// 无覆盖时为定义来源；覆盖时是被冻结选中的贡献来源。
    pub selected_source: String,
    pub overrides: BTreeMap<String, Number>,
    pub consumers: BTreeSet<Key>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComposedSlot {
    pub key: Key,
    pub stage: kk_mod_api::Stage,
    pub original_center: Key,
    pub frozen: Dispatch,
    /// 槽所属包停用时为 None；否则与实际安装的链共用选择逻辑。
    pub active: Option<ExecutionChain>,
    pub execution: ExecutionMode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Native,
    Hosted,
    Inactive,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompositionReport {
    pub report_version: u32,
    pub plan_hash: String,
    pub sources: BTreeMap<String, kk_mod_api::Source>,
    /// 实际内容身份：显式选择输入、展开后安装清单与自动加入项。
    /// 报告版本 2 起提供；选择输入本身不参与 plan_hash。
    pub selection: crate::SelectionReport,
    /// 计划声明的命名空间资源目录（报告版本 3 起提供）：键与定义来源。
    /// 内建 RESOURCES_ALL 不在此列；玩家常量由两者合并同步。
    pub resources: Vec<DeclaredResource>,
    pub handlers: Vec<HandlerDescriptor>,
    pub slots: Vec<ComposedSlot>,
    pub rules: Vec<RuleSelection>,
    /// manifest 与显式 handler 依赖合并后的 Mod 级停用关系；不代表加载顺序。
    pub dependencies: BTreeMap<String, BTreeSet<String>>,
    /// 每个包停用时受影响的传递闭包，包含自身。
    pub disable_closures: BTreeMap<String, BTreeSet<String>>,
    pub disabled_mods: BTreeSet<String>,
}

/// 组合报告中的资源目录条目：键与声明它的贡献来源。
#[derive(Clone, Debug, Serialize)]
pub struct DeclaredResource {
    pub key: Key,
    pub source: String,
}

impl CompositionReport {
    pub(crate) fn build(
        plan: &ContributionPlan,
        contributions: &[Contribution],
        packages: &BTreeMap<String, ModPackage>,
        options: &kk_mod_api::FreezeOptions,
        disabled: &BTreeSet<String>,
        selection: &crate::SelectionReport,
    ) -> Result<Self, String> {
        let builtin = kk_mod_api::builtin::link();
        let all: Vec<_> = std::iter::once(&builtin).chain(contributions).collect();
        let centers: BTreeMap<_, _> = all
            .iter()
            .flat_map(|c| &c.slots)
            .map(|s| (&s.key, &s.native_binding))
            .collect();
        let mut handlers: Vec<_> = all.iter().flat_map(|c| &c.handlers).cloned().collect();
        handlers.sort_by(|a, b| a.key.cmp(&b.key));
        let mut dependencies: BTreeMap<_, BTreeSet<_>> = packages
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    p.manifest.dependencies.keys().cloned().collect(),
                )
            })
            .collect();
        for handler in &handlers {
            for dependency in &handler.depends_on {
                let owner = handler.key.namespace();
                if owner != dependency.namespace()
                    && dependencies.contains_key(dependency.namespace())
                {
                    dependencies
                        .get_mut(owner)
                        .ok_or("处理器来源缺少包身份")?
                        .insert(dependency.namespace().into());
                }
            }
        }
        let disable_closures = dependencies
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    closure(&dependencies, BTreeSet::from([id.clone()])),
                )
            })
            .collect();
        let overrides: BTreeMap<Key, BTreeMap<String, Number>> =
            all.iter().fold(BTreeMap::new(), |mut values, c| {
                for value in &c.overrides {
                    values
                        .entry(value.rule.clone())
                        .or_default()
                        .insert(c.source.namespace.clone(), value.value.clone());
                }
                values
            });
        let mut rules = Vec::new();
        for descriptor in all.iter().flat_map(|c| &c.rules) {
            let handle = plan
                .rules()
                .resolve(&descriptor.key)
                .ok_or("冻结计划缺少规则")?;
            let value = plan.rules().get(handle).ok_or("冻结规则句柄失效")?.clone();
            let candidates = overrides.get(&descriptor.key).cloned().unwrap_or_default();
            let selected_source = options
                .rule_sources
                .get(&descriptor.key)
                .cloned()
                .or_else(|| candidates.keys().next().cloned())
                .unwrap_or_else(|| descriptor.key.namespace().into());
            rules.push(RuleSelection {
                key: descriptor.key.clone(),
                default: descriptor.default.clone(),
                value,
                definition_source: descriptor.key.namespace().into(),
                selected_source,
                overrides: candidates,
                consumers: descriptor.consumers.clone(),
            });
        }
        rules.sort_by(|a, b| a.key.cmp(&b.key));
        let mut resources: Vec<_> = all
            .iter()
            .flat_map(|c| {
                c.resources.iter().map(|resource| DeclaredResource {
                    key: resource.key.clone(),
                    source: c.source.namespace.clone(),
                })
            })
            .collect();
        resources.sort_by(|a, b| a.key.cmp(&b.key));
        let slots = plan
            .slots()
            .iter()
            .map(|slot| {
                let original = centers
                    .get(&slot.key)
                    .copied()
                    .ok_or("冻结槽缺少原始中心")?;
                let active = ExecutionChain::for_slot(slot, original, disabled)?;
                Ok(ComposedSlot {
                    key: slot.key.clone(),
                    stage: slot.stage,
                    original_center: original.clone(),
                    frozen: slot.dispatch.clone(),
                    execution: mode(&active, &slot.dispatch),
                    active,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            report_version: 3,
            plan_hash: plan.hash().hex(),
            sources: plan.sources().clone(),
            selection: selection.clone(),
            resources,
            handlers,
            slots,
            rules,
            dependencies,
            disable_closures,
            disabled_mods: disabled.clone(),
        })
    }

    /// 预测/实施一次停用使用同一闭包；不重注册、不重算规则。
    pub fn with_disabled(&self, requested: &BTreeSet<String>) -> Result<Self, String> {
        for id in requested {
            if !self.dependencies.contains_key(id) {
                return Err(format!("未知 Mod {id}"));
            }
        }
        let mut report = self.clone();
        let seeds = self.disabled_mods.union(requested).cloned().collect();
        report.disabled_mods = closure(&self.dependencies, seeds);
        for slot in &mut report.slots {
            let plan = SlotPlan {
                key: slot.key.clone(),
                stage: slot.stage,
                dispatch: slot.frozen.clone(),
            };
            slot.active =
                ExecutionChain::for_slot(&plan, &slot.original_center, &report.disabled_mods)?;
            slot.execution = mode(&slot.active, &slot.frozen);
        }
        Ok(report)
    }
}

// 引擎按冻结 dispatch 选择 Native/Hosted；停用钩子不会重装引擎执行表。
fn mode(active: &Option<ExecutionChain>, frozen: &Dispatch) -> ExecutionMode {
    match active {
        None => ExecutionMode::Inactive,
        Some(_) if matches!(frozen, Dispatch::Native(binding) if binding.namespace()=="builtin") => {
            ExecutionMode::Native
        }
        Some(_) => ExecutionMode::Hosted,
    }
}

fn closure(
    graph: &BTreeMap<String, BTreeSet<String>>,
    mut disabled: BTreeSet<String>,
) -> BTreeSet<String> {
    loop {
        let count = disabled.len();
        for (id, deps) in graph {
            if !deps.is_disjoint(&disabled) {
                disabled.insert(id.clone());
            }
        }
        if count == disabled.len() {
            return disabled;
        }
    }
}
