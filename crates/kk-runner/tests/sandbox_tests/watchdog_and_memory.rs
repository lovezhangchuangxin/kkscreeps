//! 验证地形协议、模块加载、Memory 写回、预算与中断后复用。
//!
//! 场景由 sandbox_tests.rs 的唯一测试串行调用，避免进程级计数互相干扰；
//! 这里不添加独立 #[test]。

use super::*;

/// 验证紧凑地形 {rev,payload} 解码：同版本读取相同值，版本改变后读取新地形。
/// 载荷为手工构造的 2500 字节 base64；第二份载荷把首格从 wall 改为 plain。
pub(super) fn terrain_compact_protocol_form() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    let anchor = serde_json::json!([
        { "id": "c9", "type": "creep", "room": "W1N1", "x": 30, "y": 30, "name": "anchor",
          "owner": "p1", "body": ["move"], "store": { "energy": 0 } }
    ]);
    let w1 = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1",
            "terrain": { "rev": 1, "payload": format!("AQAA{}==", "A".repeat(3330)) } } },
        "objects": anchor
    });
    tick(&mut sb, &w1)?;
    let v = sb.run_expr("Game.map.getRoomTerrain('W1N1').get(0,0)")?;
    assert_eq!(v["value"], 1, "紧凑形态 byte0=wall 解码正确");
    let v = sb.run_expr("Game.map.getRoomTerrain('W1N1').get(1,0)")?;
    assert_eq!(v["value"], 0, "其余字节 plain");
    let v = sb.run_expr("Game.map.getTerrainAt(0,0,'W1N1')")?;
    assert_eq!(v["value"], "wall");
    let v = sb.run_expr(
        "(() => { const b = Game.map.getRoomTerrain('W1N1').getRawBuffer(); return b.length + ':' + b[0] + ':' + b[2499]; })()",
    )?;
    assert_eq!(
        v["value"], "2500:1:0",
        "getRawBuffer 拷 2500 字节（Uint8Array 形态）"
    );

    // 同 rev 第二 tick：缓存复用路径（值不变）；rev 递增 + 新载荷：必须重解码
    tick(&mut sb, &w1)?;
    let v = sb.run_expr("Game.map.getRoomTerrain('W1N1').get(0,0)")?;
    assert_eq!(v["value"], 1, "同 rev 复用解码产物");
    let w2 = serde_json::json!({
        "time": 2,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1",
            "terrain": { "rev": 2, "payload": format!("{}==", "A".repeat(3334)) } } },
        "objects": anchor
    });
    tick(&mut sb, &w2)?;
    let v = sb.run_expr("Game.map.getRoomTerrain('W1N1').get(0,0)")?;
    assert_eq!(v["value"], 0, "rev 递增后重解码（无陈旧命中）");
    let v = sb.run_expr("Game.map.getTerrainAt(0,0,'W1N1')")?;
    assert_eq!(v["value"], "plain");
    Ok(())
}

