//! KKPV 二进制通道与 JSON 参考通道的玩家可见面等价测试。
//!
//! 同一夹具世界分别经 `begin_tick`（JSON）与 `begin_tick_kkpv`（信封 +
//! 块束）注入两个沙箱，执行同一份深度快照表达式后逐字节比对——覆盖
//! 解码器的头/目录/桶/位图/嵌套标签/UTF-8 全路径。快照分两层：对象
//! 自有可枚举键的通吃遍历 + 各对象族的显式字段探针（原型 getter 上的
//! 字段不会被 Object.keys 触发，须显式读取）。夹具 id 使用 kk 前缀
//! 十六进制，使两条通道的对象 ID 完全一致；Ref 编码只出现在权威世界
//! 桥（墓碑/废墟），由 kk-runtime 集成测试覆盖。

use serde_json::json;

use kk_runner::{Sandbox, SandboxConfig, TickPlan};

/// 深度快照表达式：遍历 Game 玩家可见集合，键排序后 JSON 化。
/// 'room' 键是循环引用（wrapper ↔ room）一律跳过；'pos' 归一为
/// {x, y, roomName}；函数跳过（与 JSON.stringify 语义一致）；显式探针
/// 逐字段读取，抛错记为 THROW 前缀字符串——两通道任何差异都会体现在
/// 输出里。
const DUMP: &str = r#"
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
  function probe(o, keys) {
    if (o === null || o === undefined) return null;
    var out = {};
    for (var i = 0; i < keys.length; i++) {
      var k = keys[i];
      try { out[k] = snap(o[k], 1); } catch (e) { out[k] = 'THROW:' + e.message; }
    }
    return out;
  }
  var CREEP_KEYS = ['name', 'owner', 'username', 'my', 'store', 'body', 'fatigue',
    'hits', 'hitsMax', 'spawning', 'saying', 'ticksToLive', 'carry', 'carryCapacity',
    'notifyWhenAttacked'];
  var STRUCT_KEYS = ['structureType', 'owner', 'my', 'hits', 'hitsMax', 'store',
    'energy', 'energyCapacity', 'isActive', 'notifyWhenAttacked'];
  function probeMap(coll, keys) {
    var out = {};
    for (var n in coll) out[n] = probe(coll[n], keys);
    return out;
  }
  var r = Game.rooms['W1N1'];
  return JSON.stringify({
    time: Game.time,
    cpuLimit: Game.cpuLimit,
    cpu: snap(Game.cpu),
    gcl: snap(Game.gcl),
    shard: snap(Game.shard),
    creeps: snap(Game.creeps),
    spawns: snap(Game.spawns),
    structures: snap(Game.structures),
    constructionSites: snap(Game.constructionSites),
    flags: snap(Game.flags),
    resources: snap(Game.resources),
    rooms: snap(Game.rooms),
    market: snap(Game.market),
    creepProbe: probeMap(Game.creeps, CREEP_KEYS),
    spawnProbe: probeMap(Game.spawns,
      STRUCT_KEYS.concat(['name', 'spawning', 'spawnRemainingTime'])),
    structureProbe: probeMap(Game.structures, STRUCT_KEYS),
    flagProbe: probeMap(Game.flags, ['name', 'color', 'secondaryColor']),
    resourceProbe: probeMap(Game.resources, ['resourceType', 'amount']),
    siteProbe: probeMap(Game.constructionSites,
      ['structureType', 'progress', 'progressTotal', 'owner', 'my']),
    // 键序探针（不排序）：Store 把原始记录 store 的键序复制为玩家可见
    // 面的自有键。两通道一致的前提是编码端保留嵌套条目顺序，且 JSON
    // 参考通道的 serde_json::Map 在 preserve_order 构建下保持插入序。
    storeKeyOrder: (function () {
      var t = Game.getObjectById('kk000000000000000c');
      var c = Game.creeps['采集者😀'];
      return {
        terminal: t ? Object.keys(t.store) : null,
        creep: c ? Object.keys(c.store) : null,
      };
    })(),
    roomProbe: r ? {
      name: r.name, energyAvailable: r.energyAvailable,
      energyCapacityAvailable: r.energyCapacityAvailable, mode: r.mode,
      controller: probe(r.controller,
        ['level', 'progress', 'progressTotal', 'owner', 'username', 'my',
         'safeMode', 'safeModeAvailable', 'ticksToDowngrade', 'isPowerEnabled']),
      storage: probe(r.storage, ['store', 'storeCapacity']),
      // FIND_CREEPS=101 / FIND_SOURCES=105 / FIND_DROPPED_RESOURCES=106 /
      // FIND_STRUCTURES=107 / FIND_CONSTRUCTION_SITES=109
      findCreeps: snap(r.find(101)), findSources: snap(r.find(105)),
      findDropped: snap(r.find(106)), findStructures: snap(r.find(107)),
      findSites: snap(r.find(109)),
      // lookAt 覆盖 look 寄存器：creep 格、source 格、tombstone 格、ruin 格。
      lookCreep: snap(r.lookAt(25, 25)), lookSource: snap(r.lookAt(15, 15)),
      lookTomb: snap(r.lookAt(27, 27)), lookRuin: snap(r.lookAt(28, 28)),
    } : null,
    byId: (function () {
      var out = {};
      var ids = ['kk0000000000000001', 'kk0000000000000002', 'kk000000000000000a',
                 'kk000000000000000b', 'kk000000000000000c', 'kk000000000000000e',
                 'kk000000000000000f', 'kk0000000000dead'];
      for (var i = 0; i < ids.length; i++) {
        var o = Game.getObjectById(ids[i]);
        out[ids[i]] = o === null || o === undefined ? null
          : probe(o, CREEP_KEYS.concat(['creepId', 'deathTime', 'ticksToDecay',
             'structure', 'destroyTime', 'storeCapacity',
             'mineralType', 'mineralAmount', 'density']));
      }
      return out;
    })(),
  });
})()
"#;

