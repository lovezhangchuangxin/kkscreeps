//! 测试旗帜归属、同轮移位改色、工地移除退款及房间动作执行顺序。

use super::*;

fn create_flag_intent(player: &str, x: i64, y: i64, name: &str, color: i64) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_room_intent(
        player,
        kk_engine::intent::RoomIntent::CreateFlag {
            room: "W1N1".into(),
            x,
            y,
            name: name.into(),
            color,
            secondary_color: color,
        },
    );
    b
}

fn remove_flag_intent(player: &str, name: &str) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_room_intent(
        player,
        kk_engine::intent::RoomIntent::RemoveFlag {
            room: "W1N1".into(),
            name: name.into(),
        },
    );
    b
}

fn flag_by_name(f: &Fixture, name: &str) -> Option<(ObjId, Option<String>)> {
    f.world
        .room("W1N1")
        .unwrap()
        .objects
        .iter()
        .find_map(|(id, o)| match o {
            RoomObject::Flag(fl) if fl.name == name => Some((*id, fl.owner.clone())),
            _ => None,
        })
}

#[test]
fn create_flag_owns_and_scopes_uniqueness() {
    // 同房间内旗帜名称按主人区分，两名玩家可各有同名旗。归属保存在内存对象。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.settle(create_flag_intent("p1", 10, 10, "home", 10));
    let (_, owner) = flag_by_name(&f, "home").expect("p1 旗落库");
    assert_eq!(owner.as_deref(), Some("p1"));

    // 同主重名拒（静默）
    f.settle(create_flag_intent("p1", 20, 20, "home", 10));
    assert_eq!(
        f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .filter(|o| matches!(o, RoomObject::Flag(_)))
            .count(),
        1,
        "同主重名拒"
    );

    // 他人同名可建（per-user 集合语义）
    f.settle(create_flag_intent("p2", 30, 30, "home", 10));
    assert_eq!(
        f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .filter(|o| matches!(o, RoomObject::Flag(_)))
            .count(),
        2,
        "两玩家各有同名旗"
    );
}

#[test]
fn remove_flag_scoped_by_owner() {
    // remove-flag.js：flags 集合按 user 定位——他人同名旗不可删、不存在静默
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.settle(create_flag_intent("p1", 10, 10, "mine", 10));
    f.settle(create_flag_intent("p2", 20, 20, "theirs", 10));

    // p2 尝试删 p1 的旗：静默无效
    f.settle(remove_flag_intent("p2", "mine"));
    assert!(flag_by_name(&f, "mine").is_some(), "他人旗不可删");

    // 本人删自己的
    f.settle(remove_flag_intent("p1", "mine"));
    assert!(flag_by_name(&f, "mine").is_none());

    // 不存在的名静默
    f.settle(remove_flag_intent("p1", "nope"));
    assert!(flag_by_name(&f, "theirs").is_some());
}

#[test]
fn flag_setposition_and_setcolor_composite() {
    // flags.js:66-90：setPosition/setColor = removeFlag + createFlag 同 tick
    // 组合；room/intents.js 组序 removeFlag 先于 createFlag——同 tick 同名
    // 换位/换色成立
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.settle(create_flag_intent("p1", 10, 10, "f", 1));

    // setPosition：(10,10) → (25,25)
    let mut b = remove_flag_intent("p1", "f");
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateFlag {
            room: "W1N1".into(),
            x: 25,
            y: 25,
            name: "f".into(),
            color: 1,
            secondary_color: 1,
        },
    );
    f.settle(b);
    let (id, owner) = flag_by_name(&f, "f").expect("同名移位后仍在");
    assert_eq!(owner.as_deref(), Some("p1"), "归属保持");
    match f.obj(id) {
        RoomObject::Flag(fl) => {
            assert_eq!((fl.x, fl.y), (25, 25), "移位生效（组序 remove→create）");
            assert_eq!(fl.color, 1);
        }
        _ => panic!(),
    }

    // setColor：1 → 10
    let mut b = remove_flag_intent("p1", "f");
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateFlag {
            room: "W1N1".into(),
            x: 25,
            y: 25,
            name: "f".into(),
            color: 10,
            secondary_color: 10,
        },
    );
    f.settle(b);
    match f.obj(flag_by_name(&f, "f").expect("换色后仍在").0) {
        RoomObject::Flag(fl) => assert_eq!(fl.color, 10, "换色生效"),
        _ => panic!(),
    }
}

