//! 原始记录的 FIND 类型过滤，不包含玩家视野判定。
use super::*;

fn obj_str<'a>(o: &'a Value, key: &str) -> &'a str {
    o.get(key).and_then(Value::as_str).unwrap_or("")
}

fn obj_energy(o: &Value) -> i64 {
    o.get("energy").and_then(Value::as_i64).unwrap_or(3000)
}

/// 按 FIND 常量匹配原始沙箱记录，不执行房间视野或私有旗过滤。
/// 有 viewer 时，MY_* 和 HOSTILE_* 按字符串 owner 判定，creep 查询排除 spawning。
/// 无 viewer 时，MY_*、HOSTILE_* 均不匹配，但 FIND_CREEPS 保留孵化中的 creep。
/// 这种中立查询供测试接口使用，不等同于玩家 room.find。
/// 参考：engine/src/game/game.js 的 findCacheFn；screeps-ok 的 vanilla findTypeMap。
pub fn matches_find(o: &Value, find_const: i64, viewer: Option<&str>) -> bool {
    let kind = obj_str(o, "type");
    let st = obj_str(o, "structureType");
    let is_struct = kind == "structure" || kind == "spawn";
    // 只有字符串 owner 参与归属；null 或缺键都不算敌方。
    // 这保留无主控制器的查询行为。creep、spawn 的结构化 owner 必为字符串；
    // 无主工地或手写 Generic 数据未必与上游玩家 getter 的敌对判定相同。
    let owner_str = o.get("owner").and_then(Value::as_str);
    let owned_by = |v: &str| owner_str == Some(v);
    let hostile_of = |v: &str| owner_str.is_some() && owner_str != Some(v);
    let spawning = o.get("spawning").and_then(Value::as_bool).unwrap_or(false);
    match find_const {
        // 玩家查询排除孵化中的 creep，中立查询保留。
        f if f == c::FIND_CREEPS => match viewer {
            Some(_) => kind == "creep" && !spawning,
            None => kind == "creep",
        },
        f if f == c::FIND_MY_CREEPS => kind == "creep" && !spawning && viewer.is_some_and(owned_by),
        f if f == c::FIND_HOSTILE_CREEPS => {
            kind == "creep" && !spawning && viewer.is_some_and(hostile_of)
        }

        f if f == c::FIND_SOURCES_ACTIVE => {
            kind == "structure" && st == "source" && obj_energy(o) > 0
        }
        f if f == c::FIND_SOURCES => kind == "structure" && st == "source",
        f if f == c::FIND_DROPPED_RESOURCES => kind == "resource",
        f if f == c::FIND_STRUCTURES => {
            // source、mineral 使用此原始 type，但玩家侧属于独立类别。
            // 来源：engine/src/game/game.js 的 structureTypes 与对象装配分支。
            is_struct && st != "source" && st != "mineral"
        }
        f if f == c::FIND_MY_STRUCTURES => {
            is_struct && st != "source" && st != "mineral" && viewer.is_some_and(owned_by)
        }
        f if f == c::FIND_HOSTILE_STRUCTURES => {
            is_struct && st != "source" && st != "mineral" && viewer.is_some_and(hostile_of)
        }
        f if f == c::FIND_FLAGS => kind == "flag",
        f if f == c::FIND_CONSTRUCTION_SITES => kind == "constructionSite",
        f if f == c::FIND_MY_SPAWNS => kind == "spawn" && viewer.is_some_and(owned_by),
        f if f == c::FIND_HOSTILE_SPAWNS => kind == "spawn" && viewer.is_some_and(hostile_of),
        f if f == c::FIND_MY_CONSTRUCTION_SITES => {
            kind == "constructionSite" && viewer.is_some_and(owned_by)
        }
        f if f == c::FIND_HOSTILE_CONSTRUCTION_SITES => {
            kind == "constructionSite" && viewer.is_some_and(hostile_of)
        }
        f if f == c::FIND_MINERALS => kind == "structure" && st == "mineral",
        f if f == c::FIND_TOMBSTONES => kind == "tombstone",
        f if f == c::FIND_POWER_CREEPS => kind == "powerCreep",
        f if f == c::FIND_DEPOSITS => kind == "deposit",
        f if f == c::FIND_RUINS => kind == "ruin",
        _ => false,
    }
}
