//! OPERATE_CONTROLLER 抬升 8 级控制器每 tick 升级限额的真实玩家链。
use super::*;
use kk_core::constants as c;

/// POWER_INFO[18]：duration 1000 / cooldown 800 / range 3 / ops 200，
/// effect [10,20,30,40,50]。8 级基础限额 CONTROLLER_MAX_UPGRADE_PER_TICK=15，
/// 仅 8 级消费（官方 upgradeController.js:44-52，边界 endTime >= time）；
/// 8 级升级不写控制器 progress，只涨 GCL。
fn fixture(binary: bool, rcl: i64) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":rcl,"powerEnabled":true}]}))?;
    Ok(e)
}

/// powerCreep 放在控制器 [1,1] 相邻的 [1,2]，自带 ops 500（两次施放）。
fn actor(e: &mut EmbedEngine, name: &str, level: i64) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({"name":name,"owner":"p1","pos":[1,2],
        "powers":{"18":level},"store":{"ops":500}}),
    )
}

/// 升级 creep 放在 [2,2]（控制器三格内），身体 works 个 work、满能量。
fn upgrader(e: &mut EmbedEngine, name: &str, works: usize) -> anyhow::Result<String> {
    let body: Vec<&str> = vec!["work"; works];
    e.place_object(
        "W1N1",
        "creep",
        &json!({"name":name,"owner":"p1","pos":[2,2],
            "body":body,"store":{"energy":works as i64},"ticksToLive":1500}),
    )
}

fn cast(name: &str, target: &str) -> String {
    format!(
        "Game.powerCreeps.{name}.usePower({},{target})",
        c::PWR_OPERATE_CONTROLLER
    )
}

const CTL: &str = "Game.rooms.W1N1.controller";

/// 各等级限额 15+effect：双 creep 40 work 依次升级，本轮累计钳到限额；
/// 无效果基线 15；8 级不写 progress 只涨 GCL；截止拍（endTime == time）
/// 仍按抬升限额结算（>= 边界，与 terminal 的严格 > 分叉）。
#[test]
fn operate_controller_raises_level8_cap_with_strict_boundary() -> anyhow::Result<()> {
    for binary in [false, true] {
        // 无效果基线：A 得 15、B 到顶被拒（能量不动）。
        let mut e = fixture(binary, 8)?;
        actor(&mut e, "op", 1)?;
        upgrader(&mut e, "ua", 40)?;
        upgrader(&mut e, "ub", 40)?;
        e.run_player(
            "p1",
            &format!(
                "[Game.creeps.ua.upgradeController({CTL}),Game.creeps.ub.upgradeController({CTL})]"
            ),
        )?;
        let g = e.run_player(
            "p1",
            "[Game.gcl.progress,Game.creeps.ua.store.energy,Game.creeps.ub.store.energy]",
        )?;
        assert_eq!(g.expr["value"][0], json!(15), "基础限额 15");
        assert_eq!(g.expr["value"][1], json!(25), "A 花费 15");
        assert_eq!(g.expr["value"][2], json!(40), "B 被限额拒绝，能量不动");

        // 各等级：限额 15+[10,20,30,40,50]，两 creep 合计钳制。
        for level in 1..=5 {
            let mut e = fixture(binary, 8)?;
            actor(&mut e, "op", level)?;
            upgrader(&mut e, "ua", 40)?;
            upgrader(&mut e, "ub", 40)?;
            e.run_player("p1", &cast("op", CTL))?;
            e.run_player(
                "p1",
                &format!(
                    "[Game.creeps.ua.upgradeController({CTL}),Game.creeps.ub.upgradeController({CTL})]"
                ),
            )?;
            let cap = 15 + [10, 20, 30, 40, 50][(level - 1) as usize];
            let got_a = 40.min(cap);
            let got_b = (cap - 40).clamp(0, 40);
            let g = e.run_player(
                "p1",
                "[Game.gcl.progress,Game.creeps.ua.store.energy,Game.creeps.ub.store.energy,\
                 Game.rooms.W1N1.controller.progress]",
            )?;
            assert_eq!(g.expr["value"][0], json!(cap), "等级 {level} 限额 {cap}");
            assert_eq!(g.expr["value"][1], json!(40 - got_a), "等级 {level} A");
            assert_eq!(g.expr["value"][2], json!(40 - got_b), "等级 {level} B");
            assert_eq!(g.expr["value"][3], json!(0), "8 级不写 progress");
        }

        // 截止拍：施放于 tick 0（end 1000），tick 1000 升级仍享抬升限额
        // （官方 >= 边界）；tick 1001 过期回基础限额——ua 剩 15 能量恰好
        // 被能量钳死，须以第二满能量 ub 钉住回落（若效果未过期，限额 25
        // 下 ub 会再花 10）。
        let mut e = fixture(binary, 8)?;
        actor(&mut e, "op", 1)?;
        upgrader(&mut e, "ua", 40)?;
        upgrader(&mut e, "ub", 40)?;
        e.run_player("p1", &cast("op", CTL))?; // tick 0，end 1000
        e.tick(999)?; // time 1000
        e.run_player("p1", &format!("Game.creeps.ua.upgradeController({CTL})"))?;
        let g = e.run_player("p1", "[Game.gcl.progress,Game.creeps.ua.store.energy]")?;
        assert_eq!(g.expr["value"][0], json!(25), "截止拍限额仍 25");
        assert_eq!(g.expr["value"][1], json!(15), "40−25");
        e.run_player(
            "p1",
            &format!(
                "[Game.creeps.ua.upgradeController({CTL}),Game.creeps.ub.upgradeController({CTL})]"
            ),
        )?; // tick 1001：效果过期
        let g = e.run_player(
            "p1",
            "[Game.gcl.progress,Game.creeps.ua.store.energy,Game.creeps.ub.store.energy]",
        )?;
        assert_eq!(g.expr["value"][0], json!(40), "过期回基础限额（25+15）");
        assert_eq!(g.expr["value"][1], json!(0), "ua 能量恰好 15");
        assert_eq!(g.expr["value"][2], json!(40), "ub 到顶被拒，能量不动");
    }
    Ok(())
}

