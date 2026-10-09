//! 测试矿物再生与采集、强化建造、命名资源槽和携带量对移动的影响。

use super::*;

#[test]
fn sign_settlement_persists_datetime() {
    // 签名 datetime 在结算时保存，单位为 Unix 毫秒，不由查询 getter 临时生成。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.id = Some(ctl);
    d.sign = Some("hello".into());
    b.add_object_intent("p1", c, Action::SignController, d);
    f.settle(b);
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            let s = t.sign.as_ref().expect("签名落库");
            assert!(
                s.datetime.is_some(),
                "datetime 结算时落一次（非 getter 合成）"
            );
        }
        _ => panic!(),
    }
}

#[test]
fn boosted_build_two_step_official_values() {
    // 强化建造分开计算能耗与进度：单 WORK 使用 5 能量，
    // LH 倍率 1.5 得 7 进度，XLH2O 倍率 2 得 10 进度。来源：上游 creeps/build.js。
    for (boost, want_progress) in [("LH", 7), ("XLH2O", 10)] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let site = f
            .world
            .insert(
                "W1N1",
                RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                    id: 0,
                    x: 10,
                    y: 10,
                    structure_type: "road".into(),
                    progress: 0,
                    progress_total: 100,
                    owner: Some("p1".into()),
                    name: None,
                }),
            )
            .unwrap();
        let c = f
            .world
            .insert(
                "W1N1",
                RoomObject::Creep(CreepObj {
                    id: 0,
                    x: 10,
                    y: 11,
                    fatigue: 0,
                    age_time: 9_000,
                    hits: 100,
                    spawning: false,
                    action_log: 0,
                    name: "w".into(),
                    owner: "p1".into(),
                    body: vec![kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: Some(boost.into()),
                    }],
                    store: kk_schema::room::Store {
                        energy: 5,
                        ghodium: 0,
                    },
                    store_extra: Vec::new(),
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
                }),
            )
            .unwrap();
        let _ = site;
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
        f.settle(b);
        let progress = f
            .world
            .rooms()
            .get("W1N1")
            .unwrap()
            .objects
            .get(&site)
            .map(|o| match o {
                RoomObject::ConstructionSite(s) => s.progress,
                _ => 0,
            })
            .unwrap_or(0);
        let energy = match f.obj(c) {
            RoomObject::Creep(cc) => cc.store.energy,
            _ => 0,
        };
        assert_eq!(progress as i64, want_progress, "{boost} 进度");
        assert_eq!(energy, 0, "{boost} 能量扣基础量 5");
    }
}

#[test]
fn transfer_mixed_holdings_respect_total_capacity() {
    // 共享容量按全资源总量计算；storage 的 energy 占用也限制 G 接收量。
    // 来源：上游 creeps/transfer.js 的 calcResources。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let st = f
        .world
        .insert(
            "W1N1",
            RoomObject::Storage(kk_schema::room::StorageObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 10_000,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 999_990,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let c1 = f.creep("w1", "p1", 21, 20, &[BodyKind::Move, BodyKind::Carry], 0);
    f.world
        .edit_object(c1, |object| {
            if let Some(RoomObject::Creep(cc)) = Some(object) {
                cc.store.ghodium = 20;
            }
            Ok(())
        })
        .unwrap();
    // 单笔 20G：余量 1e6-999990=10 ⇒ 只收 10
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.id = Some(st);
    d.amount = Some(20);
    d.resource_type = Some("G".into());
    b.add_object_intent("p1", c1, Action::Transfer, d);
    f.settle(b);
    match f.obj(st) {
        RoomObject::Storage(s) => {
            assert_eq!(s.store.energy, 999_990);
            assert_eq!(s.store.ghodium, 10, "全槽总量钳制：余量 10 G");
        }
        _ => panic!(),
    }
    match f.obj(c1) {
        RoomObject::Creep(cc) => assert_eq!(cc.store.ghodium, 10, "退回未收的 10"),
        _ => panic!(),
    }
}

#[test]
fn pickup_ghodium_routes_to_ghodium_slot() {
    // 官方 pickup.js:9/23-28：carry 全槽求和、按堆 resourceType 入槽
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 30);
    let pile = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 11,
                resource_type: 2,
                amount: 10,
                resource: None,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Pickup, IntentData::target(pile));
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!(cc.store.energy, 30, "energy 槽不动");
            assert_eq!(cc.store.ghodium, 10, "G 入 ghodium 槽（GPT 二轮 P1-3）");
        }
        _ => panic!(),
    }
    assert!(f.world.get(pile).is_none(), "堆清零移除");
}

