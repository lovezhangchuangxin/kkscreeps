//! Mod 建筑的结算面：工地创建、多资源建造与完工、生产周期、受击/
//! 维修/拆解、寻路障碍与工地退款。配方经冻结计划进入贡献平面。
use super::*;

mod lifecycle;
mod regressions;

fn foundry() -> kk_mod_api::buildings::BuildingDescriptor {
    kk_mod_api::buildings::BuildingDescriptor {
        key: kk_mod_api::Key::new("bm:foundry").unwrap(),
        player_class: "Foundry".into(),
        cost: vec![("bm:ore".into(), 100), ("energy".into(), 1000)],
        hits_max: 2000,
        pathing: kk_mod_api::buildings::BuildingPathing::Obstacle,
        store_capacity: Some(500),
        production: Some(kk_mod_api::buildings::ProductionSpec {
            interval: 10,
            inputs: vec![("bm:ore".into(), 20), ("energy".into(), 50)],
            outputs: vec![("bm:ingot".into(), 5)],
        }),
        fields: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
    }
}

fn solar() -> kk_mod_api::buildings::BuildingDescriptor {
    kk_mod_api::buildings::BuildingDescriptor {
        key: kk_mod_api::Key::new("bm:solar").unwrap(),
        player_class: "Solar".into(),
        cost: vec![("energy".into(), 400)],
        hits_max: 300,
        pathing: kk_mod_api::buildings::BuildingPathing::Road,
        store_capacity: Some(100),
        production: Some(kk_mod_api::buildings::ProductionSpec {
            interval: 5,
            inputs: Vec::new(),
            outputs: vec![("energy".into(), 20)],
        }),
        fields: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
    }
}

fn pass_thru() -> kk_mod_api::buildings::BuildingDescriptor {
    kk_mod_api::buildings::BuildingDescriptor {
        key: kk_mod_api::Key::new("bm:pass").unwrap(),
        player_class: "Pass".into(),
        cost: vec![("energy".into(), 100)],
        hits_max: 100,
        pathing: kk_mod_api::buildings::BuildingPathing::Plain,
        store_capacity: None,
        production: None,
        fields: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
    }
}

/// 声明 bm:* 资源与 foundry/solar/pass 建筑的贡献平面。
fn building_plane() -> kk_engine::settle::Plane {
    building_plane_with(vec![foundry(), pass_thru(), solar()])
}

fn building_plane_with(
    buildings: Vec<kk_mod_api::buildings::BuildingDescriptor>,
) -> kk_engine::settle::Plane {
    let mut contribution = kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "bm".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::new(),
        field_mounts: Vec::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: vec![
            kk_mod_api::resources::ResourceDescriptor {
                key: kk_mod_api::Key::new("bm:ingot").unwrap(),
            },
            kk_mod_api::resources::ResourceDescriptor {
                key: kk_mod_api::Key::new("bm:ore").unwrap(),
            },
        ],
        buildings,
    };
    contribution.resources.sort_by(|a, b| a.key.cmp(&b.key));
    let plan = std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    );
    kk_engine::settle::Plane {
        links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
        stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
        combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
        move_gate: Default::default(),
        terrain_passability: Default::default(),
        resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
        buildings: std::rc::Rc::new(
            kk_mod_api::buildings::BuildingCatalog::from_plan(&plan).unwrap(),
        ),
        contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan,
    }
}

/// 直接放置 Mod 建筑工地（宿主夹具口径；真实世界经玩家指令或场景放置）。
fn mod_site(
    f: &mut Fixture,
    structure_type: &str,
    x: u8,
    y: u8,
    progress: u32,
    total: u32,
) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x,
                y,
                structure_type: structure_type.into(),
                progress,
                progress_total: total,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .unwrap()
}

