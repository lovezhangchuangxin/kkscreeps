//! PathFinder.search / findPath / moveTo 原生搜索接线的玩家面形态。
//!
//! 场景由 sandbox_tests.rs 的唯一测试串行调用（沙箱计数与销毁纪律）；
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
            new RoomPosition(30, 25, 'W1N1'), {roomCallback(name) {
                const m=new PathFinder.CostMatrix(); m.set(20,25,255); return m;
            }});
        let grid; Game.rooms.W1N1.findPath(new RoomPosition(5,25,'W1N1'),
            new RoomPosition(9,25,'W1N1'), {costCallback(name,m){grid=m.get(4,25);}});
        return { grid, incomplete: r.incomplete, len: r.path.length, cost: r.cost,
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

/// 同步玩家回调、原异常、矩阵偏移和重入均在正式沙箱通路验证。
pub(super) fn pathfinder_callback_family_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    sb.set_modules(table)?;
    tick(&mut sb, &pathfinder_world())?;
    let result = sb.run_expr(
        r#"(() => {
        const from = new RoomPosition(5,25,'W1N1');
        const to = new RoomPosition(9,25,'W1N1');
        const calls = [];
        const blocked = PathFinder.search(from, to, {roomCallback(name) {
            calls.push(name); return false;
        }});
        const matrix = new PathFinder.CostMatrix();
        matrix._bits = new Uint8Array(new ArrayBuffer(2700), 100, 2500);
        for (let y=0; y<50; y++) matrix.set(7,y,255);
        const wall = PathFinder.search(from,to,{maxRooms:1,roomCallback:()=>matrix});
        const short = PathFinder.search(from,to,{maxRooms:1,
            roomCallback:()=>({_bits:new Uint8Array(2499)})});
        const nonFunction = PathFinder.search(from,to,{roomCallback:3});
        const token = {message:'identity'};
        let same=false, getterSame=false;
        try { PathFinder.search(from,to,{roomCallback:()=>{throw token;}}); }
        catch(e) { same=e===token; }
        try { PathFinder.search(from,to,{roomCallback:()=>({get _bits(){throw token;}})}); }
        catch(e) { getterSame=e===token; }
        const primitiveTokens=[undefined,null,Symbol('identity'),17];
        const primitives=primitiveTokens.every(token=>{
            let caught=false;
            try {PathFinder.search(from,to,{roomCallback(){throw token;}});}
            catch(e){caught=e===token;}
            return caught;
        });
        let innerCaught=false;
        function nested(depth) {
            return PathFinder.search(from,to,{maxRooms:1,roomCallback:()=>{
                if (depth) nested(depth-1);
                else {
                    try { PathFinder.search(from,to,{roomCallback:()=>{throw token;}}); }
                    catch(e) { innerCaught=e===token; }
                }
            }});
        }
        const after=nested(3);
        const typed=PathFinder.search(from,to,{maxRooms:1,
            roomCallback:()=>({_bits:new Uint16Array(1250)})});
        const sharedBits=new Uint8Array(new SharedArrayBuffer(2700),100,2500);
        for(let y=0;y<50;y++)sharedBits[7*50+y]=255;
        const shared=PathFinder.search(from,to,{maxRooms:1,roomCallback:()=>({_bits:sharedBits})});
        return {calls, blocked:blocked.incomplete&&blocked.path.length===0,
            wall:wall.incomplete&&wall.path.every(p=>p.x<7),
            short:!short.incomplete&&short.cost===4,
            nonFunction:!nonFunction.incomplete, same, getterSame, primitives, innerCaught,
            nested:!after.incomplete&&after.cost===4, typed:typed.cost===4,
            shared:shared.incomplete&&shared.path.every(p=>p.x<7)};
    })()"#,
    )?;
    assert_eq!(result["ok"], true, "回调不得使沙箱或借用失效：{result}");
    assert_eq!(result["value"]["calls"], json!(["W1N1"]));
    for key in [
        "blocked",
        "wall",
        "short",
        "nonFunction",
        "same",
        "getterSame",
        "primitives",
        "innerCaught",
        "nested",
        "typed",
        "shared",
    ] {
        assert_eq!(result["value"][key], true, "回调契约 {key}: {result}");
    }

    // 道路穿墙、creep 障碍和 costCallback 克隆隔离同走 Room.findPath 通路。
    let mut world = pathfinder_world();
    world["objects"].as_array_mut().unwrap().extend([
        json!({"id":"road1","type":"structure","structureType":"road","room":"W1N1","x":10,"y":25}),
        json!({"id":"block1","type":"creep","room":"W1N1","x":7,"y":25,"name":"block","owner":"p1","body":["move"]}),
        json!({"id":"ramp1","type":"structure","structureType":"rampart","room":"W1N1","x":8,"y":25,"owner":"p2","isPublic":false}),
        json!({"id":"site1","type":"constructionSite","structureType":"spawn","room":"W1N1","x":9,"y":25,"owner":"p1"}),
    ]);
    tick(&mut sb, &world)?;
    let result = sb.run_expr(
        r#"(() => {
        const room=Game.rooms.W1N1, from=new RoomPosition(5,25,'W1N1');
        const to=new RoomPosition(15,25,'W1N1');
        let base;
        const path=room.findPath(from,to,{ignoreCreeps:true,costCallback(name,m){
            base=[m.get(4,25),m.get(7,25),m.get(8,25),m.get(9,25),m.get(10,25)];
        }});
        let creep;
        room.findPath(from,to,{costCallback(name,m){creep=m.get(7,25);}});
        const denied=from.findClosestByPath([to],{ignoreCreeps:true,costCallback(name,m){
            for(let y=0;y<50;y++)m.set(6,y,255);
        }});
        const again=room.findPath(from,to,{ignoreCreeps:true});
        const ignore=room.findPath(from,to,{ignoreCreeps:true,ignoreDestructibleStructures:true,
            costCallback(name,m){if(m.get(8,25)!==0||m.get(9,25)!==0)throw new Error('ignore');}});
        return {base,creep,road:path.some(p=>p.x===10&&p.y===25),
            denied:denied===null,clone:again.some(p=>p.x===10),ignore:ignore.length>0};
    })()"#,
    )?;
    assert_eq!(result["ok"], true, "房间代价矩阵: {result}");
    assert_eq!(result["value"]["base"], json!([255, 0, 255, 255, 1]));
    assert_eq!(result["value"]["creep"], 255);
    for key in ["road", "denied", "clone", "ignore"] {
        assert_eq!(result["value"][key], true, "房间代价 {key}: {result}");
    }
    // 新 tick 重建缓存；自己的安全模式只忽略敌方 creep，不忽略自己的。
    // 来源：官方 rooms.js::makePathfindingGrid2 的 safeMode 分支。
    world["time"] = json!(8);
    world["objects"].as_array_mut().unwrap().extend([
        json!({"id":"ctl1","type":"structure","structureType":"controller","room":"W1N1","x":20,"y":20,"owner":"p1","level":3,"safeMode":100}),
        json!({"id":"hostile1","type":"creep","room":"W1N1","x":6,"y":25,"name":"hostile","owner":"p2","body":["move"]}),
    ]);
    tick(&mut sb, &world)?;
    let result = sb.run_expr(
        r#"(() => {
        let grid;
        Game.rooms.W1N1.findPath(new RoomPosition(5,25,'W1N1'),new RoomPosition(9,25,'W1N1'),
            {costCallback(name,m){grid=[m.get(6,25),m.get(7,25),m.get(20,20)];}});
        return grid;
    })()"#,
    )?;
    assert_eq!(
        result["value"],
        json!([0, 255, 255]),
        "安全模式与 tick 缓存: {result}"
    );
    // 套件 ROOMPOS-FIND-010 要求尚未发布的 range 增强；锁定 stable vanilla
    // 固定 range=1 的语义，不能为通过该例绕过真实 costCallback。
    let result = sb.run_expr(
        r#"new RoomPosition(11,10,'W1N1').findClosestByPath(
        [new RoomPosition(30,10,'W1N1')], {range:5,costCallback(name,m){
            for(let x=26;x<=34;x++)for(let y=6;y<=14;y++)m.set(x,y,255);
            return m;
        }})"#,
    )?;
    assert_eq!(result["value"], serde_json::Value::Null);
    // 官方 creeps.js::moveTo：疲劳且有绘制选项时，缓存 moveByPath 的
    // ERR_TIRED 仍落入重算；套件 UNDOC-MOVECACHE-004 的增强尚未在 vanilla 发布。
    sb.begin_tick(&world)?;
    let result =
        sb.run_expr("Game.creeps.w1.moveTo(new RoomPosition(3,25,'W1N1'),{reusePath:20})")?;
    assert_eq!(result["value"], 0);
    sb.end_tick()?;
    world["time"] = json!(9);
    world["objects"][0]["fatigue"] = json!(10);
    sb.begin_tick(&world)?;
    let result=sb.run_expr(r#"(() => {
        let calls=0;
        const rc=Game.creeps.w1.moveTo(new RoomPosition(3,25,'W1N1'),{
            reusePath:20,visualizePathStyle:{stroke:'#fff'},costCallback(name,m){calls++;return m;}});
        return {rc,calls};
    })()"#)?;
    assert_eq!(result["value"], json!({"rc":-11,"calls":1}));
    sb.end_tick()?;
    Ok(())
}

