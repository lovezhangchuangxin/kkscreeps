//! 测试塔的距离功率和动作优先级、link 转移损耗与限制，以及 rampart 公开标志。

use super::*;
use serde_json::json;

/// 插入指定 JSON 数据的 Generic 结构。
fn place_generic(f: &mut Fixture, x: u8, y: u8, body: serde_json::Value) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: body.to_string(),
            }),
        )
        .unwrap()
}

fn tower(f: &mut Fixture, x: u8, y: u8, energy: i64) -> ObjId {
    place_generic(
        f,
        x,
        y,
        json!({
            "type": "structure", "structureType": "tower", "owner": "p1",
            "hits": 3000, "hitsMax": 3000, "nextDecayTime": 1_000_000,
            "store": {"energy": energy}, "storeCapacity": 1000,
            "storeCapacityResource": {"energy": 1000},
        }),
    )
}

fn link(f: &mut Fixture, x: u8, y: u8, energy: i64, cooldown: i64) -> ObjId {
    place_generic(
        f,
        x,
        y,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": energy}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
            "cooldown": cooldown,
        }),
    )
}

/// 读取当前 Generic 数据；rampart 的 hitsMax 会在每轮结算按房主和 RCL 同步。
fn data_of(f: &Fixture, id: ObjId) -> Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("Generic 不存在：{other:?}"),
    }
}

/// 官方 attack.js:32-38 距离衰减曲线三点：optimal（5 格）内全功率 600；
/// range 10 中段 600×(1−0.75×5/15)=450；range 25 钳 20 后底 150。
#[test]
fn tower_attack_falloff_near_mid_and_far() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let t_near = tower(&mut f, 10, 10, 100);
    let t_mid = tower(&mut f, 30, 30, 100);
    let t_far = tower(&mut f, 10, 40, 100);
    let near = f.creep("e1", "p2", 10, 15, &[BodyKind::Move; 8], 0);
    let mid = f.creep("e2", "p2", 40, 40, &[BodyKind::Move; 8], 0);
    let far = f.creep("e3", "p2", 35, 40, &[BodyKind::Move; 8], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t_near, Action::Attack, IntentData::target(near));
    batch.add_object_intent("p1", t_mid, Action::Attack, IntentData::target(mid));
    batch.add_object_intent("p1", t_far, Action::Attack, IntentData::target(far));
    f.settle(batch);
    // mid：t_mid(30,30)→(40,40) Chebyshev 10 → 600−600×0.75×5/15=450
    // far：t_far(10,40)→(35,40) Chebyshev 25 → 钳 20 → 600−450=150
    for (id, expect, label) in [
        (near, 200u32, "5 格全功率 600"),
        (mid, 350, "10 格 450"),
        (far, 650, "25 格钳 20 底 150"),
    ] {
        match f.world.get(id).map(|(_, o)| o) {
            Some(RoomObject::Creep(c)) => assert_eq!(c.hits, expect, "{label}"),
            other => panic!("目标存活：{other:?}"),
        }
    }
    for t in [t_near, t_mid, t_far] {
        assert_eq!(
            data_of(&f, t).pointer("/store/energy"),
            Some(&json!(90)),
            "各扣一次 TOWER_ENERGY_COST"
        );
    }
}

/// attack.js:33-36：目标同格 rampart 承伤（重定向后立即扣血，creep 无伤）。
#[test]
fn tower_attack_rampart_redirect() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let t = tower(&mut f, 10, 10, 100);
    let enemy = f.creep("e1", "p2", 12, 12, &[BodyKind::Move; 8], 0);
    let ramp = place_generic(
        &mut f,
        12,
        12,
        json!({
            "type": "structure", "structureType": "rampart", "owner": "p2",
            "hits": 5000, "hitsMax": 5000, "nextDecayTime": 1_000_000,
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t, Action::Attack, IntentData::target(enemy));
    f.settle(batch);
    match f.world.get(enemy).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => assert_eq!(c.hits, 800, "rampart 承伤，creep 无伤"),
        other => panic!("enemy 存活：{other:?}"),
    }
    assert_eq!(
        data_of(&f, ramp)["hits"],
        json!(5000 - 600),
        "range 2（optimal 内）全功率 600 立即扣"
    );
}

