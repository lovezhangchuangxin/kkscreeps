//! 普通 Invader 追击、逃离、治疗与私有路径保存恢复。
use super::*;
use harness::{hits, pos_of};
use kk_core::constants as c;
use kk_world::domain::npc::INVADER_OWNER;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1"],"rooms":[{"name":"W1N1"}]}))?;
    Ok(e)
}

fn creep(
    e: &mut EmbedEngine,
    name: &str,
    owner: &str,
    pos: [u8; 2],
    body: &[&str],
) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({"name":name,"owner":owner,"pos":pos,"body":body,"ticksToLive":1500}),
    )
}

// 放置接口不提供损毁部件/孵化标记；从真实保存格式构造并经恢复校验。
fn edit_object(
    e: &mut EmbedEngine,
    id: &str,
    edit: impl FnOnce(&mut kk_schema::room::RoomObject),
) -> anyhow::Result<()> {
    let mut saved = e.checkpoint(Value::Null)?;
    let bytes: Vec<u8> =
        serde_json::from_value(saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"].clone())?;
    let mut room = kk_schema::room::decode_snapshot(&bytes)?;
    edit(
        room.objects
            .iter_mut()
            .find(|o| o.id() == kk_world::ObjId::parse(id).unwrap().0)
            .unwrap(),
    );
    saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"] =
        json!(kk_schema::room::encode_snapshot(&room));
    e.restore_checkpoint(saved, None)?;
    Ok(())
}

fn memory(e: &mut EmbedEngine, id: &str) -> anyhow::Result<Value> {
    Ok(
        e.checkpoint(Value::Null)?["engine"]["world"]["rooms"]["W1N1"]["invader_memory"]
            [kk_world::ObjId::parse(id).unwrap().0.to_string()]
        .clone(),
    )
}

fn injure(e: &mut EmbedEngine, id: &str, damage: u32) -> anyhow::Result<()> {
    edit_object(e, id, |o| {
        let kk_schema::room::RoomObject::Creep(c) = o else {
            unreachable!()
        };
        let mut remaining = damage;
        for part in &mut c.body {
            let loss = remaining.min(part.hits);
            part.hits -= loss;
            remaining -= loss;
        }
        c.hits -= damage;
    })
}

#[test]
fn invader_ai_chases_far_target_and_reuses_five_tick_path_with_private_last_move()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let actor = creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["attack", "move"],
        )?;
        let target = creep(&mut e, "target", "p1", [20, 10], &["move", "move"])?;
        e.tick(1)?;
        assert_ne!(pos_of(&e, &actor), (10, 10));
        let first = memory(&mut e, &actor)?;
        assert!(first["last_move"].is_number());
        assert!(first["move_cache"]["path"].as_array().unwrap().len() > 1);
        e.tick(5)?;
        assert_eq!(memory(&mut e, &actor)?["move_cache"], first["move_cache"]);
        e.tick(1)?;
        assert_eq!(
            memory(&mut e, &actor)?["move_cache"]["time"]
                .as_u64()
                .unwrap(),
            first["move_cache"]["time"].as_u64().unwrap() + 6
        );
        e.tick(5)?;
        assert!(hits(&e, &target) < 200);
        assert_eq!(e.run_player("p1",&format!("const k=Game.getObjectById('{actor}');[k!==null,'memory_move' in k,'invader_memory' in k,'last_move' in k]"))?.expr["value"],json!([true,false,false,false]));
    }
    Ok(())
}

#[test]
fn invader_ai_ranged_flees_near_hostile_but_still_shoots() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let actor = creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["ranged_attack", "move"],
        )?;
        let target = creep(&mut e, "target", "p1", [11, 10], &["move", "move"])?;
        e.tick(1)?;
        let p = pos_of(&e, &actor);
        assert!(p.0.abs_diff(11).max(p.1.abs_diff(10)) > 1);
        assert_eq!(hits(&e, &target), 190);
        assert_eq!(
            memory(&mut e, &actor)?,
            Value::Null,
            "flee 不调用 moveTo，不写路径缓存"
        );
    }
    Ok(())
}