/// minerals/tick.js:8-18：矿量耗尽首见挂钟 gameTime+MINERAL_REGEN_TIME；
/// 到点（gameTime >= next-1）按**旧** density 查 MINERAL_DENSITY 回填并清钟。
/// 密度轮转（:19-34）：LOW/ULTRA 强制、其余 5%；do-while 拒绝同值。
#[test]
fn mineral_regeneration_clock_and_density_rotation() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 首见耗尽：挂钟（time 0 → 50000），矿量保持 0
    let m = f.mineral(10, 10, "U", 0, 2, None);
    f.settle(IntentBatch::default()); // time 0→1
    match f.obj(m) {
        RoomObject::Mineral(mm) => {
            assert_eq!(mm.next_regeneration_time, Some(50_000), "首见挂钟 0+50000");
            assert_eq!(mm.mineral_amount, 0, "未到点不回填");
        }
        _ => panic!(),
    }
    // 把钟拨近（等价很久以前挂的钟——避免 5 万 tick）：MODERATE(2) 起步
    let t = f.world.time();
    f.world
        .edit_object(m, |object| {
            if let Some(RoomObject::Mineral(mm)) = Some(object) {
                mm.next_regeneration_time = Some(t + 2); // time 1 → 3
            }
            Ok(())
        })
        .unwrap();
    f.settle(IntentBatch::default()); // time 1：1 >= 2? 否 → 不动
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 0, "差一 tick 不回填"),
        _ => panic!(),
    }
    let events = f.settle(IntentBatch::default()); // time 2：2 >= 3-1 → 回填
    assert!(
        events.is_empty(),
        "再生无事件（官方 minerals/tick 不产事件）"
    );
    let moderate_outcome = match f.obj(m) {
        RoomObject::Mineral(mm) => {
            assert_eq!(mm.mineral_amount, 35_000, "MINERAL_DENSITY[2]=35000");
            assert_eq!(mm.next_regeneration_time, None, "钟清");
            mm.density
        }
        _ => panic!(),
    };
    // replay 确定性：同世界状态重放必得同密度（官方 Math.random 不可复现——
    // 灰区表口径，我方无状态种子链保证 replay）
    let mut f2 = Fixture::new(7, terrain_with(&[]));
    let m2 = f2.mineral(10, 10, "U", 0, 2, None);
    f2.settle(IntentBatch::default());
    let t = f2.world.time();
    f2.world
        .edit_object(m2, |object| {
            if let Some(RoomObject::Mineral(mm)) = Some(object) {
                mm.next_regeneration_time = Some(t + 2);
            }
            Ok(())
        })
        .unwrap();
    f2.settle(IntentBatch::default());
    f2.settle(IntentBatch::default());
    match f2.obj(m2) {
        RoomObject::Mineral(mm) => assert_eq!(
            mm.density, moderate_outcome,
            "确定性：seed 不同但世界状态同 ⇒ 同轮转结果"
        ),
        _ => panic!(),
    }
    // LOW(1) 强制轮转（do-while 保证必变）；回填量按旧 density=15000
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let m3 = f3.mineral(12, 12, "K", 0, 1, None);
    f3.settle(IntentBatch::default());
    let t = f3.world.time();
    f3.world
        .edit_object(m3, |object| {
            if let Some(RoomObject::Mineral(mm)) = Some(object) {
                mm.next_regeneration_time = Some(t + 1);
            }
            Ok(())
        })
        .unwrap();
    f3.settle(IntentBatch::default());
    match f3.obj(m3) {
        RoomObject::Mineral(mm) => {
            assert_eq!(
                mm.mineral_amount, 15_000,
                "MINERAL_DENSITY[1]=15000（旧密度）"
            );
            assert_ne!(mm.density, 1, "LOW 强制轮转（do-while 拒绝同值）");
            assert!((2..=4).contains(&mm.density));
        }
        _ => panic!(),
    }
}

