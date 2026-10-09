//! OPERATE_TERMINAL 费用/冷却折扣与 DISRUPT_TERMINAL 提取封锁的真实玩家链。
use super::*;
use kk_core::constants as c;

/// POWER_INFO[8]：duration 1000 / cooldown 500 / range 3 / ops 100，
/// effect [0.9,0.8,0.7,0.6,0.5]；[15]：duration 10 / cooldown 8 / range 50 /
/// ops [50,40,30,20,10]。
fn fixture(binary: bool) -> anyhow::Result<(EmbedEngine, String, String)> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":6,"powerEnabled":true},
                 {"name":"W3N1","owner":"p1","rcl":6,"powerEnabled":true}]}))?;
    let src = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[25,25],"structureType":"terminal","owner":"p1",
            "store":{"energy":100000,"H":1000}}),
    )?;
    let dst = e.place_object(
        "W3N1",
        "structure",
        &json!({"pos":[25,25],"structureType":"terminal","owner":"p1",
            "store":{"energy":500}}),
    )?;
    Ok((e, src, dst))
}

fn actor(e: &mut EmbedEngine, name: &str, power: i64, level: i64) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({"name":name,"owner":"p1","pos":[24,25],
        "powers":{power.to_string():level},"store":{"ops":300}}),
    )
}
fn cast(name: &str, power: i64, target: &str) -> String {
    format!("Game.powerCreeps.{name}.usePower({power},Game.getObjectById('{target}'))")
}
fn terminal_multiplier(level: i64) -> f64 {
    [0.9, 0.8, 0.7, 0.6, 0.5][(level - 1) as usize]
}

/// 发送费用 ceil(基费×倍率)、到账与冷却 round(10×倍率)；截止拍
/// （endTime == time）全局相按全费与满冷却收账（market.js 严格 >）。
/// 意图相 send.js 的 >= 折扣存在，但被更严的玩家面预检（不乘倍率）
/// 支配，经玩家链不可独立观测，本套只钉住全局相边界。
#[test]
fn operate_terminal_send_discount_cooldown_and_boundary_tick() -> anyhow::Result<()> {
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, src, dst) = fixture(binary)?;
            actor(&mut e, "op", c::PWR_OPERATE_TERMINAL, level)?;
            e.run_player("p1", &cast("op", c::PWR_OPERATE_TERMINAL, &src))?; // 当前 tick 施放，持续 1000 拍
            let rc = e.run_player(
                "p1",
                &format!("Game.getObjectById('{src}').send('H', 100, 'W3N1')"),
            )?; // 下一 tick 发送结算
            assert_eq!(rc.expr["value"], 0);
            // W1N1→W3N1 距离 2（环绕不变：房集跨度 5、dx 2 → min(5−2,2)=2）：
            // 基费 ceil(100×(1−e^(−2/30))) = ceil(6.44) = 7。
            let base = 7i64;
            let discounted = ((base as f64) * terminal_multiplier(level)).ceil() as i64;
            assert_eq!(
                e.get_object(&src).unwrap()["store"]["energy"],
                json!(100_000 - discounted),
                "等级 {level} 费用 {discounted}"
            );
            assert_eq!(e.get_object(&dst).unwrap()["store"]["H"], 100);
            let cooldown = ((10.0_f64) * terminal_multiplier(level)).round() as i64;
            let rc = e.run_player("p1", &format!("Game.getObjectById('{src}').cooldown"))?;
            assert_eq!(
                rc.expr["value"],
                json!(cooldown - 1),
                "发送后读取剩余冷却，已经推进一拍"
            );
        }
        // 用实际施放时间计算截止拍，分别验证前一拍、等号拍和后一拍。
        // 每个场景独立发送，避免之前的冷却遮住效果边界。
        for offset in [-1i64, 0, 1] {
            let (mut e, src, dst) = fixture(binary)?;
            actor(&mut e, "op", c::PWR_OPERATE_TERMINAL, 5)?;
            let end_time = e.get_game_time() + 1000;
            e.run_player("p1", &cast("op", c::PWR_OPERATE_TERMINAL, &src))?;
            let send_time = (end_time as i64 + offset) as u64;
            e.tick(send_time - e.get_game_time())?;
            assert_eq!(e.get_game_time(), send_time);
            let rc = e.run_player(
                "p1",
                &format!("Game.getObjectById('{src}').send('H', 100, 'W3N1')"),
            )?;
            assert_eq!(rc.expr["value"], 0);
            let fee = if offset < 0 { 4 } else { 7 };
            assert_eq!(
                e.get_object(&src).unwrap()["store"]["energy"],
                100_000 - fee
            );
            assert_eq!(e.get_object(&dst).unwrap()["store"]["H"], 100);
            let cooldown = e.run_player("p1", &format!("Game.getObjectById('{src}').cooldown"))?;
            assert_eq!(
                cooldown.expr["value"],
                if offset < 0 { 4 } else { 9 },
                "endTime 偏移 {offset} 的冷却"
            );
        }
    }
    Ok(())
}