/// 覆盖各字段形态的夹具世界：定宽列（bool 含 true、整数含负数、浮点、
/// 超 2^53 的 u64）、字符串（CJK、emoji、空串）、嵌套数组/记录
/// （body/store/spawning、多资源 store 的插入序键）、缺失与 null 并存、
/// 多类型桶、敌方对象（可见房 creep、被排除的敌方旗）、不可见房
/// （E2N2 只给地形；W2N2 只含自家工地——不授予视野但入全局集合）。
fn kkpv_world() -> serde_json::Value {
    json!({
        "time": 7,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "gcl": 1234,
        "visibleRooms": ["W1N1"],
        "rooms": {
            "W1N1": { "name": "W1N1", "terrain": vec![0u8; 2500],
                      "energyAvailable": 300, "energyCapacityAvailable": 550,
                      "events": [ { "event": 1, "objectId": "kk0000000000000002", "data": { "amount": 5 } } ] },
            "E2N2": { "name": "E2N2", "terrain": vec![1u8; 2500] },
            "W2N2": { "name": "W2N2", "terrain": vec![2u8; 2500] }
        },
        "objects": [
            { "id": "kk0000000000000001", "type": "creep", "room": "W1N1", "x": 25, "y": 25,
              "name": "采集者😀", "owner": "p1", "body": ["move", "work", "carry"],
              "store": { "energy": 12.5 }, "fatigue": -3000, "hits": 100, "age_time": 1500,
              "spawning": false, "notifyWhenAttacked": true,
              "say": null, "saying": "" },
            { "id": "kk0000000000000002", "type": "spawn", "room": "W1N1", "x": 20, "y": 20,
              "name": "Spawn1", "owner": "p1", "store": { "energy": 300 },
              "energyCapacity": 300, "hits": 5000, "off": false,
              "spawning": { "name": "新兵", "need": [50, 30, 30], "remainingTime": 12,
                            "spawnRemainingTime": 20 } },
            { "id": "kk0000000000000003", "type": "structure", "structureType": "controller",
              "room": "W1N1", "x": 10, "y": 10, "level": 3, "progress": 100.5,
              "progressTotal": 9007199254740993u64, "owner": "p1", "safeMode": 0,
              "downgradeProgress": null },
            { "id": "kk0000000000000004", "type": "structure", "structureType": "source",
              "room": "W1N1", "x": 15, "y": 15, "energy": 2500, "energyCapacity": 3000,
              "ticksToRegeneration": null },
            { "id": "kk0000000000000005", "type": "structure", "structureType": "extension",
              "room": "W1N1", "x": 12, "y": 12, "owner": "p1",
              "store": { "energy": 50 }, "energyCapacity": 50, "hits": 1000 },
            { "id": "kk0000000000000006", "type": "flag", "room": "W1N1", "x": 30, "y": 30,
              "name": "旗帜", "owner": "p1", "color": 1, "secondaryColor": 1 },
            { "id": "kk0000000000000007", "type": "constructionSite", "room": "W1N1",
              "x": 31, "y": 31, "owner": "p1", "structureType": "road", "progress": 0,
              "progressTotal": 300 },
            { "id": "kk0000000000000008", "type": "resource", "room": "W1N1", "x": 26, "y": 26,
              "resourceType": "energy", "amount": 200 },
            { "id": "kk0000000000000009", "type": "structure", "structureType": "rampart",
              "room": "W1N1", "x": 20, "y": 21, "hits": 10000, "owner": "p1",
              "nextDecayTime": 8888888 },
            { "id": "kk000000000000000a", "type": "tombstone", "room": "W1N1", "x": 27, "y": 27,
              "creepId": null, "deathTime": 5, "ticksToDecay": 5 },
            { "id": "kk000000000000000b", "type": "ruin", "room": "W1N1", "x": 28, "y": 28,
              "structure": { "id": "kk0000000000000005", "type": "extension",
                             "x": 12, "y": 12, "owner": "p1" },
              "destroyTime": 6, "ticksToDecay": 10 },
            // 多资源 store：键序为插入序（energy 在大写资源前），
            // storeKeyOrder 探针不排序地透出。
            { "id": "kk000000000000000c", "type": "structure", "structureType": "terminal",
              "room": "W1N1", "x": 5, "y": 5, "owner": "p1", "hits": 10000,
              "store": { "energy": 4000, "U": 10, "XUH2O": 3, "K": 7 },
              "storeCapacityResource": { "energy": 300000 } },
            { "id": "kk000000000000000d", "type": "structure", "structureType": "mineral",
              "room": "W1N1", "x": 30, "y": 5, "mineralType": "XUH2O",
              "mineralAmount": 50000, "density": 3 },
            // 敌方 creep：可见房内对 p1 出现（find/byId 可达，不入 Game.creeps）；
            // 空 body 数组兼锁空集合的嵌套形态
            { "id": "kk000000000000000e", "type": "creep", "room": "W1N1", "x": 40, "y": 40,
              "name": "敌兵", "owner": "p2", "body": [], "store": {},
              "fatigue": 0, "hits": 100 },
            // 敌方旗：他人旗被排除，任何玩家集合都不可达
            { "id": "kk000000000000000f", "type": "flag", "room": "W1N1", "x": 41, "y": 41,
              "name": "敌旗", "owner": "p2", "color": 2, "secondaryColor": 2 },
            // 不可见房（W2N2）的自家工地：入 Game.constructionSites，不建 Game.rooms
            { "id": "kk0000000000000010", "type": "constructionSite", "room": "W2N2",
              "x": 10, "y": 10, "owner": "p1", "structureType": "extension",
              "progress": 10, "progressTotal": 200 }
        ]
    })
}

