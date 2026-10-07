//! 数据模型与管理入口的单元行为验证。
use super::*;
use kk_schema::room::EnergyObj;

fn energy(amount: u32) -> RoomObject {
    RoomObject::Energy(EnergyObj {
        id: 0,
        x: 1,
        y: 1,
        resource_type: 0,
        amount,
        resource: None,
    })
}

#[test]
fn generation_bumps_on_content_changes() {
    let mut world = World::new(1);
    assert_eq!((world.generation(), world.terrain_epoch()), (0, 0));
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let after_room = world.generation();
    assert_eq!((after_room, world.terrain_epoch()), (1, 1));
    // 已存在房重注册：不替换地形，不 bump
    world.register_room("W1N1", Terrain::plains()).unwrap();
    assert_eq!((world.generation(), world.terrain_epoch()), (1, 1));
    world.insert("W1N1", energy(1)).unwrap();
    assert_eq!(world.generation(), 2);
    assert_eq!(world.terrain_epoch(), 1, "对象变更不动地形纪元");
    // tick 推进（结算提交位）：时钟折叠面变化 ⇒ 代际必须失效
    world.advance_ticks(1).unwrap();
    let after_tick = world.generation();
    assert_eq!(after_tick, 3);
    // player_ticks>0 后 set_terrain 拒绝（不 bump）
    assert!(world.set_terrain("W1N1", Terrain::plains()).is_err());
    assert_eq!((world.generation(), world.terrain_epoch()), (after_tick, 1));
    // 句柄 = tick + generation 的快照
    let handle = world.room_version("W1N1").unwrap();
    assert_eq!(
        handle,
        RoomVersionHandle {
            room: "W1N1".into(),
            tick: 1,
            generation: 3
        }
    );
    assert!(world.room_version("nope").is_none());
    // 分别检查地形更新、删除和可变访问的版本变化。
    let mut fresh = World::new(2);
    fresh.register_room("E1S1", Terrain::plains()).unwrap();
    fresh.set_terrain("E1S1", Terrain::plains()).unwrap();
    assert_eq!((fresh.generation(), fresh.terrain_epoch()), (2, 2));
    let id = fresh.insert("E1S1", energy(1)).unwrap();
    fresh.remove(id).unwrap();
    assert_eq!(fresh.generation(), 4, "insert→remove 净 +2");
    let _ = fresh.get_mut(id);
    assert_eq!(fresh.generation(), 5, "get_mut 保守 +1（对象已删仍计）");
    fresh.room_mut("E1S1");
    assert_eq!(fresh.generation(), 6, "room_mut 保守 +1");
}

#[test]
fn obj_id_str_roundtrip() {
    let id = ObjId(255);
    assert_eq!(ObjId::parse(&id.as_str()), Some(id));
    assert_eq!(ObjId::parse("zz"), None);
}

#[test]
fn ids_are_globally_unique_across_rooms() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world.register_room("E2S3", Terrain::plains()).unwrap();
    let a = world.insert("W1N1", energy(1)).unwrap();
    let b = world.insert("E2S3", energy(2)).unwrap();
    assert_ne!(a, b, "跨房间 id 必须唯一");
    assert_eq!(world.get(a).unwrap().0, "W1N1");
    assert_eq!(world.get(b).unwrap().0, "E2S3");
}

#[test]
fn insert_unknown_room_errs() {
    let mut world = World::new(1);
    assert!(world.insert("nowhere", energy(1)).is_err());
}

#[test]
fn insert_rejects_invalid_position_and_exhausted_ids() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let mut invalid = energy(1);
    if let RoomObject::Energy(obj) = &mut invalid {
        obj.y = 50;
    }
    assert!(world.insert("W1N1", invalid).is_err());
    assert_eq!(world.next_obj_id(), 1);

    world.bump_next_obj_id(u64::MAX);
    assert!(world.insert("W1N1", energy(1)).is_err());
    assert!(world.room("W1N1").unwrap().objects.is_empty());
}

