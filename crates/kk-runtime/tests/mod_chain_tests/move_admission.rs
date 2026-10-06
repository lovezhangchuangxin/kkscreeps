//! 移动准入终值链：输入完整性、放行/收紧策略、拒绝回退、能力门与故障恢复。
//!
//! 方向形态 move() 在玩家面预检疲劳与 MOVE 部件（同官方），疲劳/无件
//! 移动者只能经朝向相邻 creep 的 move(target) 形态到达引擎——无配对时
//! 原生拒绝、准入槽可放行；有效配对时原生放行（正对照）。
use super::*;
use harness::{creep_at, observe_pkg, pos_in};

/// puller(10,10) 让位到 (10,9)；f(10,11) 无 MOVE 部件，经 move(puller)
/// 请求移入拉者原格。带 pull 意图时原生即获准。
fn pair(e: &mut EmbedEngine) -> anyhow::Result<(String, String)> {
    let puller = creep_at(e, "W1N1", "puller", "p1", 10, 10, &["move"])?;
    let f = creep_at(e, "W1N1", "f", "p1", 10, 11, &["work"])?;
    Ok((puller, f))
}

/// 无 pull 意图：f 未配对，原生拒绝其移动。
const DRAG: &str = "Game.creeps.puller.move(TOP);Game.creeps.f.move(Game.creeps.puller)";
/// 有 pull 意图：配对完好，f 免除部件门槛原生获准。
use std::sync::LazyLock;
static PULL: LazyLock<String> =
    LazyLock::new(|| format!("Game.creeps.puller.pull(Game.creeps.f);{DRAG}"));

const TIRELESS: &str = "api.handlers.replace('builtin:creep.moveAdmission',{id:'tireless'},ctx=>{
  if(!Array.isArray(ctx.args.movers))throw Error('movers 必须为数组');
  for(const m of ctx.args.movers){
    if(!m.id||!m.owner||typeof m.originX!=='number'||typeof m.originY!=='number'
      ||typeof m.targetX!=='number'||typeof m.targetY!=='number'
      ||typeof m.oldFatigue!=='number'||typeof m.activeMoves!=='number'
      ||typeof m.movePower!=='number'||typeof m.weight!=='number'
      ||typeof m.pulled!=='boolean')throw Error('准入要素');
    ctx.admission.setAdmitted(m.id,true);
  }
  ctx.state={applied:(ctx.state?.applied||0)+1};
});";

#[test]
fn admission_input_is_complete_and_override_admits_unpaired_mover() -> anyhow::Result<()> {
    // 初读校验：未配对无件移动者的原生终值为否，放行后移动生效。
    let probe = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'probe'},ctx=>{
        for(const m of ctx.args.movers){
          if(m.activeMoves===0&&ctx.admission.getAdmitted(m.id)!==false)throw Error('原生初值');
        }
        for(const m of ctx.args.movers)ctx.admission.setAdmitted(m.id,true);
        ctx.state={applied:(ctx.state?.applied||0)+1};
      });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", probe.clone())], binary)?;
        let (puller, f) = pair(&mut e)?;
        e.run_player("p1", DRAG)?;
        assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9), "拉者让位");
        assert_eq!(pos_in(&e, "W1N1", &f), (10, 10), "未配对无件者被放行");
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        // 无移动者的 tick 不进入 JS，也不重复计数。
        e.run_player("p1", "Memory.tick=Game.time")?;
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
    }
    Ok(())
}

#[test]
fn admission_native_pairing_still_admits_pulled_mover() -> anyhow::Result<()> {
    // 正对照：仅观察准入输入，不覆盖终值。有效配对的被拉者原生获准。
    let source = observe_pkg("builtin:creep.moveAdmission");
    let mut e = engine(&[("alpha", source)], false)?;
    let (puller, f) = pair(&mut e)?;
    e.run_player("p1", &PULL)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
    assert_eq!(pos_in(&e, "W1N1", &f), (10, 10), "配对完好：被拉者原生放行");
    Ok(())
}

