//! Mod 对象的扩展意图派发：按冻结计划独立校验后进入槽处理链。
//!
//! 收集层只认键形（`namespace:name`）并保留原始参数；此处的校验才是
//! 权威——意图必须在计划目录中、参数满足描述符 schema（严格转换）、
//! 目标对象类型匹配接收者、玩家通过归属字段授权。任何一项不满足都
//! 是玩家侧错误：意图丢弃，本轮照常结算（与未知 vanilla 动作同待遇）。
//! 校验全部通过后执行槽处理链，链内失败（含 JS 处理器异常）使本轮
//! 候选失败。
//!
//! 归属策略：类型未声明 owner_field 时玩家不可执行其对象上的任何
//! 扩展意图（fail-closed）；显式授权策略声明随 SDK 面冻结再开放。

use std::collections::BTreeMap;

use kk_mod_api::{IntentScope, Key, validate};
use kk_world::ObjId;
use serde_json::Value;

use super::Working;

/// 派发一个玩家在一个 Mod 对象上的全部扩展意图。调用方已确认对象
/// 是 [`kk_schema::room::RoomObject::Mod`]；目标在此重新读取，对象
/// 在本轮指令处理中先被销毁的极端形态按"目标不存在"丢弃。
pub(crate) fn process_object_ext_intents(
    w: &mut Working,
    player: &str,
    id: ObjId,
    list: &BTreeMap<Key, Value>,
) -> Result<(), String> {
    let type_key = match w.room.objects.get(&id) {
        Some(kk_schema::room::RoomObject::Mod(o)) => o.type_key.clone(),
        _ => return Ok(()),
    };
    for (intent_key, args) in list {
        let Some(handle) = w.plane.plan.resolve_intent(intent_key) else {
            continue; // 不在冻结目录：未知动作丢弃
        };
        let descriptor = w.plane.plan.get_intent(handle).expect("句柄来自计划");
        if descriptor.slot.namespace() == "builtin" {
            // builtin 意图（link.transfer）走 Rust 执行器，不经扩展链；
            // 装配门已拒绝 Mod 意图挂 builtin 槽，此处防御手工构造的批。
            continue;
        }
        let Some(receiver) = &descriptor.receiver else {
            continue; // 装配门保证对象域意图必有接收者；防御性丢弃
        };
        if receiver.key.as_str() != type_key {
            continue; // 接收者类型不匹配：丢弃
        }
        if descriptor.scope != IntentScope::Object {
            continue; // 装配门已拒绝非对象域；防御性丢弃
        }
        if validate::check_value(args, &descriptor.arguments, &format!("动作 {intent_key}"))
            .is_err()
        {
            continue; // 参数不满足 schema（严格转换）：玩家侧错误丢弃
        }
        // 授权：目标类型须声明归属字段且归属字段值等于发射玩家。
        // 未声明归属字段的类型对玩家 fail-closed。
        // 接收者比较已保证 type_key 来自冻结计划（receiver 在投影类型中），
        // 下方两处 Err 是防御分支：正常装配下不可达，手搓批/直改世界才触及。
        let type_key_parsed = Key::new(type_key.clone())
            .map_err(|e| format!("Mod 对象类型键非法 {type_key:?}: {e}"))?;
        let Some(type_handle) = w.plane.plan.projection().resolve(&type_key_parsed) else {
            return Err(format!(
                "Mod 对象 {} 的类型 {type_key} 不在计划中",
                id.as_str()
            ));
        };
        let ty = w
            .plane
            .plan
            .projection()
            .get(type_handle)
            .expect("句柄来自计划");
        let authorized = ty
            .descriptor
            .owner_field
            .as_ref()
            .and_then(|owner_field| {
                w.mod_fields(id)
                    .and_then(|fields| fields.get(validate::field_name(owner_field)).cloned())
            })
            .and_then(|owner| owner.as_str().map(str::to_string))
            .is_some_and(|owner| owner == player);
        if !authorized {
            continue;
        }
        w.plane
            .contrib
            .run_ext(&descriptor.slot, w, id, args)
            .map_err(|error| format!("动作 {intent_key} @ {}: {error}", w.room.name))?;
    }
    Ok(())
}