/// 同 tick 受到 600 伤害并治疗 400，目标初始 hits=300，最终 hits=100，仍应存活。
/// 本例能区分提前判死；因不触及 hitsMax，不能区分先伤后治与先治后伤。
#[test]
fn tower_heal_and_attack_pending_order() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let t_heal = tower(&mut f, 10, 10, 100);
    let t_atk = tower(&mut f, 11, 11, 100);
    let vic = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 12,
                y: 12,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "vic".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Move; 8]),
                store: kk_schema::room::Store::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t_heal, Action::Heal, IntentData::target(vic));
    batch.add_object_intent("p1", t_atk, Action::Attack, IntentData::target(vic));
    f.settle(batch);
    match f.world.get(vic).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => assert_eq!(
            c.hits, 100,
            "先伤害后治疗：300−600+400=100 存活（官方同 tick 序）"
        ),
        other => panic!("vic 存活：{other:?}"),
    }
}

/// towers/intents.js:3-8 else-if 序：heal 压制 repair/attack——同 tick 提交
/// heal+attack 两个 intent 只执行 heal。
#[test]
fn tower_single_action_heal_wins() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let t = tower(&mut f, 10, 10, 20);
    let heal_tgt = f.creep("h1", "p1", 11, 11, &[BodyKind::Move; 8], 0);
    let atk_tgt = f.creep("e1", "p2", 12, 12, &[BodyKind::Move; 8], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t, Action::Heal, IntentData::target(heal_tgt));
    batch.add_object_intent("p1", t, Action::Attack, IntentData::target(atk_tgt));
    f.settle(batch);
    assert_eq!(
        data_of(&f, t).pointer("/store/energy"),
        Some(&json!(10)),
        "只扣一次能量"
    );
    match f.world.get(atk_tgt).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => assert_eq!(c.hits, 800, "attack 被 else-if 压制不执行"),
        other => panic!("atk_tgt 存活：{other:?}"),
    }
}

/// repair.js:10-11：CONSTRUCTION_COST 门 + 满血拒；:30-33 回补钳 hitsMax。
#[test]
fn tower_repair_clamps_and_gates() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let t = tower(&mut f, 10, 10, 100);
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                id: 0,
                x: 14,
                y: 14,
                hits: 1000,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let creep = f.creep("c1", "p1", 15, 15, &[BodyKind::Move; 8], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t, Action::Repair, IntentData::target(wall));
    f.settle(batch);
    match f.world.get(wall).map(|(_, o)| o) {
        Some(RoomObject::ConstructedWall(w)) => {
            assert_eq!(w.hits, 1800, "1000+800（optimal 内全功率）")
        }
        other => panic!("wall 存活：{other:?}"),
    }
    // creep 目标：CONSTRUCTION_COST 门拒——能量不扣、无事件
    let before = data_of(&f, t).pointer("/store/energy").cloned();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t, Action::Repair, IntentData::target(creep));
    f.settle(batch);
    assert_eq!(
        data_of(&f, t).pointer("/store/energy"),
        before.as_ref(),
        "非可修结构整单拒（能量不动）"
    );
}

