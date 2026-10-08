//! spawn power 从真实玩家施放到任务时钟、出生及恢复的闭环。
use super::*;
use kk_core::constants as c;

fn fixture(binary: bool, power: i64, level: i64) -> anyhow::Result<(EmbedEngine, String, String)> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}))?;
    let spawn = e.place_object(
        "W1N1",
        "structure",
        &json!({
        "structureType":"spawn","name":"Spawn1","owner":"p1","pos":[25,25],
        "store":{"energy":300}}),
    )?;
    let pc = e.place_object(
        "W1N1",
        "powerCreep",
        &json!({
        "name":"operator","owner":"p1","pos":[22,25],
        "powers":{power.to_string():level},"store":{"ops":300}}),
    )?;
    Ok((e, spawn, pc))
}

fn cast(e: &mut EmbedEngine, power: i64, target: &str) -> anyhow::Result<()> {
    let result = e.run_player(
        "p1",
        &format!("Game.powerCreeps.operator.usePower({power},Game.getObjectById('{target}'))"),
    )?;
    assert_eq!(result.expr["value"], 0);
    Ok(())
}

fn create(e: &mut EmbedEngine, spawn: &str, body: &str) -> anyhow::Result<()> {
    let result = e.run_player(
        "p1",
        &format!(
            "Game.getObjectById('{spawn}').spawnCreep({body},'child',{{directions:[BOTTOM]}})"
        ),
    )?;
    assert_eq!(result.expr["value"], 0);
    Ok(())
}

fn remaining(e: &EmbedEngine, spawn: &str) -> i64 {
    e.get_object(spawn).unwrap()["spawning"]["remainingTime"]
        .as_i64()
        .unwrap()
}

#[test]
fn operate_spawn_all_levels_round_up_once_and_charge_ops() -> anyhow::Result<()> {
    for binary in [false, true] {
        for (level, duration) in [(1, 9), (2, 7), (3, 5), (4, 4), (5, 2)] {
            let (mut e, spawn, pc) = fixture(binary, c::PWR_OPERATE_SPAWN, level)?;
            cast(&mut e, c::PWR_OPERATE_SPAWN, &spawn)?;
            assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 200);
            assert_eq!(
                e.get_object(&spawn).unwrap()["effects"][0]["ticksRemaining"],
                999
            );
            assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
            create(&mut e, &spawn, "[MOVE,MOVE,MOVE]")?;
            let job = e.get_object(&spawn).unwrap()["spawning"].clone();
            assert_eq!(job["needTime"], duration);
            assert_eq!(job["remainingTime"], duration - 1);
            e.tick((duration - 1) as u64)?;
            let state =
                e.run_player("p1", "[Game.creeps.child.spawning,Game.creeps.child.pos.y]")?;
            assert_eq!(state.expr["value"], json!([false, 26]));
        }
    }
    Ok(())
}

#[test]
fn disrupt_spawn_all_levels_pause_exactly_duration_without_changing_need_time() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, spawn, pc) = fixture(binary, c::PWR_DISRUPT_SPAWN, level)?;
            create(&mut e, &spawn, "[MOVE,MOVE,MOVE,MOVE,MOVE]")?;
            let before = remaining(&e, &spawn);
            cast(&mut e, c::PWR_DISRUPT_SPAWN, &spawn)?;
            assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 290);
            assert_eq!(remaining(&e, &spawn), before);
            assert_eq!(e.get_object(&spawn).unwrap()["spawning"]["needTime"], 15);
            e.tick((level - 1) as u64)?;
            assert_eq!(remaining(&e, &spawn), before);
            e.tick(1)?;
            assert_eq!(remaining(&e, &spawn), before - 1);
            assert_eq!(e.get_object(&spawn).unwrap()["effects"], json!([]));
        }
    }
    Ok(())
}

