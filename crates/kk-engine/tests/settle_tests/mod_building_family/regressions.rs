//! Review 回归：生产先扣输入再校验最终容量；出生的两轮出口检查共用建筑障碍目录。
use super::*;

#[test]
fn full_input_store_can_produce_after_consumption_frees_capacity() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let factory = place_building(
        &mut f,
        "bm:foundry",
        20,
        20,
        serde_json::json!({"energy": 480, "bm:ore": 20}),
    );
    f.settle_with_plane(IntentBatch::default(), building_plane());
    assert_eq!(
        building_data(&f, factory)["store"],
        serde_json::json!({"energy": 430, "bm:ore": 0, "bm:ingot": 5}),
        "满仓 500 先消耗 70 再产出 5，最终库存 435 合法"
    );
}

#[test]
fn same_resource_input_and_output_are_checked_at_the_final_amount() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let mut recipe = foundry();
    recipe.production = Some(kk_mod_api::buildings::ProductionSpec {
        interval: 1,
        inputs: vec![("energy".into(), 50)],
        outputs: vec![("energy".into(), 20)],
    });
    let factory = place_building(
        &mut f,
        "bm:foundry",
        20,
        20,
        serde_json::json!({"energy": 500}),
    );
    f.settle_with_plane(IntentBatch::default(), building_plane_with(vec![recipe]));
    assert_eq!(
        building_data(&f, factory)["store"],
        serde_json::json!({"energy": 470}),
        "同一种资源消耗与产出按净变化结算"
    );
}

#[test]
fn production_accepts_exact_capacity_and_rejects_overflow_without_consumption() {
    for (output, expected) in [
        (
            70,
            serde_json::json!({"energy": 430, "bm:ore": 0, "bm:ingot": 70}),
        ),
        (71, serde_json::json!({"energy": 480, "bm:ore": 20})),
    ] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let mut recipe = foundry();
        recipe.production.as_mut().unwrap().outputs = vec![("bm:ingot".into(), output)];
        let factory = place_building(
            &mut f,
            "bm:foundry",
            20,
            20,
            serde_json::json!({"energy": 480, "bm:ore": 20}),
        );
        f.settle_with_plane(IntentBatch::default(), building_plane_with(vec![recipe]));
        assert_eq!(building_data(&f, factory)["store"], expected);
    }
}

/// 直接设置到期任务，只考察出生出口，不把等待孵化的多个 tick 混入断言。
fn ready_spawn(f: &mut Fixture, directions: Vec<u8>) -> (ObjId, ObjId) {
    f.controller(3, Some("p1"));
    let spawn = spawn_with_job(
        f,
        20,
        20,
        300,
        Some(kk_schema::room::SpawningJob {
            name: "child".into(),
            need_time: 3,
            spawn_time: 0,
            directions,
        }),
    );
    let child = f.creep("child", "p1", 20, 20, &[BodyKind::Move], 0);
    let Some(RoomObject::Creep(c)) = f.world.get_mut(child) else {
        panic!("出生夹具必须是 creep");
    };
    c.spawning = true;
    (spawn, child)
}

fn child_position(f: &Fixture, child: ObjId) -> (u8, u8, bool) {
    let RoomObject::Creep(c) = f.obj(child) else {
        panic!("雏形存在");
    };
    (c.x, c.y, c.spawning)
}

#[test]
fn spawn_waits_when_selected_exit_has_a_mod_obstacle_or_any_owned_site() {
    // 出生对任何主人的障碍工地阻挡；普通移动只挡自家工地，不能直接复用。
    for site_owner in [None, Some("p1"), Some("p2")] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let (spawn, child) = ready_spawn(&mut f, vec![3]);
        let obstacle = match site_owner {
            None => place_building(&mut f, "bm:foundry", 21, 20, Value::Null),
            Some(owner) => {
                let id = mod_site(&mut f, "bm:foundry", 21, 20, 0, 1000);
                let Some(RoomObject::ConstructionSite(site)) = f.world.get_mut(id) else {
                    panic!("工地夹具存在");
                };
                site.owner = Some(owner.into());
                id
            }
        };
        let plane = building_plane();
        f.settle_with_plane(IntentBatch::default(), plane.clone());
        assert_eq!(child_position(&f, child), (20, 20, true));
        let RoomObject::Spawn(s) = f.obj(spawn) else {
            panic!()
        };
        assert!(s.spawning.is_some(), "其他方向有空位但未选择，继续延期");
        f.world.remove(obstacle).unwrap();
        f.settle_with_plane(IntentBatch::default(), plane);
        assert_eq!(child_position(&f, child), (21, 20, false));
        let RoomObject::Spawn(s) = f.obj(spawn) else {
            panic!()
        };
        assert!(s.spawning.is_none(), "解除阻挡后清除任务");
    }
}

#[test]
fn spawn_skips_obstacle_exit_and_uses_road_or_plain_building_exit() {
    for passable_type in ["bm:solar", "bm:pass"] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let (_, child) = ready_spawn(&mut f, vec![3, 5]);
        place_building(&mut f, "bm:foundry", 21, 20, Value::Null);
        place_building(&mut f, passable_type, 20, 21, Value::Null);
        f.settle_with_plane(IntentBatch::default(), building_plane());
        assert_eq!(child_position(&f, child), (20, 21, false));
    }
}

#[test]
fn spawn_stomp_detects_mod_obstacles_in_unselected_exits() {
    for use_sites in [false, true] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let (_, child) = ready_spawn(&mut f, vec![3]);
        let hostile = f.creep("enemy", "p2", 21, 20, &[BodyKind::Move], 0);
        for (x, y) in [
            (20, 19),
            (21, 19),
            (21, 21),
            (20, 21),
            (19, 21),
            (19, 20),
            (19, 19),
        ] {
            if use_sites {
                let id = mod_site(&mut f, "bm:foundry", x, y, 0, 1000);
                let Some(RoomObject::ConstructionSite(site)) = f.world.get_mut(id) else {
                    panic!("工地夹具存在");
                };
                site.owner = Some("p2".into());
            } else {
                place_building(&mut f, "bm:foundry", x, y, Value::Null);
            }
        }
        f.settle_with_plane(IntentBatch::default(), building_plane());
        assert!(f.world.get(hostile).is_none(), "无其他出口时踩死选中格敌人");
        assert_eq!(child_position(&f, child), (21, 20, false));
    }
}