/// 官方 nextRegenerationTime 真值判（minerals/tick.js `if(!object.
/// nextRegenerationTime)`）：时钟 0 视同未设置——不立即回填，挂新钟。
#[test]
fn mineral_zero_clock_is_treated_as_unset_for_natural_regen() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let m = f.mineral(10, 10, "U", 0, 2, Some(0));
    f.settle(IntentBatch::default()); // time 0
    match f.obj(m) {
        RoomObject::Mineral(mm) => {
            assert_eq!(mm.next_regeneration_time, Some(50_000), "0 与未设置等价");
            assert_eq!(mm.mineral_amount, 0, "不因 0 钟立即回填");
        }
        _ => panic!(),
    }
}

/// 自然再生当拍与在位 REGEN_MINERAL 效果叠加（官方 bulk.update 以对象引用
/// 调用时 `_.merge` 回写内存，效果门读回填后的新值）；相位不命中不叠。
/// 该状态正常玩家序列不可达（施放门拒耗尽矿物、耗尽挂 50000 钟），由
/// 装填/导入数据构造，锁定官方同拍语义。
#[test]
fn mineral_regen_effect_stacks_on_natural_regeneration_tick() {
    // 相位命中：end_time 11 在 time 0 满足 (11-0-1)%10==0，回填 35000 后 +6。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let m = f.mineral(10, 10, "U", 0, 2, Some(1)); // time 0 >= 1-1 → 回填拍
    f.world
        .edit_room("W1N1", |room| {
            room.effects.insert(
                m,
                vec![kk_world::EffectEntry {
                    effect: Some(14),
                    power: 14,
                    level: 3,
                    end_time: 11,
                }],
            );
            Ok(())
        })
        .unwrap();
    f.settle(IntentBatch::default()); // time 0
    match f.obj(m) {
        RoomObject::Mineral(mm) => {
            assert_eq!(mm.mineral_amount, 35_006, "回填 35000 叠加等级 3 补量 6");
            assert_eq!(mm.next_regeneration_time, None);
        }
        _ => panic!(),
    }
    // 相位不命中：end_time 12 → (12-0-1)%10==1，只回填不叠。
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let m2 = f2.mineral(10, 10, "U", 0, 2, Some(1));
    f2.world
        .edit_room("W1N1", |room| {
            room.effects.insert(
                m2,
                vec![kk_world::EffectEntry {
                    effect: Some(14),
                    power: 14,
                    level: 3,
                    end_time: 12,
                }],
            );
            Ok(())
        })
        .unwrap();
    f2.settle(IntentBatch::default());
    match f2.obj(m2) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 35_000, "相位不命中不叠加"),
        _ => panic!(),
    }
}

