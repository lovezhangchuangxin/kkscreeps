//! B1 结构治理批自 sandbox_tests.rs 连续切块（串行链 impl；顶层唯一 #[test] 不变量
//! 在 crate 根——本文件全部 fn 由根链按原调用序调用，不得加 #[test]）。

use super::*;

pub(super) fn resource_family_prelude_matrix_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" }, "W2N1": { "name": "W2N1" } },
        "objects": [
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["carry", "carry"], "store": { "energy": 80 } },
            { "id": "kk2", "type": "creep", "room": "W1N1", "x": 25, "y": 26, "name": "w2",
              "owner": "p1", "body": ["carry"], "store": { "energy": 10 } },
            { "id": "kk3", "type": "creep", "room": "W1N1", "x": 26, "y": 26, "name": "w3",
              "owner": "p2", "body": ["carry"], "store": { "energy": 5 } },
            { "id": "kk4", "type": "creep", "room": "W1N1", "x": 27, "y": 27, "name": "w4",
              "owner": "p1", "body": ["carry"], "store": { "energy": 50 }, "spawning": true },
            { "id": "kk5", "type": "creep", "room": "W1N1", "x": 28, "y": 28, "name": "w5",
              "owner": "p1", "body": ["carry"], "store": { "energy": 0 } },
            { "id": "kk6", "type": "creep", "room": "W1N1", "x": 24, "y": 24, "name": "w6",
              "owner": "p1", "body": ["carry", "carry"], "store": { "energy": 100 } },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 24, "y": 25, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 300 }, "energyCapacity": 300 },
            { "id": "e1", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 26, "y": 24, "owner": "p1", "store": { "energy": 0 } },
            { "id": "e2", "type": "structure", "structureType": "extension", "room": "W1N1",
              "x": 24, "y": 26, "owner": "p1", "store": { "energy": 50 } },
            { "id": "k1", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "owner": "p1" },
            { "id": "src1", "type": "structure", "structureType": "source", "room": "W1N1",
              "x": 40, "y": 40, "energy": 2500 },
            { "id": "st1", "type": "structure", "structureType": "storage", "room": "W1N1",
              "x": 45, "y": 45, "owner": "p1", "store": { "energy": 500 } },
            { "id": "r1", "type": "resource", "room": "W1N1", "x": 25, "y": 24,
              "resourceType": "energy", "amount": 30 },
            { "id": "r2", "type": "resource", "room": "W1N1", "x": 45, "y": 44,
              "resourceType": "energy", "amount": 30 },
            // A4-ii 评审 3 P1 补测面：nuker/powerBank/terminal（W1N1，kk2 邻接
            // (25,27)/(26,27)/(24,27)；powerBank 手写世界造 store 使 :532 分支
            // 可达——官方 powerBank 无 store 在目标门即拒）与 p2 safeMode 房
            { "id": "nk1", "type": "structure", "structureType": "nuker", "room": "W1N1",
              "x": 25, "y": 27, "owner": "p2", "store": { "energy": 100 } },
            { "id": "pb1", "type": "structure", "structureType": "powerBank", "room": "W1N1",
              "x": 26, "y": 27, "owner": "p2", "store": { "power": 100 } },
            { "id": "tm1", "type": "structure", "structureType": "terminal", "room": "W1N1",
              "x": 24, "y": 27, "owner": "p2", "store": { "energy": 500 },
              "effects": [{ "power": 15, "ticksRemaining": 5 }] },
            { "id": "tm2", "type": "structure", "structureType": "terminal", "room": "W1N1",
              "x": 26, "y": 25, "owner": "p1", "store": { "energy": 500 } },
            { "id": "w7", "type": "creep", "room": "W2N1", "x": 25, "y": 25, "name": "w7",
              "owner": "p1", "body": ["carry"], "store": { "energy": 0 } },
            { "id": "k2", "type": "structure", "structureType": "controller", "room": "W2N1",
              "x": 10, "y": 10, "level": 3, "owner": "p2", "safeMode": 100 },
            { "id": "sp2", "type": "spawn", "room": "W2N1", "x": 26, "y": 25, "name": "Spawn2",
              "owner": "p2", "store": { "energy": 300 }, "energyCapacity": 300 }
        ]
    });
    // 预检返回码断言宏（直持 sb——闭包形式与穿插的裸 run_expr 抢借用）
    macro_rules! ck {
        ($expr:expr, $want:expr) => {{
            let v = sb.run_expr($expr)?;
            assert_eq!(v["ok"], true, "矩阵求值须成功: {} → {:?}", $expr, v);
            assert_eq!(v["value"].as_i64(), Some($want), "预检返回码: {}", $expr);
        }};
    }

    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            // ---- drop（官方 creeps.js:404-427：my/spawning → 白名单 → 保有 → 缺省）----
            ck!("Game.creeps['w1'].drop(RESOURCE_ENERGY, 30)", 0);
            // 官方无 amount<0 预检：负数不落 INVALID_ARGS（全段无 amount<0 检查，
            // store 80 ≥ -5 放行——执行器 amount>0 才落地，官方同形 no-op）
            ck!("Game.creeps['w1'].drop(RESOURCE_ENERGY, -5)", 0);
            ck!("Game.creeps['w1'].drop(RESOURCE_ENERGY, 999)", -6);
            ck!("Game.creeps['w1'].drop('notaresource')", -10);
            ck!("Game.creeps['w5'].drop(RESOURCE_ENERGY)", -6);
            ck!("Game.getObjectById('kk3').drop(RESOURCE_ENERGY)", -1);
            ck!("Game.creeps['w4'].drop(RESOURCE_ENERGY)", -4);

            // ---- transfer（官方 :428-492：amount<0 → 白名单 → 目标族 → 容量 →
            // 邻接 → 保有 → 缺省钳制/显式超容拒）----
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('e1'), RESOURCE_ENERGY)",
                0
            );
            // e2 已满（rcl3 档 50——容量折算的投影锁值）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('e2'), RESOURCE_ENERGY)",
                -8
            );
            // spawn 满仓 300/300
            ck!(
                "Game.creeps['w1'].transfer(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -8
            );
            // creep 目标：显式超容拒（60+10 > 50），部分量放行（20+10 ≤ 50）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), RESOURCE_ENERGY, 60)",
                -8
            );
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), RESOURCE_ENERGY, 20)",
                0
            );
            // 邻接后置：远 storage 过容量门后在 isNearTo 拒
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('st1'), RESOURCE_ENERGY)",
                -9
            );
            // source 不在 register.structures（game.js:224-247 键集无 source）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('src1'), RESOURCE_ENERGY)",
                -7
            );
            // spawning 目标拒（:445）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk4'), RESOURCE_ENERGY)",
                -7
            );
            // 非能量资源进 perResource 结构：容量 0 → INVALID_TARGET（:459）
            ck!(
                "Game.creeps['w1'].transfer(Game.spawns['Spawn1'], RESOURCE_POWER)",
                -7
            );
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('e1'), RESOURCE_POWER)",
                -7
            );
            // 参数门在目标门之前（:435/:438）
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), RESOURCE_ENERGY, -5)",
                -10
            );
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('kk2'), 'bogus')",
                -10
            );
            ck!("Game.creeps['w1'].transfer(null, RESOURCE_ENERGY)", -7);
            ck!(
                "Game.creeps['w1'].transfer({id: 'short'}, RESOURCE_ENERGY)",
                -7
            );
            // controller+能量重定向 upgradeController（:449-451）——A5-lite 起
            // upgradeController 是真十门：夹具 w1 纯 carry 无 WORK → 门③
            // ERR_NO_BODYPART（-12 本身证明重定向进了真方法预检链；桩时代
            // 的假 OK 断言已废）。正路径见 redirect_upgrade_positive_impl
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('k1'), RESOURCE_ENERGY)",
                -12
            );
            // 非能量进 controller：容量 0 → INVALID_TARGET
            ck!(
                "Game.creeps['w1'].transfer(Game.getObjectById('k1'), RESOURCE_POWER)",
                -7
            );
            // 序列化对象拒绝（game.js:122-126 assertTargetObject 抛错点）
            let v = sb.run_expr(
                "Game.creeps['w1'].transfer({id: '123456789012345678901234'}, RESOURCE_ENERGY)",
            )?;
            assert_eq!(v["ok"], false, "plain object + 24 位 id 须抛序列化拒绝");
            assert!(
                v["error"]
                    .as_str()
                    .unwrap_or("")
                    .contains("serialized game object"),
                "官方文案: {v:?}"
            );

            // ---- withdraw（官方 :493-565：参数门 → store 面/目标族 → terminal →
            // rampart（桩面恒放行）→ safeMode → nuker/powerBank → 容量/保有 →
            // 邻接 → 空容量 → 缺省/超容拒）----
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY, 10)",
                0
            );
            // 超过自身空容量（100-80=20）
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY, 30)",
                -8
            );
            // 满仓 creep
            ck!(
                "Game.creeps['w6'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -8
            );
            // 空目标结构：过容量门后缺省 amount=0 → NOT_ENOUGH_RESOURCES
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('e1'), RESOURCE_ENERGY)",
                -6
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('st1'), RESOURCE_ENERGY)",
                -9
            );
            // source/creep 目标族拒（:507-516）
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('src1'), RESOURCE_ENERGY)",
                -7
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.getObjectById('kk2'), RESOURCE_ENERGY)",
                -7
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY, -1)",
                -10
            );
            ck!(
                "Game.creeps['w1'].withdraw(Game.spawns['Spawn1'], 'bogus')",
                -10
            );
            ck!(
                "Game.getObjectById('kk3').withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -1
            );
            ck!(
                "Game.creeps['w4'].withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY)",
                -4
            );
            // 未知 id 在 store 面（data() 求值点）抛官方同文案——早于 assertTargetObject
            let v = sb.run_expr(
                "Game.creeps['w1'].withdraw({id: 'aaaaaaaaaaaaaaaaaaaaaaaa'}, RESOURCE_ENERGY)",
            )?;
            assert_eq!(v["ok"], false, "未知 id 须抛 Could not find");
            assert!(
                v["error"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Could not find an object with ID"),
                "官方文案: {v:?}"
            );

            // ---- withdraw 补测（A4-ii 评审 3 P1：已实现零覆盖分支）----
            // safeMode 门（:528-530）：W2N1 是 p2 的 safeMode 生效房（k2
            // safeMode=100 绝对挂钟 > time=1），p1 creep 提取该房任何 store 即拒
            ck!(
                "Game.getObjectById('w7').withdraw(Game.getObjectById('sp2'), RESOURCE_ENERGY)",
                -1
            );
            // nuker/powerBank 不可提取（:532-534）——powerBank 经手写 store 造
            // 到该分支（官方无 store 在目标门即拒）
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('nk1'), RESOURCE_ENERGY)",
                -7
            );
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('pb1'), RESOURCE_ENERGY)",
                -7
            );
            // terminal 被 PWR_DISRUPT_TERMINAL(15) 封锁（:518-521）与无 effects 放行
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('tm1'), RESOURCE_ENERGY)",
                -7
            );
            // actor 用 w2（w1 的 withdraw intent 已被矩阵主链 last-wins 锁 s1——
            // 不覆盖；w2 之前只有被拒的 nk1 调用无 intent）
            ck!(
                "Game.creeps['w2'].withdraw(Game.getObjectById('tm2'), RESOURCE_ENERGY, 5)",
                0
            );
            // transfer 的 owner/busy 门（评审 3：同形门在 drop/withdraw 有覆盖
            // 而 transfer 缺席）
            ck!(
                "Game.getObjectById('kk3').transfer(Game.creeps['w1'], RESOURCE_ENERGY, 1)",
                -1
            );
            ck!(
                "Game.creeps['w4'].transfer(Game.creeps['w1'], RESOURCE_ENERGY, 1)",
                -4
            );

            // ---- pickup（官方 :566-593：my/spawning → Resource 目标 → 满仓 →
            // isNearTo ≤1 格）----
            ck!("Game.creeps['w1'].pickup(Game.getObjectById('r1'))", 0);
            ck!("Game.creeps['w1'].pickup(Game.getObjectById('r2'))", -9);
            ck!("Game.creeps['w6'].pickup(Game.getObjectById('r1'))", -8);
            ck!("Game.creeps['w1'].pickup(Game.spawns['Spawn1'])", -7);
            ck!(
                "Game.getObjectById('kk3').pickup(Game.getObjectById('r1'))",
                -1
            );
            ck!("Game.creeps['w4'].pickup(Game.getObjectById('r1'))", -4);
            let v = sb.run_expr("Game.creeps['w1'].pickup({id: '123456789012345678901234'})")?;
            assert_eq!(v["ok"], false, "pickup 序列化对象同抛");
            assert!(
                v["error"]
                    .as_str()
                    .unwrap_or("")
                    .contains("serialized game object"),
                "官方文案: {v:?}"
            );

            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            // 矩阵 tick 的 last-wins 锁值：drop 负 amount 官方无门原样进 intent
            //（执行器 amount>0 才落地——no-op）；transfer 到 kk2 20
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(
                intents["kk1"]["drop"]["amount"], -5,
                "负 amount 原样记录（官方无门）"
            );
            assert_eq!(intents["kk1"]["drop"]["resourceType"], "energy");
            assert_eq!(intents["kk1"]["transfer"]["id"], "kk2");
            assert_eq!(intents["kk1"]["transfer"]["amount"], 20, "显式部分量 20");
            // 矩阵内唯一放行的 withdraw（10）last-wins 存留；被拒的调用不得记 intent
            assert_eq!(intents["kk1"]["withdraw"]["id"], "s1");
            assert_eq!(intents["kk1"]["withdraw"]["amount"], 10);
            assert_eq!(
                intents["kk1"]["pickup"]["id"], "r1",
                "矩阵内唯一放行的 pickup"
            );
        }
        _ => panic!("应正常执行"),
    }

    // ---- 第二 tick：四方法缺省量/钳制的 intent 载荷锁值 ----
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(
                r#"(()=>{
                const w1 = Game.creeps['w1'];
                return [
                    w1.drop(RESOURCE_ENERGY, 30),
                    w1.transfer(Game.getObjectById('e1'), RESOURCE_ENERGY),
                    w1.withdraw(Game.spawns['Spawn1'], RESOURCE_ENERGY),
                    w1.pickup(Game.getObjectById('r1')),
                ].join(',');
            })()"#,
            )?;
            assert_eq!(v["value"], "0,0,0,0", "四方法缺省形态全部放行");
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            let intents = sb.take_intents().expect("intentsList 带出");
            // drop 显式 30
            assert_eq!(intents["kk1"]["drop"]["amount"], 30);
            // transfer 缺省 = min(保有 80, 目标容量 50 − 已存 0) = 50——
            // extension 容量按房 rcl3 折算（官方 extensions/tick.js:10-17 的
            // storeCapacityResource.energy 投影等价锁值）
            assert_eq!(intents["kk1"]["transfer"]["id"], "e1");
            assert_eq!(
                intents["kk1"]["transfer"]["amount"], 50,
                "缺省钳到 rcl3 extension 容量"
            );
            assert_eq!(intents["kk1"]["transfer"]["resourceType"], "energy");
            // withdraw 缺省 = min(空容量 20, spawn 存量 300) = 20——
            // actor 容量按 body CARRY×50 折算（_recalc-body.js:23 投影等价锁值）
            assert_eq!(intents["kk1"]["withdraw"]["id"], "s1");
            assert_eq!(
                intents["kk1"]["withdraw"]["amount"], 20,
                "缺省钳到自身空容量"
            );
            assert_eq!(intents["kk1"]["withdraw"]["resourceType"], "energy");
            // pickup 载荷只有 id（官方 :586）
            assert_eq!(intents["kk1"]["pickup"]["id"], "r1");
            assert_eq!(
                serde_json::to_string(&intents["kk1"]["pickup"])?,
                r#"{"id":"r1"}"#,
                "pickup 载荷仅 id（无 amount/resourceType）"
            );
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A4-iv 移动杂项族 prelude 预检矩阵（moveTo 直线桩/moveByPath/say/suicide/
/// pull/notifyWhenAttacked）：逐分支对照官方 creeps.js:158-257/305-327/826-838/
/// 813-825/1101-1126 与 structures.js:89-101/1220-1233 的返回码与 intent 形态。
pub(super) fn move_misc_family_prelude_forms_impl() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            // 主体：p1 的 move+work @ (25,25)
            { "id": "kk1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"] },
            // 敌方 creep 邻接 @ (26,25)（pull 可拉敌 creep——官方无目标归属门）
            { "id": "kk2", "type": "creep", "room": "W1N1", "x": 26, "y": 25, "name": "e1",
              "owner": "p2", "body": ["move"] },
            // 远处敌方 creep @ (40,40)（NOT_IN_RANGE 分支）
            { "id": "kk3", "type": "creep", "room": "W1N1", "x": 40, "y": 40, "name": "e2",
              "owner": "p2", "body": ["move"] },
            // 孵化中自家 creep（BUSY 分支）
            { "id": "kk4", "type": "creep", "room": "W1N1", "x": 25, "y": 26, "name": "b1",
              "owner": "p1", "body": ["move"], "spawning": true },
            // 疲劳 creep（moveTo 的 TIRED 门——官方 fatigue 显式回查）
            { "id": "kk5", "type": "creep", "room": "W1N1", "x": 25, "y": 24, "name": "t1",
              "owner": "p1", "body": ["move", "work"], "fatigue": 2 },
            // 无 MOVE creep（NO_BODYPART 门）
            { "id": "kk6", "type": "creep", "room": "W1N1", "x": 25, "y": 23, "name": "nm1",
              "owner": "p1", "body": ["work", "carry"] },
            // 自家 spawn（notifyWhenAttacked 覆盖版 + 已存 true 的差分门）
            { "id": "kk7", "type": "spawn", "structureType": "spawn", "room": "W1N1",
              "x": 24, "y": 25, "name": "S1", "owner": "p1", "store": { "energy": 300 },
              "notifyWhenAttacked": true },
            // 中性路（基类版：my===undefined 放行——房间归 p1 controller）
            { "id": "kk8", "type": "structure", "structureType": "road", "room": "W1N1",
              "x": 25, "y": 27 },
            // 敌方 tower（基类版 my===false → NOT_OWNER）
            { "id": "kk9", "type": "structure", "structureType": "tower", "room": "W1N1",
              "x": 23, "y": 25, "owner": "p2", "hits": 1 },
            // controller 归 p1（基类版的房归属分支基准）
            { "id": "kk10", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "owner": "p1" }
        ]
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {
            let v = sb.run_expr(r#"(()=>{
                const g = (i) => Game.getObjectById(i);
                const RP = RoomPosition;
                return {
                  say: [g('kk1').say('hi', true), g('kk2').say('x'), g('kk4').say('x'), g('kk5').say(42)],
                  suicide: [g('kk1').suicide(), g('kk4').suicide()],
                  pull: [g('kk1').pull(g('kk2')), g('kk1').pull(g('kk1')), g('kk1').pull(g('kk7')),
                         g('kk1').pull(g('kk3')), g('kk4').pull(g('kk1')), g('kk1').pull(null)],
                  moveTo: [g('kk2').moveTo(30, 25),
                           g('kk1').moveTo(new RP(25, 25, 'W1N1')), g('kk1').moveTo(25, 25),
                           g('kk1').moveTo('abc'), g('kk1').moveTo({x: 30}),
                           g('kk1').moveTo(30, 25), g('kk1').moveTo(g('kk2')),
                           g('kk5').moveTo(30, 25), g('kk6').moveTo(30, 25),
                           g('kk1').moveTo(30, 25, {noPathFinding: true}) ],
                  moveByPath: [g('kk1').moveByPath([new RP(25, 25, 'W1N1'), new RP(26, 25, 'W1N1')]),
                               g('kk1').moveByPath([new RP(40, 40, 'W1N1')]),
                               g('kk1').moveByPath('26253'),
                               g('kk1').moveByPath(5),
                               g('kk1').moveByPath([]),
                               (()=>{ try { g('kk1').moveByPath('abc'); return 'no-throw'; } catch (e) { return 'threw'; } })()],
                  nwa: [g('kk7').notifyWhenAttacked(true), g('kk7').notifyWhenAttacked(false),
                        g('kk7').notifyWhenAttacked(42), g('kk8').notifyWhenAttacked(true),
                        g('kk9').notifyWhenAttacked(true)],
                };
            })()"#)?;
            let r = &v["value"];
            let expect = |arr: serde_json::Value, want: &[i64], label: &str| {
                let got: Vec<i64> = arr
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_i64().unwrap())
                    .collect();
                assert_eq!(&got, want, "{label}");
            };
            // say：OK / NOT_OWNER / BUSY(spawning) / OK（疲劳 creep 可 say——
            // 官方无疲劳门；数字字符串化在 intent 面）
            expect(
                r["say"].clone(),
                &[0, -1, -4, 0],
                "say 预检序（creeps.js:826-838）",
            );
            expect(
                r["suicide"].clone(),
                &[0, -4],
                "suicide（creeps.js:813-825）",
            );
            // pull：邻接敌方 OK / 自身 INVALID_TARGET / spawn 非 creep INVALID_TARGET /
            // 远处 NOT_IN_RANGE / spawning BUSY / null INVALID_TARGET
            expect(
                r["pull"].clone(),
                &[0, -7, -7, -9, -4, -7],
                "pull（creeps.js:1101-1126）",
            );
            // moveTo：敌方 NOT_OWNER / 同格 RoomPosition OK / 同格数值 OK /
            // 非对象目标 INVALID_TARGET / 无 pos 对象 INVALID_TARGET / 数值 OK /
            // 对象形态（creep 目标解包 .pos）OK / 疲劳 TIRED（显式回查）/
            // 无 MOVE NO_BODYPART / noPathFinding 且无缓存 → NOT_FOUND
            expect(
                r["moveTo"].clone(),
                &[-1, 0, 0, -7, -7, 0, 0, -11, -12, -5],
                "moveTo 预检序（creeps.js:158-257 直线桩形态）",
            );
            // moveByPath：RoomPosition 数组（路径首项即当前位 → 下一步方向 3）OK /
            // 不在路径且不邻首项 NOT_FOUND / 序列化串（首落点 (26,25) 的起步格
            // (25,25) 命中）OK / 数字 INVALID_ARGS / 空数组 NOT_FOUND /
            // 非法串抛官方同文案错误（第 6 项是字符串哨兵，单独断言）
            let mbp: Vec<i64> = r["moveByPath"].as_array().unwrap()[..5]
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect();
            assert_eq!(
                &mbp,
                &[0, -5, 0, -10, -5],
                "moveByPath（creeps.js:305-327）"
            );
            assert_eq!(
                r["moveByPath"][5], "threw",
                "非法序列化串抛错（utils.js:584）"
            );
            // notifyWhenAttacked：spawn 覆盖版已存 true → 同值不记（OK）/
            // false 记 intent / 非布尔 INVALID_ARGS / 中性路（本房归 p1）OK /
            // 敌方 tower NOT_OWNER
            expect(
                r["nwa"].clone(),
                &[0, 0, -10, 0, -1],
                "notifyWhenAttacked 两版预检",
            );
            let end = sb.end_tick()?;
            assert_eq!(end["ok"], true);
            // 免费表：say/pull 免费（runtime.js:61）——计费面只有 move/suicide/
            // notifyWhenAttacked(false)/中性路共 4×0.2
            let cpu = end["intentsCpu"].as_f64().unwrap();
            assert!(
                (cpu - 0.8).abs() < 1e-9,
                "免费表生效（say/pull 免费）：{cpu}"
            );
            let intents = sb.take_intents().expect("intentsList 带出");
            assert_eq!(intents["kk1"]["say"]["message"], "hi");
            assert_eq!(intents["kk1"]["say"]["isPublic"], true);
            assert_eq!(
                intents["kk5"]["say"]["message"], "42",
                "数字字符串化（空串拼接 message）"
            );
            assert_eq!(intents["kk1"]["suicide"], json!({}), "空载荷");
            assert_eq!(intents["kk1"]["pull"]["id"], "kk2");
            assert_eq!(
                intents["kk1"]["move"]["direction"], 3,
                "直线方向（getDirectionTo 阈值算法）"
            );
            // 同值差分门：spawn 已存 true，notifyWhenAttacked(true) 不记
            assert!(intents["kk7"].get("notifyWhenAttacked").is_some());
            assert_eq!(
                intents["kk7"]["notifyWhenAttacked"],
                json!({"enabled": false}),
                "只有 false 调用记录（true 与存值相同被差分门拦）"
            );
            assert_eq!(
                intents["kk8"]["notifyWhenAttacked"],
                json!({"enabled": true}),
                "中性路首调恒记（undefined != true）"
            );
            assert!(intents.get("kk9").is_none(), "敌方结构无 intent");
            // 桩清偿面：本族方法不再进 stub 计数
            let stubs = sb.drain_stub_calls()?;
            for name in [
                "creep.moveTo",
                "creep.moveByPath",
                "creep.say",
                "creep.suicide",
                "creep.pull",
                "spawn.notifyWhenAttacked",
                "structure.notifyWhenAttacked",
            ] {
                assert!(stubs.get(name).is_none(), "{name} 已换真，不应再有桩计数");
            }
        }
        _ => panic!("应正常执行"),
    }
    Ok(())
}

