//! powerBank 域集成：放置归一→快照折叠→玩家面攻击反射→衰变→战毁
//! 废墟→泼洒全链（套件 14.3/18.2/26.0/inspection 同形），以及官方
//! genPowerBanks 生成器调度与检查点保存恢复。
//! 行为锚点：backend cronjobs.js:194-243、engine _damage.js:35-41、
//! processor.js:421-426、_destroy.js、ruins/tick.js。
use super::*;
use kk_core::constants as c;

const ROOM: &str = "W1N1";
/// 高速路房（E0 贴 x 边界线）。
const HIGHWAY: &str = "E0N5";

fn plain_rooms(extra: Vec<Value>) -> Value {
    let mut rooms = vec![json!({ "name": ROOM, "terrain": vec![0u8; 2500] })];
    rooms.extend(extra);
    json!({ "players": ["p1", "p2"], "rooms": rooms })
}

fn owned_rooms(owner: &str, rcl: i64) -> Value {
    json!({ "players": ["p1"], "rooms": [
        { "name": ROOM, "terrain": vec![0u8; 2500], "owner": owner, "rcl": rcl }
    ]})
}

/// 高速路房地形：默认平原，仅给定格为墙（生成器放置循环的落点目标）。
fn wall_terrain(walls: &[(usize, usize)]) -> Vec<u8> {
    let mut terrain = vec![0u8; 2500];
    for &(x, y) in walls {
        terrain[y * 50 + x] = kk_core::TERRAIN_MASK_WALL;
    }
    terrain
}

/// 双通道引擎（raid 批惯例：生成器用例 JSON/KKPV 都要过）。
fn engine_on(binary: bool) -> EmbedEngine {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e
}

/// 房内 powerBank 结构（FIND_STRUCTURES 含 create_shard 自动放的控制器，须过滤）。
fn banks_in(e: &EmbedEngine, room: &str) -> Vec<Value> {
    e.find_in_room(room, c::FIND_STRUCTURES)
        .into_iter()
        .filter(|s| s["structureType"] == json!("powerBank"))
        .collect()
}

fn place_bank(e: &mut EmbedEngine, spec: Value) -> anyhow::Result<String> {
    e.place_object(ROOM, "powerBank", &spec)
}

/// POWER-BANK-001 同形：1×ATTACK+4×TOUGH+MOVE（300 HP）近战打银行，
/// 反射 30×0.5=15；银行耐久同步扣 30。
#[test]
fn hitback_reflects_half_damage_to_attacker() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&plain_rooms(vec![]))?;
    let pb = place_bank(
        &mut e,
        json!({
            "pos": [25, 25], "power": 1000, "hits": c::POWER_BANK_HITS, "ticksToDecay": 50000
        }),
    )?;
    let attacker = e.place_object(
        ROOM,
        "creep",
        &json!({
            "pos": [25, 26], "owner": "p2",
            "body": ["attack", "tough", "tough", "tough", "tough", "move"]
        }),
    )?;
    let run = e.run_player(
        "p2",
        &format!("Game.getObjectById('{attacker}').attack(Game.getObjectById('{pb}'))"),
    )?;
    assert_eq!(run.expr["value"], json!(0), "attack 返回 OK");
    e.tick(1)?;
    assert_eq!(
        e.get_object(&attacker).unwrap()["hits"],
        json!(585),
        "600-15；套件注释的 300 是笔误"
    );
    assert_eq!(
        e.get_object(&pb).unwrap()["hits"],
        json!(c::POWER_BANK_HITS - 30),
        "银行扣 30"
    );
    Ok(())
}

/// POWER-BANK-002 同形：ticksToDecay 3 → 读数 [2,1,null]；移除不留废墟
/// 不泼资源。
#[test]
fn decay_reads_two_one_then_removes() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&owned_rooms("p1", 1))?;
    let pb = place_bank(
        &mut e,
        json!({
            "pos": [25, 25], "power": 500, "hits": c::POWER_BANK_HITS, "ticksToDecay": 3
        }),
    )?;
    e.tick(1)?;
    let mut readings = Vec::new();
    for _ in 0..3 {
        let run = e.run_player(
            "p1",
            &format!("Game.getObjectById('{pb}')?.ticksToDecay ?? null"),
        )?;
        readings.push(run.expr["value"].clone());
    }
    assert_eq!(readings, vec![json!(2), json!(1), json!(null)]);
    assert_eq!(
        e.find_in_room(ROOM, c::FIND_RUINS).len(),
        0,
        "衰变移除不产废墟"
    );
    assert_eq!(
        e.find_in_room(ROOM, c::FIND_DROPPED_RESOURCES).len(),
        0,
        "衰变移除不泼资源"
    );
    Ok(())
}

