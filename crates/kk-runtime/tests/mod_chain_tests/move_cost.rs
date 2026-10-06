//! 移动代价链：输入完整性、覆盖替换语义（对照原生沼泽停走）、拒绝
//! 回退、组合（replace×after 逐层改写）、能力门与故障停用回退。
//!
//! 场景：scout(move,work 重量 1) 从平原 (19,25) 向右进入沼泽带
//! （x=20..49）；walker(move,work) 在纯平原 (5,25) 向右。原生沼泽
//! 代价 10、衰减 2，scout 进沼泽后停走；平原代价 2 与衰减相抵，
//! walker 每 tick 前进。
use super::*;
use harness::{creep_at, observe_pkg, pos_in};

/// 50×50 平原，x≥20 为沼泽（位掩码 2）。
fn swamp_shard() -> Value {
    let mut terrain = vec![0u8; 2500];
    for y in 0..50 {
        for x in 20..50 {
            terrain[y * 50 + x] = 2;
        }
    }
    json!({
        "players": ["p1"], "rooms": [
            {"name": "W1N1", "rcl": 6, "owner": "p1", "terrain": terrain}
        ]
    })
}

fn engine_with_swamp(packages: &[(&str, String)], kkpv: bool) -> anyhow::Result<EmbedEngine> {
    engine_with_shard(
        packages,
        kkpv,
        &[],
        RegisterOptions::default(),
        &swamp_shard(),
    )
}

fn scouts(e: &mut EmbedEngine) -> anyhow::Result<(String, String)> {
    let scout = creep_at(e, "W1N1", "scout", "p1", 19, 25, &["move", "work"])?;
    let walker = creep_at(e, "W1N1", "walker", "p1", 5, 25, &["move", "work"])?;
    Ok((scout, walker))
}

const WALK: &str = "Game.creeps.scout.move(RIGHT);Game.creeps.walker.move(RIGHT)";

/// 沼泽步行：被拉/重载策略不做假设，只按要素把代价压到平原当量。
const SWAMPWALKER: &str = "api.handlers.after('builtin:room.moveCost',{id:'swampwalk'},ctx=>{
  for(const m of ctx.args.movers){
    if(m.terrainSwamp&&!m.hasRoad&&ctx.cost.getFatigue(m.id)!==m.weight*2){
      ctx.cost.setFatigue(m.id,m.weight*2);
    }
  }
  ctx.state={applied:(ctx.state?.applied||0)+1,count:ctx.args.movers.length};
});";

/// 沼泽化：一切移动代价翻倍（封顶 u32::MAX）。
const BOG: &str = "api.handlers.replace('builtin:room.moveCost',{id:'bog'},ctx=>{
  for(const m of ctx.args.movers){
    ctx.cost.setFatigue(m.id,Math.min(ctx.cost.getFatigue(m.id)*2,4294967295));
  }
  ctx.state={applied:(ctx.state?.applied||0)+1,count:ctx.args.movers.length};
});";

#[test]
fn cost_input_is_complete_and_native_contrast() -> anyhow::Result<()> {
    // 初读校验字段形态与原生代价要素；原生对照：scout 进沼泽一步后
    // 停走（代价 10 衰减 2），walker 平原每 tick 前进。
    let probe = package(
        "api.handlers.replace('builtin:room.moveCost',{id:'probe'},ctx=>{
        for(const m of ctx.args.movers){
          if(typeof m.id!=='string'||typeof m.owner!=='string'
            ||typeof m.originX!=='number'||typeof m.originY!=='number'
            ||typeof m.targetX!=='number'||typeof m.targetY!=='number'
            ||typeof m.weight!=='number'||typeof m.terrainSwamp!=='boolean'
            ||typeof m.hasRoad!=='boolean'||typeof m.bodyLength!=='number'
            ||typeof m.nativeFatigue!=='number')throw Error('代价要素');
          if(m.pulledBy!==null&&typeof m.pulledBy!=='string')throw Error('拉者要素');
        }
        const plains=ctx.args.movers.find(m=>!m.terrainSwamp&&!m.hasRoad);
        if(plains.nativeFatigue!==plains.weight*2)throw Error('平原原生代价');
        const swamp=ctx.args.movers.find(m=>m.terrainSwamp);
        if(swamp&&swamp.nativeFatigue!==swamp.weight*10)throw Error('沼泽原生代价');
        ctx.state={applied:(ctx.state?.applied||0)+1,count:ctx.args.movers.length};
      });",
    );
    let mut e = engine_with_swamp(&[("alpha", probe)], false)?;
    let (scout, walker) = scouts(&mut e)?;
    for _ in 0..3 {
        e.run_player("p1", WALK)?;
    }
    assert_eq!(pos_in(&e, "W1N1", &scout), (20, 25), "原生沼泽代价停走");
    assert_eq!(pos_in(&e, "W1N1", &walker), (8, 25), "平原代价与衰减相抵");
    // 无移动者的 tick 不进入 JS。
    e.run_player("p1", "Memory.tick=Game.time")?;
    // 3 轮各恰一次；scout 自第 2 轮起停走（疲劳未清），最后一轮输入
    // 只剩 walker——停走者不产生请求，不进输入。
    assert_eq!(
        e.engine_mod_state("alpha"),
        json!({"applied":3,"count":1}),
        "每移动 tick 恰一次、空轮不进 JS"
    );

    // 观察包对照：槽进入宿主路径但不改动结算，与原生一致。
    let mut e = engine_with_swamp(&[("watch", observe_pkg("builtin:room.moveCost"))], false)?;
    let (scout, walker) = scouts(&mut e)?;
    for _ in 0..3 {
        e.run_player("p1", WALK)?;
    }
    assert_eq!(pos_in(&e, "W1N1", &scout), (20, 25));
    assert_eq!(pos_in(&e, "W1N1", &walker), (8, 25));
    Ok(())
}

