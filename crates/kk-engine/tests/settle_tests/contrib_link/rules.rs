//! 有效规则在放置、资源搬运、建造和配额中的一致性。
use super::*;

/// 规则覆盖贯穿放置默认值：容量、hits 与工地总进度读取同一有效规则，
/// 各消费者不单独读 kk-core 常量——否则覆盖无法到达该入口。
#[test]
fn rule_overrides_flow_into_placement_defaults() {
    let plan = frozen_plan(&[
        ("link.capacity", kk_mod_api::Number::Signed(400)),
        ("link.hits", kk_mod_api::Number::Signed(500)),
        ("link.hitsMax", kk_mod_api::Number::Signed(600)),
        ("link.buildCost", kk_mod_api::Number::Signed(120)),
    ]);
    let links = rules_of(&plan);

    let obj = kk_world::place::normalize(
        "structure",
        &json!({"structureType": "link", "pos": [10, 11], "owner": "p1"}),
        &place_ctx(links),
    )
    .unwrap();
    let RoomObject::Generic(g) = obj else {
        panic!("link 放置为 Generic");
    };
    let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
    assert_eq!(data["storeCapacity"], json!(400), "平面容量应取覆盖值");
    assert_eq!(
        data["storeCapacityResource"]["energy"],
        json!(400),
        "按资源容量应取覆盖值"
    );
    assert_eq!(data["hits"], json!(500));
    assert_eq!(data["hitsMax"], json!(600));
    // 显式放置字段不被默认值覆盖。
    let explicit = kk_world::place::normalize(
        "structure",
        &json!({"structureType": "link", "pos": [12, 11], "owner": "p1",
                "storeCapacity": 111, "storeCapacityResource": {"energy": 222}}),
        &place_ctx(links),
    )
    .unwrap();
    let RoomObject::Generic(eg) = explicit else {
        panic!();
    };
    let ed: serde_json::Value = serde_json::from_str(&eg.data).unwrap();
    assert_eq!(ed["storeCapacity"], json!(111));
    assert_eq!(ed["storeCapacityResource"]["energy"], json!(222));

    // 工地总进度来自同一 buildCost 规则。
    let site = kk_world::place::normalize(
        "constructionSite",
        &json!({"structureType": "link", "pos": [13, 11]}),
        &place_ctx(links),
    )
    .unwrap();
    let RoomObject::ConstructionSite(s) = site else {
        panic!();
    };
    assert_eq!(s.progress_total, 120);
}

/// 规则覆盖贯穿结算：转移损耗、冷却系数与目标容量钳都读有效规则。
#[test]
fn rule_overrides_flow_into_transfer_settlement() {
    let plan = frozen_plan(&[
        ("link.capacity", kk_mod_api::Number::Signed(400)),
        ("link.lossRatio", kk_mod_api::Number::Float(0.5)),
        ("link.cooldown", kk_mod_api::Number::Signed(3)),
    ]);

    let mut f = Fixture::new(7, terrain_with(&[]));
    // CONTROLLER_STRUCTURES 中 link 的 rcl0..4 配额为 0、rcl5 起
    // 非零；控制器用 RCL5 使配额门放行转移。
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 400,
            "storeCapacityResource": {"energy": 400},
        }),
    );
    let l2 = place_generic(
        &mut f,
        13,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 400,
            "storeCapacityResource": {"energy": 400},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, &plan);

    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    // 损耗 ceil(200*0.5)=100：源扣 200，目标进 100。
    assert_eq!(d1["store"]["energy"], json!(100));
    assert_eq!(d2["store"]["energy"], json!(100));
    // 冷却 = 3 × 切比雪夫距离 3 = 9；同轮对象 tick 再减一——转移在
    // 对象动作阶段执行、递减在对象 tick 阶段执行（官方 links/tick.js
    // 的逐 tick 递减序）。
    assert_eq!(d1["cooldown"], json!(8));

    // 目标余量钳走权威字段 storeCapacityResource（放置时已按覆盖规则落值）；
    // 此处验证字段链路，规则到字段的贯穿由放置测试锁定。
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l2,
        Action::Transfer,
        IntentData {
            id: Some(l1),
            amount: Some(100),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    // 清两轮冷却并把 l1 预置到余量 50 的位置。
    for id in [l1, l2] {
        f.world
            .edit_object(id, |object| {
                if let Some(RoomObject::Generic(g)) = Some(object) {
                    let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                    v["cooldown"] = json!(0);
                    if id == l1 {
                        v["store"]["energy"] = json!(350);
                    }
                    g.data = v.to_string().into();
                }
                Ok(())
            })
            .unwrap();
    }
    f.settle_with_rules(batch, &plan);
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    assert_eq!(d1["store"]["energy"], json!(375), "只进钳后 50−损耗 25");
    assert_eq!(d2["store"]["energy"], json!(50));
}

