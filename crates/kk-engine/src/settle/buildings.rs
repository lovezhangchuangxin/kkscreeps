//! Mod 建筑的结算消费面：成本判定、工地创建、多资源建造消耗、完工
//! 结构与生产周期。配方数据全部来自冻结计划的 [`BuildingCatalog`]；
//! 内建结构继续走官方常量表，不经过本模块。

use kk_core::constants as c;
use kk_mod_api::buildings::BuildingDescriptor;
use kk_schema::room::RoomObject;
use serde_json::{Value, json};

use super::Working;

/// 类型是否携带建造成本语义：内建 CONSTRUCTION_COST 成员，或计划声明
/// 的 Mod 建筑。工地创建、维修/拆除/销毁与受袭开关的准入都按它判定。
pub(in crate::settle) fn cost_bearing(w: &Working, type_name: &str) -> bool {
    kk_world::types::construction_cost(type_name).is_some()
        || w.plane.buildings.get(type_name).is_some()
}

/// 类型在当前计划下是否阻断通行：内建静态表或 Mod 建筑的 obstacle 语义。
pub(in crate::settle) fn obstacle_at(w: &Working, type_name: &str) -> bool {
    kk_world::types::is_obstacle_type(type_name) || w.plane.buildings.is_obstacle(type_name)
}

/// 进度 p 时某成本资源的累计应付量：amount·p/T 向下取整。完工（p=T）
/// 恰好付清；单资源成本退化为 1:1。用 u128 中间量避免乘法溢出。
fn cumulative(amount: u64, total: u64, progress: i64) -> i64 {
    if total == 0 {
        return 0;
    }
    let value = (amount as i128) * (progress.clamp(0, i64::MAX) as i128) / (total as i128);
    value.min(i64::MAX as i128) as i64
}

/// creep 当前持有的某资源量（含内建槽与命名槽）。
fn held_of(w: &Working, id: kk_world::ObjId, resource: &str) -> i64 {
    match w.creep(id) {
        Some(creep) => {
            kk_world::types::store_extra_slot(&creep.store, &creep.store_extra, resource) as i64
        }
        None => 0,
    }
}

/// 扣减 creep 的某资源（正数为支付扣除、负数为返还增加）。
fn apply_cost(w: &mut Working, id: kk_world::ObjId, resource: &str, delta: i64) {
    let Some(creep) = w.creep_mut(id) else {
        return;
    };
    let mut store = creep.store.clone();
    let mut extra = creep.store_extra.clone();
    if delta > 0 {
        super::creep::slots_sub(&mut store, &mut extra, resource, delta);
    } else if delta < 0 {
        super::creep::slots_add(&mut store, &mut extra, resource, -delta);
    }
    if let Some(c) = w.creep_mut(id) {
        c.store = store;
        c.store_extra = extra;
    }
}