/// harvest.js mineral 分支（67-112）全链：矿量/邻接/extractor 存在性/归属/
/// RCL 门/cooldown、HARVEST_MINERAL_POWER、产出入命名槽、事件 amount 是
/// 未钳制的 harvestAmount（:110 官方 quirk）、extractor cooldown 六 tick 周期。
#[test]
fn harvest_mineral_named_slot_and_cooldown_cycle() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let m = f.mineral(10, 10, "U", 70_000, 3, None);
    let ex = f.extractor(10, 10, Some("p1"), 0);
    let c = f.creep(
        "w1",
        "p1",
        10,
        11,
        &[BodyKind::Work, BodyKind::Work, BodyKind::Carry],
        0,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    let events = f.settle(b); // T0：cooldown 0 → 放行；同 tick 持久化 5
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_HARVEST);
    assert_eq!(events[0]["data"]["targetId"], m.as_str());
    assert_eq!(
        events[0]["data"]["amount"], 2,
        "2 WORK × HARVEST_MINERAL_POWER(1)"
    );
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 69_998),
        _ => panic!(),
    }
    match f.obj(c) {
        RoomObject::Creep(cr) => {
            assert_eq!(cr.store.energy, 0, "不洗白进 energy");
            assert_eq!(
                cr.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "U".into(),
                    amount: 2,
                }],
                "产出入矿种命名槽（harvest.js:95）"
            );
        }
        _ => panic!(),
    }
    match f.obj(ex) {
        RoomObject::Extractor(e) => assert_eq!(e.cooldown, 5, "harvest 瞬态同 tick 持久化"),
        _ => panic!(),
    }
    // T1-T5：cooldown 拒采并逐 tick 递减（5→4→3→2→1→0）
    for expect in [4u64, 3, 2, 1, 0] {
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
        f.settle(b);
        match f.obj(ex) {
            RoomObject::Extractor(e) => assert_eq!(e.cooldown, expect),
            _ => panic!(),
        }
        match f.obj(m) {
            RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 69_998, "冷却期不采"),
            _ => panic!(),
        }
    }
    // T6：cooldown 归零 → 可再采（六 tick 周期，EXTRACTOR_COOLDOWN=5）
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(b);
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 69_996),
        _ => panic!(),
    }
    // 官方 quirk 锁：矿量 < 功率时事件 amount 仍是未钳制的 harvestAmount
    //（T6 采集又挂了 cooldown=5——拨回 0 聚焦本断言，冷却周期已由上文锁定）
    f.world
        .edit_object(m, |object| {
            if let Some(RoomObject::Mineral(mm)) = Some(object) {
                mm.mineral_amount = 1;
                mm.next_regeneration_time = None;
            }
            Ok(())
        })
        .unwrap();
    f.world
        .edit_object(ex, |object| {
            if let Some(RoomObject::Extractor(e)) = Some(object) {
                e.cooldown = 0;
            }
            Ok(())
        })
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    let events = f.settle(b);
    assert_eq!(
        events[0]["data"]["amount"], 2,
        "事件 amount=harvestAmount（未钳制——harvest.js:110 vs source 分支 :63 的差异）"
    );
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 0, "实采钳到存量 1"),
        _ => panic!(),
    }
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(
            cr.store_extra[0].amount, 5,
            "T0 2 + T6 2 + quirk 1（实采钳到存量 1）"
        ),
        _ => panic!(),
    }
}

/// harvest.js mineral 分支的门矩阵：无 extractor / 敌主 extractor / RCL 不足 /
/// 无主 extractor（无 controller——utils.js:457-460 无主恒 active）/ 矿量耗尽。
#[test]
fn harvest_mineral_gates() {
    // ① 无 extractor：intent 静默丢弃（API 层 ERR_NOT_FOUND 已挡——结算复核）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let m = f.mineral(10, 10, "U", 1000, 3, None);
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(b);
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 1000, "无 extractor 不采"),
        _ => panic!(),
    }
    // ② 敌主 extractor（harvest.js:78-80）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let m = f.mineral(10, 10, "U", 1000, 3, None);
    f.extractor(10, 10, Some("p2"), 0);
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(b);
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 1000, "敌主 extractor 拒"),
        _ => panic!(),
    }
    // ③ RCL 5（CONTROLLER_STRUCTURES.extractor[5]=0 → checkStructureAgainstController 拒）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let m = f.mineral(10, 10, "U", 1000, 3, None);
    f.extractor(10, 10, Some("p1"), 0);
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(b);
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 1000, "RCL<6 拒（配额 0）"),
        _ => panic!(),
    }
    // ④ 无主 extractor + 无 controller：无主恒 active（utils.js:457-460）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let m = f.mineral(10, 10, "U", 1000, 3, None);
    f.extractor(10, 10, None, 0);
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(b);
    match f.obj(m) {
        RoomObject::Mineral(mm) => assert_eq!(mm.mineral_amount, 999, "无主 extractor 放行"),
        _ => panic!(),
    }
    // ⑤ 矿量耗尽（harvest.js:69-71）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let m = f.mineral(10, 10, "U", 0, 3, None);
    f.extractor(10, 10, Some("p1"), 0);
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    let events = f.settle(b);
    assert!(events.is_empty(), "零矿量不产事件");
}

