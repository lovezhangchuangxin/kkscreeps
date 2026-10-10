//! Keeper 预意图、私有巡守记忆与真实候选/检查点边界。
use super::*;
use harness::{hits, pos_of};
use kk_core::constants as c;
use kk_world::domain::{keeper::OWNER, npc::INVADER_OWNER};

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1"],"rooms":[{"name":"W1N1"}]}))?;
    Ok(e)
}
fn creep(
    e: &mut EmbedEngine,
    name: &str,
    owner: &str,
    pos: [u8; 2],
    body: &[&str],
) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({"name":name,"owner":owner,"pos":pos,"body":body,"ticksToLive":1500}),
    )
}
/// 放置通道没有独立 source 类别；原生 Source 变体走 structure 通道
/// （与 screeps-ok placeSource 同形），kind="source" 会落入 Generic。
fn source(e: &mut EmbedEngine, pos: [u8; 2]) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":"source","pos":pos}),
    )
}
fn memory(e: &mut EmbedEngine) -> anyhow::Result<Value> {
    Ok(e.checkpoint(Value::Null)?["engine"]["world"]["rooms"]["W1N1"]["keeper_memory"].clone())
}
fn key(id: &str) -> String {
    kk_world::ObjId::parse(id).unwrap().0.to_string()
}

#[test]
fn keeper_patrol_remembers_first_resource_and_stops_adjacent() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let src = source(&mut e, [15, 10])?;
        let first_mineral =
            e.place_object("W1N1", "mineral", &json!({"pos":[10,12],"mineralType":"H"}))?;
        let id = creep(&mut e, "keeper", OWNER, [10, 10], &["move"])?;
        e.tick(1)?;
        let saved = memory(&mut e)?;
        assert_eq!(
            saved[key(&id)]["source_id"],
            kk_world::ObjId::parse(&src).unwrap().0
        );
        assert_ne!(pos_of(&e, &id), (10, 10));
        e.tick(8)?;
        let (x, y) = pos_of(&e, &id);
        assert_eq!((x - 15).abs().max((y - 10).abs()), 1);
        assert_eq!(
            memory(&mut e)?[key(&id)]["move_cache"],
            saved[key(&id)]["move_cache"],
            "缓存不逐拍裁剪"
        );
        let before = pos_of(&e, &id);
        e.tick(3)?;
        assert_eq!(pos_of(&e, &id), before);
        // p1 在房内无对象时整房不可见；放一个远处工人建立视野后验证
        // 玩家投影不泄漏 Keeper 私有记忆的真实字段名（body 无 ATTACK）。
        creep(&mut e, "worker", "p1", [40, 40], &["move", "work"])?;
        e.tick(1)?;
        let value = e.run_player("p1", &format!("const k=Game.getObjectById('{id}');const ks=Object.keys(k);[k!==null,!ks.includes('source_id'),!ks.includes('move_cache'),!ks.includes('keeper_memory')]"))?.expr["value"].clone();
        assert_eq!(value, json!([true, true, true, true]));
        // 目标删除后记忆失效重选：经真实检查点删掉记忆中的 source，
        // 下一拍重选同房 mineral（首个 mineral ID 最小、距离恰为 5）。
        let mut saved = e.checkpoint(Value::Null)?;
        let bytes = serde_json::from_value::<Vec<u8>>(
            saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"].clone(),
        )?;
        let mut room = kk_schema::room::decode_snapshot(&bytes)?;
        room.objects
            .retain(|o| o.id() != kk_world::ObjId::parse(&src).unwrap().0);
        saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"] =
            json!(kk_schema::room::encode_snapshot(&room));
        e.restore_checkpoint(saved, None)?;
        e.tick(1)?;
        assert_eq!(
            memory(&mut e)?[key(&id)]["source_id"],
            kk_world::ObjId::parse(&first_mineral).unwrap().0,
            "旧 ID 悬空后重选同房资源"
        );
        // 无 source 时 mineral 同样可以作为巡守目标；超距资源（距离 >5）
        // 不进入选择门，即使其 ID 更小（先放）。
        let mut mineral_only = fixture(binary)?;
        let far = mineral_only.place_object(
            "W1N1",
            "mineral",
            &json!({"pos":[10,45],"mineralType":"H"}),
        )?;
        let near = mineral_only.place_object(
            "W1N1",
            "mineral",
            &json!({"pos":[13,10],"mineralType":"H"}),
        )?;
        let actor = creep(&mut mineral_only, "keeper", OWNER, [10, 10], &["move"])?;
        mineral_only.tick(1)?;
        let chosen = memory(&mut mineral_only)?[key(&actor)]["source_id"].clone();
        assert_eq!(chosen, kk_world::ObjId::parse(&near).unwrap().0);
        assert_ne!(chosen, kk_world::ObjId::parse(&far).unwrap().0);
    }
    Ok(())
}