/// 限额只钳未强化量：XGH2O（upgradeController=2，每部件增量 +1）的
/// boost 增量不占限额也不计入本轮累计；官方 boostedParts 只按钳后
/// 基础量取前缀、_upgraded 只累计未强化量（upgradeController.js:47-53）。
#[test]
fn operate_controller_cap_clamps_base_not_boost() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary, 8)?;
        actor(&mut e, "op", 1)?;
        let body: Vec<&str> = vec!["work"; 40];
        let mut boosts = serde_json::Map::new();
        for i in 0..40 {
            boosts.insert(i.to_string(), json!("XGH2O"));
        }
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"ub1","owner":"p1","pos":[2,2],
                "body":body,"boosts":boosts,
                "store":{"energy":40},"ticksToLive":1500}),
        )?;
        upgrader(&mut e, "uc", 40)?;
        e.run_player("p1", &cast("op", CTL))?; // L1：限额 25
        e.run_player(
            "p1",
            &format!(
                "[Game.creeps.ub1.upgradeController({CTL}),Game.creeps.uc.upgradeController({CTL})]"
            ),
        )?;
        let g = e.run_player(
            "p1",
            "[Game.gcl.progress,Game.creeps.ub1.store.energy,Game.creeps.uc.store.energy,             Game.rooms.W1N1.controller.progress]",
        )?;
        assert_eq!(g.expr["value"][0], json!(50), "25 钳后基础 + 25 增量");
        assert_eq!(g.expr["value"][1], json!(15), "能量只扣基础量 25");
        assert_eq!(g.expr["value"][2], json!(40), "未强化 creep 按累计 25 被拒");
        assert_eq!(g.expr["value"][3], json!(0), "8 级不写 progress");
    }
    Ok(())
}

