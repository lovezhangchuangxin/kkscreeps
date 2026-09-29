//! B1 结构治理批自 embed_tests.rs 连续切块（串行链 impl；顶层唯一 #[test] 不变量
//! 在 crate 根——本文件全部 fn 由根链按原调用序调用，不得加 #[test]）。

use super::*;

/// M2A-A2 intent 通道端到端（tick 契约场景 1）：真实沙箱的 move/harvest 经
/// 清洗、结算，下一轮玩家视图可见；假沙箱（ScriptedSandbox）与真沙箱走同一条
/// 结算入口（Engine::tick 与 tick_with_intents 汇合于 settle_room），相同
/// intent 序列产出相同世界转移。
pub(super) fn embed_intent_channel_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    let creep_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move", "work", "carry"], "name": "w1" }),
    )?;
    let source_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [24, 25], "structureType": "source" }),
    )?;
    let src_json = serde_json::to_string(&source_id)?;

    // 真沙箱 move(BOTTOM)：预检过返回 OK，下一 tick 视图位置 (25,26)
    let run = e.run_player("p1", "Game.creeps['w1'].move(5)")?;
    assert_eq!(run.expr["value"], 0, "预检通过返回 OK");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["pos"]["x"], 25, "x 不变");
    assert_eq!(snap["pos"]["y"], 26, "BOTTOM 落格 (25,26)");

    // 真沙箱 harvest（相邻 source）：WORK×HARVEST_POWER(2) 进 store、source 扣减
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].harvest(Game.getObjectById({src_json}))"),
    )?;
    assert_eq!(run.expr["value"], 0);
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 2, "1×WORK × 2");
    let src_snap = e.get_object(&source_id).unwrap();
    assert_eq!(src_snap["energy"], 2998, "3000 − 2");
    e.teardown();

    // ---- 同一入口等价：ScriptedSandbox 相同 intent 序列 → 相同世界转移 ----
    let mut world = kk_world::World::new(1);
    let mut terrain = kk_core::Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Controller(kk_schema::room::ControllerObj {
                id: 0,
                x: 1,
                y: 1,
                level: 3,
                safe_mode: 0,
                safe_mode_available: 1,
                progress: 0,
                downgrade_time: None,
                owner: Some("p1".into()),
                owner_cleared: false,
                is_power_enabled: false,
                reservation: None,
                sign: None,
                upgrade_blocked: None,
                safe_mode_cooldown: None,
            }),
        )
        .unwrap();
    let part = |k: kk_world::types::BodyKind| kk_schema::room::BodyPart {
        kind: k.code(),
        hits: 100,
        boost: None,
    };
    let fake_creep = world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Creep(kk_schema::room::CreepObj {
                id: 0,
                x: 25,
                y: 25,
                fatigue: 0,
                age_time: 1500,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![
                    part(kk_world::types::BodyKind::Move),
                    part(kk_world::types::BodyKind::Work),
                    part(kk_world::types::BodyKind::Carry),
                ],
                store: Default::default(),
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let fake_source = world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 24,
                y: 25,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    let mut sandbox = kk_engine::ScriptedSandbox::new();
    let mut move_batch = kk_engine::intent::IntentBatch::default();
    move_batch.add_object_intent(
        "p1",
        fake_creep,
        kk_engine::intent::Action::Move,
        kk_engine::intent::IntentData {
            direction: Some(5),
            ..kk_engine::intent::IntentData::bare()
        },
    );
    let mut harvest_batch = kk_engine::intent::IntentBatch::default();
    harvest_batch.add_object_intent(
        "p1",
        fake_creep,
        kk_engine::intent::Action::Harvest,
        kk_engine::intent::IntentData::target(fake_source),
    );
    sandbox.at(0, move_batch);
    sandbox.at(1, harvest_batch);
    let mut engine = kk_engine::Engine::new(world);
    engine.sandboxes.insert("p1".into(), Box::new(sandbox));
    engine.tick().map_err(|err| anyhow::anyhow!("{err}"))?;
    engine.tick().map_err(|err| anyhow::anyhow!("{err}"))?;
    let (x, y, energy, src_energy) = match engine.world.get(fake_creep).unwrap().1 {
        kk_schema::room::RoomObject::Creep(c) => match engine.world.get(fake_source).unwrap().1 {
            kk_schema::room::RoomObject::Source(s) => {
                (c.x, c.y, c.store.energy as i64, s.energy as i64)
            }
            _ => panic!(),
        },
        _ => panic!(),
    };
    assert_eq!(
        (x, y, energy, src_energy),
        (25, 26, 2, 2998),
        "假沙箱与真沙箱同一结算入口：世界转移一致"
    );
    Ok(())
}

/// 外部复审 P1-2/P1-3 修复的回归锁：per-viewer 视野裁切（不可见房/对象/集合）
/// 与工地总量帽（伪造批量 room intent 也被 100 帽挡住）。
pub(super) fn embed_per_viewer_and_caps_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 3, "owner": "p1" },
            { "name": "W2N2", "rcl": 3, "owner": "p2" },
        ],
    }))?;
    let mine = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "mine" }),
    )?;
    let secret = e.place_object(
        "W2N2",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p2", "body": ["move"], "name": "secret" }),
    )?;
    let secret_json = serde_json::to_string(&secret)?;

    // P1-3：p1 的视图——看不见 W2N2、看不见 secret、Game.creeps 只收自家
    let run = e.run_player(
        "p1",
        &format!(
            "[Object.keys(Game.rooms).sort().join(','), Object.keys(Game.creeps).join(','), String(Game.getObjectById({secret_json}) == null)]"
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], "W1N1", "敌方房间不进 Game.rooms: {v:?}");
    assert_eq!(v[1], "mine", "Game.creeps 只收本人: {v:?}");
    // getObjectById 对不可见对象返回 null（官方同构；== null 同时覆盖 undefined）
    assert_eq!(v[2], "true", "敌方对象 getObjectById 得 null: {v:?}");

    // p2 的视图对偶
    let run = e.run_player("p2", "Object.keys(Game.rooms).join(',')")?;
    assert_eq!(run.expr["value"], "W2N2", "p2 只见自家房");

    // 我方对象的 my 判定
    let mine_json = serde_json::to_string(&mine)?;
    let run = e.run_player("p1", &format!("Game.getObjectById({mine_json}).my"))?;
    assert_eq!(run.expr["value"], true);

    // P1-2：批量 room intent（engine 直驱，等价伪造面）被总量帽挡住——
    // 120 条 createConstructionSite 只落 100
    let mut room_intents = Vec::new();
    for i in 0..120 {
        room_intents.push(json!({
            "roomName": "W1N1", "x": (i % 40) + 5, "y": (i / 40) * 10 + 5,
            "structureType": "road"
        }));
    }
    let list = json!({ "room": { "createConstructionSite": room_intents } });
    let batch = kk_engine::intent::IntentBatch::from_json("p1", &list);
    let sites_before = e.find_in_room("W1N1", 111).len();
    e.settle_external(batch)?;
    let sites_after = e.find_in_room("W1N1", 111).len();
    assert_eq!(
        sites_after - sites_before,
        100,
        "MAX_CONSTRUCTION_SITES 总量帽：120 条只落 100"
    );

    // 自家工地全集（官方 data.js:103 按 user 全量拉 + game.js:366-374）：工地
    // 不授房视野（data.js:113-116），但 Game.constructionSites 收全部自家工地
    // 含不可见房——两个语义正交（对抗评审 P2 的回归锁：world 破例导出 +
    // buildGame visibleRooms 排除工地）
    let own_site_in_hidden_room = e.place_object(
        "W2N2",
        "constructionSite",
        &json!({ "pos": [15, 15], "owner": "p1", "structureType": "road" }),
    )?;
    let site_json = serde_json::to_string(&own_site_in_hidden_room)?;
    let run = e.run_player(
        "p1",
        &format!(
            "[Object.keys(Game.rooms).sort().join(','), \
             Object.keys(Game.constructionSites).join(','), \
             String(Game.getObjectById({site_json}) !== null), \
             String(Game.rooms['W2N2'])]"
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], "W1N1", "工地不授房视野：W2N2 不进 Game.rooms: {v:?}");
    let sites_keys = v[1].as_str().unwrap_or_default();
    assert!(
        sites_keys
            .split(',')
            .any(|k| k == site_json.trim_matches('"')),
        "自家工地全集进 Game.constructionSites（含不可见房，此前帽测试的 100 工地 + 新 1）: {v:?}"
    );
    assert_eq!(v[2], "true", "不可见房自家工地 getObjectById 可达: {v:?}");
    assert_eq!(
        v[3], "undefined",
        "工地所在不可见房 Game.rooms 条目 undefined: {v:?}"
    );
    Ok(())
}

