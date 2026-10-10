//! deposit 采集、累计冷却、寿命刷新与到期的真实玩家链。
use super::*;
use kk_core::constants as c;

/// 官方常量：HARVEST_DEPOSIT_POWER=1（含 harvest 类 boost）、
/// DEPOSIT_EXHAUST_MULTIPLY=0.001 / POW=1.2、DEPOSIT_DECAY_TIME=50000。
/// 寿命门为 processor.js 移除相的 `time >= decayTime-1`（早于
/// deposits/tick.js 的严格 >，两门同拍时前者先生效）。
fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1"}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":6}]}))?;
    Ok(e)
}

fn deposit(e: &mut EmbedEngine, spec: Value) -> anyhow::Result<String> {
    e.place_object("W1N1", "deposit", &spec)
}

fn miner(
    e: &mut EmbedEngine,
    name: &str,
    works: usize,
    carries: usize,
    boosts: Option<Value>,
) -> anyhow::Result<String> {
    let mut body: Vec<&str> = vec!["work"; works];
    body.extend(vec!["carry"; carries]);
    // [25,24] 与 deposit [25,25] 邻接——移动此坐标会破坏采集前提。
    let mut spec = json!({"name":name,"owner":"p1","pos":[25,24],
        "body":body,"ticksToLive":1500});
    if let Some(b) = boosts {
        spec["boosts"] = b;
    }
    e.place_object("W1N1", "creep", &spec)
}

fn find_deposit() -> &'static str {
    "Game.rooms.W1N1.find(FIND_DEPOSITS).filter(d=>d.pos.x===25&&d.pos.y===25)"
}

/// 采集入仓、harvested 累计与冷却公式、寿命刷新、冷却期 ERR_TIRED 与
/// 结算静默、同 tick 双 creep 均成功（冷却经对象 tick 相落成，同轮
/// 后续采集读旧 cooldownTime）；不发 EVENT_HARVEST。
#[test]
fn deposit_harvest_cooldown_accumulation() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        // harvested 预置 1000：双次采集后冷却 ceil(0.001×1012^1.2)=5。
        let dep_id = deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"mist","harvested":1000,"decayTime":100}),
        )?;
        miner(&mut e, "m1", 6, 6, None)?;
        miner(&mut e, "m2", 6, 6, None)?;
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"far","owner":"p1","pos":[25,20],
                "body":["work","move"],"ticksToLive":1500}),
        )?;
        let dep = find_deposit();

        // 预检距离门：不邻接返回 ERR_NOT_IN_RANGE（官方先距离后冷却）。
        let rc = e.run_player("p1", &format!("Game.creeps.far.harvest({dep}[0])"))?;
        assert_eq!(rc.expr["value"], c::ERR_NOT_IN_RANGE);

        // 同 tick 双 creep：均成功（官方意图相读旧 cooldownTime）。
        let rc = e.run_player(
            "p1",
            &format!("[Game.creeps.m1.harvest({dep}[0]),Game.creeps.m2.harvest({dep}[0])]"),
        )?;
        assert_eq!(rc.expr["value"][0], 0);
        assert_eq!(rc.expr["value"][1], 0, "同轮第二采集仍成功");
        assert_eq!(
            e.get_object(&dep_id).unwrap()["harvested"],
            1012,
            "1000+6+6"
        );
        // 事件断言须紧随产生它的轮：事件 1-tick 生命期，读字段轮会清空。
        assert!(
            harness::events(&e, c::EVENT_HARVEST).is_empty(),
            "官方 deposit 采集不发 EVENT_HARVEST"
        );
        let r = e.run_player(
            "p1",
            &format!(
                "[{dep}[0].cooldown,{dep}[0].lastCooldown,\
                 {dep}[0].ticksToDecay,{dep}[0].depositType,\
                 Game.creeps.m1.store.mist,Game.creeps.m2.store.mist]"
            ),
        )?;
        assert_eq!(r.expr["value"][0], 4, "施放后下一拍，冷却剩余 4");
        assert_eq!(r.expr["value"][1], 5, "ceil(0.001×1012^1.2)");
        assert_eq!(r.expr["value"][2], 49999, "采集后下一拍，寿命剩余 49999");
        assert_eq!(r.expr["value"][3], "mist");
        assert_eq!(r.expr["value"][4], 6);
        assert_eq!(r.expr["value"][5], 6);

        // 冷却期：预检 ERR_TIRED、结算静默。
        let rc = e.run_player("p1", &format!("Game.creeps.m1.harvest({dep}[0])"))?;
        assert_eq!(rc.expr["value"], c::ERR_TIRED);
        assert_eq!(
            e.get_object(&dep_id).unwrap()["harvested"],
            1012,
            "冷却期不再累计"
        );
        // 到期拍两侧钉边：截止前一拍 cooldown=1（严格 > 仍拒），截止拍归 0 恢复。
        e.tick(1)?; // 再推进一拍冷却
        let rc = e.run_player("p1", &format!("Game.creeps.m1.harvest({dep}[0])"))?;
        assert_eq!(rc.expr["value"], c::ERR_TIRED, "截止前一拍最后一次拒绝");
        let rc = e.run_player("p1", &format!("Game.creeps.m1.harvest({dep}[0])"))?;
        assert_eq!(rc.expr["value"], 0, "截止拍即恢复");
        assert_eq!(e.get_object(&dep_id).unwrap()["harvested"], 1018);
    }
    Ok(())
}

