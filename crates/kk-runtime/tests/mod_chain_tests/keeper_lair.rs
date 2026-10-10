//! keeper lair 的倒计时、真实出生、替换及恢复边界。
use super::*;
use kk_core::constants as c;
use kk_world::domain::keeper::OWNER;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1"],"rooms":[
        {"name":"W1N1","owner":"p1","rcl":8},{"name":"W2N1"}]}))?;
    Ok(e)
}
fn lair(e: &mut EmbedEngine, ttl: Option<u64>, pos: [u8; 2]) -> anyhow::Result<String> {
    let mut spec = json!({"pos":pos});
    if let Some(ttl) = ttl {
        spec["ticksToSpawn"] = json!(ttl);
    }
    e.place_object("W1N1", "keeperLair", &spec)
}
fn keepers(e: &EmbedEngine) -> Vec<Value> {
    e.find_in_room("W1N1", c::FIND_CREEPS)
        .into_iter()
        .filter(|c| c["owner"] == OWNER)
        .collect()
}

#[test]
fn keeper_countdown_spawn_shape_and_full_health_suppresses_restart() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = lair(&mut e, Some(3), [25, 25])?;
        assert_eq!(e.get_object(&id).unwrap()["ticksToSpawn"], 3);
        e.tick(1)?;
        let code = format!(
            "const l=Game.getObjectById('{id}');[l.ticksToSpawn,l.my,l.owner.username,l instanceof StructureKeeperLair,Game.rooms.W1N1.find(FIND_STRUCTURES).some(s=>s.id===l.id),Game.rooms.W1N1.find(FIND_HOSTILE_STRUCTURES).some(s=>s.id===l.id),Game.structures[l.id]===undefined]"
        );
        assert_eq!(
            e.run_player("p1", &code)?.expr["value"],
            json!([2, false, OWNER, true, true, true, true])
        );
        assert!(keepers(&e).is_empty());
        assert_eq!(e.run_player("p1", &code)?.expr["value"][0], 1);
        let keeper = keepers(&e).remove(0);
        assert_eq!(keeper["name"], format!("Keeper{id}"));
        assert_eq!(keeper["owner"], OWNER);
        assert_eq!(keeper["hits"], 5000);
        assert!(keeper["storeCapacity"].is_null());
        assert!(keeper["ticksToLive"].is_null(), "出生当拍尚未初始化寿命");
        let body = keeper["body"].as_array().unwrap();
        assert_eq!(body.len(), 50);
        assert!(body[..17].iter().all(|p| p["type"] == "tough"));
        assert!(body[17..30].iter().all(|p| p["type"] == "move"));
        for pair in body[30..].as_chunks::<2>().0 {
            assert_eq!(pair[0]["type"], "attack");
            assert_eq!(pair[1]["type"], "ranged_attack");
        }
        let value = e.run_player("p1", "const k=Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0];[k.my,k.owner.username,k.ticksToLive===undefined,k.store.getCapacity(),Object.keys(Game.creeps).length]")?.expr["value"].clone();
        assert_eq!(value, json!([false, OWNER, true, null, 0]));
        e.tick(3)?;
        assert_eq!(keepers(&e).len(), 1);
        assert!(e.get_object(&id).unwrap()["ticksToSpawn"].is_null());
        assert!(keepers(&e)[0]["ticksToLive"].as_u64().unwrap() < 1500);
    }
    Ok(())
}

