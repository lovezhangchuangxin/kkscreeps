//! 障碍摘要的有效安全模式主人，与真实占格取消同源。
use super::*;
use harness::{creep_at, pos_in};

#[test]
fn safe_mode_summary_explains_creep_and_power_creep_occupancy_for_both_owners() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for active in [false, true] {
            for occupant_kind in ["creep", "powerCreep"] {
                for occupant_owner in ["p1", "p2"] {
                    for mover_owner in ["p1", "p2"] {
                        let code = package(
                            r#"
                        api.handlers.after('builtin:room.moveObstacle',{id:'observe'},ctx=>{
                            ctx.state={owner:ctx.args.safeModeOwner,movers:ctx.args.movers,ids:ctx.obstacle.evaluate()};
                        });
                    "#,
                        );
                        let mut e = engine_with_shard(
                            &[("observer", code)],
                            binary,
                            &[],
                            RegisterOptions::default(),
                            &json!({"players":["p1","p2"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"safeMode":if active {10} else {0}}]}),
                        )?;
                        let mover = creep_at(&mut e, "W1N1", "m", mover_owner, 10, 10, &["move"])?;
                        if occupant_kind == "creep" {
                            creep_at(&mut e, "W1N1", "parked", occupant_owner, 11, 10, &["work"])?;
                        } else {
                            e.place_object("W1N1","powerCreep",&json!({"name":"parked","owner":occupant_owner,"pos":[11,10],"ticksToLive":100,"hits":100,"hitsMax":100}))?;
                        }
                        e.run_player(mover_owner, "Game.creeps.m.move(RIGHT)")?;
                        let state = e.engine_mod_state("observer");
                        assert_eq!(
                            state["owner"],
                            if active { json!("p1") } else { Value::Null }
                        );
                        let passes = active && mover_owner == "p1" && occupant_owner != mover_owner;
                        assert_eq!(state["movers"][0]["nativeBlocked"], !passes);
                        assert_eq!(
                            state["ids"],
                            if passes { json!([mover]) } else { json!([]) }
                        );
                        assert_eq!(
                            pos_in(&e, "W1N1", &mover),
                            if passes { (11, 10) } else { (10, 10) }
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn safe_mode_summary_expires_at_same_tick_as_native_cancellation() -> anyhow::Result<()> {
    for binary in [false, true] {
        let source = package(
            r#"
            api.handlers.after('builtin:room.moveObstacle',{id:'observe'},ctx=>{
                ctx.state={owner:ctx.args.safeModeOwner,ids:ctx.obstacle.evaluate()};
            });
        "#,
        );
        let mut e = engine_with_shard(
            &[("observer", source)],
            binary,
            &[],
            RegisterOptions::default(),
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"safeMode":1}]}),
        )?;
        let mover = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        creep_at(&mut e, "W1N1", "enemy1", "p2", 11, 10, &["work"])?;
        creep_at(&mut e, "W1N1", "enemy2", "p2", 12, 10, &["work"])?;
        e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
        assert_eq!(e.engine_mod_state("observer")["owner"], "p1");
        assert_eq!(pos_in(&e, "W1N1", &mover), (11, 10));
        e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
        assert_eq!(e.engine_mod_state("observer")["owner"], Value::Null);
        assert_eq!(e.engine_mod_state("observer")["ids"], json!([]));
        assert_eq!(pos_in(&e, "W1N1", &mover), (11, 10));
    }
    Ok(())
}
