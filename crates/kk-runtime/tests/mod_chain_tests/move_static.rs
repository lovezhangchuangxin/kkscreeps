//! G02.d1：公共 JS 静态放行、不可逆否决、动态取消链与恢复边界。
use super::*;
use harness::{creep_at, pos_in};

const ALLOW: &str = "api.handlers.before('builtin:room.movePrecheck',{id:'ghost'},ctx=>{
  for(const r of ctx.args.requests){
    if(typeof r.nativeRegistrationBlocked!=='boolean'||typeof r.nativeStaticBlocked!=='boolean')throw Error('静态初判缺失');
    ctx.precheck.allowStatic(r.moverId);
  }
  ctx.state={calls:(ctx.state?.calls||0)+1};
});";

fn wall(e: &mut EmbedEngine, x: u8, y: u8) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[x,y],"structureType":"constructedWall","hits":10000}),
    )
}

#[test]
fn static_allowance_opens_structures_ramparts_sites_and_terrain() -> anyhow::Result<()> {
    for binary in [false, true] {
        for kind in ["constructedWall", "rampart", "constructionSite", "terrain"] {
            for active in [false, true] {
                let mut terrain = vec![0; 2500];
                if kind == "terrain" {
                    terrain[10 * 50 + 11] = 1;
                }
                let shard = json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"terrain":terrain}]});
                let source = if active {
                    package(ALLOW)
                } else {
                    package("api.handlers.before('builtin:room.movePrecheck',{id:'noop'},()=>{});")
                };
                let mut e = engine_with_shard(
                    &[("alpha", source)],
                    binary,
                    &[],
                    RegisterOptions::default(),
                    &shard,
                )?;
                if kind != "terrain" {
                    e.place_object("W1N1", if kind == "constructionSite" { kind } else { "structure" },
                        &json!({"pos":[11,10],"structureType":if kind=="constructionSite" {"extension"} else {kind},
                            "owner":if kind=="rampart" {"p2"} else {"p1"},"isPublic":false,"hits":10000,"progress":0,"progressTotal":200}))?;
                }
                let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
                let run = e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
                assert_eq!(
                    run.expr["value"],
                    json!(0),
                    "静态障碍不改变玩家 move 返回值"
                );
                assert_eq!(
                    pos_in(&e, "W1N1", &id),
                    if active { (11, 10) } else { (10, 10) },
                    "{binary} {kind} {active}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn static_allowance_keeps_explicit_veto_and_obstacle_policy() -> anyhow::Result<()> {
    for slot in ["builtin:room.movePrecheck", "builtin:room.moveObstacle"] {
        let deny = if slot.ends_with("Precheck") {
            "for(const r of ctx.args.requests){ctx.precheck.block(r.moverId);ctx.precheck.allowStatic(r.moverId);if(!ctx.precheck.isBlocked(r.moverId))throw Error('否决被清除');}"
        } else {
            "for(const m of ctx.args.movers){if(m.nativeBlocked)throw Error('静态放行未进入障碍初判');ctx.obstacle.block(m.id);}"
        };
        let mut e = engine(
            &[
                ("ghost", package(ALLOW)),
                (
                    "deny",
                    package(&format!(
                        "api.handlers.after('{slot}',{{id:'deny'}},ctx=>{{{deny}}});"
                    )),
                ),
            ],
            false,
        )?;
        wall(&mut e, 11, 10)?;
        let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
        assert_eq!(pos_in(&e, "W1N1", &id), (10, 10), "{slot}");
    }
    Ok(())
}

#[test]
fn static_allowance_preview_and_dynamic_chain_agree() -> anyhow::Result<()> {
    for occupied in [false, true] {
        let preview = package(
            "api.handlers.after('builtin:creep.moveAdmission',{id:'preview'},ctx=>{ctx.state={ids:ctx.admission.evaluate()};});",
        );
        let mut e = engine(&[("ghost", package(ALLOW)), ("preview", preview)], false)?;
        wall(&mut e, 11, 10)?;
        let head = creep_at(&mut e, "W1N1", "head", "p1", 10, 10, &["move"])?;
        let follower = creep_at(&mut e, "W1N1", "follower", "p1", 9, 10, &["move"])?;
        if occupied {
            creep_at(&mut e, "W1N1", "parked", "p1", 11, 10, &["work"])?;
        }
        e.run_player(
            "p1",
            "Game.creeps.head.move(RIGHT);Game.creeps.follower.move(RIGHT)",
        )?;
        assert_eq!(
            pos_in(&e, "W1N1", &head),
            if occupied { (10, 10) } else { (11, 10) }
        );
        assert_eq!(
            pos_in(&e, "W1N1", &follower),
            if occupied { (9, 10) } else { (10, 10) }
        );
        let mut ids = if occupied {
            vec![]
        } else {
            vec![head, follower]
        };
        ids.sort();
        assert_eq!(e.engine_mod_state("preview"), json!({"ids":ids}));
    }
    Ok(())
}

