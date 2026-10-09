//! 玩家仓库搬运、GCL 输入和升级累计在两种投影通道中的闭环。
use super::*;
use serde_json::Value;

pub(super) fn economy_gcl_e2e_impl() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard(&json!({
            "players": [{"name": "p1", "gcl": {"level": 3, "progress": 17}}],
            "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 3}],
        }))?;
        let target = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos": [1, 3], "structureType": "container", "store": {"energy": 10, "U": 5}
            }),
        )?;
        let actor = e.place_object(
            "W1N1",
            "creep",
            &json!({
                "pos": [1, 2], "owner": "p1", "name": "hauler", "body": ["work", "carry", "move"]
            }),
        )?;
        let target_js = serde_json::to_string(&target)?;
        let run = e.run_player(
            "p1",
            &format!(
                "Game.creeps.hauler.withdraw(Game.getObjectById({target_js}), RESOURCE_ENERGY, 10)"
            ),
        )?;
        assert_eq!(run.expr["value"], 0);
        assert_eq!(e.get_object(&actor).unwrap()["store"]["energy"], 10);
        let time = e.get_game_time();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", &format!("Memory.upgrades = (Memory.upgrades || 0) + 1; [Game.creeps.hauler.transfer(Game.getObjectById({target_js}), RESOURCE_ENERGY, 9), Game.creeps.hauler.upgradeController(Game.rooms.W1N1.controller)]")).is_err());
        assert_eq!(e.get_game_time(), time);
        assert_eq!(e.get_object(&actor).unwrap()["store"]["energy"], 10);
        assert_eq!(
            e.get_object(&target).unwrap()["store"]["energy"]
                .as_i64()
                .unwrap_or(0),
            0
        );
        // 重试复用旧意图，忽略传入的另一个动作，不重复执行玩家代码。
        let run = e.run_player("p1", "Game.creeps.hauler.drop(RESOURCE_ENERGY)")?;
        assert_eq!(run.expr["value"], json!([0, 0]));
        let run = e.run_player(
            "p1",
            "[Game.gcl.level, Math.round(Game.gcl.progress), Game.rooms.W1N1.controller.progress, Memory.upgrades]",
        )?;
        assert_eq!(run.expr["value"], json!([3, 18, 1, 1]));
        assert_eq!(
            e.get_object(&actor).unwrap()["store"]["energy"]
                .as_i64()
                .unwrap_or(0),
            0
        );
        assert_eq!(e.get_object(&target).unwrap()["store"]["energy"], 9);
        assert_eq!(e.get_object(&target).unwrap()["store"]["U"], 5);

        // 输入校验必须先于旧世界回收，非法 GCL 不丢房间、时间或玩家。
        let time = e.get_game_time();
        for gcl in [
            json!({"level": 0}),
            json!({"level": 1.5}),
            json!({"level": 1, "progress": 1_000_000}),
            json!({"level": 2, "progress": -1}),
            json!({"level": 2, "progress": null}),
            json!({"level": 1e20}),
            Value::Null,
        ] {
            assert!(
                e.create_shard(&json!({"players": [{"name": "p1", "gcl": gcl}], "rooms": []}))
                    .is_err()
            );
            assert_eq!(e.get_game_time(), time);
            assert!(e.get_object(&target).is_some());
        }

        // 持续采集 → 存入容器 → 提取升级：两名玩家单位共享同一候选世界。
        e.create_shard(
            &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 3}]}),
        )?;
        let source = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType": "source", "pos": [2, 2], "energy": 3000}),
        )?;
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType": "container", "pos": [1, 3]}),
        )?;
        let miner = e.place_object(
            "W1N1",
            "creep",
            &json!({"owner": "p1", "name": "miner", "body": ["work", "carry"], "pos": [1, 2]}),
        )?;
        let upgrader = e.place_object(
            "W1N1",
            "creep",
            &json!({"owner": "p1", "name": "upgrader", "body": ["work", "carry"], "pos": [2, 3]}),
        )?;
        let code = format!(
            r#"(() => {{
            const miner = Game.creeps.miner, upgrader = Game.creeps.upgrader;
            const container = Game.getObjectById({container:?});
            const codes = [];
            if (miner.store.energy >= 8) codes.push(miner.transfer(container, RESOURCE_ENERGY));
            else codes.push(miner.harvest(Game.getObjectById({source:?})));
            if (upgrader.store.energy) codes.push(upgrader.upgradeController(Game.rooms.W1N1.controller));
            else if (container.store.energy) codes.push(upgrader.withdraw(container, RESOURCE_ENERGY));
            Memory.cycles = (Memory.cycles || 0) + 1;
            return codes;
        }})()"#
        );
        for tick in 0..40 {
            let run = e.run_player("p1", &code)?;
            assert!(
                run.expr["value"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|rc| rc == 0),
                "binary={binary}, tick={tick}: {:?}, source={:?}, miner={:?}",
                run.expr,
                e.get_object(&source),
                e.get_object(&miner)
            );
        }
        assert_eq!(e.get_game_time(), 41);
        let run = e.run_player(
            "p1",
            "[Memory.cycles, Game.rooms.W1N1.controller.progress, Game.gcl.progress]",
        )?;
        let values = run.expr["value"].as_array().unwrap();
        assert_eq!(values[0], 40);
        let spent = values[1].as_i64().unwrap();
        assert!(spent > 10, "连续世界必须发生实际升级");
        assert_eq!(values[2], spent);
        let energy = |id: &str| {
            e.get_object(id).unwrap()["store"]["energy"]
                .as_i64()
                .unwrap_or(0)
        };
        assert_eq!(
            e.get_object(&source).unwrap()["energy"].as_i64().unwrap()
                + energy(&container)
                + energy(&miner)
                + energy(&upgrader)
                + spent,
            3000,
            "采集和搬运守恒，升级才消耗能量"
        );
        // 固定能量仓初始为显式 0，避免空对象的 undefined 绕过资源预检。
        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType": "tower", "owner": "p1", "pos": [3, 3]}),
        )?;
        let run = e.run_player("p1", &format!("(() => {{ const tower = Game.getObjectById({tower:?}); return [tower.attack(Game.creeps.miner), tower.heal(Game.creeps.miner), tower.repair(Game.getObjectById({container:?}))]; }})()"))?;
        assert_eq!(run.expr["value"], json!([-6, -6, -6]));
        e.teardown();
    }

    // 默认 GCL 比当前房间数多一级，真实玩家预检和结算均能认领下一房。
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({"players": ["p1"], "rooms": [
        {"name": "W1N1", "owner": "p1"}, {"name": "W1N2", "owner": "p1"}, {"name": "W1N3"}
    ]}))?;
    e.place_object(
        "W1N3",
        "creep",
        &json!({"pos": [1, 2], "owner": "p1", "name": "claimer", "body": ["claim", "move"]}),
    )?;
    let run = e.run_player(
        "p1",
        "[Game.gcl.level, Game.creeps.claimer.claimController(Game.rooms.W1N3.controller)]",
    )?;
    assert_eq!(run.expr["value"], json!([3, 0]));
    let run = e.run_player("p1", "Game.rooms.W1N3.controller.my")?;
    assert_eq!(run.expr["value"], true);
    e.teardown();
    Ok(())
}
