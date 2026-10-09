//! G02.d3：计划级静态对象通行贯通默认矩阵、登记/仲裁、NPC 与冷恢复。
use super::*;
use harness::{creep_at, pos_in};

const OBSTACLES: &str = "api.rules.override('builtin:movement.obstaclesPassable',1);";
const RAMPARTS: &str = "api.rules.override('builtin:movement.privateRampartsPassable',1);";
const OBSERVE: &str = "api.handlers.before('builtin:room.movePrecheck',{id:'observe'},ctx=>{
  ctx.state={requests:ctx.args.requests.map(r=>({blocked:r.nativeStaticBlocked,registration:r.nativeRegistrationBlocked}))};
});";

fn shard() -> Value {
    let mut terrain = vec![1; 2500];
    for x in 0..50 {
        terrain[10 * 50 + x] = 0;
    }
    json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"terrain":terrain}]})
}

fn fixture(packages: &[(&str, String)], binary: bool) -> anyhow::Result<EmbedEngine> {
    engine_with_shard(packages, binary, &[], RegisterOptions::default(), &shard())
}

fn barrier(e: &mut EmbedEngine, kind: &str) -> anyhow::Result<String> {
    let (channel, structure) = match kind {
        "site" => ("constructionSite", "extension"),
        "foreignSite" => ("constructionSite", "extension"),
        "ownRampart" | "publicRampart" => ("structure", "rampart"),
        _ => ("structure", kind),
    };
    e.place_object(
        "W1N1",
        channel,
        &json!({"pos":[11,10],"structureType":structure,
        "owner":if kind=="rampart"||kind=="publicRampart"||kind=="foreignSite" {"p2"} else {"p1"},
        "isPublic":kind=="publicRampart","mineralType":"H","hits":10000,"progress":0,"progressTotal":200}),
    )
}

