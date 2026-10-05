//! 移动竞争优先级链：输入完整性、组合、拒绝回退、能力门与故障恢复。
use super::*;

/// 三个不同身体的竞争者挤 (10,11)：mid(work,move)=1.0、light(move,move)=2.0、
/// heavy(work,work,move)=0.5；默认排序 light 胜。
fn trio(e: &mut EmbedEngine) -> anyhow::Result<()> {
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
    Ok(())
}

fn contest_code() -> &'static str {
    "Game.creeps.mid.move(BOTTOM);Game.creeps.light.move(LEFT);Game.creeps.heavy.move(TOP)"
}

fn pos_of(e: &EmbedEngine, name: &str) -> (u8, u8) {
    let id = e
        .find_in_room("W1N1", kk_core::constants::FIND_CREEPS)
        .into_iter()
        .find(|o| o["name"] == name)
        .expect("creep 在房间中");
    (
        id["pos"]["x"].as_u64().unwrap() as u8,
        id["pos"]["y"].as_u64().unwrap() as u8,
    )
}

#[test]
fn contest_input_is_complete_and_priority_reorders_winner() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.moveContest',{id:'heavy'},ctx=>{
        if(ctx.args.contests.length!==1)throw Error('竞争格数');
        const cell=ctx.args.contests[0];
        if(cell.x!==10||cell.y!==11)throw Error('目的格');
        if(cell.contenders.length!==3)throw Error('竞争者不得截断');
        let best=null,w=-1;
        for(const c of cell.contenders){
            if(!c.id||!c.owner||typeof c.powerRatio!=='number'||typeof c.weight!=='number'
                ||typeof c.movePower!=='number'||typeof c.originRequestCount!=='number'
                ||typeof c.originX!=='number'||typeof c.originY!=='number'
                ||typeof c.reciprocal!=='boolean'||typeof c.pulled!=='boolean'
                ||typeof c.pulling!=='boolean')throw Error('评分要素');
            if(c.weight>w){w=c.weight;best=c.id;}
        }
        for(const c of cell.contenders)ctx.contest.setPriority(c.id,c.id===best?1:-1);
        ctx.state={applied:(ctx.state?.applied||0)+1};
    });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        trio(&mut e)?;
        e.run_player("p1", contest_code())?;
        assert_eq!(pos_of(&e, "heavy"), (10, 11), "重量最大者反超默认胜者");
        assert_eq!(pos_of(&e, "light"), (11, 11), "失败者不移动");
        assert_eq!(pos_of(&e, "mid"), (10, 10));
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        // 无请求的 tick 不进入 JS，也不重复计数。
        e.run_player("p1", "Memory.tick=Game.time")?;
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
    }
    Ok(())
}

#[test]
fn contest_decline_falls_back_to_native_ordering() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.moveContest',{id:'pass'},ctx=>{
        ctx.contest.setPriority(ctx.args.contests[0].contenders[0].id,5);
        return api.decline('busy');
    });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    trio(&mut e)?;
    e.run_player("p1", contest_code())?;
    assert_eq!(pos_of(&e, "light"), (10, 11), "拒绝回退默认功率比排序");
    assert_eq!(
        e.engine_mod_state("alpha"),
        Value::Null,
        "拒绝丢弃暂存 state"
    );
    Ok(())
}

