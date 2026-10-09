//! observer 的 observeRoom 限时视野、预检门序与 OPERATE_OBSERVER 无限射程。
use super::*;
use kk_core::constants as c;

/// POWER_INFO[7]：cooldown 400 / duration [200,400,600,800,1000] /
/// range 3 / ops 10，无 effect 数组（消费者只看效果存在性）。
/// 房间用途：W1N1 主房（rcl8 放 observer/pc）；W2N1 无对象目标房
/// （可见性用例）；W12N1 差 11 超距房（射程/power 豁免用例）。
fn fixture(binary: bool) -> anyhow::Result<(EmbedEngine, String)> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(
        &json!({"players":[{"name":"p1","power":100000},{"name":"p2"}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true},
                 {"name":"W2N1"},{"name":"W12N1"}]}),
    )?;
    let obs = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[25,25],"structureType":"observer","owner":"p1"}),
    )?;
    Ok((e, obs))
}

fn observe(obs: &str, room: &str) -> String {
    format!("Game.getObjectById('{obs}').observeRoom('{room}')")
}

/// 官方生命周期：观察轮 OK 且当拍不可见 → 下一拍可见（含对象导出）→
/// 无重复意图的再下一拍清零（恰 1 拍限时视野）。观察未注册房（房名
/// 合法、射程内）OK 但不产生可见房（官方 data.js 查询自然落空）。
#[test]
fn observer_observe_next_tick_visibility_and_expiry() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, obs) = fixture(binary)?;
        let r = e.run_player(
            "p1",
            &format!("[{},!!Game.rooms.W2N1]", observe(&obs, "W2N1")),
        )?;
        assert_eq!(r.expr["value"][0], 0, "观察返回 OK");
        assert_eq!(r.expr["value"][1], false, "当拍不可见");
        let r = e.run_player(
            "p1",
            "[!!Game.rooms.W2N1,Game.rooms.W2N1.controller instanceof StructureController]",
        )?;
        assert_eq!(r.expr["value"][0], true, "下一拍可见");
        assert_eq!(r.expr["value"][1], true, "被观察房的对象（控制器）导出");
        // 观察视野按玩家隔离：p2 不因 p1 的观察获得可见性。
        let r = e.run_player("p2", "!!Game.rooms.W2N1")?;
        assert_eq!(r.expr["value"], false, "他人观察不授视野");
        let r = e.run_player("p1", "!!Game.rooms.W2N1")?;
        assert_eq!(r.expr["value"], false, "无重复意图，再下一拍清零");

        // 未注册房：门全过（房名合法 + 射程内）但查询落空。
        let r = e.run_player("p1", &observe(&obs, "W11N1"))?;
        assert_eq!(r.expr["value"], 0, "观察未注册房同样 OK");
        let r = e.run_player("p1", "!!Game.rooms.W11N1")?;
        assert_eq!(r.expr["value"], false, "未注册房不进 Game.rooms");
    }
    Ok(())
}

