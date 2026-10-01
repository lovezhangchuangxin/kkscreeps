//! 测试远程单体与群体攻击、say 文本重置，以及受袭通知标志。

use super::*;

fn ranged_attack_intent(player: &str, attacker: ObjId, target: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent(
        player,
        attacker,
        Action::RangedAttack,
        IntentData::target(target),
    );
    b
}

fn ranged_mass_intent(player: &str, attacker: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent(
        player,
        attacker,
        Action::RangedMassAttack,
        IntentData::bare(),
    );
    b
}

fn say_intent(creep: ObjId, message: &str, is_public: bool) -> IntentBatch {
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.message = Some(message.into());
    d.is_public = Some(is_public);
    b.add_object_intent("p1", creep, Action::Say, d);
    b
}

/// 插入无主 Generic rampart。
fn rampart(f: &mut Fixture, x: u8, y: u8, hits: i64) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: serde_json::json!({
                    "type": "structure", "structureType": "rampart",
                    "hits": hits, "hitsMax": hits,
                    // 远期时钟防止本例的 rampart 被周期衰减移除。
                    "nextDecayTime": 1_000_000,
                })
                .to_string(),
            }),
        )
        .unwrap()
}

fn generic_hits(f: &Fixture, id: ObjId) -> i64 {
    match f.obj(id) {
        RoomObject::Generic(o) => {
            let v: serde_json::Value = serde_json::from_str(&o.data).unwrap();
            v["hits"].as_i64().unwrap()
        }
        _ => panic!("非 Generic"),
    }
}

#[test]
fn ranged_attack_damage_event_and_action_log() {
    // rangedAttack.js:9-41：box 射程 3、延迟伤害（_damage.js:20-21）、
    // 事件 attackType=RANGED（:93）、actionLog.rangedAttack（:66-71——本仓
    // 位图 ACTION_RANGED_ATTACK）。1×RANGED_ATTACK×10。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    // victim 带 ATTACK 部件：反击仅近战（_damage.js:17-19 的 MELEE 门）——
    // ranged 下攻击者不掉血（变异「ranged 也反击」此断言红）
    let victim = f.creep("v", "p2", 12, 12, &[BodyKind::Attack, BodyKind::Attack], 0);

    let events = f.settle(ranged_attack_intent("p1", atk, victim));

    assert_eq!(events.len(), 1, "单目标单事件（反击仅近战——_damage.js:17）");
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(events[0]["objectId"], atk.as_str());
    assert_eq!(events[0]["data"]["targetId"], victim.as_str());
    assert_eq!(events[0]["data"]["damage"], 10);
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_RANGED
    );
    assert_eq!(creep_parts(&f, victim).0, 190, "200−10 延迟施加后");
    assert_eq!(creep_parts(&f, victim).1, vec![90, 100], "前列先掉");
    assert_eq!(creep_parts(&f, atk).0, 100, "反击仅近战——ranged 攻击者无损");
    // actionLog：施攻者 rangedAttack 位；受攻者 attacked 位
    let (log_a, log_v) = match (f.obj(atk), f.obj(victim)) {
        (RoomObject::Creep(a), RoomObject::Creep(b)) => (a.action_log, b.action_log),
        _ => panic!(),
    };
    assert_eq!(
        log_a,
        kk_engine::settle::creep::ACTION_RANGED_ATTACK,
        "_damage.js:66-71 官方独立键"
    );
    assert_eq!(
        log_v,
        kk_engine::settle::creep::ACTION_ATTACKED as u16,
        "_damage.js:73-75"
    );
}

#[test]
fn ranged_attack_rampart_redirect() {
    // rangedAttack.js:33-35：目标格有 rampart → 伤害改打 rampart（任何归属——
    // 官方 _.find 无所有权过滤）。creep 不掉血、rampart 立即扣（结构路径
    // _damage.js:26-32）。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let victim = f.creep("v", "p2", 11, 11, &[BodyKind::Tough], 0);
    let wall = rampart(&mut f, 11, 11, 1000);

    let events = f.settle(ranged_attack_intent("p1", atk, victim));

    assert_eq!(creep_parts(&f, victim).0, 100, "重定向后本体无损");
    assert_eq!(generic_hits(&f, wall), 990, "rampart 立即扣血");
    assert_eq!(
        events[0]["data"]["targetId"],
        wall.as_str(),
        "事件指重定向目标"
    );
}

