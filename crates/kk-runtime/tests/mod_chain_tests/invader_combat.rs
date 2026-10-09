//! Invader 自动远程攻击的真实结算、Mod 消费与候选恢复边界。
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
fn invader_ranged_selects_lowest_hits_in_three_tiles_without_mass_attack() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let actor = creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["ranged_attack", "heal"],
        )?;
        let friend = creep(&mut e, "friend", INVADER_OWNER, [9, 10], &["move"])?;
        let keeper = creep(&mut e, "keeper", OWNER, [7, 10], &["move"])?;
        let near = creep(&mut e, "near", "p1", [11, 10], &["move", "move"])?;
        let weak = creep(&mut e, "weak", "p1", [13, 13], &["move"])?;
        let tie = creep(&mut e, "tie", "p1", [12, 10], &["move"])?;
        let far = creep(&mut e, "far", "p1", [14, 10], &["move"])?;
        e.tick(1)?;
        for (id, expected) in [
            (&friend, 100),
            (&keeper, 100),
            (&near, 200),
            (&weak, 90),
            (&tie, 100),
            (&far, 100),
        ] {
            assert_eq!(hits(&e, id), expected, "{id}");
        }
        let events = harness::events(&e, c::EVENT_ATTACK);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["objectId"], actor);
        assert_eq!(events[0]["data"]["targetId"], weak);
        assert_eq!(events[0]["data"]["attackType"], c::EVENT_ATTACK_TYPE_RANGED);
    }
    Ok(())
}

#[test]
fn invader_ranged_filters_spawning_targets_and_inactive_body() -> anyhow::Result<()> {
    for binary in [false, true] {
        for inactive in [false, true] {
            let mut e = fixture(binary)?;
            let actor = creep(
                &mut e,
                "invader",
                INVADER_OWNER,
                [10, 10],
                &["ranged_attack", "move"],
            )?;
            let spawning = creep(&mut e, "spawning", "p1", [11, 10], &["move"])?;
            let target = creep(&mut e, "target", "p1", [12, 10], &["move", "move"])?;
            edit_object(&mut e, &spawning, |o| {
                let kk_schema::room::RoomObject::Creep(c) = o else {
                    unreachable!()
                };
                c.spawning = true;
            })?;
            if inactive {
                edit_object(&mut e, &actor, |o| {
                    let kk_schema::room::RoomObject::Creep(c) = o else {
                        unreachable!()
                    };
                    c.body[0].hits = 0;
                    c.hits = 100;
                })?;
            }
            e.tick(1)?;
            assert_eq!(hits(&e, &target), if inactive { 200 } else { 190 });
            assert_eq!(
                harness::events(&e, c::EVENT_ATTACK).len(),
                usize::from(!inactive)
            );
        }
    }
    Ok(())
}

#[test]
fn invader_ranged_targets_power_creeps_and_ignores_missing_hits() -> anyhow::Result<()> {
    for binary in [false, true] {
        for missing in [false, true] {
            let mut e = fixture(binary)?;
            creep(
                &mut e,
                "invader",
                INVADER_OWNER,
                [10, 10],
                &["ranged_attack"],
            )?;
            let target = e.place_object(
                "W1N1",
                "powerCreep",
                &json!({"pos":[13,10],"owner":"p1","name":"pc","ticksToLive":1000}),
            )?;
            let before = hits(&e, &target);
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
            if missing {
                assert!(harness::events(&e, c::EVENT_ATTACK).is_empty());
            } else {
                assert_eq!(hits(&e, &target), before - 10);
            }
        }
    }
    Ok(())
}

#[test]
fn invader_ranged_explicit_intent_overrides_automatic_target() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let actor = creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["ranged_attack"],
        )?;
        let low = creep(&mut e, "low", "p1", [11, 10], &["move"])?;
        let high = creep(&mut e, "high", "p1", [12, 10], &["move", "move"])?;
        e.settle_external(kk_engine::intent::IntentBatch::from_json(
            INVADER_OWNER,
            &json!({&actor:{"rangedAttack":{"id":high}}}),
        ))?;
        assert_eq!(hits(&e, &low), 100);
        assert_eq!(hits(&e, &high), 190);
    }
    Ok(())
}

#[test]
fn invader_ranged_range_and_actor_spawning_gates_prevent_damage() -> anyhow::Result<()> {
    for binary in [false, true] {
        for guard in ["range", "spawning", "noRanged"] {
            let mut e = fixture(binary)?;
            let body = if guard == "noRanged" {
                "move"
            } else {
                "ranged_attack"
            };
            let actor = creep(&mut e, "invader", INVADER_OWNER, [10, 10], &[body])?;
            let target = creep(
                &mut e,
                "target",
                "p1",
                if guard == "range" { [14, 10] } else { [11, 10] },
                &["move"],
            )?;
            if guard == "spawning" {
                edit_object(&mut e, &actor, |o| {
                    let kk_schema::room::RoomObject::Creep(c) = o else {
                        unreachable!()
                    };
                    c.spawning = true;
                })?;
            }
            e.tick(1)?;
            assert_eq!(hits(&e, &target), 100, "{guard}");
            let attacks = harness::events(&e, c::EVENT_ATTACK);
            if guard == "noRanged" {
                assert_eq!(attacks.len(), 1);
                assert_eq!(attacks[0]["data"]["damage"], 0);
            } else {
                assert!(attacks.is_empty(), "{guard}");
            }
        }
    }
    Ok(())
}

#[test]
fn invader_ranged_boosted_damage_redirects_to_rampart() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        e.place_object("W1N1","creep", &json!({"pos":[10,10],"owner":INVADER_OWNER,"name":"boosted","body":["ranged_attack"],"boosts":{"0":"XKHO2"}}))?;
        let target = creep(&mut e, "target", "p1", [13, 10], &["move"])?;
        let rampart = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[13,10],"structureType":"rampart","owner":"p1","hits":1000}),
        )?;
        e.tick(1)?;
        assert_eq!(hits(&e, &target), 100);
        assert_eq!(hits(&e, &rampart), 960);
        let events = harness::events(&e, c::EVENT_ATTACK);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["data"]["targetId"], rampart);
        assert_eq!(events[0]["data"]["damage"], 40);
    }
    Ok(())
}

#[test]
fn invader_ranged_consumes_mod_combat_chain() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.replace('builtin:creep.rangedAttack',{id:'cancel'},ctx=>{
  if(ctx.target.fields.owner==='Invader') {ctx.combat.cancel();ctx.state={cancelled:true};}
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
            &["ranged_attack"],
        )?;
        let target = creep(&mut e, "target", "p1", [11, 10], &["move"])?;
        e.tick(1)?;
        assert_eq!(hits(&e, &target), 100);
        assert!(harness::events(&e, c::EVENT_ATTACK).is_empty());
        assert_eq!(e.engine_mod_state("alpha"), json!({"cancelled":true}));
    }
    Ok(())
}

#[test]
fn invader_ranged_retry_and_cold_restore_preserve_damage_once() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        creep(
            &mut e,
            "invader",
            INVADER_OWNER,
            [10, 10],
            &["ranged_attack"],
        )?;
        let target = creep(&mut e, "target", "p1", [11, 10], &["move", "move", "move"])?;
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
        assert_eq!(hits(&e, &target), 300);
        assert!(e.checkpoint(Value::Null).is_err());
        reference.run_player("p1", "Memory.runs=(Memory.runs||0)+1")?;
        e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(hits(&e, &target), 290);
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
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}
