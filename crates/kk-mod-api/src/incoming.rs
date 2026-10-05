//! Creep 受击策略的有界输入；数值须可由 JS Number 精确表示。
use serde::Serialize;

pub const MAX_DAMAGE: i64 = 9_007_199_254_740_991;
pub const MAX_SOURCES: usize = 256;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageSource {
    pub source_id: String,
    pub amount: i64,
    pub attack_type: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingInput {
    pub raw_damage: i64,
    pub default_damage: i64,
    pub pending_heal: i64,
    pub sources: Vec<DamageSource>,
    pub source_count: usize,
    pub sources_truncated: bool,
    pub unattributed_damage: i64,
}
