//! PathFinder.search / findPath / moveTo 原生搜索接线的玩家面形态。
//!
//! 场景由 sandbox_tests.rs 的唯一测试串行调用（进程级地形表与沙箱计数）；
//! 这里不添加独立 #[test]。两通道等价与墙环边界在此族锁定。

use super::*;

/// 带一道纵墙的 1×N 世界地形（数组形态，宿主夹具路径）。
fn terrain_with_wall(wall_x: usize) -> Vec<u8> {
    let mut cells = vec![0u8; 2500];
    for y in 0..50 {
        cells[y * 50 + wall_x] = 1; // TERRAIN_MASK_WALL
    }
    cells
}

fn pathfinder_world() -> serde_json::Value {
    json!({
        "time": 7,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": {
            "W1N1": { "name": "W1N1", "terrain": terrain_with_wall(10) },
            "W2N1": { "name": "W2N1", "terrain": vec![0u8; 2500] },
        },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 5, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 50 } },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 4, "y": 25, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 300 } },
        ]
    })
}

/// 核心断言族：避障路径、结果形状、range 语义、空目标早退、跨房路径、
/// findPath 步进形态与 moveTo 首步 intent。
pub(super) fn pathfinder_search_family_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    tick(&mut sb, &pathfinder_world())?;

    // 避障：x=10 整列墙（0 与 49 边界不可通行除外——本世界单房无通道时
    // 官方全域可绕行；此处断言 incomplete + 部分路径止于墙前）。
    let v = sb.run_expr(
        r#"(() => {
            const r = PathFinder.search(
                new RoomPosition(5, 25, 'W1N1'),
                { pos: new RoomPosition(30, 25, 'W1N1'), range: 0 },
                { maxRooms: 1 });
            const last = r.path[r.path.length - 1];
            return {
                incomplete: r.incomplete,
                opsType: typeof r.ops,
                costType: typeof r.cost,
                keys: Object.keys(r).sort(),
                lastX: last && last.x,
                lastRoom: last && last.roomName,
                instanceof: r.path.every((p) => p instanceof RoomPosition),
                noStart: !r.path.some((p) => p.x === 5 && p.y === 25),
            };
        })()"#,
    )?;
    assert_eq!(v["ok"], true, "搜索表达式须成功: {v:?}");
    let r = &v["value"];
    assert_eq!(r["incomplete"], true, "单房被纵墙分隔：不可达");
    assert_eq!(r["opsType"], "number");
    assert_eq!(r["costType"], "number");
    assert_eq!(
        r["keys"],
        json!(["cost", "incomplete", "ops", "path"]),
        "结果四键"
    );
    assert_eq!(r["instanceof"], true, "path 元素是 RoomPosition 实例");
    assert_eq!(r["noStart"], true, "path 不含起点");
    // A* 在墙上贴齐 x=9 列（不可达时的最近可达格族）
    assert_eq!(r["lastX"], 9, "部分路径止于墙前一格: {r:?}");
    assert_eq!(r["lastRoom"], "W1N1");

    // 纵墙右侧同侧搜索：完整路径沿墙侧绕行到 range 边界
    let v = sb.run_expr(
        r#"(() => {
            const r = PathFinder.search(
                new RoomPosition(11, 25, 'W1N1'),
                new RoomPosition(30, 25, 'W1N1'));
            return { incomplete: r.incomplete, len: r.path.length,
                     endX: r.path[r.path.length - 1].x, cost: r.cost };
        })()"#,
    )?;
    assert_eq!(v["value"]["incomplete"], false, "同侧可达");
    assert_eq!(v["value"]["len"], 19, "19 步直线");
    assert_eq!(v["value"]["endX"], 30);
    assert_eq!(v["value"]["cost"], 19, "全平原 cost=步数");

    // 空目标早退：两键形态（engine path-finder.js:60-62）
    let v = sb.run_expr(
        "(() => { const a = PathFinder.search(new RoomPosition(5,25,'W1N1'), []); \
         const b = PathFinder.search(new RoomPosition(5,25,'W1N1'), null); \
         return { keys: Object.keys(a).sort(), same: JSON.stringify(a) === JSON.stringify(b) }; })()",
    )?;
    assert_eq!(v["value"]["keys"], json!(["ops", "path"]), "空目标两键");
    assert_eq!(v["value"]["same"], true, "null 与 [] 同形");

    // 起点已在 range 内：完整空结果（四键 incomplete:false）
    let v = sb.run_expr(
        r#"(() => {
            const r = PathFinder.search(
                new RoomPosition(5, 25, 'W1N1'),
                { pos: new RoomPosition(5, 25, 'W1N1'), range: 3 });
            return { len: r.path.length, incomplete: r.incomplete, ops: r.ops };
        })()"#,
    )?;
    assert_eq!(v["value"]["len"], 0);
    assert_eq!(v["value"]["incomplete"], false);
    assert_eq!(v["value"]["ops"], 0);

    // 跨房：W1N1 → W2N1 连续世界坐标路径（maxRooms 默认 16 可入邻房）
    let v = sb.run_expr(
        r#"(() => {
            const r = PathFinder.search(
                new RoomPosition(5, 25, 'W1N1'),
                { pos: new RoomPosition(45, 25, 'W2N1'), range: 0 });
            const rooms = new Set(r.path.map((p) => p.roomName));
            return { incomplete: r.incomplete, roomCount: rooms.size,
                     hasW1: rooms.has('W1N1'), hasW2: rooms.has('W2N1'),
                     last: r.path[r.path.length - 1].roomName };
        })()"#,
    )?;
    let r = &v["value"];
    // W1N1 有 x=10 纵墙：向西绕到 x<10 区域后从 W1N1 西界进入 W2N1 东界
    assert_eq!(r["incomplete"], false, "跨房可达（墙只挡直行）: {r:?}");
    assert_eq!(r["roomCount"], 2, "路径跨两房");
    assert_eq!(r["hasW1"], true);
    assert_eq!(r["hasW2"], true);
    assert_eq!(r["last"], "W2N1");

    // findPath：步进 {x,y,dx,dy,direction} 且同房截断。全平原 4 步等代价
    // 路径存在多条（直行/先对角），原生 JPS 平局规则决定具体形态——不断言
    // 逐步坐标，断言步进不变式与终点。
    let v = sb.run_expr(
        r#"(() => {
            const room = Game.rooms['W1N1'];
            const path = room.findPath(new RoomPosition(11, 25, 'W1N1'),
                                       new RoomPosition(15, 25, 'W1N1'));
            let px = 11, py = 25, ok = path.length === 4;
            for (const s of path) {
                if (Math.abs(s.dx) > 1 || Math.abs(s.dy) > 1
                    || s.x - px !== s.dx || s.y - py !== s.dy) { ok = false; }
                px = s.x; py = s.y;
            }
            return { ok, endX: px, endY: py,
                     serial: typeof room.findPath(new RoomPosition(11,25,'W1N1'),
                        new RoomPosition(15,25,'W1N1'), { serialize: true }) };
        })()"#,
    )?;
    let r = &v["value"];
    assert_eq!(r["ok"], true, "4 步且 dx/dy 与坐标增量一致");
    assert_eq!(r["endX"], 15);
    assert_eq!(r["endY"], 25);
    assert_eq!(r["serial"], "string");

    // moveTo：真路径首步 intent + _move 缓存写回。intents 在 end_tick 才
    // 带出，moveTo 断言须独立走一轮 begin → expr → end。
    let w = pathfinder_world();
    sb.begin_tick(&w)?;
    let v = sb.run_expr(
        r#"(() => {
            Game.creeps['w1'].moveTo(new RoomPosition(3, 25, 'W1N1'));
            const m = Memory.creeps && Memory.creeps['w1'] && Memory.creeps['w1']._move;
            return { hasCache: !!m, dest: m && m.dest, pathType: m && typeof m.path };
        })()"#,
    )?;
    let r = &v["value"];
    assert_eq!(r["hasCache"], true, "moveTo 写入 _move 缓存");
    assert_eq!(r["dest"], json!({"x": 3, "y": 25, "room": "W1N1"}));
    assert_eq!(r["pathType"], "string", "serializeMemory 缺省序列化串");
    sb.end_tick()?;
    let intents = sb
        .take_intents()
        .ok_or_else(|| anyhow::anyhow!("intents 应存在"))?;
    let direction = intents["c1"]["move"]["direction"]
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("move intent 缺 direction: {intents:?}"))?;
    // (5,25)→(3,25) range 1 的目标环三格等价（(4,24)/(4,25)/(4,26) 均 1 步
    // 可达），JPS 平局可取对角首步；断言方向属西向族而非具体枚举。
    assert!(
        (6..=8).contains(&direction),
        "首步向西（TOP_LEFT/LEFT/BOTTOM_LEFT 族），实际 {direction}"
    );
    Ok(())
}

