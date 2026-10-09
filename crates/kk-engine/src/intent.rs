//! 解析玩家指令，保留同名动作的最后一次调用，并定义 creep 动作的压制关系。
//!
//! 清洗器将输入绑定到调用方给定的玩家名；它不验证对象存在或所有权。
//! 数字按 parseInt 前缀规则转换，未知动作被丢弃。对象载荷采用已知字段的
//! 并集，具体动作的合法性仍由执行器检查。来源参考：上游 common/lib/system.js
//! 及 engine/src/processor/intents/creeps/intents.js。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::Value;

/// 引擎可识别的对象级动作。Creep、spawn、controller 和结构使用各自的执行器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Drop,
    Transfer,
    Withdraw,
    Pickup,
    Heal,
    RangedHeal,
    Dismantle,
    Attack,
    Harvest,
    Move,
    Repair,
    Build,
    RangedMassAttack,
    RangedAttack,
    Say,
    Suicide,
    ClaimController,
    UpgradeController,
    ReserveController,
    AttackController,
    GenerateSafeMode,
    SignController,
    Pull,
    /// 对象级通知开关，由 settle_room 在类型分派前处理。
    NotifyWhenAttacked,
    /// spawn 的创建指令，不属于 creep 执行序列。
    CreateCreep,
    RenewCreep,
    RecycleCreep,
    SetSpawnDirections,
    CancelSpawning,
    /// controller 的激活安全模式指令，不属于 creep 执行序列。
    ActivateSafeMode,
    Unclaim,
    SetPublic,
    /// 房间阶段校验并保存 terminal 发送请求，跨房资源划转在市场阶段执行。
    Send,
    /// lab 四动作（官方意图名 runReaction/reverseReaction/boostCreep/unboostCreep，
    /// system.js:37/:64/:77）；由 settle::labs 按 Generic structureType=lab 分派。
    RunReaction,
    ReverseReaction,
    BoostCreep,
    UnboostCreep,
    /// factory 的 produce（官方意图名 system.js:55，白名单
    /// {resourceType, amount} 但处理器只读 resourceType——amount 进意图
    /// 并集解析、结算忽略，与官方等价）；由 settle::factories 分派。
    Produce,
    /// nuker 的 launchNuke（官方载荷 {roomName, x, y}，system.js）；房间
    /// 阶段验证官方门并写 launchNuke 标记，跨房插入在全局阶段执行。
    LaunchNuke,
    /// powerCreep 的 renew（官方载荷 {id: 目标 powerBank/powerSpawn}）；
    /// 由 settle::powers 分派。
    Renew,
    /// powerCreep 的 enableRoom（官方载荷 {id: 控制器}）。
    EnableRoom,
    /// powerCreep 的 usePower（官方载荷 {power, id: 可选目标}）；本批仅
    /// PWR_GENERATE_OPS 结算效果，其余 power 静默不结算。
    UsePower,
    /// powerSpawn 的 processPower（官方无载荷）。
    ProcessPower,
    /// observer 的 observeRoom（官方载荷 {roomName}，system.js）；房间阶段
    /// 复验官方门后写 observeRoom 标记，下一拍投影据此授予限时视野。
    ObserveRoom,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Transfer => "transfer",
            Self::Withdraw => "withdraw",
            Self::Pickup => "pickup",
            Self::Heal => "heal",
            Self::RangedHeal => "rangedHeal",
            Self::Dismantle => "dismantle",
            Self::Attack => "attack",
            Self::Harvest => "harvest",
            Self::Move => "move",
            Self::Repair => "repair",
            Self::Build => "build",
            Self::RangedMassAttack => "rangedMassAttack",
            Self::RangedAttack => "rangedAttack",
            Self::Say => "say",
            Self::Suicide => "suicide",
            Self::ClaimController => "claimController",
            Self::UpgradeController => "upgradeController",
            Self::ReserveController => "reserveController",
            Self::AttackController => "attackController",
            Self::GenerateSafeMode => "generateSafeMode",
            Self::SignController => "signController",
            Self::Pull => "pull",
            Self::NotifyWhenAttacked => "notifyWhenAttacked",
            Self::CreateCreep => "createCreep",
            Self::RenewCreep => "renewCreep",
            Self::RecycleCreep => "recycleCreep",
            Self::SetSpawnDirections => "setSpawnDirections",
            Self::CancelSpawning => "cancelSpawning",
            Self::ActivateSafeMode => "activateSafeMode",
            Self::Unclaim => "unclaim",
            Self::SetPublic => "setPublic",
            Self::Send => "send",
            Self::RunReaction => "runReaction",
            Self::ReverseReaction => "reverseReaction",
            Self::BoostCreep => "boostCreep",
            Self::UnboostCreep => "unboostCreep",
            Self::Produce => "produce",
            Self::LaunchNuke => "launchNuke",
            Self::Renew => "renew",
            Self::EnableRoom => "enableRoom",
            Self::UsePower => "usePower",
            Self::ProcessPower => "processPower",
            Self::ObserveRoom => "observeRoom",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "drop" => Self::Drop,
            "transfer" => Self::Transfer,
            "withdraw" => Self::Withdraw,
            "pickup" => Self::Pickup,
            "heal" => Self::Heal,
            "rangedHeal" => Self::RangedHeal,
            "dismantle" => Self::Dismantle,
            "attack" => Self::Attack,
            "harvest" => Self::Harvest,
            "move" => Self::Move,
            "repair" => Self::Repair,
            "build" => Self::Build,
            "rangedMassAttack" => Self::RangedMassAttack,
            "rangedAttack" => Self::RangedAttack,
            "say" => Self::Say,
            "suicide" => Self::Suicide,
            "claimController" => Self::ClaimController,
            "upgradeController" => Self::UpgradeController,
            "reserveController" => Self::ReserveController,
            "attackController" => Self::AttackController,
            "generateSafeMode" => Self::GenerateSafeMode,
            "signController" => Self::SignController,
            "pull" => Self::Pull,
            "notifyWhenAttacked" => Self::NotifyWhenAttacked,
            "createCreep" => Self::CreateCreep,
            "renewCreep" => Self::RenewCreep,
            "recycleCreep" => Self::RecycleCreep,
            "setSpawnDirections" => Self::SetSpawnDirections,
            "cancelSpawning" => Self::CancelSpawning,
            "activateSafeMode" => Self::ActivateSafeMode,
            "unclaim" => Self::Unclaim,
            "setPublic" => Self::SetPublic,
            "send" => Self::Send,
            "runReaction" => Self::RunReaction,
            "reverseReaction" => Self::ReverseReaction,
            "boostCreep" => Self::BoostCreep,
            "unboostCreep" => Self::UnboostCreep,
            "produce" => Self::Produce,
            "launchNuke" => Self::LaunchNuke,
            "renew" => Self::Renew,
            "enableRoom" => Self::EnableRoom,
            "usePower" => Self::UsePower,
            "processPower" => Self::ProcessPower,
            "observeRoom" => Self::ObserveRoom,
            _ => return None,
        })
    }

    /// 战斗族动作对应的具名槽键；仅五个战斗动作返回 Some，派发侧据此
    /// 进入贡献槽。非战斗动作返回 None，保持各自的原生派发路径。
    pub fn combat_slot_key(self) -> Option<&'static str> {
        Some(match self {
            Self::Attack => "builtin:creep.attack",
            Self::RangedAttack => "builtin:creep.rangedAttack",
            Self::RangedMassAttack => "builtin:creep.rangedMassAttack",
            Self::Heal => "builtin:creep.heal",
            Self::RangedHeal => "builtin:creep.rangedHeal",
            _ => return None,
        })
    }

    /// 建造族动作对应的具名槽键；仅 build/repair/dismantle 返回 Some，
    /// 派发侧据此进入贡献槽，与战斗族同型。
    pub fn build_slot_key(self) -> Option<&'static str> {
        Some(match self {
            Self::Build => "builtin:creep.build",
            Self::Repair => "builtin:creep.repair",
            Self::Dismantle => "builtin:creep.dismantle",
            _ => return None,
        })
    }

    /// 返回会压制本动作的指令。判断只看指令是否存在，不看其执行是否成功。
    /// 来源：上游 engine/src/processor/intents/creeps/intents.js 的 priorities。
    fn suppressed_by(self) -> &'static [Action] {
        match self {
            Self::RangedHeal => &[Self::Heal],
            Self::AttackController => &[Self::RangedHeal, Self::Heal],
            Self::Dismantle => &[Self::AttackController, Self::RangedHeal, Self::Heal],
            Self::Repair => &[
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::Build => &[
                Self::Repair,
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::Attack => &[
                Self::Build,
                Self::Repair,
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::Harvest => &[
                Self::Attack,
                Self::Build,
                Self::Repair,
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::RangedMassAttack => &[Self::Build, Self::Repair, Self::RangedHeal],
            Self::RangedAttack => &[
                Self::RangedMassAttack,
                Self::Build,
                Self::Repair,
                Self::RangedHeal,
            ],
            _ => &[],
        }
    }

    /// 动作存在，且没有压制它的指令时才执行。
    /// 玩家 API 返回 OK 只表示指令已记录，不保证最终被执行。
    pub fn executes(present: &BTreeSet<Action>, action: Action) -> bool {
        present.contains(&action) && !action.suppressed_by().iter().any(|a| present.contains(a))
    }
}

/// Creep 动作执行顺序。类型专属动作另由对应处理器分派。
/// 来源：上游 engine/src/processor/intents/creeps/intents.js 的 creepActions。
pub const EXECUTION_ORDER: [Action; 23] = [
    Action::Drop,
    Action::Transfer,
    Action::Withdraw,
    Action::Pickup,
    Action::Heal,
    Action::RangedHeal,
    Action::Dismantle,
    Action::Attack,
    Action::Harvest,
    Action::Move,
    Action::Repair,
    Action::Build,
    Action::RangedMassAttack,
    Action::RangedAttack,
    Action::Say,
    Action::Suicide,
    Action::ClaimController,
    Action::UpgradeController,
    Action::ReserveController,
    Action::AttackController,
    Action::GenerateSafeMode,
    Action::SignController,
    Action::Pull,
];

/// 对象指令载荷。字段为已识别动作的并集，执行器仅读取对应动作所需的字段。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentData {
    /// 目标对象 ID；move 可用相邻 creep 的位置确定方向。
    pub id: Option<ObjId>,
    pub direction: Option<u8>,
    pub amount: Option<i64>,
    pub resource_type: Option<String>,
    pub x: Option<i64>,
    pub y: Option<i64>,
    /// 创建 creep 时的名称。
    pub name: Option<String>,
    /// say 文本，最多 100 个 Unicode 字符；缺失时执行器使用空串。
    pub message: Option<String>,
    /// say 或 setPublic 的公开标志，按 JavaScript 真值规则转换。
    pub is_public: Option<bool>,
    /// notifyWhenAttacked 的开关，按 JavaScript 真值规则转换。
    pub enabled: Option<bool>,
    /// 控制器签名，最多 100 个 Unicode 字符；缺失与空串均表示清除签名。
    /// 截断单位不同于上游 userString 的 UTF-16 码元。
    pub sign: Option<String>,
    pub body: Vec<BodyKind>,
    /// None 表示使用房间内默认能量结构；Some(空列表) 表示显式列表，
    /// 可用能量为 0。不能把空列表改为缺省值。
    pub energy_structures: Option<Vec<ObjId>>,
    /// 方向列表：逐项按 parseInt 转换，再保序去重；无效值用 i64::MIN 保留。
    /// 执行器校验 1..=8，发现无效值时拒绝整条指令，不能在此过滤掉。
    pub directions: Vec<i64>,
    /// terminal.send 的目标房名，只接收 JSON 字符串。
    pub target_room_name: Option<String>,
    /// terminal.send 的描述，最多 100 个 Unicode 字符。
    pub description: Option<String>,
    /// lab runReaction/reverseReaction 的两个试剂/输出 lab（官方载荷键
    /// lab1/lab2，system.js:64）；调用方 lab 是意图对象本身。
    pub lab1: Option<ObjId>,
    pub lab2: Option<ObjId>,
    /// lab boostCreep 的部件数上限（官方载荷键 bodyPartsCount，system.js:37）；
    /// 缺失或 0 表示不限制。复用 amount 会与官方字段名脱钩，故独立存放。
    pub body_parts_count: Option<i64>,
    /// usePower 的 power 常量 ID（官方载荷键 power，system.js）。
    pub power: Option<i64>,
}

impl IntentData {
    pub fn target(id: ObjId) -> Self {
        Self {
            id: Some(id),
            direction: None,
            amount: None,
            resource_type: None,
            x: None,
            y: None,
            name: None,
            message: None,
            is_public: None,
            enabled: None,
            sign: None,
            body: Vec::new(),
            energy_structures: None,
            directions: Vec::new(),
            target_room_name: None,
            description: None,
            lab1: None,
            lab2: None,
            body_parts_count: None,
            power: None,
        }
    }

    pub fn bare() -> Self {
        Self {
            id: None,
            direction: None,
            amount: None,
            resource_type: None,
            x: None,
            y: None,
            name: None,
            message: None,
            is_public: None,
            enabled: None,
            sign: None,
            body: Vec::new(),
            energy_structures: None,
            directions: Vec::new(),
            target_room_name: None,
            description: None,
            lab1: None,
            lab2: None,
            body_parts_count: None,
            power: None,
        }
    }
}

/// 房间级指令（官方挂在 objects['room'] 下，数组形态）。
#[derive(Debug, Clone, PartialEq)]
pub enum RoomIntent {
    CreateConstructionSite {
        room: String,
        x: i64,
        y: i64,
        structure_type: String,
        name: Option<String>,
    },
    CreateFlag {
        room: String,
        x: i64,
        y: i64,
        name: String,
        color: i64,
        secondary_color: i64,
    },
    DestroyStructure {
        room: String,
        id: ObjId,
    },
    /// flag.setPosition/setColor 也通过删除和重建旗帜实现。
    RemoveFlag {
        room: String,
        name: String,
    },
    /// 删除工地，结算器检查所有权并退还 progress/2 的掉落能量。
    RemoveConstructionSite {
        room: String,
        id: ObjId,
    },
}

/// 单玩家的对象、房间、通知与市场指令。对象动作同名覆盖，列表指令保留顺序。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserIntents {
    pub objects: BTreeMap<ObjId, BTreeMap<Action, IntentData>>,
    pub room: Vec<RoomIntent>,
    /// Game.notify 的全局列表，每条在 tick 发布时记录一次。
    pub notify: Vec<NotifyIntent>,
    /// Game.market 的全局列表，由市场阶段按动作类别处理。
    pub market: Vec<MarketIntent>,
    /// power 账号域全局列表（create/rename/delete/upgrade/spawn/suicide
    /// PowerCreep），由全局 power 阶段处理。
    pub power: Vec<PowerIntent>,
    /// 扩展意图：对象目标按 ObjId 分组，动作键为冻结计划里的意图稳定键；
    /// 同一对象同一意图后发覆盖先发（合并模式的队列形态尚未接线）。
    /// args 保留原始 JSON，参数 schema、接收者与所有权由结算侧按计划校验。
    pub ext: BTreeMap<ObjId, BTreeMap<kk_mod_api::Key, Value>>,
}