/// 两条通道装配出的 Game 玩家可见面逐字节一致。
pub fn kkpv_channel_equivalence_impl() -> anyhow::Result<()> {
    let world = kkpv_world();
    let json_dump = {
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(matches!(sb.begin_tick(&world)?, TickPlan::Run { .. }));
        let dump = sb.run_expr(DUMP)?;
        sb.end_tick()?;
        sb.settle_bill();
        dump
    };
    let kkpv_dump = {
        let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
        let bundle = kk_world::kkpv::kkpv_bundle_from_world_json(&world, [0; 32], [0; 32])
            .map_err(|e| anyhow::anyhow!("夹具世界 KKPV 转换失败：{e}"))?;
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(matches!(
            sb.begin_tick_kkpv(&envelope, &bundle)?,
            TickPlan::Run { .. }
        ));
        let dump = sb.run_expr(DUMP)?;
        sb.end_tick()?;
        sb.settle_bill();
        dump
    };
    // 首个差异窗口先行输出：主断言的两份超长单行 JSON 不可读。
    let a_str = json_dump["value"].as_str().unwrap_or_default();
    let b_str = kkpv_dump["value"].as_str().unwrap_or_default();
    if a_str != b_str {
        let mut at = 0;
        while at < a_str.len() && at < b_str.len() && a_str.as_bytes()[at] == b_str.as_bytes()[at] {
            at += 1;
        }
        let window = |s: &str| s[at.saturating_sub(80)..(at + 160).min(s.len())].to_string();
        panic!(
            "KKPV 通道与 JSON 参考通道的玩家可见面不一致，首个差异 @ {at}：
  JSON: …{}…
  KKPV: …{}…",
            window(a_str),
            window(b_str)
        );
    }
    // 快照非空且覆盖核心集合，防止两条通道同时产出空壳导致假等价。
    let dump_str = b_str;
    for needle in [
        "采集者😀",
        "Spawn1",
        "controller",
        "rampart",
        "tombstone",
        "ruin",
        "spawning",
        "新兵",
        "terminal",
        "XUH2O",
        "敌兵",
        "THROW",
    ] {
        let expect_throw = needle == "THROW";
        let hit = dump_str.contains(needle);
        assert!(
            if expect_throw { !hit } else { hit },
            "快照覆盖检查失败（{needle}）：{}",
            &dump_str[..dump_str.len().min(1500)]
        );
    }
    // 手搓世界不做对象级权限过滤（可见性裁切在权威桥层锁定）：
    // 敌方对象按宿主投递原样携带，两通道一致即为本层验收。
    // 负数与键序的正向锁定：负疲劳与插入序 store 键序必须在快照内。
    assert!(dump_str.contains("-3000"), "负数 Int 必须精确解码");
    assert!(
        dump_str.contains("\"energy\",\"U\",\"XUH2O\""),
        "多资源 store 的键序必须是插入序（编码端保序）：{}",
        &dump_str[..dump_str.len().min(1500)]
    );
    Ok(())
}