/// 手工导入高 ID 后，查询和分配器须刷新缓存；删除它后候选 ID 可回落。
#[test]
fn obj_dir_lookup_remove_and_next_id_equivalence() {
    let mut world = World::new(1);
    for name in ["W1N1", "W2N2", "E3S3"] {
        world.register_room(name, Terrain::plains()).unwrap();
    }
    let a = world.insert("W1N1", energy(10)).unwrap();
    let b = world.insert("W2N2", energy(20)).unwrap();
    // 手工导入（room_mut → 代际 bump → 目录重建吸收）
    world
        .room_mut("E3S3")
        .unwrap()
        .insert_with_id(900, energy(30))
        .unwrap();
    // 目录新鲜度断言：手工导入后未触达 → 下次触达重建
    for (id, room, amount) in [(a, "W1N1", 10), (b, "W2N2", 20), (ObjId(900), "E3S3", 30)] {
        let (name, obj) = world.get(id).unwrap();
        assert_eq!(name, room, "目录定位 {id:?}");
        match obj {
            RoomObject::Energy(e) => assert_eq!(e.amount, amount),
            _ => panic!("非 energy"),
        }
    }
    assert_eq!(world.next_obj_id(), 901, "手工导入高 id 抬升候选");
    // 移除后目录失效（remove bump）→ get None；max 重算不含已移除
    assert!(world.remove(ObjId(900)).unwrap().is_some());
    assert!(world.get(ObjId(900)).is_none());
    // 游标只保留普通分配所用的范围；手工导入的 900 被删除后不再抬高候选。
    assert_eq!(
        world.next_obj_id(),
        3,
        "移除导入 max 后候选回落（扫描同构）"
    );
    // get_mut 经目录定位
    if let RoomObject::Energy(e) = world.get_mut(b).unwrap() {
        e.amount = 25;
    }
    match world.get(b).unwrap().1 {
        RoomObject::Energy(e) => assert_eq!(e.amount, 25),
        _ => panic!("非 energy"),
    }
}

/// 构造场景时设置 time 不改变版本或地形修改的限制条件。
#[test]
fn set_time_for_test_invalidates_content_without_charging_player_ticks() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let g0 = world.generation();
    let pt0 = world.player_ticks;
    world.set_time_for_test(5).unwrap();
    assert_eq!(world.time(), 5);
    assert_eq!(world.generation(), g0 + 1, "摆钟须失效投影代际");
    assert_eq!(world.player_ticks, pt0, "摆钟不动 player_ticks");
}

/// 批量写回后推进时间，目录及最大 ID 缓存须看到新对象。
#[test]
fn obj_dir_rebuilds_after_settle_style_commit() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let a = world.insert("W1N1", energy(1)).unwrap();
    // 模拟引擎先替换房间，再统一推进时间的提交顺序。
    let mut room = RoomState::new("W1N1", Terrain::plains());
    room.insert_with_id(a.0, energy(2)).unwrap();
    room.insert_with_id(777, energy(3)).unwrap();
    world.commit_room("W1N1".to_string(), room);
    world.advance_ticks(1).unwrap();
    // bump 后首次触达重建 → 新对象可见
    assert!(world.get(ObjId(777)).is_some(), "提交尾新对象经目录可见");
    assert_eq!(world.next_obj_id(), 778);
}

/// 首次查询建立目录后，连续插入须增量维护目录和最大 ID 缓存。
#[test]
fn obj_dir_eager_maintenance_on_consecutive_inserts() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    for i in 0..50 {
        let id = world.insert("W1N1", energy(i)).unwrap();
        // max 缓存增量续档：每次 insert 后保持新鲜
        assert_eq!(
            *world.max_id_cache.borrow(),
            (world.generation, (i + 1) as u64),
            "第 {i} 次 insert 后 max 缓存新鲜"
        );
        if i == 0 {
            // 目录首次触达（读侧消费者出现）
            assert!(world.get(id).is_some());
        }
        // 目录增量续档：触达后保持新鲜
        assert_eq!(
            world.obj_dir.borrow().generation,
            world.generation,
            "第 {i} 次 insert 后目录新鲜"
        );
    }
    assert_eq!(world.obj_dir.borrow().rooms.len(), 50);
}

