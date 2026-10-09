//! invaderCore 的部署/孵化/坍塌生命周期、NPC 预定与归属面。
use super::*;
use kk_core::constants as c;
use kk_world::domain::npc::INVADER_OWNER;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1"],"rooms":[
        {"name":"W1N1","owner":"p1","rcl":2},{"name":"W2N1"}]}))?;
    Ok(e)
}
fn core(e: &mut EmbedEngine, room: &str, spec: Value) -> anyhow::Result<String> {
    e.place_object(room, "invaderCore", &spec)
}
fn controller_snapshot(e: &EmbedEngine, room: &str) -> Value {
    e.find_in_room(room, c::FIND_STRUCTURES)
        .into_iter()
        .find(|s| s["structureType"] == "controller")
        .expect("房间必有控制器")
}

#[test]
fn invader_core_level_countdown_and_ownership_projection() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = core(
            &mut e,
            "W1N1",
            json!({"pos":[25,25],"level":3,"ticksToDeploy":20}),
        )?;
        // 管理快照折算相对键；玩家面经类 getter 折算绝对键，两通道同拍等值。
        assert_eq!(e.get_object(&id).unwrap()["ticksToDeploy"], 20);
        let code = format!(
            "const s=Game.getObjectById('{id}');[s.level,s.ticksToDeploy,s.my,s.owner.username,s instanceof StructureInvaderCore,Game.rooms.W1N1.find(FIND_STRUCTURES).some(x=>x.id===s.id),Game.rooms.W1N1.find(FIND_HOSTILE_STRUCTURES).some(x=>x.id===s.id),Game.structures[s.id]===undefined]"
        );
        assert_eq!(
            e.run_player("p1", &code)?.expr["value"],
            json!([3, 20, false, INVADER_OWNER, true, true, true, true])
        );
        assert_eq!(e.run_player("p1", &code)?.expr["value"][1], 19);
        // 无部署期时官方读 undefined，快照面为 null。
        let bare = core(&mut e, "W1N1", json!({"pos":[27,27],"level":0}))?;
        assert_eq!(
            e.run_player(
                "p1",
                &format!("Game.getObjectById('{bare}').ticksToDeploy===undefined")
            )?
            .expr["value"],
            true
        );
        assert_eq!(e.get_object(&bare).unwrap()["ticksToDeploy"], Value::Null);
    }
    Ok(())
}

#[test]
fn invader_core_spawning_incubates_and_borns_on_adjacent_tile() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = core(
            &mut e,
            "W1N1",
            json!({"pos":[25,25],"level":2,
                   "spawning":{"name":"defender1","body":["attack","move"],"needTime":12,"remainingTime":6}}),
        )?;
        // 孵化期公开 spawning 状态（官方复用 Spawning 形态）。
        assert_eq!(
            e.run_player(
                "p1",
                &format!("const s=Game.getObjectById('{id}');s.spawning?[s.spawning.name,s.spawning.needTime,s.spawning.remainingTime>0]:null")
            )?
            .expr["value"],
            json!(["defender1", 12, true])
        );
        let mut born: Option<Value> = None;
        let mut polls = 0;
        while born.is_none() && polls < 10 {
            polls += 1;
            let value = e
                .run_player(
                    "p1",
                    &format!("const s=Game.getObjectById('{id}');const c=Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS).find(x=>x.name==='defender1'&&!x.spawning);c?[c.pos.x,c.pos.y,!!s.spawning,c.owner.username,c.body.length]:null"),
                )?
                .expr["value"]
                .clone();
            born = if value.is_null() { None } else { Some(value) };
        }
        let born = born.expect("孵化应在到期拍出生");
        let (x, y) = (born[0].as_i64().unwrap(), born[1].as_i64().unwrap());
        let range = (x - 25).abs().max((y - 25).abs());
        assert_eq!(range, 1, "出生在核心邻格");
        assert_eq!(born[2], false, "出生同拍清核心孵化状态");
        assert_eq!(born[3], INVADER_OWNER);
        assert_eq!(born[4], 2);
    }
    Ok(())
}