#[test]
fn static_allowance_decline_and_disable_restore_native() -> anyhow::Result<()> {
    for decline in [false, true] {
        let source = if decline {
            package(&format!(
                "{ALLOW} api.handlers.after('builtin:room.movePrecheck',{{id:'decline'}},()=>api.decline('native'));"
            ))
        } else {
            package(ALLOW)
        };
        let mut e = engine(&[("ghost", source)], false)?;
        wall(&mut e, 11, 10)?;
        let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        if !decline {
            e.disable_mod("ghost")?;
        }
        e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
        assert_eq!(pos_in(&e, "W1N1", &id), (10, 10));
        assert_eq!(e.engine_mod_state("ghost"), Value::Null);
    }
    Ok(())
}

#[test]
fn static_allowance_faults_cannot_hide_behind_decline() -> anyhow::Result<()> {
    for (slot, argument) in [
        ("builtin:room.movePrecheck", "'not-an-id'"),
        ("builtin:room.movePrecheck", "'kk0000000000000063'"),
        ("builtin:room.movePrecheck", "null"),
        ("builtin:creep.moveAdmission", "ctx.args.movers[0].id"),
        ("builtin:room.moveObstacle", "ctx.args.movers[0].id"),
    ] {
        let bad = package(&format!(
            "api.handlers.after('{slot}',{{id:'bad'}},ctx=>{{try{{ctx.precheck.allowStatic({argument})}}catch(e){{}}return api.decline('hide');}});"
        ));
        let mut e = engine(&[("ghost", package(ALLOW)), ("bad", bad)], false)?;
        wall(&mut e, 11, 10)?;
        creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player("p1", "Game.creeps.m.move(RIGHT)").is_err(),
            "{slot} {argument}"
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("ghost"), Value::Null);
    }
    Ok(())
}

#[test]
fn static_allowance_writeback_retry_commits_once() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine(&[("ghost", package(ALLOW))], binary)?;
        wall(&mut e, 11, 10)?;
        let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.runs=1;Game.creeps.m.move(RIGHT)")
                .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("ghost"), Value::Null);
        e.run_player("p1", "throw Error('must use cache')")?;
        assert_eq!(pos_in(&e, "W1N1", &id), (11, 10));
        assert_eq!(e.engine_mod_state("ghost"), json!({"calls":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}

#[test]
fn static_allowance_movement_channel_and_decline_share_registration() -> anyhow::Result<()> {
    for decline in [false, true] {
        let redirect = package(&format!(
            "api.handlers.after('builtin:creep.move',{{id:'redirect'}},ctx=>{{ctx.movement.request(1); {} }});",
            if decline {
                "return api.decline('restore');"
            } else {
                ""
            }
        ));
        let mut e = engine(&[("ghost", package(ALLOW)), ("redirect", redirect)], false)?;
        wall(&mut e, 11, 10)?;
        wall(&mut e, 10, 9)?;
        let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
        e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
        // 移动槽正常拒绝恢复槽前请求（尚无请求），静态放行不能凭空造请求。
        assert_eq!(
            pos_in(&e, "W1N1", &id),
            if decline { (10, 10) } else { (10, 9) }
        );
        assert_eq!(
            e.engine_mod_state("ghost"),
            if decline {
                Value::Null
            } else {
                json!({"calls":1})
            }
        );
    }
    Ok(())
}

#[test]
fn static_allowance_pull_and_cross_room_keep_native_transition() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine(&[("ghost", package(ALLOW))], binary)?;
        wall(&mut e, 9, 10)?;
        let p = creep_at(&mut e, "W1N1", "p", "p1", 10, 10, &["move"])?;
        let t = creep_at(&mut e, "W1N1", "t", "p1", 11, 10, &["work"])?;
        e.run_player("p1","Game.creeps.p.pull(Game.creeps.t);Game.creeps.t.move(Game.creeps.p);Game.creeps.p.move(LEFT)")?;
        assert_eq!(pos_in(&e, "W1N1", &p), (9, 10));
        assert_eq!(pos_in(&e, "W1N1", &t), (10, 10));
        let shard = json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6},{"name":"W0N1","owner":"p1","rcl":6}]});
        let mut e = engine_with_shard(
            &[("ghost", package(ALLOW))],
            binary,
            &[],
            RegisterOptions::default(),
            &shard,
        )?;
        wall(&mut e, 49, 10)?;
        let id = creep_at(&mut e, "W1N1", "m", "p1", 48, 10, &["move"])?;
        e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
        assert_eq!(pos_in(&e, "W0N1", &id), (0, 10));
    }
    Ok(())
}

