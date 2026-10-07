//! Mod 对象的扩展意图与自主生命周期派发：按冻结计划进入槽处理链。
//!
//! 收集层只认键形（`namespace:name`）并保留原始参数；此处的校验才是
//! 权威——意图必须在计划目录中、参数满足描述符 schema（严格转换）、
//! 目标对象类型匹配接收者、玩家通过归属字段授权。任何一项不满足都
//! 是玩家侧错误：意图丢弃，本轮照常结算（与未知 vanilla 动作同待遇）。
//! 校验全部通过后执行槽处理链，链内失败（含 JS 处理器异常）使本轮
//! 候选失败；宿主正常拒绝已撤销本次动作写入，后续动作继续结算。
//!
//! 归属策略：类型未声明 owner_field 时玩家不可执行其对象上的任何
//! 扩展意图（fail-closed）；显式授权策略声明随 SDK 面冻结再开放。

use std::collections::BTreeMap;

use kk_mod_api::{IntentScope, Key, validate};
use kk_world::ObjId;
use serde_json::Value;

use super::RoomCtx;

/// 在对象 tick 阶段派发自有生命周期；没有玩家授权或玩家意图前提。
/// ID 列表由房间循环固定，进入宿主前释放候选借用。生命周期首版
/// 不支持局部拒绝，任何异常或 Declined 都使整轮候选失败。
pub(crate) fn process_object_tick(ctx: &RoomCtx, id: ObjId) -> Result<(), String> {
    let (plan, type_key, contrib) = {
        let w = ctx.borrow();
        let Some(kk_schema::room::RoomObject::Mod(object)) = w.room.objects.get(&id) else {
            return Ok(());
        };
        (
            w.plane.plan.clone(),
            object.type_key.clone(),
            w.plane.contrib.clone(),
        )
    };
    let key = Key::new(type_key)?;
    let ty = plan
        .projection()
        .resolve(&key)
        .and_then(|handle| plan.projection().get(handle))
        .ok_or_else(|| format!("Mod 对象类型 {key} 不在计划中"))?;
    for slot in &ty.descriptor.lifecycle_slots {
        let outcome = contrib.run_ext(slot, ctx, id, &Value::Null)?;
        if matches!(outcome, kk_mod_api::SlotOutcome::Declined(_)) {
            return Err(format!("生命周期槽 {slot} 不支持正常拒绝"));
        }
    }
    Ok(())
}

/// 在对象 tick 阶段派发 Mod 建筑的生命周期槽。与自有对象同律：不支持
/// 局部拒绝，Declined 或异常使整轮候选失败；目标在进入宿主前重新读取。
pub(crate) fn process_building_tick(ctx: &RoomCtx, id: ObjId) -> Result<(), String> {
    let (slots, contrib) = {
        let w = ctx.borrow();
        let Some(kk_schema::room::RoomObject::Generic(g)) = w.room.objects.get(&id) else {
            return Ok(());
        };
        let Ok(data) = serde_json::from_str::<Value>(&g.data) else {
            return Ok(());
        };
        let Some(st) = data.get("structureType").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(building) = w.plane.buildings.get(st) else {
            return Ok(());
        };
        (
            building.lifecycle_slots.iter().cloned().collect::<Vec<_>>(),
            w.plane.contrib.clone(),
        )
    };
    for slot in &slots {
        let outcome = contrib.run_ext(slot, ctx, id, &Value::Null)?;
        if matches!(outcome, kk_mod_api::SlotOutcome::Declined(_)) {
            return Err(format!("建筑生命周期槽 {slot} 不支持正常拒绝"));
        }
    }
    Ok(())
}

/// 派发一个玩家在一个 Mod 对象上的全部扩展意图。调用方已确认对象
/// 是 [`kk_schema::room::RoomObject::Mod`]；目标在此重新读取，对象
/// 在本轮指令处理中先被销毁的极端形态按"目标不存在"丢弃。
pub(crate) fn process_object_ext_intents(
    ctx: &RoomCtx,
    player: &str,
    id: ObjId,
    list: &BTreeMap<Key, Value>,
) -> Result<(), String> {
    let type_key = match ctx.borrow().room.objects.get(&id) {
        Some(kk_schema::room::RoomObject::Mod(o)) => o.type_key.clone(),
        _ => return Ok(()),
    };
    // 冻结计划是共享的 Arc：在短借用内取出句柄后即释放借用，随后的
    // 派发不得持有候选借用。
    let plan = ctx.borrow().plane.plan.clone();
    for (intent_key, args) in list {
        let Some(handle) = plan.resolve_intent(intent_key) else {
            continue; // 不在冻结目录：未知动作丢弃
        };
        let descriptor = plan.get_intent(handle).expect("句柄来自计划");
        if descriptor.slot.namespace() == "builtin" {
            // builtin 意图（link.transfer）走 Rust 执行器，不经扩展链；
            // 装配门已拒绝 Mod 意图挂 builtin 槽，此处防御手工构造的批。
            continue;
        }
        // 压制（G07.b）：与 vanilla `Action::executes` 同语义——同接收者
        // 同 tick，压制者键存在于本对象的意图表即跳过被压制键。只看存在
        // 性不看压制者自身是否执行成功（压制者参数越界被丢弃时被压制键
        // 仍不执行）；玩家侧记录照常返 OK（官方静默形态）。冻结校验保证
        // 压制动作与被压制动作同 scope 同接收者，跨对象意图表天然隔离。
        if descriptor
            .suppressed_by
            .iter()
            .any(|suppressor| list.contains_key(suppressor))
        {
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
        let Some(type_handle) = plan.projection().resolve(&type_key_parsed) else {
            return Err(format!(
                "Mod 对象 {} 的类型 {type_key} 不在计划中",
                id.as_str()
            ));
        };
        let ty = plan.projection().get(type_handle).expect("句柄来自计划");
        let authorized = ty
            .descriptor
            .owner_field
            .as_ref()
            .and_then(|owner_field| {
                ctx.borrow()
                    .mod_fields(id)
                    .and_then(|fields| fields.get(validate::field_name(owner_field)).cloned())
            })
            .and_then(|owner| owner.as_str().map(str::to_string))
            .is_some_and(|owner| owner == player);
        if !authorized {
            continue;
        }
        // 描述符、槽键与执行表都在短借用内取到；派发时不得持有借用——
        // 链内 wrap 的 next() 会重新借用同一候选。
        let slot = descriptor.slot.clone();
        let contrib = ctx.borrow().plane.contrib.clone();
        let room_name = ctx.borrow().room.name.clone();
        let _outcome = contrib
            .run_ext(&slot, ctx, id, args)
            .map_err(|error| format!("动作 {intent_key} @ {room_name}: {error}"))?;
    }
    Ok(())
}
