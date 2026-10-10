//! 查询投影、沙箱投影和结算共用的对象归属、资源过滤与控制器等级规则。
//! JSON 的缺键、null 或数值缺省由各投影调用方决定。

pub mod controller;
pub(crate) mod deposit;
pub mod generic;
pub mod invader_core;
pub mod invader_memory;
pub mod invader_raid;
pub mod keeper;
pub mod keeper_memory;
pub mod npc;
pub mod owner;

/// 目标字面量的可靠前置门：命中字面量或载荷含 \u 转义时才需要完整
/// 解析。合法 JSON 中 ASCII 字母只能以字面量或 \uXXXX 转义出现——
/// 藏匿目标键/值（如 \"\\u0065ffects\"）必含 \u 转义；未含 \u 时
/// 未转义的双引号字面量只能是键名或完全相等的字符串值。误命中只多
/// 一次解析，不改变解析后的判定结果。
pub fn literal_gate(data: &str, literal: &str) -> bool {
    data.contains(literal) || data.contains("\\u")
}

/// Generic 写入门与发布校验共用的前置子串门：effects 键或五个域校验器
/// 的类型门（deposit/keeperLair/invaderCore/powerBank/portal）任一可能
/// 命中（含转义藏匿，见 [`literal_gate`]）时才需要完整解析。
pub(crate) fn domain_validate_markers_present(data: &str) -> bool {
    literal_gate(data, "\"effects\"")
        || literal_gate(data, "\"deposit\"")
        || literal_gate(data, "\"keeperLair\"")
        || literal_gate(data, "\"invaderCore\"")
        || literal_gate(data, "\"powerBank\"")
        || literal_gate(data, "\"portal\"")
}

pub mod portal;
pub mod power_bank;
pub(crate) mod room;
pub mod room_status;
pub mod store;
pub mod stronghold;
pub(crate) mod stronghold_templates;

pub use controller::controller_progress_total_opt;
pub(crate) use owner::controller_owner_value;
pub use owner::object_owner;
pub(crate) use store::{apply_store_extra, store_positive_entries};
