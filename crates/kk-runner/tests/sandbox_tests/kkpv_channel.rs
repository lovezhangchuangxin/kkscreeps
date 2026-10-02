//! KKPV 二进制通道与 JSON 参考通道的玩家可见面等价测试。
//!
//! 同一夹具世界分别经 `begin_tick`（JSON）与 `begin_tick_kkpv`（信封 +
//! 公开地形束 + 私有对象束）注入两个沙箱，执行同一份深度快照表达式后
//! 逐字节比对——覆盖解码器的头/目录/桶/位图/嵌套标签/UTF-8 全路径与
//! 两条地形路径（本夹具的 JSON 臂是数组旧形态；b64+rev 紧凑形态由
//! embed e2e 与 terrain_compact_protocol 测试覆盖，KKPV 臂是公开束零
//! 拷贝视图）。快照分两层：对象自有可枚举键的通吃遍历 + 各对象族的
//! 显式字段探针（原型 getter 上的字段不会被 Object.keys 触发，须显式
//! 读取）。夹具 id 使用 kk 前缀十六进制，使两条通道的对象 ID 完全
//! 一致；Ref 编码只出现在权威世界桥（墓碑/废墟），由 kk-runtime
//! 集成测试覆盖。敌意环境探针（HOSTILE）锁定共享地形视图的玩家
//! 不可达性。

use std::sync::Arc;

use serde_json::json;

use kk_runner::{PlayerThreads, Sandbox, SandboxConfig, SharedKkpvBundle, TickPlan};

/// 夹具世界的两束（公开地形束 + 私有对象束），都以共享句柄返回。
fn fixture_bundles(
    world: &serde_json::Value,
) -> anyhow::Result<(SharedKkpvBundle, SharedKkpvBundle)> {
    let private = kk_world::kkpv::kkpv_bundle_from_world_json(world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("夹具世界私有束转换失败：{e}"))?;
    let public = kk_world::kkpv::kkpv_public_bundle_from_world_json(world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("夹具世界公开束转换失败：{e}"))?;
    Ok((
        SharedKkpvBundle::from_vec(public),
        SharedKkpvBundle::from_vec(private),
    ))
}

/// 深度快照表达式：遍历 Game 玩家可见集合，键排序后 JSON 化。
/// 'room' 键是循环引用（wrapper ↔ room）一律跳过；'pos' 归一为
/// {x, y, roomName}；函数跳过（与 JSON.stringify 语义一致）；显式探针
/// 逐字段读取，抛错记为 THROW 前缀字符串——两通道任何差异都会体现在
/// 输出里。
const DUMP: &str = r#"(function () {
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
      // 地形面：可见房 Terrain.get、跨房（含不可见房）getTerrainAt。
      terrainGet: (function () {
        var t = new Room.Terrain('W1N1');
        return [t.get(0, 0), t.get(25, 25), t.get(49, 49)];
      })(),
      exits: snap(r.find(7)),
    } : null,
    // 逐房地形全量走查（可见与不可见房都要同值）：抽样位码 + 全量和 +
    // getRawBuffer 拷出语义——向返回缓冲区写入不得影响后续读取（KKPV
    // 通道的视图锚定共享内存，玩家面只能拿到拷贝）。
    terrainProbe: (function () {
      var out = {};
      ['W1N1', 'E2N2', 'W2N2'].forEach(function (room) {
        var t = new Room.Terrain(room);
        var raw = t.getRawBuffer();
        var before = t.get(10, 10);
        raw[10 * 50 + 10] = 99;
        out[room] = {
          len: raw.length,
          sum: raw.reduce(function (a, b) { return a + b; }, 0) - 99 + before,
          at: [raw[0], raw[625], raw[1249], raw[2499]],
          mapAt: [Game.map.getTerrainAt(0, 0, room),
                  Game.map.getTerrainAt(25, 25, room),
                  Game.map.getTerrainAt(49, 49, room)],
          mutationSafe: t.get(10, 10) === before,
        };
      });
      return out;
    })(),
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

