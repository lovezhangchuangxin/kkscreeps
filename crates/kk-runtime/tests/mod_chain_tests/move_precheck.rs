//! 移动请求预检链：输入完整性、请求级否决语义（对照障碍槽的胜者级）、
//! 拒绝回退、能力门与组合。
//!
//! 请求级语义展示：strong(move,move) 与 weak(move) 竞争 (40,19)。
//! 预检否决 strong 的**请求**使其不参与竞争，weak 独占目的格胜出并
//! 移动——障碍槽挡下胜者后无人可动，两者形成对照。
use super::*;
use harness::{creep_at, observe_pkg, pos_in};

fn contested(e: &mut EmbedEngine) -> anyhow::Result<(String, String)> {
    let strong = creep_at(e, "W1N1", "strong", "p1", 40, 20, &["move", "move"])?;
    let weak = creep_at(e, "W1N1", "weak", "p1", 41, 20, &["move"])?;
    Ok((strong, weak))
}

const RACE: &str = "Game.creeps.strong.move(TOP);Game.creeps.weak.move(TOP_LEFT)";

const FENCED: &str = "api.handlers.replace('builtin:room.movePrecheck',{id:'fenced'},ctx=>{
  for(const r of ctx.args.requests){
    if(r.targetX<10)ctx.precheck.block(r.moverId);
  }
  ctx.state={applied:(ctx.state?.applied||0)+1};
});";

const COURTESY: &str = "api.handlers.after('builtin:room.movePrecheck',{id:'courtesy'},ctx=>{
  for(const r of ctx.args.requests){
    if(r.movePower>r.weight)ctx.precheck.block(r.moverId);
  }
  ctx.state={applied:(ctx.state?.applied||0)+1};
});";

#[test]
fn precheck_input_is_complete_and_native_contrast() -> anyhow::Result<()> {
    // 初读校验字段形态；否决 strong 请求后 weak 独占竞争格——原生
    // 对照里 strong 胜出、weak 原地。
    let probe = package(
        "api.handlers.replace('builtin:room.movePrecheck',{id:'probe'},ctx=>{
        for(const r of ctx.args.requests){
          if(typeof r.moverId!=='string'||typeof r.owner!=='string'
            ||typeof r.originX!=='number'||typeof r.originY!=='number'
            ||typeof r.targetX!=='number'||typeof r.targetY!=='number'
            ||typeof r.movePower!=='number'||typeof r.weight!=='number'
            ||typeof r.pulled!=='boolean'||typeof r.pulling!=='boolean'
            ||typeof r.terrainWall!=='boolean'||typeof r.terrainSwamp!=='boolean'
            ||typeof r.hasRoad!=='boolean'||!Array.isArray(r.objects))throw Error('请求要素');
        }
        const strong=ctx.args.requests.find(r=>r.movePower>r.weight);
        if(!ctx.precheck.isBlocked(strong.moverId))ctx.precheck.block(strong.moverId);
        ctx.state={applied:(ctx.state?.applied||0)+1,count:ctx.args.requests.length};
      });",
    );
    let mut e = engine(&[("alpha", probe)], false)?;
    let (strong, weak) = contested(&mut e)?;
    e.run_player("p1", RACE)?;
    assert_eq!(pos_in(&e, "W1N1", &strong), (40, 20), "请求被否决原地");
    assert_eq!(pos_in(&e, "W1N1", &weak), (40, 19), "弱者独占竞争格");
    assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1,"count":2}));
    // 无请求的 tick 不进入 JS。
    e.run_player("p1", "Memory.tick=Game.time")?;
    assert_eq!(
        e.engine_mod_state("alpha"),
        json!({"applied":1,"count":2}),
        "计数不重复"
    );

    // 原生对照：无策略包（仅观察计数）时 strong 胜出、weak 原地。
    let mut e = engine(
        &[("watch", observe_pkg("builtin:room.movePrecheck"))],
        false,
    )?;
    let (strong, weak) = contested(&mut e)?;
    e.run_player("p1", RACE)?;
    assert_eq!(pos_in(&e, "W1N1", &strong), (40, 19));
    assert_eq!(pos_in(&e, "W1N1", &weak), (41, 20));
    Ok(())
}