/// POWER-BANK-004 + RUIN-002:powerBank 同形：hits 1 战毁 → 废墟
///（structureType=powerBank、RUIN_DECAY_STRUCTURES=10）→ 废墟到期前
/// 一拍把 store.power 泼成 (25,25) 的资源堆。
#[test]
fn destroyed_bank_leaves_ruin_then_spills_power() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&owned_rooms("p1", 1))?;
    let pb = place_bank(
        &mut e,
        json!({
            "pos": [25, 25], "power": 500, "hits": 1, "ticksToDecay": 50000
        }),
    )?;
    let attacker = e.place_object(
        ROOM,
        "creep",
        &json!({
            "pos": [25, 26], "owner": "p1",
            "body": ["attack", "attack", "attack", "attack", "move"]
        }),
    )?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!("Game.getObjectById('{attacker}').attack(Game.getObjectById('{pb}'))"),
    )?;
    assert_eq!(run.expr["value"], json!(0));
    // 战毁拍 T+1 读废墟：ticksToDecay 9、structureType powerBank。
    let run = e.run_player(
        "p1",
        r#"
        const r = Game.rooms['W1N1'].lookForAt(LOOK_RUINS, 25, 25)[0];
        r ? { structureType: r.structure.structureType, ticksToDecay: r.ticksToDecay } : null
    "#,
    )?;
    assert_eq!(
        run.expr["value"],
        json!({ "structureType": "powerBank", "ticksToDecay": 9 })
    );
    // 再 8 拍到 T+10：ruins/tick 的 `time >= decayTime-1` 泼洒并移除，
    // 读取发生在泼洒拍——资源堆尚未开始衰减，恰为 500。
    e.tick(8)?;
    let pile = e
        .find_in_room(ROOM, c::FIND_DROPPED_RESOURCES)
        .into_iter()
        .find(|d| {
            d["resourceType"] == json!("power")
                && d["pos"]["x"] == json!(25)
                && d["pos"]["y"] == json!(25)
        })
        .expect("泼洒出 power 资源堆");
    assert_eq!(pile["amount"], json!(500));
    assert!(e.get_object(&pb).is_none(), "原银行对象已消失");
    assert_eq!(e.find_in_room(ROOM, c::FIND_RUINS).len(), 0, "废墟已移除");
    Ok(())
}

/// inspection 同形：快照折叠 power/hitsMax/ticksToDecay，显式 hits 不改
/// hitsMax（恒 POWER_BANK_HITS）。
#[test]
fn snapshot_exposes_power_and_folded_decay() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&plain_rooms(vec![]))?;
    let pb = place_bank(
        &mut e,
        json!({
            "pos": [25, 25], "power": 2500, "hits": 1000000, "ticksToDecay": 500
        }),
    )?;
    e.tick(1)?;
    let obj = e.get_object(&pb).unwrap();
    assert_eq!(obj["kind"], json!("structure"));
    assert_eq!(obj["structureType"], json!("powerBank"));
    assert_eq!(obj["power"], json!(2500));
    assert_eq!(obj["hits"], json!(1000000));
    assert_eq!(obj["hitsMax"], json!(2_000_000));
    assert_eq!(obj["ticksToDecay"], json!(499), "500-1 拍后");
    Ok(())
}

/// NPC-OWNERSHIP 同形（SHAPE-NPC-003 的 10 键全表面枚举由套件 26.0
/// 权威执行，此处镜像核心派生键）：my=false、owner='Power Bank'，
/// hits/hitsMax/structureType 经基类原型链可读。
#[test]
fn player_surface_owner_and_shape() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&owned_rooms("p1", 1))?;
    let pb = place_bank(&mut e, json!({ "pos": [25, 25], "power": 500 }))?;
    e.tick(1)?;
    let run = e.run_player(
        "p1",
        &format!(
            "const s = Game.getObjectById('{pb}'); \
             [s.my, s.owner && s.owner.username, s.structureType, s.hits, s.hitsMax, s.power]"
        ),
    )?;
    assert_eq!(
        run.expr["value"],
        json!([false, "Power Bank", "powerBank", 2_000_000, 2_000_000, 500])
    );
    Ok(())
}