/// 直接放置完工的 Mod 建筑（带初始库存可选项）。
fn place_building(f: &mut Fixture, key: &str, x: u8, y: u8, store: Value) -> ObjId {
    let hits = if key == "bm:foundry" { 2000 } else { 300 };
    let capacity = if key == "bm:foundry" { 500 } else { 100 };
    let mut data = serde_json::json!({
        "type": "structure", "structureType": key, "owner": "p1",
        "hits": hits, "hitsMax": hits, "storeCapacity": capacity,
    });
    if !store.is_null() {
        data["store"] = store;
    } else {
        data["store"] = serde_json::json!({"energy": 0});
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

fn building_data(f: &Fixture, id: ObjId) -> Value {
    match f.obj(id) {
        RoomObject::Generic(g) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("非 Generic 结构：{other:?}"),
    }
}

fn site_intent(structure_type: &str, x: i64, y: i64) -> IntentBatch {
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().room.push(
        kk_engine::intent::RoomIntent::CreateConstructionSite {
            room: "W1N1".into(),
            x,
            y,
            structure_type: structure_type.into(),
            name: None,
        },
    );
    batch
}

fn build_intent(creep: ObjId, site: ObjId) -> IntentBatch {
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", creep, Action::Build, IntentData::target(site));
    batch
}

/// 玩家指令可创建 Mod 建筑工地：总进度取配方最大成本量，RCL 配额不设限
/// （rcl1 也能建）；同格造价对象互斥照常生效。
#[test]
fn mod_site_creation_uses_recipe_total_and_skips_rcl_quota() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    f.settle_with_plane(site_intent("bm:foundry", 20, 20), building_plane());
    let site = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::ConstructionSite(s) if s.structure_type == "bm:foundry" => Some(s.clone()),
            _ => None,
        })
        .expect("工地已创建");
    assert_eq!(site.progress_total, 1000, "总进度 = 最大成本量");
    assert_eq!(site.owner.as_deref(), Some("p1"));
    // 同格再放一个工地被拒（既有互斥语义覆盖 Mod 建筑）。
    f.settle_with_plane(site_intent("bm:solar", 20, 20), building_plane());
    let count = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter(|o| matches!(o, RoomObject::ConstructionSite(_)))
        .count();
    assert_eq!(count, 1);
}

/// 多资源成本按累计比例支付：单 tick 部分建造时 energy 与 bm:ore 各按
/// 比例扣减；单资源成本的 solar 退化为 1:1 能量行为。
#[test]
fn multi_resource_build_pays_proportionally() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let builder = f.creep_with_mineral(
        "b",
        20,
        19,
        &[BodyKind::Work, BodyKind::Work],
        100,
        "bm:ore",
        100,
    );
    let site = mod_site(&mut f, "bm:foundry", 20, 20, 0, 1000);
    f.settle_with_plane(build_intent(builder, site), plane);
    // 2 WORK × 5 = 10 点：energy 付 10、ore 付 floor(100·10/1000)=1。
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => assert_eq!(s.progress, 10),
        other => panic!("工地仍在：{other:?}"),
    }
    match f.obj(builder) {
        RoomObject::Creep(c) => {
            assert_eq!(c.store.energy, 90);
            assert_eq!(
                kk_world::types::store_extra_slot(&c.store, &c.store_extra, "bm:ore"),
                99
            );
        }
        _ => panic!(),
    }
}

/// 建造完成：工地移除，落成 Generic 结构携带耐久/归属/仓库默认值，
/// 各成本资源恰好付清。
#[test]
fn build_completion_creates_generic_structure_and_pays_in_full() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let builder = f.creep_with_mineral("b", 20, 19, &[BodyKind::Work; 10], 1000, "bm:ore", 100);
    let site = mod_site(&mut f, "bm:foundry", 20, 20, 950, 1000);
    f.settle_with_plane(build_intent(builder, site), plane);
    assert!(f.world.get(site).is_none(), "完工后工地移除");
    let building = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Generic(g) => {
                let data: Value = serde_json::from_str(&g.data).unwrap();
                (data["structureType"] == "bm:foundry").then_some(g.clone())
            }
            _ => None,
        })
        .expect("建筑落成");
    let data: Value = serde_json::from_str(&building.data).unwrap();
    assert_eq!(data["hits"], 2000);
    assert_eq!(data["hitsMax"], 2000);
    assert_eq!(data["owner"], "p1");
    assert_eq!(data["storeCapacity"], 500);
    assert_eq!(data["store"]["energy"], 0);
    // 尾款：energy 付 cum(1000,1000,1000)-cum(1000,1000,950)=50，
    // ore 付 100-95=5。
    match f.obj(builder) {
        RoomObject::Creep(c) => {
            assert_eq!(c.store.energy, 950);
            assert_eq!(
                kk_world::types::store_extra_slot(&c.store, &c.store_extra, "bm:ore"),
                95
            );
        }
        _ => panic!(),
    }
}

