//! 官方 deployStronghold 的布防执行：部署到期时清格、按模板插入布防
//! 结构、核心挂坍塌效果并写入 decayTime。战术 AI（refillTowers/
//! focusClosest/maintainPopulation）与 create-creep 等意图处理器另片接线。
//! 来源：上游 engine/src/processor/intents/invader-core/stronghold/stronghold.js。
use kk_core::constants as c;
use kk_schema::room::{GenericObj, RoadObj, RoomObject};
use kk_world::ObjId;
use kk_world::domain::npc::INVADER_OWNER;
use kk_world::domain::stronghold::{self, StrongholdTemplate};
use serde_json::{Map, Value, json};

use super::{SettleError, Working};

/// 部署随机流：房名 + 核心 ID + 部署 tick 整体混合，重试/回放/冷恢复
/// 同值（与矿物密度 MineralRng 同族的 SplitMix64 终结器）。
struct DeployRng(u64);

impl DeployRng {
    fn new(room: &str, domain: &str, id: ObjId, time: u64) -> Self {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in format!("{domain}#{room}").as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        Self(
            h ^ id.0.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ time.wrapping_mul(0xbf58_476d_1ce4_e5b9),
        )
    }

    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// 官方 deployStronghold 的 duration 抽样：STRONGHOLD_DECAY_TICKS ×
/// (0.9 + Math.random()×0.2)，不使用官方 Math.random。
fn decay_duration(w: &Working, id: ObjId) -> i64 {
    let mut rng = DeployRng::new(&w.room.name, "stronghold#deploy", id, w.time);
    (c::STRONGHOLD_DECAY_TICKS as f64 * (0.9 + rng.unit() * 0.2)).round() as i64
}

/// 官方 utils.calcReward 的逐字复刻（含其怪癖）：洗牌取 3 项资源，
/// 逐项 round(rand×(target-current)/该项密度)；末项除数是
/// `densities[order.length-1]`——位置索引 2 恒指向密度表第三项
///（zynthium_bar=5），与实际抽到末位的资源无关，官方 utils.js:695
/// 固有形态，末位密度非 5 时总量可超发——对照语义按源码不改。
/// 资源名与密度来自 kk_world::domain::stronghold 的官方奖励表。
fn calc_container_reward(w: &Working, id: ObjId, template: &StrongholdTemplate) -> Value {
    let target = stronghold::CONTAINER_AMOUNTS
        .get(template.reward_level.max(0) as usize)
        .copied()
        .unwrap_or(0);
    if target <= 0 {
        return json!({});
    }
    let mut rng = DeployRng::new(&w.room.name, "stronghold#reward", id, w.time);
    // 官方 _.shuffle(_.range(n)).slice(0, 3)：前缀 Fisher-Yates 洗牌。
    let n = stronghold::CONTAINER_REWARDS.len();
    let mut order: Vec<usize> = (0..n).collect();
    for i in 0..3.min(n) {
        let j = (i + (rng.unit() * ((n - i) as f64)).floor() as usize).min(n - 1);
        order.swap(i, j);
    }
    order.truncate(3.min(n));
    // 官方 densities[order.length-1]：位置下标（=2）查密度表，非末位资源。
    let last_divisor = stronghold::CONTAINER_REWARDS[order.len() - 1].1;
    let mut result = Map::new();
    let mut current = 0i64;
    for (i, &slot) in order.iter().enumerate() {
        let density = stronghold::CONTAINER_REWARDS[slot].1;
        let amount = if i + 1 == order.len() {
            ((target - current) as f64 / last_divisor as f64).round() as i64
        } else {
            (rng.unit() * (target - current) as f64 / density as f64).round() as i64
        }
        .max(0);
        result.insert(
            stronghold::CONTAINER_REWARDS[slot].0.to_string(),
            json!(amount),
        );
        current += amount * density;
    }
    Value::Object(result)
}

/// 官方按等级查表的通用取值（STRONGHOLD_RAMPART_HITS / RAMPART_HITS_MAX）。
fn table_value(table: &[(&str, i64)], key: i64) -> i64 {
    table
        .iter()
        .find(|(k, _)| *k == key.to_string())
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// 部署的常量上下文：模板档位、核心等级与共享截止/标识。
struct DeployParams<'a> {
    template: &'static StrongholdTemplate,
    core_level: i64,
    decay_time: u64,
    stronghold_id: Option<&'a str>,
}

/// 按官方 objectOptions 生成布防结构对象；effects 由调用方统一写入房间
/// 效果表（对象数据不保存 effects 数组）。road 使用专用变体，其余走
/// Generic JSON——与放置/建造完成入口的结构形态一致。
fn structure_object(
    kind: &str,
    x: u8,
    y: u8,
    params: &DeployParams<'_>,
    reward: Value,
) -> RoomObject {
    let mut data = json!({
        "type": "structure",
        "structureType": kind,
        "decayTime": params.decay_time,
    });
    if let Some(id) = params.stronghold_id {
        data["strongholdId"] = json!(id);
    }
    match kind {
        // rampart/tower 归属核心主人（官方 s.user = core.user）。
        "rampart" => {
            data["owner"] = json!(INVADER_OWNER);
            data["isPublic"] = json!(false);
            data["hits"] = json!(table_value(
                c::STRONGHOLD_RAMPART_HITS,
                params.template.reward_level
            ));
            data["hitsMax"] = json!(table_value(c::RAMPART_HITS_MAX, 8));
            data["hitsTarget"] = json!(table_value(c::STRONGHOLD_RAMPART_HITS, params.core_level));
            data["nextDecayTime"] = json!(params.decay_time);
        }
        "tower" => {
            data["owner"] = json!(INVADER_OWNER);
            data["hits"] = json!(c::TOWER_HITS);
            data["hitsMax"] = json!(c::TOWER_HITS);
            data["store"] = json!({ "energy": c::TOWER_CAPACITY });
            data["storeCapacity"] = json!(c::TOWER_CAPACITY);
            data["storeCapacityResource"] = json!({ "energy": c::TOWER_CAPACITY });
        }
        "container" => {
            data["notifyWhenAttacked"] = json!(false);
            data["hits"] = json!(c::CONTAINER_HITS);
            data["hitsMax"] = json!(c::CONTAINER_HITS);
            data["nextDecayTime"] = json!(params.decay_time);
            // 官方显式 storeCapacity: 0，奖励存量不受常规容量约束。
            data["storeCapacity"] = json!(0);
            data["store"] = reward;
        }
        // road 走专用变体（移动/衰减消费者按变体分派）；变体无数据袋，
        // strongholdId 不落 road（差异登记见台账）。
        "road" => {
            return RoomObject::Road(RoadObj {
                id: 0,
                x,
                y,
                hits: c::ROAD_HITS as u32,
                next_decay_time: params.decay_time,
                notify_when_attacked: Some(false),
            });
        }
        // 官方模板目前只含四种结构；未知类型按 Generic 落格但无默认值。
        _ => {}
    }
    RoomObject::Generic(GenericObj {
        id: 0,
        x,
        y,
        data: data.to_string(),
    })
}

/// 部署到期的清格：击杀格上已出生 creep、移除工地（进度折半掉能量）、
/// 摧毁有建造成本的玩家结构（建废墟，不发事件——官方 deploy 清格直呼
/// _destroy，事件由战斗路径的调用方发）。布防成员豁免：官方以对象
/// strongholdId 判定；kk 的布防结构以数据键（Generic）或坍塌效果条目
///（含同拍先铺的 road 变体）标识，孵化中 creep 以 spawning 形态近似
/// 保护与核心同格的孵化守军（差异登记见台账）。
fn clear_tile(w: &mut Working, x: u8, y: u8) {
    let ids: Vec<ObjId> = w.index.at(x as usize, y as usize).to_vec();
    for tid in ids {
        if is_bunker_member(w, tid) {
            continue;
        }
        match w.room.objects.get(&tid) {
            Some(RoomObject::Creep(creep)) => {
                if creep.spawning {
                    continue;
                }
                super::creep::die_creep(w, tid);
            }
            Some(RoomObject::ConstructionSite(site)) => {
                let progress = site.progress;
                w.remove_obj(tid);
                if progress > 1 {
                    w.create_resource(x, y, (progress / 2) as i64, "energy");
                }
            }
            Some(obj) => {
                let name = super::movement::type_name_of(obj);
                if kk_world::types::construction_cost(&name).is_some() {
                    super::damage::destroy_structure(w, tid);
                }
            }
            None => {}
        }
    }
}

/// 布防成员判定：Generic 数据带 strongholdId，或房间效果表已登记坍塌
/// 条目（同拍先铺的结构即时生效）。
fn is_bunker_member(w: &Working, tid: ObjId) -> bool {
    if w.room.effects.get(&tid).is_some_and(|entries| {
        entries
            .iter()
            .any(|e| e.effect == Some(c::EFFECT_COLLAPSE_TIMER))
    }) {
        return true;
    }
    matches!(w.room.objects.get(&tid), Some(RoomObject::Generic(g))
        if serde_json::from_str::<Value>(&g.data)
            .ok()
            .and_then(|d| d.get("strongholdId").cloned())
            .is_some_and(|v| !v.is_null()))
}

/// 部署执行：调用方已确认核心带模板名且 deployTime <= time+1。完成后
/// deployTime 置 null、写 decayTime 与坍塌效果；布防结构与核心共享
/// 同一截止时刻，到期由通用坍塌门统一移除。
pub(super) fn deploy(w: &mut Working, id: ObjId, data: &mut Value) -> Result<(), SettleError> {
    let Some(template) = data
        .get("templateName")
        .and_then(Value::as_str)
        .and_then(stronghold::template)
    else {
        return Ok(());
    };
    let Some(RoomObject::Generic(core)) = w.room.objects.get(&id) else {
        return Ok(());
    };
    let (cx, cy) = (core.x, core.y);
    let core_level = data.get("level").and_then(Value::as_i64).unwrap_or(0);
    let stronghold_id = data
        .get("strongholdId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let decay_time = w.time + decay_duration(w, id).max(0) as u64;
    let reward = calc_container_reward(w, id, template);
    let params = DeployParams {
        template,
        core_level,
        decay_time,
        stronghold_id: stronghold_id.as_deref(),
    };

    // 核心状态：deployTime 清空、decayTime/满血/坍塌效果随部署写入。
    data["deployTime"] = Value::Null;
    data["decayTime"] = json!(decay_time);
    data["hits"] = json!(c::INVADER_CORE_HITS);
    data["hitsMax"] = json!(c::INVADER_CORE_HITS);
    // 同 power 坍塌条目不重复：放置已写过时改写截止时刻。
    let entries = w.room.effects.entry(id).or_default();
    if let Some(entry) = entries
        .iter_mut()
        .find(|e| e.power == c::EFFECT_COLLAPSE_TIMER)
    {
        entry.end_time = decay_time;
    } else {
        entries.push(collapse_entry(decay_time));
    }

    // 官方先整体移除 Invader 归属的 rampart（布防重摆，无废墟）。
    let old_ramparts: Vec<ObjId> = w
        .room
        .objects
        .iter()
        .filter(|(_, obj)| match obj {
            RoomObject::Generic(g) => {
                let d: Value = serde_json::from_str(&g.data).unwrap_or(Value::Null);
                d.get("structureType").and_then(Value::as_str) == Some("rampart")
                    && d.get("owner").and_then(Value::as_str) == Some(INVADER_OWNER)
            }
            _ => false,
        })
        .map(|(rid, _)| *rid)
        .collect();
    for rid in old_ramparts {
        w.remove_obj(rid);
    }

    for entry in template.structures {
        let nx = cx as i32 + entry.dx as i32;
        let ny = cy as i32 + entry.dy as i32;
        if !(0..=49).contains(&nx) || !(0..=49).contains(&ny) {
            continue;
        }
        let (nx, ny) = (nx as u8, ny as u8);
        clear_tile(w, nx, ny);
        let obj = structure_object(entry.kind, nx, ny, &params, reward.clone());
        let new_id = w.insert_new(obj)?;
        w.room
            .effects
            .entry(new_id)
            .or_default()
            .push(collapse_entry(decay_time));
    }

    // 放置路径的孵化守军寿命绑定核心 decayTime（官方 create-creep 的
    // ageTime 同源语义）：布防成立后守军随据点共存亡。
    if let Some(name) = data
        .get("spawning")
        .filter(|v| !v.is_null())
        .and_then(|s| s.get("name"))
        .and_then(Value::as_str)
    {
        let chicks: Vec<ObjId> = w
            .room
            .objects
            .iter()
            .filter(|(_, obj)| match obj {
                RoomObject::Creep(c) => c.spawning && c.name == name && c.x == cx && c.y == cy,
                _ => false,
            })
            .map(|(cid, _)| *cid)
            .collect();
        for cid in chicks {
            if let Some(c) = w.creep_mut(cid) {
                c.age_time = decay_time;
            }
        }
    }
    Ok(())
}

fn collapse_entry(end_time: u64) -> kk_world::EffectEntry {
    kk_world::EffectEntry {
        effect: Some(c::EFFECT_COLLAPSE_TIMER),
        power: c::EFFECT_COLLAPSE_TIMER,
        level: 1,
        end_time,
    }
}