/// 敌意环境探针（§6 玩家不可达性的负向验收）：三条窃取地形状体的路径
/// ——伪 destination 的 duck `.set`、替换全局 `Uint8Array` 构造器、替换
/// `Uint8Array.prototype.set`。任何一条把内部数组（KKPV 通道下锚定
/// 跨玩家共享内存的视图）递给玩家代码即判失败。两条通道都执行并比对。
const HOSTILE: &str = r#"
(function () {
  var leaked = [];
  var realU8 = Uint8Array;
  var realSet = Uint8Array.prototype.set;
  var out = {};
  var t = new Room.Terrain('W1N1');
  // 1) 伪 destination：真 set 的 V8 品牌校验应在执行任何玩家代码前抛错。
  try {
    t.getRawBuffer({ set: function (v) { leaked.push(v); }, length: 2500 });
    out.fakeDest = 'no-throw';
  } catch (e) { out.fakeDest = 'THROW:' + e.constructor.name; }
  // 2) 替换全局 Uint8Array 为间谍构造器：拷出路径须用加载期捕获的构造器。
  globalThis.Uint8Array = function (a) { leaked.push(a); return new realU8(0); };
  try {
    var r = t.getRawBuffer();
    out.replacedCtor = r && r.length === 2500 ? 'copy-2500' : 'wrong:' + (r && r.length);
  } catch (e) { out.replacedCtor = 'THROW:' + e.constructor.name; }
  globalThis.Uint8Array = realU8;
  // 3) 替换原型 set 为间谍：合法 destination 的拷贝须走捕获的真 set。
  //    用非零地形房（E2N2 全墙=1）：间谍拦截真拷贝时 dest 保持全 0，
  //    与期望的 filled:1:1 分离——本路径独立杀伤，不依赖 leakCount。
  Uint8Array.prototype.set = function (v) { leaked.push(v); };
  try {
    var dest = new realU8(2500);
    new Room.Terrain('E2N2').getRawBuffer(dest);
    out.replacedSet = 'filled:' + dest[0] + ':' + dest[2499];
  } catch (e) { out.replacedSet = 'THROW:' + e.constructor.name; }
  Uint8Array.prototype.set = realSet;
  // 敌意环境不得残留影响地形读取。
  out.terrainAfter = new Room.Terrain('W1N1').get(25, 25);
  out.leakCount = leaked.length;
  return JSON.stringify(out);
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
        let hostile = sb.run_expr(HOSTILE)?.clone();
        sb.end_tick()?;
        sb.settle_bill();
        (dump, hostile)
    };
    let kkpv_dump = {
        let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
        let (public, private) = fixture_bundles(&world)?;
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(matches!(
            sb.begin_tick_kkpv(&envelope, &public, &private)?,
            TickPlan::Run { .. }
        ));
        let dump = sb.run_expr(DUMP)?;
        let hostile = sb.run_expr(HOSTILE)?.clone();
        sb.end_tick()?;
        sb.settle_bill();
        (dump, hostile)
    };
    let (json_dump, json_hostile) = json_dump;
    let (kkpv_dump, kkpv_hostile) = kkpv_dump;
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
    // 地形面的正向锁定：三房地形走查与拷出语义必须在快照内（W1N1 全
    // 平原、E2N2 全墙、W2N2 全沼泽——三条地形路径的分型由夹具区分）。
    let terrain = serde_json::from_str::<serde_json::Value>(dump_str)?["terrainProbe"].clone();
    assert_eq!(terrain["W1N1"]["len"], json!(2500));
    assert_eq!(terrain["W1N1"]["mapAt"], json!(["plain", "plain", "plain"]));
    assert_eq!(terrain["E2N2"]["mapAt"], json!(["wall", "wall", "wall"]));
    assert_eq!(terrain["W2N2"]["mapAt"], json!(["swamp", "swamp", "swamp"]));
    assert_eq!(terrain["W2N2"]["sum"], json!(5000), "沼泽房字节全为 2");
    assert_eq!(
        terrain["W1N1"]["mutationSafe"],
        json!(true),
        "getRawBuffer 须拷出：写入返回缓冲区不得影响地形读取"
    );
    // 敌意探针两通道一致，且三条窃取路径全部失败（§6 不可达性负向验收）。
    assert_eq!(json_hostile, kkpv_hostile, "敌意环境行为两通道一致");
    let hostile = serde_json::from_str::<serde_json::Value>(
        kkpv_hostile["value"].as_str().expect("敌意探针返回字符串"),
    )?;
    assert_eq!(
        hostile["fakeDest"],
        json!("THROW:TypeError"),
        "伪 destination 须被真 set 的品牌校验拒绝（不得执行玩家代码）"
    );
    assert_eq!(
        hostile["replacedCtor"],
        json!("copy-2500"),
        "替换全局构造器后仍须返回真拷贝"
    );
    assert_eq!(
        hostile["replacedSet"],
        json!("filled:1:1"),
        "替换原型 set 后合法 destination 仍须被真 set 填充（E2N2 全墙字节，独立杀伤）"
    );
    assert_eq!(
        hostile["leakCount"],
        json!(0),
        "三条窃取路径都不得把内部地形数组交给玩家代码"
    );
    assert_eq!(hostile["terrainAfter"], json!(0), "敌意环境不残留影响");
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
        terrain: None,
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
    let public_bundle = SharedKkpvBundle::from_vec(
        kk_world::kkpv::kkpv_public_bundle_from_world_json(
            &json!({
                "time": 3,
                "rooms": { "W1N1": { "name": "W1N1", "terrain": vec![0u8; 2500] } },
            }),
            [0; 32],
            [0; 32],
        )
        .map_err(|e| anyhow::anyhow!("公开束转换失败：{e}"))?,
    );
    let envelope = json!({
        "time": 3,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "visibleRooms": ["W1N1"],
        "rooms": { "W1N1": { "name": "W1N1" } },
    });
    let private_bundle = SharedKkpvBundle::from_vec(bundle);
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    assert!(matches!(
        sb.begin_tick_kkpv(&envelope, &public_bundle, &private_bundle)?,
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
        let (public, private) = fixture_bundles(&world)?;
        assert_eq!(private.byte_len(), 4, "零对象世界是仅含零计数的私有束");
        assert!(public.byte_len() > 4, "公开地形束恒有块（全注册房间）");
        let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(matches!(
            sb.begin_tick_kkpv(&envelope, &public, &private)?,
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

/// 损坏束的 fail-loud：截断的私有束使注入失败并带回 KKPV 错误文本，
/// 不产生部分装配的世界。公开束的截断、"私块混入公开束/公开块混入
/// 私有束"的 flags 错配，以及公开块形状违约（缺地形段/携带对象/重复
/// 房名）与私有块携带地形段，全部点名失败——两条束的校验口径一致。
pub fn kkpv_truncated_bundle_fails_impl() -> anyhow::Result<()> {
    use kk_schema::kkpv::{KkpvBlockInput, KkpvLimits, KkpvObject, KkpvValue, encode_bundle};
    let world = kkpv_world();
    let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
    let (public, private) = fixture_bundles(&world)?;
    {
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(
            matches!(
                sb.begin_tick_kkpv(&envelope, &public, &private)?,
                TickPlan::Run { .. }
            ),
            "完整两束是注入对照组（必须成功）"
        );
        sb.end_tick()?;
        sb.settle_bill();
    }
    let expect_fail = |public_bytes: Vec<u8>,
                       private_bytes: Vec<u8>,
                       needle: &str,
                       why: &str|
     -> anyhow::Result<()> {
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        let err = sb
            .begin_tick_kkpv(
                &envelope,
                &SharedKkpvBundle::from_vec(public_bytes),
                &SharedKkpvBundle::from_vec(private_bytes),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains(needle),
            "{why}（期望点名「{needle}」，实得 {err:#}）"
        );
        Ok(())
    };
    // 截断私有束：从共享句柄无法取回字节，重新编码一份可截断的拷贝。
    let private_bytes = kk_world::kkpv::kkpv_bundle_from_world_json(&world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("夹具转换失败：{e}"))?;
    let truncated = &private_bytes[..private_bytes.len() - 1];
    expect_fail(
        kk_world::kkpv::kkpv_public_bundle_from_world_json(&world, [0; 32], [0; 32])
            .map_err(|e| anyhow::anyhow!("公开束转换失败：{e}"))?,
        truncated.to_vec(),
        "KKPV",
        "截断私有束必须注入失败",
    )?;
    // 截断公开束。
    let public_bytes = kk_world::kkpv::kkpv_public_bundle_from_world_json(&world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("公开束转换失败：{e}"))?;
    expect_fail(
        public_bytes[..public_bytes.len() - 1].to_vec(),
        private_bytes.clone(),
        "KKPV",
        "截断公开束必须注入失败",
    )?;
    // flags 错配双向点名。
    expect_fail(
        private_bytes.clone(),
        private_bytes.clone(),
        "公开束要求公开块",
        "私有块混入公开束必须失败",
    )?;
    expect_fail(
        public_bytes.clone(),
        public_bytes.clone(),
        "私有束要求私有块",
        "公开块混入私有束必须失败",
    )?;
    // 公开块形状违约：缺地形段 / 携带对象 / 重复房名 / 地形段长度违约。
    let public_block =
        |room: &str, terrain: Option<Vec<u8>>, objects: Vec<KkpvObject>| KkpvBlockInput {
            private: false,
            tick: 7,
            generation: 0,
            plan_hash: [0; 32],
            audience_hash: [0; 32],
            room_name: room.into(),
            terrain,
            objects,
        };
    expect_fail(
        encode_bundle(
            &[public_block("W1N1", None, Vec::new())],
            &KkpvLimits::default(),
        )?,
        private_bytes.clone(),
        "公开地形块缺地形段",
        "公开块缺地形段必须失败",
    )?;
    expect_fail(
        encode_bundle(
            &[public_block(
                "W1N1",
                Some(vec![0u8; 2500]),
                vec![KkpvObject {
                    id: 1,
                    type_key: "builtin:creep".into(),
                    fields: vec![("x".into(), KkpvValue::Uint(1))],
                }],
            )],
            &KkpvLimits::default(),
        )?,
        private_bytes.clone(),
        "公开地形块不带对象",
        "公开块携带对象必须失败",
    )?;
    expect_fail(
        encode_bundle(
            &[
                public_block("W1N1", Some(vec![0u8; 2500]), Vec::new()),
                public_block("W1N1", Some(vec![0u8; 2500]), Vec::new()),
            ],
            &KkpvLimits::default(),
        )?,
        private_bytes.clone(),
        "公开束房间名重复",
        "重复房名必须失败",
    )?;
    expect_fail(
        encode_bundle(
            &[public_block("W1N1", Some(vec![0u8; 64]), Vec::new())],
            &KkpvLimits::default(),
        )?,
        private_bytes.clone(),
        "地形段须为 2500 字节",
        "短地形段必须失败（不得静默按 plain 读出）",
    )?;
    // 私有块携带地形段（B.6 收紧的对称面）。
    let private_with_terrain = KkpvBlockInput {
        private: true,
        tick: 7,
        generation: 0,
        plan_hash: [0; 32],
        audience_hash: [0; 32],
        room_name: "W1N1".into(),
        terrain: Some(vec![0u8; 2500]),
        objects: Vec::new(),
    };
    expect_fail(
        public_bytes.clone(),
        encode_bundle(&[private_with_terrain], &KkpvLimits::default())?,
        "私有对象块不带地形段",
        "私有块携带地形段必须失败",
    )?;
    Ok(())
}

/// 同一公开地形束的同一份共享内存被两个线程上的隔离并发挂载与读取：
/// 跨线程共享直读的经验验证（B.6 的 §6 验收面）。两线程各自持有句柄
/// 克隆（Send 移动）；玩家侧在读地形前自旋消耗时间，受迫形成真实的
/// 并发读取窗口（栅栏只对齐派发时刻，自旋保证窗口重叠）。并发解码后
/// 地形值与单线程结果一致。
pub fn kkpv_shared_bundle_cross_thread_impl() -> anyhow::Result<()> {
    let world = kkpv_world();
    let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
    let (public, private) = fixture_bundles(&world)?;
    let code = r#"(function () {
  for (var spin = 0; spin < 2e7; spin++) { if (spin < 0) break; }
  var out = {};
  ['W1N1', 'E2N2', 'W2N2'].forEach(function (room) {
    var raw = new Room.Terrain(room).getRawBuffer();
    out[room] = raw.length + ':' + raw.reduce(function (a, b) { return a + b; }, 0);
  });
  return JSON.stringify(out);
})()"#;
    let expected = {
        let mut sb = Sandbox::new(SandboxConfig::default())?;
        assert!(matches!(
            sb.begin_tick_kkpv(&envelope, &public, &private)?,
            TickPlan::Run { .. }
        ));
        let out = sb.run_expr(code)?.clone();
        sb.end_tick()?;
        sb.settle_bill();
        out["value"].clone()
    };
    let private_bytes = kk_world::kkpv::kkpv_bundle_from_world_json(&world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("夹具转换失败：{e}"))?;
    let envelope_text = serde_json::to_string(&envelope)?;
    let epoch = kk_runner::RunEpoch {
        tick: 7,
        generation: 0,
    };
    // 两个线程共享同一份公开束内存（各自克隆句柄）；栅栏对齐并发窗口。
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let barrier = barrier.clone();
        let public = public.clone();
        let envelope_text = envelope_text.clone();
        let private_bytes = private_bytes.clone();
        let code = code.to_string();
        handles.push(std::thread::spawn(
            move || -> anyhow::Result<serde_json::Value> {
                let mut pool = PlayerThreads::new();
                pool.spawn_player(1, SandboxConfig::default())?;
                barrier.wait();
                let report = pool.run_player_kkpv(
                    1,
                    &serde_json::from_str(&envelope_text)?,
                    &public,
                    &private_bytes,
                    &code,
                    epoch,
                )?;
                pool.shutdown_all();
                Ok(report.expr["value"].clone())
            },
        ));
    }
    for handle in handles {
        let value = handle
            .join()
            .map_err(|_| anyhow::anyhow!("共享束线程 panic"))??;
        assert_eq!(value, expected, "并发共享挂载的地形读取须与单线程结果一致");
    }
    Ok(())
}