/// A4-ii 资源族四方法端到端（prelude 换真 ↔ 执行器已有结算的接线）：
/// transfer/withdraw/drop/pickup 的能量流经真实投影（含 extension 容量按
/// rcl 折算的 prelude/执行器双源一致）。
pub(super) fn embed_resource_family_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    let creep_id = e.place_object(
        "W1N1",
        "creep",
        &json!({
            "pos": [25, 25], "owner": "p1", "body": ["carry", "carry", "move"], "name": "w1",
            "store": { "energy": 80 },
        }),
    )?;
    let ext_id = e.place_object(
        "W1N1",
        "structure",
        &json!({
            "pos": [26, 24], "structureType": "extension", "owner": "p1",
            "store": { "energy": 0 },
        }),
    )?;
    let ext_json = serde_json::to_string(&ext_id)?;

    // transfer：缺省钳到 rcl3 extension 容量 50（prelude 折算）＝ 执行器
    // extension_cap(rcl) 钳制（settle/creep.rs）——双源一致的能量转移
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].transfer(Game.getObjectById({ext_json}), RESOURCE_ENERGY)"),
    )?;
    assert_eq!(run.expr["value"], 0, "transfer 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 30, "80 − 50（缺省钳到目标容量）");
    let ext = e.get_object(&ext_id).unwrap();
    assert_eq!(ext["store"]["energy"], 50, "extension 0 → 50");

    // withdraw：缺省 min(空容量 70, 存量 50) = 50 全量取回
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].withdraw(Game.getObjectById({ext_json}), RESOURCE_ENERGY)"),
    )?;
    assert_eq!(run.expr["value"], 0, "withdraw 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 80, "30 + 50");
    let ext = e.get_object(&ext_id).unwrap();
    // snapshot 契约：空 store 折叠为 {}（snap_store），0 值键缺省
    assert_eq!(
        ext["store"]["energy"].as_i64().unwrap_or(0),
        0,
        "extension 50 → 0"
    );

    // drop 30：同格生成 resource 堆
    let run = e.run_player("p1", "Game.creeps['w1'].drop(RESOURCE_ENERGY, 30)")?;
    assert_eq!(run.expr["value"], 0, "drop 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 50, "80 − 30");
    let dropped = e.find_in_room("W1N1", 106);
    assert_eq!(dropped.len(), 1, "drop 落地一个 resource");
    // 掉落当 tick 即经 energy 衰减（objects/tick：ceil(30/1000)=1）→ 29
    assert_eq!(dropped[0]["amount"], 29, "掉落 30 − 当 tick 衰减 1");
    assert_eq!(dropped[0]["pos"]["x"], 25, "掉落位置 = creep 格");
    let pile_id = dropped[0]["id"].as_str().unwrap().to_string();

    // pickup：整堆拾取（余量 50 ≥ 30）
    let pile_json = serde_json::to_string(&pile_id)?;
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].pickup(Game.getObjectById({pile_json}))"),
    )?;
    assert_eq!(run.expr["value"], 0, "pickup 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 79, "50 + 29（衰减后余量）");
    assert!(e.get_object(&pile_id).is_none(), "堆清零移除");
    assert!(
        e.find_in_room("W1N1", 106).is_empty(),
        "房间无残留 resource"
    );
    e.teardown();
    Ok(())
}