#[test]
fn precheck_decline_falls_back_to_native() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.movePrecheck',{id:'decline'},ctx=>{
        ctx.precheck.block(ctx.args.requests[0].moverId);
        return api.decline('full');
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (strong, weak) = contested(&mut e)?;
    e.run_player("p1", RACE)?;
    // 拒绝丢弃全部否决：与原生一致。
    assert_eq!(pos_in(&e, "W1N1", &strong), (40, 19), "拒绝回退原生竞争");
    assert_eq!(pos_in(&e, "W1N1", &weak), (41, 20));
    assert_eq!(e.engine_mod_state("alpha"), Value::Null, "拒绝不提交 state");
    Ok(())
}

#[test]
fn precheck_after_sees_replace_blocks_via_is_blocked() -> anyhow::Result<()> {
    // 组合：replace（fenced 区域管制）+ after（courtesy 功率让行）。
    // after 层经 isBlocked 读到 replace 的否决；两者并集。
    let fenced = package(FENCED);
    let courtesy = package(COURTESY);
    let mut e = engine(&[("fenced", fenced), ("courtesy", courtesy)], false)?;
    let west = creep_at(&mut e, "W1N1", "west", "p1", 5, 30, &["move"])?;
    let (strong, weak) = contested(&mut e)?;
    e.run_player(
        "p1",
        "Game.creeps.west.move(RIGHT);Game.creeps.strong.move(TOP);Game.creeps.weak.move(TOP_LEFT)",
    )?;
    assert_eq!(pos_in(&e, "W1N1", &west), (5, 30), "fenced 否决西区请求");
    assert_eq!(
        pos_in(&e, "W1N1", &strong),
        (40, 20),
        "courtesy 否决功率过剩者"
    );
    assert_eq!(pos_in(&e, "W1N1", &weak), (40, 19));
    assert_eq!(e.engine_mod_state("fenced"), json!({"applied":1}));
    assert_eq!(e.engine_mod_state("courtesy"), json!({"applied":1}));
    Ok(())
}

#[test]
fn precheck_abilities_are_gated_to_precheck_slot() -> anyhow::Result<()> {
    // ctx.precheck 只在预检槽有效：在准入槽处理器里调用走宿主能力门
    // 故障，不能被 catch+decline 掩盖，整轮候选失败且世界不变。
    for slot in ["builtin:creep.moveAdmission", "builtin:room.moveObstacle"] {
        let source = package(&format!(
            "api.handlers.replace('{slot}',{{id:'wrong'}},ctx=>{{
            try{{ctx.precheck.block(ctx.args.movers[0].id)}}catch(e){{}}return api.decline('hide');
          }});"
        ));
        let mut e = engine(&[("bad", source)], false)?;
        let (_strong, _weak) = contested(&mut e)?;
        let before = e.world_snapshot();
        assert!(e.run_player("p1", RACE).is_err(), "{slot}");
        assert_eq!(e.world_snapshot(), before, "{slot}");
        assert_eq!(e.engine_mod_state("bad"), Value::Null, "{slot}");
    }
    Ok(())
}