/// A5-lite（B3 接线批）：Structure.destroy 的 room intent 通道全链——桩循环
/// 曾遮蔽基类真方法（坑⑥复发，症状假 OK 零 intent）；通道打通后 intentsList
/// 形态与官方 room 级数组通道一致（from_json "room" 分支同形），计费含 room 级。
/// 评审 1 P2-I：**不得独立 #[test]**——sandbox 单测试串行不变量（终局计数器
/// 断言被并行测试污染，实测 5 跑 2 挂）；并入 m0_sandbox_suite 顺序调用。
pub(super) fn destroy_room_intent_channel() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let w = json!({
        "time": 1,
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "k1", "type": "structure", "structureType": "controller",
              "room": "W1N1", "x": 10, "y": 10, "level": 1, "owner": "p1" },
            { "id": "c1", "type": "structure", "structureType": "container",
              "room": "W1N1", "x": 20, "y": 20, "owner": "p1", "hits": 1000 },
            { "id": "w1", "type": "creep", "room": "W1N1", "x": 20, "y": 21,
              "name": "worker", "owner": "p1", "body": ["move"], "spawning": false },
        ],
    });
    sb.begin_tick(&w)?;
    let v = sb.run_expr(
        "Game.rooms['W1N1'].find(FIND_STRUCTURES).find(s=>s.structureType==='container').destroy()",
    )?;
    assert_eq!(v["value"], 0, "destroy 记 intent 返回 OK");
    let v = sb.run_expr("Game.creeps['worker'].move(1)")?;
    assert_eq!(v["value"], 0, "move 对照组（对象级通道）");
    let v = sb.end_tick()?;
    assert_eq!(v["ok"], true, "写回成功: {v:?}");
    let intents = sb.take_intents().expect("intents 带出");
    assert_eq!(
        intents["room"]["destroyStructure"][0]["roomName"], "W1N1",
        "room 级数组通道形态（官方 pushByName('room', ...) 同构）"
    );
    assert_eq!(intents["room"]["destroyStructure"][0]["id"], "c1");
    // room 级计费并入（make.js:100-106 按元素）：move 0.2 + destroy 0.2
    assert_eq!(v["intentsCpu"], 0.4, "room 级意图计入玩家可见计费");
    Ok(())
}