/// A4-iv 移动杂项族闭环（prelude API → intentsList 清洗 → settle）：
/// pull+move(puller) 配对位移（被拉者无 MOVE 照样动、疲劳归链拉者）；
/// suicide 当 tick 死亡并掉落；say 免费且不影响位移。
pub(super) fn embed_move_misc_family_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    // 拉者 [move,work]@(10,10)：被拉者 [work]@(11,10)（无 MOVE、无 fatigue）
    let _puller = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["move", "work"], "name": "puller" }),
    )?;
    let _pulled = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [11, 10], "owner": "p1", "body": ["work"], "name": "pulled" }),
    )?;
    // 自杀者携带 40 energy
    let _martyr = e.place_object(
        "W1N1",
        "creep",
        &json!({
            "pos": [20, 20], "owner": "p1", "body": ["move", "carry"],
            "name": "martyr", "store": { "energy": 40 }
        }),
    )?;

    let run = e.run_player(
        "p1",
        r#"
        Game.creeps['puller'].pull(Game.creeps['pulled']);
        Game.creeps['pulled'].move(Game.creeps['puller']);
        Game.creeps['puller'].move(LEFT);
        Game.creeps['martyr'].suicide();
        Game.creeps['martyr'].say('bye', true);
        Game.creeps['puller'].say('hi');
    "#,
    )?;
    assert!(run.executed, "玩家代码应真实执行");

    // 拉者 (10,10)→(9,10)；被拉者 (11,10)→(10,10)（补拉者让位格——无 MOVE
    // 经 pull 配对移动，官方 canMove 的 _pulled 分支）
    let by_name = |e: &EmbedEngine, name: &str| {
        e.find_in_room("W1N1", 101)
            .into_iter()
            .find(|o| o["name"] == name)
            .unwrap_or_else(|| panic!("{name} 应存活"))
            .clone()
    };
    let puller = by_name(&e, "puller");
    assert_eq!(
        (puller["pos"]["x"].clone(), puller["pos"]["y"].clone()),
        (json!(9), json!(10))
    );
    let pulled = by_name(&e, "pulled");
    assert_eq!(
        (pulled["pos"]["x"].clone(), pulled["pos"]["y"].clone()),
        (json!(10), json!(10))
    );
    // 疲劳归链：拉者自身 1 work×2 + 被拉者 1 work×2 归链 = 4，1 MOVE 恢复 -2
    assert_eq!(puller["fatigue"], json!(2), "被拉者的移动疲劳记在拉者头上");
    assert_eq!(pulled["fatigue"], json!(0), "被拉者无疲劳");

    // suicide：当 tick消亡——B3 墓碑承接（官方 _die.js）：携带 40 + 造价回收
    // min(125,50×0.2)×2 = 20（dropRate 0.2、TTL 满寿 lifeRate=0.2）⇒ store 60；
    // 掉落发生在墓碑衰减到点（2 部件 ⇒ 10 tick）
    assert!(
        e.find_in_room("W1N1", 101)
            .into_iter()
            .all(|o| o["name"] != "martyr"),
        "suicide 当 tick 死亡"
    );
    let martyrs_tomb: Vec<_> = e
        .find_in_room("W1N1", 118)
        .into_iter()
        .filter(|o| o["pos"]["x"] == 20 && o["pos"]["y"] == 20)
        .collect();
    assert_eq!(martyrs_tomb.len(), 1, "suicide 产墓碑");
    assert_eq!(
        martyrs_tomb[0]["store"]["energy"],
        json!(60),
        "携带 40 + 造价回收 20"
    );
    assert!(e.find_in_room("W1N1", 106).is_empty(), "不直接掉地");
    e.teardown();
    Ok(())
}

