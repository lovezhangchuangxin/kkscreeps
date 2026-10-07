//! 验证旗帜和工地的创建、修改、删除、可见性，以及房间查询辅助方法。
//! 每次 `run_players` 都会结算一个 tick。

use std::collections::BTreeMap;

use super::*;

pub(super) fn create_flag_and_site_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" },
                  { "name": "W2N2" }],
    }))?;

    // p2 在 W1N1 摆 creep 授房视野（官方：无对象的房不进 Game.rooms——
    // createFlag 面对不可见房无从发起）
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [30, 31], "owner": "p2", "body": ["move"], "name": "p2eye" }),
    )?;
    // tick1：p1 建旗 + 建工地；p2 同名建旗（per-user 判重不冲突）
    let run = e.run_player(
        "p1",
        r#"
        console.log('f1=' + Game.rooms['W1N1'].createFlag(10, 10, 'home'));
        console.log('sametick=' + (Game.flags['home'] !== undefined && Game.flags['home'].name));
        console.log('shellroom=' + (Game.flags['home'].room && Game.flags['home'].room.name));
        console.log('dup=' + Game.rooms['W1N1'].createFlag(11, 11, 'home'));
        console.log('s1=' + Game.rooms['W1N1'].createConstructionSite(20, 20, 'extension'));
        console.log('pos=' + Game.rooms['W1N1'].createFlag(new RoomPosition(12, 12, 'W1N1'), 'posf', COLOR_RED));
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("f1=home")),
        "{:?}",
        run.console
    );
    // 创建旗帜后，同 tick 的 Game.flags 应立即可读，其 room 指向当前 Room 实例。
    assert!(
        run.console.iter().any(|l| l.contains("sametick=home")),
        "{:?}",
        run.console
    );
    assert!(
        run.console.iter().any(|l| l.contains("shellroom=W1N1")),
        "{:?}",
        run.console
    );
    assert!(
        run.console.iter().any(|l| l.contains("dup=-3")),
        "同 tick 同名 → ERR_NAME_EXISTS"
    );
    assert!(run.console.iter().any(|l| l.contains("s1=0")), "工地 OK");
    assert!(
        run.console.iter().any(|l| l.contains("pos=posf")),
        "RoomPosition 形态转发"
    );
    let _ = e.run_player(
        "p2",
        r#"Game.rooms['W1N1'].createFlag(30, 30, 'home', COLOR_BLUE);"#,
    )?;

    // tick2：双观察者同 tick——p1 见自家旗不见 p2 旗；工地进全集
    let mut codes = BTreeMap::new();
    codes.insert(
        "p1".into(),
        r#"[ !!Game.flags['home'], !!Game.flags['posf'],
             Game.rooms['W1N1'].find(FIND_FLAGS).length,
             Game.rooms['W1N1'].find(FIND_CONSTRUCTION_SITES).length ]"#
            .into(),
    );
    codes.insert(
        "p2".into(),
        r#"[ !!Game.flags['home'], Game.rooms['W1N1'].find(FIND_FLAGS).length ]"#.into(),
    );
    let runs = e.run_players(&codes)?;
    let v1 = &runs["p1"].expr["value"];
    assert_eq!(v1[0], true, "p1 见自家 home 旗");
    assert_eq!(v1[1], true, "p1 见 posf 旗");
    assert_eq!(v1[2], 2, "FIND_FLAGS 只含自家（p2 的 home 不可见）");
    assert_eq!(v1[3], 1, "工地入 FIND 集");
    let v2 = &runs["p2"].expr["value"];
    assert_eq!(v2[0], true, "p2 见自家同名 home 旗（per-user 集合）");
    assert_eq!(v2[1], 1, "p2 的 FIND_FLAGS 只见自家");

    // 旗换色与改位置分两轮调用，避免同 tick 的同名创建意图处理顺序影响本场景。
    let run = e.run_player(
        "p1",
        r#"
        console.log('sc=' + Game.flags['home'].setColor(COLOR_GREEN));
        const sites = Game.rooms['W1N1'].find(FIND_CONSTRUCTION_SITES);
        console.log('rm=' + sites[0].remove());
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("sc=0")),
        "{:?}",
        run.console
    );
    assert!(run.console.iter().any(|l| l.contains("rm=0")));

    // tick4：换色已生效；移位
    let run = e.run_player(
        "p1",
        r#"[ Game.flags['home'].color,
             Game.rooms['W1N1'].find(FIND_CONSTRUCTION_SITES).length,
             Game.flags['home'].setPosition(40, 40) ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 5, "换色生效（COLOR_GREEN=5）");
    assert_eq!(v[1], 0, "工地经 API 删除");
    assert_eq!(v[2], 0, "setPosition OK");
    // tick5：移位生效 + 跨房移位（RoomPosition 形态带目标房名）
    let run = e.run_player(
        "p1",
        r#"[ Game.flags['home'].pos.x, Game.flags['home'].pos.y, Game.flags['home'].setPosition(new RoomPosition(5, 5, 'W2N2')) ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 40, "移位生效 x");
    assert_eq!(v[1], 40, "移位生效 y");
    assert_eq!(v[2], 0);
    // 自家旗帜即使在不可见房仍出现在 Game.flags；旗帜不授予视野，房间不进入 Game.rooms。
    let run = e.run_player(
        "p1",
        r#"[ !!Game.flags['home'], Game.rooms['W2N2'] === undefined, Game.flags['home'].remove() ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], true, "不可见房自家旗仍在 Game.flags");
    assert_eq!(v[1], true, "旗不授房视野");
    assert_eq!(v[2], 0);
    // tick7：删除生效
    let run = e.run_player("p1", r#"!!Game.flags['home']"#)?;
    assert_eq!(run.expr["value"], false, "remove 生效");
    Ok(())
}

pub(super) fn creation_gates_and_helpers_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "c" }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [27, 25], "owner": "p1", "body": ["move"], "name": "far" }),
    )?;

    let run = e.run_player(
        "p1",
        r#"
        // RCL 配额：rcl1 的 extension 配额 0 → 第二个工地 ERR_RCL_NOT_ENOUGH
        console.log('q=' + Game.rooms['W1N1'].createConstructionSite(20, 20, 'extension'));
        // 同格同类互斥（上一 tick 已建的工地进占位检查——同 tick 双建官方
        // API 双 OK、结算静默去重，不在此断言）
        console.log('t1=' + Game.rooms['W1N1'].createConstructionSite(5, 5, 'road'));
        // findClosestByRange：c(25,25) 距离 0 优于 far(27,25)
        console.log('near=' + Game.creeps.c.pos.findClosestByRange(FIND_CREEPS).name);
        // findPath 真路径：range 1 目标环 + 终点回填共两步；首步可直行或
        // 对角（等代价路径由原生 JPS 平局规则决定），断言长度与 dx 族。
        const path = Game.rooms['W1N1'].findPath(Game.creeps.c.pos, new RoomPosition(27, 25, 'W1N1'));
        console.log('path=' + path.length + ':' + path[0].dx + ':' + path[0].direction);
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("q=-14")),
        "{:?}",
        run.console
    );
    assert!(run.console.iter().any(|l| l.contains("t1=0")));
    assert!(run.console.iter().any(|l| l.contains("near=c")));
    let path_line = run
        .console
        .iter()
        .find(|l| l.starts_with("path="))
        .ok_or_else(|| anyhow::anyhow!("缺少 path 行: {:?}", run.console))?;
    assert!(
        path_line.starts_with("path=2:1:"),
        "两步且首步向东（dx=1），实际 {path_line}"
    );

    // 下一 tick：同格同类 → ERR_INVALID_TARGET（工地已进投影）
    let run = e.run_player(
        "p1",
        r#"Game.rooms['W1N1'].createConstructionSite(5, 5, 'road')"#,
    )?;
    assert_eq!(run.expr["value"], -7, "同格同类 ERR_INVALID_TARGET");

    // toJSON：stringify 不含 controller/storage/terminal 键；官方 Room 无
    // mode 字段（rooms.js 构造器不挂），toJSON 拷贝上同样缺席
    let run = e.run_player(
        "p1",
        r#"(s => [ 'controller' in s, 'storage' in s, 'toJSON' in s, 'mode' in s ])(JSON.parse(JSON.stringify(Game.rooms['W1N1'])))"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], false, "controller 键不入 toJSON");
    assert_eq!(v[1], false);
    assert_eq!(v[2], false);
    assert_eq!(v[3], false, "官方无 mode 字段");
    Ok(())
}

