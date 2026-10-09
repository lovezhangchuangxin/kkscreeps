//! G03 障碍后链预演：候选当前值、链传播、阶段能力和恢复边界。
use super::*;
use harness::{creep_at, pos_in};

fn sorted(mut ids: Vec<String>) -> Value {
    ids.sort();
    json!(ids)
}

#[test]
fn obstacle_preview_matches_actual_swap_cycle_chain_and_pull_after_each_block() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for topology in ["swap", "cycle", "chain", "pull"] {
            for block_independent in [false, true] {
                let code = package(&format!(
                    r#"
                    api.handlers.before('builtin:room.moveObstacle',{{id:'preview'}},ctx=>{{
                        const before=ctx.obstacle.evaluate();
                        const dependent=ctx.args.movers.find(m=>m.targetX<20);
                        ctx.obstacle.block(dependent.id);
                        const after=ctx.obstacle.evaluate();
                        const detached=ctx.obstacle.evaluate();detached.push('not-an-object');
                        {}
                        ctx.state={{before,after,final:ctx.obstacle.evaluate()}};
                    }});
                "#,
                    if block_independent {
                        "for(const m of ctx.args.movers)if(m.targetX>=20)ctx.obstacle.block(m.id);"
                    } else {
                        ""
                    }
                ));
                let mut e = engine(&[("preview", code)], binary)?;
                let mut movers = vec![];
                let directions = match topology {
                    "swap" => vec![
                        ("a", 10, 10, "move", "BOTTOM"),
                        ("b", 10, 11, "move", "TOP"),
                    ],
                    "cycle" => vec![
                        ("a", 10, 10, "move", "RIGHT"),
                        ("b", 11, 10, "move", "BOTTOM_LEFT"),
                        ("c", 10, 11, "move", "TOP"),
                    ],
                    "chain" => vec![
                        ("a", 10, 10, "move", "RIGHT"),
                        ("b", 9, 10, "move", "RIGHT"),
                        ("c", 8, 10, "move", "RIGHT"),
                    ],
                    _ => vec![
                        ("a", 10, 10, "move", "TOP"),
                        ("b", 10, 11, "work", "Game.creeps.a"),
                    ],
                };
                let mut player = if topology == "pull" {
                    "Game.creeps.a.pull(Game.creeps.b);".to_string()
                } else {
                    String::new()
                };
                for (name, x, y, body, direction) in directions {
                    let id = creep_at(&mut e, "W1N1", name, "p1", x, y, &[body])?;
                    movers.push((id, (i64::from(x), i64::from(y))));
                    player.push_str(&format!("Game.creeps.{name}.move({direction});"));
                }
                let independent = creep_at(&mut e, "W1N1", "independent", "p1", 30, 30, &["move"])?;
                movers.push((independent.clone(), (30, 30)));
                player.push_str("Game.creeps.independent.move(RIGHT);");
                e.run_player("p1", &player)?;
                let state = e.engine_mod_state("preview");
                assert_eq!(
                    state["before"],
                    sorted(movers.iter().map(|(id, _)| id.clone()).collect())
                );
                assert_eq!(state["after"], json!([independent]));
                let actual = movers
                    .iter()
                    .filter(|(id, pos)| pos_in(&e, "W1N1", id) != *pos)
                    .map(|(id, _)| id.clone())
                    .collect();
                assert_eq!(
                    state["final"],
                    sorted(actual),
                    "{binary} {topology} {block_independent}"
                );
                assert_eq!(
                    state["final"].as_array().unwrap().len(),
                    usize::from(!block_independent)
                );
            }
        }
    }
    Ok(())
}