/// 验证 link 的 3% 损耗、按距离增加冷却和目标余量限制。
/// 意图设置冷却后，对象 tick 在同轮减 1。来源：上游 links/transfer.js 和 links/tick.js。
#[test]
fn link_transfer_math_cooldown_and_clamp() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // rcl6 配额 3：本用例三座 link（l1/l2/src）须全部 active，避免配额门
    // 误伤（第一段 transfer 的拒因须是本用例所测的满仓截断，非配额）
    f.controller(6, Some("p1"));
    let l1 = link(&mut f, 10, 10, 400, 0);
    let l2 = link(&mut f, 20, 10, 0, 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(100),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle(batch);
    let d1 = data_of(&f, l1);
    assert_eq!(d1.pointer("/store/energy"), Some(&json!(300)), "源扣量");
    // 冷却 = |20−10|×1=10，随后**同 tick** 的 phase 8 links/tick.js 递减 1
    //（官方序：intent 期挂钟 → phase 8 减——结算后净读 9）
    assert_eq!(
        d1["cooldown"],
        json!(9),
        "冷却净读 = 距离 10 − 同 tick 递减 1"
    );
    let d2 = data_of(&f, l2);
    assert_eq!(
        d2.pointer("/store/energy"),
        Some(&json!(97)),
        "100−ceil(100×0.03)=97"
    );
    // 下一 tick 继续递减冷却。
    f.settle(IntentBatch::default());
    assert_eq!(data_of(&f, l1)["cooldown"], json!(8));

    // 满仓截断：l2 已 97（余量 703）→ 请求 800 截到 703；
    // l2 = 97+703−ceil(703×0.03)=800−22=778；src 扣 703
    let src = link(&mut f, 30, 10, 1000, 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        src,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(800),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle(batch);
    assert_eq!(
        data_of(&f, l2).pointer("/store/energy"),
        Some(&json!(778)),
        "余量 703 截断后 97+703−ceil(703×0.03)=778"
    );
    assert_eq!(
        data_of(&f, src).pointer("/store/energy"),
        Some(&json!(297)),
        "源按截断后的量扣（1000−703）"
    );
}

/// transfer.js 的四个拒绝门：RCL 配额（rcl4 link 允许 0）、源冷却、目标满、
/// 源能量不足——整单拒（双侧不动、无事件）。
#[test]
fn link_transfer_reject_gates() {
    // RCL 4：link 配额 0 → checkStructureAgainstController 拒
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(4, Some("p1"));
    let l1 = link(&mut f, 10, 10, 400, 0);
    let l2 = link(&mut f, 20, 10, 0, 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(100),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle(batch);
    assert_eq!(
        data_of(&f, l1).pointer("/store/energy"),
        Some(&json!(400)),
        "RCL 配额 0 整单拒"
    );

    // 其他拒绝条件使用新的 Fixture，避免两个 controller 同房时始终读取第一个。
    // 每种情况都满足其余条件，使拒绝结果能够归因于被测检查。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(8, Some("p1"));
    let src_cd = link(&mut f, 10, 30, 400, 5);
    let dst0 = link(&mut f, 20, 30, 0, 0);
    transfer(&mut f, src_cd, dst0, 100);
    assert_eq!(
        data_of(&f, src_cd).pointer("/store/energy"),
        Some(&json!(400)),
        "冷却拒：能量不动"
    );
    assert_eq!(data_of(&f, dst0).pointer("/store/energy"), Some(&json!(0)));
    // 冷却拒后净读 5−1：intent 被拒不叠加，但 phase 8 links/tick 照常递减
    assert_eq!(
        data_of(&f, src_cd)["cooldown"],
        json!(4),
        "冷却拒：不叠加（仅 phase 8 常规递减 5→4）"
    );

    // 目标已满，源有足量能量且不在冷却，只验证目标容量检查。
    let src_ok = link(&mut f, 10, 40, 400, 0);
    let dst_full = link(&mut f, 20, 40, 800, 0);
    transfer(&mut f, src_ok, dst_full, 100);
    assert_eq!(
        data_of(&f, src_ok).pointer("/store/energy"),
        Some(&json!(400)),
        "目标满拒：源不动（结算层 FULL，非 API 预检）"
    );
    assert_eq!(
        data_of(&f, dst_full).pointer("/store/energy"),
        Some(&json!(800))
    );
    // 只检查能量不变无法区分满仓拒绝和数量钳到 0；必须再检查没有增加冷却。
    assert_eq!(
        data_of(&f, src_ok)["cooldown"],
        json!(0),
        "满仓拒：不挂冷却（与 clamp-0 路径的分界）"
    );

    // ③源能量不足：store.energy(50) < amount(100)（官方守卫链首门
    //   transfer.js:7-9——先于配额门，独立承重）
    let src_poor = link(&mut f, 10, 49, 50, 0);
    transfer(&mut f, src_poor, dst0, 100);
    assert_eq!(
        data_of(&f, src_poor).pointer("/store/energy"),
        Some(&json!(50)),
        "能量不足拒"
    );
    assert_eq!(
        data_of(&f, dst0).pointer("/store/energy"),
        Some(&json!(0)),
        "三次全拒目标不动"
    );
    assert_eq!(
        data_of(&f, src_poor)["cooldown"],
        json!(0),
        "能量不足拒：不挂冷却"
    );
}

/// link transfer intent 的构造捷径（三个拒因用例共用）。
fn transfer(f: &mut Fixture, src: ObjId, dst: ObjId, amount: i64) {
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        src,
        Action::Transfer,
        IntentData {
            id: Some(dst),
            amount: Some(amount),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle(batch);
}

/// ramparts/set-public.js：isPublic 布尔写回。
#[test]
fn rampart_set_public_write() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let ramp = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "rampart", "owner": "p1",
            "hits": 5000, "hitsMax": 5000, "nextDecayTime": 1_000_000,
            "isPublic": false,
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        ramp,
        Action::SetPublic,
        IntentData {
            is_public: Some(true),
            ..IntentData::bare()
        },
    );
    f.settle(batch);
    assert_eq!(data_of(&f, ramp)["isPublic"], json!(true));
}

/// 官方 processor.js:248 所有权门：p1 对 p2 的塔提交 attack intent → 跳过。
#[test]
fn structure_intent_ownership_gate() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p2"));
    let t = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "tower", "owner": "p2",
            "hits": 3000, "hitsMax": 3000, "nextDecayTime": 1_000_000,
            "store": {"energy": 100},
        }),
    );
    let enemy = f.creep("c1", "p1", 11, 11, &[BodyKind::Move; 8], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t, Action::Attack, IntentData::target(enemy));
    f.settle(batch);
    match f.world.get(enemy).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => assert_eq!(c.hits, 800, "他人结构 intent 跳过"),
        other => panic!("enemy 存活：{other:?}"),
    }
    assert_eq!(
        data_of(&f, t).pointer("/store/energy"),
        Some(&json!(100)),
        "能量不动"
    );
}

