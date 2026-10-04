//! 移动槽开放后保留既有世界语义：冲突/交换/环移/阻塞/疲劳/损毁 MOVE/
//! pull/道路磨损/工地踩踏/边缘自动过渡/跨房迁移与双玩家一致性。
use super::*;

fn creep_at(
    e: &mut EmbedEngine,
    room: &str,
    name: &str,
    owner: &str,
    x: u8,
    y: u8,
    body: &[&str],
) -> anyhow::Result<String> {
    e.place_object(
        room,
        "creep",
        &json!({"name":name,"owner":owner,"pos":[x,y],"body":body,"ticksToLive":100}),
    )
}

fn pos_in(e: &EmbedEngine, room: &str, id: &str) -> (i64, i64) {
    let o = e.get_object(id).unwrap();
    let p = &o["pos"];
    assert_eq!(p["roomName"], room, "对象应仍在原房");
    (p["x"].as_i64().unwrap(), p["y"].as_i64().unwrap())
}

fn observe() -> String {
    // 观察链使移动槽进入整槽宿主路径，不拒绝、不改动登记。
    "api.handlers.before('builtin:creep.move',{id:'observe'},ctx=>{ctx.state={seen:(ctx.state?.seen||0)+1};});".into()
}

/// 两 creep 抢同格、交换与环形移动在观察链下保持官方仲裁结果。
#[test]
fn creep_move_conflicts_swap_and_ring_keep_native_arbitration() -> anyhow::Result<()> {
    let source = package(&observe());
    for binary in [false, true] {
        // 抢格：恰一个胜者。
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
        let b = creep_at(&mut e, "W1N1", "b", "p1", 12, 12, &["move"])?;
        e.run_player(
            "p1",
            "Game.creeps.a.move(BOTTOM_RIGHT);Game.creeps.b.move(TOP_LEFT)",
        )?;
        let hit_a = pos_in(&e, "W1N1", &a) == (11, 11);
        let hit_b = pos_in(&e, "W1N1", &b) == (11, 11);
        assert!(hit_a ^ hit_b, "恰一个胜者：a={hit_a} b={hit_b}");

        // 交换：双方成功。
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
        let b = creep_at(&mut e, "W1N1", "b", "p1", 11, 10, &["move"])?;
        e.run_player("p1", "Game.creeps.a.move(RIGHT);Game.creeps.b.move(LEFT)")?;
        assert_eq!(pos_in(&e, "W1N1", &a), (11, 10), "对穿 a");
        assert_eq!(pos_in(&e, "W1N1", &b), (10, 10), "对穿 b");

        // 环移：三 creep 依次让位，全部成功。
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
        let b = creep_at(&mut e, "W1N1", "b", "p1", 11, 10, &["move"])?;
        let c = creep_at(&mut e, "W1N1", "c", "p1", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            "Game.creeps.a.move(RIGHT);Game.creeps.b.move(BOTTOM_LEFT);Game.creeps.c.move(TOP)",
        )?;
        assert_eq!(pos_in(&e, "W1N1", &a), (11, 10), "环移 a");
        assert_eq!(pos_in(&e, "W1N1", &b), (10, 11), "环移 b");
        assert_eq!(pos_in(&e, "W1N1", &c), (10, 10), "环移 c");
    }
    Ok(())
}