pub(super) fn storage_terminal_backfill_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 4, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "storage", "owner": "p1", "store": { "energy": 100 } }),
    )?;

    let run = e.run_player(
        "p1",
        r#"[ !!Game.rooms['W1N1'].storage, Game.rooms['W1N1'].storage.structureType,
             Game.rooms['W1N1'].storage.store.energy, Game.rooms['W1N1'].terminal === undefined,
             'storage' in Game.rooms['W1N1'], 'terminal' in Game.rooms['W1N1'],
             'mode' in Game.rooms['W1N1'], 'survivalInfo' in Game.rooms['W1N1'] ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], true, "storage 回填为结构实例");
    assert_eq!(v[1], "storage");
    assert_eq!(v[2], 100);
    assert_eq!(v[3], true, "无 terminal 为 undefined");
    // 官方挂载面（rooms.js:422-437 / structures.js:41-56）：controller/
    // storage/terminal 仅在结构存在时成为自有属性；无 mode 字段；
    // survivalInfo 恒挂（普通房 undefined）。
    assert_eq!(v[4], true, "storage 存在时为自有属性");
    assert_eq!(v[5], false, "terminal 缺席时键不存在");
    assert_eq!(v[6], false, "官方无 mode");
    assert_eq!(v[7], true, "survivalInfo 恒挂");
    Ok(())
}