/// 单一 energy 成本的 solar：支付与进度 1:1，与内建结构行为一致；
/// 完工产物 hitsMax 来自配方。
#[test]
fn single_resource_cost_degenerates_to_builtin_behavior() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let builder = f.creep("b", "p1", 20, 19, &[BodyKind::Work; 10], 100);
    let site = mod_site(&mut f, "bm:solar", 20, 20, 350, 400);
    f.settle_with_plane(build_intent(builder, site), plane);
    match f.obj(builder) {
        RoomObject::Creep(c) => assert_eq!(c.store.energy, 50, "付 50 点 = 50 能量"),
        _ => panic!(),
    }
    let building = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Generic(g) => {
                let data: Value = serde_json::from_str(&g.data).unwrap();
                (data["structureType"] == "bm:solar").then_some(data)
            }
            _ => None,
        })
        .expect("solar 落成");
    assert_eq!(building["hitsMax"], 300);
    assert_eq!(building["storeCapacity"], 100);
}

/// 生产周期：周期 tick 上输入充足则原子消耗并产出；非周期 tick 或输入
/// 不足不生产；产出受共享容量限制。
#[test]
fn production_cycles_consume_inputs_and_respect_capacity() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let foundry = place_building(
        &mut f,
        "bm:foundry",
        20,
        20,
        serde_json::json!({"energy": 60, "bm:ore": 20}),
    );
    // 世界时间 0：0 % 10 == 0，首个周期生产。
    f.settle_with_plane(IntentBatch::default(), plane.clone());
    let data = building_data(&f, foundry);
    assert_eq!(data["store"]["energy"], 10, "消耗 50 能量");
    assert_eq!(data["store"]["bm:ore"], 0, "消耗 20 矿");
    assert_eq!(data["store"]["bm:ingot"], 5, "产出 5 锭");
    // 时间 1：非周期 tick 不生产。
    f.settle_with_plane(IntentBatch::default(), plane.clone());
    let data = building_data(&f, foundry);
    assert_eq!(data["store"]["bm:ingot"], 5);
    // 输入不足（energy 10 < 50）：周期 tick（时间 10）也不生产。
    for _ in 2..10 {
        f.settle_with_plane(IntentBatch::default(), plane.clone());
    }
    let data = building_data(&f, foundry);
    assert_eq!(data["store"]["energy"], 10, "输入不足不消耗");
    assert_eq!(data["store"]["bm:ingot"], 5);
    // 免费生产的 solar：无输入要求，产出至容量后停（时间 10/15 各产 20）。
    let solar = place_building(
        &mut f,
        "bm:solar",
        25,
        25,
        serde_json::json!({"energy": 60}),
    );
    for _ in 0..10 {
        f.settle_with_plane(IntentBatch::default(), plane.clone());
    }
    let data = building_data(&f, solar);
    assert_eq!(data["store"]["energy"], 100, "产出至容量 100 后停止");
}

/// 受击、维修与击毁：Mod 建筑按配方 hitsMax 参与伤害/维修/摧毁链，
/// 击毁后资源转入废墟。
#[test]
fn mod_building_takes_damage_repairs_and_ruins_on_destroy() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let foundry = place_building(
        &mut f,
        "bm:foundry",
        20,
        20,
        serde_json::json!({"energy": 30, "bm:ingot": 5}),
    );
    // 预伤到 1000 hits。
    f.world
        .edit_object(foundry, |object| {
            if let Some(RoomObject::Generic(g)) = Some(object) {
                let mut v: Value = serde_json::from_str(&g.data).unwrap();
                v["hits"] = serde_json::json!(1000);
                g.data = v.to_string();
            }
            Ok(())
        })
        .unwrap();
    let attacker = f.creep("a", "p2", 21, 20, &[BodyKind::Attack], 0);
    let mut attack = IntentBatch::default();
    attack.add_object_intent("p2", attacker, Action::Attack, IntentData::target(foundry));
    f.settle_with_plane(attack, plane.clone());
    assert_eq!(building_data(&f, foundry)["hits"], 970, "ATTACK_POWER=30");

    // 维修：WORK×2 每次恢复 200（受能量限制 2）。
    let repairer = f.creep("r", "p1", 19, 20, &[BodyKind::Work, BodyKind::Work], 100);
    let mut repair = IntentBatch::default();
    repair.add_object_intent("p1", repairer, Action::Repair, IntentData::target(foundry));
    f.settle_with_plane(repair, plane.clone());
    let data = building_data(&f, foundry);
    assert_eq!(data["hits"], 1170, "970 + REPAIR_POWER×2=200");
    match f.obj(repairer) {
        RoomObject::Creep(c) => assert_eq!(c.store.energy, 98, "能耗 ceil(200×0.01)=2"),
        _ => panic!(),
    }

    // 大火力击毁 → 废墟承接库存。
    let killer = f.creep("k", "p1", 19, 20, &[BodyKind::RangedAttack; 20], 0);
    for _ in 0..20 {
        let mut kill = IntentBatch::default();
        kill.add_object_intent(
            "p1",
            killer,
            Action::RangedAttack,
            IntentData::target(foundry),
        );
        f.settle_with_plane(kill, plane.clone());
    }
    assert!(f.world.get(foundry).is_none(), "建筑被击毁");
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.structure_type == "bm:foundry" => Some(r.clone()),
            _ => None,
        })
        .expect("击毁产生废墟");
    assert_eq!(ruin.store.energy, 30);
    assert!(
        ruin.store_extra
            .iter()
            .any(|s| s.resource == "bm:ingot" && s.amount == 5)
    );
}

