//! 移动障碍追加链：输入完整性、追加取消与让位链传播、拒绝回退、
//! 能力门与故障恢复。
//!
//! 交换让位链：a(10,10)↔b(10,11) 互为原格目的格；追加阻挡任一方
// 会让另一方失去让位依赖、随链取消——单向 block 不能放行 native
//! 阻挡，但取消会沿让位依赖传播。
use super::*;
use harness::{creep_at, observe_pkg, pos_in};

fn swap_trio(e: &mut EmbedEngine) -> anyhow::Result<(String, String, String)> {
    let a = creep_at(e, "W1N1", "a", "p1", 10, 10, &["move"])?;
    let b = creep_at(e, "W1N1", "b", "p1", 10, 11, &["move"])?;
    let c = creep_at(e, "W1N1", "c", "p1", 30, 30, &["move"])?;
    Ok((a, b, c))
}

const SWAP: &str = "Game.creeps.a.move(BOTTOM);Game.creeps.b.move(TOP);Game.creeps.c.move(BOTTOM)";

const STAY_NORTH: &str = "api.handlers.replace('builtin:room.moveObstacle',{id:'stayNorth'},ctx=>{
  for(const m of ctx.args.movers){
    if(m.targetY>=20)ctx.obstacle.block(m.id);
  }
  ctx.state={applied:(ctx.state?.applied||0)+1};
});";

const NO_SWAP: &str = "api.handlers.after('builtin:room.moveObstacle',{id:'noSwap'},ctx=>{
  for(const m of ctx.args.movers){
    if(m.objects.some(o=>o.kind==='creep'&&o.yielding))ctx.obstacle.block(m.id);
  }
  ctx.state={applied:(ctx.state?.applied||0)+1};
});";

#[test]
fn obstacle_input_is_complete_and_block_propagates_chain() -> anyhow::Result<()> {
    // 输入完整性与追加传播：交换双方的目的格摘要互含对方（yielding），
    // c 的目的格为空；block a 后 b 随让位链取消，c 不受影响。
    let source = package(
        "api.handlers.replace('builtin:room.moveObstacle',{id:'probe'},ctx=>{
        if(ctx.args.movers.length!==5)throw Error('胜者不得截断（含准入被拒者）');
        for(const m of ctx.args.movers){
          if(!m.id||!m.owner||typeof m.targetX!=='number'||typeof m.targetY!=='number'
            ||typeof m.terrainWall!=='boolean'||typeof m.hasRoad!=='boolean'
            ||typeof m.nativeBlocked!=='boolean'||!Array.isArray(m.objects))throw Error('障碍要素');
          for(const o of m.objects){
            if(!o.id||typeof o.kind!=='string'||typeof o.owner!=='string'
              ||typeof o.isPublic!=='boolean'||typeof o.yielding!=='boolean')throw Error('对象摘要');
          }
          if(ctx.obstacle.isBlocked(m.id)!==m.nativeBlocked)throw Error('初判可读');
        }
        for(const m of ctx.args.movers)if(m.targetY===10)ctx.obstacle.block(m.id);
        ctx.state={applied:(ctx.state?.applied||0)+1};
      });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let (a, b, c) = swap_trio(&mut e)?;
        // 未配对无件者：准入终值为否，但仍在障碍输入中（对被拒者
        // 追加阻挡无额外效果）。probe 已断言 movers 数恰为胜者数。
        let puller = creep_at(&mut e, "W1N1", "puller", "p1", 10, 20, &["move"])?;
        creep_at(&mut e, "W1N1", "f", "p1", 10, 21, &["work"])?;
        e.run_player(
            "p1",
            &format!("{SWAP};Game.creeps.puller.move(TOP);Game.creeps.f.move(Game.creeps.puller)"),
        )?;
        assert_eq!(pos_in(&e, "W1N1", &puller), (10, 19));
        assert_eq!(pos_in(&e, "W1N1", &b), (10, 11), "b 被直接追加阻挡");
        assert_eq!(
            pos_in(&e, "W1N1", &a),
            (10, 10),
            "a 随让位链取消：b 不再让位 (10,11)"
        );
        assert_eq!(pos_in(&e, "W1N1", &c), (30, 31), "无依赖者照常移动");
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        // 无移动者的 tick 不进入 JS。
        e.run_player("p1", "Memory.tick=Game.time")?;
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
    }
    Ok(())
}