#[test]
fn precheck_decline_discards_before_layer_state() -> anyhow::Result<()> {
    // before 层写 state 后中心拒绝：暂存 state 必须丢弃（与竞争/准入/
    // 障碍槽同语义），否则同输入重试会双计。
    let source = package(
        "api.handlers.before('builtin:room.movePrecheck',{id:'note'},ctx=>{
        ctx.state={seen:(ctx.state?.seen||0)+1};
      });
        api.handlers.replace('builtin:room.movePrecheck',{id:'gate'},ctx=>{
        return api.decline('full');
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (strong, weak) = contested(&mut e)?;
    e.run_player("p1", RACE)?;
    assert_eq!(pos_in(&e, "W1N1", &strong), (40, 19), "拒绝回退原生竞争");
    assert_eq!(pos_in(&e, "W1N1", &weak), (41, 20));
    assert_eq!(e.engine_mod_state("alpha"), Value::Null, "暂存 state 丢弃");
    Ok(())
}

#[test]
fn precheck_blocks_movement_slot_channel_request() -> anyhow::Result<()> {
    // 同源可观测证据：移动槽能力 ctx.movement.request 登记的请求
    // 与玩家意图请求汇入同一矩阵，预检统一否决。before 层把 creep
    // 的方向改到 TOP，预检包否决 targetY<10 的请求——通道无关。
    // after 层在 native 中心登记之后改向；before 先登记会被后到的
    // native 请求覆盖（后到覆盖先到，单执行者单请求）。
    let redirect = package(
        "api.handlers.after('builtin:creep.move',{id:'redirect'},ctx=>{
        if(ctx.args.direction===3){ctx.movement.request(1);}
        ctx.state={applied:(ctx.state?.applied||0)+1};
      });",
    );
    let north_gate = package(
        "api.handlers.replace('builtin:room.movePrecheck',{id:'northGate'},ctx=>{
        for(const r of ctx.args.requests){
          if(r.targetY<10)ctx.precheck.block(r.moverId);
        }
        ctx.state={applied:(ctx.state?.applied||0)+1};
      });",
    );
    // 仅改向：槽通道请求生效，creep 北移。
    let mut e = engine(&[("redirect", redirect.clone())], false)?;
    let creep = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W1N1", &creep), (10, 9), "槽通道改向生效");
    // 加预检：同一请求被否决，creep 原地。
    let mut e = engine(&[("redirect", redirect), ("gate", north_gate)], false)?;
    let creep = creep_at(&mut e, "W1N1", "m", "p1", 10, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m.move(RIGHT)")?;
    assert_eq!(pos_in(&e, "W1N1", &creep), (10, 10), "槽通道请求同被否决");
    assert_eq!(e.engine_mod_state("redirect"), json!({"applied":1}));
    assert_eq!(e.engine_mod_state("gate"), json!({"applied":1}));
    Ok(())
}

#[test]
fn precheck_and_obstacle_compose_in_same_room() -> anyhow::Result<()> {
    // 两级语义同房组合：预检否决 weak 的请求（弱者不再登记），strong
    // 独占胜出；障碍槽再挡 strong（胜者级）——(40,19) 无人进入。
    let precheck = package(
        "api.handlers.replace('builtin:room.movePrecheck',{id:'dropEast'},ctx=>{
        for(const r of ctx.args.requests){
          if(r.originX>40)ctx.precheck.block(r.moverId);
        }
      });",
    );
    let obstacle = package(
        "api.handlers.replace('builtin:room.moveObstacle',{id:'sealCell'},ctx=>{
        for(const m of ctx.args.movers){
          if(m.targetY<21&&m.targetX===40)ctx.obstacle.block(m.id);
        }
      });",
    );
    let mut e = engine(&[("p", precheck), ("o", obstacle)], false)?;
    let (strong, weak) = contested(&mut e)?;
    e.run_player("p1", RACE)?;
    assert_eq!(pos_in(&e, "W1N1", &strong), (40, 20), "胜者被障碍槽挡下");
    assert_eq!(pos_in(&e, "W1N1", &weak), (41, 20), "弱者请求已被预检否决");
    Ok(())
}

#[test]
fn precheck_fault_disable_falls_back_and_state_commits_once() -> anyhow::Result<()> {
    let bad = package(
        "api.handlers.replace('builtin:room.movePrecheck',{id:'fault'},ctx=>{
        ctx.state={dirty:true};throw Error('fault');
      });",
    );
    let healthy = package(COURTESY);
    let mut e = engine(&[("bad", bad), ("courtesy", healthy)], false)?;
    let (strong, weak) = contested(&mut e)?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", RACE).is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.engine_mod_state("courtesy"), Value::Null);
    // 停用出错包：下一轮回退 native 预检，健康 after 继续组合。
    e.disable_mod("bad")?;
    e.run_player("p1", RACE)?;
    assert_eq!(pos_in(&e, "W1N1", &strong), (40, 20), "courtesy 否决快车");
    assert_eq!(pos_in(&e, "W1N1", &weak), (40, 19));
    assert_eq!(e.engine_mod_state("courtesy"), json!({"applied":1}));
    assert_eq!(e.engine_mod_state("bad"), Value::Null);

    // 晚期写回失败经玩家缓存重试：否决与计数恰一次提交。
    for binary in [false, true] {
        let mut e = engine(&[("alpha", package(COURTESY))], binary)?;
        let (strong, weak) = contested(&mut e)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let code = format!("Memory.runs=1;{RACE}");
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        e.run_player("p1", "throw Error('cached only')")?;
        assert_eq!(pos_in(&e, "W1N1", &strong), (40, 20));
        assert_eq!(pos_in(&e, "W1N1", &weak), (40, 19));
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}
