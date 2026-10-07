//! 验证结构默认容量、衰减和冷却字段如何出现在查询快照与玩家视图中。

use super::*;

pub(super) fn structure_readfaces_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" },
                  { "name": "W2N1" }],
    }))?;

    // setup 契约：默认钟——有主房容器 500（CONTAINER_DECAY_TIME_OWNED）、
    // p1 看不到野房容器，但宿主快照接口可以读取；在玩家运行推进倒计时前先检查初始值。
    let c_owned = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "container", "store": {"energy": 200} }),
    )?)?;
    let c_wild = serde_json::to_string(&e.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "container" }),
    )?)?;
    let ramp = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [11, 11], "structureType": "rampart", "owner": "p1" }),
    )?)?;
    let c_ovr = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [12, 12], "structureType": "container", "ticksToDecay": 5 }),
    )?)?;
    let link = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [13, 13], "structureType": "link", "owner": "p1", "cooldown": 7 }),
    )?)?;
    // 野房容器：p1 无视野（data 面只投影可见+自家破例）——放置后立刻走
    // 快照通道断言（run_player 会结算一 tick，先读后跑）
    let wild_snap = e.get_object(c_wild.trim_matches('"')).expect("野房快照");
    assert_eq!(
        wild_snap["ticksToDecay"], 100,
        "野房容器默认 CONTAINER_DECAY_TIME"
    );
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({c_owned}).ticksToDecay,
                 Game.getObjectById({ramp}).ticksToDecay,
                 Game.getObjectById({c_owned}).store.getUsedCapacity(),
                 Game.getObjectById({c_owned}).storeCapacity,
                 Game.getObjectById({link}).cooldown ]"#
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 500, "有主房容器默认 CONTAINER_DECAY_TIME_OWNED");
    assert_eq!(v[1], 100, "壁垒默认 RAMPART_DECAY_TIME");
    assert_eq!(v[2], 200, "container store 读取面");
    assert_eq!(v[3], 2000, "container storeCapacity");
    assert_eq!(v[4], 7, "link cooldown 回程（放置同 tick）");

    // 覆写钟随 tick 递减；lab 矿物双 getter；Resource.energy 别名
    let lab = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [14, 14], "structureType": "lab", "owner": "p1",
                      "store": {"energy": 100, "U": 40, "L": 60} }),
    )?)?;
    let pile_e = serde_json::to_string(&e.place_object(
        "W1N1",
        "resource",
        &json!({ "pos": [15, 15], "amount": 42 }),
    )?)?;
    let pile_u = serde_json::to_string(&e.place_object(
        "W1N1",
        "resource",
        &json!({ "pos": [16, 16], "amount": 30, "resourceType": "U" }),
    )?)?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({c_ovr}).ticksToDecay,
                 Game.getObjectById({lab}).mineralAmount,
                 Game.getObjectById({lab}).mineralType,
                 Game.getObjectById({lab}).energy,
                 Game.getObjectById({pile_e}).energy,
                 Game.getObjectById({pile_u}).energy,
                 Game.getObjectById({lab}).energyCapacity,
                 Game.getObjectById({lab}).store.getCapacity(),
                 Game.getObjectById({lab}).store.getCapacity('U'),
                 Game.getObjectById({lab}).store.getCapacity('L'),
                 Game.getObjectById({lab}).store.getCapacity('O'),
                 Game.getObjectById({link}).cooldown ]"#
        ),
    )?;
    let v = &run.expr["value"];
    // 覆写 5：run1 与本 run 各结算一 tick（中间又 place+tick(1)）→ 5−2=3；
    // 「一 tick 后 4」的套件原式由 5−elapsed 语义覆盖
    assert_eq!(v[0], 3, "覆写 5 → 两 tick 后 3（差值随 tick 递减）");
    assert_eq!(v[1], 100, "lab mineralAmount=Σstore−energy（40+60）");
    assert_eq!(v[2], "U", "lab mineralType 首个非 energy 正量键");
    assert_eq!(v[3], 100, "lab energy");
    // 42 放置后经一 tick：小堆衰减 ceil(42/1000)=1 → 41（落地堆次 tick 起
    // 衰减——官方 energy/tick.js 语义）
    assert_eq!(v[4], 41, "Resource.energy 别名键（energy 堆=amount−衰减）");
    assert_eq!(
        v[5],
        serde_json::Value::Null,
        "非 energy 堆无别名键（官方 o.energy undefined）"
    );
    // lab.energyCapacity 由 storeCapacityResource.energy 得到，不能读取原始对象的 energyCapacity。
    assert_eq!(v[6], 2000, "lab energyCapacity = LAB_ENERGY_CAPACITY");
    // 存在按资源容量表时，无参数 getCapacity() 返回 null，不能改成返回总容量。
    assert_eq!(
        v[7],
        serde_json::Value::Null,
        "lab store.getCapacity() 为 null"
    );
    // lab 的 store 含矿物时，应为缺失的矿物容量补充 3000。
    // 单查已存矿物容量无法区分默认回退与实际补充，所以下面还检查容量键是否存在。
    assert_eq!(v[8], 3000, "lab getCapacity('U') = LAB_MINERAL_CAPACITY");
    assert_eq!(v[9], 3000, "lab getCapacity('L') 同式（多矿物全填）");
    // 矿物容量已绑定到 U、L 后，未存储的 O 应返回 null；
    // 若漏填这两个容量键，O 会错误地得到默认矿物容量 3000。
    assert_eq!(
        v[10],
        serde_json::Value::Null,
        "lab 未存矿物 getCapacity('O') = null（槽已绑定）"
    );
    // link 的 cooldown 是每 tick 递减计数器（官方 links/tick.js:11-22）：
    // 放置 7 → 两 tick 结算后 5（run1 一 tick + tick(1) 一 tick）
    assert_eq!(v[11], 5, "link cooldown 每 tick 递减（非静态回读 7）");
    // 查询快照也须保留 U、L 容量键，直接检查实际填充，避免默认容量回退掩盖遗漏。
    let lab_snap = e.get_object(lab.trim_matches('"')).expect("lab 快照");
    assert_eq!(
        lab_snap["storeCapacityResource"],
        json!({"energy": 2000, "U": 3000, "L": 3000}),
        "lab 快照 storeCapacityResource 含矿物键（vanilla :1122-1128 填充形态）"
    );

    // 衰减结算全链：短窗容器（ticksToDecay 2）两 tick 后死亡、store 落地成堆
    let c_die = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        // hits 100：一窗衰减（CONTAINER_DECAY 5000）即死——死亡路径
        //（缺省 hits 250k 会存活，只验证钟语义）
        &json!({ "pos": [17, 17], "structureType": "container", "hits": 100,
                      "store": {"energy": 50}, "ticksToDecay": 2 }),
    )?)?;
    e.tick(2)?;
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({c_die}) === null,
                 Game.rooms['W1N1'].find(FIND_DROPPED_RESOURCES)
                   .filter(r => r.pos.x === 17 && r.pos.y === 17)[0].amount ]"#
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], true, "短窗容器死亡");
    assert_eq!(v[1], 50, "store 全量落地成堆");
    Ok(())
}

