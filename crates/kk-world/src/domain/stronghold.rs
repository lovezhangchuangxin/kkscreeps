//! 官方 stronghold 布防模板与容器奖励表；部署执行由 engine 结算层完成。
//! 来源：上游 common/lib/strongholds.js（templates/containerRewards/containerAmounts）。

/// 模板内一条结构放置项，坐标为相对核心的位移（可为负）。
pub struct StrongholdEntry {
    pub kind: &'static str,
    pub dx: i8,
    pub dy: i8,
}

/// 一套 bunker 模板：reward_level 决定 rampart/容器奖励取值档位，
/// core_level 是模板自带的核心等级（放置层负责写入核心数据）。
pub struct StrongholdTemplate {
    pub name: &'static str,
    pub reward_level: i64,
    pub core_level: i64,
    /// 不含 invaderCore 条目：核心在部署中保持原位不重插。
    pub structures: &'static [StrongholdEntry],
}

/// 官方模板条目中的核心项（type/level/strongholdBehavior）不进入结构表：
/// 核心对象由放置归一建好，部署只负责布防结构。
pub use super::stronghold_templates::TEMPLATES;

/// 容器奖励资源与密度权重；部署时随机取 3 项凑满奖励总量。
/// 资源名与 kk/官方资源目录一致（utrium_bar 等）；键序保持官方序。
/// 来源：上游 strongholds.js 的 containerRewards。
pub const CONTAINER_REWARDS: &[(&str, i64)] = &[
    ("utrium_bar", 5),
    ("lemergium_bar", 5),
    ("zynthium_bar", 5),
    ("keanium_bar", 5),
    ("oxidant", 5),
    ("reductant", 5),
    ("purifier", 5),
    ("ghodium_melt", 20),
    ("battery", 10),
    ("composite", 10),
    ("crystal", 15),
    ("liquid", 30),
];

/// 按模板奖励档位（0..=5）的容器奖励总量；0 档无奖励。
/// 来源：上游 strongholds.js 的 containerAmounts。
pub const CONTAINER_AMOUNTS: &[i64] = &[0, 500, 4000, 10000, 50000, 360000];

/// 按名字查找布防模板。
pub fn template(name: &str) -> Option<&'static StrongholdTemplate> {
    TEMPLATES.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 common/lib/strongholds.js 的结构搬运核对：条目计数、抽查
    /// 坐标、无重复条目；计数按官方源逐模板数出。
    #[test]
    fn templates_match_official_layouts() {
        assert_eq!(TEMPLATES.len(), 5);
        let counts: Vec<usize> = TEMPLATES.iter().map(|t| t.structures.len()).collect();
        assert_eq!(counts, [8, 19, 34, 53, 93]);
        for t in TEMPLATES {
            assert_eq!(t.reward_level, t.core_level);
            // 坐标必须落在以核心为中心的 7×7 布防区内。
            assert!(
                t.structures
                    .iter()
                    .all(|e| e.dx.abs() <= 3 && e.dy.abs() <= 3),
                "{} 存在超出布防区的条目",
                t.name
            );
            // 同 (type,dx,dy) 不得重复；同格异类型是官方常态。
            let mut seen = std::collections::HashSet::new();
            for e in t.structures {
                assert!(seen.insert((e.kind, e.dx, e.dy)), "{} 重复条目", t.name);
            }
        }
        // 抽查各模板代表条目与官方源一致。
        let b1 = template("bunker1").unwrap();
        assert_eq!(b1.structures[0].kind, "rampart");
        assert_eq!((b1.structures[0].dx, b1.structures[0].dy), (0, 0));
        assert_eq!(b1.structures[3].kind, "road");
        assert_eq!((b1.structures[3].dx, b1.structures[3].dy), (0, 1));
        assert_eq!(b1.structures[4].kind, "rampart");
        assert_eq!((b1.structures[4].dx, b1.structures[4].dy), (0, 1));
        let b5 = template("bunker5").unwrap();
        assert_eq!(b5.structures[9].kind, "tower");
        assert_eq!((b5.structures[9].dx, b5.structures[9].dy), (0, -2));
        assert_eq!(b5.structures[92].kind, "rampart");
        assert_eq!((b5.structures[92].dx, b5.structures[92].dy), (-2, 2));
        assert!(template("bunker6").is_none());
    }

    /// 奖励资源名都在 kk 资源目录内，密度为正；档位总量与官方一致。
    #[test]
    fn reward_tables_match_official_values() {
        assert_eq!(CONTAINER_REWARDS.len(), 12);
        for (name, density) in CONTAINER_REWARDS {
            assert!(
                kk_core::constants::RESOURCES_ALL.iter().any(|r| r == name),
                "{name} 不在资源目录"
            );
            assert!(*density > 0);
        }
        assert_eq!(CONTAINER_AMOUNTS, &[0, 500, 4000, 10000, 50000, 360000]);
    }
}
