//! Source Keeper 私有巡守记忆；随房间候选与完整检查点保存，不进入玩家投影。
use serde::{Deserialize, Serialize};

use crate::RoomState;
use kk_schema::room::RoomObject;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeeperMemory {
    /// 目标删除后可暂留旧 ID，下一次预意图重新选择附近资源。
    pub source_id: Option<u64>,
    pub move_cache: Option<KeeperPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeeperPath {
    pub destination: [u8; 2],
    pub time: u64,
    /// 同房路径，按行走顺序保存，不含起点。
    pub path: Vec<[u8; 2]>,
}

/// 编辑、恢复与候选发布共用值域校验；不要求目标仍存活或缓存仍有效。
/// 缺少整个 keeper_memory 字段的旧检查点由 serde 拒绝，不补造记忆。
pub(crate) fn validate(room: &RoomState, next_id: u64) -> Result<(), String> {
    let valid_pos = |p: &[u8; 2]| p[0] < 50 && p[1] < 50;
    for (id, memory) in &room.keeper_memory {
        if !matches!(room.objects.get(id), Some(RoomObject::Creep(c)) if c.owner == super::keeper::OWNER)
        {
            return Err(format!("Keeper 记忆引用无效执行者 {}", id.as_str()));
        }
        if memory.source_id.is_some_and(|id| id == 0 || id >= next_id) {
            return Err("Keeper 目标 ID 非法".into());
        }
        if let Some(cache) = &memory.move_cache
            && (!valid_pos(&cache.destination)
                || cache.time > 9_007_199_254_740_991
                || cache.path.len() > 2500
                || cache.path.iter().any(|p| !valid_pos(p))
                || cache.path.windows(2).any(|p| {
                    let dx = p[0][0].abs_diff(p[1][0]);
                    let dy = p[0][1].abs_diff(p[1][1]);
                    dx.max(dy) != 1
                }))
        {
            return Err("Keeper 路径坐标、时钟或相邻步进非法".into());
        }
    }
    Ok(())
}