/// 非法目标静默拒绝；施放落效果表、扣 ops 200、进冷却 800、发 EVENT_POWER；
/// 8 级以下效果惰性（cast 成功、升级不受影响）；射程与覆盖门。
#[test]
fn operate_controller_cast_gates_and_below_level8_inert() -> anyhow::Result<()> {
    for binary in [false, true] {
        // 非法目标（storage）：结算静默拒绝。
        let mut e = fixture(binary, 8)?;
        let storage = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[3,3],"structureType":"storage","owner":"p1"}),
        )?;
        let pc_id = actor(&mut e, "op", 1)?;
        e.run_player(
            "p1",
            &cast("op", &format!("Game.getObjectById('{storage}')")),
        )?;
        assert_eq!(
            e.get_object(&pc_id).unwrap()["store"]["ops"],
            500,
            "ops 不扣"
        );
        assert!(
            e.get_object(&pc_id).unwrap()["powers"]["18"]["cooldownTime"].is_null(),
            "不进冷却"
        );
        assert_eq!(
            e.run_player("p1", &format!("({CTL}.effects||[]).length"))?
                .expr["value"],
            0,
            "控制器无效果"
        );
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());

        // 正常施放：效果四键、ops、冷却、事件。
        let mut e = fixture(binary, 8)?;
        let pc_id = actor(&mut e, "op", 2)?;
        upgrader(&mut e, "ua", 3)?;
        e.run_player("p1", &cast("op", CTL))?;
        assert_eq!(
            e.get_object(&pc_id).unwrap()["store"]["ops"],
            300,
            "ops 200"
        );
        assert_eq!(
            e.get_object(&pc_id).unwrap()["powers"]["18"]["cooldownTime"],
            json!(801),
            "冷却 800（施放结算于 tick 1：热身后首个玩家拍）"
        );
        // 事件 1-tick 生命期：施放后立即核对，再读效果。
        assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
        let eff = e.run_player(
            "p1",
            &format!("{CTL}.effects[0] && [{CTL}.effects[0].power,{CTL}.effects[0].level,{CTL}.effects[0].ticksRemaining]"),
        )?;
        assert_eq!(eff.expr["value"], json!([18, 2, 999]), "读取时 time 1");

        // 射程门：远离控制器的 pc 预检 ERR_NOT_IN_RANGE。
        e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"far","owner":"p1","pos":[10,10],
            "powers":{"18":1},"store":{"ops":500}}),
        )?;
        let rc = e.run_player("p1", &cast("far", CTL))?;
        assert_eq!(rc.expr["value"], c::ERR_NOT_IN_RANGE);

        // 8 级以下（rcl 4）：效果落表但惰性，升级照常累计 progress。
        let mut e = fixture(binary, 4)?;
        actor(&mut e, "op", 1)?;
        upgrader(&mut e, "ua", 3)?;
        e.run_player("p1", &cast("op", CTL))?;
        e.run_player("p1", &format!("Game.creeps.ua.upgradeController({CTL})"))?;
        let g = e.run_player(
            "p1",
            &format!("[{CTL}.progress,{CTL}.level,{CTL}.effects.length]"),
        )?;
        assert_eq!(g.expr["value"][0], json!(3), "升级照常");
        assert_eq!(g.expr["value"][1], json!(4), "等级不变");
        assert_eq!(g.expr["value"][2], json!(1), "效果在表");
    }
    Ok(())
}

/// 施放+升级轮写回失败缓存整轮候选；重试原子提交；检查点冷恢复与继续
/// 推进逐拍一致。
#[test]
fn operate_controller_retry_and_cold_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary, 8)?;
        actor(&mut e, "op", 1)?;
        upgrader(&mut e, "ua", 20)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;[{},Game.creeps.ua.upgradeController({CTL})]",
                    cast("op", CTL)
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        let g = e.run_player(
            "p1",
            "[Game.gcl.progress,Game.powerCreeps.op.store[RESOURCE_OPS],Game.creeps.ua.store.energy]",
        )?;
        assert_eq!(g.expr["value"][0], json!(20), "升级已结算（20<限额 25）");
        assert_eq!(g.expr["value"][1], json!(300), "施放已结算");
        assert_eq!(g.expr["value"][2], json!(0));
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