/// Ref 编码的手工块解码：权威桥只把墓碑 `creepId` 与废墟
/// `structure.id` 编为 Ref（u64），夹具转换器全部保字符串，因此 Ref
/// 路径在此用手工块直接锁定——列 Ref 与嵌套记录内 Ref 各一。
pub fn kkpv_ref_decode_impl() -> anyhow::Result<()> {
    use kk_schema::kkpv::{KkpvBlockInput, KkpvLimits, KkpvObject, KkpvValue, encode_bundle};
    let block = KkpvBlockInput {
        private: true,
        tick: 3,
        generation: 1,
        plan_hash: [0; 32],
        audience_hash: [0; 32],
        room_name: "W1N1".into(),
        terrain: Some(vec![0u8; 2500]),
        objects: vec![
            KkpvObject {
                id: 0x1a,
                type_key: "builtin:tombstone".into(),
                fields: vec![
                    ("room".into(), KkpvValue::Str("W1N1".into())),
                    ("type".into(), KkpvValue::Str("tombstone".into())),
                    ("x".into(), KkpvValue::Uint(27)),
                    ("y".into(), KkpvValue::Uint(27)),
                    // 列式 Ref：解码为 kk 前缀十六进制字符串
                    ("creepId".into(), KkpvValue::Ref(0x05)),
                    ("deathTime".into(), KkpvValue::Uint(5)),
                    ("ticksToDecay".into(), KkpvValue::Uint(5)),
                ],
            },
            KkpvObject {
                id: 0x1b,
                type_key: "builtin:ruin".into(),
                fields: vec![
                    ("room".into(), KkpvValue::Str("W1N1".into())),
                    ("type".into(), KkpvValue::Str("ruin".into())),
                    ("x".into(), KkpvValue::Uint(28)),
                    ("y".into(), KkpvValue::Uint(28)),
                    // 嵌套记录内的 Ref 与字符串混排；键形与 object_json 的
                    // 废墟 structure 成员一致（type 而非 structureType）
                    (
                        "structure".into(),
                        KkpvValue::Object(vec![
                            ("id".into(), KkpvValue::Ref(0x06)),
                            ("type".into(), KkpvValue::Str("extension".into())),
                            ("x".into(), KkpvValue::Uint(12)),
                            ("y".into(), KkpvValue::Uint(12)),
                        ]),
                    ),
                    ("destroyTime".into(), KkpvValue::Uint(6)),
                    ("ticksToDecay".into(), KkpvValue::Uint(10)),
                ],
            },
        ],
    };
    let bundle = encode_bundle(&[block], &KkpvLimits::default())
        .map_err(|e| anyhow::anyhow!("手工块编码失败：{e}"))?;
    let envelope = json!({
        "time": 3,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "visibleRooms": ["W1N1"],
        "rooms": { "W1N1": { "name": "W1N1", "terrain": vec![0u8; 2500] } },
    });
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    assert!(matches!(
        sb.begin_tick_kkpv(&envelope, &bundle)?,
        TickPlan::Run { .. }
    ));
    let out = sb.run_expr(
        r#"(function () {
  var t = Game.getObjectById('kk000000000000001a');
  var r = Game.getObjectById('kk000000000000001b');
  return JSON.stringify({
    tomb: t && { deathTime: t.deathTime, ticksToDecay: t.ticksToDecay,
                 creepId: t.creep ? t.creep.id : null },
    ruin: r && { destroyTime: r.destroyTime, ticksToDecay: r.ticksToDecay,
                 structureType: r.structure && r.structure.structureType,
                 structureId: r.structure && r.structure.id },
  });
})()"#,
    )?;
    sb.end_tick()?;
    sb.settle_bill();
    let value = serde_json::from_str::<serde_json::Value>(
        out["value"].as_str().expect("表达式返回字符串"),
    )?;
    assert_eq!(value["tomb"]["deathTime"], json!(5));
    assert_eq!(value["tomb"]["ticksToDecay"], json!(5));
    // 列式 Ref → kk0000000000000005；death creep 代理的 id 即 creepId
    assert_eq!(value["tomb"]["creepId"], json!("kk0000000000000005"));
    assert_eq!(value["ruin"]["destroyTime"], json!(6));
    assert_eq!(value["ruin"]["ticksToDecay"], json!(10));
    assert_eq!(value["ruin"]["structureType"], json!("extension"));
    // 嵌套记录内 Ref → kk0000000000000006
    assert_eq!(value["ruin"]["structureId"], json!("kk0000000000000006"));
    Ok(())
}

