//! B1 结构治理批自 sandbox_tests.rs 连续切块（串行链 impl；顶层唯一 #[test] 不变量
//! 在 crate 根——本文件全部 fn 由根链按原调用序调用，不得加 #[test]）。

use super::*;

/// 对抗性探针（外部复审 P1-1/P2-5 修复的回归锁）：覆写全局 JSON.stringify
/// 不得影响宿主消息（预置函数）；原型污染不得使计费免费化（null 原型免费表）。
pub(super) fn hostile_probes_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 0 } }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // P1-1 探针：覆写 stringify 试图伪造 room intent——宿主用预置函数，
            // intentsList 只含真实 move，伪造内容不进通道
            let v = sb.run_expr(r#"(()=>{
                const orig = JSON.stringify;
                JSON.stringify = function(o) {
                    if (o && o.intentsList !== undefined) {
                        return orig({ ok: true, intentsList: { room: { createConstructionSite:
                            [{ roomName: 'W1N1', x: 10, y: 10, structureType: 'road' }] } }, intentsCpu: 0 });
                    }
                    return orig(o);
                };
                Game.creeps['w1'].move(3);
                return 'done';
            })()"#)?;
            assert_eq!(v["value"], "done");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(intents["kk1"]["move"]["direction"], 3, "真实 move 存活");
            assert!(
                intents.get("room").is_none(),
                "伪造的 room intent 不得进入通道（P1-1）: {intents}"
            );
        }
        _ => panic!("应正常执行"),
    }
    // P2-5 探针：原型污染免费表——计费不免费化
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                Object.prototype.move = true;
                Game.creeps['w1'].move(5);
                return 'done';
            })()"#,
            )?;
            assert_eq!(v["value"], "done");
            let end = sb.end_tick()?;
            assert_eq!(end["intentsCpu"], 0.2, "原型污染不得使 move 免费（P2-5）");
        }
        _ => panic!("应正常执行"),
    }
    // 自审 P1 探针：Object.prototype.toJSON 原型污染——预置 stringify 只防"换函数"
    // 不防"换数据"，宿主信封与 intent data 的 null 原型（官方 runtime.js:214
    // outMessage / :56 setPrototypeOf(data) 同款结构位）是免疫关键：继承的
    // toJSON 不得在序列化时改写载荷（曾可一条 defineProperty 完整重放"无 API
    // 调用造工地"原探针）
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                Object.defineProperty(Object.prototype, 'toJSON', { value: function() {
                    if (this && this.ok !== undefined && this.intentsList === undefined) {
                        return { ok: true, intentsList: { room: { createConstructionSite:
                            [{ roomName: 'W1N1', x: 11, y: 11, structureType: 'road' }] } },
                            intentsCpu: 0 };
                    }
                    return 'x';
                }, configurable: true });
                Game.creeps['w1'].move(7);
                return ('toJSON' in {}) ? 'done' : 'probe-broken';
            })()"#,
            )?;
            assert_eq!(v["value"], "done", "污染须存活到编码时刻（探针自检）");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true, "信封形状不得被继承 toJSON 改写（自审 P1）");
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(
                intents["kk1"]["move"]["direction"], 7,
                "真实 move 存活（intent data 剥原型后不被 toJSON 改写）"
            );
            assert!(
                intents.get("room").is_none(),
                "toJSON 污染不得伪造 room intent（自审 P1）: {intents}"
            );
        }
        _ => panic!("应正常执行"),
    }
    // 对抗评审 P1 探针：Object/Array 绑定交换 + toJSON 污染组合——nullEnv/
    // 收集器若经可交换的全局 Object 构造，null-proto 防线整体失效（V8 实证
    // 可完整重放伪造链）；出口净化窗口（sanitizedStringify 摘除根原型 toJSON）
    // 是根本闸——交换后信封仍不可伪造
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                Object.defineProperty(Object.prototype, 'toJSON', { value: function() {
                    if (this && this.ok !== undefined && this.intentsList === undefined) {
                        return { ok: true, intentsList: { room: { createConstructionSite:
                            [{ roomName: 'W1N1', x: 13, y: 13, structureType: 'road' }] } },
                            intentsCpu: 0 };
                    }
                    return 'x';
                }, configurable: true });
                globalThis.Object = { assign: (t, s) => ({ ...s }), create: () => ({}),
                    setPrototypeOf: () => {}, defineProperty: () => {}, entries: (o) => [],
                    keys: () => [], prototype: Object.prototype };
                Game.creeps['w1'].move(8);
                return 'done';
            })()"#,
            )?;
            assert_eq!(v["value"], "done");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true, "Object 交换不得破坏宿主信封（对抗 P1）");
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(intents["kk1"]["move"]["direction"], 8, "真实 move 存活");
            assert!(
                intents.get("room").is_none(),
                "Object 交换 + toJSON 污染不得伪造 room intent（对抗 P1）: {intents}"
            );
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A4-i Store 类官方全语义（engine/src/game/store.js:19-101 逐条）：
/// 四形态（per-resource 商店 spawn/extension、数值容量 storage/creep）的
/// getCapacity/getUsedCapacity/getFreeCapacity 全分岔 + Proxy 缺省 0 +
/// toString/instanceof/stringify 形态。世界 fixture 手写 world_json 的投影
/// 键（storeCapacityResource/storeCapacity——typed 投影同形）。
pub(super) fn store_class_semantics_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 5,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "energyAvailable": 200, "energyCapacityAvailable": 250 } },
        "objects": [
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 150 },
              "storeCapacityResource": { "energy": 300 }, "off": false, "spawning": null },
            { "id": "e1", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 21, "y": 20, "owner": "p1", "store": { "energy": 50 },
              "storeCapacityResource": { "energy": 50 }, "off": false },
            { "id": "st1", "type": "structure", "structureType": "storage", "room": "W1N1",
              "x": 22, "y": 20, "owner": "p1", "store": { "energy": 500 }, "storeCapacity": 1000000 },
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 20, "y": 21, "name": "carrier",
              "owner": "p1", "body": ["carry", "carry", "move"], "store": { "energy": 30 },
              "storeCapacity": 100, "ticksToLive": 1000, "spawning": false }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // spawn（per-resource 商店）：store.js 全分岔——getCapacity() null、
            // getCapacity('energy') 300、非 energy 资源 null；getUsedCapacity()
            // null（capacityForResource(o,undefined)===0）、getUsedCapacity('H')
            // null（容量为 0 的资源）、getFreeCapacity() null、
            // getFreeCapacity('energy') = 300−150（无 storeCapacity 分支）
            let v = sb.run_expr(
                r#"(()=>{
                const s = Game.spawns['Spawn1'].store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getCapacity('H'),
                        s.getUsedCapacity(), s.getUsedCapacity('energy'), s.getUsedCapacity('H'),
                        s.getFreeCapacity(), s.getFreeCapacity('energy')];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], serde_json::Value::Null, "spawn getCapacity() → null");
            assert_eq!(v[1], 300, "spawn getCapacity('energy') → 300");
            assert_eq!(
                v[2],
                serde_json::Value::Null,
                "spawn getCapacity('H') → null"
            );
            assert_eq!(
                v[3],
                serde_json::Value::Null,
                "spawn getUsedCapacity() → null"
            );
            assert_eq!(v[4], 150, "spawn getUsedCapacity('energy')");
            assert_eq!(
                v[5],
                serde_json::Value::Null,
                "spawn getUsedCapacity('H') → null（容量 0 资源）"
            );
            assert_eq!(
                v[6],
                serde_json::Value::Null,
                "spawn getFreeCapacity() → null"
            );
            assert_eq!(v[7], 150, "spawn getFreeCapacity('energy') = 300−150");

            // extension（RCL 归一的 per-resource 容量 50）
            let v = sb.run_expr(
                r#"(()=>{
                const s = Game.getObjectById('e1').store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getFreeCapacity('energy')];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(
                v[0],
                serde_json::Value::Null,
                "extension getCapacity() → null"
            );
            assert_eq!(v[1], 50, "extension getCapacity('energy') → 50");
            assert_eq!(v[2], 0, "extension getFreeCapacity('energy') = 50−50");

            // storage（数值容量）：_sum 惰性缓存路径
            let v = sb.run_expr(r#"(()=>{
                const s = Game.getObjectById('st1').store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getUsedCapacity(),
                        s.getUsedCapacity('energy'), s.getFreeCapacity(), s.getFreeCapacity('energy')];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 1000000, "storage getCapacity()");
            assert_eq!(v[1], 1000000, "storage getCapacity('energy')（容量共享）");
            assert_eq!(v[2], 500, "storage getUsedCapacity()（_sum）");
            assert_eq!(v[3], 500, "storage getUsedCapacity('energy')");
            assert_eq!(v[4], 999500, "storage getFreeCapacity() = 1000000−500");
            assert_eq!(v[5], 999500, "storage getFreeCapacity('energy')");

            // creep（carry 派生容量 100）
            let v = sb.run_expr(
                r#"(()=>{
                const c = Game.creeps['carrier'];
                const s = c.store;
                return [s.getCapacity(), s.getCapacity('energy'), s.getUsedCapacity(),
                        s.getFreeCapacity(), c.carryCapacity, c.carry.getCapacity()];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 100, "creep store.getCapacity()");
            assert_eq!(v[1], 100, "creep store.getCapacity('energy')");
            assert_eq!(v[2], 30, "creep store.getUsedCapacity()");
            assert_eq!(v[3], 70, "creep store.getFreeCapacity()");
            assert_eq!(
                v[4], 100,
                "creep carryCapacity（官方 o.storeCapacity 直读）"
            );
            assert_eq!(v[5], 100, "creep carry 同 store 形态");

            // Proxy 语义（store.js:87-96）：RESOURCES_ALL 缺省 0、未知名
            // undefined、toString '[store]'、instanceof、可枚举面只有正量资源
            let v = sb.run_expr(
                r#"(()=>{
                const s = Game.spawns['Spawn1'].store;
                return [s.energy, s.H, s.void_unknown_name === undefined, String(s),
                        s instanceof Store, JSON.stringify(s),
                        JSON.stringify(Object.keys(s))];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 150, "正量资源直读");
            assert_eq!(v[1], 0, "RESOURCES_ALL 资源缺省 0（非 undefined）");
            assert_eq!(v[2], true, "未知属性 undefined");
            assert_eq!(v[3], "[store]", "toString（官方 store.js:80-84）");
            assert_eq!(v[4], true, "instanceof Store（构造器挂 globals）");
            assert_eq!(
                v[5], r#"{"energy":150}"#,
                "stringify 只含正量资源（三函数非 enumerable）"
            );
            assert_eq!(v[6], r#"["energy"]"#, "Object.keys 只有正量资源");
            let _ = sb.end_tick()?;
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A4-i spawn 方法族（structures.js:1063-1334）：spawnCreep 全预检序 + 同 tick
/// 幽灵壳 + Memory 初始化 + intent 载荷；Spawning getter/setDirections/cancel；
/// renewCreep/recycleCreep 预检码与 intent。
pub(super) fn spawn_family_api_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 5,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "energyAvailable": 200, "energyCapacityAvailable": 250 } },
        "objects": [
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 150 },
              "storeCapacityResource": { "energy": 300 }, "off": false, "spawning": null },
            { "id": "s2", "type": "spawn", "room": "W1N1", "x": 23, "y": 20, "name": "Spawn2",
              "owner": "p1", "store": { "energy": 300 },
              "storeCapacityResource": { "energy": 300 }, "off": false,
              "spawning": { "name": "child", "needTime": 6, "spawnTime": 11, "directions": [3, 5] } },
            { "id": "s3", "type": "spawn", "room": "W1N1", "x": 24, "y": 20, "name": "EnemySpawn",
              "owner": "p2", "store": { "energy": 300 },
              "storeCapacityResource": { "energy": 300 }, "off": false, "spawning": null },
            { "id": "s4", "type": "spawn", "room": "W1N1", "x": 25, "y": 20, "name": "OffSpawn",
              "owner": "p1", "store": { "energy": 300 },
              "storeCapacityResource": { "energy": 300 }, "off": true, "spawning": null },
            { "id": "e1", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 21, "y": 20, "owner": "p1", "store": { "energy": 50 },
              "storeCapacityResource": { "energy": 50 }, "off": false },
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 20, "y": 21, "name": "carrier",
              "owner": "p1", "body": ["carry", "carry", "move"], "store": { "energy": 30 },
              "storeCapacity": 100, "ticksToLive": 1000, "spawning": false },
            { "id": "c2", "type": "creep", "room": "W1N1", "x": 40, "y": 40, "name": "far",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 },
              "storeCapacity": 50, "ticksToLive": 1000, "spawning": false },
            { "id": "c3", "type": "creep", "room": "W1N1", "x": 20, "y": 19, "name": "claimer",
              "owner": "p1", "body": ["claim"], "store": { "energy": 0 },
              "storeCapacity": 0, "ticksToLive": 500, "spawning": false }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // ---- spawnCreep 预检族（错误码逐条对照 :1065-1116）----
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                return [
                    sp.spawnCreep([WORK, WORK, WORK], 'W1'),                       // -6 能量不足（300>200）
                    sp.spawnCreep(['fly'], 'W1'),                                   // -10 非法部件
                    sp.spawnCrerep_typo_guard === undefined ? sp.spawnCreep([MOVE], 'W1', null) : -99, // -10 options 非 object
                    sp.spawnCreep([MOVE], 'W1', { directions: [0] }),               // -10 越界方向
                    sp.spawnCreep([MOVE], 'W1', { directions: [] }),                // -10 空方向
                    sp.spawnCreep([MOVE], 'W1', { directions: 5 }),                 // -10 非数组方向
                    sp.spawnCreep([MOVE], 'x'.repeat(101)),                         // -10 名字超 100
                    sp.spawnCreep([MOVE], 'W1', { dryRun: true }),                  // 0 dryRun（不消费名字）
                    sp.spawnCreep([MOVE], 'W1')                                     // 0 正常（同 tick 前面全拒）
                ];
            })()"#)?;
            // 上面的 typo 守卫恒 false → 走 spawnCreep([MOVE], 'W1', null) 分支
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], -6, "ERR_NOT_ENOUGH_ENERGY");
            assert_eq!(v[1], -10, "非法部件 ERR_INVALID_ARGS");
            assert_eq!(
                v[2], -10,
                "options=null ERR_INVALID_ARGS（默认参只兜 undefined）"
            );
            assert_eq!(v[3], -10, "directions 越界");
            assert_eq!(v[4], -10, "directions 空");
            assert_eq!(v[5], -10, "directions 非数组");
            assert_eq!(v[6], -10, "名字 > 100 字符");
            assert_eq!(v[7], 0, "dryRun 返回 OK");
            assert_eq!(v[8], 0, "dryRun 后同名可用（名单未消费）");

            // 同 tick 幽灵壳（官方 :1132-1213 十三字段逐项）+ Memory 初始化
            let v = sb.run_expr(
                r#"(()=>{
                const g = Game.creeps['W1'];
                const m = Memory.creeps['W1'];
                return [g instanceof Creep, g.spawning, g.my, g.name, g.ticksToLive,
                        g.body.length, g.body[0].type, g.body[0].hits, g.carryCapacity,
                        g.carry.energy, g.store.energy, g.store.getCapacity(),
                        g.hits, g.hitsMax, g.saying === undefined, g.id === undefined,
                        g.pos.x, g.pos.y, g.pos.roomName, g.room === Game.rooms['W1N1'],
                        JSON.stringify(m)];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], true, "instanceof Creep");
            assert_eq!(v[1], true, "spawning: true");
            assert_eq!(v[2], true, "my: true");
            assert_eq!(v[3], "W1");
            assert_eq!(v[4], 1500, "ticksToLive = CREEP_LIFE_TIME");
            assert_eq!(v[5], 1);
            assert_eq!(v[6], "move", "body 映射 {{type, hits:100}}");
            assert_eq!(v[7], 100);
            assert_eq!(v[8], 0, "carryCapacity：无 CARRY");
            assert_eq!(v[9], 0, "carry.energy");
            assert_eq!(v[10], 0, "store.energy（Proxy 缺省 0）");
            assert_eq!(v[11], serde_json::Value::Null, "幽灵壳 store 无容量 → null");
            assert_eq!(v[12], 100, "hits = body.length×100");
            assert_eq!(v[13], 100, "hitsMax");
            assert_eq!(v[14], true, "saying undefined");
            assert_eq!(v[15], true, "官方幽灵壳无 id");
            assert_eq!(
                v[16..19],
                [
                    serde_json::json!(20),
                    serde_json::json!(20),
                    serde_json::json!("W1N1")
                ]
            );
            assert_eq!(v[19], true, "room 是当前 Game.rooms 的实例");
            assert_eq!(v[20], "{}", "Memory.creeps['W1'] 初始化为空对象");

            // 同 tick 同名拒（createdCreepNames + Game.creeps 双通道）
            let v = sb.run_expr(r#"Game.spawns['Spawn1'].spawnCreep([MOVE], 'W1')"#)?;
            assert_eq!(v["value"], -3, "ERR_NAME_EXISTS");

            // options.memory 落 Memory.creeps
            let v = sb.run_expr(
                r#"Game.spawns['Spawn1'].spawnCreep([MOVE], 'Mem1', { memory: { role: 'harvester' } })"#,
            )?;
            assert_eq!(v["value"], 0);
            let v = sb.run_expr(r#"JSON.stringify(Memory.creeps['Mem1'])"#)?;
            assert_eq!(v["value"], r#"{"role":"harvester"}"#);

            // energyStructures 显式模式（:1053-1061 无 owner 过滤 + 空列表零能量）
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                const ext = Game.getObjectById('e1');
                return [
                    sp.spawnCreep([MOVE], 'W3', { energyStructures: [ext] }),   // 0（ext 50 ≥ 50）
                    sp.spawnCreep([MOVE], 'W4', { energyStructures: [] }),      // -6（显式空列表 = 零可用）
                    sp.spawnCreep([MOVE], 'W5', { energyStructures: [Game.creeps['carrier']] }) // -6（非 spawn/extension 计 0）
                ];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 0, "显式列表按列表计能量");
            assert_eq!(v[1], -6, "空列表是显式零能量模式（非缺省全房）");
            assert_eq!(v[2], -6, "非 spawn/extension 的列表元素计 0");

            // 敌方 spawn：NOT_OWNER（预检 ⑥ 在 energy 之前）
            let v = sb.run_expr(r#"Game.getObjectById('s3').spawnCreep([MOVE], 'Enemy1')"#)?;
            assert_eq!(v["value"], -1, "ERR_NOT_OWNER");
            // off 的 spawn：ERR_RCL_NOT_ENOUGH（⑧ 显式回查）
            let v = sb.run_expr(r#"Game.spawns['OffSpawn'].spawnCreep([MOVE], 'Off1')"#)?;
            assert_eq!(v["value"], -14, "ERR_RCL_NOT_ENOUGH（off）");
            // 孵化中的 spawn：ERR_BUSY（⑦ 显式回查）
            let v = sb.run_expr(r#"Game.spawns['Spawn2'].spawnCreep([MOVE], 'Busy1')"#)?;
            assert_eq!(v["value"], -4, "ERR_BUSY（spawning）");

            // ---- Spawning（structures.js:1301-1334）----
            let v = sb.run_expr(
                r#"(()=>{
                const sp = Game.spawns['Spawn2'].spawning;
                return [sp instanceof StructureSpawn.Spawning, sp.name, sp.needTime,
                        sp.remainingTime, sp.directions[0], sp.directions[1],
                        sp.spawn === Game.spawns['Spawn2'],
                        JSON.stringify(sp)];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], true, "instanceof StructureSpawn.Spawning");
            assert_eq!(v[1], "child");
            assert_eq!(v[2], 6, "needTime");
            assert_eq!(v[3], 6, "remainingTime = spawnTime(11) − time(5)");
            assert_eq!(v[4], 3);
            assert_eq!(v[5], 5);
            assert_eq!(v[6], true, "spawn 引用是当前壳（非 enumerable 定格）");
            assert_eq!(
                v[7],
                r#"{"name":"child","needTime":6,"remainingTime":6,"directions":[3,5]}"#
            );
            let v = sb.run_expr(r#"Game.spawns['Spawn1'].spawning === null"#)?;
            assert_eq!(v["value"], true, "无任务读 null");

            // setDirections / cancel（含数字化去重与越界拒）
            let v = sb.run_expr(
                r#"(()=>{
                const sp = Game.spawns['Spawn2'].spawning;
                return [sp.setDirections([2, '2', 3]), sp.setDirections([0]), sp.setDirections([]),
                        sp.setDirections(5), sp.cancel(),
                        Game.getObjectById('s3').spawning === null];
            })()"#,
            )?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], 0, "合法组（数字化去重 [2,3]）");
            assert_eq!(v[1], -10, "越界拒");
            assert_eq!(v[2], -10, "空拒");
            assert_eq!(v[3], -10, "非数组拒");
            assert_eq!(v[4], 0, "cancel OK");
            assert_eq!(v[5], true);

            // ---- renewCreep 预检码（:1237-1267）----
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                const near = Game.creeps['carrier'];
                return [
                    Game.spawns['Spawn2'].renewCreep(near),   // -4 busy（spawning 最先）
                    sp.renewCreep(near),                       // 0（cost=ceil(1.2·150/3/3)=20 ≤ 200；1000+200 ≤ 1500）
                    sp.renewCreep(Game.creeps['claimer']),    // -7 CLAIM 部件
                    sp.renewCreep(Game.creeps['far']),        // -9 非邻接
                    sp.renewCreep(null),                      // -7 空 target
                    Game.getObjectById('s3').renewCreep(near),// -1 敌方 spawn
                    Game.spawns['OffSpawn'].renewCreep(near)  // -14 off
                ];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], -4, "ERR_BUSY");
            assert_eq!(v[1], 0, "正常续命（intent 记录）");
            assert_eq!(v[2], -7, "CLAIM 部件 ERR_INVALID_TARGET");
            assert_eq!(v[3], -9, "ERR_NOT_IN_RANGE");
            assert_eq!(v[4], -7, "空 target ERR_INVALID_TARGET");
            assert_eq!(v[5], -1, "敌方 spawn ERR_NOT_OWNER");
            assert_eq!(v[6], -14, "off spawn ERR_RCL_NOT_ENOUGH");

            // ---- recycleCreep 预检码（:1269-1290——my 最先、无 CLAIM 条件）----
            let v = sb.run_expr(r#"(()=>{
                const sp = Game.spawns['Spawn1'];
                const near = Game.creeps['claimer'];
                return [
                    Game.getObjectById('s3').recycleCreep(near), // -1 my 最先
                    sp.recycleCreep(Game.creeps['far']),          // -9 非邻接
                    sp.recycleCreep(near),                        // 0（CLAIM 可回收——与 renew 的差异面）
                    sp.recycleCreep(null)                         // -7
                ];
            })()"#)?;
            let v = v["value"].as_array().expect("数组");
            assert_eq!(v[0], -1, "ERR_NOT_OWNER 最先");
            assert_eq!(v[1], -9, "ERR_NOT_IN_RANGE");
            assert_eq!(v[2], 0, "CLAIM creep 可回收");
            assert_eq!(v[3], -7, "ERR_INVALID_TARGET");

            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            // intent 载荷形态：同名覆盖 last-wins（W1 系全拒后只有最后成功的
            // createCreep 留存）+ renewCreep/recycleCreep/setSpawnDirections/
            // cancelSpawning 白名单字段
            let intents = sb.take_intents().expect("intentsList 带出");
            let create = &intents["s1"]["createCreep"];
            assert_eq!(
                create["name"], "W3",
                "last-wins：W1→Mem1→W3 逐次覆盖（错误分支不记录）"
            );
            assert_eq!(create["body"], serde_json::json!(["move"]));
            assert_eq!(
                create["energyStructures"],
                serde_json::json!(["e1"]),
                "显式列表的 uniq pluck 形态"
            );
            assert!(
                create.get("directions").is_none(),
                "未传 directions 缺键（undefined 不入 stringify）"
            );
            assert_eq!(intents["s1"]["renewCreep"]["id"], "c1");
            assert_eq!(intents["s1"]["recycleCreep"]["id"], "c3");
            assert_eq!(
                intents["s2"]["setSpawnDirections"]["directions"],
                serde_json::json!([2, 3])
            );
            assert_eq!(intents["s2"]["cancelSpawning"], serde_json::json!({}));
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A4-ii 资源族四方法（transfer/withdraw/drop/pickup）prelude 换真的预检返回码
/// 矩阵——官方 creeps.js:404-593 的预检序逐条锁值（预检顺序即断言顺序）。
/// 世界：rcl3 房（extension 容量 50）；w1 两 carry（容量 100 存 80）为 actor。
/// A3c per-viewer 元信息：Game.gcl 官方点数正变换（game.js:130-163——
/// level=floor(pow(gcl/1e6, 1/2.4))+1、progress=点数−前级基数、progressTotal
/// 浮点不取整）与 Game.cpu 预算真值（begin_tick 注入点覆写——自定义配置下
/// limit/tickLimit=min(maxPerTick, limit+bucket)/bucket=初桶）。
pub(super) fn viewer_meta_semantics_impl() -> anyhow::Result<()> {
    let mk_world = |gcl: i64| {
        json!({
            "time": 1,
            "user": "p1",
            "gcl": gcl,
            "rooms": { "W1N1": { "name": "W1N1" } },
            "objects": [],
        })
    };
    // 0 点：1 级、progress 0、progressTotal = 1^2.4×1e6 − 0 = 1e6
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    sb.begin_tick(&mk_world(0))?;
    let v =
        sb.run_expr("[Game.gcl.level, Game.gcl.progress, Game.gcl.progressTotal, Game.gpl.level]")?;
    let v = v["value"].as_array().expect("数组");
    assert_eq!(v[0], 1, "0 点 → 1 级");
    assert_eq!(v[1], 0, "0 点 → progress 0");
    assert_eq!(v[2], 1_000_000, "1 级 progressTotal = 1e6");
    assert_eq!(v[3], 0, "gpl 桩不随本批变化");

    // 1e6 点：正跨 2 级边界（pow(1, 1/2.4)=1 → floor+1=2）、progress 归 0；
    // 1.5e6：2 级 progress 500000
    sb.begin_tick(&mk_world(1_000_000))?;
    let v = sb.run_expr("[Game.gcl.level, Game.gcl.progress, Game.gcl.progressTotal]")?;
    let v = v["value"].as_array().expect("数组");
    assert_eq!(v[0], 2, "1e6 点 → 2 级");
    assert_eq!(v[1], 0, "恰好跨级 → progress 0");
    let expected_total = 2.0_f64.powf(2.4) * 1e6 - 1e6;
    let got = v[2].as_f64().expect("浮点");
    assert!(
        (got - expected_total).abs() < 1e-6,
        "2 级 progressTotal 浮点不取整: {got} vs {expected_total}"
    );

    sb.begin_tick(&mk_world(1_500_000))?;
    let v = sb.run_expr("[Game.gcl.level, Game.gcl.progress]")?;
    let v = v["value"].as_array().expect("数组");
    assert_eq!(v[0], 2, "1.5e6 点仍在 2 级");
    assert_eq!(v[1], 500_000, "progress = 点数 − 2 级基数");

    // Game.cpu 预算真值：自定义配置（limit 10、初桶 30）+ 世界无 cpu 键——
    // begin_tick 注入点创建并覆写（tickLimit = min(500, 10+30) = 40）
    let cfg = SandboxConfig {
        cpu_limit: 10.0,
        initial_bucket: 30.0,
        ..SandboxConfig::default()
    };
    let mut sb2 = Sandbox::new(cfg)?;
    sb2.begin_tick(&mk_world(0))?;
    let v = sb2.run_expr(
        "[Game.cpu.limit, Game.cpu.tickLimit, Game.cpu.bucket, Game.cpu.shardLimits.shard0]",
    )?;
    let v = v["value"].as_array().expect("数组");
    assert_eq!(v[0], 10, "limit = 配置真值");
    assert_eq!(v[1], 40, "tickLimit = min(maxPerTick, limit+bucket)");
    assert_eq!(v[2], 30, "bucket = tick 初桶值");
    assert_eq!(v[3], 10, "shardLimits 跟随 limit");
    Ok(())
}