#[test]
fn invader_core_collapse_clears_controller_and_removes_core_without_ruin() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = core(
            &mut e,
            "W1N1",
            json!({"pos":[30,30],"level":0,"ticksToCollapse":6}),
        )?;
        // 坍塌倒计时以 EFFECT_COLLAPSE_TIMER 暴露给玩家。
        assert_eq!(
            e.run_player(
                "p1",
                &format!("const s=Game.getObjectById('{id}');const e=((s&&s.effects)||[]).find(x=>x.effect===1002);e?e.ticksRemaining:null")
            )?
            .expr["value"],
            json!(6)
        );
        // 预挂控制器效果条目：官方重置清单含 effects:null，坍塌后须随清空。
        let ctl_id = e
            .find_in_room("W1N1", c::FIND_STRUCTURES)
            .into_iter()
            .find(|s| s["structureType"] == "controller")
            .expect("控制器存在")["id"]
            .as_str()
            .expect("控制器有 id")
            .to_owned();
        let mut checkpoint = e.checkpoint(Value::Null)?;
        checkpoint["engine"]["world"]["rooms"]["W1N1"]["effects"]
            [kk_world::ObjId::parse(&ctl_id).unwrap().0.to_string()] =
            json!([{"effect":1001,"power":1001,"level":1,"end_time":e.get_game_time()+100}]);
        e.restore_checkpoint(checkpoint, None)?;
        e.tick(8)?;
        // 控制器清空（官方重置清单；rcl2 有主控制器的降级钟在 8 tick 后
        // 本应非空，重置后须为 null）＋核心静默移除、无 ruin。
        let ctl = controller_snapshot(&e, "W1N1");
        assert_eq!(ctl["owner"], Value::Null);
        assert_eq!(ctl["level"], 0);
        assert_eq!(ctl["progress"], Value::Null);
        assert_eq!(ctl["isPowerEnabled"], false);
        assert_eq!(ctl["safeMode"], Value::Null);
        assert_eq!(
            ctl["ticksToDowngrade"],
            Value::Null,
            "downgradeTime 随重置清空"
        );
        // 官方清单不含 sign/upgradeBlocked/reservation，kk 不触碰
        //（控制器结构化字段在二进制快照内、夹具不可注入，经代码复审锁定）；
        // 效果条目按 effects:null 随清。
        let after = e.checkpoint(Value::Null)?;
        assert!(
            after["engine"]["world"]["rooms"]["W1N1"]["effects"]
                .as_object()
                .is_none_or(|m| !m.contains_key(&ctl_id)),
            "控制器效果条目随坍塌清空（官方 effects:null）"
        );
        assert!(e.get_object(&id).is_none());
        let ruins = e.find_in_room("W1N1", c::FIND_RUINS);
        assert!(
            ruins
                .iter()
                .all(|r| r["pos"]["x"] != 30 || r["pos"]["y"] != 30),
            "坍塌移除不落 ruin"
        );
    }
    Ok(())
}

#[test]
fn invader_core_collapse_survives_cold_restore() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = core(
            &mut e,
            "W1N1",
            json!({"pos":[30,30],"level":0,"ticksToCollapse":6}),
        )?;
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        e.tick(8)?;
        restored.tick(8)?;
        // 冷恢复后效果表与核心数据同源恢复：坍塌照常清控制器并移除核心。
        assert_eq!(e.world_snapshot(), restored.world_snapshot());
        assert!(e.get_object(&id).is_none());
        assert_eq!(
            controller_snapshot(&e, "W1N1"),
            controller_snapshot(&restored, "W1N1")
        );
        // 到期前保存、恢复后不提前坍塌：核心与坍塌效果保留。
        let mut early = fixture(binary)?;
        let id2 = core(
            &mut early,
            "W1N1",
            json!({"pos":[30,30],"level":0,"ticksToCollapse":50}),
        )?;
        let saved2 = early.checkpoint(Value::Null)?;
        let mut early2 = EmbedEngine::new();
        early2.set_kkpv_channel(binary);
        early2.restore_checkpoint(saved2, None)?;
        early.tick(3)?;
        early2.tick(3)?;
        assert_eq!(early.world_snapshot(), early2.world_snapshot());
        assert!(early.get_object(&id2).is_some(), "未到期不坍塌");
        assert_eq!(
            early.run_player(
                "p1",
                &format!("Game.getObjectById('{id2}').effects.find(x=>x.effect===1002).ticksRemaining")
            )?
            .expr["value"],
            47
        );
    }
    Ok(())
}