/// 生成器首轮调度：高速路房只写到期 tick 不生成对象（官方 L200-204）；
/// 到期滚轮 round(0.5×25000+37500)=50000。
#[test]
fn spawner_first_run_schedules_highway_room_only() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&plain_rooms(vec![json!({
        "name": HIGHWAY, "terrain": vec![0u8; 2500]
    })]))?;
    e.run_power_bank_spawner(Some(vec![0.5]))?;
    let saved = e.checkpoint(Value::Null)?;
    let now = saved["engine"]["world"]["time"].as_u64().unwrap();
    assert_eq!(
        saved["engine"]["world"]["rooms"][HIGHWAY]["power_bank_time"],
        json!(now + 50_000),
        "round(0.5*25000+37500)，锚定世界时间（create_shard 后非 0 起）"
    );
    assert_eq!(
        saved["engine"]["world"]["rooms"][ROOM]["power_bank_time"],
        json!(null),
        "非高速路房不进官方 {{bus:true}} 查询"
    );
    assert_eq!(banks_in(&e, HIGHWAY).len(), 0);
    Ok(())
}

/// 非高速路房不消耗任何随机（官方查询先按 bus 过滤，掷轮只对合格房）。
#[test]
fn spawner_skips_non_highway_without_consuming_random() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&plain_rooms(vec![]))?;
    // 空序列：任何消耗都会报错。
    e.run_power_bank_spawner(Some(vec![]))?;
    Ok(())
}

/// 到期生成：位置 do/while 落在墙体格（3×3 有出口邻格）、容量掷骰与
/// 暴击、对象形态（store.power/hits=hitsMax=2M/decayTime=+5000）、
/// 到期时间重置为本次滚轮值。
#[test]
fn spawner_spawns_due_bank_on_wall_with_exit() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": HIGHWAY, "terrain": wall_terrain(&[(10, 10)]) }]
        }))?;
        let now = e.checkpoint(Value::Null)?["engine"]["world"]["time"]
            .as_u64()
            .unwrap();
        e.set_power_bank_state(HIGHWAY, &json!({ "powerBankTime": now }))?;
        // 0.125×40+5=10；容量 floor(0.5×4500+500)=2750；暴击 0.9 不触发。
        e.run_power_bank_spawner(Some(vec![0.0, 0.125, 0.125, 0.5, 0.9]))?;
        let structures = banks_in(&e, HIGHWAY);
        assert_eq!(structures.len(), 1, "恰一个银行");
        let bank = &structures[0];
        assert_eq!(bank["structureType"], json!("powerBank"));
        assert_eq!(bank["pos"]["x"], json!(10));
        assert_eq!(bank["pos"]["y"], json!(10));
        assert_eq!(bank["power"], json!(2750), "快照折叠 store.power");
        assert_eq!(bank["hits"], json!(c::POWER_BANK_HITS));
        assert_eq!(bank["ticksToDecay"], json!(c::POWER_BANK_DECAY), "0+5000");
        let saved = e.checkpoint(Value::Null)?;
        let now = saved["engine"]["world"]["time"].as_u64().unwrap();
        assert_eq!(
            saved["engine"]["world"]["rooms"][HIGHWAY]["power_bank_time"],
            json!(now + 37_500),
            "到期重置为滚轮值 round(0*25000+37500)"
        );
    }
    Ok(())
}

/// 位置重掷：首掷平原格不合格，第二掷落墙；暴击掷 <0.3 时容量加一份
/// MAX（官方 L228-230）。
#[test]
fn spawner_retries_position_and_crit_doubles_capacity() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": HIGHWAY, "terrain": wall_terrain(&[(20, 20)]) }]
        }))?;
        let now = e.checkpoint(Value::Null)?["engine"]["world"]["time"]
            .as_u64()
            .unwrap();
        e.set_power_bank_state(HIGHWAY, &json!({ "powerBankTime": now }))?;
        // 首掷 (5,5) 平原拒绝；0.375×40+5=20 落墙；容量 floor(0×4500+500)=500；
        // 暴击 0.1 < 0.3 → 500+5000=5500。
        e.run_power_bank_spawner(Some(vec![0.0, 0.0, 0.0, 0.375, 0.375, 0.0, 0.1]))?;
        let structures = banks_in(&e, HIGHWAY);
        assert_eq!(structures.len(), 1);
        let bank = &structures[0];
        assert_eq!(bank["pos"]["x"], json!(20));
        assert_eq!(bank["pos"]["y"], json!(20));
        assert_eq!(bank["power"], json!(5500), "500 + 暴击 5000");
    }
    Ok(())
}