#[test]
fn invader_ai_healer_selects_greatest_missing_hits_then_follows_injured() -> anyhow::Result<()> {
    for binary in [false, true] {
        for ranged in [false, true] {
            let mut e = fixture(binary)?;
            let healer = creep(&mut e, "healer", INVADER_OWNER, [10, 10], &["heal", "move"])?;
            let little = creep(&mut e, "little", INVADER_OWNER, [9, 10], &["move", "move"])?;
            let hurt = creep(
                &mut e,
                "hurt",
                INVADER_OWNER,
                if ranged { [13, 10] } else { [11, 10] },
                &["move", "move", "move"],
            )?;
            injure(&mut e, &little, 20)?;
            injure(&mut e, &hurt, 100)?;
            e.tick(1)?;
            assert_eq!(hits(&e, &little), 180);
            assert_eq!(hits(&e, &hurt), if ranged { 204 } else { 212 });
            let events = harness::events(&e, c::EVENT_HEAL);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["objectId"], healer);
            assert_eq!(events[0]["data"]["targetId"], hurt);
            if ranged {
                assert_eq!(
                    pos_of(&e, &healer),
                    (10, 10),
                    "跟随选择最近伤员 little，与治疗评分目标不同，邻格占用阻止移动"
                );
                assert_eq!(
                    memory(&mut e, &healer)?,
                    Value::Null,
                    "直接邻格 move 不调用 moveTo"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn invader_ai_healer_tie_keeps_first_friend_and_lone_full_healer_retires() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let first = creep(&mut e, "first", INVADER_OWNER, [11, 10], &["move", "move"])?;
        let second = creep(&mut e, "second", INVADER_OWNER, [10, 11], &["move", "move"])?;
        creep(&mut e, "healer", INVADER_OWNER, [10, 10], &["heal", "move"])?;
        injure(&mut e, &first, 30)?;
        injure(&mut e, &second, 30)?;
        e.tick(1)?;
        assert_eq!(hits(&e, &first), 182);
        assert_eq!(hits(&e, &second), 170);
        let mut lone = fixture(binary)?;
        let id = creep(
            &mut lone,
            "lone",
            INVADER_OWNER,
            [10, 10],
            &["heal", "move"],
        )?;
        lone.tick(1)?;
        assert!(lone.get_object(&id).is_none());
        assert_eq!(lone.find_in_room("W1N1", c::FIND_TOMBSTONES).len(), 1);
    }
    Ok(())
}

#[test]
fn invader_ai_injured_healer_flees_and_weak_fighter_seeks_healer() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let healer = creep(
            &mut e,
            "healer",
            INVADER_OWNER,
            [10, 10],
            &["tough", "tough", "heal", "move"],
        )?;
        injure(&mut e, &healer, 250)?;
        creep(&mut e, "enemy", "p1", [11, 10], &["move"])?;
        e.tick(1)?;
        let p = pos_of(&e, &healer);
        assert!(p.0.abs_diff(11).max(p.1.abs_diff(10)) > 1);
        assert_eq!(hits(&e, &healer), 162, "逃离前仍治疗自己");
        let mut e = fixture(binary)?;
        let fighter = creep(
            &mut e,
            "fighter",
            INVADER_OWNER,
            [10, 10],
            &["tough", "tough", "move"],
        )?;
        injure(&mut e, &fighter, 200)?;
        creep(&mut e, "healer", INVADER_OWNER, [16, 10], &["heal", "move"])?;
        e.tick(1)?;
        assert_ne!(pos_of(&e, &fighter), (10, 10));
        assert!(memory(&mut e, &fighter)?["move_cache"].is_object());
    }
    Ok(())
}

#[test]
fn invader_ai_path_retry_cold_resume_and_deletion_preserve_private_memory() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let actor = creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["attack", "move"],
        )?;
        creep(
            &mut e,
            "target",
            "p1",
            [25, 10],
            &["move", "move", "move", "move"],
        )?;
        let before = e.checkpoint(Value::Null)?;
        let mut reference = EmbedEngine::new();
        reference.set_kkpv_channel(binary);
        reference.restore_checkpoint(before.clone(), None)?;
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.runs=(Memory.runs||0)+1")
                .is_err()
        );
        assert_eq!(pos_of(&e, &actor), (10, 10));
        e.discard_pending_round();
        assert_eq!(e.checkpoint(Value::Null)?, before, "路径写入随失败候选丢弃");
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.runs=(Memory.runs||0)+1")
                .is_err()
        );
        reference.run_player("p1", "Memory.runs=(Memory.runs||0)+1")?;
        e.run_player("p1", "throw Error('不得重跑')")?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        for ticks in [1, 5, 7] {
            e.tick(ticks)?;
            reference.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), reference.world_snapshot());
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
            assert_eq!(memory(&mut e, &actor)?, memory(&mut reference, &actor)?);
            assert_eq!(memory(&mut e, &actor)?, memory(&mut restored, &actor)?);
        }
        e.settle_external(kk_engine::intent::IntentBatch::from_json(
            INVADER_OWNER,
            &json!({&actor:{"suicide":{}}}),
        ))?;
        assert!(e.get_object(&actor).is_none());
        assert_eq!(memory(&mut e, &actor)?, Value::Null);
    }
    Ok(())
}