/// 预检门序与单项：NOT_OWNER → INVALID_ARGS → RCL_NOT_ENOUGH →
/// NOT_IN_RANGE（逐轴绝对差 > OBSERVER_RANGE=10，无效果时）。
#[test]
fn observer_precheck_matrix() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, obs) = fixture(binary)?;
        let r = e.run_player("p1", &observe(&obs, "W12N1"))?;
        assert_eq!(
            r.expr["value"],
            c::ERR_NOT_IN_RANGE,
            "差 11 超 OBSERVER_RANGE"
        );
        let r = e.run_player("p1", &observe(&obs, "not_a_room"))?;
        assert_eq!(r.expr["value"], c::ERR_INVALID_ARGS);
        let r = e.run_player("p1", &observe(&obs, "E20S3"))?;
        assert_eq!(
            r.expr["value"],
            c::ERR_NOT_IN_RANGE,
            "任意合法远房名同样超距"
        );

        // 他人 observer：放在 p1 可见房内（对象可引用），!my 先于其余门
        // （owner 与房主不一致的 RCL 后果轮不到检验）。
        let (mut e, _obs) = fixture(binary)?;
        let obs2 = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[26,25],"structureType":"observer","owner":"p2"}),
        )?;
        let r = e.run_player("p1", &observe(&obs2, "W2N1"))?;
        assert_eq!(r.expr["value"], c::ERR_NOT_OWNER);
        // 门序锁定：非法房名下仍先撞 !my。
        let r = e.run_player("p1", &observe(&obs2, "not_a_room"))?;
        assert_eq!(r.expr["value"], c::ERR_NOT_OWNER, "NOT_OWNER 先于房名门");

        // RCL 7：observer 配额 [8]=1，isActive 为假。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard(&json!({"players":["p1"],
            "rooms":[{"name":"W1N1","owner":"p1","rcl":7}]}))?;
        let obs3 = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[25,25],"structureType":"observer","owner":"p1"}),
        )?;
        let r = e.run_player("p1", &observe(&obs3, "W2N1"))?;
        assert_eq!(r.expr["value"], c::ERR_RCL_NOT_ENOUGH, "RCL7 未激活");
        // 门序锁定：超距房名下仍先撞 RCL 门。
        let r = e.run_player("p1", &observe(&obs3, "W20S20"))?;
        assert_eq!(r.expr["value"], c::ERR_RCL_NOT_ENOUGH, "RCL 先于射程门");
    }
    Ok(())
}

/// OPERATE_OBSERVER：施放挂效果（官方四键形状）、扣 ops 进冷却发事件；
/// 效果激活即无限射程（官方无 10+N 公式）；无效目标（tower）静默丢弃
/// 不扣费。
#[test]
fn observer_operate_power_unlimited_range() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, obs) = fixture(binary)?;
        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[25,26],"structureType":"tower","owner":"p1",
                "store":{"energy":100}}),
        )?;
        let pc_id = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"opc","owner":"p1","pos":[24,25],
                "powers":{"7":1},"store":{"ops":200}}),
        )?;
        // 无效目标先证静默：rc OK、不扣 ops、无效果、不进冷却。
        let r = e.run_player(
            "p1",
            &format!(
                "Game.powerCreeps.opc.usePower(7,Game.getObjectById('{}'))",
                tower
            ),
        )?;
        assert_eq!(r.expr["value"], 0, "无效目标玩家面仍 OK");
        assert!(
            harness::events(&e, c::EVENT_POWER).is_empty(),
            "无效目标不发事件"
        );
        let r = e.run_player(
            "p1",
            &format!(
                "[Game.powerCreeps.opc.store[RESOURCE_OPS],\
                 (Game.getObjectById('{}').effects||[]).length]",
                tower
            ),
        )?;
        assert_eq!(r.expr["value"][0], 200, "无效目标不扣 ops");
        assert_eq!(r.expr["value"][1], 0, "tower 无效果");

        // 有效目标：扣 10 ops、进冷却、效果四键、事件。
        let r = e.run_player(
            "p1",
            &format!(
                "Game.powerCreeps.opc.usePower(7,Game.getObjectById('{}'))",
                obs
            ),
        )?;
        assert_eq!(r.expr["value"], 0);
        let events = harness::events(&e, c::EVENT_POWER);
        assert_eq!(events.len(), 1, "EVENT_POWER 恰一条");
        assert_eq!(events[0]["data"]["power"], c::PWR_OPERATE_OBSERVER);
        let r = e.run_player(
            "p1",
            &format!(
                "[Game.powerCreeps.opc.store[RESOURCE_OPS],\
                 JSON.stringify(Game.getObjectById('{}').effects.map(e=>\
                 [Object.keys(e).sort(),e.power,e.level,e.ticksRemaining>0]))]",
                obs
            ),
        )?;
        assert_eq!(r.expr["value"][0], 190, "扣 ops 10");
        let shape = r.expr["value"][1].as_str().unwrap_or_default();
        assert_eq!(
            shape, r#"[[["effect","level","power","ticksRemaining"],7,1,true]]"#,
            "官方四键形状（外层为 map 数组）"
        );
        // 冷却是 raw 字段（玩家面 powers 不暴露 cooldownTime），经快照读。
        // 施放前已有两次 run_player（无效施放 + ops/effects 读数）：
        // 有效施放结算于 t3，冷却 = 3+400。
        assert_eq!(
            e.get_object(&pc_id).unwrap()["powers"]["7"]["cooldownTime"],
            403,
            "冷却 2+400"
        );

        // 效果激活：超距观察放行且下一拍可见。
        let r = e.run_player("p1", &observe(&obs, "W12N1"))?;
        assert_eq!(r.expr["value"], 0, "效果期无限射程");
        let r = e.run_player("p1", "!!Game.rooms.W12N1")?;
        assert_eq!(r.expr["value"], true, "超距观察生效");

        // 效果到期拍（endTime 202 == time）：豁免消失，超距恢复拒绝
        // （active_power 与玩家面过滤同用严格 > 边界）。
        e.tick(196)?; // time 6 → 202
        let r = e.run_player("p1", &observe(&obs, "W12N1"))?;
        assert_eq!(r.expr["value"], c::ERR_NOT_IN_RANGE, "到期拍豁免消失");
    }
    Ok(())
}