/// ID 构造器批（2026-10-08）：官方游戏类构造器吃 id 回查投影并挂
/// id/pos/room 自有属性（官方 creeps.js:8-15 `if(id){data(id);
/// RoomObject.call(...); this.id=id}`）；子类 extends 官方类经 super(id)
/// 保留子类原型；无参构造空实例；未知 id 抛官方文案。
pub(super) fn id_constructor_reconstruction_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 4, "owner": "p1" }],
    }))?;
    let creep_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["move", "work"],
                 "name": "ctor-pin", "store": { "energy": 50 } }),
    )?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [12, 12], "structureType": "spawn", "owner": "p1",
                 "name": "Spawn1", "store": { "energy": 50 } }),
    )?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &r#"(function () {
            class RoleCreep extends Creep { constructor(id) { super(id); } }
            const live = Game.getObjectById(CREEP_ID);
            const made = new Creep(CREEP_ID);
            const role = new RoleCreep(CREEP_ID);
            const empty = new Creep();
            return [
                made instanceof Creep, made.id === live.id, made.name,
                made.pos.x, made.pos.y, made.pos.roomName, made.room.name,
                made.store.energy,
                role instanceof RoleCreep, role instanceof Creep, role.name,
                'id' in empty, Object.keys(empty).length,
            ];
        })()"#
            .replace("CREEP_ID", &format!("{:?}", creep_id)),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], true, "instanceof 构造类");
    assert_eq!(v[1], true, "id 等于活对象");
    assert_eq!(v[2], "ctor-pin", "原型 getter 经 rawData 回查");
    assert_eq!(v[3], 10);
    assert_eq!(v[4], 10);
    assert_eq!(v[5], "W1N1", "pos 自有属性");
    assert_eq!(v[6], "W1N1", "room 取当前 Game 实例");
    assert_eq!(v[7], 50, "store getter 同源");
    assert_eq!(v[8], true, "子类保留子类原型");
    assert_eq!(v[9], true, "子类同时 instanceof 官方类");
    assert_eq!(v[10], "ctor-pin", "super(id) 同款回查");
    assert_eq!(v[11], false, "无参构造空实例");
    assert_eq!(v[12], 0, "空实例无自有键");
    // 未知 id：官方 data(id) 抛同款文案
    let miss = e.run_player(
        "p1",
        r#"(function () { try { new Creep('kkdeadbeef'); return 'no-throw'; } catch (err) { return err.message; } })()"#,
    )?;
    assert_eq!(
        miss.expr["value"], "Could not find an object with ID kkdeadbeef",
        "未知 id 官方文案"
    );
    // strictId 族（sources.js:19-31 无守卫）：假参同样抛错而非空实例；
    // Flag 六参构造器（flags.js:20-26）不挂 id；Structure 族键序 id 在前。
    let extra = e.run_player(
        "p1",
        r#"(function () {
            const strictThrow = (function () { try { new Source(); return 'no-throw'; } catch (err) { return err.message; } })();
            const spawnId = Object.keys(Game.structures)[0];
            const structKeys = Object.keys(new Structure(spawnId));
            const flag = new Flag('f1', 1, 2, 'W1N1', 10, 11);
            return [ strictThrow, structKeys[0],
                     flag.name, flag.color, flag.secondaryColor,
                     flag.pos.x, flag.pos.y, flag.pos.roomName, 'id' in flag ];
        })()"#,
    )?;
    let x = &extra.expr["value"];
    assert_eq!(
        x[0], "Could not find an object with ID undefined",
        "strictId 族假参抛官方文案"
    );
    assert_eq!(
        x[1], "id",
        "Structure 族键序 id 在前（官方 structures.js:39-45）"
    );
    assert_eq!(x[2], "f1");
    assert_eq!(x[3], 1);
    assert_eq!(x[4], 2);
    assert_eq!(x[5], 10);
    assert_eq!(x[6], 11);
    assert_eq!(x[7], "W1N1");
    assert_eq!(x[8], false, "Flag 构造器不挂 id");
    Ok(())
}

/// 对象形状批（2026-10-08）：结构类字段表按官方类层次挂原型，兜底桶
/// 不再把 energy/store 族泄漏到官方无这些 getter 的类型——官方
/// structures.js StructureRoad 仅 ticksToDecay、StructureTower 三件族。
pub(super) fn structure_surface_placement_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 4, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [11, 11], "structureType": "road" }),
    )?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [12, 12], "structureType": "tower", "owner": "p1",
                 "store": { "energy": 10 } }),
    )?;
    let run = e.run_player(
        "p1",
        r#"(function () {
            const objs = Game.rooms['W1N1'].find(FIND_STRUCTURES);
            const road = objs.find(o => o.structureType === 'road');
            const tower = objs.find(o => o.structureType === 'tower');
            return [ 'energy' in road, 'store' in road, 'ticksToDecay' in road,
                     'energy' in tower, 'store' in tower, tower.energy ];
        })()"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], false, "road 无 energy getter");
    assert_eq!(v[1], false, "road 无 store getter");
    assert_eq!(v[2], true, "road 有 ticksToDecay");
    assert_eq!(v[3], true, "tower 有 energy");
    assert_eq!(v[4], true, "tower 有 store");
    assert_eq!(v[5], 10, "tower energy 读 store");
    Ok(())
}
