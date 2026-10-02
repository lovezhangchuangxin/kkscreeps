//! 验证玩家意图从沙箱传到结算，再出现在下一轮玩家视图中的完整过程。
//! 场景由 `embed_engine_suite` 顺序调用。

use super::*;

/// KKPV 二进制通道与 JSON 参考通道在真实分片上的端到端等价：同一分片
/// 设置与玩家脚本，两条通道各自的可见快照、意图返回码与结算后的世界
/// 状态完全一致。权威桥的 Ref 编码（墓碑/废墟）无法经 place 通道构造，
/// 由 kk-runner 沙箱级测试用手工块锁定。
pub(super) fn kkpv_channel_e2e_equivalence_impl() -> anyhow::Result<()> {
    let dump_code = r#"
(function () {
  function snap(v, d) {
    if (v === null || v === undefined) return null;
    var t = typeof v;
    if (t === 'function') return undefined;
    if (t !== 'object') return v;
    if (d > 6) return '[deep]';
    if (Array.isArray(v)) {
      var a = [];
      for (var i = 0; i < v.length; i++) a.push(snap(v[i], d + 1));
      return a;
    }
    var o = {};
    var keys = Object.keys(v).sort();
    for (var i = 0; i < keys.length; i++) {
      var k = keys[i];
      if (k === 'room') continue;
      if (k === 'pos') {
        o.pos = { x: v.pos.x, y: v.pos.y, roomName: v.pos.roomName };
        continue;
      }
      var s = snap(v[k], d + 1);
      if (s !== undefined) o[k] = s;
    }
    return o;
  }
  var r = Game.rooms['W1N1'];
  return JSON.stringify({
    time: Game.time, gcl: snap(Game.gcl), cpu: snap(Game.cpu),
    creeps: snap(Game.creeps), spawns: snap(Game.spawns),
    structures: snap(Game.structures), rooms: snap(Game.rooms),
    market: snap(Game.market),
    creepProbe: (function () {
      var c = Game.creeps['w1'];
      var out = {};
      var keys = ['name', 'owner', 'my', 'store', 'body', 'fatigue', 'hits',
        'spawning', 'ticksToLive', 'carry', 'carryCapacity'];
      for (var i = 0; i < keys.length; i++) {
        try { out[keys[i]] = snap(c[keys[i]], 1); } catch (e) { out[keys[i]] = 'THROW'; }
      }
      return out;
    })(),
    roomProbe: r ? {
      energyAvailable: r.energyAvailable,
      energyCapacityAvailable: r.energyCapacityAvailable,
      controller: snap(r.controller), findCreeps: snap(r.find(101)),
      findStructures: snap(r.find(107)),
    } : null,
    // 地形面：KKPV 通道经公开束零拷贝视图、JSON 通道经 b64 缓存解码，
    // 逐格值与 getRawBuffer 拷出语义必须一致。
    terrainProbe: (function () {
      var raw = new Room.Terrain('W1N1').getRawBuffer();
      var before = Game.map.getTerrainAt(10, 10, 'W1N1');
      raw[10 * 50 + 10] = 99;
      return {
        len: raw.length,
        at: [Game.map.getTerrainAt(0, 0, 'W1N1'), Game.map.getTerrainAt(25, 25, 'W1N1'),
             Game.map.getTerrainAt(49, 49, 'W1N1')],
        mutationSafe: Game.map.getTerrainAt(10, 10, 'W1N1') === before,
      };
    })(),
  });
})()
"#;
    let mut runs: Vec<(
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
    )> = Vec::new();
    let mut worlds: Vec<(serde_json::Value, serde_json::Value, serde_json::Value)> = Vec::new();
    let mut creep_ids: Vec<String> = Vec::new();
    for channel in [false, true] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(channel);
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
        let spawn_id = e.place_object(
            "W1N1",
            "structure",
            &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1", "store": { "energy": 300 } }),
        )?;
        let dump = e.run_player("p1", dump_code)?;
        // 意图→结算：harvest 落地后由宿主查询快照核对世界状态；
        // 随后的自杀→墓碑轮把权威桥的 Ref 编码（墓碑 creepId）接入
        // 沙箱装配的完整环，并用第二份玩家视图核对结算后的下一轮投影。
        let src_json = serde_json::to_string(&source_id)?;
        let harvest = e.run_player(
            "p1",
            &format!("Game.creeps['w1'].harvest(Game.getObjectById({src_json}))"),
        )?;
        // 结算后的世界快照在自杀前采集（harvest 结果仍在 creep 上）。
        worlds.push((
            e.get_object(&creep_id).unwrap_or(serde_json::Value::Null),
            e.get_object(&source_id).unwrap_or(serde_json::Value::Null),
            e.get_object(&spawn_id).unwrap_or(serde_json::Value::Null),
        ));
        creep_ids.push(creep_id);
        let suicide = e.run_player("p1", "Game.creeps['w1'].suicide()")?;
        let dump2_code = r"(function () {
  var t = Game.rooms['W1N1'].find(118);
  return JSON.stringify({
    creepCount: Object.keys(Game.creeps).length,
    storeAfterHarvest: Memory.lastStore === undefined ? null : Memory.lastStore,
    tombstones: t.length,
    tombProbe: t.length ? {
      deathTime: t[0].deathTime, ticksToDecay: t[0].ticksToDecay,
      creepId: t[0].creep ? t[0].creep.id : null,
      store: t[0].store ? Object.keys(t[0].store) : null,
    } : null,
  });
})()";
        let dump2 = e.run_player("p1", dump2_code)?;
        runs.push((
            dump.expr.clone(),
            harvest.expr.clone(),
            suicide.expr.clone(),
            dump2.expr.clone(),
        ));
    }
    assert_eq!(runs[0].0, runs[1].0, "玩家可见快照两通道一致");
    assert_eq!(runs[0].1, runs[1].1, "harvest 返回码两通道一致");
    assert_eq!(runs[0].1["value"], json!(0), "harvest 预检通过");
    assert_eq!(runs[0].2, runs[1].2, "suicide 返回码两通道一致");
    assert_eq!(runs[0].3, runs[1].3, "自杀结算后的下一轮玩家视图两通道一致");
    // 地形探针的正向锁定（假等价防护）：2500 字节、三采样位为合法
    // 地形词、getRawBuffer 拷出语义成立。引擎房间的真实地形不作全平
    // 原假设——具体值由两通道主断言锁定。
    let dump_value = serde_json::from_str::<serde_json::Value>(
        runs[0].0["value"].as_str().expect("dump 返回字符串"),
    )?;
    assert_eq!(dump_value["terrainProbe"]["len"], json!(2500));
    let at = dump_value["terrainProbe"]["at"]
        .as_array()
        .expect("三采样位");
    assert_eq!(at.len(), 3);
    for word in at {
        assert!(
            matches!(word.as_str(), Some("plain") | Some("wall") | Some("swamp")),
            "地形词合法：{word}"
        );
    }
    assert_eq!(dump_value["terrainProbe"]["mutationSafe"], json!(true));
    // 墓碑经权威桥编码 creepId 为 Ref，沙箱解回 kk 前缀字符串——
    // 与死亡 creep 的 id 一致即全环闭环。
    let tomb = serde_json::from_str::<serde_json::Value>(
        runs[0].3["value"].as_str().expect("dump2 返回字符串"),
    )?;
    assert_eq!(tomb["creepCount"], json!(0), "自杀后无 creep");
    assert_eq!(tomb["tombstones"], json!(1), "恰一座墓碑");
    assert_eq!(
        tomb["tombProbe"]["creepId"],
        json!(creep_ids[0]),
        "墓碑 creepId（Ref 解码）指向死亡 creep"
    );
    assert_eq!(
        worlds[0].0["store"]["energy"],
        json!(2),
        "1×WORK×2 进 store"
    );
    assert_eq!(worlds[0].1["energy"], json!(2998), "source 扣减 2");
    assert_eq!(worlds[0], worlds[1], "结算后的世界状态两通道一致");
    Ok(())
}