/// B0 WP4 宿主面收口（ADR-013 决策 4）：玩家全局零宿主通道——GPT 复现序列
/// （直调 __kk.beginTick 伪造世界 / 抢先 drainMissing 清仪表 / 清自己刚发的
/// intents）不再可达；宿主生命周期经 Rust 私有 v8::Global 句柄驱动。
/// 含 set_raw_memory 私有通道的正反两面（灰区 :172 销账）：注入走闭包
/// rawMemoryData，玩家换掉 globalThis.RawMemory 后伪造 set 不被执行。
pub(super) fn host_face_closed() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    // ①玩家全局零宿主通道（typeof 判存在性——isFrozen 对非对象在本 realm
    //   恒真、无鉴别力，探针实测）
    for name in ["__kk", "__kkp"] {
        let v = sb.run_expr(&format!("typeof globalThis.{name}"))?;
        assert_eq!(v["value"], "undefined", "{name} 不在玩家全局");
    }
    // ②私有通道注入的正向：set_raw_memory → 下一 tick RawMemory.get() 读回
    sb.set_raw_memory("{\"hostFace\": 7}")?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": []
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {}
        plan => panic!("begin_tick 应为 Run：{plan:?}"),
    }
    let v = sb.run_expr("RawMemory.get()")?;
    assert_eq!(v["value"], "{\"hostFace\": 7}", "私有通道注入生效");
    // ③伪造面不可达：玩家把 globalThis.RawMemory 换成带探针的伪造对象，
    //   tick 外宿主注入不得执行伪造 set（旧路径经全局查找 RawMemory.set）
    sb.end_tick()?;
    sb.run_expr(
        "globalThis.__fakeSetCalls = 0; \
         globalThis.RawMemory = { set: function (s) { globalThis.__fakeSetCalls++; } }; \
         'armed'",
    )?;
    sb.set_raw_memory("{\"hostFace\": 8}")?;
    let v = sb.run_expr("globalThis.__fakeSetCalls")?;
    assert_eq!(v["value"], 0, "伪造 RawMemory.set 未被宿主触发");
    // 私有通道注入仍生效（绕过伪造对象直达闭包）
    sb.set_raw_memory("{\"hostFace\": 9}")?;
    // ④宿主通道经私有句柄照常工作（世界注入/写回）
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {}
        plan => panic!("第二次 begin_tick 应为 Run：{plan:?}"),
    }
    let v = sb.run_expr("RawMemory.get()")?;
    assert_eq!(
        v["value"], "{\"hostFace\": 9}",
        "伪造面存在时私有注入照常生效"
    );
    let out = sb.end_tick()?;
    assert_eq!(out["ok"], true, "私有句柄驱动的 endTick 正常");
    Ok(())
}

