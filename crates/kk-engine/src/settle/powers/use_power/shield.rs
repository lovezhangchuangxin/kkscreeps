//! SHIELD 的工地清理、地形准入与临时 rampart 候选创建。
use super::super::{pc_data, power_effect, power_info};
use crate::settle::Working;
use kk_core::constants as c;
use kk_schema::room::RoomObject;
use serde_json::{Value, json};

/// 工地删除发生在放置门之前；失败仍保留该候选变更，但不进入施放成功尾部。
/// 来源：engine/src/processor/intents/power-creeps/usePower.js 的 SHIELD 分支。
pub(super) fn cast(w: &mut Working, pos: (u8, u8), owner: &str, level: i64) -> bool {
    let Some(info) = power_info(c::PWR_SHIELD) else {
        return false;
    };
    // 官方 usePower.js:228-257：先删除自身位置的 constructionSite，
    // 通过对象/地形两门 rampart 放置检查后原地插入临时 rampart：
    // hits=effect[level-1]、hitsMax=0、nextDecayTime=endTime，携带
    // PWR_SHIELD 效果；衰减由 generic_decay_tick 的删除门接管。
    // POWER_INFO.energy 字段官方无消费者，不扣（ops 亦为 0）。
    let Some(duration) = info.get("duration").and_then(Value::as_u64) else {
        return false;
    };
    let Some(hits) = power_effect(c::PWR_SHIELD, level) else {
        return false;
    };
    let (x, y) = pos;
    let mut rampart_blocked = false;
    let mut site_ids = Vec::new();
    for (tid, obj) in w.room.objects.iter() {
        let (ox, oy) = obj.pos();
        if ox != x || oy != y {
            continue;
        }
        match obj {
            RoomObject::ConstructionSite(_) => site_ids.push(*tid),
            other if pc_data(other).is_some_and(|t| t["structureType"] == "rampart") => {
                rampart_blocked = true;
            }
            _ => {}
        }
    }
    for tid in site_ids {
        w.remove_obj(tid);
    }
    // 官方 checkConstructionSite 对象门末尾的坐标界检查在工地
    // 删除之后（utils.js:186）：越界 pc 的工地同样被删再拒绝。
    if x == 0 || y == 0 || x >= 49 || y >= 49 {
        return false;
    }
    if rampart_blocked {
        return false;
    }
    // 官方 checkConstructionSite 地形门：本格非墙；贴边（1/48）时
    // 三块边界格须全为墙（rampart 不适用其余结构避让规则）。
    if w.room
        .terrain()
        .check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_WALL)
    {
        return false;
    }
    // 官方 utils.js:134-137 为顺序 if：四角位置 y 分支覆盖 x 分支，
    // 最终检查 y 侧三块边界格（非四角两实现等价）。
    let mut border: Option<[(usize, usize); 3]> = None;
    if x == 1 {
        border = Some([(0, y as usize - 1), (0, y as usize), (0, y as usize + 1)]);
    }
    if x == 48 {
        border = Some([(49, y as usize - 1), (49, y as usize), (49, y as usize + 1)]);
    }
    if y == 1 {
        border = Some([(x as usize - 1, 0), (x as usize, 0), (x as usize + 1, 0)]);
    }
    if y == 48 {
        border = Some([(x as usize - 1, 49), (x as usize, 49), (x as usize + 1, 49)]);
    }
    if let Some(tiles) = border
        && tiles.iter().any(|(bx, by)| {
            !w.room
                .terrain()
                .check_mask(*bx, *by, kk_core::TERRAIN_MASK_WALL)
        })
    {
        return false;
    }
    let end_time = w.time + duration;
    // 官方 bulk.insert 不带 notifyWhenAttacked（受击不发通知）。
    let data = json!({
        "type": "structure", "structureType": "rampart",
        "owner": owner, "hits": hits, "hitsMax": 0,
        "nextDecayTime": end_time,
    });
    let Ok(new_id) = w.insert_new(RoomObject::Generic(kk_schema::room::GenericObj {
        id: 0,
        x,
        y,
        data: data.to_string().into(),
    })) else {
        return false;
    };
    w.room
        .effects
        .entry(new_id)
        .or_default()
        .push(kk_world::EffectEntry {
            // 官方 SHIELD 专用分支不写 effect（usePower.js:248-253），
            // 玩家面读 undefined；通用路径 effect==power。
            effect: None,
            power: c::PWR_SHIELD,
            level,
            end_time,
        });
    true
}
