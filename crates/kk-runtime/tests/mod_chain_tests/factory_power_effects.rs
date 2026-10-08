//! 工厂首次锁级、同级刷新与等级配方生产的真实玩家链。
use super::*;
use kk_core::constants as c;

fn fixture(binary: bool, level: i64) -> anyhow::Result<(EmbedEngine, String, String, Value)> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}))?;
    let table: Value = serde_json::from_str(c::COMMODITIES)?;
    let (resource, recipe) = table
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, v)| v["level"] == level)
        .unwrap();
    let factory = e.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":"factory","owner":"p1","pos":[20,20],"store":recipe["components"]}),
    )?;
    let pc=e.place_object("W1N1","powerCreep",&json!({"name":"operator","owner":"p1","pos":[19,20],"powers":{c::PWR_OPERATE_FACTORY.to_string():level},"store":{"ops":300}}))?;
    Ok((e, factory, pc, json!({"resource":resource,"recipe":recipe})))
}

fn cast_code(name: &str, target: &str) -> String {
    format!("Game.powerCreeps.{name}.usePower(PWR_OPERATE_FACTORY,Game.getObjectById('{target}'))")
}
fn produce_code(target: &str, recipe: &Value) -> String {
    format!(
        "Game.getObjectById('{target}').produce('{}')",
        recipe["resource"].as_str().unwrap()
    )
}

#[test]
fn factory_power_all_levels_lock_level_and_produce_recipe_with_exact_costs() -> anyhow::Result<()> {
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, f, pc, r) = fixture(binary, level)?;
            assert_eq!(
                e.run_player("p1", &produce_code(&f, &r))?.expr["value"],
                c::ERR_INVALID_TARGET
            );
            assert_eq!(
                e.run_player("p1", &cast_code("operator", &f))?.expr["value"],
                0
            );
            assert_eq!(e.get_object(&f).unwrap()["level"], level);
            assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 200);
            assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
            assert_eq!(e.run_player("p1", &produce_code(&f, &r))?.expr["value"], 0);
            let store = e.get_object(&f).unwrap()["store"].clone();
            assert_eq!(
                store[r["resource"].as_str().unwrap()],
                r["recipe"]["amount"]
            );
            for resource in r["recipe"]["components"].as_object().unwrap().keys() {
                assert_eq!(store[resource].as_i64().unwrap_or(0), 0);
            }
            assert_eq!(
                e.get_object(&f).unwrap()["cooldown"],
                r["recipe"]["cooldown"].as_i64().unwrap() - 1
            );
        }
    }
    Ok(())
}

#[test]
fn factory_level_survives_expiry_rejects_other_levels_and_same_level_refreshes()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, f, _, r) = fixture(binary, 3)?;
        e.run_player("p1", &cast_code("operator", &f))?;
        e.tick(1000)?;
        assert_eq!(e.get_object(&f).unwrap()["level"], 3);
        assert_eq!(e.get_object(&f).unwrap()["effects"], json!([]));
        assert_eq!(
            e.run_player("p1", &produce_code(&f, &r))?.expr["value"],
            c::ERR_BUSY
        );
        for level in [2, 4] {
            let name = format!("other{level}");
            let pc=e.place_object("W1N1","powerCreep",&json!({"name":name,"owner":"p1","pos":[19,20],"powers":{c::PWR_OPERATE_FACTORY.to_string():level},"store":{"ops":100}}))?;
            assert_eq!(
                e.run_player("p1", &cast_code(&name, &f))?.expr["value"],
                0,
                "预检放行，由结算拒绝异级"
            );
            assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 100);
            assert_eq!(e.get_object(&f).unwrap()["level"], 3);
            assert_eq!(e.get_object(&f).unwrap()["effects"], json!([]));
            assert!(harness::events(&e, c::EVENT_POWER).is_empty());
        }
        assert_eq!(
            e.run_player("p1", &cast_code("operator", &f))?.expr["value"],
            0
        );
        e.tick(800)?;
        assert_eq!(
            e.run_player("p1", &cast_code("operator", &f))?.expr["value"],
            0
        );
        let effects = e.get_object(&f).unwrap()["effects"].clone();
        assert_eq!(effects.as_array().unwrap().len(), 1);
        assert_eq!(effects[0]["ticksRemaining"], 999);
        assert_eq!(e.run_player("p1", &produce_code(&f, &r))?.expr["value"], 0);
    }
    Ok(())
}

#[test]
fn factory_recipe_includes_expiry_tick_but_public_effect_forgery_cannot_reenable_it()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, f, _, r) = fixture(binary, 1)?;
        e.run_player("p1", &cast_code("operator", &f))?;
        let saved = e.checkpoint(Value::Null)?;
        // 分别从同一状态推进到截止 tick 和下一 tick，保留同一份原料。
        let mut expired = EmbedEngine::new();
        expired.set_kkpv_channel(binary);
        expired.restore_checkpoint(saved, None)?;
        e.tick(999)?;
        assert_eq!(e.get_object(&f).unwrap()["effects"], json!([]));
        assert_eq!(e.run_player("p1", &produce_code(&f, &r))?.expr["value"], 0);
        expired.tick(1000)?;
        let before = expired.get_object(&f).unwrap()["store"].clone();
        let code = format!(
            "Game.getObjectById('{f}').effects.push({{power:PWR_OPERATE_FACTORY,level:1,ticksRemaining:1000}});{}",
            produce_code(&f, &r)
        );
        assert_eq!(expired.run_player("p1", &code)?.expr["value"], c::ERR_BUSY);
        assert_eq!(expired.get_object(&f).unwrap()["store"], before);
    }
    Ok(())
}

#[test]
fn factory_level_effect_and_production_retry_and_cold_restore_are_atomic() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, f, pc, r) = fixture(binary, 5)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;{}",
                    cast_code("operator", &f)
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 200);
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        assert_eq!(e.world_snapshot(), restored.world_snapshot());
        let before = e.world_snapshot();
        let code = produce_code(&f, &r);
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑生产')")?;
        restored.run_player("p1", &code)?;
        assert_eq!(e.world_snapshot(), restored.world_snapshot());
        assert_eq!(e.get_object(&f).unwrap()["level"], 5);
        assert_eq!(
            e.get_object(&f).unwrap()["store"][r["resource"].as_str().unwrap()],
            r["recipe"]["amount"]
        );
    }
    Ok(())
}