#[test]
fn keeper_attack_selects_lowest_hits_and_mass_threshold_is_strict() -> anyhow::Result<()> {
    for binary in [false, true] {
        for mass in [false, true] {
            let mut e = fixture(binary)?;
            creep(
                &mut e,
                "keeper",
                OWNER,
                [10, 10],
                &["move", "attack", "ranged_attack"],
            )?;
            let near = creep(&mut e, "near", "p1", [11, 10], &["move", "move"])?;
            let weak = creep(
                &mut e,
                "weak",
                "p1",
                if mass { [12, 10] } else { [13, 10] },
                &["move"],
            )?;
            if !mass {
                creep(&mut e, "far1", "p1", [13, 11], &["move", "move"])?;
                creep(&mut e, "far2", "p1", [13, 12], &["move", "move"])?;
            }
            // 10+4=14 使用范围攻击；10+1+1+1=13 仍是最低 hits 单体。
            e.tick(1)?;
            assert_eq!(hits(&e, &near), if mass { 160 } else { 170 });
            assert_eq!(hits(&e, &weak), if mass { 96 } else { 90 });
            let events = harness::events(&e, c::EVENT_ATTACK);
            assert_eq!(events.len(), if mass { 3 } else { 2 });
            assert!(events.iter().any(|ev| ev["data"]["attackType"]
                == if mass {
                    c::EVENT_ATTACK_TYPE_RANGED_MASS
                } else {
                    c::EVENT_ATTACK_TYPE_RANGED
                }));
        }
        // 两个近战目标也按最低 hits，NPC 同伴不参与敌方评分。friend 先放
        // 使其 ID 最小：若错误计入敌方，平手时 min 取 friend，hits 断言即挂。
        let mut e = fixture(binary)?;
        creep(&mut e, "keeper", OWNER, [10, 10], &["attack"])?;
        let friend = creep(&mut e, "friend", INVADER_OWNER, [9, 10], &["move"])?;
        let high = creep(&mut e, "high", "p1", [11, 10], &["move", "move"])?;
        let low = creep(&mut e, "low", "p1", [10, 11], &["move"])?;
        e.tick(1)?;
        assert_eq!(hits(&e, &high), 200);
        assert_eq!(hits(&e, &low), 70);
        assert_eq!(hits(&e, &friend), 100);
        let mut e = fixture(binary)?;
        creep(&mut e, "keeper", OWNER, [10, 10], &["ranged_attack"])?;
        let power = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"pos":[12,10],"owner":"p1","name":"power","ticksToLive":1000}),
        )?;
        let before = hits(&e, &power);
        e.tick(1)?;
        assert_eq!(hits(&e, &power), before - 10);
        // 手工检查点注入缺失 hits 的 powerCreep（正常对局不可达）：
        // 官方入桶不读 hits——仍参与 mass 估计（near 10 + pc 4 + far 1 =
        // 15 > 13 触发 mass），最低 hits 选择跳过它。
        let strip_hits = |e: &mut EmbedEngine, id: &str| {
            edit_object(e, id, |object| {
                if let kk_schema::room::RoomObject::Generic(g) = object {
                    let mut data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                    data.as_object_mut().unwrap().remove("hits");
                    g.data = data.to_string().into();
                }
            })
            .unwrap();
        };
        let mut e = fixture(binary)?;
        creep(&mut e, "keeper", OWNER, [10, 10], &["ranged_attack"])?;
        creep(&mut e, "near", "p1", [11, 10], &["move", "move"])?;
        let nohits = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"pos":[12,10],"owner":"p1","name":"pc2","ticksToLive":1000}),
        )?;
        let far = creep(&mut e, "far", "p1", [13, 10], &["move"])?;
        strip_hits(&mut e, &nohits);
        e.tick(1)?;
        assert_eq!(
            hits(&e, &far),
            99,
            "缺 hits 的 powerCreep 仍计入 mass（15>13，far 距离 3 受 10×0.1）"
        );
        // 敌桶全部缺失 hits 时官方 _.min 得 Infinity 发废意图（无效果），
        // kk 不发：无攻击事件（strip 后投影无 hits，用事件断言）。
        let mut e = fixture(binary)?;
        creep(
            &mut e,
            "keeper",
            OWNER,
            [10, 10],
            &["attack", "ranged_attack"],
        )?;
        let alone = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"pos":[10,11],"owner":"p1","name":"pc3","ticksToLive":1000}),
        )?;
        strip_hits(&mut e, &alone);
        e.tick(1)?;
        assert!(
            harness::events(&e, c::EVENT_ATTACK).is_empty(),
            "全缺 hits 时最低 hits 选择无目标，不发攻击"
        );
    }
    Ok(())
}