/// B4 合入冒烟锁值：embed place mineral/extractor 直通道（裸 kind——原落
/// Generic 透传致 FIND 失配，B4 评审 3 P3-1 登记项清偿）与玩家面
/// FIND_MINERALS/FIND_STRUCTURES 命中。并入主套件（embed 单测试不变量——
/// 独立 #[test] 会留 LIFO 违规，A5-lite P2-I 同款教训）。
pub(super) fn embed_mineral_direct_channel_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 6, "owner": "p1" }],
    }))
    .unwrap();
    let id = e
        .place_object(
            "W1N1",
            "mineral",
            &serde_json::json!({ "pos": [10, 10], "mineralType": "U", "density": 3 }),
        )
        .unwrap();
    let snap = e.get_object(&id).expect("getObject");
    assert_eq!(snap["kind"], "mineral");
    assert_eq!(snap["mineralType"], "U");
    assert_eq!(
        e.find_in_room("W1N1", 116).len(),
        1,
        "FIND_MINERALS 契约面命中"
    );
    let ex = e
        .place_object(
            "W1N1",
            "extractor",
            &serde_json::json!({ "pos": [10, 10], "owner": "p1" }),
        )
        .unwrap();
    assert_eq!(e.get_object(&ex).unwrap()["structureType"], "extractor");
    let structures: Vec<String> = e
        .find_in_room("W1N1", 107)
        .iter()
        .filter_map(|v| v["structureType"].as_str().map(str::to_string))
        .collect();
    assert_eq!(
        structures,
        vec!["controller".to_string(), "extractor".to_string()],
        "extractor 进 FIND_STRUCTURES（mineral 不进）"
    );
    Ok(())
}