#[test]
fn one_tick_spawn_defers_birth_and_uses_default_directions_unless_disrupted() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for disrupt in [false, true] {
            let (mut e, spawn, _) = fixture(binary, c::PWR_OPERATE_SPAWN, 5)?;
            let jammer = e.place_object(
                "W1N1",
                "powerCreep",
                &json!({
                "name":"jammer","owner":"p1","pos":[22,26],
                "powers":{c::PWR_DISRUPT_SPAWN.to_string():1},"store":{"ops":30}}),
            )?;
            cast(&mut e, c::PWR_OPERATE_SPAWN, &spawn)?;
            let code = format!(
                "{} Game.getObjectById('{spawn}').spawnCreep([MOVE],'child',{{directions:[BOTTOM]}})",
                if disrupt {
                    format!(
                        "Game.getObjectById('{jammer}').usePower(PWR_DISRUPT_SPAWN,Game.getObjectById('{spawn}'));"
                    )
                } else {
                    String::new()
                }
            );
            assert_eq!(e.run_player("p1", &code)?.expr["value"], 0);
            let child = e
                .find_in_room("W1N1", c::FIND_CREEPS)
                .into_iter()
                .find(|o| o["name"] == "child")
                .unwrap();
            assert_eq!(child["spawning"], true, "新 creep 本轮不提前出生");
            assert_eq!(
                e.get_object(&spawn).unwrap()["spawning"].is_null(),
                !disrupt
            );
            e.tick(1)?;
            let pos = e.run_player(
                "p1",
                "[Game.creeps.child.spawning,Game.creeps.child.pos.x,Game.creeps.child.pos.y]",
            )?;
            assert_eq!(
                pos.expr["value"],
                json!([false, 25, if disrupt { 26 } else { 24 }])
            );
        }
    }
    Ok(())
}

#[test]
fn spawn_effects_retry_once_and_cold_restore_preserves_paused_clock() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, spawn, pc) = fixture(binary, c::PWR_OPERATE_SPAWN, 3)?;
        cast(&mut e, c::PWR_OPERATE_SPAWN, &spawn)?;
        create(&mut e, &spawn, "[MOVE,MOVE,MOVE,MOVE,MOVE]")?;
        let jammer = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({
            "name":"jammer","owner":"p1","pos":[22,26],
            "powers":{c::PWR_DISRUPT_SPAWN.to_string():5},"store":{"ops":30}}),
        )?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", &format!(
            "Memory.runs=(Memory.runs||0)+1;Game.getObjectById('{jammer}').usePower(PWR_DISRUPT_SPAWN,Game.getObjectById('{spawn}'))")).is_err());
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(
            e.get_object(&spawn).unwrap()["effects"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 200);
        assert_eq!(e.get_object(&jammer).unwrap()["store"]["ops"], 20);
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        assert_eq!(restored.world_snapshot(), e.world_snapshot());
        assert_eq!(remaining(&e, &spawn), 7);
        e.tick(11)?;
        restored.tick(11)?;
        assert_eq!(restored.world_snapshot(), e.world_snapshot());
        assert!(
            restored.get_object(&spawn).unwrap()["spawning"].is_null(),
            "{:?}",
            restored.get_object(&spawn)
        );
        let code = "[Memory.runs,Game.creeps.child.spawning,Game.creeps.child.pos.y]";
        assert_eq!(
            restored.run_player("p1", code)?.expr["value"],
            json!([1, false, 26])
        );
        e.run_player("p1", code)?;
        assert_eq!(restored.world_snapshot(), e.world_snapshot());
    }
    Ok(())
}

#[test]
fn operate_spawn_expiry_does_not_retime_existing_job() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, spawn, _) = fixture(binary, c::PWR_OPERATE_SPAWN, 1)?;
        create(&mut e, &spawn, "[MOVE,MOVE,MOVE,MOVE,MOVE]")?;
        cast(&mut e, c::PWR_OPERATE_SPAWN, &spawn)?;
        assert_eq!(e.get_object(&spawn).unwrap()["spawning"]["needTime"], 15);
        assert_eq!(remaining(&e, &spawn), 13, "施放不重算已存在的任务");
        e.tick(1000)?;
        assert_eq!(e.get_object(&spawn).unwrap()["effects"], json!([]));
        assert_eq!(
            e.run_player(
                "p1",
                &format!("Game.getObjectById('{spawn}').spawnCreep([MOVE,MOVE,MOVE],'next')")
            )?
            .expr["value"],
            0
        );
        assert_eq!(e.get_object(&spawn).unwrap()["spawning"]["needTime"], 9);
    }
    Ok(())
}