/// 低累计不触发冷却（ceil ≤1 不挂）；XUHO2 强化按 harvest 倍率放大
/// 采集量；超容掉落地面资源堆。
#[test]
fn deposit_harvest_boost_and_drop() -> anyhow::Result<()> {
    for binary in [false, true] {
        // harvested 0：冷却 ceil(0.001×42^1.2)=1，不 >1 → 不挂冷却。
        let mut e = fixture(binary)?;
        let dep_id = deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"metal","harvested":0,"decayTime":50000}),
        )?;
        let mut boosts = serde_json::Map::new();
        for i in 0..6 {
            boosts.insert(i.to_string(), json!("XUHO2"));
        }
        miner(&mut e, "b1", 6, 6, Some(Value::Object(boosts)))?;
        let dep = find_deposit();
        e.run_player("p1", &format!("Game.creeps.b1.harvest({dep}[0])"))?;
        assert_eq!(
            e.get_object(&dep_id).unwrap()["harvested"],
            42,
            "6×1×7（XUHO2 harvest=7）"
        );
        let r = e.run_player(
            "p1",
            &format!("[{dep}[0].cooldown,Game.creeps.b1.store.metal]"),
        )?;
        assert_eq!(r.expr["value"][0], 0, "ceil(0.001×42^1.2)=1 不挂");
        assert_eq!(r.expr["value"][1], 42);

        // 掉落：预填 energy 45、单 carry（容量 50），10 work 采 10 超容 5。
        let mut e = fixture(binary)?;
        deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"silicon","harvested":0,"decayTime":50000}),
        )?;
        let mut body: Vec<&str> = vec!["work"; 10];
        body.push("carry");
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"full","owner":"p1","pos":[25,24],
                "body":body,"store":{"energy":45},"ticksToLive":1500}),
        )?;
        e.run_player("p1", &format!("Game.creeps.full.harvest({dep}[0])"))?;
        let r = e.run_player(
            "p1",
            "[Game.creeps.full.store.energy,Game.creeps.full.store.silicon||0,\
             Game.rooms.W1N1.find(FIND_DROPPED_RESOURCES)\
             .filter(r=>r.resourceType===RESOURCE_SILICON&&r.amount===4).length]",
        )?;
        assert_eq!(r.expr["value"][0], 45, "energy 不受影响");
        assert_eq!(r.expr["value"][1], 5, "入库钳到容量");
        // 掉落 5 当拍经资源 tick 衰减 ceil(5/1000)=1，读时为 4。
        assert_eq!(r.expr["value"][2], 1, "超容 5 掉落后余 4");

        // 极值：仓已满（energy 50 = 单 carry 容量），采集 10 全量掉落。
        let mut e = fixture(binary)?;
        deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"silicon","harvested":0,"decayTime":50000}),
        )?;
        let mut body: Vec<&str> = vec!["work"; 10];
        body.push("carry");
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"full2","owner":"p1","pos":[25,24],
                "body":body,"store":{"energy":50},"ticksToLive":1500}),
        )?;
        e.run_player("p1", &format!("Game.creeps.full2.harvest({dep}[0])"))?;
        let r = e.run_player(
            "p1",
            "[Game.creeps.full2.store.energy,Game.creeps.full2.store.silicon||0,\
             Game.rooms.W1N1.find(FIND_DROPPED_RESOURCES)\
             .filter(r=>r.resourceType===RESOURCE_SILICON&&r.amount===9).length]",
        )?;
        assert_eq!(r.expr["value"][0], 50, "满仓 energy 不动");
        assert_eq!(r.expr["value"][1], 0, "超容全量掉落");
        // 掉落 10 当拍衰减 ceil(10/1000)=1，读时为 9。
        assert_eq!(r.expr["value"][2], 1);
    }
    Ok(())
}