/// 阻塞（己方障碍工地/静止 creep）、疲劳门槛与损毁 MOVE 部件语义不变；
/// 能力 request 与 native 在这些门槛上同口径。
#[test]
fn creep_move_blocked_fatigue_and_broken_move_parts_keep_gates() -> anyhow::Result<()> {
    let source = package(&observe());
    let mut e = engine(&[("alpha", source)], false)?;
    // 静止 creep 阻挡。
    let s = creep_at(&mut e, "W1N1", "s", "p1", 11, 10, &["work"])?;
    let m = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W1N1", &m), (10, 10), "静止 creep 阻挡");
    assert_eq!(pos_in(&e, "W1N1", &s), (11, 10));

    // 疲劳门槛：动一停一；未移动的 tick 仍恢复疲劳。
    let f = creep_at(&mut e, "W1N1", "f", "p1", 20, 20, &["move", "work", "work"])?;
    e.run_player("p1", "Game.creeps.f.move(BOTTOM)")?;
    let f0 = e.get_object(&f).unwrap();
    assert_eq!(
        (
            f0["pos"]["x"].as_i64().unwrap(),
            f0["fatigue"].as_i64().unwrap()
        ),
        (20, 2)
    );
    e.run_player("p1", "Game.creeps.f.move(BOTTOM)")?;
    assert_eq!(pos_in(&e, "W1N1", &f), (20, 21));
    let f1 = e.get_object(&f).unwrap();
    assert_eq!(f1["pos"]["y"].as_i64().unwrap(), 21, "疲劳 2≠0 不动");
    assert_eq!(f1["fatigue"].as_i64().unwrap(), 0, "未移动仍恢复 -2");
    e.run_player("p1", "Game.creeps.f.move(BOTTOM)")?;
    assert_eq!(pos_in(&e, "W1N1", &f), (20, 22), "清零后恢复移动");

    // 无 MOVE 部件：能力 request 也不移动（can_move 在仲裁/执行拒绝）。
    let source = package(
        "api.handlers.replace('builtin:creep.move',{id:'force'},ctx=>{ctx.movement.request(3);});",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let w = creep_at(&mut e, "W1N1", "w", "p1", 30, 30, &["work"])?;
    e.run_player("p1", "Game.creeps.w.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W1N1", &w), (30, 30), "损毁/缺失 MOVE 不动");
    Ok(())
}

/// pull 保持 native：被拉者免疲劳/MOVE 检查；被拉者移动被拒绝时拉者
/// 仍可移动，配对按既有仲裁规则解除。
#[test]
fn creep_move_pull_relations_survive_move_slot_and_decline() -> anyhow::Result<()> {
    let observe = package(&observe());
    // 拒绝条件只作用于朝相邻 creep 的移动形态（被拉者的随行请求），
    // 拉者自身的方向移动照常走 native 中心。
    let decline_pulled = package(
        r#"
api.handlers.before('builtin:creep.move',{id:'deny-pulled'},ctx=>{
  if(ctx.args.id!==null)return api.decline('blocked');
});
"#,
    );
    // 目标无 MOVE 部件、被拉时仍随拉者移动：观察链下两者都动。
    let mut e = engine(&[("alpha", observe.clone())], false)?;
    let puller = creep_at(&mut e, "W1N1", "puller", "p1", 10, 10, &["move"])?;
    let pulled = creep_at(&mut e, "W1N1", "pulled", "p1", 11, 10, &["work"])?;
    e.run_player(
        "p1",
        "const p=Game.creeps.puller,t=Game.creeps.pulled;p.pull(t);t.move(p);p.move(LEFT)",
    )?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (9, 10));
    assert_eq!(
        pos_in(&e, "W1N1", &pulled),
        (10, 10),
        "被拉者免 MOVE 检查随行"
    );

    // 拒绝被拉者的移动：拉者照常移动，被拉者留在原格。
    let mut e = engine(&[("alpha", decline_pulled)], false)?;
    let puller = creep_at(&mut e, "W1N1", "puller", "p1", 10, 10, &["move"])?;
    let pulled = creep_at(&mut e, "W1N1", "pulled", "p1", 11, 10, &["work"])?;
    e.run_player(
        "p1",
        "const p=Game.creeps.puller,t=Game.creeps.pulled;p.pull(t);t.move(p);p.move(LEFT)",
    )?;
    assert_eq!(
        pos_in(&e, "W1N1", &puller),
        (9, 10),
        "拉者不受被拉者拒绝影响"
    );
    assert_eq!(pos_in(&e, "W1N1", &pulled), (11, 10), "被拉者请求被撤销");
    assert_eq!(e.engine_mod_state("alpha"), Value::Null);
    Ok(())
}