#[test]
fn static_objects_rules_keep_axes_independent_and_default_matrix_matches_actual_move()
-> anyhow::Result<()> {
    for binary in [false, true] {
        for kind in [
            "constructedWall",
            "spawn",
            "source",
            "mineral",
            "site",
            "foreignSite",
            "ownRampart",
            "publicRampart",
            "rampart",
        ] {
            for (obstacles, ramparts) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let code = format!(
                    "{}{}{}",
                    if obstacles { OBSTACLES } else { "" },
                    if ramparts { RAMPARTS } else { "" },
                    OBSERVE
                );
                let mut e = fixture(&[("walk", package(&code))], binary)?;
                barrier(&mut e, kind)?;
                let mover = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
                let passable = if matches!(kind, "foreignSite" | "ownRampart" | "publicRampart") {
                    true
                } else if kind == "rampart" {
                    ramparts
                } else {
                    obstacles
                };
                let result=e.run_player("p1",r#"
                    const from=Game.creeps.m.pos,to=new RoomPosition(13,10,'W1N1');
                    Game.rooms.W1N1.findPath(from,to,{range:1,maxRooms:1,ignoreCreeps:true,costCallback(name,matrix){Memory.cost=matrix.get(11,10)}});
                    const custom=PathFinder.search(from,{pos:to,range:1},{maxRooms:1,roomCallback(){const m=new PathFinder.CostMatrix();m.set(11,10,255);return m}});
                    const rc=Game.creeps.m.moveTo(to,{range:1,maxRooms:1,ignoreCreeps:true});
                    [Memory.cost,custom.incomplete,rc,typeof globalThis.__kkp]
                "#)?;
                assert_eq!(
                    result.expr["value"][0],
                    json!(if passable { 0 } else { 255 }),
                    "{binary} {kind} {obstacles} {ramparts}"
                );
                assert_eq!(result.expr["value"][1], true, "玩家显式 255 不得被计划擦除");
                assert_eq!(result.expr["value"][3], "undefined");
                let actual = pos_in(&e, "W1N1", &mover);
                if passable {
                    assert_eq!(actual, (11, 10));
                } else {
                    assert!(
                        actual.0 <= 10,
                        "阻挡路径可以局部绕行，但不可越过障碍：{actual:?}"
                    );
                }
                if passable {
                    assert_eq!(
                        e.engine_mod_state("walk")["requests"],
                        json!([{"blocked":false,"registration":false}])
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn static_objects_rules_keep_dynamic_occupants_and_explicit_veto() -> anyhow::Result<()> {
    for binary in [false, true] {
        for mode in ["creep", "powerCreep", "precheck", "obstacle"] {
            let extra = match mode {
                "precheck" => {
                    "api.handlers.before('builtin:room.movePrecheck',{id:'deny'},ctx=>{for(const r of ctx.args.requests)ctx.precheck.block(r.moverId)});"
                }
                "obstacle" => {
                    "api.handlers.before('builtin:room.moveObstacle',{id:'deny'},ctx=>{for(const r of ctx.args.movers)ctx.obstacle.block(r.id)});"
                }
                _ => "",
            };
            let mut e = fixture(
                &[("walk", package(&format!("{OBSTACLES}{RAMPARTS}{extra}")))],
                binary,
            )?;
            barrier(&mut e, "constructedWall")?;
            let id = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
            if mode == "creep" {
                creep_at(&mut e, "W1N1", "parked", "p1", 11, 10, &["work"])?;
            }
            if mode == "powerCreep" {
                e.place_object("W1N1","powerCreep",&json!({"name":"parked","owner":"p1","pos":[11,10],"ticksToLive":100,"hits":100,"hitsMax":100}))?;
            }
            e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
            assert_eq!(pos_in(&e, "W1N1", &id), (10, 10), "{binary} {mode}");
        }
    }
    Ok(())
}

#[test]
fn static_objects_rules_retry_once_and_cold_restore_rebuilds_search_policy() -> anyhow::Result<()> {
    for binary in [false, true] {
        for id in ["ghost", "deepwalk"] {
            let packages = [(id, package(OBSTACLES)), ("ramparts", package(RAMPARTS))];
            let mut e = fixture(&packages, binary)?;
            barrier(&mut e, "constructedWall")?;
            e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[12,10],"structureType":"rampart","owner":"p2","hits":10000}),
            )?;
            let actor = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
            let before = e.world_snapshot();
            e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
            assert!(
                e.run_player(
                    "p1",
                    "Memory.calls=(Memory.calls||0)+1;Game.creeps.m.moveTo(14,10,{range:1})"
                )
                .is_err()
            );
            assert_eq!(e.world_snapshot(), before);
            e.run_player("p1", "throw Error('cached player must not rerun')")?;
            assert_eq!(pos_in(&e, "W1N1", &actor), (11, 10));
            let memory: Value = serde_json::from_str(&e.player_state("p1").unwrap().raw_memory)?;
            assert_eq!(memory["calls"], 1);
            let checkpoint = e.checkpoint(Value::Null)?;
            let root = PackageRoot(std::env::temp_dir().join(format!(
                "kk-static-restore-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            )));
            for (name, code) in &packages {
                let dir = root.0.join(name);
                std::fs::create_dir_all(&dir)?;
                std::fs::write(
                    dir.join("mod.json"),
                    json!({"id":name,"version":"1.0.0","apiVersion":"^1.0.0","dependencies":{}})
                        .to_string(),
                )?;
                std::fs::write(dir.join("server.js"), code)?;
            }
            let mods = discover_packages(&root.0)?;
            let mut restored = EmbedEngine::new();
            restored.set_kkpv_channel(binary);
            restored.restore_checkpoint(checkpoint, Some((&mods, &RegisterOptions::default())))?;
            let step = "Game.creeps.m.moveTo(14,10,{range:1,reusePath:0})";
            e.run_player("p1", step)?;
            restored.run_player("p1", step)?;
            assert_eq!(pos_in(&restored, "W1N1", &actor), (12, 10));
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
            assert_eq!(
                e.player_state("p1").unwrap().raw_memory,
                restored.player_state("p1").unwrap().raw_memory
            );
        }
    }
    Ok(())
}

#[test]
fn static_objects_rules_keeper_search_consumes_object_and_wall_declarations() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for kind in ["constructedWall", "rampart", "terrain"] {
            for active in [false, true] {
                let code = if !active {
                    ""
                } else if kind == "rampart" {
                    RAMPARTS
                } else if kind == "terrain" {
                    "api.rules.override('builtin:terrain.wallPassable',1);"
                } else {
                    OBSTACLES
                };
                // 墙声明开启时不能使用全墙走廊夹具；否则 NPC 可以从邻格绕行。
                let mut world = shard();
                if kind == "terrain" {
                    world["rooms"][0]["terrain"] = json!(
                        (0..2500)
                            .map(|i| if i % 50 == 11 { 1 } else { 0 })
                            .collect::<Vec<_>>()
                    );
                }
                let mut e = engine_with_shard(
                    &[("walk", package(code))],
                    binary,
                    &[],
                    RegisterOptions::default(),
                    &world,
                )?;
                if kind != "terrain" {
                    barrier(&mut e, kind)?;
                }
                e.place_object(
                    "W1N1",
                    "structure",
                    &json!({"pos":[14,10],"structureType":"source"}),
                )?;
                let keeper =
                    creep_at(&mut e, "W1N1", "keeper", "Source Keeper", 10, 10, &["move"])?;
                e.tick(1)?;
                let pos = pos_in(&e, "W1N1", &keeper);
                assert_eq!(pos.0 > 10, active, "{binary} {kind} {active}");
            }
        }
    }
    Ok(())
}

#[test]
fn static_objects_rules_reject_conflicting_or_out_of_domain_declarations() {
    assert!(
        engine(
            &[
                ("ghost", package(OBSTACLES)),
                ("deepwalk", package(OBSTACLES))
            ],
            false
        )
        .is_err()
    );
    for rule in ["obstaclesPassable", "privateRampartsPassable"] {
        for value in [-1, 2] {
            assert!(
                engine(
                    &[(
                        "bad",
                        package(&format!(
                            "api.rules.override('builtin:movement.{rule}',{value});"
                        ))
                    )],
                    false
                )
                .is_err()
            );
        }
    }
}

#[test]
fn static_objects_rules_cover_mod_obstacle_buildings_and_preserve_creation_checks()
-> anyhow::Result<()> {
    for binary in [false, true] {
        for active in [false, true] {
            let server = package(&format!(
                r#"
                api.buildings.define('bm:gate',{{playerClass:'Gate',cost:{{energy:1}},hitsMax:100,pathing:'obstacle'}});
                {}
            "#,
                if active { OBSTACLES } else { "" }
            ));
            let player = "export default function install(api){class Gate extends api.builtins.OwnedStructure{};api.buildings.bind('bm:gate',Gate);}";
            let mut e = engine_with_player_files(&[("bm", server)], &[("bm", player)], binary)?;
            let site=e.place_object("W1N1","constructionSite",&json!({"pos":[11,10],"structureType":"bm:gate","owner":"p1","progress":0,"progressTotal":1}))?;
            harness::fighter(&mut e, "builder", 10, 9, &["work", "carry", "move"])?;
            e.run_player(
                "p1",
                &format!("Game.creeps.builder.build(Game.getObjectById('{site}'))"),
            )?;
            assert!(
                e.world_snapshot()["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|o| o["structureType"] == "bm:gate" && o["type"] == "structure")
            );
            let actor = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
            let result = e.run_player(
                "p1",
                r#"
                Game.rooms.W1N1.findPath(Game.creeps.m.pos,new RoomPosition(13,10,'W1N1'),
                    {costCallback(name,matrix){Memory.cost=matrix.get(11,10)}});
                Game.creeps.m.move(RIGHT);
                [Memory.cost,Game.rooms.W1N1.createConstructionSite(11,10,STRUCTURE_SPAWN)]
            "#,
            )?;
            assert_eq!(
                result.expr["value"],
                json!([if active { 0 } else { 255 }, -7])
            );
            assert_eq!(
                pos_in(&e, "W1N1", &actor),
                if active { (11, 10) } else { (10, 10) }
            );
        }
    }
    Ok(())
}

#[test]
fn static_objects_rules_power_creep_registration_uses_same_passability() -> anyhow::Result<()> {
    for binary in [false, true] {
        for active in [false, true] {
            let mut e = fixture(
                &[("walk", package(if active { OBSTACLES } else { "" }))],
                binary,
            )?;
            barrier(&mut e, "constructedWall")?;
            let id=e.place_object("W1N1","powerCreep",&json!({"name":"operator","owner":"p1","pos":[10,10],"ticksToLive":100,"hits":100,"hitsMax":100}))?;
            e.run_player("p1", "Game.powerCreeps.operator.move(RIGHT)")?;
            assert_eq!(
                pos_in(&e, "W1N1", &id),
                if active { (11, 10) } else { (10, 10) }
            );
        }
    }
    Ok(())
}
