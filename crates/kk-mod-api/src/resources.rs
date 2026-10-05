//! 资源领域能力的值协议与冻结资源目录；这里只定义数据，不访问世界。

use crate::{ContributionPlan, Key};
use serde::{Deserialize, Serialize};

/// 当前候选中的单资源库存与容量。遗留物仅可作为来源，容量与余量为 None。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceView {
    pub amount: i64,
    pub capacity: Option<i64>,
    pub free: Option<i64>,
}

/// 一次精确转账的结果；库存或容量不足时三项均为 0，不产生写入或损耗。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceTransfer {
    pub debited: i64,
    pub credited: i64,
    pub lost: i64,
}

/// Mod 声明的命名空间资源。键即玩家面与库存槽中的资源名
/// （如 `demo:crystal`）；键含冒号且命名空间归本包，结构上不可能
/// 与内建资源名（均不含冒号）冲突。首片只有目录身份，没有逐资源
/// 数值；容量沿既有仓库规则面。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResourceDescriptor {
    pub key: Key,
}

/// 冻结后的资源目录：内建 `RESOURCES_ALL` 加计划声明的命名空间资源。
/// 由装配期解析一次，结算热路径按 `contains` 查询；键解析只应发生在
/// 装配时。市场与跨房发送不读取本目录（明确只认内建资源，见
/// 架构文档的当前边界）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceCatalog {
    /// 排序后的声明键；BTreeSet 冻结产物天然有序，二分查找。
    declared: Box<[Key]>,
}

impl Default for ResourceCatalog {
    fn default() -> Self {
        Self {
            declared: Box::new([]),
        }
    }
}

impl ResourceCatalog {
    pub fn from_plan(plan: &ContributionPlan) -> Self {
        Self {
            declared: plan.resources().to_vec().into_boxed_slice(),
        }
    }

    /// 资源名是否在目录中。含冒号的名字只可能来自声明面（键要求
    /// `namespace:name`），不含冒号的名字只可能在内建表——先按冒号
    /// 分流再做线性/二分查找，两条路径互不污染。
    pub fn contains(&self, name: &str) -> bool {
        if name.as_bytes().contains(&b':') {
            self.find_declared(name).is_some()
        } else {
            kk_core::constants::RESOURCES_ALL.contains(&name)
        }
    }

    /// 是否为本计划声明的命名空间资源（非内建）。当前无生产消费方：
    /// 市场与跨房三道门直接以内建表白名单拒绝（含冒号的名字必不在其中），
    /// 不读取本目录；此方法供 N3 开放 Mod 资源市场时复用，测试锁定语义。
    pub fn is_declared(&self, name: &str) -> bool {
        name.as_bytes().contains(&b':') && self.find_declared(name).is_some()
    }

    /// 玩家常量 `RESOURCES_ALL` 的完整值：内建序在前，声明键按
    /// 字典序追加。无声明资源时与内建表完全一致（调用方可据此跳过覆盖）。
    pub fn player_resources_all(&self) -> Vec<String> {
        kk_core::constants::RESOURCES_ALL
            .iter()
            .map(|name| (*name).to_string())
            .chain(self.declared.iter().map(|key| key.to_string()))
            .collect()
    }

    /// 掉落/超容回收的既定遍历序：内建声明序，随后声明键字典序。
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        kk_core::constants::RESOURCES_ALL
            .iter()
            .copied()
            .chain(self.declared.iter().map(Key::as_str))
    }

    pub fn declared_resources(&self) -> &[Key] {
        &self.declared
    }

    fn find_declared(&self, name: &str) -> Option<usize> {
        self.declared
            .binary_search_by(|key| key.as_str().cmp(name))
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Contribution, FreezeOptions, Source, builtin, freeze_with_builtin};
    use std::collections::BTreeSet;

    fn plan_with(resource: &str) -> ContributionPlan {
        let mut contribution = Contribution {
            source: Source {
                namespace: "demo".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [1; 32],
                config_digest: [2; 32],
                player_api_digest: [3; 32],
            },
            bindings: BTreeSet::new(),
            field_mounts: Vec::new(),
            object_types: Vec::new(),
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
            buildings: Vec::new(),
        };
        contribution.resources.push(ResourceDescriptor {
            key: Key::new(resource).unwrap(),
        });
        freeze_with_builtin(
            builtin::link(),
            vec![contribution],
            FreezeOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn catalog_merges_builtin_and_declared_in_stable_order() {
        let plan = plan_with("demo:crystal");
        let catalog = ResourceCatalog::from_plan(&plan);
        assert!(catalog.contains("energy"));
        assert!(catalog.contains("demo:crystal"));
        assert!(!catalog.contains("demo:missing"));
        assert!(!catalog.contains("not-a-resource"));
        assert!(catalog.is_declared("demo:crystal"));
        assert!(!catalog.is_declared("energy"));
        let all = catalog.player_resources_all();
        assert_eq!(all.len(), kk_core::constants::RESOURCES_ALL.len() + 1);
        assert_eq!(all.last().map(String::as_str), Some("demo:crystal"));
        // 内建序保持 constants.json 的原始顺序（首元素 energy）。
        assert_eq!(all.first().map(String::as_str), Some("energy"));
    }

    #[test]
    fn builtin_only_catalog_matches_vanilla_resources_all() {
        let plan = freeze_with_builtin(builtin::link(), vec![], FreezeOptions::default()).unwrap();
        let catalog = ResourceCatalog::from_plan(&plan);
        assert_eq!(
            catalog.player_resources_all(),
            kk_core::constants::RESOURCES_ALL
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        );
    }
}
