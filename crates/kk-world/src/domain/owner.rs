//! 读取对象归属，并为两种 JSON 投影保留控制器的归属三态。

use kk_schema::room::RoomObject;
use serde_json::{Value, json};

/// 读取对象的主人，供结算所有权检查和沙箱可见房间计算使用。
/// 结构化对象直接借用字段；Generic 的 data.owner 经 JSON 解析后返回拥有的字符串。
/// 有主人不等于授予房间视野：world_json 另行排除旗和工地。
pub fn object_owner(object: &RoomObject) -> Option<std::borrow::Cow<'_, str>> {
    match object {
        RoomObject::Creep(o) => Some(o.owner.as_str().into()),
        RoomObject::Spawn(o) => Some(o.owner.as_str().into()),
        RoomObject::Extension(o) => Some(o.owner.as_str().into()),
        RoomObject::Storage(o) => Some(o.owner.as_str().into()),
        RoomObject::Controller(o) => o.owner.as_deref().map(Into::into),
        RoomObject::ConstructionSite(o) => o.owner.as_deref().map(Into::into),
        // 旗有归属，但不授予房间视野；可见性过滤由 world_json 处理。
        RoomObject::Flag(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Tombstone(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Ruin(o) => o.owner.as_deref().map(Into::into),

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

/// 有主人时返回字符串；归属被清除时返回 null；从未拥有时不输出 owner 键。
/// owner_cleared 区分后两种情况，供沙箱 getter 区分 my=false 和 my=undefined。
/// 上游依据：engine/src/processor/intents/controllers/tick.js 的 user=null 处理。
pub(crate) fn controller_owner_value(o: &kk_schema::room::ControllerObj) -> Option<Value> {
    match (&o.owner, o.owner_cleared) {
        (Some(owner), _) => Some(json!(owner)),
        (None, true) => Some(Value::Null),
        (None, false) => None,
    }
}