/// 同一 link 自转移只损失 ceil(amount*0.03) 能量，距离为 0，不增加冷却。
/// 分别写回源、目标旧快照会覆盖接收量，错误地扣除全部 amount。
#[test]
fn link_transfer_self_target_converges() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(8, Some("p1"));
    let solo = link(&mut f, 10, 10, 400, 0);
    transfer(&mut f, solo, solo, 100);
    let d = data_of(&f, solo);
    assert_eq!(
        d.pointer("/store/energy"),
        Some(&json!(397)),
        "自指净收敛 400−ceil(100×0.03)=397（非 400−100=300）"
    );
    assert_eq!(
        d["cooldown"],
        json!(0),
        "冷却 += LINK_COOLDOWN×|10−10|=+0（自指距离 0；links/tick 对 0 不减）"
    );
}

/// 接近满血时修复量钳到 hitsMax，已满血时不再扣能量。
#[test]
fn tower_repair_clamps_to_hitsmax_and_rejects_full() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let t = tower(&mut f, 10, 10, 100);
    let damaged = link(&mut f, 12, 12, 0, 0); // hits 1000（缺省 LINK_HITS_MAX）
    place_generic_field(&mut f, damaged, "hits", 900);
    let full = link(&mut f, 13, 13, 0, 0); // hits 1000 = hitsMax
    repair_intent(&mut f, t, damaged);
    assert_eq!(
        data_of(&f, damaged)["hits"],
        json!(1000),
        "900+800 钳到 hitsMax 1000（`.min(hits_max)` 分支）"
    );
    assert_eq!(
        data_of(&f, t).pointer("/store/energy"),
        Some(&json!(90)),
        "钳制路径照常扣能量"
    );
    repair_intent(&mut f, t, full);
    assert_eq!(data_of(&f, full)["hits"], json!(1000), "满血不动");
    assert_eq!(
        data_of(&f, t).pointer("/store/energy"),
        Some(&json!(90)),
        "满血拒：能量不动（`hits >= hitsMax` 分支）"
    );
}

/// 同时提交 heal 和 repair，只执行 heal；墙耐久必须不变。
#[test]
fn tower_single_action_heal_beats_repair() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let t = tower(&mut f, 10, 10, 20);
    let heal_tgt = f.creep("h1", "p1", 11, 11, &[BodyKind::Move; 8], 0);
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                id: 0,
                x: 12,
                y: 12,
                hits: 1000,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t, Action::Heal, IntentData::target(heal_tgt));
    batch.add_object_intent("p1", t, Action::Repair, IntentData::target(wall));
    f.settle(batch);
    match f.world.get(wall).map(|(_, o)| o) {
        Some(RoomObject::ConstructedWall(w)) => {
            assert_eq!(w.hits, 1000, "repair 被 else-if 压制不执行")
        }
        other => panic!("wall 存活：{other:?}"),
    }
    assert_eq!(
        data_of(&f, t).pointer("/store/energy"),
        Some(&json!(10)),
        "heal 单次扣能"
    );
}

/// repair intent 的构造捷径。
fn repair_intent(f: &mut Fixture, t: ObjId, target: ObjId) {
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", t, Action::Repair, IntentData::target(target));
    f.settle(batch);
}

/// 覆写 Generic 夹具的单字段（hits 微调用）。
fn place_generic_field(f: &mut Fixture, id: ObjId, key: &str, value: i64) {
    if let Some(RoomObject::Generic(g)) = f.world.get_mut(id) {
        let mut v: Value = serde_json::from_str(&g.data).unwrap();
        v[key] = json!(value);
        g.data = v.to_string();
    }
}
