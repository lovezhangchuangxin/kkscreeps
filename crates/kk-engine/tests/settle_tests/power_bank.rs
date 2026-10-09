//! powerBank 消费者结算面：攻击反伤（_damage.js:35-41）、衰变移除
//!（processor.js:421-426）与战毁废墟（_destroy.js 的 store 转移，
//! 泼洒由既有 ruin 通道承担，此处验 powerBank 形态）。
use super::*;

/// 直插 canonical powerBank 形态（生成器/放置层同款 data）；
/// decay_time 模拟放置层的绝对权威键（None = 永不衰变移除）。
fn power_bank(
    f: &mut Fixture,
    x: u8,
    y: u8,
    hits: i64,
    power: u64,
    decay_time: Option<u64>,
) -> ObjId {
    let mut data = serde_json::json!({
        "type": "structure",
        "structureType": "powerBank",
        "store": { "power": power },
        "hits": hits,
        "hitsMax": kk_core::constants::POWER_BANK_HITS,
    });
    if let Some(deadline) = decay_time {
        data["decayTime"] = serde_json::json!(deadline);
    }
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: data.to_string(),
            }),
        )
        .unwrap()
}

/// 从 Generic data 读 hits（银行耐久断言小工具）。
fn bank_hits(f: &Fixture, id: ObjId) -> i64 {
    match f.obj(id) {
        RoomObject::Generic(g) => {
            let v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            v["hits"].as_i64().unwrap()
        }
        _ => panic!("非 Generic 对象"),
    }
}

fn mass_attack_intent(player: &str, attacker: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent(
        player,
        attacker,
        Action::RangedMassAttack,
        IntentData::bare(),
    );
    b
}

/// 任意攻击类型都反射半伤（_damage.js:36 不分攻击类型）；反击事件
/// objectId 是 powerBank、targetId 是攻击者，先于主攻击事件（:89-93）；
/// 反射经待伤害队列当拍应用。
#[test]
fn attack_power_bank_reflects_half_damage_with_hit_back_event() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let bank = power_bank(&mut f, 11, 10, 2_000_000, 500, Some(1_000_000));
    let atk = f.creep(
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
    let events = f.settle(attack_intent(atk, bank));
    // 4×ATTACK×30=120 主伤害；反射 60。
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[0]["objectId"],
        bank.as_str(),
        "反射事件以银行为 objectId"
    );
    assert_eq!(events[0]["data"]["targetId"], atk.as_str());
    assert_eq!(events[0]["data"]["damage"], 60);
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
    );
    assert_eq!(events[1]["objectId"], atk.as_str());
    assert_eq!(events[1]["data"]["damage"], 120);
    // 银行 2000000-120；攻击者 400-60=340，当拍应用。
    assert_eq!(bank_hits(&f, bank), 1_999_880);
    assert_eq!(creep_parts(&f, atk).0, 340);
    // 攻击者同时记 attack 与 attacked（_damage.js:74 + :88）。
    let log = match f.obj(atk) {
        RoomObject::Creep(c) => c.action_log,
        _ => panic!(),
    };
    assert_eq!(
        log,
        kk_engine::settle::creep::ACTION_ATTACK as u16
            | kk_engine::settle::creep::ACTION_ATTACKED as u16
    );
}

/// 群体攻击距离 3 档伤害 1（round(10×0.1)）反射 0.5——毫精度向下取整
/// 不足 1 不触发：无反射事件、攻击者不掉血（官方 JS 存小数待伤害，
/// 套件整数反射不可观测，登记为有意分歧）。
#[test]
fn odd_damage_below_half_unit_does_not_reflect() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let bank = power_bank(&mut f, 13, 10, 2_000_000, 500, Some(1_000_000));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let events = f.settle(mass_attack_intent("p1", atk));
    // 主事件 1 条；反射 0.5 < 1 不登记。
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["data"]["damage"], 1, "距离 3 档 round(10*0.1)=1");
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_RANGED_MASS
    );
    assert_eq!(creep_parts(&f, atk).0, 100, "反射不足 1 不触发");
    assert_eq!(bank_hits(&f, bank), 1_999_999);
}

/// 房主安全模式抑制反射（_damage.js:39-41：controller.user == 攻击者
/// 主人且 safeMode 激活时清零）；主伤害与主事件不受影响。
#[test]
fn attacker_own_safemode_suppresses_reflect() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(1, Some("p1"), |ctl| ctl.safe_mode = 1000);
    let bank = power_bank(&mut f, 11, 10, 2_000_000, 500, Some(1_000_000));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    let events = f.settle(attack_intent(atk, bank));
    assert_eq!(events.len(), 1, "仅主事件：{events:?}");
    assert_eq!(events[0]["data"]["damage"], 30);
    assert_eq!(creep_parts(&f, atk).0, 100, "反射被安全模式抑制");
    assert_eq!(bank_hits(&f, bank), 1_999_970);
}

