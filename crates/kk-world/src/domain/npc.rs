//! NPC 使用固定用户名表示归属，不复用玩家 handle 或官方数据库短 ID。

/// 官方用户 2 的公开用户名；不能注册为玩家工作者。
pub const INVADER_OWNER: &str = "Invader";

/// 玩家注册与跨房规则共用精确名称判定；短用户名仍可属于普通玩家。
pub fn is_npc_owner(owner: &str) -> bool {
    owner == INVADER_OWNER || owner == super::keeper::OWNER
}