/// 注入体积采样两通道形态：JSON 臂给整世界串字节；KKPV 臂给信封+束
/// 双计数与块数（=注册房间数），全部非零且只读（采样前后世界代际不变）。
pub(super) fn kkpv_injection_size_sample_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1", "store": { "energy": 300 } }),
    )?;
    let time_before = e.get_game_time();
    let json_sample = e.injection_size_sample("p1")?;
    assert_eq!(json_sample["channel"], json!("json"));
    let json_bytes = json_sample["world_json_bytes"].as_u64().unwrap();
    assert!(json_bytes > 0, "JSON 臂采样非空");
    e.set_kkpv_channel(true);
    let kkpv_sample = e.injection_size_sample("p1")?;
    assert_eq!(kkpv_sample["channel"], json!("kkpv"));
    assert_eq!(kkpv_sample["blocks"], json!(1), "每注册房间一块");
    assert!(kkpv_sample["envelope_bytes"].as_u64().unwrap() > 0);
    assert!(kkpv_sample["bundle_bytes"].as_u64().unwrap() > 0);
    // 公开地形束：与私有束同房间数、全玩家共享一份。
    assert_eq!(kkpv_sample["public_shared"], json!(true));
    let public_bytes = kkpv_sample["public_bundle_bytes"].as_u64().unwrap();
    assert!(public_bytes > 160, "公开束含至少一个块的头与地形段");
    // 每玩家注入量 = 信封 + 私有束（公开束全服一份，随玩家数摊销，
    // 不计入单玩家口径）。信封不再携带地形后，单玩家注入量须显著低于
    // JSON 整世界（其中 b64 地形约占 3/4）。
    let envelope = kkpv_sample["envelope_bytes"].as_u64().unwrap();
    let private = kkpv_sample["bundle_bytes"].as_u64().unwrap();
    assert!(
        envelope + private < json_bytes,
        "每玩家注入量（信封 {envelope} + 私有束 {private}）应低于 JSON 整世界（{json_bytes}）"
    );
    // 采样只读：世界代际不变。
    assert_eq!(e.get_game_time(), time_before, "采样不得推进世界");
    // 零注册房世界：公开束以空束直接下发（库发布拒绝无版本空束），
    // 玩家面与 JSON 通道的零房行为一致。
    e.create_shard(&json!({ "players": ["p1"], "rooms": [] }))?;
    let mut codes = std::collections::BTreeMap::new();
    codes.insert(
        "p1".to_string(),
        "JSON.stringify({ rooms: Object.keys(Game.rooms), time: Game.time })".to_string(),
    );
    let run = e.run_players(&codes)?;
    let result = run.get("p1").expect("玩家执行完成");
    assert!(result.executed, "零房世界的 KKPV 轮须照常执行");
    assert_eq!(
        result.expr["value"],
        json!("{\"rooms\":[],\"time\":0}"),
        "零房玩家面两通道同形（空 Game.rooms）"
    );
    Ok(())
}

