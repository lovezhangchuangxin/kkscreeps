//! 处理 creep 的说话、自杀、拉拽和死亡资源转移。
//!
//! 死亡先移除 creep，再按剩余寿命回收身体成本，随后转移携带资源。
//! 同格容器先接收，剩余资源进入墓碑，最后产生死亡事件。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_SAY, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;

/// 已出生 creep 的说话文本截为 10 个 UTF-16 码元，并保存公开标志。
/// 来源：上游 engine/src/processor/intents/creeps/say.js。
pub(super) fn intent_say(w: &mut Working, id: ObjId, data: &IntentData) {
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    let message = utf16_truncate(data.message.as_deref().unwrap_or(""), 10);
    let is_public = data.is_public.unwrap_or(false);
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_SAY);
        c.say_message = Some(message);
        c.say_public = Some(is_public);
    }
}

/// 按 UTF-16 码元截断并分配 String。若截断拆开代理对，用 U+FFFD 替换孤立项；
/// Rust String 不能保留 JavaScript substring 可能产生的孤立代理项。
fn utf16_truncate(s: &str, units: usize) -> String {
    if s.encode_utf16().count() <= units {
        return s.to_string();
    }
    let cut: Vec<u16> = s.encode_utf16().take(units).collect();
    String::from_utf16_lossy(&cut)
}

/// 在指令阶段移除已出生 creep，已登记的移动不再执行。
/// 后续动作重新查找对象，因此也被跳过。上游在 suicide 后仍持有对象引用，
/// 部分后续动作可以继续影响其他对象；此处保留该兼容性差异。
/// 来源：上游 engine/src/processor/intents/creeps/suicide.js 和 intents.js。
pub(super) fn intent_suicide(w: &mut Working, id: ObjId) {
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    die_creep(w, id);
}

/// 为相邻且已出生的 creep 登记拉拽关系，循环链由 Movement 拒绝。
/// 被拉者还须自己提交 move；它的移动疲劳经 Working::add_fatigue 传给拉者。
/// 来源：上游 engine/src/processor/intents/creeps/pull.js。
pub(super) fn intent_pull(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (x, y) = match w.creep(id) {
        Some(c) if !c.spawning => (c.x as i32, c.y as i32),
        _ => return,
    };
    let (tx, ty) = match w.creep(target_id) {
        Some(t) if !t.spawning => (t.x as i32, t.y as i32),
        _ => return,
    };
    if (tx - x).abs() > 1 || (ty - y).abs() > 1 {
        return;
    }
    w.movement.add_pulling(w.room, id, target_id);
}

/// 按 CREEP_CORPSE_RATE 回收死亡 creep。TTL、战斗死亡和 suicide 使用此入口；
/// recycle 使用 die_creep_with_rate 并传 1.0。
pub fn die_creep(w: &mut Working, id: ObjId) {
    die_creep_with_rate(w, id, c::CREEP_CORPSE_RATE);
}

