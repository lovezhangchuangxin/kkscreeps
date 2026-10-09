//! 普通 Invader 私有移动缓存与最近移动时钟；不进入玩家投影。
use super::{keeper_memory::KeeperPath, npc::INVADER_OWNER};
use crate::RoomState;
use kk_schema::room::RoomObject;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvaderMemory {
    #[serde(deserialize_with = "required_option")]
    pub move_cache: Option<KeeperPath>,
    #[serde(deserialize_with = "required_option")]
    pub last_move: Option<u64>,
}

pub(crate) fn validate(room: &RoomState) -> Result<(), String> {
    let valid_pos = |p: &[u8; 2]| p[0] < 50 && p[1] < 50;
    for (id, memory) in &room.invader_memory {
        if !matches!(room.objects.get(id), Some(RoomObject::Creep(c)) if c.owner == INVADER_OWNER) {
            return Err(format!("Invader 记忆引用无效执行者 {}", id.as_str()));
        }
        if memory.last_move.is_some_and(|t| t > 9_007_199_254_740_991) {
            return Err("Invader 最近移动时钟非法".into());
        }
        if let Some(cache) = &memory.move_cache
            && (!valid_pos(&cache.destination)
                || cache.time > 9_007_199_254_740_991
                || cache.path.len() > 2500
                || cache.path.iter().any(|p| !valid_pos(p))
                || cache
                    .path
                    .windows(2)
                    .any(|p| p[0][0].abs_diff(p[1][0]).max(p[0][1].abs_diff(p[1][1])) != 1))
        {
            return Err("Invader 路径坐标、时钟或相邻步进非法".into());
        }
    }
    Ok(())
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
