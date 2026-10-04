//! Generic 仓库容量的冻结数值规则。对象容量仍通过权威字段投影，
//! 此表提供放置、建造和实验室槽切换的默认值，不新增可存储类型白名单。

use super::*;

/// 装配期解析一次的容量快照，结算热路径不解析规则键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreRules {
    pub container: i64,
    pub factory: i64,
    pub terminal: i64,
    pub tower: i64,
    pub lab_energy: i64,
    pub lab_mineral: i64,
    pub power_spawn_energy: i64,
    pub power_spawn_power: i64,
    pub nuker_energy: i64,
    pub nuker_ghodium: i64,
}

// 同一清单同时生成声明、读取顺序和玩家常量映射，防止注册键与消费者分叉。
const ENTRIES: [(&str, &str, i64); 10] = [
    (
        "store.container.capacity",
        "CONTAINER_CAPACITY",
        c::CONTAINER_CAPACITY,
    ),
    (
        "store.factory.capacity",
        "FACTORY_CAPACITY",
        c::FACTORY_CAPACITY,
    ),
    (
        "store.terminal.capacity",
        "TERMINAL_CAPACITY",
        c::TERMINAL_CAPACITY,
    ),
    ("store.tower.capacity", "TOWER_CAPACITY", c::TOWER_CAPACITY),
    (
        "store.lab.energyCapacity",
        "LAB_ENERGY_CAPACITY",
        c::LAB_ENERGY_CAPACITY,
    ),
    (
        "store.lab.mineralCapacity",
        "LAB_MINERAL_CAPACITY",
        c::LAB_MINERAL_CAPACITY,
    ),
    (
        "store.powerSpawn.energyCapacity",
        "POWER_SPAWN_ENERGY_CAPACITY",
        c::POWER_SPAWN_ENERGY_CAPACITY,
    ),
    (
        "store.powerSpawn.powerCapacity",
        "POWER_SPAWN_POWER_CAPACITY",
        c::POWER_SPAWN_POWER_CAPACITY,
    ),
    (
        "store.nuker.energyCapacity",
        "NUKER_ENERGY_CAPACITY",
        c::NUKER_ENERGY_CAPACITY,
    ),
    (
        "store.nuker.ghodiumCapacity",
        "NUKER_GHODIUM_CAPACITY",
        c::NUKER_GHODIUM_CAPACITY,
    ),
];

impl StoreRules {
    pub fn from_plan(rules: &EffectiveRules) -> Result<Self, String> {
        let mut values = [0; 10];
        for (i, (name, _, _)) in ENTRIES.iter().enumerate() {
            let handle = rules
                .resolve(&key(name))
                .ok_or_else(|| format!("计划缺少规则 builtin:{name}"))?;
            values[i] = match rules.get(handle) {
                Some(Number::Signed(n)) => *n,
                _ => return Err(format!("规则 builtin:{name} 必须为整数")),
            };
        }
        let [
            container,
            factory,
            terminal,
            tower,
            lab_energy,
            lab_mineral,
            power_spawn_energy,
            power_spawn_power,
            nuker_energy,
            nuker_ghodium,
        ] = values;
        Ok(Self {
            container,
            factory,
            terminal,
            tower,
            lab_energy,
            lab_mineral,
            power_spawn_energy,
            power_spawn_power,
            nuker_energy,
            nuker_ghodium,
        })
    }

    /// 同步标准玩家常量；投影和 API 预检仍优先读取对象的显式容量字段。
    pub fn constant_overrides(&self) -> BTreeMap<String, serde_json::Value> {
        let values = [
            self.container,
            self.factory,
            self.terminal,
            self.tower,
            self.lab_energy,
            self.lab_mineral,
            self.power_spawn_energy,
            self.power_spawn_power,
            self.nuker_energy,
            self.nuker_ghodium,
        ];
        ENTRIES
            .iter()
            .zip(values)
            .map(|((_, constant, _), value)| ((*constant).into(), json!(value)))
            .collect()
    }
}

pub(super) fn declarations() -> (BTreeSet<Key>, Vec<RuleDescriptor>) {
    let consumers = keys(&[
        "store.init",
        "store.build",
        "store.constants",
        "store.labSlots",
        "store.terminalTick",
        "store.containerDeposit",
    ]);
    let rules = ENTRIES
        .iter()
        .map(|(name, _, value)| {
            let mut used_by = keys(&["store.init", "store.build", "store.constants"]);
            if name.starts_with("store.lab.") {
                used_by.insert(key("store.labSlots"));
            }
            if *name == "store.terminal.capacity" {
                used_by.insert(key("store.terminalTick"));
            }
            if *name == "store.container.capacity" {
                used_by.insert(key("store.containerDeposit"));
            }
            RuleDescriptor {
                key: key(name),
                // 两槽总容量最多 2*u32::MAX，i64 相加不会溢出。
                domain: NumberDomain::Signed {
                    min: 0,
                    max: i64::from(u32::MAX),
                },
                default: Number::Signed(*value),
                consumers: used_by,
            }
        })
        .collect();
    (consumers, rules)
}
