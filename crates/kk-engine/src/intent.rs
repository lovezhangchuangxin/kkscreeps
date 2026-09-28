//! 玩家指令（intent）模型：输入清洗、同对象覆盖、冲突裁决。
//!
//! 官方语义（本地官方源码 ~/Desktop/projects/sources/screeps/）：
//! - 字段白名单与 transforms：`common/lib/system.js`——number 字段一律按
//!   parseInt 截断（见 [`to_int`]）、bodypart[] 过滤非白名单部件、userString
//!   截 100、白名单外的未知字段丢弃；
//! - 同对象同名指令后者覆盖前者（只记最后一次）；
//! - 冲突裁决优先级图：`engine/src/processor/intents/creeps/intents.js`——
//!   动作执行当且仅当其 intent 存在**且**无更高优先级 intent 存在。被压制的
//!   动作不执行，但玩家 API 早已返回成功——这是官方语义，不是缺陷；
//! - 执行顺序：creepActions 数组序（intents.js:15-17）。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::Value;

/// 动作名（官方 creepActions 全集 + spawn 的 createCreep + controller 的
/// activateSafeMode）。
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
    /// spawn 对象的动作（官方 spawns/intents.js:3-24 的分派序）。
    CreateCreep,
    /// renewCreep（白名单 `{id: 'string'}`，system.js:62；结算链见
    /// settle/spawn.rs intent_renew_creep——对照 spawns/renew-creep.js）。
    RenewCreep,
    /// recycleCreep（白名单 `{id: 'string'}`，system.js:60；
    /// spawns/recycle-creep.js）。
    RecycleCreep,
    /// setSpawnDirections（白名单 `{directions: 'number[]'}`，system.js:73；
    /// spawns/set-spawn-directions.js）。
    SetSpawnDirections,
    /// cancelSpawning（空白名单 `{}`，system.js:39；
    /// spawns/cancel-spawning.js）。
    CancelSpawning,
    /// controller 对象的动作（官方 processor.js:306-311 分派，intent 白名单
    /// `activateSafeMode: {}`——system.js:34；语义链见 settle/controller.rs）。
    /// 与 CreateCreep 同理不在 creepActions 序列（EXECUTION_ORDER）里。
    ActivateSafeMode,
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
            Self::RenewCreep => "renewCreep",
            Self::RecycleCreep => "recycleCreep",
            Self::SetSpawnDirections => "setSpawnDirections",
            Self::CancelSpawning => "cancelSpawning",
            Self::ActivateSafeMode => "activateSafeMode",
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
            "createCreep" => Self::CreateCreep,
            "renewCreep" => Self::RenewCreep,
            "recycleCreep" => Self::RecycleCreep,
            "setSpawnDirections" => Self::SetSpawnDirections,
            "cancelSpawning" => Self::CancelSpawning,
            "activateSafeMode" => Self::ActivateSafeMode,
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

/// 官方执行顺序（creepActions，intents.js:15-17）。CreateCreep/RenewCreep/
/// RecycleCreep/SetSpawnDirections/CancelSpawning 是 spawn 对象的动作
/// （spawns/intents.js:3-24 按 createCreep→renewCreep→recycleCreep→
/// setSpawnDirections→cancelSpawning 的序分派），不在 creep 序列里、由
/// settle/spawn.rs 的 spawn 分派单独执行。
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
    /// signController 专属：官方白名单 `sign: 'userString'`（system.js:74），
    /// 截 100（UTF-16 码元 vs Unicode chars 的差异沿用既有灰区登记）。
    /// 官方形态缺省与空串同值（userString 的 `value||""` 折叠），signController
    /// 结算层 `intent.sign ?` 判空——两者都落 None 语义（清除签名）。
    pub sign: Option<String>,
    pub body: Vec<BodyKind>,
    /// None = 玩家未传（缺省全房扣费）；Some([]) = 显式空列表（官方走
    /// "仅列表扣费"模式——可用 0 → 失败）。presence 由 JSON 键存在性区分
    /// （官方 system.js transforms 不区分，语义分叉在 create-creep.js:19-24）。
    pub energy_structures: Option<Vec<ObjId>>,
    /// 官方链路分两步：system.js 的 number[] transform 逐元素 parseInt（在此
    /// 完成，无法解析的元素以 i64::MIN 哨兵传递）；create-creep.js:27 再
    /// _.uniq 保序全局去重。1..=8 校验在 createCreep 侧整单拒绝（官方语义是
    /// 拒绝整条指令，不是过滤元素）。
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
            sign: None,
            body: Vec::new(),
            energy_structures: None,
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
            sign: None,
            body: Vec::new(),
            energy_structures: None,
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