#[test]
fn keeper_memory_retry_and_cold_restore_preserve_future_patrol() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        source(&mut e, [15, 10])?;
        let id = creep(&mut e, "keeper", OWNER, [10, 10], &["move"])?;
        let saved = e.checkpoint(Value::Null)?;
        let mut reference = EmbedEngine::new();
        reference.set_kkpv_channel(binary);
        reference.restore_checkpoint(saved, None)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.runs=(Memory.runs||0)+1")
                .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert!(e.checkpoint(Value::Null).is_err());
        reference.run_player("p1", "Memory.runs=(Memory.runs||0)+1")?;
        e.run_player("p1", "throw Error('不得重跑')")?;
        // 重试轮的玩家 Memory 写回与直跑等值（两侧对称执行同一读取表达式）。
        assert_eq!(
            e.run_player("p1", "Memory.runs")?.expr["value"],
            reference.run_player("p1", "Memory.runs")?.expr["value"],
            "失败轮的 RawMemory 候选经重试轮发布"
        );
        assert_eq!(memory(&mut e)?, memory(&mut reference)?);
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        // 循环验证续跑确定性（keeper 已贴邻、无路径计算；缓存过期边界由
        // keeper_path_reuse 测试单独承担）。
        for ticks in [1, 3, 51] {
            e.tick(ticks)?;
            reference.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), reference.world_snapshot());
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
            assert_eq!(memory(&mut e)?, memory(&mut restored)?);
        }
        // 普通死亡清除记忆，墓碑仍走原回收链；from_json 的线格式是对象
        // ID 直接做键，套 "objects" 外壳会被静默丢弃。
        e.settle_external(kk_engine::intent::IntentBatch::from_json(
            OWNER,
            &json!({&id:{"suicide":{}}}),
        ))?;
        assert!(e.get_object(&id).is_none());
        assert!(memory(&mut e)?.get(key(&id)).is_none());
        assert_eq!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).len(), 1);
    }
    Ok(())
}

// 放置接口不提供 fatigue/字段剥除；通过真实检查点保存格式构造测试态。
fn edit_object<F>(e: &mut EmbedEngine, id: &str, edit: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut kk_schema::room::RoomObject),
{
    let mut saved = e.checkpoint(Value::Null)?;
    let bytes = serde_json::from_value::<Vec<u8>>(
        saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"].clone(),
    )?;
    let mut room = kk_schema::room::decode_snapshot(&bytes)?;
    let object = room
        .objects
        .iter_mut()
        .find(|o| o.id() == kk_world::ObjId::parse(id).unwrap().0)
        .unwrap();
    edit(object);
    saved["engine"]["world"]["rooms"]["W1N1"]["snapshot"] =
        json!(kk_schema::room::encode_snapshot(&room));
    e.restore_checkpoint(saved, None)?;
    Ok(())
}

fn fatigued(e: &mut EmbedEngine, id: &str) -> anyhow::Result<()> {
    edit_object(e, id, |object| {
        let kk_schema::room::RoomObject::Creep(actor) = object else {
            unreachable!()
        };
        actor.fatigue = 1000;
    })
}

#[test]
fn keeper_path_reuse_expires_only_after_fifty_ticks() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        source(&mut e, [15, 10])?;
        let id = creep(&mut e, "keeper", OWNER, [10, 10], &["move"])?;
        fatigued(&mut e, &id)?;
        e.tick(1)?;
        let original = memory(&mut e)?[key(&id)]["move_cache"].clone();
        e.tick(50)?;
        assert_eq!(pos_of(&e, &id), (10, 10));
        assert_eq!(memory(&mut e)?[key(&id)]["move_cache"], original);
        e.tick(1)?;
        assert_eq!(
            memory(&mut e)?[key(&id)]["move_cache"]["time"],
            original["time"].as_u64().unwrap() + 51
        );
    }
    Ok(())
}