/// Mod 工地的建造结算：WORK 效果（含 boost）推进进度，各成本资源按
/// 累计比例扣减，完工恰好付清全部成本。可行进度由全部成本资源的持有
/// 量共同限定；宿主放置的超限工地沿既有约定回拨进度并返还等价资源。
///
/// 与内建的 boost 语义有意分歧：内建「能耗按未强化效果、进度按 boost
/// 后效果」（build 是 boost 越多越省），本路径支付恒锚定进度比例——
/// boost 加速进度的同时按同比例提高支付速率，单资源成本仅在无 boost
/// 时与内建 1:1 一致。多资源支付需要逐工地付款账本才能复刻内建口径
/// （工地只持久 progress），不值得为此加快照字段。
/// 另一处次要不对称：无支付能力时本路径不记 amount=0 的 build 事件
/// （内建在有能量但 remaining<=0 时记）；玩家面预检先行拦截，仅直注
/// 意图可达。
pub(in crate::settle) fn build_mod_site(
    w: &mut Working,
    id: kk_world::ObjId,
    target_id: kk_world::ObjId,
    site: &kk_schema::room::ConstructionSiteObj,
    building: &BuildingDescriptor,
) {
    let total = building.progress_total() as i64;
    if total <= 0 {
        return;
    }
    let progress = site.progress as i64;
    let remaining = total - progress;
    // boost 后的单 tick 最大推进量；无 WORK 部件为 0。
    let boosted_max = match w.creep(id) {
        Some(creep) => kk_world::types::boosted_effect(
            &creep.body,
            kk_world::types::BodyKind::Work,
            "work",
            "build",
            c::BUILD_POWER,
            (kk_world::types::BodyKind::count_active(&creep.body, kk_world::types::BodyKind::Work)
                as i64)
                .saturating_mul(c::BUILD_POWER),
            remaining,
        ),
        None => 0,
    };
    let mut effect = boosted_max.min(remaining);
    if effect > 0 {
        // 各资源最多支持的推进点数：cum(p') - cum(p0) <= held。
        for (resource, amount) in &building.cost {
            let budget = cumulative(*amount, total as u64, progress) + held_of(w, id, resource);
            // floor(amount·p'/T) <= budget  ⟺  p' <= ceil(T·(budget+1)/amount) - 1
            let ceil = ((total as i128 * (budget as i128 + 1) + *amount as i128 - 1)
                / (*amount as i128)) as i64;
            effect = effect.min(ceil.saturating_sub(1) - progress);
        }
    }
    if effect <= 0 && remaining > 0 {
        // 无 WORK 或无支付能力：与内建能量不足同待遇，不记事件。
        return;
    }
    let new_progress = progress + effect;
    // 消耗按累计比例从旧进度推进到新进度；负值对应超限回拨的返还。
    for (resource, amount) in &building.cost {
        let delta = cumulative(*amount, total as u64, new_progress)
            - cumulative(*amount, total as u64, progress);
        if delta != 0 {
            apply_cost(w, id, resource, delta);
        }
    }
    let incomplete = new_progress < total;
    w.push_event(
        c::EVENT_BUILD,
        id,
        json!({
            "targetId": target_id.as_str(),
            "amount": effect,
            "structureType": site.structure_type,
            "x": site.x,
            "y": site.y,
            "incomplete": incomplete,
        }),
    );
    if incomplete {
        if let Some(RoomObject::ConstructionSite(s)) = w.room.objects.get_mut(&target_id) {
            s.progress = new_progress.max(0) as u32;
        }
    } else {
        complete_mod_construction(w, target_id, site, building);
    }
}

/// 移除 Mod 工地时按进度比例返还一半已付资源（与内建 progress/2 退能量
/// 同口径）；同格容器优先接收，剩余进掉落堆。
pub(in crate::settle) fn refund_mod_site_from(
    w: &mut Working,
    x: u8,
    y: u8,
    progress: u32,
    building: &BuildingDescriptor,
) {
    let total = building.progress_total();
    if total == 0 {
        return;
    }
    for (resource, amount) in &building.cost {
        let paid = cumulative(*amount, total, progress as i64);
        let refund = paid / 2;
        if refund > 0 {
            w.create_resource(x, y, refund, resource);
        }
    }
}

/// 拆解 Mod 建筑的回收：按成本构成占比瓜分伤害量/200——各资源分母取
/// 成本总量 ΣA，回收总值恰为伤害/200（单资源成本与内建 amount/200 严格
/// 等值；若按最大成本量做分母，N 条等量成本会放大 N 倍回收）。入执行者
/// 库存，超容由既有掉落路径处理。
pub(in crate::settle) fn dismantle_refund(
    w: &mut Working,
    id: kk_world::ObjId,
    building: &BuildingDescriptor,
    amount: i64,
) {
    let total_cost: i128 = building.cost.iter().map(|(_, cost)| *cost as i128).sum();
    if total_cost <= 0 || amount <= 0 {
        return;
    }
    for (resource, cost_amount) in &building.cost {
        let gain = (amount as i128) * (*cost_amount as i128) / (200 * total_cost);
        if gain > 0 {
            apply_cost(w, id, resource, -(gain as i64));
        }
    }
}