/// 空束（零块、4 字节框架）与无对象世界：两通道装配出的空玩家面一致，
/// 且不因缺 objects 键而分叉。
pub fn kkpv_empty_bundle_impl() -> anyhow::Result<()> {
    let world = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "visibleRooms": [],
        "rooms": { "E2N2": { "name": "E2N2", "terrain": vec![0u8; 2500] } },
        "objects": [],
    });
    let code =
        "JSON.stringify({ time: Game.time, creeps: Game.creeps, rooms: Object.keys(Game.rooms) })";
    let json_out = {
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(matches!(sb.begin_tick(&world)?, TickPlan::Run { .. }));
        let out = sb.run_expr(code)?.clone();
        sb.end_tick()?;
        sb.settle_bill();
        out
    };
    let kkpv_out = {
        let bundle = kk_world::kkpv::kkpv_bundle_from_world_json(&world, [0; 32], [0; 32])
            .map_err(|e| anyhow::anyhow!("空世界转换失败：{e}"))?;
        assert_eq!(bundle.len(), 4, "零对象世界是仅含零计数的束");
        let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(matches!(
            sb.begin_tick_kkpv(&envelope, &bundle)?,
            TickPlan::Run { .. }
        ));
        let out = sb.run_expr(code)?.clone();
        sb.end_tick()?;
        sb.settle_bill();
        out
    };
    assert_eq!(json_out, kkpv_out, "空玩家面两通道一致");
    // 不可见房不进 Game.rooms；空玩家面在两通道同形且非空壳。
    assert_eq!(
        json_out["value"],
        json!("{\"time\":1,\"creeps\":{},\"rooms\":[]}")
    );
    Ok(())
}

/// 损坏束的 fail-loud：截断的块束使注入失败并带回 KKPV 错误文本，
/// 不产生部分装配的世界。
pub fn kkpv_truncated_bundle_fails_impl() -> anyhow::Result<()> {
    let world = kkpv_world();
    let bundle = kk_world::kkpv::kkpv_bundle_from_world_json(&world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("夹具转换失败：{e}"))?;
    let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
    let truncated = &bundle[..bundle.len() - 1];
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let err = sb
        .begin_tick_kkpv(&envelope, truncated)
        .expect_err("截断束必须注入失败");
    assert!(
        err.to_string().contains("KKPV"),
        "错误须带 KKPV 前缀：{err:#}"
    );
    Ok(())
}