#[test]
fn keeper_missing_or_wounded_starts_timer_and_replacement_has_no_death_artifacts()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = lair(&mut e, Some(1), [25, 25])?;
        e.tick(1)?;
        let old = keepers(&e)[0]["id"].as_str().unwrap().to_owned();
        e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[25,24],"structureType":"tower","owner":"p1","store":{"energy":1000}}),
        )?;
        e.run_player("p1", &format!("Game.rooms.W1N1.find(FIND_MY_STRUCTURES).find(s=>s.structureType===STRUCTURE_TOWER).attack(Game.getObjectById('{old}'))"))?;
        assert!(keepers(&e)[0]["hits"].as_u64().unwrap() < 5000);
        e.tick(1)?;
        assert_eq!(e.get_object(&id).unwrap()["ticksToSpawn"], 299);
        e.tick(298)?;
        assert_eq!(e.get_object(&id).unwrap()["ticksToSpawn"], 1);
        assert!(e.get_object(&old).is_some());
        e.tick(1)?;
        assert!(e.get_object(&old).is_none());
        let new = keepers(&e);
        assert_eq!(new.len(), 1);
        assert_ne!(new[0]["id"], old);
        assert_eq!(new[0]["hits"], 5000);
        assert!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).is_empty());
        assert!(harness::events(&e, c::EVENT_OBJECT_DESTROYED).is_empty());
        let missing = lair(&mut e, None, [20, 20])?;
        e.tick(1)?;
        assert_eq!(e.get_object(&missing).unwrap()["ticksToSpawn"], 299);
    }
    Ok(())
}

#[test]
fn keeper_spawn_retry_and_cold_restore_preserve_ids_and_deadline() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = lair(&mut e, Some(3), [25, 25])?;
        e.tick(2)?;
        let saved = e.checkpoint(Value::Null)?;
        let mut reference = EmbedEngine::new();
        reference.set_kkpv_channel(binary);
        reference.restore_checkpoint(saved.clone(), None)?;
        let code = "Memory.runs=(Memory.runs||0)+1";
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", code).is_err());
        assert!(e.checkpoint(Value::Null).is_err(), "开放失败候选不可保存");
        assert!(keepers(&e).is_empty());
        assert_eq!(e.get_object(&id).unwrap()["ticksToSpawn"], 1);
        reference.run_player("p1", code)?;
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(e.world_snapshot(), reference.world_snapshot());
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        for ticks in [1, 3, 301] {
            e.tick(ticks)?;
            reference.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), reference.world_snapshot());
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}

#[test]
fn keeper_edge_does_not_cross_rooms_and_invalid_setup_is_atomic() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let saved = e.checkpoint(Value::Null)?;
        assert!(
            e.create_shard(&json!({"players":[OWNER],"rooms":[]}))
                .is_err()
        );
        assert_eq!(e.checkpoint(Value::Null)?, saved);
        for key in ["ticksToSpawn", "nextSpawnTime"] {
            let mut spec = json!({"pos":[0,25]});
            spec[key] = json!(u64::MAX);
            assert!(e.place_object("W1N1", "keeperLair", &spec).is_err());
            assert_eq!(e.checkpoint(Value::Null)?, saved);
        }
        let id = lair(&mut e, Some(1), [0, 25])?;
        e.tick(3)?;
        assert_eq!(keepers(&e).len(), 1);
        assert!(e.find_in_room("W2N1", c::FIND_CREEPS).is_empty());
        assert!(harness::events(&e, c::EVENT_EXIT).is_empty());
        let saved = e.checkpoint(Value::Null)?;
        let mut bad = saved.clone();
        let bytes: Vec<u8> =
            serde_json::from_value(bad["engine"]["world"]["rooms"]["W1N1"]["snapshot"].clone())?;
        let mut snapshot = kk_schema::room::decode_snapshot(&bytes)?;
        let lair = snapshot
            .objects
            .iter_mut()
            .find(|o| o.id() == kk_world::ObjId::parse(&id).unwrap().0)
            .unwrap();
        let kk_schema::room::RoomObject::Generic(g) = lair else {
            panic!("lair 应为 Generic")
        };
        let mut data: Value = serde_json::from_str(&g.data)?;
        data["nextSpawnTime"] = json!(u64::MAX);
        g.data = data.to_string().into();
        bad["engine"]["world"]["rooms"]["W1N1"]["snapshot"] =
            json!(kk_schema::room::encode_snapshot(&snapshot));
        assert!(e.restore_checkpoint(bad, None).is_err());
        assert_eq!(
            e.checkpoint(Value::Null)?,
            saved,
            "损坏时钟不能替换运行世界"
        );
    }
    Ok(())
}