/// 拆解按成本构成比例回收：energy 为主成本按 amount/200，次要资源
/// 不足 1 单位的部分按整数下取整舍去（与内建口径一致）。
#[test]
fn dismantle_refunds_proportional_resources() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let foundry = place_building(&mut f, "bm:foundry", 20, 20, Value::Null);
    let worker = f.creep(
        "d",
        "p1",
        21,
        20,
        &[
            BodyKind::Carry,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
        ],
        0,
    );
    let mut dismantle = IntentBatch::default();
    dismantle.add_object_intent("p1", worker, Action::Dismantle, IntentData::target(foundry));
    f.settle_with_plane(dismantle, plane);
    // DISMANTLE_POWER×10 = 500：energy 回收 500·1000/(200·1000)=2；
    // ore 比例 500·100/(200·1000)=0.25 → 0（整数下取整）。
    match f.obj(worker) {
        RoomObject::Creep(c) => {
            assert_eq!(c.store.energy, 2);
            assert_eq!(
                kk_world::types::store_extra_slot(&c.store, &c.store_extra, "bm:ore"),
                0
            );
        }
        _ => panic!(),
    }
    assert_eq!(building_data(&f, foundry)["hits"], 1500);
}

/// 寻路/仲裁障碍：obstacle 建筑阻挡移动，road 语义建筑不阻挡。
#[test]
fn obstacle_building_blocks_movement_while_road_building_does_not() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    place_building(&mut f, "bm:foundry", 11, 10, Value::Null);
    place_building(&mut f, "bm:solar", 10, 12, Value::Null);
    let walker = f.creep("w", "p1", 10, 10, &[BodyKind::Move], 0);
    let move_right = move_intent(walker, 3);
    f.settle_with_plane(move_right, plane.clone());
    assert_eq!(f.creep_state(walker).0, 10, "obstacle 建筑阻挡");
    // 走向 road 语义建筑所在格：可通行。
    let mut d = IntentData::bare();
    d.direction = Some(5); // BOTTOM → 10,11
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", walker, Action::Move, d);
    f.settle_with_plane(batch, plane.clone());
    assert_eq!(f.creep_state(walker).1, 11);
    let mut d = IntentData::bare();
    d.direction = Some(5); // BOTTOM → 10,12（solar 格）
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", walker, Action::Move, d);
    f.settle_with_plane(batch, plane.clone());
    assert_eq!(f.creep_state(walker).1, 12, "road 语义建筑可通行");
    // plain 语义：可通行且不改变寻路成本。
    place_building(&mut f, "bm:pass", 10, 13, Value::Null);
    let mut d = IntentData::bare();
    d.direction = Some(5); // BOTTOM → 10,13（pass 格）
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", walker, Action::Move, d);
    f.settle_with_plane(batch, plane);
    assert_eq!(f.creep_state(walker).1, 13, "plain 语义建筑可通行");
}

/// 移除工地按各成本资源返还一半已付量（命名掉落堆）。
#[test]
fn site_removal_refunds_half_paid_resources() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let site = mod_site(&mut f, "bm:foundry", 20, 20, 500, 1000);
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().room.push(
        kk_engine::intent::RoomIntent::RemoveConstructionSite {
            room: "W1N1".into(),
            id: site,
        },
    );
    f.settle_with_plane(batch, plane);
    assert!(f.world.get(site).is_none(), "工地移除");
    let mut piles: Vec<(Option<String>, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) => Some((e.resource.clone(), e.amount)),
            _ => None,
        })
        .collect();
    piles.sort_by_key(|(r, _)| r.clone().unwrap_or_default());
    // 进度 500：energy 已付 500 退 250；ore 已付 50 退 25。
    // 掉落当 tick 衰减 ceil(amount/ENERGY_DECAY)=1。
    assert_eq!(
        piles,
        vec![(Some("bm:ore".into()), 24), (Some("energy".into()), 249)]
    );
}

