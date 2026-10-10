//! lab 结算：runReaction、reverseReaction、boostCreep、unboostCreep。
//!
//! 逐条移植官方 engine/src/processor/intents/labs/*.js：处理器对一切失败
//! 静默 no-op（错误码由玩家预检层产生），本模块只镜像官方检查顺序与记账。
//! PWR_OPERATE_LAB 增量由公共效果表读取，正/逆反应共用严格有效期。
//! 不保存上游渲染用 actionLog 的结构侧字段（与 structures.rs 同约定）；
//! boost 对目标 creep 写 healed 位是官方 processor 的既有行为，保留。

use std::collections::BTreeMap;

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::{BodyKind, reaction_product, total_reaction_time};
use serde_json::{Value, json};

use super::Working;
use super::structures::{chebyshev, check_structure_against_controller, generic_view};
use crate::intent::IntentData;

/// REACTION_TIME[mineral]；未知矿物按 0（官方查表 undefined 参与加法）。
fn reaction_time(mineral: &str) -> i64 {
    c::REACTION_TIME
        .iter()
        .find(|(k, _)| *k == mineral)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// 官方 utils.getReactionVariants：按 REACTIONS 表序收集产物的全部有向
/// 试剂对（表双向对称，同一对两方向均入列）。serde_json 经 preserve_order
/// 保持常量文件的官方键序，空输出 lab 的方向分配与官方一致。
fn reaction_variants(mineral: &str) -> Vec<(String, String)> {
    use std::sync::OnceLock;
    static REACTIONS: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    let table = REACTIONS.get_or_init(|| serde_json::from_str(c::REACTIONS).ok());
    let mut out = Vec::new();
    if let Some(table) = table.as_ref().and_then(|t| t.as_object()) {
        for (r1, inner) in table {
            let Some(inner) = inner.as_object() else {
                continue;
            };
            for (r2, product) in inner {
                if product.as_str() == Some(mineral) {
                    out.push((r1.clone(), r2.clone()));
                }
            }
        }
    }
    out
}

/// BOOSTS[部件][矿物] 是否存在（官方 `C.BOOSTS[i.type] && C.BOOSTS[i.type][m]`）。
fn boost_applies(kind: BodyKind, mineral: &str) -> bool {
    use std::sync::OnceLock;
    static BOOSTS: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    let table = BOOSTS.get_or_init(|| serde_json::from_str(c::BOOSTS).ok());
    table
        .as_ref()
        .and_then(|t| t.get(kind.name()))
        .is_some_and(|t| t.get(mineral).is_some())
}

/// store 中首个非 energy 正量资源（官方 `_(store).keys().filter(...).first()`）。
/// lab 现实中至多一种矿物，键序差异只在多矿物容错路径上出现。
fn mineral_type_of(data: &Value) -> Option<String> {
    data.get("store")?
        .as_object()?
        .iter()
        .find(|(k, v)| k.as_str() != c::RESOURCE_ENERGY && v.as_i64().is_some_and(|n| n > 0))
        .map(|(k, _)| k.clone())
}

fn store_amount(data: &Value, resource: &str) -> i64 {
    data.pointer(&format!("/store/{resource}"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// 读取 lab 的位置与 data JSON；非 Generic lab 返回 None。
fn lab_snapshot(w: &Working, id: ObjId) -> Option<((u8, u8), Value)> {
    let RoomObject::Generic(g) = w.room.objects.get(&id)? else {
        return None;
    };
    let data: Value = serde_json::from_str(&g.data).ok()?;
    if data.get("structureType").and_then(Value::as_str) != Some("lab") {
        return None;
    }
    Some(((g.x, g.y), data))
}

/// 对指定 lab 的 data JSON 应用修改并写回。
fn update_lab(w: &mut Working, id: ObjId, f: impl FnOnce(&mut Value)) {
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
    {
        f(&mut v);
        g.data = v.to_string().into();
    }
}

/// lab 收进矿物：数量累加；首次出现的矿物绑定矿物容量槽并撤销共享容量
/// （官方 storeCapacity: null）。槽值用放置面同源的 StoreRules，保持
/// 玩家面 getCapacity 与结算一致。
fn lab_receive(
    data: &mut Value,
    mineral: &str,
    amount: i64,
    stores: &kk_mod_api::builtin::StoreRules,
) {
    let current = store_amount(data, mineral);
    data["store"][mineral] = json!(current + amount);
    let bound = data
        .pointer(&format!("/storeCapacityResource/{mineral}"))
        .and_then(Value::as_i64);
    if bound.is_none() {
        data["storeCapacityResource"][mineral] = json!(stores.lab_mineral);
        data["storeCapacity"] = Value::Null;
    }
}

/// lab 放出矿物：数量扣减；清零时解绑容量槽并恢复共享容量
/// （官方复位 LAB_ENERGY_CAPACITY + LAB_MINERAL_CAPACITY）。store 保留 0
/// 值键与官方同形。
fn lab_drain(
    data: &mut Value,
    mineral: &str,
    amount: i64,
    stores: &kk_mod_api::builtin::StoreRules,
) {
    let left = store_amount(data, mineral) - amount;
    data["store"][mineral] = json!(left);
    if left <= 0 {
        if let Some(slots) = data["storeCapacityResource"].as_object_mut() {
            slots.remove(mineral);
        }
        data["storeCapacity"] = json!(stores.lab_energy + stores.lab_mineral);
    }
}

fn on_cooldown(data: &Value, time: u64) -> bool {
    data.get("cooldownTime")
        .and_then(Value::as_u64)
        .is_some_and(|t| t > time)
}

/// run/reverse 同源：效果提高产量，也提高每份试剂的消耗量。
fn reaction_amount(w: &Working, id: ObjId) -> i64 {
    c::LAB_REACTION_AMOUNT
        + kk_world::effects::active_power(w.room.effects.get(&id), c::PWR_OPERATE_LAB, w.time)
            .and_then(|e| super::powers::power_effect(e.power, e.level))
            .unwrap_or(0)
}

/// 调用 lab 按有效效果计算批量，消耗两试剂各一份批量，产出反应产物并进入
/// REACTION_TIME[产物] 冷却。来源：processor/intents/labs/run-reaction.js。
pub fn run_reaction(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(((x, y), data)) = lab_snapshot(w, id) else {
        return;
    };
    if on_cooldown(&data, w.time) {
        return;
    }
    let amount = reaction_amount(w, id);
    let (Some(lab1), Some(lab2)) = (d.lab1, d.lab2) else {
        return;
    };
    let Some(((x1, y1), d1)) = lab_snapshot(w, lab1) else {
        return;
    };
    let Some(m1) = mineral_type_of(&d1) else {
        return;
    };
    if store_amount(&d1, &m1) < amount {
        return;
    }
    if chebyshev((x, y), (x1, y1)) > 2 {
        return;
    }
    let Some(((x2, y2), d2)) = lab_snapshot(w, lab2) else {
        return;
    };
    let Some(m2) = mineral_type_of(&d2) else {
        return;
    };
    if store_amount(&d2, &m2) < amount {
        return;
    }
    if chebyshev((x, y), (x2, y2)) > 2 {
        return;
    }
    // 官方满仓检查用常量容量，不读已绑槽。
    let own = mineral_type_of(&data);
    let own_amount = own.as_deref().map(|m| store_amount(&data, m)).unwrap_or(0);
    if own_amount + amount > w.plane.stores.lab_mineral {
        return;
    }
    let Some(product) = reaction_product(&m1, &m2) else {
        return;
    };
    if own.as_deref().is_some_and(|m| m != product) {
        return;
    }
    let cooldown_end = w.time + reaction_time(product) as u64;
    let product = product.to_string();
    let stores = w.plane.stores;
    update_lab(w, id, |v| {
        lab_receive(v, &product, amount, &stores);
        v["cooldownTime"] = json!(cooldown_end);
    });
    update_lab(w, lab1, |v| lab_drain(v, &m1, amount, &stores));
    update_lab(w, lab2, |v| lab_drain(v, &m2, amount, &stores));
    // 官方 run-reaction.js:88：调用 lab 写 runReaction，坐标=两个输入
    // 试剂 lab。
    w.record_action(
        id,
        "runReaction",
        super::action_log::ActionLogPayload::Reaction { x1, y1, x2, y2 },
    );
}

/// 调用 lab 按有效效果计算批量，分解化合物并向两个输出 lab 各发一份试剂，进入
/// REACTION_TIME[化合物] 冷却。官方 processor 不检查射程（driver 预检负责），
/// 本移植保持同形。来源：processor/intents/labs/reverse-reaction.js。
pub fn reverse_reaction(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some((_, data)) = lab_snapshot(w, id) else {
        return;
    };
    if on_cooldown(&data, w.time) {
        return;
    }
    let (Some(lab1), Some(lab2)) = (d.lab1, d.lab2) else {
        return;
    };
    if lab1 == lab2 {
        return;
    }
    let amount = reaction_amount(w, id);
    let Some(mineral) = mineral_type_of(&data) else {
        return;
    };
    if store_amount(&data, &mineral) < amount {
        return;
    }
    let output_full = |data: &Value, mineral: &Option<String>| -> bool {
        // 官方读已绑槽 storeCapacityResource[m]；未绑槽不构成满仓。
        mineral
            .as_deref()
            .and_then(|m| {
                data.pointer(&format!("/storeCapacityResource/{m}"))
                    .and_then(Value::as_i64)
            })
            .is_some_and(|cap| store_amount(data, mineral.as_deref().unwrap_or("")) + amount > cap)
    };
    let Some((_, d1)) = lab_snapshot(w, lab1) else {
        return;
    };
    let m1 = mineral_type_of(&d1);
    if output_full(&d1, &m1) {
        return;
    }
    let Some((_, d2)) = lab_snapshot(w, lab2) else {
        return;
    };
    let m2 = mineral_type_of(&d2);
    if output_full(&d2, &m2) {
        return;
    }
    let variants = reaction_variants(&mineral);
    if variants.is_empty() {
        return;
    }
    // 官方 _.find(variants)：表序取首个与两输出 lab 现有内容相容的方向
    // （空 lab 接收任一试剂）。
    let Some((v1, v2)) = variants.into_iter().find(|(a, b)| {
        m1.as_deref().is_none_or(|m| m == a.as_str())
            && m2.as_deref().is_none_or(|m| m == b.as_str())
    }) else {
        return;
    };
    let cooldown_end = w.time + reaction_time(&mineral) as u64;
    let stores = w.plane.stores;
    update_lab(w, id, |v| {
        lab_drain(v, &mineral, amount, &stores);
        v["cooldownTime"] = json!(cooldown_end);
    });
    update_lab(w, lab1, |v| lab_receive(v, &v1, amount, &stores));
    update_lab(w, lab2, |v| lab_receive(v, &v2, amount, &stores));
    // 官方 reverse-reaction.js:90：调用 lab 写 reverseReaction，坐标=
    // 两个输出 lab。
    if let Some((x1, y1)) = w.room.objects.get(&lab1).map(|o| o.pos())
        && let Some((x2, y2)) = w.room.objects.get(&lab2).map(|o| o.pos())
    {
        w.record_action(
            id,
            "reverseReaction",
            super::action_log::ActionLogPayload::Reaction { x1, y1, x2, y2 },
        );
    }
}

/// lab 向相邻 creep 的可强化部件注入矿物：每部件耗 LAB_BOOST_MINERAL 矿物
/// 与 LAB_BOOST_ENERGY 能量，资源不足即停；无冷却门。
/// 首个候选部件非 TOUGH 时从身体尾部开始强化（官方 quirk）。
/// 来源：processor/intents/labs/boost-creep.js。
pub fn boost_creep(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(((x, y), data)) = lab_snapshot(w, id) else {
        return;
    };
    let Some(mineral) = mineral_type_of(&data) else {
        return;
    };
    if store_amount(&data, &mineral) < c::LAB_BOOST_MINERAL
        || store_amount(&data, c::RESOURCE_ENERGY) < c::LAB_BOOST_ENERGY
    {
        return;
    }
    let Some(target) = d.id else { return };
    let Some(RoomObject::Creep(creep)) = w.room.objects.get(&target).cloned() else {
        return;
    };
    if creep.spawning {
        return;
    }
    if chebyshev((x, y), (creep.x, creep.y)) > 1 {
        return;
    }
    let mut candidates: Vec<usize> = creep
        .body
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.boost.is_none()
                && BodyKind::from_code(p.kind).is_some_and(|k| boost_applies(k, &mineral))
        })
        .map(|(i, _)| i)
        .collect();
    if candidates.is_empty() {
        return;
    }
    let first_is_tough =
        BodyKind::from_code(creep.body[candidates[0]].kind).is_some_and(|k| k == BodyKind::Tough);
    if !first_is_tough {
        candidates.reverse();
    }
    // 官方 `if(intent.bodyPartsCount)` 后 `slice(0, n)`：缺失或 0 不限制，
    // 负数从候选尾部收缩（JS slice 语义）。
    if let Some(n) = d.body_parts_count.filter(|n| *n != 0) {
        if n > 0 {
            candidates.truncate(n as usize);
        } else {
            let keep = (candidates.len() as i64 + n).max(0) as usize;
            candidates.truncate(keep);
        }
    }
    // 官方 while 循环按序消耗，任一资源不足即停。
    let mut mineral_left = store_amount(&data, &mineral);
    let mut energy_left = store_amount(&data, c::RESOURCE_ENERGY);
    let mut applied: Vec<usize> = Vec::new();
    for idx in candidates {
        if mineral_left < c::LAB_BOOST_MINERAL || energy_left < c::LAB_BOOST_ENERGY {
            break;
        }
        applied.push(idx);
        mineral_left -= c::LAB_BOOST_MINERAL;
        energy_left -= c::LAB_BOOST_ENERGY;
    }
    if applied.is_empty() {
        // 资源预检已保证 ≥30/≥20，此处清空只能来自负计数截空候选
        // （官方 slice(0,负数)）——官方仍写 healed 位与不变量的 store
        // 回写；此处补动作位，资源记账无变化。
        if let Some(creep) = w.creep_mut(target) {
            super::creep::set_action_log(&mut creep.action_log, super::creep::ACTION_HEALED);
        }
        if let Some((lx, ly)) = w.room.objects.get(&id).map(|o| o.pos()) {
            w.record_action(
                target,
                "healed",
                super::action_log::ActionLogPayload::Coords { x: lx, y: ly },
            );
        }
        return;
    }
    let drained = store_amount(&data, &mineral) - mineral_left;
    if let Some(creep) = w.creep_mut(target) {
        for idx in &applied {
            creep.body[*idx].boost = Some(mineral.clone());
        }
        super::creep::set_action_log(&mut creep.action_log, super::creep::ACTION_HEALED);
    }
    if let Some((lx, ly)) = w.room.objects.get(&id).map(|o| o.pos()) {
        w.record_action(
            target,
            "healed",
            super::action_log::ActionLogPayload::Coords { x: lx, y: ly },
        );
    }
    let stores = w.plane.stores;
    update_lab(w, id, |v| {
        lab_drain(v, &mineral, drained, &stores);
        v["store"][c::RESOURCE_ENERGY] = json!(energy_left);
    });
}

/// lab 清除相邻同主 creep 的全部强化并进入冷却：每部件退 LAB_UNBOOST_MINERAL
/// 矿物到 creep 所在格资源堆，冷却 = Σ部件×total_reaction_time×15/5。
/// 来源：processor/intents/labs/unboost-creep.js。
pub fn unboost_creep(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "lab" {
        return;
    }
    if on_cooldown(&view.data, w.time) {
        return;
    }
    let Some(target) = d.id else { return };
    let Some(RoomObject::Creep(creep)) = w.room.objects.get(&target).cloned() else {
        return;
    };
    if view.owner.as_deref() != Some(creep.owner.as_str()) {
        return;
    }
    if !check_structure_against_controller(w, id, &view) {
        return;
    }
    let Some(((x, y), _)) = lab_snapshot(w, id) else {
        return;
    };
    if chebyshev((x, y), (creep.x, creep.y)) > 1 {
        return;
    }
    let mut boosted: BTreeMap<String, i64> = BTreeMap::new();
    for part in &creep.body {
        if let Some(mineral) = &part.boost {
            *boosted.entry(mineral.clone()).or_insert(0) += 1;
        }
    }
    if boosted.is_empty() {
        return;
    }
    if let Some(creep) = w.creep_mut(target) {
        for part in creep.body.iter_mut() {
            part.boost = None;
        }
    }
    let (tx, ty) = (creep.x, creep.y);
    let mut cooldown = 0i64;
    for (mineral, parts) in &boosted {
        // LAB_UNBOOST_ENERGY 为 0，能量堆不产生；保留官方条件分支。
        let energy_return = parts * c::LAB_UNBOOST_ENERGY;
        if energy_return > 0 {
            w.create_resource(tx, ty, energy_return, c::RESOURCE_ENERGY);
        }
        let mineral_return = parts * c::LAB_UNBOOST_MINERAL;
        if mineral_return > 0 {
            w.create_resource(tx, ty, mineral_return, mineral);
        }
        cooldown +=
            parts * total_reaction_time(mineral) * c::LAB_UNBOOST_MINERAL / c::LAB_REACTION_AMOUNT;
    }
    if cooldown > 0 {
        let end = w.time + cooldown as u64;
        update_lab(w, id, |v| {
            v["cooldownTime"] = json!(end);
        });
    }
}