/// 陈旧目录不能只补新插入对象就标为有效。
/// 删除或导入后立即插入，再检查定位与分配结果，防止丢失未吸收的变更。
#[test]
fn obj_dir_guard_skips_extension_when_stale() {
    // 形态一：remove 致陈旧 → insert → 触达
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let _ = world.next_obj_id(); // 暖缓存（稳态）
    let a = world.insert("W1N1", energy(1)).unwrap();
    assert!(world.remove(a).unwrap().is_some()); // bump，两缓存陈旧
    let b = world.insert("W1N1", energy(2)).unwrap(); // 不得续档
    assert_eq!(b.0, 2, "游标分配");
    assert_eq!(
        world.next_obj_id(),
        3,
        "重建吸收 remove（不残留 a 的 max=1 作 2）"
    );
    assert!(world.get(a).is_none());
    assert!(world.get(b).is_some());

    // 形态二：room_mut 手工导入高 id 致陈旧 → insert → 触达
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let _ = world.next_obj_id(); // 暖缓存
    let c = world.insert("W1N1", energy(1)).unwrap();
    world
        .room_mut("W1N1")
        .unwrap()
        .insert_with_id(900, energy(9))
        .unwrap(); // bump，缓存陈旧
    let d = world.insert("W1N1", energy(2)).unwrap(); // 不得续档
    assert_eq!(d.0, 901, "分配必须看到导入的 900（重建吸收）");
    assert_eq!(world.next_obj_id(), 902);
    assert!(world.get(ObjId(900)).is_some());
    assert!(world.get(c).is_some());
    assert!(world.get(d).is_some());
}

#[test]
fn insert_after_manual_import_keeps_ids_unique_across_rooms() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world.register_room("W2N2", Terrain::plains()).unwrap();
    world
        .room_mut("W1N1")
        .unwrap()
        .insert_with_id(1, energy(1))
        .unwrap();
    world
        .room_mut("W2N2")
        .unwrap()
        .insert_with_id(5, energy(2))
        .unwrap();

    assert_eq!(world.next_obj_id(), 6);
    assert_eq!(world.insert("W1N1", energy(3)).unwrap(), ObjId(6));
    assert_eq!(world.room("W1N1").unwrap().objects.len(), 2);
    assert_eq!(world.room("W2N2").unwrap().objects.len(), 1);
    assert!(
        world
            .room_mut("W1N1")
            .unwrap()
            .insert_with_id(1, energy(99))
            .is_err()
    );
    assert!(matches!(
        world.get(ObjId(1)),
        Some((_, RoomObject::Energy(EnergyObj { amount: 1, .. })))
    ));
}

/// Mod 对象随二进制快照往返（新标签编码）：type_key 与字段 payload
/// 字节保持，混合对象表内顺序不变；查询快照读面带类型键与字段。
#[test]
fn snapshot_roundtrip_carries_mod_objects() {
    let mut world = World::new(42);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world.insert("W1N1", energy(1)).unwrap();
    let mod_id = world
        .insert(
            "W1N1",
            RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x: 10,
                y: 12,
                type_key: "extmod:counter".into(),
                data: r#"{"owner":"p1","x":10,"y":12,"value":3}"#.into(),
            }),
        )
        .unwrap();
    let snap = world
        .room("W1N1")
        .unwrap()
        .encode_snapshot(77, 42, world.next_obj_id());
    let back = RoomState::decode_snapshot(&snap).unwrap();
    assert_eq!(back.encode_snapshot(77, 42, world.next_obj_id()), snap);
    let mod_obj = back
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Mod(m) => Some(m),
            _ => None,
        })
        .unwrap();
    assert_eq!(mod_obj.type_key, "extmod:counter");
    assert_eq!(mod_obj.x, 10);
    assert!(mod_obj.data.contains("\"value\":3"));
    world.commit_room("W1N1".into(), back);
    let snapshot = world.object_snapshot_by_id(mod_id).unwrap();
    assert_eq!(snapshot["kind"], "mod");
    assert_eq!(snapshot["typeKey"], "extmod:counter");
    assert_eq!(snapshot["fields"]["value"], 3);
}

