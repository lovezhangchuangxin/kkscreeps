//! 私有束按可见集分组共享的单元测试（从 tests.rs 按职责拆出）。
use super::*;
use kk_core::Terrain;
use kk_schema::room::{ConstructionSiteObj, FlagObj, GenericObj, RoomObject};

#[test]
fn grouped_bundles_share_identical_bytes_for_overlapping_viewers() {
    // 束分组共享的等价基础：视野重叠的查看者（互驻军——两玩家在同样的
    // 房间各有对象；探索——observer 补足对方房视野）可见集相同、分组键
    // 相同；世界全房可共享且空块可共享时，其逐查看者装配束把 audience
    // 改写位归零后逐字节一致。可见集不同的查看者键不同、束不同；世界
    // 带归属旗/归属工地（块字节依赖查看者裁切或己方工地候选）时分组
    // 资格失效。
    let mut world = World::new(9);
    world.set_time_for_test(30).unwrap();
    for room in ["W1N1", "E2S2"] {
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room(room, terrain).unwrap();
    }
    let insert_owner = |world: &mut World, room: &str, owner: &str, x: u8| {
        world
            .insert(
                room,
                RoomObject::Generic(GenericObj {
                    id: 0,
                    x,
                    y: 8,
                    data: format!(
                        r#"{{"type":"structure","structureType":"road","owner":"{owner}","hits":1,"hitsMax":1}}"#
                    )
                    .into(),
                }),
            )
            .expect("夹具对象可插入");
    };
    // p1/p2 互驻军：两房各有双方对象 → 可见集同为 {W1N1, E2S2}。
    for owner in ["p1", "p2"] {
        insert_owner(&mut world, "W1N1", owner, 8);
        insert_owner(&mut world, "E2S2", owner, 10);
    }
    // p3 只在 W1N1 驻军 → 可见集 {W1N1}。
    insert_owner(&mut world, "W1N1", "p3", 12);
    // p4 探索：自有对象仅在 E2S2，observer 观察 W1N1 → 可见集补齐为
    // {W1N1, E2S2}，与 p1/p2 同组。
    insert_owner(&mut world, "E2S2", "p4", 12);
    world
        .insert(
            "E2S2",
            RoomObject::Generic(GenericObj {
                id: 0,
                x: 14,
                y: 8,
                data: r#"{"type":"structure","structureType":"observer","owner":"p4","observeRoom":"W1N1"}"#
                    .into(),
            }),
        )
        .expect("observer 夹具可插入");

    assert!(
        world.kkpv_grouping_eligible([7u8; 32]),
        "无旗/工地/Mod/扩展的世界可分组"
    );
    let key = |user: &str| world.kkpv_viewer_group_key(user);
    assert_eq!(key("p1"), key("p2"), "互驻军查看者同组");
    assert_eq!(key("p1"), key("p4"), "observer 补足视野后同组");
    assert_ne!(key("p1"), key("p3"), "可见集不同的查看者不同组");

    let catalog = kk_mod_api::extension::ExtensionCatalog::default();
    let audience = |user: &str| {
        let mut hash = [0u8; 32];
        hash[..user.len()].copy_from_slice(user.as_bytes());
        hash
    };
    let neutralized = |user: &str| {
        let (_, bundle) = world
            .kkpv_bundle_for_viewer(user, 42, &catalog, None, [7u8; 32], audience(user))
            .unwrap_or_else(|e| panic!("查看者 {user} 装配失败: {e}"));
        let mut bytes = bundle;
        let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let mut at = 4 + count * 4;
        for i in 0..count {
            let len = u32::from_le_bytes(bytes[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
            kk_schema::kkpv::patch_block_audience(&mut bytes[at..at + len], &[0u8; 32])
                .expect("audience 归零改写合法");
            at += len;
        }
        assert_eq!(at, bytes.len(), "分帧走查恰好覆盖整束");
        bytes
    };
    assert_eq!(
        neutralized("p1"),
        neutralized("p2"),
        "同组查看者的束（audience 归零后）必须逐字节一致"
    );
    assert_eq!(
        neutralized("p1"),
        neutralized("p4"),
        "observer 同组查看者的束（audience 归零后）必须逐字节一致"
    );
    assert_ne!(
        neutralized("p1"),
        neutralized("p3"),
        "异组查看者的束必须不同（E2S2 全块 vs 空块）"
    );
    // 信封配套：分组命中时逐玩家信封仍可得（user/gcl 逐玩家）。
    assert!(world.kkpv_envelope_json_for_viewer("p1", 42).is_object());

    // 资格失效一：归属旗使块依赖查看者裁切（敌旗排除）。
    let mut flagged = World::new(9);
    flagged.set_time_for_test(30).unwrap();
    for room in ["W1N1", "E2S2"] {
        let mut terrain = Terrain::plains();
        terrain.normalize();
        flagged.register_room(room, terrain).unwrap();
    }
    insert_owner(&mut flagged, "W1N1", "p1", 8);
    flagged
        .insert(
            "E2S2",
            RoomObject::Flag(FlagObj {
                id: 0,
                x: 3,
                y: 3,
                name: "owned-flag".into(),
                color: 1,
                secondary_color: 1,
                owner: Some("p2".into()),
            }),
        )
        .expect("夹具旗可插入");
    assert!(
        !flagged.kkpv_grouping_eligible([7u8; 32]),
        "带归属旗的世界不可分组"
    );

    // 资格失效二：归属工地使空块必须逐查看者枚举己方工地。
    let mut sited = World::new(9);
    sited.set_time_for_test(30).unwrap();
    for room in ["W1N1", "E2S2"] {
        let mut terrain = Terrain::plains();
        terrain.normalize();
        sited.register_room(room, terrain).unwrap();
    }
    insert_owner(&mut sited, "W1N1", "p1", 8);
    sited
        .insert(
            "E2S2",
            RoomObject::ConstructionSite(ConstructionSiteObj {
                id: 0,
                x: 4,
                y: 4,
                structure_type: "road".into(),
                progress: 0,
                progress_total: 100,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .expect("夹具工地可插入");
    assert!(
        !sited.kkpv_grouping_eligible([7u8; 32]),
        "带归属工地的世界不可分组"
    );
}