/// 未到期只消耗滚轮、不生成、不重写到期时间（官方 L206-208 直接
/// continue；respawnTime 已在循环头掷出但未使用）。
#[test]
fn spawner_not_due_consumes_only_respawn_roll() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": HIGHWAY, "terrain": wall_terrain(&[(10, 10)]) }]
        }))?;
        // 播种锚定世界时间（create_shard 后时间起点不是固定值）。
        let now = e.checkpoint(Value::Null)?["engine"]["world"]["time"]
            .as_u64()
            .unwrap();
        e.set_power_bank_state(HIGHWAY, &json!({ "powerBankTime": now + 100 }))?;
        // 恰一枚滚轮；任何位置掷骰都会耗尽序列报错。
        e.run_power_bank_spawner(Some(vec![0.5]))?;
        assert_eq!(banks_in(&e, HIGHWAY).len(), 0);
        let saved = e.checkpoint(Value::Null)?;
        assert_eq!(
            saved["engine"]["world"]["rooms"][HIGHWAY]["power_bank_time"],
            json!(now + 100),
            "未到期分支不重写"
        );
    }
    Ok(())
}

/// 序列耗尽报错且候选整体回滚：对象与到期时间都保持原值。
#[test]
fn spawner_sequence_exhaustion_keeps_world() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": HIGHWAY, "terrain": wall_terrain(&[(10, 10)]) }]
        }))?;
        let now = e.checkpoint(Value::Null)?["engine"]["world"]["time"]
            .as_u64()
            .unwrap();
        e.set_power_bank_state(HIGHWAY, &json!({ "powerBankTime": now }))?;
        // 只有滚轮，位置掷骰耗尽。
        let err = e
            .run_power_bank_spawner(Some(vec![0.0]))
            .expect_err("序列不足须失败");
        assert!(err.to_string().contains("随机序列耗尽"), "{err}");
        assert_eq!(banks_in(&e, HIGHWAY).len(), 0);
        let saved = e.checkpoint(Value::Null)?;
        assert_eq!(
            saved["engine"]["world"]["rooms"][HIGHWAY]["power_bank_time"],
            json!(now),
            "候选回滚：到期时间保持"
        );
    }
    Ok(())
}

/// 调度随完整检查点保存恢复（v8 域）；恢复世界的生成器继续按到期时间
/// 生成等值对象（同随机流）。
#[test]
fn schedule_survives_cold_restore_and_drives_spawn() -> anyhow::Result<()> {
    let build = |binary: bool| -> anyhow::Result<EmbedEngine> {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": HIGHWAY, "terrain": wall_terrain(&[(10, 10)]) }]
        }))?;
        let now = e.checkpoint(Value::Null)?["engine"]["world"]["time"]
            .as_u64()
            .unwrap();
        e.set_power_bank_state(HIGHWAY, &json!({ "powerBankTime": now }))?;
        Ok(e)
    };
    for binary in [false, true] {
        let e = build(binary)?;
        let saved = e.checkpoint(Value::Null)?;
        assert_eq!(saved["checkpoint_format_version"], json!(8));
        assert!(
            saved["engine"]["world"]["rooms"][HIGHWAY]["power_bank_time"]
                .as_u64()
                .is_some(),
            "播种的到期调度随载荷保存（非 null）"
        );
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        restored.run_power_bank_spawner(Some(vec![0.0, 0.125, 0.125, 0.5, 0.9]))?;
        let structures = banks_in(&restored, HIGHWAY);
        assert_eq!(structures.len(), 1, "恢复的调度驱动生成");
        assert_eq!(structures[0]["power"], json!(2750));
    }
    Ok(())
}

