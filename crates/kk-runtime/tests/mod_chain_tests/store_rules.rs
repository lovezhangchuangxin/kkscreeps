//! JS 数值覆盖必须贯穿玩家预检、权威写入、建造与失败重试。
use super::*;

fn stores(binary: bool) -> anyhow::Result<EmbedEngine> {
    engine(
        &[(
            "alpha",
            package(
                r#"
        api.rules.override('builtin:store.container.capacity', 37);
        api.rules.override('builtin:store.factory.capacity', 41);
        api.rules.override('builtin:store.terminal.capacity', 43);
        api.rules.override('builtin:store.tower.capacity', 47);
        api.rules.override('builtin:store.lab.energyCapacity', 17);
        api.rules.override('builtin:store.lab.mineralCapacity', 23);
        api.rules.override('builtin:store.powerSpawn.energyCapacity', 53);
        api.rules.override('builtin:store.powerSpawn.powerCapacity', 7);
        api.rules.override('builtin:store.nuker.energyCapacity', 59);
        api.rules.override('builtin:store.nuker.ghodiumCapacity', 11);
    "#,
            ),
        )],
        binary,
    )
}

fn place(e: &mut EmbedEngine, kind: &str, x: u8, extra: Value) -> anyhow::Result<String> {
    let mut spec = json!({"structureType":kind,"pos":[x,10],"owner":"p1"});
    spec.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    e.place_object("W1N1", "structure", &spec)
}

#[test]
fn js_capacity_overrides_reach_projection_constants_and_terminal_ticks() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = stores(binary)?;
        let cases = [
            ("container", 37, json!(null)),
            ("factory", 41, json!(null)),
            ("terminal", 43, json!(null)),
            ("tower", 47, json!({"energy":47})),
            ("lab", 40, json!({"energy":17})),
            ("powerSpawn", 60, json!({"energy":53,"power":7})),
            ("nuker", 70, json!({"energy":59,"G":11})),
        ];
        for (i, (kind, total, slots)) in cases.iter().enumerate() {
            let id = place(&mut e, kind, 10 + i as u8, json!({}))?;
            let object = e.get_object(&id).unwrap();
            assert_eq!(object["storeCapacity"], *total, "{kind}");
            assert_eq!(object["storeCapacityResource"], *slots, "{kind}");
            let run = e.run_player("p1",&format!("(() => {{ const o=Game.getObjectById({id:?}); return [o.store.getCapacity(),o.store.getCapacity(RESOURCE_ENERGY)]; }})()"))?;
            assert_eq!(run.expr["ok"], true, "{}", run.expr);
            let energy = if slots.is_null() {
                json!(total)
            } else {
                slots["energy"].clone()
            };
            assert_eq!(
                run.expr["value"],
                json!([
                    if slots.is_null() {
                        json!(total)
                    } else {
                        Value::Null
                    },
                    energy
                ]),
                "{kind}"
            );
            // terminal 在本轮 tick 归一后不能回到官方固定容量。
            assert_eq!(e.get_object(&id).unwrap()["storeCapacity"], *total);
        }
        let run=e.run_player("p1","[CONTAINER_CAPACITY,FACTORY_CAPACITY,TERMINAL_CAPACITY,TOWER_CAPACITY,LAB_ENERGY_CAPACITY,LAB_MINERAL_CAPACITY,POWER_SPAWN_ENERGY_CAPACITY,POWER_SPAWN_POWER_CAPACITY,NUKER_ENERGY_CAPACITY,NUKER_GHODIUM_CAPACITY]")?;
        assert_eq!(
            run.expr["value"],
            json!([37, 41, 43, 47, 17, 23, 53, 7, 59, 11])
        );
        let explicit = place(&mut e, "container", 20, json!({"storeCapacity":19}))?;
        assert_eq!(e.get_object(&explicit).unwrap()["storeCapacity"], 19);
        let occupied = place(&mut e, "lab", 21, json!({"store":{"U":1}}))?;
        assert_eq!(
            e.get_object(&occupied).unwrap()["storeCapacityResource"]["U"],
            23
        );
        e.teardown();
    }
    Ok(())
}