#[test]
fn obstacle_preview_uses_committed_admission_and_static_allowance_with_dynamic_occupancy()
-> anyhow::Result<()> {
    for binary in [false, true] {
        for parked in [false, true] {
            let source = package(
                r#"
                api.rules.override('builtin:movement.obstaclesPassable',1);
                api.rules.override('builtin:creep.move.minActiveMoves',0);
                api.handlers.replace('builtin:creep.moveAdmission',{id:'override'},ctx=>{
                    for(const m of ctx.args.movers)ctx.admission.setAdmitted(m.id,true);
                });
                api.handlers.after('builtin:room.moveObstacle',{id:'preview'},ctx=>{ctx.state={ids:ctx.obstacle.evaluate()}});
            "#,
            );
            let mut e = engine(&[("preview", source)], binary)?;
            e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[11,10],"structureType":"constructedWall","hits":10000}),
            )?;
            let mover = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["work"])?;
            if parked {
                creep_at(&mut e, "W1N1", "parked", "p1", 11, 10, &["work"])?;
            }
            e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
            assert_eq!(
                e.engine_mod_state("preview")["ids"],
                if parked { json!([]) } else { json!([mover]) }
            );
            assert_eq!(
                pos_in(&e, "W1N1", &mover),
                if parked { (10, 10) } else { (11, 10) }
            );
        }
    }
    Ok(())
}

#[test]
fn obstacle_preview_decline_discards_candidate_and_state_after_predicting_cancellation()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let source = package(
            r#"
            api.handlers.before('builtin:room.moveObstacle',{id:'deny'},ctx=>{
                for(const m of ctx.args.movers)ctx.obstacle.block(m.id);
                if(ctx.obstacle.evaluate().length)throw Error('should cancel all');
                ctx.state={dirty:true};return api.decline('restore');
            });
        "#,
        );
        let mut e = engine(&[("preview", source)], binary)?;
        let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
        assert_eq!(pos_in(&e, "W1N1", &id), (11, 10));
        assert_eq!(e.engine_mod_state("preview"), Value::Null);
    }
    Ok(())
}

#[test]
fn obstacle_preview_foreign_slot_and_expired_capability_faults_cannot_be_hidden()
-> anyhow::Result<()> {
    for (slot, call) in [
        ("builtin:creep.move", "ctx.obstacle.evaluate()"),
        ("builtin:creep.moveAdmission", "ctx.obstacle.evaluate()"),
        ("builtin:room.moveObstacle", "globalThis.saved.evaluate()"),
    ] {
        let source = package(&format!(
            r#"
            api.handlers.before('builtin:room.moveObstacle',{{id:'capture'}},ctx=>{{globalThis.saved=ctx.obstacle;}});
            api.handlers.after('{slot}',{{id:'expired'}},ctx=>{{
                try{{{call}}}catch(e){{}}return api.decline('hide');
            }});
        "#
        ));
        let mut e = engine(&[("bad", source)], false)?;
        creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player("p1", "Game.creeps.m.move(RIGHT)").is_err(),
            "{slot}"
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
    }
    Ok(())
}

#[test]
fn obstacle_preview_failure_retry_commits_once_and_cold_restore_keeps_observation()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let root = PackageRoot(std::env::temp_dir().join(format!(
            "kk-obstacle-preview-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        )));
        let dir = root.0.join("preview");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("mod.json"),
            json!({"id":"preview","version":"1.0.0","apiVersion":"^1.0.0"}).to_string(),
        )?;
        std::fs::write(
            dir.join("server.js"),
            package(
                r#"
            api.handlers.before('builtin:room.moveObstacle',{id:'preview'},ctx=>{
                const first=ctx.args.movers.find(m=>m.targetX<20);ctx.obstacle.block(first.id);
                ctx.state={calls:(ctx.state?.calls||0)+1,ids:ctx.obstacle.evaluate()};
            });
        "#,
            ),
        )?;
        let mods = discover_packages(&root.0)?;
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard_with_mods(
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6}]}),
            &mods,
            RegisterOptions::default(),
        )?;
        let a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
        let b = creep_at(&mut e, "W1N1", "b", "p1", 10, 11, &["move"])?;
        let c = creep_at(&mut e, "W1N1", "c", "p1", 30, 30, &["move"])?;
        let player = "Memory.calls=(Memory.calls||0)+1;Game.creeps.a.move(BOTTOM);Game.creeps.b.move(TOP);Game.creeps.c.move(RIGHT)";
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", player).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("preview"), Value::Null);
        e.run_player("p1", "throw Error('must use cached input')")?;
        assert_eq!(e.engine_mod_state("preview"), json!({"calls":1,"ids":[c]}));
        assert_eq!(pos_in(&e, "W1N1", &a), (10, 10));
        assert_eq!(pos_in(&e, "W1N1", &b), (10, 11));
        let saved = e.checkpoint(Value::Null)?;
        e.run_player("p1", player)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, Some((&mods, &RegisterOptions::default())))?;
        restored.run_player("p1", player)?;
        assert_eq!(e.world_snapshot(), restored.world_snapshot());
        assert_eq!(
            e.engine_mod_state("preview"),
            restored.engine_mod_state("preview")
        );
        assert_eq!(
            e.player_state("p1").unwrap().raw_memory,
            restored.player_state("p1").unwrap().raw_memory
        );
        assert_eq!(restored.engine_mod_state("preview")["calls"], 2);
    }
    Ok(())
}