/// 道路磨损、沼泽疲劳与工地踩踏仅在位移执行时发生；拒绝不留残留。
#[test]
fn creep_move_road_wear_swamp_and_stomp_leave_no_residue_on_decline() -> anyhow::Result<()> {
    fn scene(e: &mut EmbedEngine) -> anyhow::Result<(String, String, String)> {
        let road = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"road"}),
        )?;
        let site = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[11,10],"structureType":"extension","owner":"p2","progress":60,"progressTotal":200}),
        )?;
        let m = creep_at(e, "W1N1", "m", "p1", 10, 10, &["move", "work", "work"])?;
        Ok((road, site, m))
    }
    fn decay(e: &EmbedEngine, id: &str) -> i64 {
        e.get_object(id).unwrap()["ticksToDecay"]
            .as_i64()
            .unwrap_or(0)
    }
    // 成功移动：road 磨损按身体部件数（3），工地被踩掉并掉落能量。
    let source = package(&observe());
    let mut e = engine(&[("alpha", source)], false)?;
    let (road, site, m) = scene(&mut e)?;
    let decay_before = decay(&e, &road);
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W1N1", &m), (11, 10));
    // 磨损 3（ROAD_WEAROUT×身体数）外加 1 tick 的时间推进投影。
    assert_eq!(
        decay(&e, &road),
        decay_before - 3 - 1,
        "磨损 ROAD_WEAROUT×身体数"
    );
    assert!(e.get_object(&site).is_none(), "敌方工地被踩");
    let drops = e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES);
    assert_eq!(drops.len(), 1, "工地进度掉落能量");
    assert_eq!(drops[0]["amount"], 30, "进度/2");

    // 拒绝移动：无磨损、无踩踏、无掉落。
    let source = package(
        "api.handlers.after('builtin:creep.move',{id:'reject'},()=>api.decline('blocked'));",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (road, site, m) = scene(&mut e)?;
    let decay_before = decay(&e, &road);
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W1N1", &m), (10, 10));
    assert_eq!(
        decay(&e, &road),
        decay_before - 1,
        "拒绝不磨损道路（仅时间推进）"
    );
    assert!(e.get_object(&site).is_some(), "拒绝不踩工地");
    assert!(
        e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES)
            .is_empty()
    );
    Ok(())
}

