//! 近战伤害、反击、rampart 和死亡转换。
use super::*;

#[test]
fn melee_attack_damage_and_body_distribution() {
    // attack.js:38 伤害 = 2×ATTACK×30=60；_recalc-body.js:10-18 从尾部灌满——
    // 前列部件先掉 hit：[tough,work,work,move] 400→340，tough 100→40
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep(
        "atk",
        "p1",
        10,
        10,
        &[BodyKind::Attack, BodyKind::Attack],
        0,
    );
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Tough,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Move,
        ],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1, "无反击目标（无 ATTACK 部件）→ 单事件");
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(events[0]["objectId"], a.as_str());
    assert_eq!(events[0]["data"]["targetId"], b.as_str());
    assert_eq!(events[0]["data"]["damage"], 60);
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_MELEE
    );
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 340, "400 - 60");
    assert_eq!(parts, vec![40, 100, 100, 100], "受伤自头部推进");
    // actionLog（_damage.js:59-75）：攻击方 attack 位、受击方 attacked 位
    let (log_a, log_b) = match (f.obj(a), f.obj(b)) {
        (RoomObject::Creep(x), RoomObject::Creep(y)) => (x.action_log, y.action_log),
        _ => panic!(),
    };
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_ATTACK as u16);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_ATTACKED as u16);
}

#[test]
fn melee_hit_back_and_event_order() {
    // _damage.js:17-19：近战 creep 目标反击（同样延迟）；:89-93 反击事件先于主事件
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep(
        "atk",
        "p1",
        10,
        10,
        &[
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Attack,
        ],
        0,
    );
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Tough,
            BodyKind::Tough,
        ],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 2, "反击事件 + 主事件");
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[0]["objectId"],
        b.as_str(),
        "反击事件以受击者为 objectId"
    );
    assert_eq!(events[0]["data"]["targetId"], a.as_str());
    assert_eq!(events[0]["data"]["damage"], 60, "2×ATTACK×30 反击");
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
    );
    assert_eq!(events[1]["data"]["damage"], 120, "4×ATTACK×30 主伤害");
    // 双方伤害同 tick 各自结算：a 400-60=340、b 400-120=280
    assert_eq!(creep_parts(&f, a).0, 340);
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 280);
    assert_eq!(parts, vec![0, 80, 100, 100], "b 首个 ATTACK 部件被击毁");
    // 攻击者同时记 attack 与 attacked（_damage.js:74 + :88）
    let log_a = match f.obj(a) {
        RoomObject::Creep(c) => c.action_log,
        _ => panic!(),
    };
    assert_eq!(
        log_a,
        kk_engine::settle::creep::ACTION_ATTACK as u16
            | kk_engine::settle::creep::ACTION_ATTACKED as u16
    );
}

#[test]
fn no_hit_back_when_attacker_on_rampart() {
    // _damage.js:17：攻击者站 rampart（不分敌我/公开性）→ 无反击
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    generic_structure(&mut f, 10, 10, "rampart", "p2", 1000);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Attack, BodyKind::Tough],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1, "仅主事件");
    assert_eq!(creep_parts(&f, a).0, 100, "攻击者无伤");
    assert_eq!(creep_parts(&f, b).0, 170, "200 - 30");
}

#[test]
fn multiple_attackers_accumulate_damage() {
    // _damage.js:20：creep 目标伤害累加进 _damageToApply，目标自己的 tick 一次结算
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a1 = f.creep("a1", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Attack], 0);
    let a2 = f.creep("a2", "p1", 12, 10, &[BodyKind::Attack, BodyKind::Attack], 0);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Tough,
            BodyKind::Tough,
            BodyKind::Tough,
            BodyKind::Tough,
        ],
        0,
    );
    let mut batch = attack_intent(a1, b);
    batch.add_object_intent("p1", a2, Action::Attack, IntentData::target(b));
    let events = f.settle(batch);
    assert_eq!(events.len(), 2, "两次主事件");
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 280, "400 - 60×2");
    assert_eq!(parts, vec![0, 80, 100, 100]);
}

