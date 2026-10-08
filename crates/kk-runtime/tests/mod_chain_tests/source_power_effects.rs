//! Source 周期补充、普通再生干扰与候选恢复的真实玩家链。
use super::*;
use kk_core::constants as c;

fn fixture(binary: bool, energy: u32, timer: Option<u64>) -> anyhow::Result<(EmbedEngine, String)> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}))?;
    let mut spec =
        json!({"structureType":"source","pos":[20,20],"energy":energy,"energyCapacity":3000});
    if let Some(timer) = timer {
        spec["ticksToRegeneration"] = json!(timer);
    }
    let source = e.place_object("W1N1", "structure", &spec)?;
    Ok((e, source))
}

fn actor(e: &mut EmbedEngine, name: &str, power: i64, level: i64) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({"name":name,"owner":"p1","pos":[19,20],
        "powers":{power.to_string():level},"store":{"ops":300}}),
    )
}
fn cast(name: &str, power: i64, source: &str) -> String {
    format!("Game.powerCreeps.{name}.usePower({power},Game.getObjectById('{source}'))")
}

#[test]
fn source_regen_all_levels_follow_cast_phase_and_refresh_restarts_phase() -> anyhow::Result<()> {
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, s) = fixture(binary, 0, Some(600))?;
            let pc = actor(&mut e, "regen", c::PWR_REGEN_SOURCE, level)?;
            e.tick(7)?; // 非全局 period 边界施放。
            assert_eq!(
                e.run_player("p1", &cast("regen", c::PWR_REGEN_SOURCE, &s))?
                    .expr["value"],
                0
            );
            e.tick(13)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 0);
            e.tick(1)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 50 * level);
            e.tick(14)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 50 * level);
            e.tick(1)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 100 * level);
            assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 300);
            assert_eq!(
                e.get_object(&pc).unwrap()["powers"][c::PWR_REGEN_SOURCE.to_string()]["cooldownTime"],
                107
            );
            actor(&mut e, "refresh", c::PWR_REGEN_SOURCE, level)?;
            e.run_player("p1", &cast("refresh", c::PWR_REGEN_SOURCE, &s))?;
            assert_eq!(
                e.get_object(&s).unwrap()["effects"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                e.get_object(&s).unwrap()["effects"][0]["ticksRemaining"],
                299
            );
            e.tick(13)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 100 * level);
            e.tick(1)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 150 * level);
        }
    }
    Ok(())
}

#[test]
fn source_disrupt_all_levels_hold_clock_until_strict_expiry_then_regenerate() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, s) = fixture(binary, 0, Some(10))?;
            let pc = actor(&mut e, "disrupt", c::PWR_DISRUPT_SOURCE, level)?;
            e.run_player("p1", &cast("disrupt", c::PWR_DISRUPT_SOURCE, &s))?;
            assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
            assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 200);
            e.tick((100 * level - 1) as u64)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 0);
            assert_eq!(e.get_object(&s).unwrap()["ticksToRegeneration"], 10);
            assert_eq!(e.get_object(&s).unwrap()["effects"], json!([]));
            e.tick(1)?;
            assert_eq!(e.get_object(&s).unwrap()["ticksToRegeneration"], 9);
            e.tick(8)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 0);
            e.tick(1)?;
            assert_eq!(e.get_object(&s).unwrap()["energy"], 3000);
            assert!(e.get_object(&s).unwrap()["ticksToRegeneration"].is_null());
        }
    }
    Ok(())
}

#[test]
fn source_powers_coexist_and_regen_last_pulse_occurs_before_expiry() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 0, None)?;
        actor(&mut e, "regen", c::PWR_REGEN_SOURCE, 1)?;
        actor(&mut e, "disrupt", c::PWR_DISRUPT_SOURCE, 5)?;
        let code = format!(
            "[{},{}]",
            cast("regen", c::PWR_REGEN_SOURCE, &s),
            cast("disrupt", c::PWR_DISRUPT_SOURCE, &s)
        );
        assert_eq!(e.run_player("p1", &code)?.expr["value"], json!([0, 0]));
        assert_eq!(e.get_object(&s).unwrap()["ticksToRegeneration"], 300);
        e.tick(298)?;
        assert_eq!(e.get_object(&s).unwrap()["energy"], 950);
        e.tick(1)?;
        assert_eq!(e.get_object(&s).unwrap()["energy"], 1000);
        assert_eq!(
            e.get_object(&s).unwrap()["effects"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        e.tick(15)?;
        assert_eq!(e.get_object(&s).unwrap()["energy"], 1000);
        assert_eq!(e.get_object(&s).unwrap()["ticksToRegeneration"], 300);
    }
    Ok(())
}

