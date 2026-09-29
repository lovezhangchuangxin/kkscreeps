//! 生命周期族：say/suicide/pull 杂项与 creep 死亡落墓（die/decay 路径、
//! TombStore 转储）。B1 结构治理批自 creep.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_SAY, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;

// ---- A4-iv：移动杂项族执行器（官方 processor/intents/creeps/ 同名文件） ----

/// 官方 say.js：spawning 拒后落 actionLog.say。官方键携带 message（截 10）与
/// isPublic——u16 位图只落事件位（ACTION_SAY），文本载荷不可承载：prelude 的
/// `saying` getter 读不到消息文本（恒 undefined），与 healType 的 B1.1 登记
/// 同口径（schema 换正式 actionLog 形态时恢复）。message 的 isString 门
///（say.js:14-16）：清洗层 userString 后官方恒字符串（缺省折叠 ""），我方
/// None 按 "" 对齐。
pub(super) fn intent_say(w: &mut Working, id: ObjId, data: &IntentData) {
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    let _message = data.message.as_deref().unwrap_or("");
    let _is_public = data.is_public.unwrap_or(false);
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_SAY);
    }
}

/// 官方 suicide.js：intent 相即死（_die.js，dropRate=CREEP_CORPSE_RATE、
/// violentDeath=false——B3 起 die_creep_with_rate 即官方 _die.js 全量，
/// TTL/暴亡/suicide 0.2、recycle 1.0 四路 dropRate 对位）。user=='2'
///（Source Keeper dropRate=0）不建模——NPC 不入 M2 世界。
/// 死亡发生在 intent 相（官方同位）；已入矩阵的 move 不再落格（官方 execute
/// 经 roomObjects forEach，死对象不被遍历——同形）。
/// **有意分歧（A4-iv 评审 1/3 双确认登记）**：官方后序处理器（claim/
/// upgrade/.../pull，creepActions 序 suicide 之后）持有死引用快照继续执行
/// 且对**其他对象**的效果落地（processor.js:241 先取引用、_die.js:12-13 只
/// delete 集合不清引用、各处理器无 actor 存活复检）——`suicide();
/// upgradeController(c)` 同 tick 官方升级照常生效。我方对象消亡后
/// w.creep(id)→None 全早退（对其他对象的效果也跳过）。实装需处理器持
/// pre-death 快照（侵入面大），真实 bot 罕见该组合——登记有意分歧。
pub(super) fn intent_suicide(w: &mut Working, id: ObjId) {
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    die_creep(w, id);
}

/// 官方 pull.js：目标 creep 存在、已出生、邻接 → movement.addPulling（配对
/// _pull/_pulled，环链拒绝）。位移的另一半是被拉 creep 自己的 move(puller)
/// intent（move 的朝相邻 creep 形态，intent_move 的 id 分支）——仲裁期
/// canMove 对被拉者放行（免疲劳/MOVE）、其移动疲劳沿链记在拉者头上
///（Working::add_fatigue 的归链）。
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
    // pull.js:16-18：邻接复核（API 层 isNearTo 的结算侧对照）
    if (tx - x).abs() > 1 || (ty - y).abs() > 1 {
        return;
    }
    w.movement.add_pulling(w.room, id, target_id);
}

/// creep 死亡（官方 _die.js 全量，B3）：移除本体 → 墓碑承接全部资源
/// （部件造价回收 + 携带 store，同格 container 先收）→ EVENT_OBJECT_DESTROYED。
/// 四条 M2 死因合流：TTL 到期（tick.js:93-95）、部件全灭的暴亡
/// （tick.js:134-136）、suicide（suicide.js:14）——官方均传 dropRate=undefined
/// （落 CREEP_CORPSE_RATE=0.2，走 [`die_creep`]）；recycleCreep
/// （recycle-creep.js:23）传 1.0（走 [`die_creep_with_rate`]）。
/// attackType 恒 undefined（墓碑抑制分支是核弹专属，M2 无核弹路径）。
///
/// 不建模面（官方 NPC/宿主体系，登记报告）：userSummoned/strongholdId 跳过
/// 回收的门（:38）；tombstoneDecay 覆盖键（:18——OSS 引擎全库无写入点）；
/// Source Keeper 的 dropRate=0（suicide.js:14 的 user=='2'）；stats.creepsLost
/// （:99-101 宿主统计面）。
pub fn die_creep(w: &mut Working, id: ObjId) {
    die_creep_with_rate(w, id, c::CREEP_CORPSE_RATE);
}