/// 玩家回调死循环仍走正常预算中断，不能转成可捕获错误或空路径。
pub(super) fn pathfinder_callback_budget_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig {
        max_per_tick: 50.0,
        initial_bucket: 100.0,
        ..SandboxConfig::default()
    })?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        r#"module.exports.loop=function(){
        try { PathFinder.search(new RoomPosition(5,25,'W1N1'),new RoomPosition(9,25,'W1N1'),
            {roomCallback(){while(true){}}}); }
        catch(e){globalThis.caught=true;}
        globalThis.returned=true;
    };"#,
    );
    sb.set_modules(table)?;
    let start = std::time::Instant::now();
    assert!(matches!(
        tick(&mut sb, &pathfinder_world())?,
        TickOutcome::Terminated
    ));
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop=function(){};");
    sb.set_modules(table)?;
    tick(&mut sb, &pathfinder_world())?;
    let value=sb.run_expr("({caught:globalThis.caught===true,returned:globalThis.returned===true, cost:PathFinder.search(new RoomPosition(5,25,'W1N1'),new RoomPosition(9,25,'W1N1')).cost})")?;
    assert_eq!(
        value["value"],
        json!({"caught":false,"returned":false,"cost":4})
    );
    Ok(())
}

/// 原生直接读取可变矩阵是行为参考：后续房间回调修改旧矩阵，JS 桥必须同形。
pub(super) fn pathfinder_matrix_mutation_impl() -> anyhow::Result<()> {
    use kk_pathfinder::{HostRoomDecision, SearchHost};
    use std::cell::Cell;
    struct Reference {
        first: Box<[Cell<u8>; 2500]>,
        blocked: Box<[Cell<u8>; 2500]>,
        live: bool,
        update: u8,
    }
    // SAFETY: 固定 Cell 存储保持到返回，仅在原生暂停的 room 回调中修改。
    unsafe impl SearchHost for Reference {
        fn room(&mut self, xx: u8, yy: u8) -> HostRoomDecision {
            let origin = kk_pathfinder::generate_room_name(xx, yy) == "W2N1";
            if !origin && self.live {
                for cell in self.first.iter() {
                    cell.set(self.update);
                }
            }
            let bytes = if origin { &self.first } else { &self.blocked };
            HostRoomDecision::Matrix {
                data: bytes.as_ptr().cast(),
                len: 2500,
            }
        }
    }
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop=function(){};");
    sb.set_modules(table)?;
    tick(&mut sb, &pathfinder_world())?;
    let result=sb.run_expr(r#"(() => {
        const bits=new Uint8Array(2500);
        const r=PathFinder.search(new RoomPosition(45,25,'W2N1'),new RoomPosition(20,25,'W1N1'),
            {maxRooms:2,roomCallback(name){if(name!=='W2N1')bits.fill(255);return {_bits:bits};}});
        return {path:r.path.map(p=>[p.x,p.y,p.roomName]),ops:r.ops,cost:r.cost,incomplete:r.incomplete};
    })()"#)?;
    assert_eq!(result["ok"], true, "可变矩阵搜索: {result}");
    let terrain = kk_runner::pathfinder::TerrainPack::from_world_json(&pathfinder_world())
        .map_err(anyhow::Error::msg)?
        .native()
        .map_err(anyhow::Error::msg)?;
    let origin = kk_pathfinder::WorldPosition::from_room("W2N1", 45, 25)?;
    let goals = [kk_pathfinder::Goal::pos(
        kk_pathfinder::WorldPosition::from_room("W1N1", 20, 25)?,
    )];
    let options = kk_pathfinder::SearchOptions {
        max_rooms: 2,
        ..Default::default()
    };
    let mut host = Reference {
        first: Box::new(std::array::from_fn(|_| Cell::new(0))),
        blocked: Box::new(std::array::from_fn(|_| Cell::new(255))),
        live: true,
        update: 255,
    };
    let expected = kk_pathfinder::search_with_host(&terrain, origin, &goals, &options, &mut host)?;
    let frozen = kk_pathfinder::search_with_host(
        &terrain,
        origin,
        &goals,
        &options,
        &mut Reference {
            first: Box::new(std::array::from_fn(|_| Cell::new(0))),
            blocked: Box::new(std::array::from_fn(|_| Cell::new(255))),
            live: false,
            update: 255,
        },
    )?;
    assert_ne!(expected, frozen, "夹具必须击杀只在首次返回时复制的错误实现");
    let path: Vec<_> = expected
        .path
        .iter()
        .map(|p| json!([p.x(), p.y(), p.room_name()]))
        .collect();
    assert_eq!(
        result["value"],
        json!({"path":path,"ops":expected.ops,"cost":expected.cost,"incomplete":expected.incomplete})
    );
    // 后续回调通过内层搜索修改外层矩阵，仍须在原生恢复前同步。
    let result=sb.run_expr(r#"(() => {
        const bits=new Uint8Array(2500),blocked=new Uint8Array(2500).fill(255);
        const r=PathFinder.search(new RoomPosition(45,25,'W2N1'),new RoomPosition(20,25,'W1N1'),
            {maxRooms:2,roomCallback(name){
                if(name==='W2N1')return {_bits:bits};
                PathFinder.search(new RoomPosition(45,25,'W2N1'),new RoomPosition(44,25,'W2N1'),
                    {maxRooms:1,roomCallback(){bits.fill(255);}});
                return {_bits:blocked};
            }});
        return {path:r.path.map(p=>[p.x,p.y,p.roomName]),ops:r.ops,cost:r.cost,incomplete:r.incomplete};
    })()"#)?;
    assert_eq!(
        result["value"],
        json!({"path":path,"ops":expected.ops,"cost":expected.cost,"incomplete":expected.incomplete})
    );
    // 已返回的视图被 detach 后，搜索持有的 backing store 不得失效。
    let result=sb.run_expr(r#"(() => {
        const bits=new Uint8Array(2500),blocked=new Uint8Array(2500).fill(255);
        let detached=false;
        const r=PathFinder.search(new RoomPosition(45,25,'W2N1'),new RoomPosition(20,25,'W1N1'),
            {maxRooms:2,roomCallback(name){
                if(name==='W2N1')return {_bits:bits};
                bits.buffer.transfer();detached=bits.byteLength===0;
                return {_bits:blocked};
            }});
        return {detached,path:r.path.map(p=>[p.x,p.y,p.roomName]),ops:r.ops,cost:r.cost,incomplete:r.incomplete};
    })()"#)?;
    let frozen_path: Vec<_> = frozen
        .path
        .iter()
        .map(|p| json!([p.x(), p.y(), p.room_name()]))
        .collect();
    assert_eq!(
        result["value"],
        json!({"detached":true,"path":frozen_path,"ops":frozen.ops,"cost":frozen.cost,"incomplete":frozen.incomplete}),
        "detach 保留存活字节: {result}"
    );
    // RAB 收缩后已移除的字节不能继续沿用旧副本。固定原生矩阵将不可读
    // 部分归零（地形默认值），避免越界读取并保持明确的本地边界策略。
    let edge_origin = kk_pathfinder::WorldPosition::from_room("W2N1", 49, 25)?;
    let shrunk = kk_pathfinder::search_with_host(
        &terrain,
        edge_origin,
        &goals,
        &options,
        &mut Reference {
            first: Box::new(std::array::from_fn(|i| {
                Cell::new(if i / 50 == 48 { 255 } else { 0 })
            })),
            blocked: Box::new(std::array::from_fn(|_| Cell::new(255))),
            live: true,
            update: 0,
        },
    )?;
    let stale = kk_pathfinder::search_with_host(
        &terrain,
        edge_origin,
        &goals,
        &options,
        &mut Reference {
            first: Box::new(std::array::from_fn(|i| {
                Cell::new(if i / 50 == 48 { 255 } else { 0 })
            })),
            blocked: Box::new(std::array::from_fn(|_| Cell::new(255))),
            live: false,
            update: 0,
        },
    )?;
    assert_ne!(shrunk, stale, "收缩夹具须能检出陈旧代价");
    let result=sb.run_expr(r#"(() => {
        const buffer=new ArrayBuffer(2700,{maxByteLength:5000});
        const bits=new Uint8Array(buffer,100,2500),blocked=new Uint8Array(2500).fill(255);
        for(let y=0;y<50;y++)bits[48*50+y]=255;
        const r=PathFinder.search(new RoomPosition(49,25,'W2N1'),new RoomPosition(20,25,'W1N1'),
            {maxRooms:2,roomCallback(name){
                if(name==='W2N1')return {_bits:bits};
                buffer.resize(0);return {_bits:blocked};
            }});
        return {path:r.path.map(p=>[p.x,p.y,p.roomName]),ops:r.ops,cost:r.cost,incomplete:r.incomplete};
    })()"#)?;
    let path: Vec<_> = shrunk
        .path
        .iter()
        .map(|p| json!([p.x(), p.y(), p.room_name()]))
        .collect();
    assert_eq!(
        result["value"],
        json!({"path":path,"ops":shrunk.ops,"cost":shrunk.cost,"incomplete":shrunk.incomplete}),
        "收缩不得保留旧矩阵: {result}"
    );
    Ok(())
}

