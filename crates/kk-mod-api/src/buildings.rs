//! 建筑声明域：JS Mod 声明可建造结构的配方数据。
//!
//! 建筑以带命名空间的 `structureType`（即类型键，如 `demo:foundry`）落成
//! 权威世界的 Generic 结构：工地创建、建造进度、耐久、库存与生产全部由
//! 宿主按本目录的纯数据执行，玩法规则（成本构成、生产配方、寻路语义）
//! 留给声明方表达。这里只定义数据与冻结目录，不访问世界。

use crate::definition::Key;
use crate::plan::ContributionPlan;
use serde::Serialize;

/// 建筑对寻路与移动仲裁的语义。`Road` 表示可通行且按道路成本寻路
/// （仅影响代价矩阵，不获得内建 road 的墙地形豁免与衰减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildingPathing {
    Obstacle,
    Plain,
    Road,
}

impl BuildingPathing {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Obstacle => "obstacle",
            Self::Plain => "plain",
            Self::Road => "road",
        }
    }
}

/// 生产配方：每隔 `interval` tick 尝试一次周期——库存同时满足全部输入
/// 且全部产物能入库时，原子地消耗输入并产出；否则本轮不生产。周期按
/// 房间绝对时间取模，无逐建筑相位状态，检查点恢复后语义不变。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProductionSpec {
    pub interval: u32,
    /// 资源名 -> 每周期消耗量；可为空（免费生产）。
    pub inputs: Vec<(String, u64)>,
    /// 资源名 -> 每周期产量；非空时须声明仓库容量。
    pub outputs: Vec<(String, u64)>,
}

/// 一条建筑声明。成本是资源向量：总工程点数取最大成本量，各资源按
/// 进度比例累计消耗（整数向下取整，完工时恰好付清）；单一 energy 成本
/// 退化为内建结构的一比一行为。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BuildingDescriptor {
    pub key: Key,
    pub player_class: String,
    /// 已按资源名排序；每项 (资源名, 数量)，数量 1..=1e9。
    pub cost: Vec<(String, u64)>,
    pub hits_max: u64,
    pub pathing: BuildingPathing,
    /// 仓库总容量；Some 时完工结构携带 store/storeCapacity，None 则无仓库。
    pub store_capacity: Option<u64>,
    pub production: Option<ProductionSpec>,
}

impl BuildingDescriptor {
    /// 总工程点数 = 最大成本量。工地 progress_total 与各资源比例分母共用。
    pub fn progress_total(&self) -> u64 {
        self.cost
            .iter()
            .map(|(_, amount)| *amount)
            .max()
            .unwrap_or(0)
    }
}

/// 冻结后的建筑目录，按键排序；结算热路径按 `get` 二分查找。
/// 含冒号的类型键结构上不可能与内建结构名（均不含冒号）冲突。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BuildingCatalog {
    entries: Box<[BuildingDescriptor]>,
}

impl BuildingCatalog {
    /// 从冻结计划解析并做玩家面校验（玩家类名须为合法 JS 标识符且在
    /// 建筑间唯一）。装配期调用一次；此处失败不产出半装好的分片。
    pub fn from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        let mut classes = std::collections::BTreeSet::new();
        for building in plan.buildings() {
            if !is_js_identifier(&building.player_class) {
                return Err(format!(
                    "建筑 {} 的玩家类名 {:?} 不是合法的 JS 标识符",
                    building.key, building.player_class
                ));
            }
            if !classes.insert(building.player_class.clone()) {
                return Err(format!(
                    "玩家类名 {} 在多个建筑上重复（玩家绑定须唯一）",
                    building.player_class
                ));
            }
        }
        Ok(Self {
            entries: plan.buildings().to_vec().into_boxed_slice(),
        })
    }

    pub fn get(&self, type_key: &str) -> Option<&BuildingDescriptor> {
        self.entries
            .binary_search_by(|b| b.key.as_str().cmp(type_key))
            .ok()
            .map(|i| &self.entries[i])
    }

    /// 该类型是否声明为阻断通行。未知类型返回 false（与内建表行为一致）。
    pub fn is_obstacle(&self, type_key: &str) -> bool {
        self.get(type_key)
            .is_some_and(|b| b.pathing == BuildingPathing::Obstacle)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &BuildingDescriptor> {
        self.entries.iter()
    }
}