/// 验证 Terrain、look 地形项、缺省 controller 和 console 格式化。
/// 来源：上游 engine/src/game/rooms.js 与 console.js 的相应接口。
pub(super) fn terrain_look_and_console_semantics() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    let mut terrain = vec![0u32; 2500];
    terrain[0] = 1; // (0,0) wall
    terrain[10] = 2; // (10,0) swamp
    let w = serde_json::json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            // 视野锚点：官方无对象玩家看不到任何房（runtimeData.rooms 为空）
            { "id": "c9", "type": "creep", "room": "W1N1", "x": 30, "y": 30, "name": "anchor",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 } }
        ]
    });
    tick(&mut sb, &w)?;

    // room.controller：无 controller 房间投影 undefined（非 null）
    let v = sb.run_expr("typeof Game.rooms['W1N1'].controller")?;
    assert_eq!(v["value"], "undefined", "无 controller → undefined");

    // room.find(type, {filter})：官方经 _.filter 应用（rooms.js:648-654）——
    // 函数谓词与 lodash 对象谓词两形态（换带对象的世界验证）
    let w2 = serde_json::json!({
        "time": 2,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": terrain } },
        "objects": [
            { "id": "k1", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "owner": "p1" },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 300 } },
        ]
    });
    sb.begin_tick(&w2)?;
    sb.run_loop()?;
    sb.end_tick()?;
    let v = sb.run_expr("Game.rooms['W1N1'].find(FIND_STRUCTURES).length")?;
    assert_eq!(v["value"], 2, "无 filter 返回全部结构");
    let v = sb.run_expr(
        "Game.rooms['W1N1'].find(FIND_STRUCTURES, {filter: (s) => s.structureType === 'spawn'}).length",
    )?;
    assert_eq!(v["value"], 1, "函数谓词过滤生效");
    let v = sb.run_expr(
        "Game.rooms['W1N1'].find(FIND_STRUCTURES, {filter: {structureType: 'spawn'}})[0].name",
    )?;
    assert_eq!(v["value"], "Spawn1", "lodash 对象谓词过滤生效");

    // RoomPosition.findInRange（官方 rooms.js:1518-1543）：数字 type + Chebyshev
    // 半径过滤；数组 type 直传；不可达房抛错
    let v = sb.run_expr(
        "Game.rooms['W1N1'].getPositionAt(10,10).findInRange(FIND_STRUCTURES, 1).length",
    )?;
    assert_eq!(v["value"], 1, "半径 1 内只有 controller");
    let v = sb.run_expr(
        "Game.rooms['W1N1'].getPositionAt(10,10).findInRange(FIND_STRUCTURES, 15).map((s) => s.structureType)",
    )?;
    assert_eq!(
        v["value"],
        serde_json::json!(["controller", "spawn"]),
        "半径 15 覆盖两结构（find 序）"
    );
    let v = sb.run_expr(
        "(() => { try { Game.rooms['W1N1'].getPositionAt(1,1).findInRange.call({roomName: 'E9S9', x: 1, y: 1}, FIND_SOURCES, 5); return 'no'; } catch (e) { return e.message; } })()",
    )?;
    assert_eq!(v["value"], "Could not access room E9S9", "不可达房抛错");

    // Room.Terrain 静态挂法（rooms.js:1135）
    let v = sb.run_expr("typeof Room.Terrain")?;
    assert_eq!(v["value"], "function", "Room.Terrain 是静态属性");

    // Terrain.get：位码语义 wall=1/swamp=2/plain=0（rooms.js:1230-1233）
    let v = sb.run_expr("[Game.rooms['W1N1'].getTerrain().get(0,0), Game.rooms['W1N1'].getTerrain().get(10,0), Game.rooms['W1N1'].getTerrain().get(5,5)]")?;
    assert_eq!(v["value"], serde_json::json!([1, 2, 0]), "get 位码");

    // getRawBuffer：无参新建、有参填充返回原数组（rooms.js:1235-1239）
    let v = sb.run_expr("Game.rooms['W1N1'].getTerrain().getRawBuffer().length")?;
    assert_eq!(v["value"], 2500, "getRawBuffer 2500 字节");
    let v = sb.run_expr("(() => { const u = new Uint8Array(2500); const t = Game.map.getRoomTerrain('W1N1'); return [t.getRawBuffer(u) === u, u[0], u[10]]; })()")?;
    assert_eq!(
        v["value"],
        serde_json::json!([true, 1, 2]),
        "getRawBuffer 填充给定数组（map.js:262-264 getRoomTerrain 同一 Terrain）"
    );

    // 无地形数据的房间：构造即抛错（rooms.js:1226-1228 官方错误文案）
    let v = sb.run_expr("(() => { try { new Room.Terrain('E9S9'); return 'no'; } catch (e) { return e.message; } })()")?;
    assert_eq!(v["value"], "Could not access room E9S9", "未知房间报错");

    // RoomPosition.lookFor(LOOK_TERRAIN)：单词数组（rooms.js:1601-1610）
    let v = sb.run_expr("JSON.stringify([new RoomPosition(0,0,'W1N1').lookFor(LOOK_TERRAIN), new RoomPosition(10,0,'W1N1').lookFor(LOOK_TERRAIN), new RoomPosition(5,5,'W1N1').lookFor(LOOK_TERRAIN)])")?;
    assert_eq!(
        v["value"], r#"[["wall"],["swamp"],["plain"]]"#,
        "lookFor(LOOK_TERRAIN) 词汇表"
    );

    // room.lookForAt(LOOK_TERRAIN) 是单词数组（rooms.js:798-806 → :681 return [result]）；
    // room.lookAt 的 terrain 项是对象形态（rooms.js:663-681 带 outArray 分支）
    let v = sb.run_expr("JSON.stringify(Game.rooms['W1N1'].lookForAt(LOOK_TERRAIN, 0, 0))")?;
    assert_eq!(v["value"], r#"["wall"]"#);
    let v =
        sb.run_expr("JSON.stringify(Game.rooms['W1N1'].lookAt(new RoomPosition(10,0,'W1N1')))")?;
    assert_eq!(v["value"], r#"[{"type":"terrain","terrain":"swamp"}]"#);

    // console 官方格式化（console.js:16-22 逐参映射）：真值走 toString、
    // undefined → 'undefined'、falsy 走 JSON.stringify（NaN → 'null'）
    sb.run_expr(
        "console.log(null, 0, undefined, NaN, '', 'x', [1,2], new RoomPosition(1,2,'W1N1'))",
    )?;
    let lines = sb.drain_console();
    assert_eq!(
        lines[0].text, "null 0 undefined null \"\" x 1,2 [room W1N1 pos 1,2]",
        "官方 console 格式化: {:?}",
        lines
    );
    Ok(())
}

#[path = "watchdog_and_memory/memory_and_budget.rs"]
mod memory_and_budget;
pub(super) use memory_and_budget::*;

#[path = "watchdog_and_memory/modules_and_api.rs"]
mod modules_and_api;
pub(super) use modules_and_api::*;