/// 原始 JSON 没有可信世界身份；相同 rev 不能复用另一个世界的地形。
pub(super) fn same_revision_world_switch_uses_current_terrain_impl() -> anyhow::Result<()> {
    let mut sandbox = Sandbox::new(SandboxConfig::default())?;
    let mut modules = ModuleTable::new();
    modules.insert_js("main", "module.exports.loop = function () {};");
    sandbox.set_modules(modules)?;
    let mut world = pathfinder_world();
    let expression = "(() => {const r=PathFinder.search(new RoomPosition(5,25,'W1N1'),new RoomPosition(30,25,'W1N1'),{maxRooms:1});return {path:r.path,cost:r.cost,ops:r.ops,incomplete:r.incomplete};})()";
    let mut answers = Vec::new();
    for cells in [terrain_with_wall(10), vec![0; 2500], terrain_with_wall(10)] {
        let mut source = kk_world::World::default();
        source
            .register_room("W1N1", kk_core::Terrain::from_codes(&cells).unwrap())
            .map_err(anyhow::Error::msg)?;
        let mut terrain = source.world_json()["rooms"]["W1N1"]["terrain"].clone();
        terrain["rev"] = json!(7);
        world["rooms"]["W1N1"]["terrain"] = terrain;
        tick(&mut sandbox, &world)?;
        let result = sandbox.run_expr(expression)?;
        assert_eq!(result["ok"], true, "{result}");
        answers.push(result["value"].clone());
    }
    assert_eq!(answers[0]["incomplete"], true);
    assert_eq!(answers[1]["incomplete"], false);
    assert_eq!(answers[0], answers[2]);
    assert_ne!(answers[0]["path"], answers[1]["path"]);
    Ok(())
}