/// room 级 intent 参数解析（官方 system.js:116-140 sanitizeUserRoomIntents 的
/// 字段面 + room/intents.js 消费字段；坐标/颜色走 parseInt，名称走 userString）。
fn parse_room_intent(name: &str, raw: &Value) -> Option<RoomIntent> {
    let f = |k: &str| raw.get(k);
    let room = f("roomName").and_then(Value::as_str)?.to_string();
    match name {
        "createConstructionSite" => Some(RoomIntent::CreateConstructionSite {
            room,
            x: to_int(f("x"))?,
            y: to_int(f("y"))?,
            structure_type: f("structureType").and_then(Value::as_str)?.to_string(),
            name: to_user_string(f("name")),
        }),
        "createFlag" => Some(RoomIntent::CreateFlag {
            room,
            x: to_int(f("x"))?,
            y: to_int(f("y"))?,
            name: to_user_string(f("name"))?,
            color: to_int(f("color")).unwrap_or(0),
            secondary_color: to_int(f("secondaryColor")).unwrap_or(0),
        }),
        _ => None,
    }
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
    /// 沙箱传出的 intentsList（官方形态：`{对象id: {动作名: 参数}}`，room 级
    /// 挂 'room' 键数组）→ 清洗后的 IntentBatch。**生产清洗入口**（M2A-A2
    /// 接线）：对照官方链路——沙箱传出原始 list（runtime.js:271-296），宿主
    /// 进程逐字段白名单清洗（utils.js:405-436 storeIntents → system.js）。
    /// 不可信输入纪律：未知动作名丢弃（sanitize 白名单语义）、对象 id 不可
    /// 解析丢弃（官方 utils.js:427-429 同款）、参数只保留白名单字段。
    /// 身份边界：全部归 `player`（调用方保证 = 沙箱注册键；settle 侧另有
    /// 结构性复核）。
    pub fn from_json(player: &str, list: &Value) -> IntentBatch {
        let mut batch = IntentBatch::default();
        let Some(objects) = list.as_object() else {
            return batch;
        };
        for (obj_key, actions) in objects {
            if obj_key == "room" {
                // room 级：pushByName('room', name, data) 的数组形态
                // （runtime.js:80-90；room/intents.js 逐条消费）
                let Some(actions) = actions.as_object() else {
                    continue;
                };
                for (name, arr) in actions {
                    let Some(items) = arr.as_array() else {
                        continue;
                    };
                    for item in items {
                        if let Some(intent) = parse_room_intent(name, item) {
                            batch.add_room_intent(player, intent);
                        }
                    }
                }
                continue;
            }
            let Some(obj) = ObjId::parse(obj_key) else {
                continue; // 官方 utils.js:427-429：反查不到的对象丢弃
            };
            let Some(actions) = actions.as_object() else {
                continue;
            };
            for (name, params) in actions {
                let Some(action) = Action::parse(name) else {
                    continue; // 未知动作名：sanitizeUserIntents 白名单外丢弃
                };
                let data = sanitize_object_intent(action, params);
                batch.add_object_intent(player, obj, action, data);
            }
        }
        batch
    }

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

    /// 合并另一批（同玩家键合并 objects/room；后写覆盖同名动作——幂等语义）。
    pub fn extend_from(&mut self, other: IntentBatch) {
        for (player, intents) in other.users {
            let entry = self.users.entry(player).or_default();
            entry.objects.extend(intents.objects);
            entry.room.extend(intents.room);
        }
    }

    pub fn add_room_intent(&mut self, player: &str, intent: RoomIntent) {
        self.users
            .entry(player.to_string())
            .or_default()
            .room
            .push(intent);
    }
}

