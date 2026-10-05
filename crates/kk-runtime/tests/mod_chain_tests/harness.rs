//! mod_chain_tests 的共享夹具：creep 放置工厂、读面助手与链观察包。
//! 各测试文件的历史助手名保持不变（mover/fighter/enemy/creep_at…），
//! 迁移时只删除本地定义并从这里导入。放置/投影/玩家面契约见同目录
//! README.md——写新测试前先读它，可避免夹具层面的试错轮次。
use super::*;

/// 放置 creep 的公共实现；energy 为 None 时不写 store 键（与历史
/// creep_at 形态一致），Some 时写入显式能量槽（mover/fighter 需要
/// 先行 transfer 等动作的能量）。
/// 放置参数聚合：房间/名字/归属/坐标/身体/初始能量（None 不写 store 键）。
struct CreepSpec<'a> {
    room: &'a str,
    name: &'a str,
    owner: &'a str,
    x: u8,
    y: u8,
    body: &'a [&'a str],
    energy: Option<u32>,
}

fn creep_placed(e: &mut EmbedEngine, spec: CreepSpec<'_>) -> anyhow::Result<String> {
    let mut json_spec = json!({
        "name": spec.name, "owner": spec.owner, "pos": [spec.x, spec.y],
        "body": spec.body, "ticksToLive": 100,
    });
    if let Some(energy) = spec.energy {
        json_spec["store"] = json!({"energy": energy});
    }
    e.place_object(spec.room, "creep", &json_spec)
}

/// 在指定房放置 creep（无携带能量）；跨房/双玩家场景用。
pub(crate) fn creep_at(
    e: &mut EmbedEngine,
    room: &str,
    name: &str,
    owner: &str,
    x: u8,
    y: u8,
    body: &[&str],
) -> anyhow::Result<String> {
    creep_placed(
        e,
        CreepSpec {
            room,
            name,
            owner,
            x,
            y,
            body,
            energy: None,
        },
    )
}

/// 主房 W1N1 的 p1 执行者，带 30 能量（供 transfer/drop 等先行动作
/// 与库存断言使用）。
pub(crate) fn fighter(
    e: &mut EmbedEngine,
    name: &str,
    x: u8,
    y: u8,
    body: &[&str],
) -> anyhow::Result<String> {
    creep_placed(
        e,
        CreepSpec {
            room: "W1N1",
            name,
            owner: "p1",
            x,
            y,
            body,
            energy: Some(30),
        },
    )
}

/// 主房 W1N1 的 p2 敌对目标（无能量）。
pub(crate) fn enemy(
    e: &mut EmbedEngine,
    name: &str,
    x: u8,
    y: u8,
    body: &[&str],
) -> anyhow::Result<String> {
    creep_placed(
        e,
        CreepSpec {
            room: "W1N1",
            name,
            owner: "p2",
            x,
            y,
            body,
            energy: None,
        },
    )
}

/// 与 [`fighter`] 同形；保留移动测试的历史命名。
pub(crate) fn mover(
    e: &mut EmbedEngine,
    name: &str,
    x: u8,
    y: u8,
    body: &[&str],
) -> anyhow::Result<String> {
    fighter(e, name, x, y, body)
}

/// 对象坐标（不校验房间）。
pub(crate) fn pos_of(e: &EmbedEngine, id: &str) -> (i64, i64) {
    let o = e.get_object(id).unwrap();
    (
        o["pos"]["x"].as_i64().unwrap(),
        o["pos"]["y"].as_i64().unwrap(),
    )
}

/// 对象坐标并断言仍在指定房（跨房迁移测试用）。
pub(crate) fn pos_in(e: &EmbedEngine, room: &str, id: &str) -> (i64, i64) {
    let o = e.get_object(id).unwrap();
    let p = &o["pos"];
    assert_eq!(p["roomName"], room, "对象应仍在原房");
    (p["x"].as_i64().unwrap(), p["y"].as_i64().unwrap())
}

/// 对象当前耐久；放置的 creep 恒为 body×100（hits 不可放置）。
pub(crate) fn hits(e: &EmbedEngine, id: &str) -> i64 {
    e.get_object(id).unwrap()["hits"].as_i64().unwrap()
}

/// 指定房本轮事件流中某类型的全部条目。事件每 tick 整体替换，
/// 断言只反映最后一次 run_player/run_players 的那一轮。
pub(crate) fn events_in(e: &EmbedEngine, room: &str, event: i64) -> Vec<Value> {
    e.world_snapshot()["rooms"][room]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["event"] == event)
        .cloned()
        .collect()
}

/// 主房 W1N1 的事件过滤。
pub(crate) fn events(e: &EmbedEngine, event: i64) -> Vec<Value> {
    events_in(e, "W1N1", event)
}

/// 观察链包：使槽进入整槽宿主路径，不拒绝、不改动结算。
pub(crate) fn observe_pkg(slot: &str) -> String {
    package(&format!(
        "api.handlers.before('{slot}',{{id:'observe'}},ctx=>{{ctx.state={{seen:(ctx.state?.seen||0)+1}};}});"
    ))
}

/// 可预测对象 ID：放置序的第 n 个对象（1 为房间基设对象如控制器，
/// 首个手动放置对象为 2）。mod 源码无法读取运行期 ID，需在构建包
/// 之前以此嵌入字面量，并在放置后断言命中——假设破裂立即报错，
/// 而不是静默测错对象。
pub(crate) fn predictable_id(n: u64) -> String {
    kk_world::ObjId(n).as_str().to_string()
}
