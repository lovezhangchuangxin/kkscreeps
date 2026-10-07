//! 常量注入与规则门族：覆盖值平铺、战斗射程四键（G07.a）与移动
//! 玩家门两键（G02.d2）的玩家面预检行为。
use super::*;

/// 常量覆盖通道：默认保持嵌入常量表原值；宿主覆盖在平铺前生效。
pub(super) fn constant_overrides_reach_player_globals_impl() -> anyhow::Result<()> {
    let mut plain = Sandbox::new(SandboxConfig::default())?;
    let v = plain.run_expr("LINK_CAPACITY")?;
    assert_eq!(v["value"], 800, "默认保持常量表原值");

    let mut config = SandboxConfig::default();
    config
        .constant_overrides
        .insert("LINK_CAPACITY".into(), json!(400));
    let mut sb = Sandbox::new(config)?;
    let v = sb.run_expr("LINK_CAPACITY")?;
    assert_eq!(v["value"], 400, "覆盖值平铺到玩家全局");

    // 覆盖键不在常量表时初始化即失败（fail fast，不静默忽略）。
    let mut bad = SandboxConfig::default();
    bad.constant_overrides
        .insert("NOT_A_CONSTANT".into(), json!(1));
    let err = Sandbox::new(bad).err().expect("未知覆盖键须拒绝");
    assert!(err.to_string().contains("NOT_A_CONSTANT"), "{err}");
    Ok(())
}

/// 战斗射程常量：默认为官方原值；四键覆盖平铺后，prelude 的
/// attack/rangedAttack/heal/rangedHeal 射程门观察覆盖值放行。
pub(super) fn combat_range_constants_impl() -> anyhow::Result<()> {
    let combat_world = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": vec![0u8; 2500] } },
        "objects": [
            { "id": "a1", "type": "creep", "room": "W1N1", "x": 10, "y": 10, "name": "atk",
              "owner": "p1", "body": ["attack", "move"], "store": { "energy": 0 } },
            { "id": "ra1", "type": "creep", "room": "W1N1", "x": 10, "y": 30, "name": "ratk",
              "owner": "p1", "body": ["ranged_attack", "move"], "store": { "energy": 0 } },
            { "id": "h1", "type": "creep", "room": "W1N1", "x": 20, "y": 20, "name": "medic",
              "owner": "p1", "body": ["heal", "move"], "store": { "energy": 0 } },
            { "id": "t1", "type": "creep", "room": "W1N1", "x": 12, "y": 10, "name": "foe",
              "owner": "p2", "body": ["move"], "store": { "energy": 0 } },
            { "id": "t2", "type": "creep", "room": "W1N1", "x": 14, "y": 30, "name": "foe2",
              "owner": "p2", "body": ["move"], "store": { "energy": 0 } },
            { "id": "w1", "type": "creep", "room": "W1N1", "x": 22, "y": 20, "name": "ally",
              "owner": "p1", "body": ["move"], "store": { "energy": 0 } },
        ]
    });
    // 目标经显式 ID 获取：不依赖 find(FIND_CREEPS) 的数组顺序。
    let call = |expr: &str| {
        format!(
            "(() => {{ const near = Game.getObjectById('t1'); \
         const far = Game.getObjectById('t2'); \
         const ally = Game.creeps['ally']; \
         {expr} }})()"
        )
    };

    // 默认：官方射程（1/3/1/3）下距离 2 的近战与距离 4 的远程均被拒。
    let mut plain_table = ModuleTable::new();
    plain_table.insert_js("main", "module.exports.loop = function () {};");
    let mut plain = Sandbox::new(SandboxConfig::default())?;
    plain.set_modules(plain_table)?;
    tick(&mut plain, &combat_world)?;
    let v = plain.run_expr(&call(
        "return [Game.creeps['atk'].attack(near), Game.creeps['ratk'].rangedAttack(far)].join(',');",
    ))?;
    assert_eq!(v["value"], "-9,-9", "默认射程：距离 2/4 均在射程外");

    // 覆盖四键：平铺生效且四个 prelude 射程门都按覆盖值放行。
    let mut config = SandboxConfig::default();
    for (name, value) in [
        ("ATTACK_RANGE", 3),
        ("RANGED_ATTACK_RANGE", 5),
        ("HEAL_RANGE", 3),
        ("RANGED_HEAL_RANGE", 5),
    ] {
        config.constant_overrides.insert(name.into(), json!(value));
    }
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    let mut sb = Sandbox::new(config)?;
    sb.set_modules(table)?;
    tick(&mut sb, &combat_world)?;
    let v = sb
        .run_expr("[ATTACK_RANGE, RANGED_ATTACK_RANGE, HEAL_RANGE, RANGED_HEAL_RANGE].join(',')")?;
    assert_eq!(v["value"], "3,5,3,5", "覆盖值平铺到玩家全局");
    let v = sb.run_expr(&call(
        "return [\
             Game.creeps['atk'].attack(near), \
             Game.creeps['ratk'].rangedAttack(far), \
             Game.creeps['medic'].heal(ally), \
             Game.creeps['medic'].rangedHeal(ally)\
         ].join(',');",
    ))?;
    assert_eq!(
        v["value"], "0,0,0,0",
        "覆盖后：近战距离 2、远程距离 4、治疗距离 2 均放行"
    );
    Ok(())
}