/// 按 JS `parseInt` 的前缀规则读取 JSON 数字或字符串；超出 i64 的值无效。
/// `number[]` 也使用这条规则，调用方保留无效值供后续校验。
fn to_int(v: Option<&Value>) -> Option<i64> {
    let text = match v? {
        Value::Number(n) => {
            let value = n.as_f64()?;
            if !value.is_finite() {
                return None;
            }
            let magnitude = value.abs();
            // 非零 JS Number 在这个区间外使用指数形式；parseInt 只读首位数字。
            if magnitude < 1e-6 || magnitude >= 1e21 {
                let first = format!("{magnitude:e}").as_bytes()[0] - b'0';
                return Some(if value.is_sign_negative() {
                    -(first as i64)
                } else {
                    first as i64
                });
            }
            let integer = value.trunc();
            if !(i64::MIN as f64..9_223_372_036_854_775_808.0).contains(&integer) {
                return None;
            }
            return Some(integer as i64);
        }
        Value::String(s) => s.clone(),
        _ => return None,
    };
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (radix, digits) = if digits.starts_with("0x") || digits.starts_with("0X") {
        (16, &digits[2..])
    } else {
        (10, digits)
    };
    let prefix = digits
        .bytes()
        .take_while(|b| (*b as char).is_digit(radix))
        .count();
    if prefix == 0 {
        return None;
    }
    let value = i128::from_str_radix(&digits[..prefix], radix).ok()?;
    i64::try_from(if negative { -value } else { value }).ok()
}

fn to_id(v: Option<&Value>) -> Option<ObjId> {
    v.and_then(Value::as_str).and_then(ObjId::parse)
}