/// 一条玩家通知意图（Game.notify）。
#[derive(Debug, Clone, PartialEq)]
pub struct NotifyIntent {
    pub message: String,
    pub group_interval: Option<i64>,
}

/// 市场动作及其清洗后的载荷。价格以千分之一 credit 表示。
///
/// 缺失或无法转换的字段保留为 None，由市场处理器拒绝或忽略。
/// CreateOrder 的 order_type=None 必须拒绝，不能默认为 buy。上游
/// common/lib/system.js 会把缺失字符串物化为 "undefined"，并把无效数字
/// 转成 NaN；这里用 Option 表示缺失，不能据此认为内部格式完全一致。
#[derive(Debug, Clone, PartialEq)]
pub enum MarketIntent {
    CreateOrder {
        /// 仅接受 buy 或 sell，None 由处理器拒绝。
        order_type: Option<String>,
        resource_type: Option<String>,
        /// 价格，单位千分之一 credit。
        price_milli: Option<i64>,
        total_amount: Option<i64>,
        room_name: Option<String>,
    },
    CancelOrder {
        order_id: Option<String>,
    },
    ChangeOrderPrice {
        order_id: Option<String>,
        new_price_milli: Option<i64>,
    },
    ExtendOrder {
        order_id: Option<String>,
        add_amount: Option<i64>,
    },
    Deal {
        order_id: Option<String>,
        amount: Option<i64>,
        target_room_name: Option<String>,
    },
}

/// power 账号域全局动作（官方 global 容器，processor/global-intents/power）。
/// id 均指 powerCreep 对象 ID；spawnCooldownTime/deleteTime 是墙钟毫秒。
#[derive(Debug, Clone, PartialEq)]
pub enum PowerIntent {
    CreatePowerCreep { name: String, class_name: String },
    RenamePowerCreep { id: ObjId, name: String },
    DeletePowerCreep { id: ObjId, cancel: bool },
    UpgradePowerCreep { id: ObjId, power: i64 },
    SpawnPowerCreep { power_spawn: ObjId, name: String },
    SuicidePowerCreep { id: ObjId },
}

/// 本轮各玩家的指令，按玩家名字排序以固定结算顺序。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntentBatch {
    pub users: BTreeMap<String, UserIntents>,
}

/// 合计已解析身体部件的成本。
pub fn creep_cost(body: &[BodyKind]) -> i64 {
    body.iter()
        .map(|p| {
            let name = p.name();
            c::BODYPART_COST
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| *v)
                .unwrap_or(0)
        })
        .sum()
}
mod from_json;
pub use from_json::sanitize_object_intent;
mod to_json;

#[cfg(test)]
mod from_json_tests;

#[cfg(test)]
mod market_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod to_json_tests;