#[test]
fn admission_decline_falls_back_to_native_gate() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'pass'},ctx=>{
        for(const m of ctx.args.movers)ctx.admission.setAdmitted(m.id,true);
        return api.decline('strict-day');
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (puller, f) = pair(&mut e)?;
    e.run_player("p1", DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
    assert_eq!(pos_in(&e, "W1N1", &f), (10, 11), "拒绝回退原生部件门槛");
    assert_eq!(
        e.engine_mod_state("alpha"),
        Value::Null,
        "拒绝丢弃暂存 state"
    );
    Ok(())
}

#[test]
fn admission_strict_policy_tightens_native_gate() -> anyhow::Result<()> {
    // 独立策略：movePower<weight 视为超载，收紧原生门槛（原生只要求
    // 疲劳 0 且有存活 MOVE）。原生对照：观察者包下同 creep 可动。
    let source = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'strict'},ctx=>{
        for(const m of ctx.args.movers){
          if(m.movePower<m.weight)ctx.admission.setAdmitted(m.id,false);
        }
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let heavy = creep_at(
        &mut e,
        "W1N1",
        "heavy",
        "p1",
        30,
        30,
        &["work", "work", "move"],
    )?;
    let light = creep_at(&mut e, "W1N1", "light", "p1", 10, 10, &["move"])?;
    e.run_player(
        "p1",
        "Game.creeps.heavy.move(BOTTOM);Game.creeps.light.move(BOTTOM)",
    )?;
    assert_eq!(pos_in(&e, "W1N1", &heavy), (30, 30), "超载被收紧拒绝");
    assert_eq!(pos_in(&e, "W1N1", &light), (10, 11), "功率配重达标照常获准");

    let mut plain = engine(
        &[("audit", observe_pkg("builtin:creep.moveAdmission"))],
        false,
    )?;
    let heavy = creep_at(
        &mut plain,
        "W1N1",
        "heavy",
        "p1",
        30,
        30,
        &["work", "work", "move"],
    )?;
    plain.run_player("p1", "Game.creeps.heavy.move(BOTTOM)")?;
    assert_eq!(pos_in(&plain, "W1N1", &heavy), (30, 31), "原生对照可动");
    Ok(())
}

#[test]
fn admission_invalid_values_and_foreign_capabilities_cannot_be_hidden_by_decline()
-> anyhow::Result<()> {
    for call in [
        "ctx.admission.setAdmitted('kk0000000000000a',true)",
        "ctx.admission.setAdmitted('kkffffffffffffffff',true)",
        "ctx.admission.setAdmitted('xx',true)",
        "ctx.admission.setAdmitted(null,true)",
        "ctx.admission.setAdmitted(123,true)",
        "ctx.admission.setAdmitted(ctx.args.movers[0].id,'yes')",
        "ctx.admission.getAdmitted(null)",
        "ctx.contest.setPriority('kk0000000000000a',1)",
        "ctx.movement.request(3)",
        "ctx.resources.transfer('a','b','energy',0,0)",
        "ctx.incoming.setDamage(0)",
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.moveAdmission',{{id:'bad'}},ctx=>{{
            try{{{call}}}catch(e){{}}return api.decline('hide');
        }});"
        ));
        let mut e = engine(&[("bad", source)], false)?;
        let (_puller, f) = pair(&mut e)?;
        let before = e.world_snapshot();
        assert!(e.run_player("p1", DRAG).is_err(), "{call}");
        assert_eq!(e.world_snapshot(), before, "{call}");
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
        assert_eq!(pos_in(&e, "W1N1", &f), (10, 11));
    }
    Ok(())
}

#[test]
fn admission_abilities_are_unavailable_in_action_slots() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.before('builtin:creep.move',{id:'wrong'},ctx=>{
        try{ctx.admission.getAdmitted(ctx.target.id)}catch(e){}
        try{ctx.admission.evaluate()}catch(e){}
        return api.decline('hide');
      });",
    );
    let mut e = engine(&[("bad", source)], false)?;
    let _f = creep_at(&mut e, "W1N1", "f", "p1", 10, 10, &["move"])?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", "Game.creeps.f.move(RIGHT)").is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.engine_mod_state("bad"), Value::Null);
    Ok(())
}

