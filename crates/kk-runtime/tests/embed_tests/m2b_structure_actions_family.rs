//! M2B 扩面 P5 e2e：结构动作族 API 全链——tower 三 API（错误码表/攻击
//! 落地）、link.transferEnergy（十二步错误码含跨房 NOT_IN_RANGE/成功链
//! 记账+冷却净读）、rampart.setPublic（含敌方 -1）、spawn deprecated 对
//! （canCreateCreep 判定/createCreep 返回 name+幽灵壳+自动名+NAME_EXISTS）。
//! 官方对照：structures.js:764-834/:488-531/:652-657/:898-1051。

use super::*;

pub(super) fn m2b_structure_actions_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        // rcl7：spawn 配额 2（两座 spawn 都 active——rcl5 只许 1 座，
        // 第二座的 createCreep 会被 isActive 结算门拒）、link 配额 4、tower 3
        "rooms": [{ "name": "W1N1", "rcl": 7, "owner": "p1" },
                  { "name": "W2N1" }],
    }))?;

    let tower = serde_json::to_string(&e.place_object(
        "W1N1", "structure",
        &json!({ "pos": [20, 20], "structureType": "tower", "owner": "p1", "store": {"energy": 500} }),
    )?)?;
    let link_a = serde_json::to_string(&e.place_object(
        "W1N1", "structure",
        &json!({ "pos": [30, 30], "structureType": "link", "owner": "p1", "store": {"energy": 400} }),
    )?)?;
    let link_b = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [35, 35], "structureType": "link", "owner": "p1", "store": {"energy": 0} }),
    )?)?;
    let link_full = serde_json::to_string(&e.place_object(
        "W1N1", "structure",
        &json!({ "pos": [36, 36], "structureType": "link", "owner": "p1", "store": {"energy": 800} }),
    )?)?;
    let link_wild = serde_json::to_string(&e.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "link", "owner": "p1", "store": {"energy": 0} }),
    )?)?;
    let ramp = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "rampart", "owner": "p1", "hits": 5000 }),
    )?)?;
    let ramp_enemy = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [26, 26], "structureType": "rampart", "owner": "p2", "hits": 5000 }),
    )?)?;
    let spawn = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "spawn", "owner": "p1",
                      "name": "Spawn1", "store": {"energy": 300} }),
    )?)?;
    let spawn2 = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [11, 10], "structureType": "spawn", "owner": "p1",
                      "name": "Spawn2", "store": {"energy": 300} }),
    )?)?;
    let foe = serde_json::to_string(&e.place_object(
        "W1N1", "creep",
        &json!({ "pos": [21, 21], "owner": "p2", "body": ["move","move","move","move","move","move","move","move"], "name": "foe" }),
    )?)?;

    // API 错误码全表（structures.js 官方预检序逐项）+ 成功链
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({tower}).heal(Game.getObjectById({ramp})),
                 Game.getObjectById({tower}).attack(null),
                 Game.getObjectById({tower}).attack(Game.getObjectById({foe})),
                 Game.getObjectById({link_a}).transferEnergy(Game.getObjectById({link_wild}), 100),
                 Game.getObjectById({link_a}).transferEnergy(Game.getObjectById({link_full}), 100),
                 Game.getObjectById({link_a}).transferEnergy(Game.getObjectById({link_b}), 999999),
                 Game.getObjectById({link_a}).transferEnergy(Game.getObjectById({link_b}), -1),
                 Game.getObjectById({ramp_enemy}).setPublic(true),
                 Game.getObjectById({ramp}).setPublic(true),
                 Game.getObjectById({spawn}).canCreateCreep(['move'], 'X'),
                 Game.getObjectById({spawn}).createCreep(['move'], 'X'),
                 Game.getObjectById({spawn}).createCreep(['move'], 'X'),
                 typeof Game.getObjectById({spawn2}).createCreep(['move']),
                 Game.getObjectById({spawn2}).createCreep(['move']) === 'X',
                 Memory.creeps.X === undefined,
                 Game.getObjectById({link_a}).transferEnergy(Game.getObjectById({link_b}), 100) ]"#
        ),
    )?;
    let v = &run.expr["value"];
    let err = |code: i64| serde_json::json!(code);
    assert_eq!(v[0], err(-7), "heal 结构目标 → ERR_INVALID_TARGET");
    assert_eq!(v[1], err(-7), "attack(null) → ERR_INVALID_TARGET");
    assert_eq!(v[2], err(0), "attack 敌 creep → OK");
    assert_eq!(
        v[3],
        err(-9),
        "跨房 transfer → ERR_NOT_IN_RANGE（同房限定）"
    );
    assert_eq!(v[4], err(-8), "目标满仓 → ERR_FULL");
    assert_eq!(v[5], err(-6), "amount 超源能量 → ERR_NOT_ENOUGH_ENERGY");
    assert_eq!(v[6], err(-10), "负 amount → ERR_INVALID_ARGS");
    assert_eq!(v[7], err(-1), "敌方 rampart → ERR_NOT_OWNER");
    assert_eq!(v[8], err(0), "自家 rampart.setPublic → OK");
    assert_eq!(v[9], err(0), "canCreateCreep → OK");
    assert_eq!(v[10], json!("X"), "createCreep 带名返回 name");
    assert_eq!(v[11], err(-3), "同 tick 同名再建 → ERR_NAME_EXISTS");
    assert_eq!(
        v[12],
        json!("string"),
        "createCreep 缺省名 → 自动名（names.js 池；第二座 spawn 独立 intent 槽）"
    );
    assert_eq!(v[13], json!(false), "自动名不取 'X'（名池固定 100+100）");
    // 官方 Memory getter 首访解析后固化：createCreep 写的 Memory.creeps.X
    // 对同 tick 后续访问可见（P5 勘正：原直调 ensureMemory 强制重解析丢写）
    assert_eq!(
        v[14],
        json!(false),
        "Memory.creeps.X 同 tick 可见（首访固化语义）"
    );
    assert_eq!(v[15], err(0), "transfer 100 → OK");

    // 幽灵壳同 tick 可读（官方 :1132-1213：Game.creeps[name].spawning true）
    let run = e.run_player(
        "p1",
        r#"[ Game.creeps.X ? Game.creeps.X.spawning : 'absent',
             Game.creeps.X ? Game.creeps.X.body.length : -1 ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!(true), "幽灵壳 spawning:true 同 tick 可读");
    assert_eq!(v[1], json!(1), "幽灵壳 body 长度");

    // 结算落地：transfer tick 一次 + tick(1) 一次
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({foe}).hits,
                 Game.getObjectById({tower}).energy,
                 Game.getObjectById({link_a}).energy,
                 Game.getObjectById({link_a}).cooldown,
                 Game.getObjectById({link_b}).energy,
                 Game.getObjectById({ramp}).isPublic,
                 Game.creeps.X === undefined,
                 Game.creeps.X ? Game.creeps.X.spawning : null,
                 Game.getObjectById({spawn}).spawning,
                 Object.keys(Game.creeps).length ]"#
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(
        v[0],
        json!(200),
        "800−600（range 1 全功率，延迟伤害同 tick 落地）"
    );
    assert_eq!(v[1], json!(490), "塔扣 TOWER_ENERGY_COST");
    assert_eq!(v[2], json!(300), "源 link 扣 100");
    // 冷却：range |35−30|=5 → 挂 5；其后每个结算 tick 的 phase 8 递减 1
    //（transfer tick 自身 + 幽灵壳 run + tick(1)）→ 净读 2
    assert_eq!(
        v[3],
        json!(2),
        "冷却净读 5−3（intent 期挂钟、逐 tick 递减）"
    );
    assert_eq!(v[4], json!(97), "目标 100−ceil(3%)=97");
    assert_eq!(v[5], json!(true), "setPublic 落地");
    assert_eq!(v[6], json!(false), "createCreep('X') 结算：chick 可见");
    assert_eq!(v[7], json!(false), "X 已出壳（3 tick 窗过）");
    assert_eq!(
        v[8],
        serde_json::Value::Null,
        "Spawn1 空闲（X 出壳后 spawning 清空）"
    );
    assert_eq!(
        v[9],
        json!(2),
        "本人 creep 两只：X + 自动名（均出壳；Game.creeps 不含敌方 foe）"
    );
    Ok(())
}