/// DISRUPT_TERMINAL 封锁 creep 从 terminal 提取（官方 withdraw.js:37，
/// 严格 > 边界）；施放门与 ops 数组费用 [50..10]；到期恢复提取。
#[test]
fn disrupt_terminal_blocks_withdraw_until_strict_expiry() -> anyhow::Result<()> {
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, src, _) = fixture(binary)?;
            actor(&mut e, "dis", c::PWR_DISRUPT_TERMINAL, level)?;
            let miner = e.place_object(
                "W1N1",
                "creep",
                &json!({"name":"miner","owner":"p1","pos":[25,24],
                    "body":["carry"],"ticksToLive":1500}),
            )?;
            let end_time = e.get_game_time() + 10;
            e.run_player("p1", &cast("dis", c::PWR_DISRUPT_TERMINAL, &src))?;
            assert_eq!(
                e.get_object(&src).unwrap()["effects"][0]["power"],
                c::PWR_DISRUPT_TERMINAL
            );
            let ops = [300 - 50, 300 - 40, 300 - 30, 300 - 20, 300 - 10][(level - 1) as usize];
            let rc = e
                .run_player(
                    "p1",
                    &format!(
                        "[Game.creeps.miner.withdraw(Game.getObjectById('{src}'),'H',10),Game.getObjectById('{src}').store.H]"
                    ),
                )
                .unwrap();
            // 官方 creeps.js:519-521：玩家面预检对被干扰 terminal 直接
            // ERR_INVALID_TARGET（与结算静默拒绝对齐）。
            assert_eq!(rc.expr["value"][0], c::ERR_INVALID_TARGET);
            assert_eq!(rc.expr["value"][1], 1000, "结算静默拒绝，库存不动");
            assert!(
                e.get_object(&miner).unwrap()["store"]["H"].is_null(),
                "creep 未获得资源（零键省略）"
            );
            // ops 数组按等级扣。
            let pc_ops = e.run_player("p1", "Game.powerCreeps.dis.store[RESOURCE_OPS]")?;
            assert_eq!(pc_ops.expr["value"], json!(ops), "等级 {level} ops");
            // 前一拍仍封锁；本次玩家轮推进到 endTime 后立即验证等号拍。
            e.tick(end_time - 1 - e.get_game_time())?;
            assert_eq!(e.get_game_time(), end_time - 1);
            let rc = e.run_player(
                "p1",
                &format!("Game.creeps.miner.withdraw(Game.getObjectById('{src}'),'H',10)"),
            )?;
            assert_eq!(rc.expr["value"], c::ERR_INVALID_TARGET, "截止前一拍仍封锁");
            assert_eq!(e.get_object(&src).unwrap()["store"]["H"], 1000);
            assert_eq!(e.get_game_time(), end_time, "精确到期拍");
            let rc = e.run_player(
                "p1",
                &format!("Game.creeps.miner.withdraw(Game.getObjectById('{src}'),'H',10)"),
            )?;
            assert_eq!(rc.expr["value"], 0);
            assert_eq!(e.get_object(&miner).unwrap()["store"]["H"], 10);
        }
    }
    Ok(())
}