#[test]
fn ranged_attack_rechecks() {
    // 官方执行器重检（与 API 预检独立——伪造 intent 兜底）：射程外静默拒
    //（:13-15）、自指拒（:11）、他人 safeMode 拒（:22-23）。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let far = f.creep("far", "p2", 20, 20, &[BodyKind::Tough], 0);
    let near = f.creep("near", "p2", 11, 10, &[BodyKind::Tough], 0);
    // box 边界：dx=dy=3 在界内（|dx|≤3 且 |dy|≤3）；dx=4 界外
    let corner = f.creep("corner", "p2", 13, 13, &[BodyKind::Tough], 0);
    let out = f.creep("out", "p2", 14, 10, &[BodyKind::Tough], 0);

    let events = f.settle(ranged_attack_intent("p1", atk, far));
    assert!(events.is_empty(), "射程外静默拒");
    assert_eq!(creep_parts(&f, far).0, 100);
    let events = f.settle(ranged_attack_intent("p1", atk, corner));
    assert_eq!(events.len(), 1, "dx=dy=3 box 界内（≡Chebyshev 3）");
    assert_eq!(creep_parts(&f, corner).0, 90);
    let events = f.settle(ranged_attack_intent("p1", atk, out));
    assert!(events.is_empty(), "dx=4 box 界外");
    assert_eq!(creep_parts(&f, out).0, 100);

    let events = f.settle(ranged_attack_intent("p1", atk, atk));
    assert!(events.is_empty(), "自指拒（rangedAttack.js:11）");

    // safeMode：p2 的房开 safeMode，p1 的攻击静默拒
    f.controller_full(3, Some("p2"), |c| c.safe_mode = 5);
    let events = f.settle(ranged_attack_intent("p1", atk, near));
    assert!(events.is_empty(), "他人 safeMode 拒");
    assert_eq!(creep_parts(&f, near).0, 100);
}

#[test]
fn ranged_mass_attack_tiers_and_exclusions() {
    // rangedMassAttack.js：目标 = 有主属他人者（+powerBank）在 ±3 box；
    // 距离衰减 {1:×1, 2:×0.4, 3:×0.1}（round）；非 rampart 且格上有 rampart
    // → 跳过（不重定向——与单体语义相反）；无主对象排除。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let d1 = f.creep("d1", "p2", 11, 10, &[BodyKind::Tough], 0); // d=1 → 10
    let d2 = f.creep("d2", "p2", 12, 10, &[BodyKind::Tough], 0); // d=2 → 4
    let d3 = f.creep("d3", "p2", 13, 10, &[BodyKind::Tough], 0); // d=3 → 1
    let own = f.creep("own", "p1", 11, 11, &[BodyKind::Tough], 0); // 自家 → 排除
    let shielded = f.creep("sh", "p2", 10, 11, &[BodyKind::Tough], 0); // 盾下 → 跳过
    let _wall = rampart(&mut f, 10, 11, 5000);
    // 无主墙（box 内但无 user）→ 排除
    let _neutral = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                id: 0,
                x: 10,
                y: 12,
                hits: 1000,
                notify_when_attacked: None,
            }),
        )
        .unwrap();

    let events = f.settle(ranged_mass_intent("p1", atk));

    assert_eq!(creep_parts(&f, d1).0, 90, "×1 → 10");
    assert_eq!(creep_parts(&f, d2).0, 96, "×0.4 → round(4.0)=4");
    assert_eq!(creep_parts(&f, d3).0, 99, "×0.1 → round(1.0)=1");
    assert_eq!(
        creep_parts(&f, own).0,
        100,
        "自家排除（i.user != object.user）"
    );
    assert_eq!(creep_parts(&f, shielded).0, 100, "盾下跳过（:28-29）");
    assert_eq!(
        generic_hits(&f, _wall),
        5000,
        "rampart 本体不因他人盾位受击"
    );
    // 三目标三事件 + 施攻者 actionLog.rangedMassAttack（:58——处理器尾部）
    assert_eq!(events.len(), 3);
    assert_eq!(
        match f.obj(atk) {
            RoomObject::Creep(c) => c.action_log,
            _ => panic!(),
        },
        kk_engine::settle::creep::ACTION_RANGED_MASS
    );
}

