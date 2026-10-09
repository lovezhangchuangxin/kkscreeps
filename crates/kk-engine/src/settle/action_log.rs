//! 每拍动作记录的旁路导出通道（room history / 客户端渲染产物形状）。
//!
//! 与 CreepObj 动作位图平行：位图只记"动作是否发生"，本通道按官方
//! processor 的 actionLog 形状记录坐标载荷，供 embed `captureActionLog`
//! 与后续渲染 feed 消费（套件契约要求的是"为 room history/client 渲染
//! 的 action-log 产物"，不是内部位图存储原样）。来源：
//! 官方 engine/src/processor.js:49-166 的模板重置与各 intent 写入点。
//!
//! 生命期：每拍随 `Working` 新建自动清空（官方 processor 每 tick 开头
//! 重置 actionLog 模板），同键后写覆盖（DEDUP 语义）。发布后随
//! `Engine.last_action_log` 跨拍保留至下一成功拍，与 `last_events`
//! 同形保存/恢复（检查点载荷一员；回放重跑时各读取点重建同值）。

use std::collections::BTreeMap;

use kk_schema::room::RoomObject;
use kk_world::ObjId;
use serde::{Deserialize, Serialize};

/// 单个动作的载荷。坐标一律指向动作目标；`attacked`/`healed` 例外，
/// 指向动作来源（官方 _damage.js:60-91、heal.js:30-31）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ActionLogPayload {
    /// 常规动作的目标坐标 `{x, y}`。
    Coords { x: u8, y: u8 },
    /// lab 反应的两个输入 lab 坐标 `{x1, y1, x2, y2}`
    ///（官方 run-reaction.js:88、reverse-reaction.js:90）。
    Reaction { x1: u8, y1: u8, x2: u8, y2: u8 },
    /// say 载荷；仅公开 say 进入导出——官方 processor.js:436-438 的
    /// 历史快照删除非公开 say。
    Say {
        message: String,
        #[serde(rename = "isPublic")]
        is_public: bool,
    },
    /// powerCreep usePower：`{id, x, y}`（目标存在取目标坐标，否则
    /// 自身坐标；官方 usePower.js:305-309）。
    Power { id: i64, x: u8, y: u8 },
    /// rangedMassAttack 的空对象载荷（官方 rangedMassAttack.js:58）。
    /// 载体为 deny_unknown_fields 的严格空结构：损坏/手改检查点里的
    /// 杂键映射、越界坐标或残缺载荷在 untagged 全变体尝试后整体拒绝，
    /// 不静默降级为空载荷。
    Mass(MassPayload),
}

/// `Mass` 的严格空载体：仅接受空对象 `{}`。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MassPayload {}

/// 一个对象的当拍动作记录与导出所需的对象元数据。
/// 字段名对齐套件 `RoomActionLogCapture.objects` 条目（room/tick 由
/// 读取面补充）；`type` 用官方 roomObjects 的具体类型名（tower/link/
/// lab），structureType 同值随结构携带。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionLogEntry {
    /// 对象 ID 的字符串形式（ObjId 未实现 Serialize；导出面用套件同形
    /// 的字符串 id）。
    pub id: String,
    #[serde(rename = "type")]
    pub obj_type: String,
    #[serde(rename = "structureType", skip_serializing_if = "Option::is_none")]
    pub structure_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub x: u8,
    pub y: u8,
    #[serde(rename = "actionLog")]
    pub action_log: BTreeMap<String, ActionLogPayload>,
}

/// 结算尾把累积的动作表装配为导出条目。对象在本拍内消失（同拍死亡）
/// 的记录随之丢弃——官方 saveRoomHistory 只快照最终的 roomObjects
///（processor.js:504-507），死亡对象不进历史。
pub fn collect(
    objects: &BTreeMap<ObjId, RoomObject>,
    recorded: BTreeMap<ObjId, BTreeMap<String, ActionLogPayload>>,
) -> Vec<ActionLogEntry> {
    recorded
        .into_iter()
        .filter_map(|(id, action_log)| {
            let obj = objects.get(&id)?;
            let (obj_type, structure_type, name, x, y) = match obj {
                RoomObject::Creep(c) => ("creep".to_string(), None, Some(c.name.clone()), c.x, c.y),
                RoomObject::Spawn(s) => ("spawn".to_string(), None, Some(s.name.clone()), s.x, s.y),
                RoomObject::Generic(g) => {
                    let data: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
                    let name = data
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string);
                    match kk_world::domain::generic::type_name(&g.data).as_deref() {
                        // powerCreep 复用 Generic 承载；导出面用官方类型名。
                        Some("powerCreep") => ("powerCreep".to_string(), None, name, g.x, g.y),
                        _ => {
                            let st = kk_world::domain::generic::structure_type(&g.data);
                            let st = st?;
                            (st.clone(), Some(st), name, g.x, g.y)
                        }
                    }
                }
                _ => return None,
            };
            Some(ActionLogEntry {
                id: id.as_str().to_string(),
                obj_type,
                structure_type,
                name,
                x,
                y,
                action_log,
            })
        })
        .collect()
}
