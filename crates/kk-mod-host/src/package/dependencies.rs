use std::collections::BTreeMap;

use super::manifest::VersionReq;
use crate::error::{ModHostError, ModHostResult};
use crate::package::ModPackage;

/// 依赖拓扑序（被依赖者在前）。同层按包 id 排序，装载顺序不参与结果。
/// 依赖缺失、版本不满足与依赖环都是装载错误。
pub fn dependency_order(packages: &BTreeMap<String, ModPackage>) -> ModHostResult<Vec<String>> {
    for package in packages.values() {
        for (dependency, range) in &package.manifest.dependencies {
            let Some(target) = packages.get(dependency) else {
                return Err(ModHostError::new(
                    package.id().to_string(),
                    format!("依赖的包不存在：{dependency}"),
                ));
            };
            let requirement =
                VersionReq::parse(range, package.id()).expect("manifest 解析已校验范围写法");
            let actual = super::manifest::Version::parse(&target.manifest.version, target.id())
                .expect("manifest 解析已校验版本");
            if !requirement.satisfied_by(actual) {
                return Err(ModHostError::new(
                    package.id().to_string(),
                    format!(
                        "依赖 {dependency} 需要 {range}，实得 {}",
                        target.manifest.version
                    ),
                ));
            }
        }
    }
    let mut remaining: BTreeMap<&str, &ModPackage> = packages
        .iter()
        .map(|(id, package)| (id.as_str(), package))
        .collect();
    let mut ordered = Vec::with_capacity(packages.len());
    while !remaining.is_empty() {
        // 就绪集合遍历序确定：BTreeMap 按 id 排序，输出与输入目录序无关。
        let ready: Vec<&str> = remaining
            .iter()
            .filter(|(_, package)| {
                package
                    .manifest
                    .dependencies
                    .keys()
                    .all(|dependency| !remaining.contains_key(dependency.as_str()))
            })
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            let cycle = remaining.keys().copied().collect::<Vec<_>>().join(", ");
            return Err(ModHostError::new(
                "dependency graph",
                format!("依赖环（含其下游）：{cycle}"),
            ));
        }
        for id in ready {
            remaining.remove(id);
            ordered.push(id.to_string());
        }
    }
    Ok(ordered)
}