#[test]
fn attack_kills_creep_into_tombstone() {
    // 伤害结算后 hits 归零时死亡，资源进入墓碑；与 TTL 死亡使用同一回收入口。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let mut bodylist = vec![BodyKind::Attack; 40];
    bodylist.push(BodyKind::Move);
    let a = f.creep("atk", "p1", 10, 10, &bodylist, 0);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Tough, BodyKind::Tough],
        40,
    );
    let events = f.settle(attack_intent(a, b));
    assert!(f.world.get(b).is_none(), "1200 ≥ 200：击杀");
    // 夹具寿命极长，每件回收钳到 125；携带 40 加两件回收得到墓碑 energy=290。
    // 两件身体对应 10 tick 衰减期限。
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) if t.x == 11 && t.y == 10 => Some(t.clone()),
            _ => None,
        })
        .expect("暴亡产墓碑");
    assert_eq!(tomb.store.energy, 290, "携带 40 + 造价回收 250");
    assert_eq!(tomb.owner.as_deref(), Some("p2"), "墓碑归死者");
    assert_eq!(tomb.creep_name.as_deref(), Some("victim"));
    // 事件序（官方）：intent 相的 EVENT_ATTACK 先（_damage.js:93）、tick 相的
    // OBJECT_DESTROYED 后（_die.js:97）
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[1]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[1]["objectId"], b.as_str());
}

#[test]
fn attack_kills_structure_and_spawn_chick() {
    // structures/_destroy.js:9-15：孵化中的 spawn 被毁 → 雏形一并移除；
    // Spawn 的库存进入废墟。来源：上游 structures/_destroy.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    // 造孵化任务与雏形（走真 createCreep intent）
    let mut b0 = IntentBatch::default();
    let mut d0 = IntentData::bare();
    d0.name = Some("child".into());
    d0.body = vec![BodyKind::Move];
    b0.add_object_intent("p1", spawn, Action::CreateCreep, d0);
    f.settle(b0);
    // 削弱 spawn 至一击必杀（120 伤害）
    if let Some(RoomObject::Spawn(s)) = f.world.get_mut(spawn) {
        s.hits = 100;
    }
    let atk = f.creep("atk", "p2", 20, 21, &[BodyKind::Attack; 4], 0);
    f.settle(attack_intent_by("p2", atk, spawn));
    assert!(f.world.get(spawn).is_none(), "spawn 被击毁");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Creep(c) if c.name == "child")),
        "雏形随 spawn 一并移除"
    );
    // 击毁 spawn 产生废墟，并保存结构信息与库存。
    // 300 初始能量减 50 孵化费，再加前一轮免费能量 1，库存为 251；衰减期限为 500 tick。
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.x == 20 && r.y == 20 => Some(r.clone()),
            _ => None,
        })
        .expect("击毁产废墟");
    assert_eq!(ruin.structure_type, "spawn");
    assert_eq!(ruin.structure_id, spawn.0, "存照 id = 原结构 id");
    assert_eq!(
        ruin.structure_hits_max,
        Some(kk_core::constants::SPAWN_HITS as u32)
    );
    assert_eq!(ruin.owner.as_deref(), Some("p1"));
    assert_eq!(ruin.destroy_time, 1, "第二个结算 tick");
    assert_eq!(ruin.decay_time, 1 + 500, "RUIN_DECAY");
    assert_eq!(ruin.store.energy, 251, "store 全量承接（不再掉地）");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(e) if e.x == 20 && e.y == 20)),
        "store 不再落地（废墟承接）"
    );
}

#[test]
fn rampart_redirects_attack_until_broken() {
    // attack.js:33-36：目标格 rampart 存在 → 伤害转打 rampart；打破后直击本体
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 11, 10, &[BodyKind::Attack; 10], 0);
    let ext = extension_at(&mut f, 12, 10, 1000, 0);
    let rampart = generic_structure(&mut f, 12, 10, "rampart", "p2", 1000);
    // t1：extension 无伤、rampart -300
    let events = f.settle(attack_intent(a, ext));
    assert_eq!(
        events[0]["data"]["targetId"],
        rampart.as_str(),
        "事件指向 rampart"
    );
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000, "本体受 rampart 保护"),
        _ => panic!(),
    }
    match f.obj(rampart) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], 700);
        }
        _ => panic!(),
    }
    // t2-t3：rampart 700→400→100
    f.settle(attack_intent(a, ext));
    f.settle(attack_intent(a, ext));
    // t4：100-300 ≤ 0 → rampart 击毁
    f.settle(attack_intent(a, ext));
    assert!(f.world.get(rampart).is_none(), "rampart 击毁");
    assert!(f.world.get(ext).is_some(), "本体仍在");
    // t5：无 rampart → 直击 extension
    let events = f.settle(attack_intent(a, ext));
    assert_eq!(events[0]["data"]["targetId"], ext.as_str());
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 700),
        _ => panic!(),
    }
}