#[test]
fn invader_ai_invalid_memory_and_missing_domain_reject_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["attack", "move"],
        )?;
        creep(&mut e, "target", "p1", [20, 10], &["move"])?;
        e.tick(1)?;
        let saved = e.checkpoint(Value::Null)?;
        let key = kk_world::ObjId::parse(&id).unwrap().0.to_string();
        assert_eq!(saved["checkpoint_format_version"], 6);
        for defect in [
            "domain",
            "actor",
            "time",
            "lastMove",
            "destination",
            "step",
            "field",
        ] {
            let mut bad = saved.clone();
            let room = &mut bad["engine"]["world"]["rooms"]["W1N1"];
            match defect {
                "domain" => {
                    room.as_object_mut().unwrap().remove("invader_memory");
                }
                "actor" => {
                    let value = room["invader_memory"]
                        .as_object_mut()
                        .unwrap()
                        .remove(&key)
                        .unwrap();
                    room["invader_memory"]["0"] = value;
                }
                "time" => room["invader_memory"][&key]["move_cache"]["time"] = json!(u64::MAX),
                "lastMove" => room["invader_memory"][&key]["last_move"] = json!(u64::MAX),
                "destination" => {
                    room["invader_memory"][&key]["move_cache"]["destination"] = json!([50, 10])
                }
                "step" => {
                    room["invader_memory"][&key]["move_cache"]["path"] = json!([[11, 10], [14, 10]])
                }
                "field" => {
                    room["invader_memory"][&key]
                        .as_object_mut()
                        .unwrap()
                        .remove("last_move");
                }
                _ => unreachable!(),
            }
            assert!(e.restore_checkpoint(bad, None).is_err(), "{defect}");
            assert_eq!(e.checkpoint(Value::Null)?, saved);
        }
        let mut old = saved.clone();
        old["checkpoint_format_version"] = json!(5);
        old["engine"]["world"]["rooms"]["W1N1"]
            .as_object_mut()
            .unwrap()
            .remove("invader_memory");
        assert!(
            e.restore_checkpoint(old, None)
                .unwrap_err()
                .to_string()
                .contains("invader_memory")
        );
        assert_eq!(e.checkpoint(Value::Null)?, saved);
    }
    Ok(())
}

#[test]
fn invader_ai_breaches_fortification_path_with_attack_or_dismantle() -> anyhow::Result<()> {
    for binary in [false, true] {
        for work in [false, true] {
            let mut e = fixture(binary)?;
            let body = if work {
                vec!["attack", "work", "move"]
            } else {
                vec!["attack", "move"]
            };
            creep(&mut e, "invader", INVADER_OWNER, [13, 10], &body)?;
            creep(&mut e, "target", "p1", [15, 10], &["move", "move"])?;
            let mut walls = Vec::new();
            for x in 14..=16 {
                for y in 9..=11 {
                    if x == 15 && y == 10 {
                        continue;
                    }
                    walls.push(e.place_object(
                        "W1N1",
                        "structure",
                        &json!({"pos":[x,y],"structureType":"constructedWall","hits":1000}),
                    )?);
                }
            }
            e.tick(1)?;
            let attacked: Vec<_> = walls.iter().filter(|id| hits(&e, id) < 1000).collect();
            // 第一拍只是走到墙前，第二拍同一路径首格指向围墙。
            if attacked.is_empty() {
                e.tick(1)?;
            }
            let losses: Vec<_> = walls
                .iter()
                .map(|id| 1000 - hits(&e, id))
                .filter(|loss| *loss > 0)
                .collect();
            assert!(
                !losses.is_empty(),
                "寻路第二轮忽略 fortification 后须攻击路径首格建筑"
            );
            assert!(
                losses
                    .iter()
                    .all(|loss| *loss == if work { 50 } else { 30 }),
                "{work}: {losses:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn invader_ai_healer_follows_distant_injured_friend_with_saved_path() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let healer = creep(&mut e, "healer", INVADER_OWNER, [10, 10], &["heal", "move"])?;
        let hurt = creep(
            &mut e,
            "hurt",
            INVADER_OWNER,
            [18, 10],
            &["move", "move", "move"],
        )?;
        injure(&mut e, &hurt, 100)?;
        e.tick(1)?;
        assert_ne!(pos_of(&e, &healer), (10, 10));
        assert!(memory(&mut e, &healer)?["move_cache"].is_object());
        e.tick(10)?;
        assert!(hits(&e, &hurt) > 200);
    }
    Ok(())
}
