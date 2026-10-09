//! Invader 相邻近战的目标覆盖顺序、过滤和候选恢复。
use super::*;
use harness::hits;
use kk_core::constants as c;
use kk_world::domain::{keeper::OWNER, npc::INVADER_OWNER};

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

#[test]
fn invader_melee_zero_step_target_is_last_adjacent_not_first_or_lowest_hits() -> anyhow::Result<()>
{
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let actor = creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["attack", "ranged_attack"],
        )?;
        let low = creep(&mut e, "low", "p1", [11, 10], &["move"])?;
        let high = creep(&mut e, "high", "p1", [11, 11], &["move", "move"])?;
        let friend = creep(&mut e, "friend", INVADER_OWNER, [9, 10], &["move"])?;
        let keeper = creep(&mut e, "keeper", OWNER, [9, 9], &["move"])?;
        let spawning = creep(&mut e, "spawning", "p1", [10, 11], &["move"])?;
        edit_object(&mut e, &spawning, |o| {
            let kk_schema::room::RoomObject::Creep(c) = o else {
                unreachable!()
            };
            c.spawning = true;
        })?;
        e.tick(1)?;
        assert_eq!(hits(&e, &low), 90, "远程仍独立选择最低 hits");
        assert_eq!(hits(&e, &high), 170, "近战最终零步结果取最后相邻目标");
        assert_eq!(hits(&e, &friend), 100);
        assert_eq!(hits(&e, &keeper), 100);
        let events: Vec<_> = harness::events(&e, c::EVENT_ATTACK)
            .into_iter()
            .filter(|ev| ev["objectId"] == actor)
            .collect();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|ev| ev["data"]["targetId"] == high
            && ev["data"]["attackType"] == c::EVENT_ATTACK_TYPE_MELEE));
    }
    Ok(())
}

#[test]
fn invader_melee_same_tile_power_creep_precedes_adjacent_and_missing_hits_stays_in_bucket()
-> anyhow::Result<()> {
    for binary in [false, true] {
        for missing in [false, true] {
            let mut e = fixture(binary)?;
            creep(&mut e, "invader", INVADER_OWNER, [10, 10], &["attack"])?;
            let target = e.place_object(
                "W1N1",
                "powerCreep",
                &json!({"pos":[10,10],"owner":"p1","name":"pc","ticksToLive":1000}),
            )?;
            let before = hits(&e, &target);
            let adjacent = creep(&mut e, "adjacent", "p1", [11, 10], &["move"])?;
            edit_object(&mut e, &target, |o| {
                let kk_schema::room::RoomObject::Generic(g) = o else {
                    unreachable!()
                };
                let mut data: Value = serde_json::from_str(&g.data).unwrap();
                data["spawning"] = json!(true);
                if missing {
                    data.as_object_mut().unwrap().remove("hits");
                }
                g.data = data.to_string();
            })?;
            e.tick(1)?;
            assert_eq!(
                hits(&e, &adjacent),
                100,
                "缺 hits 仍是同格首选，由战斗门拒绝，不改选相邻目标"
            );
            if missing {
                assert!(harness::events(&e, c::EVENT_ATTACK).is_empty());
            } else {
                assert_eq!(hits(&e, &target), before - 30);
            }
        }
    }
    Ok(())
}

#[test]
fn invader_melee_body_healer_spawning_range_and_safe_mode_gates() -> anyhow::Result<()> {
    for binary in [false, true] {
        for guard in [
            "inactive",
            "heal",
            "damagedHeal",
            "spawning",
            "range",
            "safeMode",
        ] {
            let mut e = fixture(binary)?;
            if guard == "safeMode" {
                e.create_shard(&json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"safeMode":100}]}))?;
            }
            let body = if matches!(guard, "heal" | "damagedHeal") {
                vec!["attack", "heal", "move"]
            } else {
                vec!["attack", "move"]
            };
            let actor = creep(&mut e, "invader", INVADER_OWNER, [10, 10], &body)?;
            let target = creep(
                &mut e,
                "target",
                "p1",
                if guard == "range" { [12, 10] } else { [11, 10] },
                &["move"],
            )?;
            if matches!(guard, "inactive" | "damagedHeal" | "spawning") {
                edit_object(&mut e, &actor, |o| {
                    let kk_schema::room::RoomObject::Creep(c) = o else {
                        unreachable!()
                    };
                    if guard == "spawning" {
                        c.spawning = true;
                    } else {
                        c.body[usize::from(guard == "damagedHeal")].hits = 0;
                        c.hits -= 100;
                    }
                })?;
            }
            e.tick(1)?;
            assert_eq!(hits(&e, &target), 100, "{guard}");
            assert!(harness::events(&e, c::EVENT_ATTACK).is_empty(), "{guard}");
        }
    }
    Ok(())
}