/// 建筑的玩家面清单条目：键、寻路语义与玩家类名，供沙箱侧白名单、
/// 代价矩阵与类绑定装配（由宿主从本目录导出，不从玩家输入读取）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SurfaceBuilding {
    pub key: String,
    pub pathing: &'static str,
    pub player_class: String,
}

impl BuildingCatalog {
    /// 玩家面建筑数据（有序）；ModPlayerApi 清单与预检 canary 共用。
    pub fn player_surface(&self) -> Vec<SurfaceBuilding> {
        self.entries
            .iter()
            .map(|b| SurfaceBuilding {
                key: b.key.to_string(),
                pathing: b.pathing.as_str(),
                player_class: b.player_class.clone(),
            })
            .collect()
    }
}

fn is_js_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Contribution, FreezeOptions, Source, builtin, freeze_with_builtin};
    use std::collections::BTreeSet;

    fn source(ns: &str) -> Source {
        Source {
            namespace: ns.into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [0; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        }
    }

    fn contribution_with(building: BuildingDescriptor) -> Contribution {
        Contribution {
            source: source("demo"),
            bindings: BTreeSet::new(),
            object_types: Vec::new(),
            field_mounts: Vec::new(),
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
            buildings: vec![building],
        }
    }

    fn foundry() -> BuildingDescriptor {
        BuildingDescriptor {
            key: Key::new("demo:foundry").unwrap(),
            player_class: "Foundry".into(),
            cost: vec![("energy".into(), 5000), ("demo:ore".into(), 100)],
            hits_max: 5000,
            pathing: BuildingPathing::Obstacle,
            store_capacity: Some(1000),
            production: Some(ProductionSpec {
                interval: 10,
                inputs: vec![("energy".into(), 50), ("demo:ore".into(), 20)],
                outputs: vec![("demo:ingot".into(), 5)],
            }),
        }
    }

    fn plan_with(contribution: Contribution) -> crate::ContributionPlan {
        let mut c = contribution;
        c.resources.push(crate::resources::ResourceDescriptor {
            key: Key::new("demo:ore").unwrap(),
        });
        c.resources.push(crate::resources::ResourceDescriptor {
            key: Key::new("demo:ingot").unwrap(),
        });
        freeze_with_builtin(builtin::link(), vec![c], FreezeOptions::default()).unwrap()
    }

    #[test]
    fn catalog_resolves_descriptors_and_pathing() {
        let plan = plan_with(contribution_with(foundry()));
        let catalog = BuildingCatalog::from_plan(&plan).unwrap();
        let b = catalog.get("demo:foundry").unwrap();
        assert_eq!(b.progress_total(), 5000);
        assert_eq!(b.production.as_ref().unwrap().interval, 10);
        assert!(catalog.is_obstacle("demo:foundry"));
        assert!(!catalog.is_obstacle("demo:missing"));
        assert!(catalog.get("extension").is_none());
        let surface = catalog.player_surface();
        assert_eq!(surface.len(), 1);
        assert_eq!(surface[0].pathing, "obstacle");
    }

    #[test]
    fn bad_player_class_rejected() {
        let mut b = foundry();
        b.player_class = "not a name".into();
        let plan = plan_with(contribution_with(b));
        let err = BuildingCatalog::from_plan(&plan).unwrap_err();
        assert!(err.contains("JS 标识符"), "{err}");

        let mut b = foundry();
        b.player_class = "$Panel".into();
        let plan = plan_with(contribution_with(b));
        assert!(BuildingCatalog::from_plan(&plan).is_ok());
    }
}