/// 多玩家同轮 KKPV 并发（B.5 残留 5 的收口）：两名玩家在并发上限 2 下
/// 同轮执行，共享同一份公开地形束；各自玩家面正确、结算后世界正确，
/// 且与串行执行（并发 1）的结果一致——共享直读不改变任何玩家可见面。
pub(super) fn kkpv_multi_player_concurrency_impl() -> anyhow::Result<()> {
    let setup = |e: &mut EmbedEngine| -> anyhow::Result<String> {
        e.create_shard(&json!({
            "players": ["p1", "p2"],
            "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" },
                      { "name": "E2N2", "rcl": 3, "owner": "p2" }],
        }))?;
        e.place_object(
            "W1N1",
            "structure",
            &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1", "store": { "energy": 300 } }),
        )?;
        e.place_object(
            "E2N2",
            "structure",
            &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p2", "store": { "energy": 300 } }),
        )?;
        Ok(String::new())
    };
    // 探针按玩家注入对方房名（p1↔p2），不在玩家侧分支用户名形态。
    let probe_for = |other_room: &str| {
        format!(
            r#"(function () {{
  var rooms = Object.keys(Game.rooms);
  // ownTerrain：己方可见房；otherTerrain：对方 owned 房——房间级裁切
  // 下不进 Game.rooms，但公开地形覆盖全部注册房（§5），须仍可读。
  var otherRaw = new Room.Terrain('{other_room}').getRawBuffer();
  var ownRaw = new Room.Terrain(rooms[0]).getRawBuffer();
  return JSON.stringify({{
    user: Game.username,
    rooms: rooms,
    ownTerrain: rooms[0] + ':' + ownRaw.length + ':' + ownRaw.reduce(function (a, b) {{ return a + b; }}, 0),
    otherTerrain: '{other_room}:' + otherRaw.length + ':' + otherRaw.reduce(function (a, b) {{ return a + b; }}, 0),
    spawns: Object.keys(Game.spawns).length,
  }});
}})()"#
        )
    };
    let mut results = Vec::new();
    for concurrency in [1usize, 2] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(true);
        setup(&mut e)?;
        e.set_player_concurrency(concurrency)?;
        let mut codes = std::collections::BTreeMap::new();
        codes.insert("p1".to_string(), probe_for("E2N2"));
        codes.insert("p2".to_string(), probe_for("W1N1"));
        let run = e.run_players(&codes)?;
        let mut per_player = std::collections::BTreeMap::new();
        for (handle, result) in run {
            assert!(result.executed, "{handle}@并发{concurrency}: 执行须完成");
            per_player.insert(handle, result.expr["value"].clone());
        }
        results.push(per_player);
    }
    // 比较两档并发的逐玩家输出。
    let serial = &results[0];
    let concurrent = &results[1];
    assert_eq!(
        serial.get("p1"),
        concurrent.get("p1"),
        "p1 玩家面不随并发档变化"
    );
    assert_eq!(
        serial.get("p2"),
        concurrent.get("p2"),
        "p2 玩家面不随并发档变化"
    );
    let p1 = serde_json::from_str::<serde_json::Value>(
        serial["p1"].as_str().expect("p1 探针返回字符串"),
    )?;
    let p2 = serde_json::from_str::<serde_json::Value>(
        serial["p2"].as_str().expect("p2 探针返回字符串"),
    )?;
    // 各玩家只见自己的房（房间级裁切）；对方 owned 房不进 Game.rooms，
    // 但公开地形覆盖全部注册房，两玩家都能读对方房的地形（§5 语义）。
    assert_eq!(p1["rooms"], json!(["W1N1"]));
    assert_eq!(p2["rooms"], json!(["E2N2"]));
    assert!(
        p1["ownTerrain"].as_str().unwrap().starts_with("W1N1:2500:"),
        "p1 己方房地形 2500 字节"
    );
    assert!(
        p1["otherTerrain"]
            .as_str()
            .unwrap()
            .starts_with("E2N2:2500:"),
        "p1 读对方 owned 房（E2N2）的公开地形仍可读"
    );
    assert!(
        p2["otherTerrain"]
            .as_str()
            .unwrap()
            .starts_with("W1N1:2500:"),
        "p2 读对方 owned 房（W1N1）的公开地形仍可读"
    );
    Ok(())
}