/// Mod 声明的命名空间资源随二进制快照往返：store_extra 的资源名与
/// 数量、命名的掉落堆都按既有 ResourceStack/EnergyObj 编码保持，
/// 查询快照读面带资源键（N2a 资源目录的快照面不引入新编码）。
#[test]
fn snapshot_roundtrip_carries_mod_named_resources() {
    let mut world = World::new(42);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let creep_id = world
        .insert(
            "W1N1",
            RoomObject::Creep(kk_schema::room::CreepObj {
                id: 0,
                x: 10,
                y: 12,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "hauler".into(),
                owner: "p1".into(),
                body: vec![kk_schema::room::BodyPart {
                    kind: crate::types::BodyKind::Carry.code(),
                    hits: 100,
                    boost: None,
                }],
                store: kk_schema::room::Store::default(),
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "resmod:crystal".into(),
                    amount: 12,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 11,
                y: 12,
                resource_type: 0,
                amount: 7,
                resource: Some("resmod:dust".into()),
            }),
        )
        .unwrap();
    let snap = world
        .room("W1N1")
        .unwrap()
        .encode_snapshot(77, 42, world.next_obj_id());
    let back = RoomState::decode_snapshot(&snap).unwrap();
    assert_eq!(back.encode_snapshot(77, 42, world.next_obj_id()), snap);
    let creep = match back.objects.get(&ObjId(creep_id.0)).expect("id 保持") {
        RoomObject::Creep(c) => c,
        _ => panic!("对象族保持"),
    };
    assert_eq!(
        creep.store_extra,
        vec![kk_schema::room::ResourceStack {
            resource: "resmod:crystal".into(),
            amount: 12,
        }]
    );
    let pile = back
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.resource.as_deref() == Some("resmod:dust") => Some(e),
            _ => None,
        })
        .unwrap();
    assert_eq!(pile.amount, 7);
    world.commit_room("W1N1".into(), back);
    let snapshot = world.object_snapshot_by_id(creep_id).unwrap();
    assert_eq!(snapshot["store"]["resmod:crystal"], 12);
}

#[test]
fn snapshot_roundtrip_preserves_order_and_ids() {
    let mut world = World::new(42);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world.insert("W1N1", energy(1)).unwrap();
    world.insert("W1N1", energy(2)).unwrap();
    let snap = world
        .room("W1N1")
        .unwrap()
        .encode_snapshot(77, 42, world.next_obj_id());
    let back = RoomState::decode_snapshot(&snap).unwrap();
    assert_eq!(back.encode_snapshot(77, 42, world.next_obj_id()), snap);
}

#[test]
fn restored_room_queries_use_world_map_key() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let id = world.insert("W1N1", energy(1)).unwrap();
    let snapshot = world.room("W1N1").unwrap().encode_snapshot(0, 1, 2);
    let restored = RoomState::decode_snapshot(&snapshot).unwrap();
    assert!(restored.name.is_empty());
    world.commit_room("W1N1".into(), restored);
    world.advance_ticks(1).unwrap();
    assert_eq!(world.get(id).unwrap().0, "W1N1");
    assert_eq!(
        world.object_json_by_id(id).unwrap()["pos"]["roomName"],
        "W1N1"
    );
    assert_eq!(
        world.object_snapshot_by_id(id).unwrap()["pos"]["roomName"],
        "W1N1"
    );
}

#[test]
fn snapshot_restore_recovers_next_id_and_rejects_bad_terrain() {
    // header.next_obj_id 落后于对象表：World 重建侧用 bump 抬高后不复用
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world.insert("W1N1", energy(1)).unwrap();
    world.insert("W1N1", energy(2)).unwrap();
    let snap = world.room("W1N1").unwrap().encode_snapshot(0, 0, 1);
    let mut world2 = World::new(1);
    world2.register_room("W1N1", Terrain::plains()).unwrap();
    for object in &snap.objects {
        world2
            .room_mut("W1N1")
            .unwrap()
            .insert_with_id(object.id(), object.clone())
            .unwrap();
    }
    let max_id = snap.objects.iter().map(|o| o.id()).max().unwrap_or(0);
    world2.bump_next_obj_id(snap.header.next_obj_id.max(max_id + 1));
    assert_eq!(world2.insert("W1N1", energy(3)).unwrap().0, 3);

    // 地形位域防御：快照字节含未知位 → 拒载
    let mut bad = world.room("W1N1").unwrap().encode_snapshot(0, 0, 3);
    bad.header.terrain[10] = 0xff;
    assert!(RoomState::decode_snapshot(&bad).is_err());
}

#[test]
fn decode_rejects_duplicate_ids_and_out_of_bounds_positions() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world.insert("W1N1", energy(1)).unwrap();
    let mut snapshot = world.room("W1N1").unwrap().encode_snapshot(0, 1, 2);
    snapshot.objects.push(snapshot.objects[0].clone());
    assert!(
        RoomState::decode_snapshot(&snapshot)
            .unwrap_err()
            .contains("id 重复")
    );

    snapshot.objects.pop();
    if let RoomObject::Energy(obj) = &mut snapshot.objects[0] {
        obj.x = 50;
    }
    assert!(
        RoomState::decode_snapshot(&snapshot)
            .unwrap_err()
            .contains("坐标越界")
    );
}