#[test]
fn overridden_capacity_clamps_real_transfer_and_retries_once() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = stores(binary)?;
        let container = place(&mut e, "container", 10, json!({"store":{"H":7}}))?;
        let actor=e.place_object("W1N1","creep",&json!({"name":"hauler","owner":"p1","pos":[10,9],"body":["carry","move"],"store":{"energy":50}}))?;
        let second=e.place_object("W1N1","creep",&json!({"name":"second","owner":"p1","pos":[9,10],"body":["carry","move"],"store":{"energy":50}}))?;
        let before = e.world_snapshot();
        let memory_before = e.player_state("p1").unwrap().raw_memory.clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1",&format!("Memory.count=(Memory.count||0)+1;[Game.creeps.hauler.transfer(Game.getObjectById({container:?}),RESOURCE_ENERGY),Game.creeps.second.transfer(Game.getObjectById({container:?}),RESOURCE_ENERGY)]")).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(e.player_state("p1").unwrap().raw_memory, memory_before);
        // 待重试轮次忽略此脚本；原已执行玩家结果只提交一次。
        let run = e.run_player("p1", "throw Error('cannot rerun')")?;
        assert_eq!(run.expr["value"], json!([0, 0]));
        assert_eq!(e.get_object(&container).unwrap()["store"]["energy"], 30);
        assert_eq!(e.get_object(&actor).unwrap()["store"]["energy"], 20);
        assert_eq!(e.get_object(&second).unwrap()["store"]["energy"], 50);
        assert_eq!(e.get_object(&container).unwrap()["store"]["H"], 7);
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"count\":1}");
        let run = e.run_player(
            "p1",
            &format!(
                "Game.creeps.hauler.transfer(Game.getObjectById({container:?}),RESOURCE_ENERGY,1)"
            ),
        )?;
        assert_eq!(run.expr["value"], -8, "仓库满时玩家预检拒绝");
        e.teardown();
    }
    Ok(())
}

#[test]
fn lab_slots_and_new_build_use_the_same_frozen_capacity() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = stores(binary)?;
        let lab = place(&mut e, "lab", 10, json!({}))?;
        let actor=e.place_object("W1N1","creep",&json!({"name":"hauler","owner":"p1","pos":[10,9],"body":["carry","move"],"store":{"U":30}}))?;
        let run = e.run_player(
            "p1",
            &format!("Game.creeps.hauler.transfer(Game.getObjectById({lab:?}),'U')"),
        )?;
        assert_eq!(run.expr["value"], 0, "{}", run.expr);
        let object = e.get_object(&lab).unwrap();
        assert_eq!(object["store"]["U"], 23);
        assert_eq!(object["storeCapacityResource"]["U"], 23);
        assert_eq!(object["storeCapacity"], Value::Null);
        let run=e.run_player("p1",&format!("[Game.getObjectById({lab:?}).store.getCapacity('U'),Game.creeps.hauler.withdraw(Game.getObjectById({lab:?}),'U',23)]"))?;
        assert_eq!(run.expr["value"], json!([23, 0]));
        assert_eq!(e.get_object(&lab).unwrap()["storeCapacity"], 40);
        assert_eq!(
            e.get_object(&lab).unwrap()["storeCapacityResource"]["U"],
            Value::Null
        );
        assert_eq!(e.get_object(&actor).unwrap()["store"]["U"], 30);

        let builder=e.place_object("W1N1","creep",&json!({"name":"builder","owner":"p1","pos":[20,9],"body":["work","carry","move"],"store":{"energy":10}}))?;
        let site = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"structureType":"container","owner":"p1","pos":[20,10],"progress":4995}),
        )?;
        let run = e.run_player(
            "p1",
            &format!("Game.creeps.builder.build(Game.getObjectById({site:?}))"),
        )?;
        assert_eq!(run.expr["value"], 0, "{}", run.expr);
        assert!(e.get_object(&site).is_none());
        let built = e
            .find_in_room("W1N1", kk_core::constants::FIND_STRUCTURES)
            .into_iter()
            .find(|o| o["structureType"] == "container")
            .unwrap();
        assert_eq!(built["storeCapacity"], 37);
        let run = e.run_player(
            "p1",
            &format!(
                "Game.creeps.builder.transfer(Game.getObjectById({:?}),RESOURCE_ENERGY)",
                built["id"].as_str().unwrap()
            ),
        )?;
        assert_eq!(run.expr["value"], 0);
        assert_eq!(
            e.get_object(&builder).unwrap()["store"]["energy"]
                .as_i64()
                .unwrap_or(0),
            0
        );
        assert_eq!(
            e.get_object(built["id"].as_str().unwrap()).unwrap()["store"]["energy"],
            5
        );
        e.teardown();
    }
    Ok(())
}

