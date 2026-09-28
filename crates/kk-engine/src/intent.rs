//! 玩家指令（intent）模型：类型、白名单清洗、同对象冲突裁决。
//!
//! 官方语义：
//! - 字段白名单与 transforms：`common/lib/system.js`（number=parseInt 截断、
//!   bodypart[] 过滤非法部件、userString 截 100、未知字段丢弃）；
//! - 同对象同名指令后者覆盖前者（game-loop.md"同一种指令只记最后一次"）；
//! - 冲突裁决优先级图：`engine/src/processor/intents/creeps/intents.js:3-13`
//!   （动作执行当且仅当其 intent 存在且无更高优先级 intent 存在——被压制
//!   的动作不执行，玩家 API 早已返回成功）；
//! - 执行顺序：creepActions 数组（intents.js:15-17）。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::Value;

/// 动作名（官方 creepActions 全集 + spawn 的 createCreep）。
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
    /// spawn 对象的动作（官方 spawns/intents.js 的 M1 子集）。
    CreateCreep,
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
            Self::CreateCreep => "createCreep",
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
            _ => return None,
        })
    }

    /// 官方 priorities 图（intents.js:3-13）：返回能压制本动作的高优先级动作集。
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

    /// checkPriorities（intents.js:21-23）：动作存在且未被更高优先级动作压制。
    pub fn executes(present: &BTreeSet<Action>, action: Action) -> bool {
        present.contains(&action) && !action.suppressed_by().iter().any(|a| present.contains(a))
    }
}

/// 官方执行顺序（creepActions，intents.js:15-17）。CreateCreep 是 spawn
/// 对象的动作（spawns/intents.js），不在 creep 序列里、由 spawn 分派单独执行。
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

/// 单条对象级指令的载荷（M1 所需字段；字段名对齐官方白名单）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentData {
    /// move 的 direction / 各动作的 target id（官方字段 id）。
    pub id: Option<ObjId>,
    pub direction: Option<u8>,
    pub amount: Option<i64>,
    pub resource_type: Option<String>,
    pub x: Option<i64>,
    pub y: Option<i64>,
    /// createCreep 专属（挂在 spawn 对象上）。
    pub name: Option<String>,
    pub body: Vec<BodyKind>,
    pub energy_structures: Vec<ObjId>,
    /// 官方链路（system.js number[] transform → create-creep.js:27-33）：本字段
    /// 只承载 parseInt 截断 + 保序去重后的值；无法 parseInt 的元素以 i64::MIN
    /// 哨兵传递（1..=8 校验在 createCreep 整单拒绝，非过滤）。
    pub directions: Vec<i64>,
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
            body: Vec::new(),
            energy_structures: Vec::new(),
            directions: Vec::new(),
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
            body: Vec::new(),
            energy_structures: Vec::new(),
            directions: Vec::new(),
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
}

/// 单玩家的指令集：对象级（对象 → 动作 → 载荷，同名覆盖）+ 房间级列表。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserIntents {
    pub objects: BTreeMap<ObjId, BTreeMap<Action, IntentData>>,
    pub room: Vec<RoomIntent>,
}

/// 一 tick 的全部玩家指令（玩家名排序——结算序确定性，方案 D5）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntentBatch {
    pub users: BTreeMap<String, UserIntents>,
}

impl IntentBatch {
    pub fn add_object_intent(
        &mut self,
        player: &str,
        obj: ObjId,
        action: Action,
        data: IntentData,
    ) {
        self.users
            .entry(player.to_string())
            .or_default()
            .objects
            .entry(obj)
            .or_default()
            .insert(action, data);
    }

    pub fn add_room_intent(&mut self, player: &str, intent: RoomIntent) {
        self.users
            .entry(player.to_string())
            .or_default()
            .room
            .push(intent);
    }
}

/// 官方 transforms 的 number 语义（system.js:9）：`parseInt` 截断。
fn to_int(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => Some(n.as_f64().unwrap_or(0.0).trunc() as i64),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|f| f.trunc() as i64),
        _ => None,
    }
}

fn to_id(v: Option<&Value>) -> Option<ObjId> {
    v.and_then(Value::as_str).and_then(ObjId::parse)
}

