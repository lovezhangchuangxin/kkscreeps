//! Mineral 周期补量、资格门、耗尽停补与候选恢复的真实玩家链。
use super::*;
use kk_core::constants as c;

fn fixture(binary: bool, amount: u32, timer: Option<u64>) -> anyhow::Result<(EmbedEngine, String)> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}))?;
    let mut spec = json!({"structureType":"mineral","pos":[20,20],
        "mineralType":"X","mineralAmount":amount,"density":3});
    if let Some(timer) = timer {
        spec["ticksToRegeneration"] = json!(timer);
    }
    let mineral = e.place_object("W1N1", "structure", &spec)?;
    Ok((e, mineral))
}

// 施放者统一放矿物左侧 [19,20]（range 3 内）；多个 powerCreep 叠放同格
// 仅测试便利，官方世界中不可达。
fn actor(e: &mut EmbedEngine, name: &str, level: i64) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({"name":name,"owner":"p1","pos":[19,20],
        "powers":{"14":level},"store":{"ops":300}}),
    )
}
fn cast(name: &str, mineral: &str) -> String {
    format!("Game.powerCreeps.{name}.usePower(14,Game.getObjectById('{mineral}'))")
}

/// POWER_INFO[14]：duration 100 / period 10 / effect [2,4,6,8,10]，无 ops
/// 费用。施放相位由 endTime 决定（endTime-time-1 ≡ 0 mod period），刷新
/// 重置窗口并按新 endTime 续拍；补量不设上限（官方无 cap）。
#[test]
fn mineral_regen_all_levels_follow_cast_phase_and_refresh_extends_window() -> anyhow::Result<()> {
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, s) = fixture(binary, 1000, None)?;
            let pc = actor(&mut e, "regen", level)?;
            e.tick(7)?;
            assert_eq!(e.run_player("p1", &cast("regen", &s))?.expr["value"], 0);
            e.tick(8)?; // 结算 tick 8..15，首脉冲在 16
            assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1000);
            e.tick(1)?;
            assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1000 + 2 * level);
            e.tick(9)?;
            assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1000 + 2 * level);
            e.tick(1)?; // tick 26 第二拍
            assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1000 + 4 * level);
            assert_eq!(
                e.get_object(&pc).unwrap()["store"]["ops"],
                300,
                "REGEN_MINERAL 无 ops 费用"
            );
            assert_eq!(
                e.get_object(&pc).unwrap()["powers"]["14"]["cooldownTime"],
                107
            );
            // 同 creep 冷却内重施 → ERR_TIRED（玩家面 power-creeps.js:262）。
            assert_eq!(
                e.run_player("p1", &cast("regen", &s))?.expr["value"],
                c::ERR_TIRED
            );
            // 刷新（time 31 施放 → end 131）：效果表单条、窗口重置；相位按
            // 新 endTime 对齐（131-1≡0 mod 10 → 40 起，区别于旧相位 36）。
            e.tick(4)?;
            actor(&mut e, "refresh", level)?;
            e.run_player("p1", &cast("refresh", &s))?;
            assert_eq!(
                e.get_object(&s).unwrap()["effects"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                e.get_object(&s).unwrap()["effects"][0]["ticksRemaining"],
                99
            );
            e.tick(8)?; // 结算 32..39，旧相位 36 不再补
            assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1000 + 4 * level);
            e.tick(1)?; // tick 40：新相位首拍
            assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1000 + 6 * level);
        }
    }
    Ok(())
}

/// 资格门（官方 usePower.js REGEN_MINERAL 分支）：耗尽与有待再生时钟的
/// 矿物都拒绝施放——玩家预检不拦（返回 OK），结算静默：无事件、无效果、
/// 不写冷却，自然再生钟不被误清。时钟 Some(0) 按官方 falsy 视为未设置
/// （放置于 time 0 得 Some(0)），施放照常成功且脉冲不受影响。
#[test]
fn mineral_regen_rejects_depleted_and_clocked_targets_silently() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 0, None)?;
        let pc = actor(&mut e, "regen", 1)?;
        e.tick(7)?;
        assert_eq!(e.run_player("p1", &cast("regen", &s))?.expr["value"], 0);
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());
        assert!(e.get_object(&s).unwrap()["effects"].is_null());
        assert!(e.get_object(&pc).unwrap()["powers"]["14"]["cooldownTime"].is_null());
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 0);
        assert_eq!(
            e.get_object(&s).unwrap()["ticksToRegeneration"],
            50_000 - 8,
            "首 tick 挂的自然钟保留（读取时 time 8）"
        );

        let (mut e, s) = fixture(binary, 500, Some(50))?;
        let pc = actor(&mut e, "regen", 1)?;
        e.run_player("p1", &cast("regen", &s))?;
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());
        assert!(e.get_object(&s).unwrap()["effects"].is_null());
        assert!(e.get_object(&pc).unwrap()["powers"]["14"]["cooldownTime"].is_null());
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 500);

        // Some(0) 时钟 ≡ 未设置：施放成功（结算于 tick 1 → end 101，脉冲
        // 位 t≡0 mod 10：10、20…），自然钟也不因 0 立即回填。
        let (mut e, s) = fixture(binary, 500, Some(0))?;
        actor(&mut e, "regen", 1)?;
        e.tick(1)?;
        e.run_player("p1", &cast("regen", &s))?; // 结算 tick 1 → end 101
        assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
        assert!(e.get_object(&s).unwrap()["ticksToRegeneration"].is_null());
        e.tick(8)?; // 结算 2..9，脉冲位 10 未到
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 500);
        e.tick(1)?; // tick 10 脉冲
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 502);

        // 超距（range 3）：玩家面预检 ERR_NOT_IN_RANGE，结算无事件。
        let (mut e, s) = fixture(binary, 500, None)?;
        e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"far","owner":"p1","pos":[16,20],
            "powers":{"14":1},"store":{"ops":300}}),
        )?;
        assert_eq!(
            e.run_player("p1", &cast("far", &s))?.expr["value"],
            c::ERR_NOT_IN_RANGE
        );
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());
        assert!(e.get_object(&s).unwrap()["effects"].is_null());
    }
    Ok(())
}

