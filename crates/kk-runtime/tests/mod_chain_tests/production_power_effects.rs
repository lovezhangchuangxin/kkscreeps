//! lab 正/逆反应、powerSpawn 处理量及到期边界的完整 power 链。
use super::*;
use kk_core::constants as c;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}))?;
    Ok(e)
}

fn structure(
    e: &mut EmbedEngine,
    kind: &str,
    x: i64,
    store: Value,
    effects: Value,
) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({
        "structureType":kind,"owner":"p1","pos":[x,20],"store":store,"effects":effects}),
    )
}

fn operator(e: &mut EmbedEngine, power: i64, level: i64) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({
        "name":"operator","owner":"p1","pos":[20,19],
        "powers":{power.to_string():level},"store":{"ops":300}}),
    )
}

fn cast(e: &mut EmbedEngine, power: i64, target: &str) -> anyhow::Result<()> {
    assert_eq!(
        e.run_player(
            "p1",
            &format!("Game.powerCreeps.operator.usePower({power},Game.getObjectById('{target}'))")
        )?
        .expr["value"],
        0
    );
    Ok(())
}

fn reaction_code(output: &str, first: &str, second: &str, reverse: bool) -> String {
    format!(
        "Game.getObjectById('{output}').{}(Game.getObjectById('{first}'),Game.getObjectById('{second}'))",
        if reverse {
            "reverseReaction"
        } else {
            "runReaction"
        }
    )
}

fn amount(e: &EmbedEngine, id: &str, resource: &str) -> i64 {
    e.get_object(id).unwrap()["store"][resource]
        .as_i64()
        .unwrap_or(0)
}

#[test]
fn operate_lab_all_levels_scale_both_reactions_and_reset_empty_mineral_slots() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for level in 1..=5 {
            for reverse in [false, true] {
                let mut e = fixture(binary)?;
                let batch = 5 + 2 * level;
                let output = structure(
                    &mut e,
                    "lab",
                    20,
                    if reverse {
                        json!({"OH":batch})
                    } else {
                        json!({})
                    },
                    json!([]),
                )?;
                let first = structure(
                    &mut e,
                    "lab",
                    21,
                    if reverse {
                        json!({})
                    } else {
                        json!({"H":batch})
                    },
                    json!([]),
                )?;
                let second = structure(
                    &mut e,
                    "lab",
                    22,
                    if reverse {
                        json!({})
                    } else {
                        json!({"O":batch})
                    },
                    json!([]),
                )?;
                let pc = operator(&mut e, c::PWR_OPERATE_LAB, level)?;
                cast(&mut e, c::PWR_OPERATE_LAB, &output)?;
                assert_eq!(amount(&e, &pc, "ops"), 290);
                assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
                assert_eq!(
                    e.run_player("p1", &reaction_code(&output, &first, &second, reverse))?
                        .expr["value"],
                    0
                );
                assert_eq!(amount(&e, &output, "OH"), if reverse { 0 } else { batch });
                assert_eq!(amount(&e, &first, "H"), if reverse { batch } else { 0 });
                assert_eq!(amount(&e, &second, "O"), if reverse { batch } else { 0 });
                let drained = if reverse { &output } else { &first };
                let mineral = if reverse { "OH" } else { "H" };
                let capacities = e.run_player("p1", &format!(
                    "[Game.getObjectById('{drained}').mineralType,Game.getObjectById('{drained}').store.getCapacity('{mineral}')]") )?;
                assert_eq!(capacities.expr["value"], json!([null, 3000]));
            }
        }
    }
    Ok(())
}

#[test]
fn lab_prechecks_use_boosted_batch_and_native_rejects_forged_public_effects() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for reverse in [false, true] {
            for full in [false, true] {
                let mut e = fixture(binary)?;
                let output_store = match (reverse, full) {
                    (true, false) => json!({"OH":14}),
                    (true, true) => json!({"OH":15}),
                    (false, false) => json!({}),
                    (false, true) => json!({"OH":2990}),
                };
                let first_store = match (reverse, full) {
                    (true, true) => json!({"H":2990}),
                    (true, false) => json!({}),
                    (false, false) => json!({"H":14}),
                    (false, true) => json!({"H":15}),
                };
                let output = structure(&mut e, "lab", 20, output_store, json!([]))?;
                let first = structure(&mut e, "lab", 21, first_store, json!([]))?;
                let second = structure(
                    &mut e,
                    "lab",
                    22,
                    if reverse { json!({}) } else { json!({"O":15}) },
                    json!([]),
                )?;
                operator(&mut e, c::PWR_OPERATE_LAB, 5)?;
                cast(&mut e, c::PWR_OPERATE_LAB, &output)?;
                let code = reaction_code(&output, &first, &second, reverse);
                assert_eq!(
                    e.run_player("p1", &code)?.expr["value"],
                    if full {
                        c::ERR_FULL
                    } else {
                        c::ERR_NOT_ENOUGH_RESOURCES
                    }
                );
                let before = [
                    e.get_object(&output).unwrap()["store"].clone(),
                    e.get_object(&first).unwrap()["store"].clone(),
                    e.get_object(&second).unwrap()["store"].clone(),
                ];
                // 玩家删公开效果使预检按基础量放行，原生结算仍按权威量拒绝。
                assert_eq!(
                    e.run_player(
                        "p1",
                        &format!("Game.getObjectById('{output}').effects.length=0;{code}")
                    )?
                    .expr["value"],
                    0
                );
                assert_eq!(
                    [
                        e.get_object(&output).unwrap()["store"].clone(),
                        e.get_object(&first).unwrap()["store"].clone(),
                        e.get_object(&second).unwrap()["store"].clone()
                    ],
                    before
                );
                assert!(e.get_object(&output).unwrap()["cooldown"].is_null());
            }
        }
    }
    Ok(())
}