#[test]
fn obstacle_native_swap_and_two_independent_policies() -> anyhow::Result<()> {
    // 原生对照：交换双方与 c 全部移动。
    let mut plain = engine(
        &[("audit", observe_pkg("builtin:room.moveObstacle"))],
        false,
    )?;
    let (a, b, c) = swap_trio(&mut plain)?;
    plain.run_player("p1", SWAP)?;
    assert_eq!(pos_in(&plain, "W1N1", &a), (10, 11));
    assert_eq!(pos_in(&plain, "W1N1", &b), (10, 10));
    assert_eq!(pos_in(&plain, "W1N1", &c), (30, 31));

    // 策略 A stayNorth：仅阻挡南下的 c，交换不受影响。
    let mut ea = engine(&[("stay", package(STAY_NORTH))], false)?;
    let (a, b, c) = swap_trio(&mut ea)?;
    ea.run_player("p1", SWAP)?;
    assert_eq!(pos_in(&ea, "W1N1", &a), (10, 11), "A：交换不受影响");
    assert_eq!(pos_in(&ea, "W1N1", &b), (10, 10));
    assert_eq!(pos_in(&ea, "W1N1", &c), (30, 30), "A：南下者被阻挡");
    assert_eq!(ea.engine_mod_state("stay"), json!({"applied":1}));

    // 策略 B noSwap：禁止依赖让位，交换双方全灭，c 照常。
    let mut eb = engine(&[("noswap", package(NO_SWAP))], false)?;
    let (a, b, c) = swap_trio(&mut eb)?;
    eb.run_player("p1", SWAP)?;
    assert_eq!(pos_in(&eb, "W1N1", &a), (10, 10), "B：依赖让位者被阻挡");
    assert_eq!(pos_in(&eb, "W1N1", &b), (10, 11));
    assert_eq!(pos_in(&eb, "W1N1", &c), (30, 31), "B：独立移动不受影响");
    assert_eq!(eb.engine_mod_state("noswap"), json!({"applied":1}));
    Ok(())
}

#[test]
fn obstacle_policies_compose_by_union() -> anyhow::Result<()> {
    // 组合：stayNorth replace + noSwap after——追加集在同一候选上
    // 取并集，交换与南下者全部被阻挡。
    let source = format!("{NO_SWAP}\n{STAY_NORTH}");
    let mut e = engine(&[("combo", package(&source))], false)?;
    let (a, b, c) = swap_trio(&mut e)?;
    e.run_player("p1", SWAP)?;
    assert_eq!(pos_in(&e, "W1N1", &a), (10, 10), "组合：交换被 noSwap 挡");
    assert_eq!(pos_in(&e, "W1N1", &b), (10, 11));
    assert_eq!(
        pos_in(&e, "W1N1", &c),
        (30, 30),
        "组合：南下被 stayNorth 挡"
    );
    Ok(())
}

#[test]
fn obstacle_decline_falls_back_to_native() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.moveObstacle',{id:'pass'},ctx=>{
        for(const m of ctx.args.movers)ctx.obstacle.block(m.id);
        return api.decline('open-day');
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (a, b, c) = swap_trio(&mut e)?;
    e.run_player("p1", SWAP)?;
    assert_eq!(pos_in(&e, "W1N1", &a), (10, 11), "拒绝丢弃追加回退 native");
    assert_eq!(pos_in(&e, "W1N1", &b), (10, 10));
    assert_eq!(pos_in(&e, "W1N1", &c), (30, 31));
    assert_eq!(e.engine_mod_state("alpha"), Value::Null);
    Ok(())
}