/// 病态地形护栏（对抗评审 P1-1）：全平原高速路房 + 种子流（无限流）
/// 重掷 10_000 次后报错回滚，不挂死服务——官方依赖 world-gen 保证
/// 合格格，kk 地形是任意管理输入，前提不成立。
#[test]
fn spawner_retry_cap_errors_on_pathological_terrain() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": HIGHWAY, "terrain": vec![0u8; 2500] }]
        }))?;
        let now = e.checkpoint(Value::Null)?["engine"]["world"]["time"]
            .as_u64()
            .unwrap();
        e.set_power_bank_state(HIGHWAY, &json!({ "powerBankTime": now }))?;
        let err = e
            .run_power_bank_spawner(None)
            .expect_err("全平原无合格格须报错");
        assert!(err.to_string().contains("重掷耗尽"), "{err}");
        assert_eq!(banks_in(&e, HIGHWAY).len(), 0);
    }
    Ok(())
}

/// 多房交织原子回滚：房 A 序列内完成生成、房 B 耗尽 → 整轮 Err 后
/// 两房都保持原状（edit_world 候选整体回滚，房名序 BTreeMap E0N1<E0N5）。
#[test]
fn spawner_multi_room_interleave_rolls_back_atomically() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [
            { "name": "E0N1", "terrain": wall_terrain(&[(10, 10)]) },
            { "name": "E0N5", "terrain": vec![0u8; 2500] }
        ]
    }))?;
    let now = e.checkpoint(Value::Null)?["engine"]["world"]["time"]
        .as_u64()
        .unwrap();
    e.set_power_bank_state("E0N1", &json!({ "powerBankTime": now }))?;
    e.set_power_bank_state("E0N5", &json!({ "powerBankTime": now }))?;
    // E0N1 完整序列后 E0N5 只有滚轮与两次位置掷即耗尽。
    let err = e
        .run_power_bank_spawner(Some(vec![
            0.0, 0.125, 0.125, 0.5, 0.9, // E0N1 生成
            0.0, 0.0, 0.0, // E0N5 滚轮 + 两次位置掷 → 耗尽
        ]))
        .expect_err("第二房序列不足须整轮失败");
    assert!(err.to_string().contains("随机序列耗尽"), "{err}");
    assert_eq!(banks_in(&e, "E0N1").len(), 0, "房 A 已生成也随整轮回滚");
    let saved = e.checkpoint(Value::Null)?;
    assert_eq!(
        saved["engine"]["world"]["rooms"]["E0N1"]["power_bank_time"],
        json!(now)
    );
    assert_eq!(
        saved["engine"]["world"]["rooms"]["E0N5"]["power_bank_time"],
        json!(now)
    );
    Ok(())
}

/// setPowerBankState 契约：未知键整单拒绝、非法值拒绝、null 清除调度。
#[test]
fn power_bank_state_contract() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&plain_rooms(vec![]))?;
    assert!(
        e.set_power_bank_state(ROOM, &json!({ "powerBankTime": 42.5 }))
            .is_err(),
        "小数拒绝"
    );
    assert!(
        e.set_power_bank_state(ROOM, &json!({ "powerBankTime": -1 }))
            .is_err(),
        "负数拒绝"
    );
    assert!(
        e.set_power_bank_state(ROOM, &json!({ "unexpected": 1 }))
            .is_err(),
        "未知键拒绝"
    );
    assert!(
        e.set_power_bank_state("W9N9", &json!({})).is_err(),
        "房间不存在拒绝"
    );
    e.set_power_bank_state(ROOM, &json!({ "powerBankTime": 100.0 }))?;
    let saved = e.checkpoint(Value::Null)?;
    assert_eq!(
        saved["engine"]["world"]["rooms"][ROOM]["power_bank_time"],
        json!(100),
        "100.0 与 100 等价（as_f64 取整）"
    );
    // 0 与 null 同义（官方 falsy 归一）。
    e.set_power_bank_state(ROOM, &json!({ "powerBankTime": 0 }))?;
    assert_eq!(
        e.checkpoint(Value::Null)?["engine"]["world"]["rooms"][ROOM]["power_bank_time"],
        json!(null),
        "0 归一为未调度（官方 !powerBankTime falsy 语义）"
    );
    e.set_power_bank_state(ROOM, &json!({ "powerBankTime": null }))?;
    let saved = e.checkpoint(Value::Null)?;
    assert_eq!(
        saved["engine"]["world"]["rooms"][ROOM]["power_bank_time"],
        json!(null),
        "null 清除调度"
    );
    Ok(())
}