#[test]
fn admission_evaluate_is_unavailable_in_obstacle_and_contest_slots() -> anyhow::Result<()> {
    // evaluate 只属于准入槽：在障碍槽或竞争槽的处理器里调用走宿主
    // 能力门故障（chain.host.admission 为 None），不能被 catch+decline
    // 掩盖，整轮候选失败且世界不变。
    for slot in ["builtin:room.moveObstacle", "builtin:room.moveContest"] {
        let source = package(&format!(
            "api.handlers.replace('{slot}',{{id:'wrong'}},ctx=>{{
            try{{ctx.admission.evaluate()}}catch(e){{}}return api.decline('hide');
          }});"
        ));
        let mut e = engine(&[("bad", source)], false)?;
        // 两格竞争触发竞争槽与障碍槽；准入槽为 Native 使 evaluate
        // 在这两个槽内的调用是唯一入口。
        let _one = creep_at(&mut e, "W1N1", "one", "p1", 10, 10, &["move", "move"])?;
        let _two = creep_at(&mut e, "W1N1", "two", "p1", 11, 10, &["move"])?;
        let before = e.world_snapshot();
        // 两者同格竞争 (10,9)：竞争槽与障碍槽都必然进入 JS。
        let script = "Game.creeps.one.move(TOP);Game.creeps.two.move(TOP_LEFT)";
        assert!(e.run_player("p1", script).is_err(), "{slot}");
        assert_eq!(e.world_snapshot(), before, "{slot}");
        assert_eq!(e.engine_mod_state("bad"), Value::Null, "{slot}");
    }
    Ok(())
}

#[test]
fn admission_evaluate_sees_overrides_and_requests_view() -> anyhow::Result<()> {
    // 覆盖→预演的端到端闭环：候选先按原生终值预演（未配对无件者被
    // 链取消，幸存 1），放行同一执行者后预演幸存回到 2——预演按候选
    // 当前终值即时计算，不是快照。requests 视野含拉者与被拉者两条
    // 登记（均 won=true、pulledBy 为 null——无 pull 意图未配对）。
    let source = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'probe'},ctx=>{
        const shape = ctx.args.requests.every((r)=>
          typeof r.moverId==='string'&&typeof r.x==='number'&&typeof r.y==='number'
          &&typeof r.index==='number'&&typeof r.originX==='number'
          &&typeof r.originY==='number'&&typeof r.won==='boolean');
        if(!shape||ctx.args.requests.length!==ctx.args.movers.length)throw Error('请求视野');
        const before = ctx.admission.evaluate();
        const unpaired = ctx.args.movers.find((m)=>m.activeMoves===0);
        ctx.admission.setAdmitted(unpaired.id, true);
        const after = ctx.admission.evaluate();
        ctx.state={before: before.length, after: after.length,
          requests: ctx.args.requests.length};
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (puller, f) = pair(&mut e)?;
    e.run_player("p1", DRAG)?;
    assert_eq!(
        e.engine_mod_state("alpha"),
        json!({"before": 1, "after": 2, "requests": 2}),
        "预演幸存随候选覆盖扩大"
    );
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9), "拉者让位");
    assert_eq!(pos_in(&e, "W1N1", &f), (10, 10), "放行后未配对者随链存活");
    Ok(())
}

#[test]
fn admission_fault_disable_falls_back_to_native_next_tick() -> anyhow::Result<()> {
    let bad = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'fault'},ctx=>{
        ctx.state={dirty:true};throw Error('fault');
      });",
    );
    let healthy = package(
        "api.handlers.after('builtin:creep.moveAdmission',{id:'audit'},ctx=>{
        ctx.state={calls:(ctx.state?.calls||0)+1};
      });",
    );
    let mut e = engine(&[("bad", bad), ("healthy", healthy)], false)?;
    let (puller, f) = pair(&mut e)?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", DRAG).is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.engine_mod_state("healthy"), Value::Null);
    e.disable_mod("bad")?;
    // 停用后健康钩子与 native 门槛继续：拉者让位、未配对无件者不动、
    // audit 恰计一次。
    e.run_player("p1", DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
    assert_eq!(pos_in(&e, "W1N1", &f), (10, 11));
    assert_eq!(e.engine_mod_state("healthy"), json!({"calls":1}));
    assert_eq!(e.engine_mod_state("bad"), Value::Null);
    Ok(())
}

#[test]
fn admission_state_commits_once_after_late_retry() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine(&[("alpha", package(TIRELESS))], binary)?;
        let (puller, f) = pair(&mut e)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let code = format!("Memory.runs=1;{DRAG}");
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        // 玩家缓存重试：准入放行与计数恰提交一次。
        e.run_player("p1", "throw Error('cached only')")?;
        assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
        assert_eq!(pos_in(&e, "W1N1", &f), (10, 10));
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}

