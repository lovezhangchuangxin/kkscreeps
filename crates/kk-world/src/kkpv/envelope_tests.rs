//! 信封字符串直构的字节锁：与 Value 路径序列化输出逐字节一致。
use super::*;
use kk_core::Terrain;
use kk_schema::room::{ControllerObj, ExtensionObj, GenericObj, RoomObject, SpawnObj, Store};

fn store(energy: u32) -> Store {
    Store { energy, ghodium: 0 }
}

#[test]
fn envelope_string_matches_value_serialization() {
    // 覆盖：可见/不可见房、带/不带公共状态、事件段（含病态条目被丢弃）、
    // 观察者补足视野、能量逐查看者（观察者非房主计 0）、代际缓存复用
    //（两次调用同串）与换代失效。
    let mut world = World::new(3);
    world.set_time_for_test(77).unwrap();
    for room in ["W1N1", "E2S2", "E3S2"] {
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room(room, terrain).unwrap();
    }
    // W1N1：p1 自有房——spawn/extension 供能量统计，事件含一条病态 JSON。
    world
        .insert(
            "W1N1",
            RoomObject::Controller(ControllerObj {
                id: 0,
                x: 5,
                y: 5,
                level: 4,
                safe_mode: 0,
                safe_mode_available: 1,
                progress: 12_345,
                downgrade_time: Some(900),
                owner: Some("p1".into()),
                owner_cleared: false,
                is_power_enabled: true,
                reservation: None,
                sign: None,
                upgrade_blocked: None,
                safe_mode_cooldown: None,
            }),
        )
        .unwrap();
    for (id, x) in [(1, 10), (2, 11)] {
        world
            .insert(
                "W1N1",
                RoomObject::Extension(ExtensionObj {
                    id,
                    x,
                    y: 10,
                    hits: 1000,
                    off: false,
                    owner: "p1".into(),
                    store: store(50),
                    notify_when_attacked: None,
                }),
            )
            .unwrap();
    }
    world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                id: 3,
                x: 12,
                y: 10,
                hits: 5000,
                off: false,
                name: "s1".into(),
                owner: "p1".into(),
                store: store(200),
                spawning: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    world
        .edit_room("W1N1", |room| {
            *room.events = vec![
                r#"{"event":"eventObjectCreated","objectId":"kk0000000000000001","data":{"type":"spawn"}}"#.into(),
                "{not-json".into(),
            ];
            Ok(())
        })
        .unwrap();
    // E2S2：无主无状态房（片段最简形态）；E3S2：p2 自有房（他人视角不可见）。
    world
        .insert(
            "E3S2",
            RoomObject::Generic(GenericObj {
                id: 0,
                x: 8,
                y: 8,
                data: r#"{"type":"structure","structureType":"road","owner":"p2","hits":1,"hitsMax":1}"#
                    .into(),
            }),
        )
        .unwrap();

    for (user, gcl) in [("p1", 42i64), ("p2", 0), ("stranger", -7)] {
        let reference = serde_json::to_string(&world.kkpv_envelope_for_viewer(user, gcl))
            .expect("Value 信封可序列化");
        let direct = world.kkpv_envelope_string_for_viewer(user, gcl);
        assert_eq!(
            reference, direct,
            "字符串直构与 Value 路径逐字节一致（查看者 {user}）"
        );
    }
    // 代际缓存复用：再次调用仍一致；换代（插入对象）后片段随状态重建。
    let again = world.kkpv_envelope_string_for_viewer("p1", 42);
    assert_eq!(
        serde_json::to_string(&world.kkpv_envelope_for_viewer("p1", 42)).unwrap(),
        again,
        "缓存复用不改变输出"
    );
    world
        .insert(
            "E2S2",
            RoomObject::Generic(GenericObj {
                id: 0,
                x: 9,
                y: 9,
                data: r#"{"type":"structure","structureType":"road","owner":"p2","hits":1,"hitsMax":1}"#
                    .into(),
            }),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_string(&world.kkpv_envelope_for_viewer("p1", 42)).unwrap(),
        world.kkpv_envelope_string_for_viewer("p1", 42),
        "换代后片段重建仍逐字节一致"
    );
}