#[test]
fn ranged_mass_attack_zero_power_and_safemode() {
    // rangedMassAttack.js:13-15：功率 0（无 RANGED_ATTACK 部件）早退——零事件
    // 且 actionLog 不落；:22-23 他人 safeMode 拒。
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 零目标仍有功率：actionLog.rangedMassAttack 照置（处理器尾部落键）——
    // 先于一切 p2 对象创建
    let gun = f.creep("gun", "p1", 10, 12, &[BodyKind::RangedAttack], 0);
    let events = f.settle(ranged_mass_intent("p1", gun));
    assert!(events.is_empty(), "房内无敌对目标");
    assert_eq!(
        match f.obj(gun) {
            RoomObject::Creep(c) => c.action_log,
            _ => panic!(),
        },
        kk_engine::settle::creep::ACTION_RANGED_MASS,
        ":58 尾部落键——0 目标也置"
    );

    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Work], 0);
    let victim = f.creep("v", "p2", 11, 10, &[BodyKind::Tough], 0);
    let events = f.settle(ranged_mass_intent("p1", atk));
    assert!(events.is_empty(), "无部件 → 功率 0 早退");
    assert_eq!(creep_parts(&f, victim).0, 100);
    assert_eq!(
        match f.obj(atk) {
            RoomObject::Creep(c) => c.action_log,
            _ => panic!(),
        },
        0,
        "功率 0 早退在尾部落键之前——actionLog 不置"
    );

    f.controller_full(3, Some("p2"), |c| c.safe_mode = 5);
    let events = f.settle(ranged_mass_intent("p1", gun));
    assert!(events.is_empty(), "他人 safeMode 拒");
    assert_eq!(creep_parts(&f, victim).0, 100);
}

#[test]
fn ranged_attack_boost_multiplier() {
    // XKHO2 的 rangedAttack 倍率为 4，查表身体类型必须使用 ranged_attack。
    // 类型键误写为 rangedAttack 会查不到 boost，退回未强化功率。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    if let Some(RoomObject::Creep(c)) = f.world.get_mut(atk) {
        for part in &mut c.body {
            if part.kind == BodyKind::RangedAttack.code() {
                part.boost = Some("XKHO2".into());
            }
        }
    }
    let victim = f.creep("v", "p2", 11, 11, &[BodyKind::Tough], 0);
    f.settle(ranged_attack_intent("p1", atk, victim));
    assert_eq!(creep_parts(&f, victim).0, 60, "100−10×4（XKHO2 boost）");
}

#[test]
fn say_payload_truncation_reset_and_tombstone() {
    // Say 截取 10 个 UTF-16 码元，消息和公开标志随每轮 action log 重置。
    // 死亡时只将公开遗言转入墓碑。来源：上游 creeps/say.js 和 creeps/_die.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let talker = f.creep("t", "p1", 10, 10, &[BodyKind::Move], 0);

    f.settle(say_intent(talker, "hello world!!!", true));
    match f.obj(talker) {
        RoomObject::Creep(c) => {
            assert_eq!(c.say_message.as_deref(), Some("hello worl"), "截 10 码元");
            assert_eq!(c.say_public, Some(true));
        }
        _ => panic!(),
    }
    // 下一 tick 无 say → 重置
    f.settle(IntentBatch::default());
    match f.obj(talker) {
        RoomObject::Creep(c) => {
            assert_eq!(c.say_message, None);
            assert_eq!(c.say_public, None);
        }
        _ => panic!(),
    }
    // 公开遗言死亡 → 墓碑承接；私有 → None
    let mut b = say_intent(talker, "bye", true);
    b.add_object_intent("p1", talker, Action::Suicide, IntentData::bare());
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
        .expect("墓碑落库");
    assert_eq!(tomb.creep_saying.as_deref(), Some("bye"), "公开遗言承接");

    let talker2 = f.creep("t2", "p1", 12, 12, &[BodyKind::Move], 0);
    let mut b = say_intent(talker2, "secret", false);
    b.add_object_intent("p1", talker2, Action::Suicide, IntentData::bare());
    f.settle(b);
    let saying = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) if t.creep_name.as_deref() == Some("t2") => {
                Some(t.creep_saying.clone())
            }
            _ => None,
        })
        .expect("第二座墓碑");
    assert_eq!(saying, None, "私有遗言不承接（_die.js:33 isPublic 门）");
}

#[test]
fn notify_when_attacked_settles_on_creep() {
    // NotifyWhenAttacked 是跨 tick 保留的标志，不随 action log 重置。
    // 来源：上游 engine/src/processor.js 的对象通知意图处理。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("c", "p1", 10, 10, &[BodyKind::Move], 0);

    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.enabled = Some(false);
    b.add_object_intent("p1", c, Action::NotifyWhenAttacked, d);
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(creep) => assert_eq!(creep.notify_when_attacked, Some(false)),
        _ => panic!(),
    }
    // 下一 tick 保持（非 actionLog 面）
    f.settle(IntentBatch::default());
    match f.obj(c) {
        RoomObject::Creep(creep) => assert_eq!(creep.notify_when_attacked, Some(false)),
        _ => panic!(),
    }
}