/// GPT 外部复审（74c16d5..d5de6ad）五条 P1 的清偿行为探针：P1-1 可见房/
/// P1-2 tombstone·ruin 提取/P1-3 冷却预检/P1-5 raw 事件日志不可污染
/// （P1-4 destroy 无事件由 settle 测试 destroy_structure_intent_paths 锁定）。
pub(super) fn embed_gpt_review_clearance_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1", "controller": true }],
    }))?;
    // P1-1：无 controller 房只放己方墓碑——官方 data.js:103-116 按用户对象
    // 归集（只排除 flag/工地），房必须在 Game.rooms
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W1N1", "rcl": 3, "owner": "p1" },
            { "name": "W2N2", "controller": false },
        ],
    }))?;
    let tomb = e.place_object(
        "W2N2",
        "tombstone",
        &serde_json::json!({
            "pos": [10, 10], "store": { "energy": 40 }, "ticksToDecay": 50,
            "creepName": "dead", "creepBody": ["carry"], "user": "p1",
        }),
    )?;
    let run = e.run_player(
        "p1",
        "Game.rooms['W2N2'] ? Game.rooms['W2N2'].name : 'MISSING'",
    )?;
    assert_eq!(
        run.expr["value"], "W2N2",
        "P1-1：仅剩己方墓碑的房必须在 Game.rooms"
    );

    // P1-2：withdraw 从墓碑提取（官方 creeps.js:507-515 接受 Tombstone 目标）
    let c = e.place_object(
        "W2N2",
        "creep",
        &serde_json::json!({
            "pos": [10, 11], "owner": "p1", "body": ["carry"], "name": "w1",
            "store": { "energy": 0 },
        }),
    )?;
    let run = e.run_player(
        "p1",
        &format!(
            "Game.getObjectById({:?}).withdraw(Game.getObjectById({:?}), RESOURCE_ENERGY, 10)",
            c, tomb
        ),
    )?;
    assert_eq!(run.expr["value"], 0, "P1-2：withdraw 墓碑返回 OK");
    let run = e.run_player("p1", &format!("Game.getObjectById({:?}).store.energy", c))?;
    assert_eq!(run.expr["value"], 10, "P1-2：提取 10 落袋（原静默丢弃）");
    let snap = e.get_object(&tomb).unwrap();
    assert_eq!(snap["store"]["energy"], 30, "P1-2：墓碑扣 10");

    // P1-3：extractor cooldown 预检（官方 structures.js:268 getter + creeps.js:383
    // 非 0 冷却 → ERR_TIRED=-6）
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [{ "name": "W3N3", "rcl": 6, "owner": "p1" }],
    }))?;
    let m = e.place_object(
        "W3N3",
        "mineral",
        &serde_json::json!({
            "pos": [10, 10], "mineralType": "U", "density": 3,
        }),
    )?;
    e.place_object(
        "W3N3",
        "extractor",
        &serde_json::json!({
            "pos": [10, 10], "owner": "p1", "cooldown": 5,
        }),
    )?;
    let c3 = e.place_object(
        "W3N3",
        "creep",
        &serde_json::json!({
            "pos": [10, 11], "owner": "p1", "body": ["work", "carry"], "name": "m1",
        }),
    )?;
    let run = e.run_player(
        "p1",
        &format!(
            "Game.getObjectById({:?}).harvest(Game.getObjectById({:?}))",
            c3, m
        ),
    )?;
    assert_eq!(
        run.expr["value"], -11,
        "P1-3：冷却期 harvest → ERR_TIRED=-11（原假 OK）"
    );
    let snap = e.get_object(&m).unwrap();
    assert_eq!(snap["mineralAmount"], 70_000, "P1-3：矿量不动");

    // P1-5：修改非 raw 解析数组不得污染 raw 字符串（官方 rooms.js:573-583
    // raw 返回持久化原始串）
    let run = e.run_player(
        "p1",
        "(()=>{ const log = Game.rooms['W3N3'].getEventLog(); \
         if (log.length > 0) { log[0].data = { hacked: true }; } \
         return Game.rooms['W3N3'].getEventLog(true); })()",
    )?;
    let raw = run.expr["value"].as_str().unwrap_or("");
    assert!(
        !raw.contains("hacked"),
        "P1-5：raw 不得被解析数组改写污染: {raw}"
    );
    Ok(())
}

