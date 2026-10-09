//! Invader 退场动作、零回收墓碑、守卫与候选恢复边界。
use super::*;
use kk_core::constants as c;
use kk_world::domain::npc::INVADER_OWNER;

fn fixture(binary: bool, owned: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    let room = if owned {
        json!({"name":"W1N1","owner":"p1","rcl":6})
    } else {
        json!({"name":"W1N1"})
    };
    e.create_shard(&json!({"players":["p1","2"],"rooms":[room,{"name":"W2N1"}]}))?;
    Ok(e)
}
fn invader(e: &mut EmbedEngine, body: &[&str], ttl: u64) -> anyhow::Result<String> {
    e.place_object("W1N1", "creep", &json!({"pos":[25,25],"owner":INVADER_OWNER,
        "name":"retiring","body":body,"ticksToLive":ttl,"store":{"energy":30,"UH":20},"boosts":{"0":"UH"}}))
}

// 真实保存格式构造放置接口未开放的孵化/受损形态，恢复校验后再执行。
fn restore_creep_shape(e: &mut EmbedEngine, id: &str, shape: &str) -> anyhow::Result<()> {
    let mut saved = e.checkpoint(Value::Null)?;
    let bytes: Vec<u8> =
        serde_json::from_value(saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"].clone())?;
    let mut room = kk_schema::room::decode_snapshot(&bytes)?;
    let object = room
        .objects
        .iter_mut()
        .find(|o| o.id() == kk_world::ObjId::parse(id).unwrap().0)
        .unwrap();
    let kk_schema::room::RoomObject::Creep(creep) = object else {
        panic!("预期 creep")
    };
    if shape == "damagedHealer" {
        creep.body[0].hits = 0;
        creep.hits = 200;
    } else if shape == "injuredWithHealer" {
        creep.body[0].hits = 0;
        creep.body[1].hits = 0;
        creep.hits = 100;
    } else {
        creep.spawning = true;
    }
    saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"] =
        json!(kk_schema::room::encode_snapshot(&room));
    e.restore_checkpoint(saved, None)?;
    Ok(())
}

#[test]
fn npc_retirement_creates_empty_tomb_without_container_reclaim() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary, true)?;
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[25,25],"structureType":"container","store":{"energy":1}}),
        )?;
        let id = invader(&mut e, &["attack", "move", "carry"], 1500)?;
        let keeper=e.place_object("W1N1","creep",&json!({"pos":[30,30],"owner":kk_world::domain::keeper::OWNER,"body":["move"],"name":"keeper"}))?;
        let value=e.run_player("p1", &format!("const n=Game.getObjectById('{id}');[n.owner.username,n.my,Game.creeps.retiring===undefined]"))?.expr["value"].clone();
        assert_eq!(value, json!([INVADER_OWNER, false, true]));
        assert!(e.get_object(&id).is_none());
        assert!(e.get_object(&keeper).is_some(), "keeper 不走 Invader 退场");
        assert_eq!(e.get_object(&container).unwrap()["store"]["energy"], 1);
        let tombs = e.find_in_room("W1N1", c::FIND_TOMBSTONES);
        assert_eq!(tombs.len(), 1);
        assert_eq!(tombs[0]["creepName"], "retiring");
        assert_eq!(tombs[0]["store"], json!({}));
        assert_eq!(
            tombs[0]["ticksToDecay"],
            3 * c::TOMBSTONE_DECAY_PER_PART - 1
        );
        let events = harness::events(&e, c::EVENT_OBJECT_DESTROYED);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["objectId"], id);
        assert_eq!(events[0]["data"]["type"], "creep");
        assert!(e.find_in_room("W1N1", c::FIND_DROPPED_RESOURCES).is_empty());
        let code = format!(
            "const t=Game.rooms.W1N1.find(FIND_TOMBSTONES)[0];[t.creep.owner.username,Object.keys(t.store),t instanceof Tombstone,t.creep.id===\"{id}\"]"
        );
        assert_eq!(
            e.run_player("p1", &code)?.expr["value"],
            json!([INVADER_OWNER, [], true, true])
        );
    }
    Ok(())
}

