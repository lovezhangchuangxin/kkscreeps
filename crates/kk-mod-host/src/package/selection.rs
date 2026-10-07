//! 新世界启动期的显式内容选择：把宿主请求解析为实际安装的包集合。
//!
//! 选择语义由三处入口共用：宿主 [`crate::RegisterOptions`]、sim 场景的
//! mods 输入与 mod inspect CLI。`include`/`exclude` 均为空表示不做显式
//! 选择，安装全部发现的包（既有行为，返回 None 不复制包表）。非空选择
//! 按 manifest 依赖闭包展开，自动加入的包在报告中列明；显式排除与必需
//! 依赖相撞时拒绝整个配置并给出依赖路径，不静默重新加入被排除内容。
//! builtin 是固定内建贡献，不可经内容选择安装或关闭；内建默认内容的
//! 逐族迁移见 docs/designs/content-packs.md。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::error::{ModHostError, ModHostResult};
use crate::package::ModPackage;

/// 显式内容选择输入。`include` 是显式选择的包 ID；`exclude` 是显式排除
/// 的包 ID（在 include 为空时表示"全部发现的包减去排除项"）。二者不得
/// 重叠，也不得包含 builtin。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentSelection {
    #[serde(default)]
    pub include: BTreeSet<String>,
    #[serde(default)]
    pub exclude: BTreeSet<String>,
}

impl ContentSelection {
    /// 是否完全没有显式选择（安装全部发现的包）。
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }
}

/// 选择解析报告：记录展开后的实际内容身份。进入组合诊断报告；
/// 选择本身不参与 PlanHash——同样的安装集合来自不同输入是同一计划。
#[derive(Clone, Debug, Serialize)]
pub struct SelectionReport {
    /// 输入的显式选择（排序）。
    pub requested: Vec<String>,
    /// 输入的显式排除（排序）。
    pub excluded: Vec<String>,
    /// 展开后实际安装的包（排序）。
    pub selected: Vec<String>,
    /// 依赖闭包自动加入的包 = selected − include；include 为空时恒为空。
    pub auto_added: Vec<String>,
    /// 已发现但未安装的包（排序）。
    pub skipped: Vec<String>,
}

/// 把选择输入解析为实际安装的包表与报告：返回 None 表示无需过滤
/// （无显式选择，调用方直接使用发现的全部包，避免整表复制）。
/// 错误在注册装载之前发生：未知包 ID、缺失依赖与排除相撞都携带
/// 依赖路径，不用旧装载错误替代。
pub(crate) fn resolve_selection(
    packages: &BTreeMap<String, ModPackage>,
    selection: &ContentSelection,
) -> ModHostResult<(Option<BTreeMap<String, ModPackage>>, SelectionReport)> {
    if let Some(id) = selection.include.intersection(&selection.exclude).next() {
        return Err(ModHostError::new(
            "内容选择",
            format!("同一名包不能同时显式选择与排除：{id}"),
        ));
    }
    for id in selection.include.iter().chain(selection.exclude.iter()) {
        if id == "builtin" {
            return Err(ModHostError::new(
                "内容选择",
                "builtin 是固定内建贡献，不可经内容选择安装或关闭（内建默认内容的迁移见 docs/designs/content-packs.md）",
            ));
        }
        if !packages.contains_key(id) {
            let kind = if selection.include.contains(id) {
                "选择"
            } else {
                "排除"
            };
            // 清单截断到前 8 个：诊断要能人工对照，但不随包根目录规模变长。
            const LISTED: usize = 8;
            let names: Vec<&str> = packages.keys().map(String::as_str).collect();
            let found = if names.is_empty() {
                "包根目录未发现任何包".to_string()
            } else {
                let listed = names[..names.len().min(LISTED)].join("、");
                if names.len() > LISTED {
                    format!("{listed} 等 {} 个", names.len())
                } else {
                    listed
                }
            };
            return Err(ModHostError::new(
                "内容选择",
                format!("{kind}的包不存在：{id}（{found}）"),
            ));
        }
    }
    let mut selected: BTreeSet<String> = BTreeSet::new();
    let mut filtered: Option<BTreeMap<String, ModPackage>> = None;
    if !selection.is_empty() {
        // 队列元素是从种子到当前包的依赖路径；展开只沿 manifest
        // dependencies，不包含 handler dependsOn（后者要求被引用包已选，
        // 冻结期按未知处理器依赖拒绝）。BTreeSet/BTreeMap 迭代有序，
        // 报错与报告都与目录装载序无关。
        let seeds: Vec<String> = if selection.include.is_empty() {
            packages
                .keys()
                .filter(|id| !selection.exclude.contains(*id))
                .cloned()
                .collect()
        } else {
            selection.include.iter().cloned().collect()
        };
        let mut queue: VecDeque<Vec<String>> = seeds.iter().map(|id| vec![id.clone()]).collect();
        while let Some(path) = queue.pop_front() {
            let id = path.last().expect("路径以种子开头，非空").clone();
            if selected.contains(&id) {
                continue;
            }
            for dependency in packages[&id].manifest.dependencies.keys() {
                if !packages.contains_key(dependency) {
                    return Err(ModHostError::new(
                        "内容选择",
                        format!(
                            "缺失依赖：{} → {dependency}（{dependency} 不在包根目录）",
                            path.join(" → ")
                        ),
                    ));
                }
                if selection.exclude.contains(dependency) {
                    return Err(ModHostError::new(
                        "内容选择",
                        format!(
                            "显式排除 {dependency} 与必需依赖相撞：{} → {dependency}；须调整排除或选择，不会静默重新加入",
                            path.join(" → ")
                        ),
                    ));
                }
                if !selected.contains(dependency) {
                    let mut next = path.clone();
                    next.push(dependency.clone());
                    queue.push_back(next);
                }
            }
            selected.insert(id);
        }
        // 显式选择才过滤出安装子表；装载器仍按既有方式各自持有包快照。
        filtered = Some(
            packages
                .iter()
                .filter(|(id, _)| selected.contains(*id))
                .map(|(id, package)| (id.clone(), package.clone()))
                .collect(),
        );
    } else {
        selected.extend(packages.keys().cloned());
    }
    let auto_added = if selection.include.is_empty() {
        Vec::new()
    } else {
        selected.difference(&selection.include).cloned().collect()
    };
    let report = SelectionReport {
        requested: selection.include.iter().cloned().collect(),
        excluded: selection.exclude.iter().cloned().collect(),
        selected: selected.iter().cloned().collect(),
        auto_added,
        skipped: packages
            .keys()
            .filter(|id| !selected.contains(*id))
            .cloned()
            .collect(),
    };
    Ok((filtered, report))
}