#[test]
fn remove_construction_site_refunds_half_progress() {
    // remove-construction-site.js:6-10：所有权门（site.user ∨ controller.user）；
    // progress>1 → floor(progress/2) 能量掉落原格
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x: 10,
                y: 10,
                structure_type: "extension".into(),
                progress: 301,
                progress_total: 3000,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .unwrap();

    let mut b = IntentBatch::default();
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::RemoveConstructionSite {
            room: "W1N1".into(),
            id: site,
        },
    );
    f.settle(b);

    assert!(f.world.get(site).is_none(), "工地移除");
    // floor(301/2)=150 能量掉落在 (10,10)
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 10 => Some(e.amount),
            _ => None,
        });
    // 掉落同 tick 经 energy 衰减（官方 energy/tick.js：amount -= ceil(amount/
    // 1000)——150 → 149；intents 相先于 energy tick 相，官方同序）
    assert_eq!(
        dropped,
        Some(149),
        "progress/2 退能量掉落（同 tick 衰减 −1）"
    );

    // progress ≤1 不掉落
    let site2 = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x: 12,
                y: 12,
                structure_type: "road".into(),
                progress: 1,
                progress_total: 300,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::RemoveConstructionSite {
            room: "W1N1".into(),
            id: site2,
        },
    );
    f.settle(b);
    assert!(f.world.get(site2).is_none());
    let no_drop = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .any(|o| matches!(o, RoomObject::Energy(e) if e.x == 12 && e.y == 12));
    assert!(!no_drop, "progress=1 不掉落（>1 门）");
}

#[test]
fn remove_construction_site_ownership_gates() {
    // 工地主人或房主可移除工地；无主房的他人工地拒绝。
    // 来源：上游 room/remove-construction-site.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let enemy_site = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x: 10,
                y: 10,
                structure_type: "extension".into(),
                progress: 100,
                progress_total: 3000,
                owner: Some("p2".into()),
                name: None,
            }),
        )
        .unwrap();
    // 无主房：p1 删 p2 工地 → 拒
    let mut b = IntentBatch::default();
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::RemoveConstructionSite {
            room: "W1N1".into(),
            id: enemy_site,
        },
    );
    f.settle(b);
    assert!(f.world.get(enemy_site).is_some(), "他人工地他人房拒");

    // p1 拿房 → 可删
    f.controller_full(3, Some("p1"), |_| {});
    let mut b = IntentBatch::default();
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::RemoveConstructionSite {
            room: "W1N1".into(),
            id: enemy_site,
        },
    );
    f.settle(b);
    assert!(f.world.get(enemy_site).is_none(), "房主可删他人工地");
}

#[test]
fn room_intent_dispatch_order_remove_before_create() {
    // 故意先提交同名 create，再提交 remove，结算仍应先删后建。
    // 必须使用同名旗：异名旗无论处理顺序如何都能通过，不能验证排序。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.settle(create_flag_intent("p1", 10, 10, "flag", 10));

    // 故意倒序 push（create 先）——组序排序后 remove 仍先于 create
    let mut b = IntentBatch::default();
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateFlag {
            room: "W1N1".into(),
            x: 15,
            y: 15,
            name: "flag".into(),
            color: 10,
            secondary_color: 10,
        },
    );
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::RemoveFlag {
            room: "W1N1".into(),
            name: "flag".into(),
        },
    );
    f.settle(b);
    let flag = flag_by_name(&f, "flag").expect("同名单名重建存活——create 必须后于 remove");
    let fl = f.world.room("W1N1").unwrap().objects.get(&flag.0).unwrap();
    match fl {
        RoomObject::Flag(fl) => assert_eq!(
            (fl.x, fl.y),
            (15, 15),
            "重建位生效（若 create 先处理则同名拒+remove 删净=旗消亡）"
        ),
        _ => panic!(),
    }
}

#[test]
fn own_construction_site_blocks_movement() {
    // 自家障碍工地在移动仲裁时挡路，而非请求预检时。
    // 来源：上游 intents/movement.js 的 checkObstacleAtXY。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(10, 11, "spawn", 0, 15000, "p1");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5)); // → (10,11) 自家 spawn 工地
    assert_eq!(f.creep_state(c), (10, 10, 0, 0), "自家 obstacle 工地挡路");
    assert!(f.world.get(site).is_some(), "工地未被动");
}

#[test]
fn moving_onto_enemy_site_stomps_it() {
    // movement.js:224-233：进格踩掉敌方工地（safeMode 门外），退一半工程为
    // 掉落能量；tick 期插入的 energy 不在阶段 8 键快照内 → 本 tick 不衰减
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(10, 11, "road", 10, 300, "p2");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 11, "敌方工地可入");
    assert!(f.world.get(site).is_none(), "工地被踩掉");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(5), "退半工程 10/2=5，本 tick 不衰减");
}