#[test]
fn zero_capacity_and_player_install_canary_use_effective_rules() -> anyhow::Result<()> {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-store-player-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    let dir = root.0.join("alpha");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("mod.json"),
        json!({"id":"alpha","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"})
            .to_string(),
    )?;
    std::fs::write(
        dir.join("server.js"),
        package(
            "api.rules.override('builtin:store.container.capacity',0);api.rules.override('builtin:store.lab.mineralCapacity',23);api.rules.override('builtin:link.capacity',71);",
        ),
    )?;
    std::fs::write(
        dir.join("player.js"),
        "export default function install(api) { if (CONTAINER_CAPACITY !== 0 || LAB_MINERAL_CAPACITY !== 23 || LINK_CAPACITY !== 71) throw Error('wrong rule constants during install'); }",
    )?;
    let packages = discover_packages(&root.0)?;
    for binary in [false, true] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard_with_mods(
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6}]}),
            &packages,
            RegisterOptions::default(),
        )?;
        let container = place(&mut e, "container", 10, json!({}))?;
        assert_eq!(e.get_object(&container).unwrap()["storeCapacity"], 0);
        let actor=e.place_object("W1N1","creep",&json!({"name":"hauler","owner":"p1","pos":[10,9],"body":["carry"],"store":{"energy":5}}))?;
        let run = e.run_player(
            "p1",
            &format!(
                "Game.creeps.hauler.transfer(Game.getObjectById({container:?}),RESOURCE_ENERGY,1)"
            ),
        )?;
        assert_eq!(
            run.expr["value"], -7,
            "零容量目标按现有玩家预检返回 ERR_INVALID_TARGET"
        );
        assert_eq!(e.get_object(&actor).unwrap()["store"]["energy"], 5);
        assert_eq!(e.get_object(&container).unwrap()["storeCapacity"], 0);
        e.teardown();
    }
    Ok(())
}

#[test]
fn automatic_container_deposit_uses_overridden_capacity_and_preserves_overflow()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = stores(binary)?;
        let container = place(
            &mut e,
            "container",
            10,
            json!({"store":{"H":7,"energy":27}}),
        )?;
        let actor=e.place_object("W1N1","creep",&json!({"name":"hauler","owner":"p1","pos":[10,10],"body":["carry"],"store":{"energy":10}}))?;
        let run = e.run_player("p1", "Game.creeps.hauler.drop(RESOURCE_ENERGY,10)")?;
        assert_eq!(run.expr["value"], 0);
        assert_eq!(e.get_object(&container).unwrap()["store"]["energy"], 30);
        assert_eq!(e.get_object(&container).unwrap()["store"]["H"], 7);
        assert_eq!(
            e.get_object(&actor).unwrap()["store"]["energy"]
                .as_i64()
                .unwrap_or(0),
            0
        );
        let drops = e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES);
        assert_eq!(drops.len(), 1);
        // 只承接余量 3，剩余 7 在本轮资源 tick 衰减 1，最终保留 6。
        assert_eq!(drops[0]["amount"], 6);
        assert_eq!(3 + drops[0]["amount"].as_i64().unwrap() + 1, 10);
        e.teardown();
    }
    Ok(())
}