/// harvest.js:97-104：满仓超容按**矿种** drop（不占 energy 槽）——满 energy
/// 双 WORK 采 2 U：sum 52 > 50 → drop 2 U 成命名堆（当 tick 衰减 1 的既有
/// 锁值口径——harvest_overflow_drops 同款），creep U 键留 0、energy 不动。
#[test]
fn harvest_mineral_overflow_drops_named_pile() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let m = f.mineral(10, 10, "U", 5000, 3, None);
    f.extractor(10, 10, Some("p1"), 0);
    let c = f.creep(
        "w1",
        "p1",
        10,
        11,
        &[BodyKind::Work, BodyKind::Work, BodyKind::Carry],
        50,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(cr) => {
            // 官方 drop 写回 store[U] = 0（键保留——正量才投影）；本 tick
            // 先入槽后掉落，净效果 = 0 值键
            assert_eq!(
                cr.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "U".into(),
                    amount: 0,
                }],
                "超容部分即产即掉（键留 0——官方 store[U]=0 落库同构）"
            );
            assert_eq!(
                cr.store.energy, 50,
                "energy 槽不动（drop 的是矿种——harvest.js:101 的 resourceType）"
            );
        }
        _ => panic!(),
    }
    // 采集意图阶段掉落 2 U，对象 tick 阶段衰减 1，最终堆量为 1。
    let pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 11 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .expect("超容 U 落地成堆");
    assert_eq!(pile, ("U".to_string(), 1), "drop 2 − 当 tick 衰减 1");
}

/// 多资源槽消费面闭环：creep 携 U → transfer 入 storage 命名槽（通用商店
/// 命名资源 U 可转入共享仓 storage、提取回来并掉落；
/// spawn 的按资源容量只允许 energy，所以拒绝 U。
#[test]
fn mineral_multislot_transfer_storage_and_drop_chain() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(4, Some("p1"));
    let storage = f
        .world
        .insert(
            "W1N1",
            RoomObject::Storage(kk_schema::room::StorageObj {
                id: 0,
                x: 10,
                y: 12,
                hits: 10_000,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                notify_when_attacked: None,
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let spawn = f.spawn("s1", 10, 13, 0);
    let c = f.creep_with_mineral("w1", 10, 11, &[BodyKind::Carry, BodyKind::Move], 0, "U", 30);

    // transfer U 30 → storage（官方 transfer.js:45 store[res] 键）
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(storage);
    d.resource_type = Some("U".into());
    d.amount = Some(30);
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["resourceType"], "U");
    match f.obj(storage) {
        RoomObject::Storage(st) => {
            assert_eq!(
                st.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "U".into(),
                    amount: 30,
                }],
                "U 入 storage 命名槽"
            );
        }
        _ => panic!(),
    }
    match f.obj(c) {
        RoomObject::Creep(cr) => assert!(
            cr.store_extra.iter().all(|st| st.amount == 0),
            "转出后键留 0（官方 store[U]=0 落库同构）"
        ),
        _ => panic!(),
    }

    // transfer U → spawn 拒（scr={energy:300}，capacityForResource(U)=undefined）
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(spawn);
    d.resource_type = Some("U".into());
    d.amount = Some(10);
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    assert!(events.is_empty(), "spawn 的 per-resource 容量拒非 energy");

    // withdraw U 30 ← storage
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(storage);
    d.resource_type = Some("U".into());
    d.amount = Some(30);
    b.add_object_intent("p1", c, Action::Withdraw, d);
    let events = f.settle(b);
    assert_eq!(events.len(), 1, "withdraw 反向事件（objectId=storage）");
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.store_extra[0].amount, 30),
        _ => panic!(),
    }
    match f.obj(storage) {
        RoomObject::Storage(st) => {
            assert!(st.store_extra.iter().all(|s| s.amount == 0), "取回后键留 0")
        }
        _ => panic!(),
    }

    // drop U 30 → 命名堆落地（drop.js:24 的 _create-energy(resourceType)）
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.resource_type = Some("U".into());
    d.amount = Some(30);
    b.add_object_intent("p1", c, Action::Drop, d);
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(cr) => assert!(
            cr.store_extra.iter().all(|st| st.amount == 0),
            "drop 后键留 0"
        ),
        _ => panic!(),
    }
    let pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 11 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .expect("drop U 落命名堆");
    assert_eq!(
        pile,
        ("U".to_string(), 29),
        "drop 30 − 当 tick 衰减 1（既有口径）"
    );
}