/// B0-3 评审 G1 锁：宿主帧零引用依赖 prelude 各段 'use strict'——V8 对
/// strict 帧的 CallSite.getFunction() 返回 undefined（玩家自己的 sloppy 帧
/// 正常透出）。玩家经 Error.prepareStackTrace 能捕到宿主**帧名**（runLoop
/// 等）但拿不到函数本体——若未来某 prelude 段丢掉 'use strict'，此防线会
/// 静默重开 prepareStackTrace 窃取向量，在此显形。
pub(super) fn strict_frame_hides_host_functions() -> anyhow::Result<()> {
    let mut sb = Sandbox::new(SandboxConfig::default())?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        r#"
module.exports.loop = function () {
  Error.prepareStackTrace = function (e, frames) {
    globalThis.__frames = frames.map(function (f) {
      const fn = f.getFunction();
      return (f.getFunctionName() || '?') + ':' + (fn === undefined ? 'hidden' : 'leaked');
    });
  };
  new Error().stack;
};"#,
    );
    sb.set_modules(table)?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": []
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {}
        plan => panic!("begin_tick 应为 Run：{plan:?}"),
    }
    match sb.run_loop()? {
        TickOutcome::Completed { .. } => {}
        _ => panic!("探针 loop 应正常完成"),
    }
    sb.end_tick()?;
    let v = sb.run_expr("globalThis.__frames.join('|')")?;
    let frames = v["value"].as_str().unwrap_or("").to_string();
    assert!(
        frames.contains("runLoop:hidden"),
        "runLoop 宿主帧存在且函数本体被隐藏（实测帧表: {frames}）"
    );
    // 玩家自己的 sloppy 帧透出函数本体是预期（对照组）；宿主帧名（prelude
    // strict IIFE 内）一个都不许 leaked
    for host in [
        "runLoop",
        "writeBackMemory",
        "endTick",
        "runExpr",
        "stringify",
    ] {
        assert!(
            !frames.contains(&format!("{host}:leaked")),
            "宿主帧 {host} 不得透出函数本体（实测帧表: {frames}）"
        );
    }
    Ok(())
}