/// 多 observer 多目标：观察集合并集——两间主房各一 observer（RCL8 配额
/// 每房 1，不可同房），同拍双目标可见。
#[test]
fn observer_multiple_targets_aggregate() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard(&json!({"players":["p1"],
            "rooms":[{"name":"W1N1","owner":"p1","rcl":8},
                     {"name":"W3N1","owner":"p1","rcl":8},
                     {"name":"W2N1"},{"name":"W4N1"}]}))?;
        let o1 = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[25,25],"structureType":"observer","owner":"p1"}),
        )?;
        let o3 = e.place_object(
            "W3N1",
            "structure",
            &json!({"pos":[25,25],"structureType":"observer","owner":"p1"}),
        )?;
        e.run_player(
            "p1",
            &format!("[{},{}]", observe(&o1, "W2N1"), observe(&o3, "W4N1")),
        )?;
        let r = e.run_player("p1", "[!!Game.rooms.W2N1,!!Game.rooms.W4N1]")?;
        assert_eq!(r.expr["value"][0], true, "第一目标可见");
        assert_eq!(
            r.expr["value"][1], true,
            "第二目标同拍可见（并集非首个命中）"
        );
    }
    Ok(())
}

/// 观察轮写回失败整轮候选回滚；重试原子提交；检查点冷恢复保留观察
/// 标记（快照随 Generic data 走既有通道）。
#[test]
fn observer_retry_and_cold_restore() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, obs) = fixture(binary)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", &format!("Memory.n=1;{}", observe(&obs, "W2N1")))
                .is_err()
        );
        assert_eq!(e.world_snapshot(), before, "失败轮整体回滚");
        e.run_player("p1", "throw Error('重放缓存轮')")?;
        let r = e.run_player("p1", "[!!Game.rooms.W2N1,Memory.n]")?;
        assert_eq!(r.expr["value"][0], true, "重试轮已提交标记");
        assert_eq!(r.expr["value"][1], 1);

        // 冷恢复：标记已写入的拍做检查点，恢复后同一读面可见。
        let (mut e, obs) = fixture(binary)?;
        e.run_player("p1", &observe(&obs, "W2N1"))?;
        let mut restored = EmbedEngine::new();
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        let r = restored.run_player("p1", "!!Game.rooms.W2N1")?;
        assert_eq!(r.expr["value"], true, "恢复保留观察标记");
        // 读数轮各推一拍，两侧时间对齐后再做逐拍快照对比。
        e.run_player("p1", "0")?;
        for ticks in [1, 3] {
            e.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
    }
    Ok(())
}