pub fn die_creep_with_rate(w: &mut Working, id: ObjId, drop_rate: f64) {
    let Some(c) = w.creep(id) else { return };
    let (x, y, owner, name, age_time, body, store, store_extra) = (
        c.x,
        c.y,
        c.owner.clone(),
        c.name.clone(),
        c.age_time,
        c.body.clone(),
        c.store.clone(),
        c.store_extra.clone(),
    );
    let time = w.time;
    // _die.js:10-11：先移除本体
    w.remove_obj(id);

    // _die.js:16-19：decayTime = body.length × TOMBSTONE_DECAY_PER_PART
    let decay_time = time + body.len() as u64 * c::TOMBSTONE_DECAY_PER_PART as u64;
    // processor.js:50：_ticksToLive = ageTime − gameTime（tick 首拍维护，死亡
    // 时点读到的即本值）
    let ticks_to_live = age_time.saturating_sub(time);

    // ---- 资源承接（_die.js:37-91；:38 `dropRate > 0` 门包住两循环——0 时
    // 跳过回收与携带承接，墓碑仍建（空 store）） ----
    // 墓碑 store 累计器（官方 tombstone.store 的内存对象）：energy/G 双槽 +
    // 额外资源列表；每次入账先经同格 container 承接（:45-59/:79-90 的两段
    // 同形分支）
    let mut tomb = TombStore::default();
    if drop_rate > 0.0 {
        // :40-41 lifeTime 按 body 是否含 CLAIM 部件（_.any 不看 hits——含损毁件）
        let life_time = if body.iter().any(|p| p.kind == BodyKind::Claim.code()) {
            c::CREEP_CLAIM_LIFE_TIME
        } else {
            c::CREEP_LIFE_TIME
        } as f64;
        // :42 lifeRate = dropRate × _ticksToLive / lifeTime
        let life_rate = drop_rate * ticks_to_live as f64 / life_time;

        // :43-53 部件造价回收：energy 起步累加；boosted 件额外产出 boost 矿物
        //（LAB_BOOST_MINERAL × lifeRate）与能量（LAB_BOOST_ENERGY × lifeRate）；
        // 每件能量 = min(CREEP_PART_MAX_ENERGY, BODYPART_COST × lifeRate)。
        // 官方键序 = bodyResources 的插入序（energy 先、矿物按 body 首现序）——
        // 仅 container 余量竞争时可观察，按同序遍历
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
        // :61-76 第一循环（bodyResources：资源总额 floor 后逐键入账）
        tomb.add(w, x, y, "energy", energy.floor() as i64);
        for (res, amount) in minerals {
            tomb.add(w, x, y, &res, amount.floor() as i64);
        }
        // :78-91 第二循环（object.store 携带资源——官方开放 store 逐键循环；
        // B4 起含命名矿物槽的全集）
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

    // _die.js:22-34/93：墓碑落库。creepSaying 官方取 actionLog.say 的公开
    // 遗言——位图 action_log 无文本载荷，恒 None（灰区表既有 say 登记的延伸）
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
        creep_saying: None,
        owner: Some(owner),
        store: tomb.store,
        store_extra: tomb.extra,
    }));

    // _die.js:97：死亡事件（墓碑先落、事件在后——官方同序）
    w.push_event(
        c::EVENT_OBJECT_DESTROYED,
        id,
        serde_json::json!({ "type": "creep" }),
    );
}

/// 墓碑 store 的入账累计器（官方 _die.js 的 tombstone.store 内存对象 +
/// 两段 container 承接分支的 Rust 形态）。
#[derive(Default)]
struct TombStore {
    store: kk_schema::room::Store,
    extra: Vec<kk_schema::room::ResourceStack>,
}

impl TombStore {
    /// 一次资源入账：同格 container 先承接（余量竞争按入账序——官方两循环
    /// 的 per-key 时序），余量进墓碑槽。amount ≤ 0 无事发生（官方 floor 后
    /// `if(amount > 0)` 门）。
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