#[test]
fn invader_core_reserves_neutral_controller_with_official_increment() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        core(&mut e, "W2N1", json!({"pos":[25,25],"level":0}))?;
        // p1 creep 提供中立房视野。
        e.place_object(
            "W2N1",
            "creep",
            &json!({"name":"scout","owner":"p1","pos":[10,10],"body":["move"],"ticksToLive":500}),
        )?;
        let code = "const r=Game.rooms['W2N1'].controller.reservation;r?r.ticksToEnd:null";
        let mut readings: Vec<i64> = Vec::new();
        for _ in 0..6 {
            if readings.len() >= 2 {
                break;
            }
            if let Some(v) = e.run_player("p1", code)?.expr["value"].as_i64() {
                readings.push(v);
            }
        }
        // 官方 INVADER-CORE-006 序列：首读恰为 power，其后 power+power-1。
        let power = c::INVADER_CORE_CONTROLLER_POWER * c::CONTROLLER_RESERVE;
        assert_eq!(readings, vec![power, power + power - 1]);
        // 预定归属为核心主人；本片不发射他人预定/有主控制器的分支意图。
        assert_eq!(
            e.run_player("p1", "Game.rooms['W2N1'].controller.reservation.username")?
                .expr["value"],
            INVADER_OWNER
        );
    }
    Ok(())
}

#[test]
fn invader_core_retry_replay_and_cold_restore_keep_world_equal() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = core(
            &mut e,
            "W1N1",
            json!({"pos":[25,25],"level":1,
                   "spawning":{"name":"guard","body":["attack"],"remainingTime":3}}),
        )?;
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        // 探针同时观测孵化状态与出生帧：裸放置核心的驻防 creep 无
        // strongholdId，按官方普通 Invader 语义在无目标房间随后自杀退场，
        // 出生帧必须在当拍捕获。
        let probe = format!(
            "Memory.calls=(Memory.calls||0)+1;const s=Game.getObjectById('{id}');[s&&s.spawning?1:0,Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS).some(x=>x.name==='guard'&&!x.spawning)]"
        );
        // 晚期写回失败整轮回滚：世界停在孵化期，重试轮重放缓存输入
        //（throw 探针不执行，返回缓存轮的表达式值）。
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", &probe).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(
            e.run_player("p1", "throw Error('cached input expected')")?
                .expr["value"],
            json!([1, false])
        );
        // 冷恢复引擎先跑一拍对齐直跑引擎的重放轮，此后双引擎逐拍等值。
        assert_eq!(
            restored.run_player("p1", &probe)?.expr["value"],
            json!([1, false])
        );
        let mut saw_born = false;
        for _ in 0..4 {
            let value = e.run_player("p1", &probe)?.expr["value"].clone();
            assert_eq!(
                value,
                restored.run_player("p1", &probe)?.expr["value"],
                "双引擎逐拍表达式等值"
            );
            saw_born |= value.as_array().is_some_and(|a| a[1] == true);
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert!(saw_born, "四拍内捕获出生帧（remainingTime=3）");
        assert_eq!(
            serde_json::from_str::<Value>(&e.player_state("p1").unwrap().raw_memory)?["calls"],
            // 失败轮整轮回滚不计，重放轮提交计 1，随后四拍各执行一次。
            5
        );
    }
    Ok(())
}

#[test]
fn invader_core_invalid_placement_is_atomic() -> anyhow::Result<()> {
    let mut e = fixture(false)?;
    for bad in [
        json!({"pos":[25,25],"level":6}),
        json!({"pos":[25,25],"level":1,"ticksToDeploy":9_007_199_254_740_991u64}),
        json!({"pos":[25,25],"level":1,"spawning":{"name":"x","remainingTime":-1}}),
        json!({"pos":[25,25],"level":1,"spawning":{"name":"x"}}),
        json!({"pos":[60,60],"level":1}),
    ] {
        assert!(
            core(&mut e, "W1N1", bad.clone()).is_err(),
            "非法 spec {bad}"
        );
    }
    // 批量放置后项失败不发布前缀。
    let valid_creep = json!({"name":"worker","owner":"p1","pos":[5,5],"body":["move"]});
    assert!(
        e.place_objects(&[
            ("W1N1", "creep", &valid_creep),
            ("W1N1", "invaderCore", &json!({"pos":[25,25],"level":9})),
        ])
        .is_err()
    );
    assert!(
        e.find_in_room("W1N1", c::FIND_CREEPS)
            .iter()
            .all(|cr| cr["name"] != "worker"),
        "批量失败不发布前缀"
    );
    // 玩家名不能冒充核心归属（放置强制 NPC 归属）。
    let id = core(
        &mut e,
        "W1N1",
        json!({"pos":[25,25],"level":1,"owner":"p1"}),
    )?;
    assert_eq!(
        e.run_player(
            "p1",
            &format!("const s=Game.getObjectById('{id}');[s.my,s.owner.username]")
        )?
        .expr["value"],
        json!([false, INVADER_OWNER])
    );
    Ok(())
}