/// A3c 清偿（评审 3 P2-1）：claimController 预检 GCL 门 gclPoints 直读的
/// 行为级锁——gcl 0+已占 1 房 → ERR_GCL_NOT_ENOUGH；gcl 1e6（=calcNeededGcl(2)
/// 的 2 房需求点）→ 放行记 intent（官方 creeps.js:849-851 读
/// runtimeData.user.gcl；本批前硬编码 Game.gcl 使该门恒 0 点）。
/// A5-lite（upgradeController 换真）：transfer(控制器, 能量) 重定向正路径
/// ——WORK+能量 creep 邻接自家 controller，十门全过返回 OK。
pub(super) fn redirect_upgrade_positive_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "k1", "type": "structure", "structureType": "controller",
              "room": "W1N1", "x": 10, "y": 10, "level": 1, "owner": "p1" },
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 10, "y": 11,
              "name": "w1", "owner": "p1", "body": ["work", "carry"],
              "store": { "energy": 30 }, "spawning": false },
        ],
    });
    sb.begin_tick(&w)?;
    let v =
        sb.run_expr("Game.creeps['w1'].transfer(Game.rooms['W1N1'].controller, RESOURCE_ENERGY)")?;
    assert_eq!(v["value"], 0, "重定向十门全过 → OK");
    // 直调同路径
    let v = sb.run_expr("Game.creeps['w1'].upgradeController(Game.rooms['W1N1'].controller)")?;
    assert_eq!(v["value"], 0, "直调 upgradeController → OK");
    Ok(())
}