#[test]
fn obstacle_invalid_values_and_foreign_capabilities_cannot_be_hidden_by_decline()
-> anyhow::Result<()> {
    for call in [
        "ctx.obstacle.block('kk0000000000000a')",
        "ctx.obstacle.block('kkffffffffffffffff')",
        "ctx.obstacle.block('xx')",
        "ctx.obstacle.block(null)",
        "ctx.obstacle.block(123)",
        "ctx.obstacle.isBlocked(null)",
        "ctx.contest.setPriority('kk0000000000000a',1)",
        "ctx.admission.setAdmitted('kk0000000000000a',true)",
        "ctx.movement.request(3)",
        "ctx.resources.transfer('a','b','energy',0,0)",
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:room.moveObstacle',{{id:'bad'}},ctx=>{{
            try{{{call}}}catch(e){{}}return api.decline('hide');
        }});"
        ));
        let mut e = engine(&[("bad", source)], false)?;
        let (a, _b, _c) = swap_trio(&mut e)?;
        let before = e.world_snapshot();
        assert!(e.run_player("p1", SWAP).is_err(), "{call}");
        assert_eq!(e.world_snapshot(), before, "{call}");
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
        assert_eq!(pos_in(&e, "W1N1", &a), (10, 10));
    }
    Ok(())
}

#[test]
fn obstacle_abilities_are_unavailable_in_other_slots() -> anyhow::Result<()> {
    // 动作槽调用障碍能力必须故障且不能被 decline 掩盖。
    let source = package(
        "api.handlers.before('builtin:creep.move',{id:'wrong'},ctx=>{
        try{ctx.obstacle.block(ctx.target.id)}catch(e){}return api.decline('hide');
      });",
    );
    let mut e = engine(&[("bad", source)], false)?;
    creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["move"])?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", "Game.creeps.a.move(RIGHT)").is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.engine_mod_state("bad"), Value::Null);
    Ok(())
}

#[test]
fn obstacle_fault_disable_falls_back_to_native_next_tick() -> anyhow::Result<()> {
    let bad = package(
        "api.handlers.replace('builtin:room.moveObstacle',{id:'fault'},ctx=>{
        ctx.state={dirty:true};throw Error('fault');
      });",
    );
    let healthy = package(
        "api.handlers.after('builtin:room.moveObstacle',{id:'audit'},ctx=>{
        ctx.state={calls:(ctx.state?.calls||0)+1};
      });",
    );
    let mut e = engine(&[("bad", bad), ("healthy", healthy)], false)?;
    let (a, b, c) = swap_trio(&mut e)?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", SWAP).is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.engine_mod_state("healthy"), Value::Null);
    e.disable_mod("bad")?;
    e.run_player("p1", SWAP)?;
    assert_eq!(pos_in(&e, "W1N1", &a), (10, 11), "停用后回退 native 交换");
    assert_eq!(pos_in(&e, "W1N1", &b), (10, 10));
    assert_eq!(pos_in(&e, "W1N1", &c), (30, 31));
    assert_eq!(e.engine_mod_state("healthy"), json!({"calls":1}));
    assert_eq!(e.engine_mod_state("bad"), Value::Null);
    Ok(())
}

#[test]
fn obstacle_state_commits_once_after_late_retry() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine(&[("alpha", package(NO_SWAP))], binary)?;
        let (a, b, c) = swap_trio(&mut e)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let code = format!("Memory.runs=1;{SWAP}");
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        // 玩家缓存重试：追加与计数恰提交一次。
        e.run_player("p1", "throw Error('cached only')")?;
        assert_eq!(pos_in(&e, "W1N1", &a), (10, 10));
        assert_eq!(pos_in(&e, "W1N1", &b), (10, 11));
        assert_eq!(pos_in(&e, "W1N1", &c), (30, 31));
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}