/// G02.d2 移动玩家门常量：默认即官方（方向形态 move 的 TIRED/
/// NO_BODYPART 返回码钉死——此前仅 moveTo 形态有断言），覆盖后
/// move()/moveTo() 门与常量平铺同源放行，两门独立。
pub(super) fn move_gate_constants_impl() -> anyhow::Result<()> {
    let gate_world = json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": vec![0u8; 2500] } },
        "objects": [
            { "id": "g1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "tired",
              "owner": "p1", "body": ["move", "work"], "fatigue": 2 },
            { "id": "g2", "type": "creep", "room": "W1N1", "x": 25, "y": 24, "name": "legless",
              "owner": "p1", "body": ["work", "carry"] },
        ],
    });

    // 默认：官方返回码与常量平铺。
    let mut plain_table = ModuleTable::new();
    plain_table.insert_js("main", "module.exports.loop = function () {};");
    let mut plain = Sandbox::new(SandboxConfig::default())?;
    plain.set_modules(plain_table)?;
    tick(&mut plain, &gate_world)?;
    let v = plain.run_expr(
        "[Game.creeps['tired'].move(BOTTOM), Game.creeps['legless'].move(TOP), \
         MOVE_FATIGUE_LIMIT, MOVE_MIN_ACTIVE_MOVES].join(',')",
    )?;
    assert_eq!(
        v["value"], "-11,-12,0,1",
        "默认门：TIRED/NO_BODYPART 与官方常量"
    );
    let v = plain.run_expr("Game.creeps['tired'].moveTo(25, 30)")?;
    assert_eq!(v["value"], -11, "默认 moveTo：疲劳拒绝");
    let v = plain.run_expr("Game.creeps['legless'].moveTo(25, 30)")?;
    assert_eq!(v["value"], -12, "默认 moveTo：无 MOVE 拒绝");

    // 覆盖 fatigueLimit=100：疲劳 creep 方向形态与 moveTo 均过门；
    // 无 MOVE 者仍被独立门拒绝。
    let mut config = SandboxConfig::default();
    config
        .constant_overrides
        .insert("MOVE_FATIGUE_LIMIT".into(), json!(100));
    let mut table = ModuleTable::new();
    table.insert_js("main", "module.exports.loop = function () {};");
    let mut sb = Sandbox::new(config)?;
    sb.set_modules(table.clone())?;
    tick(&mut sb, &gate_world)?;
    let v = sb.run_expr(
        "[MOVE_FATIGUE_LIMIT, Game.creeps['tired'].move(BOTTOM), \
         Game.creeps['tired'].moveTo(25, 30), Game.creeps['legless'].move(TOP)].join(',')",
    )?;
    assert_eq!(
        v["value"], "100,0,0,-12",
        "fatigueLimit 覆盖：疲劳门放行、MOVE 门独立"
    );

    // 覆盖 minActiveMoves=0：无 MOVE 者两形态均过门；疲劳门独立。
    let mut config = SandboxConfig::default();
    config
        .constant_overrides
        .insert("MOVE_MIN_ACTIVE_MOVES".into(), json!(0));
    let mut sb = Sandbox::new(config)?;
    sb.set_modules(table)?;
    tick(&mut sb, &gate_world)?;
    let v = sb.run_expr(
        "[MOVE_MIN_ACTIVE_MOVES, Game.creeps['legless'].move(TOP), \
         Game.creeps['legless'].moveTo(25, 30), Game.creeps['tired'].move(BOTTOM)].join(',')",
    )?;
    assert_eq!(
        v["value"], "0,0,0,-11",
        "minActiveMoves=0：MOVE 门放行、疲劳门独立"
    );
    Ok(())
}