#[test]
fn admission_composes_with_contest_slot_in_same_tick() -> anyhow::Result<()> {
    // 同一包同时挂竞争排序与准入放行：排序决定谁胜出（heavy 反超
    // 默认功率比胜者），准入放行未配对无件的 f——两槽解耦、同 tick
    // 均运行且互不否决。
    let source = package(
        "api.handlers.replace('builtin:room.moveContest',{id:'heaviest'},ctx=>{
        for(const cell of ctx.args.contests){
          let best=-1,pick=null;
          for(const c of cell.contenders)if(c.weight>best){best=c.weight;pick=c.id;}
          for(const c of cell.contenders)ctx.contest.setPriority(c.id,c.id===pick?1:-1);
        }
      });
      api.handlers.replace('builtin:creep.moveAdmission',{id:'tireless'},ctx=>{
        for(const m of ctx.args.movers)ctx.admission.setAdmitted(m.id,true);
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    for (name, pos, body) in [
        ("mid", (10, 10), vec!["work", "move"]),
        ("light", (11, 11), vec!["move", "move"]),
        ("heavy", (10, 12), vec!["work", "work", "move"]),
    ] {
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":name,"owner":"p1","pos":pos,"body":body,"ticksToLive":100}),
        )?;
    }
    let puller = creep_at(&mut e, "W1N1", "puller", "p1", 10, 20, &["move"])?;
    let f = creep_at(&mut e, "W1N1", "f", "p1", 10, 21, &["work"])?;
    e.run_player(
        "p1",
        "Game.creeps.mid.move(BOTTOM);Game.creeps.light.move(LEFT);Game.creeps.heavy.move(TOP);\
         Game.creeps.puller.move(TOP);Game.creeps.f.move(Game.creeps.puller)",
    )?;
    let pos = |e: &EmbedEngine, name: &str| {
        e.find_in_room("W1N1", kk_core::constants::FIND_CREEPS)
            .into_iter()
            .find(|o| o["name"] == name)
            .map(|o| {
                (
                    o["pos"]["x"].as_u64().unwrap(),
                    o["pos"]["y"].as_u64().unwrap(),
                )
            })
            .unwrap()
    };
    assert_eq!(pos(&e, "heavy"), (10, 11), "竞争排序使最重者胜出");
    assert_eq!(pos(&e, "light"), (11, 11), "落选者不递补");
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 19));
    assert_eq!(pos_in(&e, "W1N1", &f), (10, 20), "准入放行与竞争排序解耦");
    Ok(())
}

#[test]
fn admission_governs_capability_request_channel_too() -> anyhow::Result<()> {
    // 移动槽能力 request 重登记的请求与玩家意图在预填汇合：strict
    // 收紧后经能力通道登记的移动同样被拒（同源消费），观察者对照下
    // 同一移动照常执行。
    let strict = package(
        "api.handlers.before('builtin:creep.move',{id:'reroute'},ctx=>{
        if(ctx.target.fields && ctx.target.fields.name==='heavy')ctx.movement.request(5);
      });
      api.handlers.after('builtin:creep.moveAdmission',{id:'strict'},ctx=>{
        for(const m of ctx.args.movers)if(m.movePower<m.weight)ctx.admission.setAdmitted(m.id,false);
      });",
    );
    let mut e = engine(&[("alpha", strict)], false)?;
    let heavy = creep_at(
        &mut e,
        "W1N1",
        "heavy",
        "p1",
        30,
        30,
        &["work", "work", "move"],
    )?;
    e.run_player("p1", "Game.creeps.heavy.move(BOTTOM)")?;
    assert_eq!(
        pos_in(&e, "W1N1", &heavy),
        (30, 30),
        "能力通道登记的请求同吃准入终值"
    );

    let mut plain = engine(
        &[("audit", observe_pkg("builtin:creep.moveAdmission"))],
        false,
    )?;
    let heavy = creep_at(
        &mut plain,
        "W1N1",
        "heavy",
        "p1",
        30,
        30,
        &["work", "work", "move"],
    )?;
    plain.run_player("p1", "Game.creeps.heavy.move(BOTTOM)")?;
    assert_eq!(
        pos_in(&plain, "W1N1", &heavy),
        (30, 31),
        "对照：无收紧照常移动"
    );
    Ok(())
}