/// 非法目标静默拒绝（tower）；市场 deal 的费用端折扣走同一 execute_transfer
/// （费用端恒为 dealer 终端，官方 market.js:339 恒传 targetTerminal）。
#[test]
fn operate_terminal_cast_gates_and_deal_fee_discount() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, _src, _dst) = fixture(binary)?;
        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[27,27],"structureType":"tower","owner":"p1"}),
        )?;
        let bad = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"bad","owner":"p1","pos":[26,27],
            "powers":{"8":1},"store":{"ops":300}}),
        )?;
        e.run_player("p1", &cast("bad", c::PWR_OPERATE_TERMINAL, &tower))?;
        assert_eq!(
            e.get_object(&bad).unwrap()["store"]["ops"],
            300,
            "tower 拒绝"
        );
        assert!(e.get_object(&tower).unwrap()["effects"].is_null());

        // deal 链：p2 的买单挂 W3N1（其 terminal 承担费用端、无效果 →
        // 全费），p1 的 terminal 带 OPERATE_TERMINAL（deal 冷却端 → 折扣）。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard(
            &json!({"players":[{"name":"p1","power":100000},{"name":"p2","money":100000}],
            "rooms":[{"name":"W1N1","owner":"p1","rcl":6,"powerEnabled":true},
                     {"name":"W3N1","owner":"p2","rcl":6}]}),
        )?;
        let src = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[25,25],"structureType":"terminal","owner":"p1",
                "store":{"energy":100000,"H":1000}}),
        )?;
        let dst = e.place_object(
            "W3N1",
            "structure",
            &json!({"pos":[25,25],"structureType":"terminal","owner":"p2",
                "store":{"energy":100000}}),
        )?;
        let oid = serde_json::to_string(&e.place_market_order(&json!({
            "owner": "p2", "type": "buy", "resourceType": "H",
            "price": 1.0, "totalAmount": 500, "roomName": "W3N1", "active": true,
        }))?)?;
        // 订单生命周期要求订单房（W3N1）有属订单主 p2 的活跃 terminal，
        // 否则买单 active=false 从公共市场视图消失——挪订单房前先核此依赖。
        e.tick(1)?;
        actor(&mut e, "op", c::PWR_OPERATE_TERMINAL, 5)?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_TERMINAL, &src))?; // 当前 tick 施放
        let rc = e.run_player("p1", &format!("Game.market.deal({oid}, 100, 'W1N1')"))?; // dealer=p1 src，资源 src → dst
        assert_eq!(rc.expr["value"], json!(0), "deal OK");
        assert_eq!(e.get_object(&dst).unwrap()["store"]["H"], 100);
        // 距离 2 基费 7；费用端=dealer 终端（p1 src，5 级效果）：
        // ceil(7×0.5)=4；订单主 dst 能量不动。
        assert_eq!(
            e.get_object(&src).unwrap()["store"]["energy"],
            99_996,
            "BUY 单费用端为 dealer 终端（官方 market.js:339）"
        );
        assert_eq!(e.get_object(&dst).unwrap()["store"]["energy"], 100_000);
        assert_eq!(e.get_object(&src).unwrap()["store"]["H"], 900, "卖出 100");
        // 资金腿：dealCost=100×1.0=100 credits，p1 收款、p2 付款。
        let after = e.run_player(
            "p1",
            &format!("[Game.market.credits, Game.getObjectById('{src}').cooldown]"),
        )?;
        assert_eq!(
            after.expr["value"][0],
            json!(10_000_100),
            "dealer 收款（credits 种子 10M + 100）"
        );
        let p2 = e.run_player("p2", "Game.market.credits")?;
        assert_eq!(
            p2.expr["value"],
            json!(9_999_900),
            "订单主付款（credits 种子 10M − 100）"
        );
        // deal 冷却在目标 terminal（dealer 的 src，带效果）：round(10×0.5)=5。
        // 下一拍读取时剩余 4，不依赖 createShard 的初始时间。
        assert_eq!(
            after.expr["value"][1],
            json!(5 - 1),
            "deal 后下一拍读取剩余冷却"
        );
    }
    Ok(())
}

/// 施放轮写回失败缓存整轮候选；重试原子提交；检查点冷恢复与继续推进
/// 逐拍一致。
#[test]
fn terminal_powers_retry_and_cold_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, src, _) = fixture(binary)?;
        actor(&mut e, "op", c::PWR_OPERATE_TERMINAL, 2)?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;[{},Game.getObjectById('{src}').send('H',100,'W3N1')]",
                    cast("op", c::PWR_OPERATE_TERMINAL, &src)
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(
            e.get_object(&src).unwrap()["effects"][0]["ticksRemaining"],
            999
        );
        assert_eq!(e.get_object(&src).unwrap()["store"]["H"], 900, "发送已结算");
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