#[test]
fn static_allowance_dual_player_prechecks_and_concurrency_agree() -> anyhow::Result<()> {
    let shard = json!({"players":["p1","p2"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6}]});
    let source = package(
        "api.handlers.before('builtin:room.movePrecheck',{id:'owner'},ctx=>{
      for(const r of ctx.args.requests)if(r.owner==='p1')ctx.precheck.allowStatic(r.moverId);
    });",
    );
    let codes = [
        ("p1".to_string(), "Game.creeps.a.move(RIGHT)".to_string()),
        ("p2".to_string(), "Game.creeps.b.move(RIGHT)".to_string()),
    ]
    .into_iter()
    .collect();
    let mut snapshots = Vec::new();
    for binary in [false, true] {
        for concurrency in [1, 2] {
            let mut e = engine_with_shard(
                &[("owner", source.clone())],
                binary,
                &[],
                RegisterOptions::default(),
                &shard,
            )?;
            wall(&mut e, 11, 10)?;
            wall(&mut e, 21, 20)?;
            let a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
            let b = creep_at(&mut e, "W1N1", "b", "p2", 20, 20, &["move"])?;
            e.set_player_concurrency(concurrency)?;
            let runs = e.run_players(&codes)?;
            assert_eq!(runs["p1"].expr["value"], json!(0));
            assert_eq!(runs["p2"].expr["value"], json!(0));
            assert_eq!(pos_in(&e, "W1N1", &a), (11, 10));
            assert_eq!(pos_in(&e, "W1N1", &b), (20, 20));
            snapshots.push(e.world_snapshot());
        }
    }
    for snapshot in &snapshots[1..] {
        assert_eq!(*snapshot, snapshots[0]);
    }
    Ok(())
}

/// G02.d2 边界钉死：ghost 放行只作用于结算侧——同一 tick 内玩家
/// PathFinder.search 仍按原生墙格绕行（不感知 allowStatic，时序上
/// 放行决策也在玩家执行之后才产生），而方向 move 经结算侧放行真实
/// 穿墙。双断言互为对照。
#[test]
fn pathfinder_stays_native_blind_to_static_allowance() -> anyhow::Result<()> {
    for binary in [false, true] {
        for active in [false, true] {
            let mut terrain = vec![0; 2500];
            terrain[10 * 50 + 11] = 1;
            let shard = json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"terrain":terrain}]});
            let source = if active {
                package(ALLOW)
            } else {
                package("api.handlers.before('builtin:room.movePrecheck',{id:'noop'},()=>{});")
            };
            let mut e = engine_with_shard(
                &[("alpha", source)],
                binary,
                &[],
                RegisterOptions::default(),
                &shard,
            )?;
            let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
            let run = e.run_player(
                "p1",
                r#"(() => {
                    const r = PathFinder.search(
                        new RoomPosition(10, 10, 'W1N1'),
                        { pos: new RoomPosition(12, 10, 'W1N1'), range: 0 },
                        { maxRooms: 1 });
                    const crosses = r.path.some((p) => p.x === 11 && p.y === 10);
                    const rc = Game.creeps.m.move(RIGHT);
                    return [rc, r.incomplete, r.path.length > 0, crosses].join(',');
                })()"#,
            )?;
            assert_eq!(
                run.expr["value"],
                json!("0,false,true,false"),
                "{binary} {active}: move 记录 OK 且寻路绕开墙格"
            );
            assert_eq!(
                pos_in(&e, "W1N1", &id),
                if active { (11, 10) } else { (10, 10) },
                "{binary} {active}: 结算侧按放行决策"
            );
        }
    }
    Ok(())
}