#[test]
fn source_regen_caps_after_harvest_and_full_sources_leave_clock_untouched() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 3000, Some(600))?;
        actor(&mut e, "regen", c::PWR_REGEN_SOURCE, 5)?;
        actor(&mut e, "disrupt", c::PWR_DISRUPT_SOURCE, 1)?;
        e.run_player(
            "p1",
            &format!(
                "[{},{}]",
                cast("regen", c::PWR_REGEN_SOURCE, &s),
                cast("disrupt", c::PWR_DISRUPT_SOURCE, &s)
            ),
        )?;
        e.tick(13)?;
        assert_eq!(e.get_object(&s).unwrap()["ticksToRegeneration"], 586);
        let creep = e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"harvester","owner":"p1","pos":[20,19],"body":["work","carry"]}),
        )?;
        assert_eq!(
            e.run_player(
                "p1",
                &format!("Game.creeps.harvester.harvest(Game.getObjectById('{s}'))")
            )?
            .expr["value"],
            0
        );
        assert_eq!(e.get_object(&creep).unwrap()["store"]["energy"], 2);
        assert_eq!(e.get_object(&s).unwrap()["energy"], 3000);
        assert_eq!(e.get_object(&s).unwrap()["ticksToRegeneration"], 586);
    }
    Ok(())
}

#[test]
fn source_disrupt_delays_due_clock_and_regen_caps_on_ordinary_regeneration_tick()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 0, Some(1))?;
        actor(&mut e, "disrupt", c::PWR_DISRUPT_SOURCE, 1)?;
        e.run_player("p1", &cast("disrupt", c::PWR_DISRUPT_SOURCE, &s))?;
        assert_eq!(e.get_object(&s).unwrap()["energy"], 0);
        assert_eq!(e.get_object(&s).unwrap()["ticksToRegeneration"], 1);
        let (mut e, s) = fixture(binary, 0, Some(15))?;
        actor(&mut e, "regen", c::PWR_REGEN_SOURCE, 5)?;
        e.run_player("p1", &cast("regen", c::PWR_REGEN_SOURCE, &s))?;
        e.tick(13)?;
        assert_eq!(e.get_object(&s).unwrap()["energy"], 0);
        e.tick(1)?;
        assert_eq!(e.get_object(&s).unwrap()["energy"], 3000);
        assert!(e.get_object(&s).unwrap()["ticksToRegeneration"].is_null());
    }
    Ok(())
}

#[test]
fn source_power_clock_and_pulses_retry_and_cold_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 0, Some(10))?;
        actor(&mut e, "regen", c::PWR_REGEN_SOURCE, 2)?;
        let pc = actor(&mut e, "disrupt", c::PWR_DISRUPT_SOURCE, 1)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;[{},{}]",
                    cast("regen", c::PWR_REGEN_SOURCE, &s),
                    cast("disrupt", c::PWR_DISRUPT_SOURCE, &s)
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 200);
        e.tick(12)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        for ticks in [1, 1, 15, 70, 10, 10] {
            e.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert_eq!(e.get_object(&s).unwrap()["energy"], 3000);
        assert!(e.get_object(&s).unwrap()["ticksToRegeneration"].is_null());
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}

#[test]
fn source_power_lower_level_and_forged_public_effects_do_not_change_native_pulses()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 0, Some(600))?;
        actor(&mut e, "regen", c::PWR_REGEN_SOURCE, 5)?;
        let lower = actor(&mut e, "lower", c::PWR_REGEN_SOURCE, 1)?;
        e.run_player("p1", &cast("regen", c::PWR_REGEN_SOURCE, &s))?;
        assert_eq!(
            e.run_player("p1", &cast("lower", c::PWR_REGEN_SOURCE, &s))?
                .expr["value"],
            c::ERR_FULL
        );
        assert_eq!(e.get_object(&lower).unwrap()["store"]["ops"], 300);
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());
        e.tick(12)?;
        let rc = e.run_player("p1", &format!("const s=Game.getObjectById('{s}');s.effects[0].level=1;s.effects[0].ticksRemaining=0;s.effects.length=0;[s.energy,s.ticksToRegeneration]"))?;
        assert_eq!(rc.expr["value"], json!([0, 586]));
        assert_eq!(e.get_object(&s).unwrap()["energy"], 250);
        assert_eq!(e.get_object(&s).unwrap()["effects"][0]["level"], 5);
    }
    Ok(())
}