#[test]
fn swampwalker_keeps_moving_with_reduced_cost() -> anyhow::Result<()> {
    // 沼泽代价压到平原当量：scout 每 tick 前进；getFatigue 缺省读原生。
    let source = package(SWAMPWALKER);
    let mut e = engine_with_swamp(&[("swampwalk", source)], false)?;
    let (scout, walker) = scouts(&mut e)?;
    for _ in 0..3 {
        e.run_player("p1", WALK)?;
    }
    assert_eq!(pos_in(&e, "W1N1", &scout), (22, 25), "沼泽按平原当量续走");
    assert_eq!(pos_in(&e, "W1N1", &walker), (8, 25), "平原策略不动");
    assert_eq!(
        e.engine_mod_state("swampwalk"),
        json!({"applied":3,"count":2})
    );
    Ok(())
}

#[test]
fn bog_doubles_cost_and_stalls_even_on_plains() -> anyhow::Result<()> {
    // 代价翻倍：平原累积疲劳导致隔 tick 前进，沼泽立即停走。
    let source = package(BOG);
    let mut e = engine_with_swamp(&[("bog", source)], false)?;
    let (scout, walker) = scouts(&mut e)?;
    for _ in 0..3 {
        e.run_player("p1", WALK)?;
    }
    assert_eq!(pos_in(&e, "W1N1", &scout), (20, 25), "沼泽翻倍仍停走");
    assert_eq!(pos_in(&e, "W1N1", &walker), (7, 25), "平原翻倍隔 tick 前进");
    // 第 2 轮两个移动者都在疲劳中停走不进槽；第 3 轮 walker 疲劳
    // 清零恢复移动，槽再次进入且输入只剩它。
    assert_eq!(e.engine_mod_state("bog"), json!({"applied":2,"count":1}));
    Ok(())
}

#[test]
fn cost_decline_falls_back_to_native() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.moveCost',{id:'decline'},ctx=>{
        ctx.cost.setFatigue(ctx.args.movers[0].id,0);
        return api.decline('full');
      });",
    );
    let mut e = engine_with_swamp(&[("alpha", source)], false)?;
    let (scout, walker) = scouts(&mut e)?;
    for _ in 0..3 {
        e.run_player("p1", WALK)?;
    }
    // 拒绝丢弃全部覆盖：与原生一致。
    assert_eq!(pos_in(&e, "W1N1", &scout), (20, 25));
    assert_eq!(pos_in(&e, "W1N1", &walker), (8, 25));
    assert_eq!(e.engine_mod_state("alpha"), Value::Null, "拒绝不提交 state");
    Ok(())
}

#[test]
fn cost_composes_replace_then_after() -> anyhow::Result<()> {
    // bog replace 翻倍后，swampwalker after 经 getFatigue 读到翻倍值
    // 并把沼泽压回平原当量——层次组合终值由后层改写决定。
    let bog = package(BOG);
    let swampwalk = package(SWAMPWALKER);
    let mut e = engine_with_swamp(&[("bog", bog), ("swampwalk", swampwalk)], false)?;
    let (scout, walker) = scouts(&mut e)?;
    for _ in 0..3 {
        e.run_player("p1", WALK)?;
    }
    assert_eq!(pos_in(&e, "W1N1", &scout), (22, 25), "后层压回平原当量");
    assert_eq!(
        pos_in(&e, "W1N1", &walker),
        (7, 25),
        "平原仍为翻倍值（后层不动）"
    );
    assert_eq!(e.engine_mod_state("bog"), json!({"applied":3,"count":2}));
    assert_eq!(
        e.engine_mod_state("swampwalk"),
        json!({"applied":3,"count":2})
    );
    Ok(())
}

#[test]
fn cost_abilities_are_gated_to_cost_slot() -> anyhow::Result<()> {
    // ctx.cost 只在代价槽有效：在准入槽处理器里调用走宿主能力门
    // 故障，不能被 catch+decline 掩盖，整轮候选失败且世界不变。
    let source = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'wrong'},ctx=>{
        try{ctx.cost.setFatigue(ctx.args.movers[0].id,0)}catch(e){}
        return api.decline('hide');
      });",
    );
    let mut e = engine_with_swamp(&[("alpha", source)], false)?;
    let _ = scouts(&mut e)?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", WALK).is_err(), "能力门故障整轮失败");
    assert_eq!(e.world_snapshot(), before, "世界不变");
    Ok(())
}

#[test]
fn cost_fault_disable_falls_back_to_native() -> anyhow::Result<()> {
    // 处理器连续故障跨过阈值后整包停用，槽回退原生代价。
    let source = package(
        "api.handlers.replace('builtin:room.moveCost',{id:'faulty'},ctx=>{
        ctx.cost.setFatigue(ctx.args.movers[0].id,0);
        throw Error('cost fault');
      });",
    );
    let options = RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(2),
        ..Default::default()
    };
    let mut e = engine_with_shard(&[("alpha", source)], false, &[], options, &swamp_shard())?;
    let (scout, walker) = scouts(&mut e)?;
    for _ in 0..2 {
        assert!(e.run_player("p1", WALK).is_err(), "故障轮整轮失败");
        e.discard_pending_round();
        e.rebuild_mod_realm()?;
    }
    assert_eq!(
        e.disabled_mods(),
        ["alpha".to_string()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    for _ in 0..3 {
        e.run_player("p1", WALK)?;
    }
    assert_eq!(pos_in(&e, "W1N1", &scout), (20, 25), "停用后回退原生代价");
    assert_eq!(pos_in(&e, "W1N1", &walker), (8, 25));
    Ok(())
}