#[test]
fn obstacle_preview_composed_packages_observe_final_blocks_after_admission_upper_bound()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let seal = package(
            r#"
            api.handlers.before('builtin:room.moveObstacle',{id:'seal'},ctx=>{
                for(const m of ctx.args.movers)if(m.targetX<20)ctx.obstacle.block(m.id);
            });
        "#,
        );
        let audit = package(
            r#"
            api.handlers.after('builtin:creep.moveAdmission',{id:'upper'},ctx=>{ctx.state={upper:ctx.admission.evaluate()};});
            api.handlers.after('builtin:room.moveObstacle',{id:'final'},ctx=>{ctx.state.final=ctx.obstacle.evaluate();});
        "#,
        );
        let mut e = engine_with_dependencies(
            &[("seal", seal), ("audit", audit)],
            binary,
            &[("audit", "seal")],
        )?;
        let a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
        let b = creep_at(&mut e, "W1N1", "b", "p1", 10, 11, &["move"])?;
        let c = creep_at(&mut e, "W1N1", "c", "p1", 30, 30, &["move"])?;
        e.run_player(
            "p1",
            "Game.creeps.a.move(BOTTOM);Game.creeps.b.move(TOP);Game.creeps.c.move(RIGHT)",
        )?;
        let state = e.engine_mod_state("audit");
        assert_eq!(
            state["upper"],
            sorted(vec![a.clone(), b.clone(), c.clone()])
        );
        assert_eq!(state["final"], json!([c]));
        assert_eq!(pos_in(&e, "W1N1", &a), (10, 10));
        assert_eq!(pos_in(&e, "W1N1", &b), (10, 11));
    }
    Ok(())
}

#[test]
fn obstacle_preview_safe_mode_uses_same_owner_as_actual_cancel_chain() -> anyhow::Result<()> {
    for binary in [false, true] {
        for active in [false, true] {
            let source = package(
                "api.handlers.after('builtin:room.moveObstacle',{id:'preview'},ctx=>{ctx.state={ids:ctx.obstacle.evaluate()}});",
            );
            let mut e = engine_with_shard(
                &[("preview", source)],
                binary,
                &[],
                RegisterOptions::default(),
                &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"safeModeAvailable":1}]}),
            )?;
            let mover = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
            creep_at(&mut e, "W1N1", "enemy", "p2", 11, 10, &["work"])?;
            if active {
                let result = e.run_player("p1", "Game.rooms.W1N1.controller.activateSafeMode()")?;
                assert_eq!(result.expr["value"], 0);
            }
            e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
            assert_eq!(
                e.engine_mod_state("preview")["ids"],
                if active { json!([mover]) } else { json!([]) }
            );
            assert_eq!(
                pos_in(&e, "W1N1", &mover),
                if active { (11, 10) } else { (10, 10) }
            );
        }
    }
    Ok(())
}