/// near-heap 触发后的恢复不变量（B0-1 登记、B0-3 补 Ok 路径清偿的锁定）：
/// 无论触发落在哪条路径（硬中断善后 / end_tick Ok / tick-外 drain / 下一次
/// begin_tick 前置 restore），堆上限必须回到配置值且沙箱继续可服务——
/// 「触发但正常完成」不再永久保持放大后的 ceiling。
pub(super) fn nearheap_trigger_then_restore_invariant() -> anyhow::Result<()> {
    let cfg = SandboxConfig {
        heap_limit_bytes: 48 * 1024 * 1024,
        ..SandboxConfig::default()
    };
    let mut sb = Sandbox::new(cfg.clone())?;
    let w = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": []
    });
    match sb.begin_tick(&w)? {
        TickPlan::Run { .. } => {}
        plan => panic!("begin_tick 应为 Run：{plan:?}"),
    }
    // 大分配逼近堆上限：near-heap 回调触发（terminate + 上限放大 limit×2）
    let v = sb.run_expr(
        r#"(function(){ const a = []; for (let i = 0; i < 64; i++) a.push(new Array(1024*1024).fill('x')); return 'done'; })()"#,
    )?;
    // 触发与否取决于 GC 时机——两条出口（done/terminated）都必须恢复
    let out = sb.end_tick()?;
    assert!(
        out.get("ok").and_then(serde_json::Value::as_bool) == Some(true)
            || out.get("terminated").and_then(serde_json::Value::as_bool) == Some(true),
        "end_tick 两条合法出口之一: {out:?}"
    );
    // 恢复不变量：不得停留在放大值 96MB（48×2，回调返回值）。实测常见
    // 72MB（48×1.5——V8 heap 增长因子在 remove 恢复后的残留，随触发时
    // current_limit 波动）；容差 80MB 区分「已恢复」与「永久放大」两态
    assert!(
        sb.heap_size_limit_bytes() <= 80 * 1024 * 1024,
        "触发后堆上限不得停留在放大 ceiling（实测 {} 字节；expr 出口: {v:?}）",
        sb.heap_size_limit_bytes()
    );
    // 沙箱继续可服务（复用不受放大残留影响）
    let v = sb.run_expr("1 + 1")?;
    assert_eq!(v["value"], 2, "触发后沙箱继续可服务");
    Ok(())
}