/// 检查默认容量从对象初始化经世界投影到玩家 getter 的结果。
/// spawn、extension、storage 使用专门的投影逻辑，需与通用结构的默认字段分别验证。
pub(super) fn structure_defaults_probe_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;

    let spawn = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1" }),
    )?)?;
    let ext = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [21, 21], "structureType": "extension", "owner": "p1" }),
    )?)?;
    let storage = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [22, 22], "structureType": "storage", "owner": "p1" }),
    )?)?;

    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({spawn}).energyCapacity,
                 Game.getObjectById({ext}).energyCapacity,
                 Game.getObjectById({storage}).storeCapacity,
                 Game.getObjectById({spawn}).store.getCapacity(),
                 Game.getObjectById({ext}).store.getCapacity() ]"#
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 300, "spawn energyCapacity = SPAWN_ENERGY_CAPACITY");
    assert_eq!(
        v[1], 50,
        "extension energyCapacity = EXTENSION_ENERGY_CAPACITY[rcl=3]"
    );
    assert_eq!(v[2], 1000000, "storage storeCapacity = STORAGE_CAPACITY");
    // 官方 spawn/extension 带 storeCapacityResource → 无参 getCapacity() 读 null
    //（store.js:28-33）
    assert_eq!(
        v[3],
        serde_json::Value::Null,
        "spawn 无参 getCapacity() 为 null"
    );
    assert_eq!(
        v[4],
        serde_json::Value::Null,
        "extension 无参 getCapacity() 为 null"
    );
    Ok(())
}