/// 紧凑地形协议端到端（M2B 前置批 P3 评审清偿）：真路径（投影 b64 载荷 →
/// prelude beginTick 解码 → terrainByRoom）的地形值断言——producer 形态
/// （world_json 测试）与 consumer 夹具探针（sandbox_tests）之外的全链闭环。
pub(super) fn terrain_compact_protocol_e2e_impl() -> anyhow::Result<()> {
    let mut terrain_codes = vec![0u32; 2500];
    terrain_codes[5 * 50 + 5] = 2; // (5,5) swamp（normalize 只强制四角墙）
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1", "terrain": terrain_codes }],
    }))?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["move"], "name": "anchor" }),
    )?;
    let run = e.run_player("p1", "Game.map.getTerrainAt(5,5,'W1N1')")?;
    assert_eq!(run.expr["value"], "swamp", "紧凑载荷端到端解码值");
    let run = e.run_player("p1", "Game.map.getRoomTerrain('W1N1').get(5,5)")?;
    assert_eq!(run.expr["value"], 2, "Terrain.get 位码");
    let run = e.run_player(
        "p1",
        "Game.map.getRoomTerrain('W1N1').getRawBuffer().length",
    )?;
    assert_eq!(run.expr["value"], 2500);
    let run = e.run_player("p1", "Game.map.getTerrainAt(4,5,'W1N1')")?;
    assert_eq!(run.expr["value"], "plain", "非沼泽格 plain");
    e.teardown();
    Ok(())
}
