//! 将一次引擎调用归一化为权威参数、native 中心与唯一领域能力。
use super::{ModChainContext, capability::ChainCapability, completion::Completion};
use kk_engine::intent::IntentData;
use kk_engine::settle::{RoomCtx, Working, contrib::SlotCall};
use kk_world::ObjId;
use serde_json::{Value, json};

pub(super) enum NativeCall {
    Action {
        data: Box<IntentData>,
        step: fn(&mut Working, ObjId, &IntentData),
    },
    Tick(fn(&mut Working, ObjId)),
    /// 数值/移动候选已在入链前初始化；中心保留 before 的修改，不重复计算。
    Preserve,
    None,
}

pub(super) struct PreparedCall {
    pub(super) id: ObjId,
    args: Value,
    native: NativeCall,
    capability: ChainCapability,
}

impl PreparedCall {
    pub(super) fn new(slot: &str, call: SlotCall<'_>) -> Result<Self, String> {
        let (id, args, native, capability) = match call {
            SlotCall::Action { id, data, native } => {
                let capability = ChainCapability::for_action(slot);
                let args = if matches!(&capability, ChainCapability::Move) {
                    // move 槽固定参数面：方向或用于求方向的相邻 creep。
                    json!({"id": data.id.map(|id| id.as_str()), "direction": data.direction})
                } else {
                    json!({"id": data.id.map(|id| id.as_str()), "amount": data.amount, "resourceType": data.resource_type})
                };
                (
                    id,
                    args,
                    NativeCall::Action {
                        data: Box::new(data.clone()),
                        step: native,
                    },
                    capability,
                )
            }
            SlotCall::Tick { id, native } => (
                id,
                Value::Null,
                NativeCall::Tick(native),
                ChainCapability::Plain,
            ),
            SlotCall::Extension { id, args } => {
                (id, args.clone(), NativeCall::None, ChainCapability::Objects)
            }
            SlotCall::Incoming { id, candidate } => (
                id,
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::Incoming(candidate),
            ),
            // G09.c 结构受击：输入为当次伤害与来源摘要，目标形状经
            // ctx.target（Mod payload 含耐久/库存信封）承载。
            SlotCall::ModIncoming { id, candidate } => (
                id,
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::ModIncoming(candidate),
            ),
            SlotCall::Contest { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::Contest(candidate),
            ),
            SlotCall::Admission { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::Admission(candidate),
            ),
            SlotCall::Obstacle { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::Obstacle(candidate),
            ),
            SlotCall::Precheck { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::Precheck(candidate),
            ),
            SlotCall::PullRegistration { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::PullRegistration(candidate),
            ),
            SlotCall::MoveCost { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Preserve,
                ChainCapability::MoveCost(candidate),
            ),
        };
        // 房间槽没有单一对象目标；u64::MAX 仅作诊断占位，不与真实 ID 冲突。
        Ok(Self {
            id,
            args,
            native,
            capability,
        })
    }

    pub(super) fn into_context(self, ctx: &RoomCtx) -> (ModChainContext, Completion) {
        let completion = Completion::prepare(ctx, &self.capability);
        let context = ModChainContext {
            ctx: ctx.clone(),
            id: self.id,
            args: self.args,
            native: self.native,
            cached_fields: None,
            staged_states: completion.staged_states(),
            capability: self.capability,
        };
        (context, completion)
    }
}
