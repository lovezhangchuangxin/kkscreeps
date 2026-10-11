//! 行级脏位启用后的共享装配语义锁。
#![cfg(test)]
use super::*;
use kk_core::Terrain;
use kk_schema::kkpv::KkpvView;
use kk_schema::room::{ControllerObj, RoomObject};

#[test]
fn dirty_shared_blocks_match_fresh_semantics_on_second_tick() {
    // 行级脏位启用后的语义锁：第二拍（世界已变更、共享 full 块带脏位
    // 段）与逐查看者全 fresh 编码必须逐对象逐字段等值；同时断言插入行
    // 命中脏位、存量行干净。字节级不再可比（fresh 无脏位段），值级等价
    // 即解码端干净行跳过的安全性来源。
    let mut world = World::new(11);
    world.set_time_for_test(500).unwrap();
    world.register_room("S1N1", Terrain::plains()).unwrap();
    let road = |x: u8, y: u8| {
        RoomObject::Road(kk_schema::room::RoadObj {
            id: 0,
            x,
            y,
            hits: 1_000,
            next_decay_time: 0,
            notify_when_attacked: Some(true),
        })
    };
    world
        .insert(
            "S1N1",
            RoomObject::Controller(ControllerObj {
                id: 0,
                x: 5,
                y: 5,
                level: 3,
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
        .expect("controller 可插入");
    let old_1 = world.insert("S1N1", road(7, 6)).expect("road1 可插入");
    let old_2 = world.insert("S1N1", road(8, 6)).expect("road2 可插入");
    let catalog = kk_mod_api::extension::ExtensionCatalog::default();
    let audience = [3u8; 32];
    // 第一拍：填充共享编码与脏位保留。
    world
        .kkpv_bundle_for_viewer("p1", 42, &catalog, None, [7; 32], audience)
        .unwrap();
    // 世界变更：插入第三条 road（换代使共享缓存重建、脏位比对生效）。
    let inserted = world.insert("S1N1", road(9, 6)).expect("road3 可插入");
    let (_, assembled) = world
        .kkpv_bundle_for_viewer("p1", 42, &catalog, None, [7; 32], audience)
        .unwrap();
    let reference = world
        .kkpv_fresh_bundle_bytes_for_viewer("p1", &catalog, None, [7; 32], audience)
        .unwrap();
    let assembled_blocks = kk_schema::kkpv::bundle_block_slices(&assembled).unwrap();
    let reference_blocks = kk_schema::kkpv::bundle_block_slices(&reference).unwrap();
    assert_eq!(assembled_blocks.len(), reference_blocks.len());
    let limits = kk_schema::kkpv::KkpvLimits::default();
    let mut target: Option<KkpvView> = None;
    for i in 0..assembled_blocks.len() {
        let a = assembled_blocks[i];
        let r = reference_blocks[i];
        let va = KkpvView::decode(a, &limits).unwrap();
        let vr = KkpvView::decode(r, &limits).unwrap();
        assert_eq!(va.room_name(), vr.room_name());
        assert_eq!(va.object_count(), vr.object_count());
        if va.room_name() == "S1N1" {
            target = Some(KkpvView::decode(a, &limits).unwrap());
        }
        for i in 0..va.object_count() {
            assert_eq!(va.object_id(i), vr.object_id(i), "对象集合一致");
            assert_eq!(va.object_type(i), vr.object_type(i), "类型桶一致");
            assert_eq!(
                va.field_keys(i),
                vr.field_keys(i),
                "字段键集合一致（{}）",
                va.object_id(i)
            );
            for key in va.field_keys(i) {
                assert_eq!(
                    va.field(i, key).unwrap(),
                    vr.field(i, key).unwrap(),
                    "字段值等价（{} {}）",
                    va.object_id(i),
                    key
                );
            }
        }
    }
    let view = target.expect("束内含 S1N1 块");
    assert_eq!(view.object_count(), 4);
    assert!(view.row_dirty(0).is_some(), "S1N1 共享块第二拍携带脏位段");
    // 插入行脏、存量行按 ID 对位干净（值级未变）。
    let dirty_of = |id: ObjId| {
        let i = view.object_index_by_id(id.0).expect("对象在目录中");
        view.row_dirty(i)
    };
    assert_eq!(dirty_of(inserted), Some(true), "插入行命中脏位");
    assert_eq!(dirty_of(old_1), Some(false), "存量 road 干净");
    assert_eq!(dirty_of(old_2), Some(false), "存量 road 干净");
}