/// 移除本体，将部件回收资源和携带资源先放入同格容器，余量进入墓碑，
/// 最后记录死亡事件。drop_rate<=0 仍创建墓碑，但不转移资源。
/// 不模拟核弹抑制墓碑、NPC 特殊回收、userSummoned 或 strongholdId。
/// 来源：上游 engine/src/processor/intents/creeps/_die.js。
pub fn die_creep_with_rate(w: &mut Working, id: ObjId, drop_rate: f64) {
    let Some(c) = w.creep(id) else { return };
    let (x, y, owner, name, age_time, body, store, store_extra, creep_saying) = (
        c.x,
        c.y,
        c.owner.clone(),
        c.name.clone(),
        c.age_time,
        c.body.clone(),
        c.store.clone(),
        c.store_extra.clone(),
        if c.say_public == Some(true) {
            c.say_message.clone()
        } else {
            None
        },
    );
    let time = w.time;
    w.remove_obj(id);

    let decay_time = time + body.len() as u64 * c::TOMBSTONE_DECAY_PER_PART as u64;
    // 墓碑剩余寿命不取负值；传入已经过期的 creep 时饱和为 0。
    let ticks_to_live = age_time.saturating_sub(time);

    let mut tomb = TombStore::default();
    if drop_rate > 0.0 {
        // CLAIM 即使已损毁也使用较短寿命，以决定回收比例。
        let life_time = if body.iter().any(|p| p.kind == BodyKind::Claim.code()) {
            c::CREEP_CLAIM_LIFE_TIME
        } else {
            c::CREEP_LIFE_TIME
        } as f64;
        let life_rate = drop_rate * ticks_to_live as f64 / life_time;

        // 先累计部件造价与 boost 回收，按资源总额向下取整。
        // 容器容量有竞争时，energy 优先，矿物按身体中首次出现的顺序放入。
        let mut energy: f64 = 0.0;
        let mut minerals: Vec<(String, f64)> = Vec::new();
        for part in &body {
            if let Some(boost) = &part.boost {
                match minerals.iter_mut().find(|(res, _)| res == boost) {
                    Some((_, amount)) => *amount += c::LAB_BOOST_MINERAL as f64 * life_rate,
                    None => minerals.push((boost.clone(), c::LAB_BOOST_MINERAL as f64 * life_rate)),
                }
                energy += c::LAB_BOOST_ENERGY as f64 * life_rate;
            }
            let part_cost = BodyKind::from_code(part.kind)
                .and_then(|k| {
                    c::BODYPART_COST
                        .iter()
                        .find(|(name, _)| *name == k.name())
                        .map(|(_, v)| *v)
                })
                .unwrap_or(0) as f64;
            energy += (c::CREEP_PART_MAX_ENERGY as f64).min(part_cost * life_rate);
        }
        tomb.add(w, x, y, "energy", energy.floor() as i64);
        for (res, amount) in minerals {
            tomb.add(w, x, y, &res, amount.floor() as i64);
        }
        // 部件回收先占容器容量，之后才转移携带资源；两轮顺序会影响墓碑内容。
        tomb.add(w, x, y, "energy", store.energy as i64);
        tomb.add(
            w,
            x,
            y,
            kk_core::constants::RESOURCE_GHODIUM,
            store.ghodium as i64,
        );
        for stack in &store_extra {
            tomb.add(w, x, y, &stack.resource, stack.amount as i64);
        }
    }

    w.insert_new(RoomObject::Tombstone(kk_schema::room::TombstoneObj {
        id: 0,
        x,
        y,
        death_time: time,
        decay_time,
        creep_id: Some(id.0),
        creep_name: Some(name),
        creep_ticks_to_live: ticks_to_live.min(u32::MAX as u64) as u32,
        creep_body: body.iter().map(|p| p.kind).collect(),
        creep_saying,
        owner: Some(owner),
        store: tomb.store,
        store_extra: tomb.extra,
    }));

    w.push_event(
        c::EVENT_OBJECT_DESTROYED,
        id,
        serde_json::json!({ "type": "creep" }),
    );
}

/// 累计无法放入容器的墓碑资源，energy/G 使用固定槽，其余使用命名槽。
#[derive(Default)]
struct TombStore {
    store: kk_schema::room::Store,
    extra: Vec<kk_schema::room::ResourceStack>,
}

impl TombStore {
    /// 非正量不处理；每次先尝试容器，再将余量加入墓碑，保持调用顺序。
    fn add(&mut self, w: &mut Working, x: u8, y: u8, resource: &str, amount: i64) {
        if amount <= 0 {
            return;
        }
        let leftover = w.deposit_into_container(x, y, resource, amount);
        if leftover <= 0 {
            return;
        }
        match resource {
            "energy" => {
                self.store.energy = self
                    .store
                    .energy
                    .saturating_add(u32::try_from(leftover).unwrap_or(u32::MAX))
            }
            kk_core::constants::RESOURCE_GHODIUM => {
                self.store.ghodium = self
                    .store
                    .ghodium
                    .saturating_add(u32::try_from(leftover).unwrap_or(u32::MAX))
            }
            other => {
                if let Some(slot) = self.extra.iter_mut().find(|s| s.resource == other) {
                    slot.amount = slot
                        .amount
                        .saturating_add(u32::try_from(leftover).unwrap_or(u32::MAX));
                } else {
                    self.extra.push(kk_schema::room::ResourceStack {
                        resource: other.to_string(),
                        amount: u32::try_from(leftover).unwrap_or(u32::MAX),
                    });
                }
            }
        }
    }
}