/// 移动重量计入命名矿物（官方 calcResourcesWeight 的 _.sum(store) 全槽——
/// movement.js:42）：[Carry,Carry,Move] 携 energy 50 + U 60 = 110 → 需 2 个
/// CARRY 承载（weight 2）——plain 移动 +4 疲劳、恢复 −2 ⇒ 净 +2/tick；
/// 不计 U 则 weight 1 ⇒ 净 0（可分辨锁值）。
#[test]
fn movement_weight_counts_named_minerals() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 携 U（总携带 110 → weight 2）：plain 移动 +4 疲劳、恢复 −2 ⇒ 净 +2——
    // 疲劳 2 ≠ 0 ⇒ 下一 tick canMove 不过（卡住），空 tick 恢复 −2 归 0
    let c = f.creep_with_mineral(
        "w1",
        10,
        10,
        &[BodyKind::Carry, BodyKind::Carry, BodyKind::Move],
        50,
        "U",
        60,
    );
    f.settle(move_intent(c, 3)); // RIGHT → plain
    assert_eq!(
        f.creep_state(c).2,
        2,
        "weight 2：+4 疲劳 −2 恢复（U 计入携带量）"
    );
    assert_eq!(f.creep_state(c).0, 11);
    f.settle(move_intent(c, 3)); // 疲劳 2 ⇒ 不能动，仅恢复
    assert_eq!(f.creep_state(c).0, 11, "疲劳卡住（canMove 需 fatigue==0）");
    assert_eq!(f.creep_state(c).2, 0, "空转 tick 恢复 −2×2 归 0");
    // 对照组：无 U（总携带 50 → weight 1）——净 0，连续移动不累积疲劳
    let c2 = f.creep(
        "w2",
        "p1",
        20,
        20,
        &[BodyKind::Carry, BodyKind::Carry, BodyKind::Move],
        50,
    );
    f.settle(move_intent(c2, 3));
    f.settle(move_intent(c2, 3));
    assert_eq!(f.creep_state(c2).0, 22, "weight 1 连续移动（净 0）");
    assert_eq!(f.creep_state(c2).2, 0);
}

/// 携带的命名矿物在死亡时进入墓碑的 store_extra。
/// 来源：上游 creeps/_die.js 的携带资源回收循环。
#[test]
fn creep_death_carries_named_minerals_to_tombstone() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep_with_mineral("w1", 10, 10, &[BodyKind::Move], 20, "XUHO2", 5);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(b);
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        })
        .expect("suicide 产墓碑");
    // Move 件回收 min(125, 50×lifeRate)=125（夹具 ageTime 极大 → lifeRate>2.5
    // 钳满）+ 携带 20 —— _die.js:43-53 与 :78-91 两循环的合计
    assert_eq!(tomb.store.energy, 145);
    assert_eq!(
        tomb.store_extra,
        vec![kk_schema::room::ResourceStack {
            resource: "XUHO2".into(),
            amount: 5,
        }],
        "携带矿物入墓碑命名槽"
    );
}