/// 玩家 transfer 可向建筑仓库搬运声明资源，容量与库存结算一致。
#[test]
fn haulers_transfer_declared_resources_into_building_store() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let foundry = place_building(&mut f, "bm:foundry", 10, 11, Value::Null);
    let hauler = f.creep_with_mineral("h", 10, 10, &[BodyKind::Carry], 400, "bm:ore", 100);
    let mut data = IntentData::target(foundry);
    data.resource_type = Some("bm:ore".into());
    data.amount = Some(80);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", hauler, Action::Transfer, data);
    f.settle_with_plane(batch, plane);
    assert_eq!(building_data(&f, foundry)["store"]["bm:ore"], 80);
    match f.obj(hauler) {
        RoomObject::Creep(c) => assert_eq!(
            kk_world::types::store_extra_slot(&c.store, &c.store_extra, "bm:ore"),
            20
        ),
        _ => panic!(),
    }
}

/// 房主可主动销毁 Mod 建筑（destroyStructure 门户对目录类型开放）。
#[test]
fn owner_can_destroy_mod_building_via_room_intent() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    f.controller(3, Some("p1"));
    let foundry = place_building(&mut f, "bm:foundry", 20, 20, Value::Null);
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().room.push(
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: foundry,
        },
    );
    f.settle_with_plane(batch, plane);
    assert!(f.world.get(foundry).is_none(), "建筑被销毁");
}

/// boost 分歧的锁定：XLH2O（build×2）的 creep 建 Mod 工地时，进度与
/// 支付同比例放大（2 WORK×5×2=20 点，付 20 能量+2 矿）——与内建
/// 「能耗按未强化效果」不同，是文档化的有意分歧。
#[test]
fn boosted_builder_pays_one_to_one_with_boosted_progress() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let builder = f.creep_with_mineral(
        "b",
        20,
        19,
        &[BodyKind::Work, BodyKind::Work],
        1000,
        "bm:ore",
        100,
    );
    f.world
        .edit_object(builder, |object| {
            if let Some(RoomObject::Creep(c)) = Some(object) {
                for part in &mut c.body {
                    if part.kind == kk_world::types::BodyKind::Work.code() {
                        part.boost = Some("XLH2O".into());
                    }
                }
            }
            Ok(())
        })
        .unwrap();
    let site = mod_site(&mut f, "bm:foundry", 20, 20, 0, 1000);
    f.settle_with_plane(build_intent(builder, site), plane);
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => assert_eq!(s.progress, 20, "boost 后进度 20 点"),
        other => panic!("工地仍在：{other:?}"),
    }
    match f.obj(builder) {
        RoomObject::Creep(c) => {
            assert_eq!(c.store.energy, 980, "支付与进度 1:1（内建此时只付 10）");
            assert_eq!(
                kk_world::types::store_extra_slot(&c.store, &c.store_extra, "bm:ore"),
                98
            );
        }
        _ => panic!(),
    }
}

/// 宿主放置的超限工地：build 把进度回拨到 total 完成建造，并按比例
/// 返还超付资源（与内建有符号回拨约定同形）。
#[test]
fn over_progress_site_builds_back_and_refunds_overpayment() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = building_plane();
    let builder = f.creep_with_mineral("b", 20, 19, &[BodyKind::Work; 2], 1000, "bm:ore", 100);
    let site = mod_site(&mut f, "bm:foundry", 20, 20, 1020, 1000);
    f.settle_with_plane(build_intent(builder, site), plane);
    assert!(f.world.get(site).is_none(), "回拨到 total 即完工");
    let building = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Generic(g) => {
                let data: Value = serde_json::from_str(&g.data).unwrap();
                (data["structureType"] == "bm:foundry").then_some(data)
            }
            _ => None,
        })
        .expect("完工落成");
    assert_eq!(building["hits"], 2000);
    match f.obj(builder) {
        RoomObject::Creep(c) => {
            // 超付 20 点：energy 返 20、ore 返 floor(100·20/1000)=2。
            assert_eq!(c.store.energy, 1020, "超付部分返还");
            assert_eq!(
                kk_world::types::store_extra_slot(&c.store, &c.store_extra, "bm:ore"),
                102
            );
        }
        _ => panic!(),
    }
}
