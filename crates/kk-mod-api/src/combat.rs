//! 战斗策略的表示边界；公式、距离和目标选择由调用策略决定。

use serde::Serialize;

/// 单次策略伤害的非负整数上限。JS 精确表示且低于生命值 u32 域；
/// 不是原生 ATTACK/RANGED_ATTACK 功率或部件规则。
pub const MAX_DAMAGE: i64 = 1_000_000_000;

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