/// 官方 userString（system.js:13）：`("" + (value||"")).substring(0, 100)`。
fn to_user_string(v: Option<&Value>) -> Option<String> {
    v.map(|x| match x {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
    .map(|s| s.chars().take(100).collect())
}

/// 从官方 intent JSON 白名单清洗（system.js intentTypes 对应项）。
/// 未知动作名返回 None；字段缺失按官方 transforms 得 undefined（Rust 侧 None）。
pub fn sanitize_object_intent(_action: Action, raw: &Value) -> IntentData {
    let f = |k: &str| raw.get(k);
    IntentData {
        id: to_id(f("id")),
        direction: to_int(f("direction")).map(|d| d.clamp(0, 255) as u8),
        amount: to_int(f("amount")),
        resource_type: f("resourceType")
            .and_then(Value::as_str)
            .map(str::to_string),
        x: to_int(f("x")),
        y: to_int(f("y")),
        name: to_user_string(f("name")),
        body: f("body")
            .and_then(Value::as_array)
            .map(|arr| {
                // bodypart[]：过滤非白名单部件（system.js:11）
                arr.iter()
                    .filter_map(|p| p.as_str().and_then(BodyKind::parse))
                    .collect()
            })
            .unwrap_or_default(),
        energy_structures: f("energyStructures")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.as_str().and_then(ObjId::parse))
                    .collect()
            })
            .unwrap_or_default(),
        directions: f("directions")
            .and_then(Value::as_array)
            .map(|arr| {
                // system.js number[] transform：_.map(parseInt)（2.5→2、数字串转数；
                // null/未定义经 transform 已折为 undefined ⇒ 本分支不可达），
                // 随后 create-creep.js:29 _.uniq 保序去重
                let mut ds: Vec<i64> = arr
                    .iter()
                    .map(|p| to_int(Some(p)).unwrap_or(i64::MIN))
                    .collect();
                ds.dedup();
                ds
            })
            .unwrap_or_default(),
    }
}

/// 官方 calcCreepCost（utils.js）：部件成本合计（部件已过白名单过滤）。
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn priority_graph_matches_official() {
        // intents.js:3-13 抽样：harvest 被 attack..heal 全家压制；move 无压制
        let mut present = BTreeSet::new();
        present.insert(Action::Harvest);
        assert!(Action::executes(&present, Action::Harvest));
        present.insert(Action::Heal);
        assert!(
            !Action::executes(&present, Action::Harvest),
            "heal 压制 harvest"
        );
        assert!(Action::executes(&present, Action::Heal));
        // repair 压制 build（官方 priorities：build 的压制表含 repair）；attack 压制 build
        let p2 = BTreeSet::from([Action::Build, Action::Repair]);
        assert!(!Action::executes(&p2, Action::Build));
        assert!(Action::executes(&p2, Action::Repair));
        let p3 = BTreeSet::from([Action::Attack, Action::Build]);
        assert!(
            !Action::executes(&p3, Action::Attack),
            "build 在 attack 的压制表里"
        );
        assert!(Action::executes(&p3, Action::Build));
        // 执行顺序：drop 最先、move 在 harvest 后
        assert_eq!(EXECUTION_ORDER[0], Action::Drop);
        assert!(
            EXECUTION_ORDER.iter().position(|a| *a == Action::Harvest)
                < EXECUTION_ORDER.iter().position(|a| *a == Action::Move)
        );
    }

    #[test]
    fn sanitize_translates_official_transforms() {
        let raw = json!({
            "id": "kk0000000000000001",
            "amount": 12.7,
            "resourceType": "energy",
            "unknown": "dropped",
        });
        let d = sanitize_object_intent(Action::Transfer, &raw);
        assert_eq!(d.id, Some(ObjId(1)));
        assert_eq!(d.amount, Some(12), "parseInt 截断");
        assert_eq!(d.resource_type.as_deref(), Some("energy"));
        // bodypart[] 过滤
        let raw2 = json!({ "body": ["work", "fly", "move"] });
        let d2 = sanitize_object_intent(Action::Suicide, &raw2);
        assert_eq!(d2.body, vec![BodyKind::Work, BodyKind::Move]);
        assert_eq!(creep_cost(&d2.body), 150);
        // userString 截 100
        let long = "x".repeat(150);
        let raw3 = json!({ "name": long });
        let d3 = sanitize_object_intent(Action::Say, &raw3);
        assert_eq!(d3.name.unwrap().chars().count(), 100);
        // directions：parseInt 截断（2.5→2）+ 保序去重；越界值原样传递——
        // 整单拒绝发生在 createCreep（1..=8 校验），无法 parseInt 的元素以
        // i64::MIN 哨兵传递（天然落入 1..=8 之外）
        let raw4 = json!({ "directions": [1, 9, 3, 3, 2.5] });
        let d4 = sanitize_object_intent(Action::Suicide, &raw4);
        assert_eq!(d4.directions, vec![1, 9, 3, 2]);
        let raw5 = json!({ "directions": [1, "abc"] });
        let d5 = sanitize_object_intent(Action::Suicide, &raw5);
        assert_eq!(d5.directions, vec![1, i64::MIN]);
    }
}
