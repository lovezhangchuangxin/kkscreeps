//! 战斗策略的表示边界；公式、距离和目标选择由调用策略决定。

use serde::Serialize;

/// 单次策略伤害的非负整数上限。JS 精确表示且低于生命值 u32 域；
/// 不是原生 ATTACK/RANGED_ATTACK 功率或部件规则。
pub const MAX_DAMAGE: i64 = 1_000_000_000;

/// 单次策略治疗的非负整数上限；与伤害相同的表示域，不是 HEAL 功率。
pub const MAX_HEAL: i64 = 1_000_000_000;

/// 单页战斗视图和单次伤害批量的资源限额；不限制伤害公式的原生功率。
pub const MAX_COMBAT_BATCH: u32 = 256;

#[derive(Debug, Clone)]
pub struct DamageRequest {
    pub id: String,
    pub amount: i64,
}

/// 按对象 ID 升序分页；next 是本页最后一个 ID，null 表示当前候选无后页。
#[derive(Serialize)]
pub struct CombatPage {
    pub objects: Vec<CombatView>,
    pub next: Option<String>,
}

/// 脱离候选所有权的战斗读面；字段使用玩家面部件名，不泄露快照码值。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatView {
    pub id: String,
    pub type_key: String,
    pub owner: Option<String>,
    pub x: u8,
    pub y: u8,
    pub hits: i64,
    pub hits_max: i64,
    pub body: Option<Vec<CombatBodyPart>>,
    pub spawning: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CombatBodyPart {
    #[serde(rename = "type")]
    pub kind: String,
    pub hits: u32,
    pub boost: Option<String>,
}