/// 寿命：无采集时 `time >= decayTime-1` 移除（绝对时间钉住边界）；
/// 采集刷新寿命使其存活过原 decayTime。
#[test]
fn deposit_decay_and_refresh() -> anyhow::Result<()> {
    for binary in [false, true] {
        // decayTime 10：t9 的结算移除，首个读到消失的世界时间为 10。
        let mut e = fixture(binary)?;
        deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"biomass","harvested":0,"decayTime":10}),
        )?;
        // 缺 decayTime 的 deposit：ticksToDecay 为 undefined、寿命门不触发。
        e.place_object(
            "W1N1",
            "deposit",
            &json!({"pos":[26,25],"depositType":"mist","harvested":0}),
        )?;
        let dep = find_deposit();
        let r = e.run_player(
            "p1",
            "[Game.rooms.W1N1.find(FIND_DEPOSITS).map(d=>\
             [d.lastCooldown,d.ticksToDecay===undefined])]",
        )?;
        assert_eq!(
            r.expr["value"][0],
            json!([[0, false], [0, true]]),
            "harvested 0 → lastCooldown 0；缺 decayTime → ticksToDecay undefined"
        );
        let mut gone_at: Option<u64> = None;
        let base = e.get_game_time();
        for i in 0..30u64 {
            let r = e.run_player("p1", &format!("{dep}.length"))?;
            if r.expr["value"] == 0 {
                gone_at = Some(base + i);
                break;
            }
        }
        assert_eq!(gone_at, Some(10), "t9 结算移除（>= decayTime-1）");
        let r = e.run_player(
            "p1",
            "Game.rooms.W1N1.find(FIND_DEPOSITS).filter(d=>d.pos.x===26).length",
        )?;
        assert_eq!(r.expr["value"], 1, "缺 decayTime 永不移除");

        // 采集刷新后的寿命为当前时间 +50000，远超原 decayTime 10。
        let mut e = fixture(binary)?;
        deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"biomass","harvested":0,"decayTime":10}),
        )?;
        miner(&mut e, "m1", 2, 2, None)?;
        e.run_player("p1", &format!("Game.creeps.m1.harvest({dep}[0])"))?;
        e.tick(20)?; // 远超原 decayTime 10
        let r = e.run_player("p1", &format!("{dep}.length"))?;
        assert_eq!(r.expr["value"], 1, "采集刷新寿命");
    }
    Ok(())
}

/// 采集轮写回失败缓存整轮候选；重试原子提交；检查点冷恢复与继续推进
/// 逐拍一致。
#[test]
fn deposit_retry_and_cold_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let dep_id = deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"mist","harvested":1000,"decayTime":50000}),
        )?;
        miner(&mut e, "m1", 6, 6, None)?;
        let dep = find_deposit();
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!("Memory.runs=(Memory.runs||0)+1;Game.creeps.m1.harvest({dep}[0])")
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(
            e.get_object(&dep_id).unwrap()["harvested"],
            1006,
            "重试轮已结算"
        );
        let r = e.run_player(
            "p1",
            &format!("[{dep}[0].cooldown,Game.creeps.m1.store.mist]"),
        )?;
        // 重试发布缓存候选（throw 代码不执行），随后下一拍读取剩余冷却。
        assert_eq!(r.expr["value"][0], 4, "原冷却 5，下一拍读取剩余 4");
        assert_eq!(r.expr["value"][1], 6);
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        for ticks in [1, 5, 20] {
            e.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}

/// 管理放置错误保留世界和 ID 游标，随后合法请求仍可运行。
#[test]
fn deposit_invalid_placement_is_atomic_and_restore_revalidates_numbers() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let before = e.checkpoint(Value::Null)?;
        for key in [
            "cooldown",
            "ticksToDecay",
            "cooldownTime",
            "decayTime",
            "harvested",
        ] {
            let mut spec = json!({"pos":[25,25],"depositType":"mist"});
            spec[key] = json!(u64::MAX);
            assert!(deposit(&mut e, spec).is_err());
            assert_eq!(e.checkpoint(Value::Null)?, before, "{key} 不修改世界或游标");
        }
        let id = deposit(
            &mut e,
            json!({"pos":[25,25],"depositType":"mist",
            "cooldown":2,"ticksToDecay":100,"harvested":0}),
        )?;
        e.tick(1)?;
        assert_eq!(e.get_object(&id).unwrap()["cooldown"], 1);
        // 合法检查点冷恢复成功；损坏的同版本载荷在替代分片校验时拒绝。
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved.clone(), None)?;
        assert_eq!(restored.checkpoint(Value::Null)?, saved);
        for key in ["cooldownTime", "decayTime", "harvested"] {
            let mut bad = saved.clone();
            let bytes: Vec<u8> = serde_json::from_value(
                bad["engine"]["world"]["rooms"]["W1N1"]["snapshot"].clone(),
            )?;
            let mut snapshot = kk_schema::room::decode_snapshot(&bytes)?;
            let object = snapshot
                .objects
                .iter_mut()
                .find(|o| o.id() == kk_world::ObjId::parse(&id).unwrap().0)
                .unwrap();
            let kk_schema::room::RoomObject::Generic(g) = object else {
                panic!()
            };
            let mut data: Value = serde_json::from_str(&g.data)?;
            data[key] = json!(u64::MAX);
            g.data = data.to_string().into();
            bad["engine"]["world"]["rooms"]["W1N1"]["snapshot"] =
                json!(kk_schema::room::encode_snapshot(&snapshot));
            assert!(restored.restore_checkpoint(bad, None).is_err());
            assert_eq!(
                restored.checkpoint(Value::Null)?,
                saved,
                "{key} 恢复失败保留现行分片"
            );
        }
    }
    Ok(())
}