#[test]
fn keeper_corrupt_memory_is_rejected_without_replacing_world() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        source(&mut e, [15, 10])?;
        let id = creep(&mut e, "keeper", OWNER, [10, 10], &["move"])?;
        e.tick(1)?;
        let saved = e.checkpoint(Value::Null)?;
        for defect in [
            "missing",
            "actor",
            "source",
            "destination",
            "step",
            "time",
            "unknown",
        ] {
            let mut bad = saved.clone();
            let room = &mut bad["engine"]["world"]["rooms"]["W1N1"];
            match defect {
                "missing" => {
                    room.as_object_mut().unwrap().remove("keeper_memory");
                }
                "actor" => {
                    let entry = room["keeper_memory"]
                        .as_object_mut()
                        .unwrap()
                        .remove(&key(&id))
                        .unwrap();
                    room["keeper_memory"]["0"] = entry;
                }
                "source" => room["keeper_memory"][key(&id)]["source_id"] = json!(0),
                "destination" => {
                    room["keeper_memory"][key(&id)]["move_cache"]["destination"] = json!([50, 10])
                }
                "step" => {
                    room["keeper_memory"][key(&id)]["move_cache"]["path"] =
                        json!([[11, 10], [14, 10]])
                }
                "time" => room["keeper_memory"][key(&id)]["move_cache"]["time"] = json!(u64::MAX),
                "unknown" => room["keeper_memory"][key(&id)]["unknown"] = json!(true),
                _ => unreachable!(),
            }
            assert!(e.restore_checkpoint(bad, None).is_err(), "{defect}");
            assert_eq!(e.checkpoint(Value::Null)?, saved, "{defect} 必须原子拒绝");
        }
        // time=0 是合法载荷（validate 只设上界；官方 !time 重算语义——
        // 正常对局 gameTime>=1 不可达，手工检查点可达）：恢复成功且
        // 下一拍缓存按当前时间重算。
        let mut zero = saved.clone();
        zero["engine"]["world"]["rooms"]["W1N1"]["keeper_memory"][key(&id)]["move_cache"]["time"] =
            json!(0);
        e.restore_checkpoint(zero, None)?;
        let next = saved["engine"]["world"]["time"].as_u64().unwrap();
        e.tick(1)?;
        assert_eq!(
            memory(&mut e)?[key(&id)]["move_cache"]["time"],
            json!(next),
            "time=0 触发缓存重算哨兵"
        );
    }
    Ok(())
}

#[test]
fn keeper_death_candidate_rollback_restores_private_memory_with_object() -> anyhow::Result<()> {
    // suicide 没有开放的 Mod 拒绝槽；对象保留场景用真实机制钉住：
    // TTL 死亡同轮玩家写回失败 → 整轮候选不发布（对象与私有记忆都不变），
    // 丢弃失败轮后记忆仍完整；下一轮死亡链才真正落盘。Mod 槽链内拒绝经
    // RemovedObject.keeper_memory 恢复的路径本片无测试触达（见台账登记）。
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        source(&mut e, [15, 10])?;
        // 寿命 2：tick(1) 后余 1，失败的那一轮玩家结算里恰好归零死亡。
        let id = e.place_object(
            "W1N1",
            "creep",
            &json!({
                "pos":[10,10],"name":"keeper","owner":OWNER,"body":["move"],"ticksToLive":2
            }),
        )?;
        e.tick(1)?;
        let world = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.runs=(Memory.runs||0)+1")
                .is_err()
        );
        assert_eq!(
            e.world_snapshot(),
            world,
            "死亡候选未落盘（world_snapshot 是玩家投影面，覆盖对象/事件回滚）"
        );
        assert!(e.get_object(&id).is_some());
        assert!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).is_empty());
        // 丢弃失败轮后检查点可读：记忆写入随候选一起回滚，source_id 保留。
        e.discard_pending_round();
        assert!(
            memory(&mut e)?[key(&id)]["source_id"].is_number(),
            "私有记忆未随对象回滚恢复"
        );
        // 丢弃后是全新一轮（非重放）：TTL=2 的 keeper 在该轮真正死亡。
        e.tick(1)?;
        assert!(e.get_object(&id).is_none());
        assert!(memory(&mut e)?.get(key(&id)).is_none());
        assert_eq!(e.find_in_room("W1N1", c::FIND_TOMBSTONES).len(), 1);
        assert_eq!(harness::events(&e, c::EVENT_OBJECT_DESTROYED).len(), 1);
    }
    Ok(())
}