#[test]
fn ghodium_resource_family_settles() {
    // 依次验证 G 的转移、提取和掉落，使用 ghodium 固定槽，不混入 energy。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let st = f
        .world
        .insert(
            "W1N1",
            RoomObject::Storage(kk_schema::room::StorageObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 10_000,
                owner: "p1".into(),
                store: kk_schema::room::Store::default(),
                store_extra: Vec::new(),
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    // G transfer：creep 40G → storage 20G
    let c = f.creep("w1", "p1", 21, 20, &[BodyKind::Move, BodyKind::Carry], 0);
    f.world
        .edit_object(c, |object| {
            if let Some(RoomObject::Creep(cc)) = Some(object) {
                cc.store.ghodium = 40;
            }
            Ok(())
        })
        .unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.id = Some(st);
    d.amount = Some(20);
    d.resource_type = Some("G".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    f.settle(b);
    match (f.obj(c), f.obj(st)) {
        (RoomObject::Creep(cc), RoomObject::Storage(s)) => {
            assert_eq!(cc.store.ghodium, 20, "creep G 40→20");
            assert_eq!(s.store.ghodium, 20, "storage G 0→20");
        }
        _ => panic!(),
    }
    // G withdraw：storage 20 → creep 全量
    let mut b2 = IntentBatch::default();
    let mut d2 = IntentData::bare();
    d2.id = Some(st);
    d2.amount = Some(20);
    d2.resource_type = Some("G".into());
    b2.add_object_intent("p1", c, Action::Withdraw, d2);
    f.settle(b2);
    match (f.obj(c), f.obj(st)) {
        (RoomObject::Creep(cc), RoomObject::Storage(s)) => {
            assert_eq!(cc.store.ghodium, 40, "withdraw 回满");
            assert_eq!(s.store.ghodium, 0);
        }
        _ => panic!(),
    }
    // G drop：掉落为 resource_type=2 的资源堆（同格同资源并堆）
    let mut b3 = IntentBatch::default();
    let mut d3 = IntentData::bare();
    d3.amount = Some(5);
    d3.resource_type = Some("G".into());
    b3.add_object_intent("p1", c, Action::Drop, d3);
    f.settle(b3);
    let pile = f
        .world
        .rooms()
        .get("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.resource_type == 2 => Some(e.amount),
            _ => None,
        });
    assert_eq!(
        pile,
        Some(4),
        "G 5 掉落 → 当 tick 衰减 ceil(5/1000)=1 → 4（官方全资源同式）"
    );
    assert_eq!(
        match f.obj(c) {
            RoomObject::Creep(cc) => cc.store.ghodium,
            _ => 0,
        },
        35
    );
}

#[test]
fn boost_effectiveness_unified() {
    // KH 使 CARRY 容量加倍，ZO 使 MOVE 疲劳恢复加倍；重量与恢复都须计入 boost。
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 1 carry(KH) + 2 move(ZO)：携带 80（无强化需 2 carry=weight 2；KH ×2
    // 容量 100 ⇒ weight 1）+ 1 move 够驱 ⇒ move 后疲劳 = (1+1) - 2×2 = 0
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 9_000,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "b1".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Carry.code(),
                        hits: 100,
                        boost: Some("KH".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: Some("ZO".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: Some("ZO".into()),
                    },
                ],
                store: kk_schema::room::Store {
                    energy: 80,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.direction = Some(3);
    b.add_object_intent("p1", c, Action::Move, d);
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!((cc.x, cc.y), (11, 10), "direction 3=RIGHT 落地");
            assert_eq!(
                cc.fatigue, 0,
                "weight=1（KH 容量×2）− 恢复 2×2（ZO 效能×2）= 1−4 → 0"
            );
        }
        _ => panic!(),
    }
}
