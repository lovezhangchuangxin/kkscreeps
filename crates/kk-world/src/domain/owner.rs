//! 归属领域语义：对象归属（视野过滤/结算所有权门共源）与 controller
//! 归属三态（两投影同形折叠）。

use kk_schema::room::RoomObject;
use serde_json::{Value, json};

/// 对象归属（视野过滤判定用）：typed 变体直读；Generic 透传解析 data.owner。
/// 官方 userObjects 按 DB 的 user 字段归集（data.js:103）——与投影面同源。
/// B3：tombstone/ruin 的 user 同样授房视野（data.js:105-116 只跳过 flag/
/// constructionSite——死爬虫的墓碑为其主人保住房间视野是官方机制）。
/// pub（M2B 前置批 P1 单源）：结算侧（kk-engine settle 的所有权门与
/// notifyWhenAttacked）与投影/视野归集同一归属语义——原 settle 侧
/// `object_owner_of` 双份拷贝删除，两消费者共此单源。
pub fn object_owner(object: &RoomObject) -> Option<std::borrow::Cow<'_, str>> {
    match object {
        RoomObject::Creep(o) => Some(o.owner.as_str().into()),
        RoomObject::Spawn(o) => Some(o.owner.as_str().into()),
        RoomObject::Extension(o) => Some(o.owner.as_str().into()),
        RoomObject::Storage(o) => Some(o.owner.as_str().into()),
        RoomObject::Controller(o) => o.owner.as_deref().map(Into::into),
        RoomObject::ConstructionSite(o) => o.owner.as_deref().map(Into::into),
        // M2B 扩面 P2 清偿：旗归属——消费面是旗私有投影（world_json 他人旗
        // 过滤+不可见房自家旗破例）；视野归集侧官方跳过表不含旗
        //（data.js:113-116），归集过滤须显式跳过 Flag 不授房视野
        RoomObject::Flag(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Tombstone(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Ruin(o) => o.owner.as_deref().map(Into::into),
        // B4：extractor 官方带 user（build.js:116/:215-219）——有主提取器为
        // 其主人授房视野（data.js:105-116 的 userObjects 归集）；mineral 无
        // user 键（官方矿物对象无 user——B3 data.js:105 注释同族）
        RoomObject::Extractor(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Generic(o) => serde_json::from_str::<Value>(&o.data).ok().and_then(|v| {
            v.get("owner")
                .and_then(Value::as_str)
                .map(str::to_string)
                .map(Into::into)
        }),
        _ => None,
    }
}

/// controller 归属三态投影：`Some` → owner 键；降级归零哨兵 → Null（官方
/// controllers/tick.js:56 `user = null`，玩家侧 my === false）；从未拥有 →
/// 不落键（my undefined）。两投影同形——原两侧手写互指「同步（层间一致）」
/// 的单源化（M2B P4）。
pub(crate) fn controller_owner_value(o: &kk_schema::room::ControllerObj) -> Option<Value> {
    match (&o.owner, o.owner_cleared) {
        (Some(owner), _) => Some(json!(owner)),
        (None, true) => Some(Value::Null),
        (None, false) => None,
    }
}