/// 高等级效果在位时低等级施放返回 ERR_FULL 不扣费；玩家篡改公开效果
/// 投影不影响原生补量。
#[test]
fn mineral_regen_lower_level_err_full_and_forged_effects_do_not_change_native_pulses()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 1000, None)?;
        actor(&mut e, "regen", 5)?;
        let lower = actor(&mut e, "lower", 1)?;
        e.tick(7)?;
        e.run_player("p1", &cast("regen", &s))?;
        assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
        assert_eq!(
            e.run_player("p1", &cast("lower", &s))?.expr["value"],
            c::ERR_FULL
        );
        assert_eq!(e.get_object(&lower).unwrap()["store"]["ops"], 300);
        assert!(
            harness::events(&e, c::EVENT_POWER).is_empty(),
            "拒绝轮无事件"
        );
        assert!(
            e.get_object(&lower).unwrap()["powers"]["14"]["cooldownTime"].is_null(),
            "拒绝轮不写冷却"
        );
        e.tick(7)?; // time 9 后结算 9..15，脉冲位 16 未到
        let rc = e.run_player(
            "p1",
            &format!(
                "const s=Game.getObjectById('{s}');s.effects[0].level=1;s.effects[0].ticksRemaining=0;s.effects.length=0;[s.mineralAmount,s.ticksToRegeneration]"
            ),
        )?;
        assert_eq!(rc.expr["value"], json!([1000, null]));
        // 该 run_player 同步结算 tick 16：脉冲按原生 level 5 生效。
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1010);
        assert_eq!(e.get_object(&s).unwrap()["effects"][0]["level"], 5);

        // 补量无上限：44999 + 等级 5 一拍 → 45009，越过 ULTRA 密度存量。
        let (mut e, s) = fixture(binary, 44_999, None)?;
        actor(&mut e, "regen", 5)?;
        e.run_player("p1", &cast("regen", &s))?; // 结算 tick 0 → end 100，脉冲位 9、19…
        e.tick(8)?; // 结算 1..8
        e.tick(1)?; // tick 9 脉冲
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 45_009);
    }
    Ok(())
}

/// 采集耗尽后效果停补（官方 `object.mineralAmount` 真值门），自然再生
/// 时钟接管并倒数。
#[test]
fn mineral_regen_stops_when_depleted_and_natural_clock_takes_over() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 1, None)?;
        e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"extractor","pos":[20,20]}),
        )?;
        actor(&mut e, "regen", 1)?;
        let miner = e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"miner","owner":"p1","pos":[20,19],
                "body":["work","carry"],"ticksToLive":1500}),
        )?;
        e.tick(7)?;
        e.run_player("p1", &cast("regen", &s))?; // tick 7：end 107
        e.run_player(
            "p1",
            &format!("Game.creeps.miner.harvest(Game.getObjectById('{s}'))"),
        )?; // tick 8：1→0，同拍 mineral_tick 挂钟 50008
        assert_eq!(e.get_object(&miner).unwrap()["store"]["X"], 1);
        e.tick(7)?; // time 16
        e.tick(1)?; // tick 16 脉冲位：存量 0 → 不补
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 0);
        assert_eq!(
            e.get_object(&s).unwrap()["ticksToRegeneration"],
            50_008 - 17
        );
        assert_eq!(
            e.get_object(&s).unwrap()["effects"][0]["ticksRemaining"],
            90
        );
        e.tick(10)?; // tick 26 脉冲位：仍不补
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 0);
    }
    Ok(())
}

/// 施放轮写回失败缓存整轮候选；重试原子提交；检查点冷恢复与继续推进
/// 逐拍一致。
#[test]
fn mineral_power_cast_and_pulses_retry_and_cold_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 1000, None)?;
        actor(&mut e, "regen", 2)?;
        e.tick(7)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!("Memory.runs=(Memory.runs||0)+1;{}", cast("regen", &s))
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(
            e.get_object(&s).unwrap()["effects"][0]["ticksRemaining"],
            99
        );
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        // [8,1,90,1] 推进到 time 108；再越过一个对齐位 116 确认到期后不补。
        for ticks in [8, 1, 90, 1, 8, 1] {
            e.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        // tick 7 施放（end 107），level 2 每拍 +4；脉冲 16..106 共 10 拍。
        assert_eq!(e.get_object(&s).unwrap()["mineralAmount"], 1040);
        assert_eq!(e.get_object(&s).unwrap()["effects"], json!([]));
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}