#[test]
fn operate_power_all_levels_cap_power_and_reject_insufficient_energy_atomically()
-> anyhow::Result<()> {
    for binary in [false, true] {
        for level in 1..=5 {
            for (stock, energy) in [(100, 1000), (1, 50), (100, 50)] {
                let mut e = fixture(binary)?;
                let target = structure(
                    &mut e,
                    "powerSpawn",
                    20,
                    json!({"power":stock,"energy":energy}),
                    json!([]),
                )?;
                let pc = operator(&mut e, c::PWR_OPERATE_POWER, level)?;
                cast(&mut e, c::PWR_OPERATE_POWER, &target)?;
                assert_eq!(amount(&e, &pc, "ops"), 100);
                assert_eq!(
                    e.run_player(
                        "p1",
                        &format!("Game.getObjectById('{target}').processPower()")
                    )?
                    .expr["value"],
                    0,
                    "预检恒按基础量"
                );
                let batch = stock.min(1 + level);
                let processed = if energy >= batch * 50 { batch } else { 0 };
                assert_eq!(amount(&e, &target, "power"), stock - processed);
                assert_eq!(amount(&e, &target, "energy"), energy - processed * 50);
                assert_eq!(e.player_state("p1").unwrap().power, 100000 + processed);
            }
        }
    }
    Ok(())
}

#[test]
fn production_effect_expiration_keeps_distinct_lab_and_power_spawn_boundaries() -> anyhow::Result<()>
{
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let ps = structure(
            &mut e,
            "powerSpawn",
            20,
            json!({"power":100,"energy":5000}),
            json!([{"power":c::PWR_OPERATE_POWER,"level":5,"endTime":1}]),
        )?;
        let output = structure(
            &mut e,
            "lab",
            20,
            json!({}),
            json!([{"power":c::PWR_OPERATE_LAB,"level":5,"endTime":1}]),
        )?;
        let first = structure(&mut e, "lab", 21, json!({"H":50}), json!([]))?;
        let second = structure(&mut e, "lab", 22, json!({"O":50}), json!([]))?;
        e.tick(1)?;
        assert_eq!(e.get_object(&ps).unwrap()["effects"], json!([]));
        let code = format!(
            "[{},Game.getObjectById('{ps}').processPower()]",
            reaction_code(&output, &first, &second, false)
        );
        assert_eq!(e.run_player("p1", &code)?.expr["value"], json!([0, 0]));
        assert_eq!(amount(&e, &output, "OH"), 5, "lab 严格 > 截止 tick");
        assert_eq!(amount(&e, &ps, "power"), 94, "powerSpawn 包含截止 tick");
        e.run_player("p1", &format!("Game.getObjectById('{ps}').processPower()"))?;
        assert_eq!(amount(&e, &ps, "power"), 93);
        assert_eq!(e.player_state("p1").unwrap().power, 100007);
    }
    Ok(())
}

#[test]
fn production_effects_retry_once_and_cold_restore_keep_stores_and_account_power()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let output = structure(&mut e, "lab", 20, json!({}), json!([]))?;
        let first = structure(&mut e, "lab", 21, json!({"H":30}), json!([]))?;
        let second = structure(&mut e, "lab", 22, json!({"O":30}), json!([]))?;
        let ps = structure(
            &mut e,
            "powerSpawn",
            21,
            json!({"power":100,"energy":5000}),
            json!([]),
        )?;
        operator(&mut e, c::PWR_OPERATE_LAB, 5)?;
        e.place_object("W1N1","powerCreep",&json!({"name":"processor","owner":"p1","pos":[20,19],"powers":{c::PWR_OPERATE_POWER.to_string():5},"store":{"ops":300}}))?;
        cast(&mut e, c::PWR_OPERATE_LAB, &output)?;
        assert_eq!(e.run_player("p1",&format!("Game.powerCreeps.processor.usePower(PWR_OPERATE_POWER,Game.getObjectById('{ps}'))"))?.expr["value"],0);
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        let code = format!(
            "Memory.runs=(Memory.runs||0)+1;[{},Game.getObjectById('{ps}').processPower()]",
            reaction_code(&output, &first, &second, false)
        );
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(
            e.run_player("p1", "throw Error('不得重跑玩家')")?.expr["value"],
            json!([0, 0])
        );
        assert_eq!(amount(&e, &output, "OH"), 15);
        assert_eq!(amount(&e, &ps, "power"), 94);
        assert_eq!(e.player_state("p1").unwrap().power, 100006);
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        assert_eq!(restored.world_snapshot(), e.world_snapshot());
        e.tick(20)?;
        restored.tick(20)?;
        assert_eq!(
            restored.run_player("p1", &code)?.expr["value"],
            json!([0, 0])
        );
        e.run_player("p1", &code)?;
        assert_eq!(restored.world_snapshot(), e.world_snapshot());
        assert_eq!(amount(&restored, &output, "OH"), 30);
        assert_eq!(amount(&restored, &first, "H"), 0);
        assert_eq!(restored.player_state("p1").unwrap().power, 100012);
    }
    Ok(())
}