/// KKPV 通道与 JSON 通道的搜索等价：同一世界两束注入下结果逐字段相等。
pub(super) fn pathfinder_kkpv_channel_equivalence_impl() -> anyhow::Result<()> {
    let world = pathfinder_world();
    // kk-world 夹具转换器产出公开地形束 + 私有对象束（数组形态地形）
    let public = kk_runner::SharedKkpvBundle::from_vec(
        kk_world::kkpv::kkpv_public_bundle_from_world_json(&world, [0; 32], [0; 32])
            .map_err(|e| anyhow::anyhow!("公开束转换失败：{e}"))?,
    );
    let private = kk_runner::SharedKkpvBundle::from_vec(
        kk_world::kkpv::kkpv_bundle_from_world_json(&world, [0; 32], [0; 32])
            .map_err(|e| anyhow::anyhow!("私有束转换失败：{e}"))?,
    );

    let expr = r#"(() => {
        const r = PathFinder.search(new RoomPosition(11, 25, 'W1N1'),
            new RoomPosition(30, 25, 'W1N1'));
        return { incomplete: r.incomplete, len: r.path.length, cost: r.cost,
                 ops: r.ops,
                 tail: r.path.length ? [r.path[r.path.length-1].x, r.path[r.path.length-1].y] : null };
    })()"#;

    let mut json_sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    json_sb.set_modules(table)?;
    json_sb.begin_tick(&world)?;
    let json_result = json_sb.run_expr(expr)?;

    let mut kkpv_sb = Sandbox::new(SandboxConfig::default())?;
    let mut table2 = ModuleTable::new();
    table2.insert_js("main", "module.exports.loop = function () {};");
    kkpv_sb.set_modules(table2)?;
    kkpv_sb.begin_tick_kkpv(
        &kk_world::kkpv::kkpv_envelope_from_world_json(&world),
        &public,
        &private,
    )?;
    let kkpv_result = kkpv_sb.run_expr(expr)?;

    assert_eq!(
        json_result["ok"], true,
        "JSON 通道搜索成功: {json_result:?}"
    );
    assert_eq!(
        kkpv_result["ok"], true,
        "KKPV 通道搜索成功: {kkpv_result:?}"
    );
    assert_eq!(
        json_result["value"], kkpv_result["value"],
        "两通道搜索结果逐字段相等"
    );
    Ok(())
}