/// RCL 配额覆盖生效：配额为 0 时转移被控制器检查拒绝，能量不动。
#[test]
fn rule_override_rcl_quota_gates_transfer() {
    // 覆盖 rcl5 为 0。CONTROLLER_STRUCTURES 的 link rcl5=2（官方
    // 常量表，kk-core 生成物），与覆盖值形成分歧窗口；若配额查表
    // 而不读规则，转移会错误通过。
    let plan = frozen_plan(&[
        ("structure.quota.link.rcl5", kk_mod_api::Number::Signed(0)),
        ("structure.quota.link.rcl8", kk_mod_api::Number::Signed(0)),
    ]);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let l2 = place_generic(
        &mut f,
        12,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(100),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, &plan);
    let d1 = data_of(&f, l1);
    assert_eq!(d1["store"]["energy"], json!(300), "配额为 0 时转移被拒");
}

/// 覆盖规则下的自转移：只扣 ceil(amount*loss_ratio)，冷却按距离 0 不增。
#[test]
fn rule_overrides_flow_into_self_transfer() {
    let plan = frozen_plan(&[
        ("link.capacity", kk_mod_api::Number::Signed(400)),
        ("link.lossRatio", kk_mod_api::Number::Float(0.5)),
        ("link.cooldown", kk_mod_api::Number::Signed(3)),
    ]);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 400,
            "storeCapacityResource": {"energy": 400},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l1),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, &plan);
    let d1 = data_of(&f, l1);
    // 自转移先过自身余量钳（300+200>400 → amount=100），再扣损耗
    // ceil(100*0.5)=50：300−50=250；距离 0，冷却增加 3×0=0。
    assert_eq!(d1["store"]["energy"], json!(250));
    assert_eq!(d1["cooldown"], json!(0));
}

/// 玩家工地的 RCL 配额门与转移门同源：覆盖 rcl5=0 后工地创建被拒。
#[test]
fn rule_override_rcl_quota_gates_player_site_creation() {
    // CONTROLLER_STRUCTURES 的 link rcl5=2（官方常量表，kk-core
    // 生成物）；覆盖为 0 形成分歧窗口——若配额查表而不读规则，工地照建。
    let plan = frozen_plan(&[
        ("structure.quota.link.rcl5", kk_mod_api::Number::Signed(0)),
        ("structure.quota.link.rcl8", kk_mod_api::Number::Signed(0)),
    ]);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let mut batch = IntentBatch::default();
    batch.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateConstructionSite {
            room: "W1N1".into(),
            x: 20,
            y: 20,
            structure_type: "link".into(),
            name: None,
        },
    );
    f.settle_with_rules(batch, &plan);
    let sites = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter(|o| matches!(o, RoomObject::ConstructionSite(s) if s.structure_type == "link"))
        .count();
    assert_eq!(sites, 0, "配额覆盖为 0 时玩家 link 工地被拒");
}

/// 玩家 createConstructionSite 的 link 工地总进度跟随 buildCost 覆盖，
/// 与放置夹具路径（normalize_site）读同一规则。
#[test]
fn rule_override_build_cost_flows_into_player_site() {
    let plan = frozen_plan(&[("link.buildCost", kk_mod_api::Number::Signed(120))]);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let mut batch = IntentBatch::default();
    batch.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateConstructionSite {
            room: "W1N1".into(),
            x: 20,
            y: 20,
            structure_type: "link".into(),
            name: None,
        },
    );
    f.settle_with_rules(batch, &plan);
    let site = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::ConstructionSite(s) if s.structure_type == "link" => Some(s.progress_total),
            _ => None,
        })
        .expect("玩家 link 工地已创建");
    assert_eq!(
        site, 120,
        "工地总进度取覆盖值而非 CONSTRUCTION_COST 的 5000"
    );
}

/// hitsMax 规则覆盖钳住修复：tower 修理不越过覆盖上限。
#[test]
fn rule_override_hits_max_caps_tower_repair() {
    let plan = frozen_plan(&[
        ("link.hitsMax", kk_mod_api::Number::Signed(600)),
        ("link.hits", kk_mod_api::Number::Signed(100)),
    ]);

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使 CONTROLLER_STRUCTURES 的 link 配额非零：native 转移可
    // 真实执行，截断/替换类断言才有判别力。
    f.controller(5, Some("p1"));
    let tower = tower(&mut f, 5, 5, 1000);
    let link = place_generic(
        &mut f,
        8,
        8,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 500, "hitsMax": 600, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        tower,
        Action::Repair,
        IntentData {
            id: Some(link),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, &plan);
    let d = data_of(&f, link);
    assert_eq!(d["hits"], json!(600), "修复钳到覆盖的 hitsMax=600");
}