/// 真实沙箱的 move、harvest 意图经清洗和结算后，在下一轮玩家视图中可见。
/// 另用脚本沙箱产生相同意图，比较两种入口的世界变化。
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
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
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

/// 检查不可见房和对象不会出现在玩家视图中；直接注入工地意图也不能绕过数量上限。
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
    // 不可见对象应返回 null；== null 同时接受 undefined。
    assert_eq!(v[2], "true", "敌方对象 getObjectById 得 null: {v:?}");

    // p2 的视图对偶
    let run = e.run_player("p2", "Object.keys(Game.rooms).join(',')")?;
    assert_eq!(run.expr["value"], "W2N2", "p2 只见自家房");

    // 我方对象的 my 判定
    let mine_json = serde_json::to_string(&mine)?;
    let run = e.run_player("p1", &format!("Game.getObjectById({mine_json}).my"))?;
    assert_eq!(run.expr["value"], true);

    // 直接注入 120 条工地创建意图，也只能创建 100 个。
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

    // 自家工地都应出现在 Game.constructionSites 中，即使所在房间不可见。
    // 工地本身不授予房间视野，因此不能把这些房间加入 Game.rooms。
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

/// 验证 transfer、withdraw、drop、pickup 的资源流向和容量限制。
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

/// 验证 pull 与 move 配对位移、suicide 的墓碑和资源回收，以及 say 不影响移动。
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

/// 检查直接摆放 mineral、extractor 后，查询快照和玩家 FIND 都能识别其类型。
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

/// 检查玩家视野、墓碑和遗迹提取、冷却预检，以及事件日志不被玩家修改。
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

/// 检查紧凑地形从世界投影经 base64 编码和 prelude 解码后，仍保留原地形值。
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