#[test]
fn invader_melee_boost_rampart_and_explicit_override_consume_existing_combat() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for override_target in [false, true] {
            let mut e = fixture(binary)?;
            let actor=e.place_object("W1N1","creep",&json!({"pos":[10,10],"owner":INVADER_OWNER,"name":"boosted","body":["attack"],"boosts":{"0":"XUH2O"}}))?;
            let alt = creep(&mut e, "alt", "p1", [11, 10], &["move", "move"])?;
            let target = creep(&mut e, "target", "p1", [11, 11], &["move"])?;
            let rampart = e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[11,11],"owner":"p1","structureType":"rampart","hits":1000}),
            )?;
            if override_target {
                e.settle_external(kk_engine::intent::IntentBatch::from_json(
                    INVADER_OWNER,
                    &json!({&actor:{"attack":{"id":alt}}}),
                ))?;
            } else {
                e.tick(1)?;
            }
            assert_eq!(hits(&e, &target), 100);
            assert_eq!(hits(&e, &alt), if override_target { 80 } else { 200 });
            assert_eq!(hits(&e, &rampart), if override_target { 1000 } else { 880 });
            let events = harness::events(&e, c::EVENT_ATTACK);
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0]["data"]["targetId"],
                if override_target { alt } else { rampart }
            );
            assert_eq!(events[0]["data"]["damage"], 120);
        }
    }
    Ok(())
}

#[test]
fn invader_melee_mod_cancel_rolls_back_action_and_preserves_other_ranged_action()
-> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.replace('builtin:creep.attack',{id:'cancel'},ctx=>{
  if(ctx.target.fields.owner==='Invader'){ctx.combat.cancel();ctx.state={cancelled:true};}
  else if(ctx.args.id!==null) ctx.combat.request(ctx.args.id);
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["attack", "ranged_attack"],
        )?;
        let target = creep(&mut e, "target", "p1", [11, 10], &["move"])?;
        e.tick(1)?;
        assert_eq!(hits(&e, &target), 90);
        assert_eq!(e.engine_mod_state("alpha"), json!({"cancelled":true}));
        let events = harness::events(&e, c::EVENT_ATTACK);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["data"]["attackType"], c::EVENT_ATTACK_TYPE_RANGED);
    }
    Ok(())
}

#[test]
fn invader_melee_retry_and_cold_restore_preserve_damage_and_death_once() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        creep(&mut e, "invader", INVADER_OWNER, [10, 10], &["attack"])?;
        let target = creep(&mut e, "target", "p1", [11, 10], &["move", "move"])?;
        let mut reference = EmbedEngine::new();
        reference.set_kkpv_channel(binary);
        reference.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.runs=(Memory.runs||0)+1")
                .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(hits(&e, &target), 200);
        assert!(e.checkpoint(Value::Null).is_err());
        reference.run_player("p1", "Memory.runs=(Memory.runs||0)+1")?;
        e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(hits(&e, &target), 170);
        assert_eq!(e.world_snapshot(), reference.world_snapshot());
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        for ticks in [1, 2, 3] {
            e.tick(ticks)?;
            reference.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), reference.world_snapshot());
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert!(e.get_object(&target).is_none());
        assert_eq!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).len(), 1);
        assert_eq!(harness::events(&e, c::EVENT_OBJECT_DESTROYED).len(), 1);
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}