/// 官方 userString（system.js:12）：`("" + (value||"")).substring(0, 100)`。
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
        sign: to_user_string(f("sign")),
        body: f("body")
            .and_then(Value::as_array)
            .map(|arr| {
                // bodypart[]：过滤非白名单部件（system.js:11）
                arr.iter()
                    .filter_map(|p| p.as_str().and_then(BodyKind::parse))
                    .collect()
            })
            .unwrap_or_default(),
        energy_structures: f("energyStructures").and_then(Value::as_array).map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str().and_then(ObjId::parse))
                .collect()
        }),
        directions: f("directions")
            .and_then(Value::as_array)
            .map(|arr| {
                // 无法解析的值仍须进入后续校验，不能静默过滤。
                let mut seen = BTreeSet::new();
                arr.iter()
                    .map(|p| to_int(Some(p)).unwrap_or(i64::MIN))
                    .filter(|d| seen.insert(*d))
                    .collect()
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
mod from_json_tests {
    use super::*;

    /// 生产清洗入口的对抗面：伪造对象 id / 未知动作名 / 畸形参数 / 'room'
    /// 键伪造（数组形态检查）——全部 fail-closed 丢弃，白名单内条目存活。
    #[test]
    fn from_json_adversarial_inputs() {
        let list = serde_json::json!({
            "kk0000000000000001": { "move": { "direction": 3 }, "explode": {}, "5": {} },
            "not-an-id": { "move": { "direction": 3 } },
            "room": "forged-non-object",
            "kk0000000000000002": 7,
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        assert_eq!(user.objects.len(), 1, "只有合法对象键存活");
        let obj = kk_world::ObjId::parse("kk0000000000000001").unwrap();
        let actions = &user.objects[&obj];
        assert_eq!(actions.len(), 1, "未知动作名 'explode' 丢弃");
        assert_eq!(actions[&Action::Move].direction, Some(3));
        assert!(user.room.is_empty(), "'room' 键非对象丢弃");
    }

    /// createCreep 经通道往返（评审 F1：parse 曾缺此臂——静默黑洞）+ presence：
    /// 缺键 None / 空数组 Some([])（显式列表模式）。
    #[test]
    fn from_json_create_creep_roundtrip_and_presence() {
        let list = serde_json::json!({
            "kk0000000000000003": {
                "createCreep": { "body": ["move", "work"], "name": "c1", "energyStructures": [] }
            },
            "kk0000000000000004": {
                "createCreep": { "body": ["move"], "name": "c2" }
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        let s1 = kk_world::ObjId::parse("kk0000000000000003").unwrap();
        let s2 = kk_world::ObjId::parse("kk0000000000000004").unwrap();
        assert_eq!(
            user.objects[&s1][&Action::CreateCreep].energy_structures,
            Some(vec![]),
            "空数组保持 Some（显式列表模式）"
        );
        assert_eq!(
            user.objects[&s2][&Action::CreateCreep].energy_structures,
            None,
            "缺键读 None（缺省全房扣费）"
        );
        assert_eq!(user.objects[&s1][&Action::CreateCreep].body.len(), 2);
    }

    /// room 级数组通道：createFlag/createConstructionSite 解析 + 未知名丢弃。
    #[test]
    fn from_json_room_intents() {
        let list = serde_json::json!({
            "room": {
                "createFlag": [
                    { "roomName": "W1N1", "x": 10, "y": 11, "name": "f1", "color": 1, "secondaryColor": 1 },
                    { "roomName": "W1N1", "x": 12, "y": 13, "name": "f2" }
                ],
                "teleport": [{ "roomName": "W1N1", "x": 0, "y": 0 }]
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        assert_eq!(batch.users["p1"].room.len(), 2, "未知 room intent 名丢弃");
    }

    /// A4-i spawn 族白名单（system.js:39/60/62/73）：renewCreep/recycleCreep
    /// 只认 id、setSpawnDirections 的 number[]、cancelSpawning 空载荷；四者
    /// 都不在 EXECUTION_ORDER（spawns/intents.js 的 spawn 分派序）里。
    #[test]
    fn from_json_spawn_family_whitelist() {
        let spawn = "kk0000000000000021";
        let creep = "kk0000000000000022";
        let list = serde_json::json!({
            spawn: {
                "renewCreep": { "id": creep, "forged": 1 },
                "recycleCreep": { "id": creep },
                "setSpawnDirections": { "directions": [3, 5, "7tail", 3] },
                "cancelSpawning": { "forged": "field" }
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        let spawn_id = ObjId::parse(spawn).unwrap();
        let creep_id = ObjId::parse(creep).unwrap();
        let actions = &user.objects[&spawn_id];
        assert_eq!(actions.len(), 4);
        assert_eq!(
            actions[&Action::RenewCreep].id,
            Some(creep_id),
            "renewCreep 只留 id"
        );
        assert_eq!(
            actions[&Action::RecycleCreep].id,
            Some(creep_id),
            "recycleCreep 只留 id"
        );
        assert_eq!(
            actions[&Action::SetSpawnDirections].directions,
            vec![3, 5, 7],
            "number[]：parseInt 截断（\"7tail\"→7）+ 保序去重"
        );
        let cancel = &actions[&Action::CancelSpawning];
        assert_eq!(cancel.id, None, "cancelSpawning 空白名单——无字段");
        // 名字与 parse 往返 + 不在 creep 执行序
        for (action, name) in [
            (Action::RenewCreep, "renewCreep"),
            (Action::RecycleCreep, "recycleCreep"),
            (Action::SetSpawnDirections, "setSpawnDirections"),
            (Action::CancelSpawning, "cancelSpawning"),
        ] {
            assert_eq!(Action::parse(name), Some(action));
            assert_eq!(action.name(), name);
            assert!(
                !EXECUTION_ORDER.contains(&action),
                "{name} 不在 creepActions 序列（spawns/intents.js 的 spawn 分派序）"
            );
        }
    }

    /// B2 controller 族白名单（system.js:34/36/40/46/67/74）：五个 creep 动作
    /// 只认 id；signController 另有 sign（userString 截 100）；activateSafeMode
    /// 无字段。白名单外的未知字段丢弃。
    #[test]
    fn from_json_controller_family_whitelist() {
        let creep = "kk0000000000000009";
        let ctl = "kk0000000000000010";
        let list = serde_json::json!({
            creep: {
                "claimController": { "id": ctl, "unknown": 1 },
                "reserveController": { "id": ctl },
                "attackController": { "id": ctl },
                "generateSafeMode": { "id": ctl },
                "signController": { "id": ctl, "sign": "x".repeat(150) }
            },
            ctl: { "activateSafeMode": { "forged": "field" } }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        let creep = ObjId::parse(creep).unwrap();
        let ctl_id = ObjId::parse(ctl).unwrap();
        for action in [
            Action::ClaimController,
            Action::ReserveController,
            Action::AttackController,
            Action::GenerateSafeMode,
        ] {
            assert_eq!(
                user.objects[&creep][&action].id,
                Some(ctl_id),
                "{:?} 白名单只留 id",
                action.name()
            );
        }
        let sign = &user.objects[&creep][&Action::SignController];
        assert_eq!(sign.id, Some(ctl_id));
        assert_eq!(
            sign.sign.as_ref().map(|s| s.chars().count()),
            Some(100),
            "sign 走 userString 截 100"
        );
        // controller 对象的 activateSafeMode（system.js:34 空白名单——无字段）
        let activate = &user.objects[&ctl_id][&Action::ActivateSafeMode];
        assert_eq!(activate.id, None, "activateSafeMode 无字段");
        assert_eq!(
            Action::parse("activateSafeMode"),
            Some(Action::ActivateSafeMode)
        );
        assert_eq!(Action::ActivateSafeMode.name(), "activateSafeMode");
        // 缺省 → None / 显式空串 → Some("")：清洗层保留 presence 区分，
        // 语义分叉在 signController 结算层（两者都落"清除签名"）
        let d = sanitize_object_intent(Action::SignController, &serde_json::json!({ "id": ctl }));
        assert_eq!(d.sign, None, "缺省 sign → None");
        let d2 = sanitize_object_intent(Action::SignController, &serde_json::json!({ "sign": "" }));
        assert_eq!(d2.sign, Some(String::new()), "显式空串保留 Some(\"\")");
    }
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
        let raw6 = json!({ "directions": [1, 2, 1, "3tail", "0x2"] });
        let d6 = sanitize_object_intent(Action::CreateCreep, &raw6);
        assert_eq!(d6.directions, vec![1, 2, 3]);
        assert_eq!(to_int(Some(&json!("1e2"))), Some(1));
        assert_eq!(to_int(Some(&json!(-12.7))), Some(-12));
        assert_eq!(to_int(Some(&json!("-9223372036854775808"))), Some(i64::MIN));
        assert_eq!(to_int(Some(&json!(1e-6))), Some(0));
        assert_eq!(to_int(Some(&json!(1e-7))), Some(1));
        assert_eq!(to_int(Some(&json!(-2e-7))), Some(-2));
        assert_eq!(to_int(Some(&json!(1e21))), Some(1));
        assert_eq!(
            sanitize_object_intent(Action::CreateCreep, &json!({ "directions": [1e-6] }))
                .directions,
            vec![0]
        );
    }
}