/// 生产周期：到达周期 tick 且库存同时满足全部输入、全部产物可入库时，
/// 原子地消耗输入并产出。周期锚定房间绝对时间（time % interval），
/// 无逐建筑相位状态，检查点恢复后续跑语义不变。
pub(in crate::settle) fn production_tick(w: &mut Working, id: kk_world::ObjId) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let Ok(mut data) = serde_json::from_str::<Value>(&g.data) else {
        return;
    };
    let Some(structure_type) = data.get("structureType").and_then(Value::as_str) else {
        return;
    };
    let Some(building) = w.plane.buildings.get(structure_type) else {
        return;
    };
    let Some(production) = &building.production else {
        return;
    };
    if production.interval == 0 || !w.time.is_multiple_of(production.interval as u64) {
        return;
    }
    // 先以不可变借用完成全部预检，再取可变 store 写入，避免借用交叉。
    let held = |resource: &str| -> i64 { store_held(&data, resource) };
    if production
        .inputs
        .iter()
        .any(|(resource, amount)| held(resource) < *amount as i64)
    {
        return;
    }
    // 共享容量语义（与 Generic 仓库一致）：无 storeCapacityResource 键，
    // 全部资源合计不得超过 storeCapacity。
    let capacity = g_data_capacity(&data);
    let used: i64 = data
        .get("store")
        .and_then(Value::as_object)
        .map(|store| store.values().filter_map(Value::as_i64).sum())
        .unwrap_or(0);
    let produced: i64 = production
        .outputs
        .iter()
        .map(|(_, amount)| *amount as i64)
        .sum();
    if used + produced > capacity {
        return;
    }
    let Some(store) = data.get_mut("store").and_then(Value::as_object_mut) else {
        return;
    };
    for (resource, amount) in &production.inputs {
        let next = store.get(resource).and_then(Value::as_i64).unwrap_or(0) - *amount as i64;
        store.insert(resource.clone(), json!(next));
    }
    for (resource, amount) in &production.outputs {
        let next = store.get(resource).and_then(Value::as_i64).unwrap_or(0) + *amount as i64;
        store.insert(resource.clone(), json!(next));
    }
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id) {
        g.data = data.to_string();
    }
}

/// 读取 Generic 数据 JSON 中某资源的库存量；缺失按 0 处理。
fn store_held(data: &Value, resource: &str) -> i64 {
    data.get("store")
        .and_then(|store| store.get(resource))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// 读取 Generic 数据 JSON 的共享容量键；缺失按 0 处理。
fn g_data_capacity(data: &Value) -> i64 {
    data.get("storeCapacity")
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// 完工：移除工地并落成 Generic 结构；耐久、归属与初始仓库来自配方，
/// 寻路语义由目录在投影/仲裁侧解释。ID 由 insert_new 分配。
pub(in crate::settle) fn complete_mod_construction(
    w: &mut Working,
    site_id: kk_world::ObjId,
    site: &kk_schema::room::ConstructionSiteObj,
    building: &BuildingDescriptor,
) {
    w.remove_obj(site_id);
    let hits = building.hits_max.min(u32::MAX as u64) as u32;
    let mut data = json!({
        "type": "structure",
        "structureType": building.key.as_str(),
        "owner": site.owner,
        "hits": hits,
        "hitsMax": hits,
        "notifyWhenAttacked": true,
    });
    // 有容量才有 store 键：玩家 transfer 的原始 store 预检与内建完工
    // 结构同口径。
    if let Some(cap) = building.store_capacity {
        data["storeCapacity"] = json!(cap.min(u32::MAX as u64));
        data["store"] = json!({"energy": 0});
    }
    w.insert_new(RoomObject::Generic(kk_schema::room::GenericObj {
        id: 0,
        x: site.x,
        y: site.y,
        data: data.to_string(),
    }));
}