#[test]
fn contest_invalid_values_and_foreign_capabilities_cannot_be_hidden_by_decline()
-> anyhow::Result<()> {
    for call in [
        "ctx.contest.setPriority('kk0000000000000a',0.5)",
        "ctx.contest.setPriority('kk0000000000000a',NaN)",
        "ctx.contest.setPriority('kk0000000000000a',Infinity)",
        "ctx.contest.setPriority('kk0000000000000a',Number.MAX_SAFE_INTEGER+1)",
        "ctx.contest.setPriority('kk0000000000000a',-Number.MAX_SAFE_INTEGER-1)",
        "ctx.contest.setPriority('xx',1)",
        "ctx.contest.setPriority('kkffffffffffffffff',1)",
        "ctx.contest.setPriority(null,1)",
        "ctx.contest.setPriority(123,1)",
        "ctx.movement.request(3)",
        "ctx.resources.transfer('a','b','energy',0,0)",
        "ctx.incoming.setDamage(0)",
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:room.moveContest',{{id:'bad'}},ctx=>{{
            try{{{call}}}catch(e){{}}return api.decline('hide');
        }});"
        ));
        let mut e = engine(&[("bad", source)], false)?;
        trio(&mut e)?;
        let before = e.world_snapshot();
        assert!(e.run_player("p1", contest_code()).is_err(), "{call}");
        assert_eq!(e.world_snapshot(), before, "{call}");
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
    }
    Ok(())
}

#[test]
fn contest_abilities_are_unavailable_in_action_slots() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.before('builtin:creep.move',{id:'wrong'},ctx=>{
        try{ctx.contest.setPriority(ctx.target.id,1)}catch(e){}return api.decline('hide');
    });",
    );
    let mut e = engine(&[("bad", source)], false)?;
    trio(&mut e)?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", contest_code()).is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.engine_mod_state("bad"), Value::Null);
    Ok(())
}

#[test]
fn contest_negative_priority_stays_below_default_layer() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.moveContest',{id:'dwarf'},ctx=>{
        for(const cell of ctx.args.contests){
            for(const c of cell.contenders){
                if(c.powerRatio>1.5)ctx.contest.setPriority(c.id,-1);
            }
        }
    });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    trio(&mut e)?;
    e.run_player("p1", contest_code())?;
    // 默认胜者 light 被压到 -1；默认层剩 mid(1.0) 与 heavy(0.5)，mid 胜。
    assert_eq!(pos_of(&e, "mid"), (10, 11));
    assert_eq!(pos_of(&e, "light"), (11, 11));
    assert_eq!(pos_of(&e, "heavy"), (10, 12));
    Ok(())
}

#[test]
fn contest_fault_disable_falls_back_to_native_next_tick() -> anyhow::Result<()> {
    let bad = package(
        "api.handlers.replace('builtin:room.moveContest',{id:'fault'},ctx=>{
        ctx.state={dirty:true};throw Error('fault');
    });",
    );
    let healthy = package(
        "api.handlers.after('builtin:room.moveContest',{id:'audit'},ctx=>{
        ctx.state={calls:(ctx.state?.calls||0)+1};
    });",
    );
    let mut e = engine(&[("bad", bad), ("healthy", healthy)], false)?;
    trio(&mut e)?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", contest_code()).is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.engine_mod_state("healthy"), Value::Null);
    e.disable_mod("bad")?;
    // 停用后健康钩子与 native 默认继续：light 胜、audit 恰计一次。
    e.run_player("p1", contest_code())?;
    assert_eq!(pos_of(&e, "light"), (10, 11));
    assert_eq!(e.engine_mod_state("healthy"), json!({"calls":1}));
    assert_eq!(e.engine_mod_state("bad"), Value::Null);
    Ok(())
}

#[test]
fn contest_state_commits_once_after_late_retry() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.moveContest',{id:'heavy'},ctx=>{
        let best=null,w=-1;
        for(const cell of ctx.args.contests)for(const c of cell.contenders){
            if(c.weight>w){w=c.weight;best=c.id;}
        }
        for(const cell of ctx.args.contests)for(const c of cell.contenders){
            ctx.contest.setPriority(c.id,c.id===best?1:-1);
        }
        ctx.state={applied:(ctx.state?.applied||0)+1};
    });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        trio(&mut e)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let code = format!("Memory.runs=1;{}", contest_code());
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        // 玩家缓存重试：不重复执行玩家代码，竞争与计数恰一次提交。
        e.run_player("p1", "throw Error('cached only')")?;
        assert_eq!(pos_of(&e, "heavy"), (10, 11));
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}
