//! 静态通行规则目录与冻结快照；所有消费者只读取校验后的同一值域。
use super::{key, keys};
use crate::{EffectiveRules, Key, Number, NumberDomain, RuleDescriptor};
use std::collections::BTreeSet;

/// 计划级静态通行快照：结算、玩家默认矩阵与 NPC 搜索同源。
/// 生物占格、portal 避让与建造/孵化放置不受对象通行规则改变。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StaticPassability {
    pub wall_passable: bool,
    pub obstacles_passable: bool,
    pub private_ramparts_passable: bool,
}

impl StaticPassability {
    /// 缺键说明计划未含完整内建目录；数值域在冻结阶段校验。
    pub fn from_plan(rules: &EffectiveRules) -> Result<Self, String> {
        let flag = |name: &str| {
            let key = key(name);
            let handle = rules
                .resolve(&key)
                .ok_or_else(|| format!("计划缺少规则 {key}"))?;
            match rules.get(handle) {
                Some(Number::Signed(0)) => Ok(false),
                Some(Number::Signed(1)) => Ok(true),
                other => Err(format!("规则 {key} 须为 0 或 1，实得 {other:?}")),
            }
        };
        Ok(Self {
            wall_passable: flag("terrain.wallPassable")?,
            obstacles_passable: flag("movement.obstaclesPassable")?,
            private_ramparts_passable: flag("movement.privateRampartsPassable")?,
        })
    }
}

pub(super) fn declarations() -> (BTreeSet<Key>, Vec<RuleDescriptor>) {
    let bindings = keys(&[
        "terrain.passability.native",
        "movement.staticPassability.native",
    ]);
    let rules = [
        ("terrain.wallPassable", "terrain.passability.native"),
        (
            "movement.obstaclesPassable",
            "movement.staticPassability.native",
        ),
        (
            "movement.privateRampartsPassable",
            "movement.staticPassability.native",
        ),
    ]
    .into_iter()
    .map(|(name, consumer)| RuleDescriptor {
        key: key(name),
        domain: NumberDomain::Signed { min: 0, max: 1 },
        default: Number::Signed(0),
        consumers: keys(&[consumer]),
    })
    .collect();
    (bindings, rules)
}