/// 衰变移除（processor.js:421-426）：`time >= decayTime-1` 直接消失，
/// 不留废墟、不泼洒 store；缺 decayTime 的形态永不移除。
#[test]
fn decay_removes_bank_without_ruin_or_spill() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let with_clock = power_bank(&mut f, 10, 10, 2_000_000, 500, Some(3));
    let no_clock = power_bank(&mut f, 20, 20, 2_000_000, 500, None);
    for _ in 0..3 {
        f.settle(IntentBatch::default());
    }
    assert!(f.world.get(with_clock).is_none(), "T+2 移除相触发后消失");
    assert!(f.world.get(no_clock).is_some(), "无 decayTime 永不移除");
    let objects: Vec<&RoomObject> = f.world.room("W1N1").unwrap().objects.values().collect();
    assert!(
        objects.iter().all(|o| !matches!(o, RoomObject::Ruin(_))),
        "衰变移除不产废墟"
    );
    assert!(
        objects.iter().all(|o| !matches!(o, RoomObject::Energy(_))),
        "衰变移除不泼洒资源"
    );
}

/// 战毁废墟（_destroy.js）：非核弹击杀产废墟，store.power 转移进
/// store_extra，衰减间隔取 RUIN_DECAY_STRUCTURES.powerBank（10）。
/// 击杀拍仍反射（官方 :36 在摧毁检查后计算）：完整事件序
/// [OBJECT_DESTROYED, HIT_BACK, ATTACK]，攻击者当拍扣 15。
#[test]
fn destroyed_bank_leaves_ruin_with_power_and_short_decay() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let bank = power_bank(&mut f, 11, 10, 1, 500, Some(1_000_000));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    let events = f.settle(attack_intent(atk, bank));
    assert_eq!(events.len(), 3, "摧毁→反射→主攻击：{events:?}");
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[1]["objectId"], bank.as_str(), "击杀拍仍反射");
    assert_eq!(events[1]["data"]["targetId"], atk.as_str());
    assert_eq!(events[1]["data"]["damage"], 15);
    assert_eq!(
        events[1]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
    );
    assert_eq!(events[2]["data"]["damage"], 30, "主攻击");
    assert_eq!(creep_parts(&f, atk).0, 85, "100-15 击杀拍反射照扣");
    assert!(f.world.get(bank).is_none(), "银行被摧毁");
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) => Some(r.clone()),
            _ => None,
        })
        .expect("战毁产废墟");
    assert_eq!(ruin.structure_type, "powerBank");
    assert_eq!(
        ruin.structure_hits_max,
        Some(kk_core::constants::POWER_BANK_HITS as u32)
    );
    assert!(ruin.owner.is_none(), "银行无主，废墟无主（_destroy.js:37）");
    assert_eq!(ruin.store_extra.len(), 1);
    assert_eq!(ruin.store_extra[0].resource, "power");
    assert_eq!(ruin.store_extra[0].amount, 500, "store.power 转入废墟");
    let expected_decay = kk_core::constants::RUIN_DECAY_STRUCTURES
        .iter()
        .find(|(k, _)| *k == "powerBank")
        .map(|(_, v)| *v)
        .unwrap() as u64;
    assert_eq!(ruin.decay_time, expected_decay);
}

/// 普通远程攻击同样反射（_damage.js:36 任意攻击类型；10×0.5=5）。
#[test]
fn ranged_attack_power_bank_also_reflects() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let bank = power_bank(&mut f, 12, 10, 2_000_000, 500, Some(1_000_000));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", atk, Action::RangedAttack, IntentData::target(bank));
    let events = f.settle(b);
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["data"]["damage"], 5, "10×0.5");
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
    );
    assert_eq!(events[1]["data"]["damage"], 10);
    assert_eq!(creep_parts(&f, atk).0, 95, "100-5");
}

/// 反射致死死亡链：1×ATTACK 攻击者 100 HP，第 6 次攻击后剩 10，
/// 第 7 次反射 15 致死——走墓碑/掉落死亡链。
#[test]
fn reflected_damage_can_kill_the_attacker() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let bank = power_bank(&mut f, 11, 10, 2_000_000, 500, Some(1_000_000));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    for i in 0..6 {
        f.settle(attack_intent(atk, bank));
        assert_eq!(creep_parts(&f, atk).0, 100 - 15 * (i + 1), "第{}次", i + 1);
    }
    assert_eq!(creep_parts(&f, atk).0, 10, "100-6×15");
    f.settle(attack_intent(atk, bank));
    assert!(f.world.get(atk).is_none(), "第 7 次反射 15 ≥ 10 致死");
    assert!(
        f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Tombstone(_))),
        "反射致死走墓碑死亡链"
    );
}