/// 移上边缘格同 tick 过渡到邻房；已在边缘格的自动过渡与拒绝移动请求
/// 互相独立；向界外的请求（native 或能力）不登记。
#[test]
fn creep_move_edge_transition_and_cross_room_moves_survive_decline() -> anyhow::Result<()> {
    let shard = json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W1N1", "rcl": 6, "owner": "p1" },
            { "name": "W0N1", "rcl": 4, "owner": "p1" },
        ]
    });
    let observe = package(&observe());
    // 从房内移上东边缘：EVENT_EXIT 先记入源房日志，对象同 tick 迁入邻房。
    let mut e = engine_with_shard(
        &[("alpha", observe.clone())],
        false,
        &[],
        RegisterOptions::default(),
        &shard,
    )?;
    let m = creep_at(&mut e, "W1N1", "m", "p1", 48, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W0N1", &m), (0, 10), "同 tick 跨房到镜像格");
    let snap = e.world_snapshot();
    let exits: Vec<&Value> = snap["rooms"]["W1N1"]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["event"] == kk_core::constants::EVENT_EXIT)
        .collect();
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0]["data"], json!({"room":"W0N1","x":0,"y":10}));

    // 已在边缘格：移动请求被拒绝，tick 级自动过渡仍发生。
    let decline = package(
        "api.handlers.before('builtin:creep.move',{id:'reject'},()=>api.decline('blocked'));",
    );
    let mut e = engine_with_shard(
        &[("alpha", decline.clone())],
        false,
        &[],
        RegisterOptions::default(),
        &shard,
    )?;
    let m = creep_at(&mut e, "W1N1", "m", "p1", 49, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(
        pos_in(&e, "W0N1", &m),
        (0, 10),
        "边缘自动过渡不依赖移动请求"
    );

    // 移上边缘格的请求被拒绝：不位移、不 EXIT、不过渡。
    let mut e = engine_with_shard(
        &[("alpha", decline)],
        false,
        &[],
        RegisterOptions::default(),
        &shard,
    )?;
    let m = creep_at(&mut e, "W1N1", "m", "p1", 48, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W1N1", &m), (48, 10), "拒绝后不入边缘格");
    let snap = e.world_snapshot();
    assert!(
        snap["rooms"]["W1N1"]["events"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|item| item["event"] != kk_core::constants::EVENT_EXIT)
    );

    // 向界外（西边界格向 LEFT）：native 与能力都不登记、不故障。
    let cap = package(
        "api.handlers.replace('builtin:creep.move',{id:'cap'},ctx=>{ctx.state={ok:ctx.movement.request(7)};});",
    );
    let mut e = engine_with_shard(
        &[("alpha", cap)],
        false,
        &[],
        RegisterOptions::default(),
        &shard,
    )?;
    let m = creep_at(&mut e, "W1N1", "m", "p1", 0, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m.move(LEFT)")?;
    assert_eq!(
        pos_in(&e, "W1N1", &m),
        (0, 10),
        "西边界格向界外：自动过渡按边缘优先级仍发生"
    );
    assert_eq!(
        e.engine_mod_state("alpha"),
        json!({"ok":false}),
        "界外 request 不登记"
    );
    Ok(())
}

/// 双玩家双房间：拒绝只作用于目标执行者；串行与并发派发结果一致。
#[test]
fn creep_move_dual_player_rooms_and_dispatch_modes_agree() -> anyhow::Result<()> {
    let shard = json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 6, "owner": "p1" },
            { "name": "W0N1", "rcl": 4, "owner": "p2" },
        ]
    });
    let source = package(
        r#"
api.handlers.before('builtin:creep.move',{id:'gate'},ctx=>{
  if(ctx.target.fields.owner==='p2')return api.decline('not-you');
  ctx.state={moves:(ctx.state?.moves||0)+1};
});
"#,
    );
    let mut codes = std::collections::BTreeMap::new();
    codes.insert(
        "p1".to_string(),
        "Memory.p=1;[Game.creeps.a.move(RIGHT),Game.creeps.b.move(RIGHT)]".to_string(),
    );
    codes.insert(
        "p2".to_string(),
        "Memory.p=2;Game.creeps.c.move(RIGHT)".to_string(),
    );
    let mut snapshots = Vec::new();
    for concurrency in [1usize, 2] {
        let mut e = engine_with_shard(
            &[("alpha", source.clone())],
            false,
            &[],
            RegisterOptions::default(),
            &shard,
        )?;
        let a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
        let b = creep_at(&mut e, "W1N1", "b", "p1", 12, 10, &["move"])?;
        let c = creep_at(&mut e, "W0N1", "c", "p2", 20, 20, &["move"])?;
        e.set_player_concurrency(concurrency)?;
        let runs = e.run_players(&codes)?;
        assert_eq!(runs["p1"].expr["value"], json!([0, 0]));
        assert_eq!(runs["p2"].expr["value"], json!(0));
        assert_eq!(pos_in(&e, "W1N1", &a), (11, 10), "p1 移动保留");
        assert_eq!(pos_in(&e, "W1N1", &b), (13, 10), "p1 移动保留");
        assert_eq!(pos_in(&e, "W0N1", &c), (20, 20), "p2 请求被拒绝");
        assert_eq!(
            e.player_state("p1").unwrap().raw_memory,
            "{\"p\":1}",
            "两名玩家整轮照常发布"
        );
        assert_eq!(e.player_state("p2").unwrap().raw_memory, "{\"p\":2}");
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"moves":2}),
            "观察 state 累计"
        );
        snapshots.push(e.world_snapshot());
    }
    assert_eq!(snapshots[0], snapshots[1], "串行与并发派发结果一致");
    Ok(())
}