pub(super) fn claim_gate_gcl_behavior_impl() -> anyhow::Result<()> {
    let mk_world = |gcl: i64| {
        json!({
            "time": 1,
            "user": "p1",
            "gcl": gcl,
            "rooms": { "W1N1": { "name": "W1N1" }, "W2N1": { "name": "W2N1" } },
            "objects": [
                { "id": "ctl1", "type": "structure", "structureType": "controller",
                  "room": "W1N1", "x": 1, "y": 1, "owner": "p1", "level": 1 },
                { "id": "ctl2", "type": "structure", "structureType": "controller",
                  "room": "W2N1", "x": 1, "y": 1, "level": 0 },
                { "id": "ck1", "type": "creep", "room": "W2N1", "x": 1, "y": 2,
                  "name": "claimer", "owner": "p1", "body": ["claim", "move"], "spawning": false },
            ],
        })
    };
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    sb.begin_tick(&mk_world(0))?;
    let v = sb.run_expr("Game.creeps['claimer'].claimController(Game.rooms['W2N1'].controller)")?;
    assert_eq!(v["value"], -15, "gcl 0 + 已占 1 房 → ERR_GCL_NOT_ENOUGH");
    sb.begin_tick(&mk_world(1_000_000))?;
    let v = sb.run_expr("Game.creeps['claimer'].claimController(Game.rooms['W2N1'].controller)")?;
    assert_eq!(v["value"], 0, "gcl 1e6 = calcNeededGcl(2) → 放行");
    Ok(())
}
