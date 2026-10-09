//! KKPV/JSON 等价性、并发与共享束生命周期。
use super::*;

/// KKPV 二进制通道与 JSON 参考通道在真实分片上的端到端等价：同一分片
/// 设置与玩家脚本，两条通道各自的可见快照、意图返回码与结算后的世界
/// 状态完全一致。权威桥的 Ref 编码（墓碑/废墟）无法经 place 通道构造，
/// 由 kk-runner 沙箱级测试用手工块锁定。
pub(crate) fn kkpv_channel_e2e_equivalence_impl() -> anyhow::Result<()> {
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
pub(crate) fn kkpv_injection_size_sample_impl() -> anyhow::Result<()> {
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
        json!("{\"rooms\":[],\"time\":1}"),
        "零房玩家面两通道同形（空 Game.rooms，time 为热身后首拍）"
    );
    Ok(())
}

/// 多玩家同轮 KKPV 并发：两名玩家在并发上限 2 下
/// 同轮执行，共享同一份公开地形束；各自玩家面正确、结算后世界正确，
/// 且与串行执行（并发 1）的结果一致——共享直读不改变任何玩家可见面。
pub(crate) fn kkpv_multi_player_concurrency_impl() -> anyhow::Result<()> {
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

/// 共享公开束的跨 tick 钉住端到端锁定。玩家把当轮地形视图钉进模块全局
/// （globalThis 数组逐 tick 追加）时须保持的性质：
/// - Rust 侧保留报告是存活下界：派发结束后句柄全部释放，稳态只列当前
///   代际（tick 等于本轮执行绑定值）且束体积逐轮不膨胀；被钉视图只在
///   V8 堆内滞留、受 V8 外部内存压力间接约束且无硬上限，不进报告读数，
///   也不在本测试断言范围。报告只数库句柄：工作者只携带共享束的
///   BackingStore 引用，不持有库内条目句柄。
/// - 拷出语义隔离：篡改 getRawBuffer 返回的拷贝不影响同一玩家对被篡改
///   格的当轮读取（拷出经加载期捕获的构造器，共享视图不外泄）。
/// - 旧视图存活且值不变：最早钉住的视图逐轮仍可读、内容不变；旧代际
///   的回收与存活由保留报告（库侧）与旧视图可读（V8 侧）组合锁定。
/// - 跨玩家完整性：钉住与拷出篡改不改变另一玩家对共享束的读取。
pub(crate) fn kkpv_public_bundle_pin_hostile_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(true);
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
    // p1：每 tick 钉住当轮公开束的地形视图，并从拷出副本与最早钉住的
    // 旧视图两端取样。pins 逐轮 +1 证明模块全局跨 tick 存活——被钉住
    // 的旧视图内存因此能逐轮线性累积（每 tick 追加一份当轮视图）。
    let pin_code = r#"(function () {
  globalThis.__pins = globalThis.__pins || [];
  globalThis.__pins.push(new Room.Terrain('W1N1'));
  var before = Game.map.getTerrainAt(10, 10, 'W1N1');
  var raw = new Room.Terrain('W1N1').getRawBuffer();
  raw[10 * 50 + 10] = 99;
  var oldest = globalThis.__pins[0];
  return JSON.stringify({
    time: Game.time,
    pins: globalThis.__pins.length,
    mutationSafe: Game.map.getTerrainAt(10, 10, 'W1N1') === before,
    currentAt: globalThis.__pins[globalThis.__pins.length - 1].get(10, 10),
    oldestAt: oldest.get(10, 10),
    oldestSum: oldest.getRawBuffer().reduce(function (a, b) { return a + b; }, 0),
  });
})()"#;
    // p2：读自己的房与对方 owned 房的公开地形；地形静态，除去 time 后
    // 逐轮结果必须完全一致。
    let probe_code = r#"(function () {
  var own = new Room.Terrain('E2N2').getRawBuffer();
  var other = new Room.Terrain('W1N1').getRawBuffer();
  var ownTerrain = new Room.Terrain('E2N2');
  return JSON.stringify({
    time: Game.time,
    own: 'E2N2:' + own.length + ':' + own.reduce(function (a, b) { return a + b; }, 0),
    other: 'W1N1:' + other.length + ':' + other.reduce(function (a, b) { return a + b; }, 0),
    at: [ownTerrain.get(0, 0), ownTerrain.get(25, 25),
         ownTerrain.get(49, 49), new Room.Terrain('W1N1').get(10, 10)],
  });
})()"#;
    let mut p2_baseline: Option<serde_json::Value> = None;
    let mut oldest_sum: Option<i64> = None;
    let mut bundle_bytes: Option<usize> = None;
    for round in 0..6u64 {
        let mut codes = std::collections::BTreeMap::new();
        codes.insert("p1".to_string(), pin_code.to_string());
        codes.insert("p2".to_string(), probe_code.to_string());
        let run = e.run_players(&codes)?;
        assert!(run["p1"].executed, "p1 须执行完成：{:?}", run["p1"].expr);
        assert!(run["p2"].executed, "p2 须执行完成：{:?}", run["p2"].expr);
        let p1 = serde_json::from_str::<serde_json::Value>(
            run["p1"].expr["value"]
                .as_str()
                .expect("p1 钉住探针返回字符串"),
        )?;
        let p2 = serde_json::from_str::<serde_json::Value>(
            run["p2"].expr["value"]
                .as_str()
                .expect("p2 读取探针返回字符串"),
        )?;
        let time = p1["time"].as_u64().expect("time 为数值");
        assert_eq!(
            time,
            round + 1,
            "第 {round} 轮的执行绑定 tick（热身后偏移 1）"
        );
        assert_eq!(
            p2["time"].as_u64(),
            Some(round + 1),
            "两名玩家同轮绑定同一 tick"
        );
        assert_eq!(
            p1["pins"].as_u64(),
            Some(round + 1),
            "模块全局跨 tick 存活，钉住视图逐轮累积"
        );
        assert_eq!(
            p1["mutationSafe"],
            json!(true),
            "篡改拷出副本不得影响地形读取"
        );
        assert_eq!(
            p1["oldestAt"].as_i64(),
            p1["currentAt"].as_i64(),
            "最早钉住的旧视图读数与当轮新视图一致（地形静态，值不变）"
        );
        assert_eq!(
            p1["oldestAt"].as_i64(),
            p2["at"][3].as_i64(),
            "钉住者的旧视图读数与另一玩家的当前读取一致（跨玩家完整性）"
        );
        let sum = p1["oldestSum"].as_i64().expect("旧视图求和为数值");
        if let Some(prev) = oldest_sum {
            assert_eq!(sum, prev, "被钉住的旧代际束内容逐轮不变");
        }
        oldest_sum = Some(sum);
        // 除去 time 后 p2 的读取逐轮一致：钉住与拷出篡改不改变另一玩家
        // 对共享束的读取面。
        let p2_static = {
            let mut v = p2.clone();
            v.as_object_mut().expect("p2 探针为对象").remove("time");
            v
        };
        if let Some(base) = &p2_baseline {
            assert_eq!(&p2_static, base, "p2 共享束读取不受钉住影响");
        }
        p2_baseline = Some(p2_static);
        // Rust 侧存活下界：本轮派发结束后旧代际全部回收，只剩当前代际
        // 且无库外句柄；V8 堆内滞留不进报告读数。夹具固定（静态地形、
        // 固定计划），束体积逐轮必须相等——随代际累积膨胀属泄漏形态。
        let report = e.kkpv_public_retention_report();
        assert_eq!(report.len(), 1, "稳态保留报告只列当前代际");
        assert_eq!(
            report[0].epoch.tick,
            round + 1,
            "当前代际即本轮执行绑定 tick（热身后偏移 1）"
        );
        assert_eq!(report[0].handles, 0, "派发结束后无库外句柄");
        assert!(report[0].bytes > 0, "当前代际束非空");
        if let Some(prev) = bundle_bytes {
            assert_eq!(report[0].bytes, prev, "静态夹具下当前代际束体积逐轮不膨胀");
        }
        bundle_bytes = Some(report[0].bytes);
    }
    Ok(())
}