#[test]
fn npc_retirement_guards_leave_unimplemented_target_branches_alive() -> anyhow::Result<()> {
    for binary in [false, true] {
        for guard in [
            "unowned",
            "spawn",
            "creep",
            "powerCreep",
            "healer",
            "damagedHealer",
            "spawning",
            "injuredWithHealer",
        ] {
            let mut e = fixture(binary, guard != "unowned")?;
            let body = if matches!(guard, "healer" | "damagedHealer") {
                vec!["heal", "move", "carry"]
            } else {
                vec!["attack", "move", "carry"]
            };
            let id = invader(&mut e, &body, 1500)?;
            match guard {
                "spawn" => {
                    e.place_object(
                        "W1N1",
                        "structure",
                        &json!({"pos":[10,10],"structureType":"spawn","owner":"p1","name":"Home"}),
                    )?;
                }
                "creep" => {
                    e.place_object(
                        "W1N1",
                        "creep",
                        &json!({"pos":[10,10],"owner":"p1","body":["move"],"name":"target"}),
                    )?;
                }
                "powerCreep" => {
                    e.place_object("W1N1","powerCreep",&json!({"pos":[10,10],"owner":"p1","name":"powerTarget","ticksToLive":1000}))?;
                }
                "injuredWithHealer" => {
                    e.place_object("W1N1", "creep", &json!({"pos":[24,25],"owner":INVADER_OWNER,"body":["heal","move"],"name":"helper"}))?;
                }
                _ => {}
            }
            if matches!(guard, "damagedHealer" | "spawning" | "injuredWithHealer") {
                restore_creep_shape(&mut e, &id, guard)?;
            }
            e.tick(1)?;
            if guard == "spawning" {
                // 无 spawn 的孵化孤儿由既有对象 tick 直接删除，不能被当成
                // NPC 自杀发射死亡事件或创建墓碑。
                assert!(e.get_object(&id).is_none());
                assert!(harness::events(&e, c::EVENT_OBJECT_DESTROYED).is_empty());
            } else {
                assert!(e.get_object(&id).is_some(), "{guard} 分支不得误退场");
            }
            assert!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).is_empty());
        }
    }
    Ok(())
}

#[test]
fn npc_retirement_retry_and_cold_restore_produce_one_identical_death() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary, true)?;
        let id = invader(&mut e, &["attack", "move", "carry"], 1500)?;
        let saved = e.checkpoint(Value::Null)?;
        let mut reference = EmbedEngine::new();
        reference.set_kkpv_channel(binary);
        reference.restore_checkpoint(saved, None)?;
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.runs=(Memory.runs||0)+1")
                .is_err()
        );
        assert!(e.get_object(&id).is_some());
        assert!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).is_empty());
        assert!(e.checkpoint(Value::Null).is_err());
        reference.run_player("p1", "Memory.runs=(Memory.runs||0)+1")?;
        e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(e.world_snapshot(), reference.world_snapshot());
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        assert_eq!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).len(), 1);
        for ticks in [1, 10, 5] {
            e.tick(ticks)?;
            reference.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), reference.world_snapshot());
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert!(
            e.find_in_room("W1N1", c::FIND_TOMBSTONES).is_empty(),
            "墓碑正常衰减"
        );
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}

#[test]
fn npc_identity_is_exact_and_ttl_death_keeps_normal_reclaim() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary, false)?;
        let saved = e.checkpoint(Value::Null)?;
        assert!(
            e.create_shard(&json!({"players":[INVADER_OWNER],"rooms":[]}))
                .is_err()
        );
        assert_eq!(e.checkpoint(Value::Null)?, saved);
        let invader_id = invader(&mut e, &["attack", "move", "carry"], 1)?;
        e.tick(1)?;
        assert!(e.get_object(&invader_id).is_none());
        assert_eq!(
            e.find_in_room("W1N1", c::FIND_TOMBSTONES)[0]["store"]["energy"],
            30,
            "TTL 死亡仍转移携带资源"
        );
        let short=e.place_object("W1N1","creep",&json!({"pos":[20,20],"owner":"2","name":"shortPlayer","body":["carry"],"store":{"energy":40}}))?;
        e.run_player("2", &format!("Game.getObjectById('{short}').suicide()"))?;
        let tomb = e
            .find_in_room("W1N1", c::FIND_TOMBSTONES)
            .into_iter()
            .find(|t| t["creepName"] == "shortPlayer")
            .unwrap();
        assert!(
            tomb["store"]["energy"].as_u64().unwrap() >= 40,
            "短玩家名不能变成零回收 NPC"
        );
        let edge = e.place_object(
            "W1N1",
            "creep",
            &json!({"pos":[0,25],"owner":INVADER_OWNER,"name":"edgeNpc","body":["move"]}),
        )?;
        e.tick(1)?;
        harness::pos_in(&e, "W1N1", &edge);
        assert!(harness::events(&e, c::EVENT_EXIT).is_empty());
    }
    Ok(())
}
