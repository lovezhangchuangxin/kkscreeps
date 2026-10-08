//! OPERATE_STORAGE 容量加成与 OPERATE_EXTENSION 即时灌能的真实玩家链。
use super::*;
use kk_core::constants as c;

fn fixture(binary: bool, rcl: u8, energy: i64) -> anyhow::Result<(EmbedEngine, String)> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":rcl,"powerEnabled":true}]}))?;
    let storage = e.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":"storage","pos":[20,20],"owner":"p1",
            "store":{"energy":energy}}),
    )?;
    Ok((e, storage))
}

fn extension_at(e: &mut EmbedEngine, pos: [u8; 2]) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":"extension","pos":pos,"owner":"p1"}),
    )
}

fn actor(
    e: &mut EmbedEngine,
    name: &str,
    power: i64,
    level: i64,
    pos: [u8; 2],
) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({"name":name,"owner":"p1","pos":pos,
        "powers":{power.to_string():level},"store":{"ops":300}}),
    )
}
fn cast(name: &str, power: i64, target: &str) -> String {
    format!("Game.powerCreeps.{name}.usePower({power},Game.getObjectById('{target}'))")
}
fn storage_boost(level: i64) -> i64 {
    [500_000, 1_000_000, 2_000_000, 4_000_000, 7_000_000][(level - 1) as usize]
}

/// POWER_INFO[4]：duration 1000 / cooldown 800 / range 3 / ops 100，效果
/// 增量 [500k,1M,2M,4M,7M]。容量投影与结算同源；效果到期回基础量，
/// 已超存的库存不被钳掉，仅把后续转入钳到剩余余量。
#[test]
fn operate_storage_all_levels_boost_capacity_and_expiry_restores_base() -> anyhow::Result<()> {
    for binary in [false, true] {
        for level in 1..=5 {
            let (mut e, s) = fixture(binary, 8, 0)?;
            actor(&mut e, "op", c::PWR_OPERATE_STORAGE, level, [19, 20])?;
            e.run_player("p1", &cast("op", c::PWR_OPERATE_STORAGE, &s))?;
            assert_eq!(
                e.get_object(&s).unwrap()["storeCapacity"],
                json!(c::STORAGE_CAPACITY + storage_boost(level))
            );
            assert_eq!(e.get_object(&s).unwrap()["effects"][0]["level"], level);
            let free = e.run_player(
                "p1",
                &format!("Game.getObjectById('{s}').store.getFreeCapacity()"),
            )?;
            assert_eq!(
                free.expr["value"],
                json!(c::STORAGE_CAPACITY + storage_boost(level)),
                "玩家面 getFreeCapacity 与投影同源"
            );
        }
        // 到期回基础量 + 超存不钳：转入链走真实 creep transfer。
        let (mut e, s) = fixture(binary, 8, 999_000)?;
        actor(&mut e, "op", c::PWR_OPERATE_STORAGE, 1, [19, 20])?;
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"mover","owner":"p1","pos":[20,19],
                "body":["carry","carry"],"store":{"energy":100}}),
        )?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_STORAGE, &s))?;
        assert_eq!(
            e.run_player(
                "p1",
                &format!(
                    "Game.creeps.mover.transfer(Game.getObjectById('{s}'), RESOURCE_ENERGY, 100)"
                )
            )?
            .expr["value"],
            0,
            "加成期内 999000+100 ≤ 1.5M 放行"
        );
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 999_100);
        e.tick(1000)?; // duration 1000 到期
        assert_eq!(
            e.get_object(&s).unwrap()["storeCapacity"],
            json!(c::STORAGE_CAPACITY),
            "到期回基础量"
        );
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 999_100);
        let spec = json!({"name":"mover2","owner":"p1","pos":[21,19],
            "body":vec!["carry"; 48],"store":{"energy":2000}});
        e.place_object("W1N1", "creep", &spec)?;
        // 官方 game 层预检：amount > 余量整体 ERR_FULL（不做部分转入）。
        assert_eq!(
            e.run_player(
                "p1",
                &format!(
                    "Game.creeps.mover2.transfer(Game.getObjectById('{s}'), RESOURCE_ENERGY, 2000)"
                )
            )?
            .expr["value"],
            c::ERR_FULL
        );
        assert_eq!(
            e.run_player(
                "p1",
                &format!(
                    "Game.creeps.mover2.transfer(Game.getObjectById('{s}'), RESOURCE_ENERGY, 900)"
                )
            )?
            .expr["value"],
            0,
            "恰余量转入放行"
        );
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 1_000_000);
        assert_eq!(
            e.run_player("p1", "Game.creeps.mover2.store.energy")?.expr["value"],
            1100
        );
    }
    Ok(())
}

/// RCL/归属门：RCL3 无 storage 配额 → 容量 0（效果在位也不加成）；
/// 他人 storage 施放成功（类型门不查归属）但容量仍 0；非 storage 目标
/// 静默拒绝（不扣 ops、无事件、无效果）。
#[test]
fn operate_storage_rcl_and_owner_gates_keep_capacity_zero() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 3, 500)?;
        actor(&mut e, "op", c::PWR_OPERATE_STORAGE, 5, [19, 20])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_STORAGE, &s))?;
        assert_eq!(e.get_object(&s).unwrap()["storeCapacity"], 0, "RCL3 配额 0");
        assert_eq!(
            e.get_object(&s).unwrap()["effects"][0]["power"],
            c::PWR_OPERATE_STORAGE,
            "施放本身无归属门，效果照挂"
        );
        // 官方 game 层 creeps.js:459-461：容量 0 的目标 transfer 预检
        // ERR_INVALID_TARGET；结算侧也不接受转入。
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"mover","owner":"p1","pos":[20,19],
                "body":["carry"],"store":{"energy":50}}),
        )?;
        assert_eq!(
            e.run_player(
                "p1",
                &format!(
                    "Game.creeps.mover.transfer(Game.getObjectById('{s}'), RESOURCE_ENERGY, 50)"
                )
            )?
            .expr["value"],
            c::ERR_INVALID_TARGET
        );
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 500);

        let (mut e, _) = fixture(binary, 8, 0)?;
        let other = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"storage","pos":[25,25],"owner":"p2"}),
        )?;
        actor(&mut e, "op", c::PWR_OPERATE_STORAGE, 5, [24, 25])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_STORAGE, &other))?;
        assert_eq!(
            e.get_object(&other).unwrap()["storeCapacity"],
            0,
            "归属不匹配容量 0（官方 storages/tick 归一门）"
        );
        let terminal = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"terminal","pos":[26,26],"owner":"p1"}),
        )?;
        let pc = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"bad","owner":"p1","pos":[25,26],
            "powers":{"4":5},"store":{"ops":300}}),
        )?;
        e.run_player("p1", &cast("bad", c::PWR_OPERATE_STORAGE, &terminal))?;
        assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 300);
        assert!(e.get_object(&terminal).unwrap()["effects"].is_null());
    }
    Ok(())
}

/// POWER_INFO[6]：cooldown 50 / range 3 / ops 2 / effect [0.2..1]（Σ容量
/// 倍率）。预算 min(目标 energy, 倍率×Σextension 容量)，按到目标距离升序
/// 灌能；未移动能量（满 extension/空仓/非 store 目标）静默拒绝不扣费。
#[test]
fn operate_extension_fill_order_budget_and_gates() -> anyhow::Result<()> {
    for binary in [false, true] {
        // 等级 1：0.2 × 600 = 120，全部进最近的 extension；他人不参与。
        let (mut e, s) = fixture(binary, 8, 1000)?;
        let ext1 = extension_at(&mut e, [21, 20])?;
        let ext2 = extension_at(&mut e, [22, 21])?;
        let ext3 = extension_at(&mut e, [23, 23])?;
        e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"extension","pos":[21,21],"owner":"p2"}),
        )?;
        let pc = actor(&mut e, "op", c::PWR_OPERATE_EXTENSION, 1, [19, 20])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_EXTENSION, &s))?;
        assert_eq!(e.get_object(&ext1).unwrap()["store"]["energy"], 120);
        // 未灌能的原始记录省略零能量键。
        assert!(e.get_object(&ext2).unwrap()["store"]["energy"].is_null());
        assert!(e.get_object(&ext3).unwrap()["store"]["energy"].is_null());
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 880);
        assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 298, "ops 扣 2");
        assert_eq!(
            e.get_object(&pc).unwrap()["powers"]["6"]["cooldownTime"],
            50,
            "cooldown 50（结算于 tick 0）"
        );
        assert!(
            e.get_object(&s).unwrap()["effects"].is_null(),
            "即时灌能不落效果表"
        );

        // 等级 2：0.4 × 600 = 240 → 最近满 200，次近得 40，最远不动。
        let (mut e, s) = fixture(binary, 8, 1000)?;
        let ext1 = extension_at(&mut e, [21, 20])?;
        let ext2 = extension_at(&mut e, [22, 21])?;
        extension_at(&mut e, [23, 23])?;
        actor(&mut e, "op", c::PWR_OPERATE_EXTENSION, 2, [19, 20])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_EXTENSION, &s))?;
        assert_eq!(e.get_object(&ext1).unwrap()["store"]["energy"], 200);
        assert_eq!(e.get_object(&ext2).unwrap()["store"]["energy"], 40);
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 760);

        // 静默门：满 extension / 空仓 / 非 store 目标（tower）→ 不扣费无事件。
        let (mut e, s) = fixture(binary, 8, 1000)?;
        let full = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"extension","pos":[21,20],"owner":"p1",
                "store":{"energy":200}}),
        )?;
        let pc = actor(&mut e, "op", c::PWR_OPERATE_EXTENSION, 1, [19, 20])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_EXTENSION, &s))?;
        assert_eq!(
            e.get_object(&pc).unwrap()["store"]["ops"],
            300,
            "未移动能量不扣费"
        );
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());
        assert!(
            e.get_object(&pc).unwrap()["powers"]["6"]["cooldownTime"].is_null(),
            "静默拒绝不进冷却"
        );
        assert_eq!(e.get_object(&full).unwrap()["store"]["energy"], 200);

        let (mut e, s) = fixture(binary, 8, 0)?;
        extension_at(&mut e, [21, 20])?;
        let pc = actor(&mut e, "op", c::PWR_OPERATE_EXTENSION, 1, [19, 20])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_EXTENSION, &s))?;
        assert_eq!(
            e.get_object(&pc).unwrap()["store"]["ops"],
            300,
            "空仓静默拒绝"
        );
        // 原始记录省略零能量键；玩家面 getter 归 0。
        assert_eq!(
            e.run_player("p1", &format!("Game.getObjectById('{s}').store.energy"))?
                .expr["value"],
            0
        );

        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"tower","pos":[26,26],"owner":"p1"}),
        )?;
        let bad = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"bad","owner":"p1","pos":[25,26],
            "powers":{"6":1},"store":{"ops":300}}),
        )?;
        e.run_player("p1", &cast("bad", c::PWR_OPERATE_EXTENSION, &tower))?;
        assert_eq!(e.get_object(&bad).unwrap()["store"]["ops"], 300);

        // 超距：距目标 4 格 → 玩家面 ERR_NOT_IN_RANGE。
        let _far = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"far","owner":"p1","pos":[16,20],
            "powers":{"6":1},"store":{"ops":300}}),
        )?;
        assert_eq!(
            e.run_player("p1", &cast("far", c::PWR_OPERATE_EXTENSION, &s))?
                .expr["value"],
            c::ERR_NOT_IN_RANGE
        );
    }
    Ok(())
}

/// 施放轮写回失败缓存整轮候选；重试原子提交；检查点冷恢复后容量投影、
/// extension 灌能与继续推进逐拍一致。
#[test]
fn capacity_powers_retry_and_cold_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 8, 1000)?;
        let ext = extension_at(&mut e, [21, 20])?;
        actor(&mut e, "op", c::PWR_OPERATE_STORAGE, 1, [19, 20])?;
        actor(&mut e, "fill", c::PWR_OPERATE_EXTENSION, 1, [19, 21])?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;[{},{}]",
                    cast("op", c::PWR_OPERATE_STORAGE, &s),
                    cast("fill", c::PWR_OPERATE_EXTENSION, &s)
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        assert_eq!(
            e.get_object(&s).unwrap()["storeCapacity"],
            json!(c::STORAGE_CAPACITY + 500_000)
        );
        assert_eq!(
            e.get_object(&ext).unwrap()["store"]["energy"],
            40,
            "0.2 × 200"
        );
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 960);
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        for ticks in [1, 5, 20] {
            e.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert_eq!(
            e.get_object(&s).unwrap()["storeCapacity"],
            json!(c::STORAGE_CAPACITY + 500_000)
        );
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}

/// Generic 目标臂：terminal 作为能量源的全链（结构类型门、store.energy
/// 读值与 JSON 回写扣减、归属门拒绝他人 terminal）；container 同通路。
#[test]
fn operate_extension_from_generic_terminal_and_container_sources() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard(&json!({"players":[{"name":"p1","power":100000}],
            "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}))?;
        let terminal = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"terminal","pos":[20,20],"owner":"p1",
                "store":{"energy":1000}}),
        )?;
        let ext = extension_at(&mut e, [21, 20])?;
        actor(&mut e, "op", c::PWR_OPERATE_EXTENSION, 1, [19, 20])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_EXTENSION, &terminal))?;
        assert_eq!(
            e.get_object(&ext).unwrap()["store"]["energy"],
            40,
            "0.2 × 200"
        );
        assert_eq!(e.get_object(&terminal).unwrap()["store"]["energy"], 960);

        // 他人 terminal（有主不匹配）静默拒绝。
        let other = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"terminal","pos":[25,25],"owner":"p2",
                "store":{"energy":1000}}),
        )?;
        let ext2 = extension_at(&mut e, [26, 25])?;
        let bad = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"bad","owner":"p1","pos":[24,25],
            "powers":{"6":1},"store":{"ops":300}}),
        )?;
        e.run_player("p1", &cast("bad", c::PWR_OPERATE_EXTENSION, &other))?;
        assert_eq!(e.get_object(&bad).unwrap()["store"]["ops"], 300);
        assert!(e.get_object(&ext2).unwrap()["store"]["energy"].is_null());

        // container 源同通路。
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"container","pos":[30,30],"owner":"p1",
                "store":{"energy":500}}),
        )?;
        let ext3 = extension_at(&mut e, [30, 29])?;
        let cpc = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"cont","owner":"p1","pos":[29,30],
            "powers":{"6":1},"store":{"ops":300}}),
        )?;
        // 预算基数是全房己方 extension（此时 3 个 → 0.2 × 600 = 120），
        // 全部灌给距 container 最近的 ext3。
        e.run_player("p1", &cast("cont", c::PWR_OPERATE_EXTENSION, &container))?;
        assert_eq!(e.get_object(&ext3).unwrap()["store"]["energy"], 120);
        assert_eq!(e.get_object(&container).unwrap()["store"]["energy"], 380);
        assert_eq!(e.get_object(&cpc).unwrap()["store"]["ops"], 298);
    }
    Ok(())
}

/// 灌能顺序锚定目标而非施放者：extension 距 pc 更近但距目标更远时，
/// 预算灌给距目标最近者（官方 comparatorDistance(target)）。
#[test]
fn operate_extension_fill_order_anchors_on_target_not_caster() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 8, 1000)?;
        // pc 在 [22,22]（距目标 storage [20,20] 为 2，射程内）。
        let far_from_target = extension_at(&mut e, [23, 23])?; // 距 pc 1、距目标 3
        let near_target = extension_at(&mut e, [21, 20])?; // 距 pc 2、距目标 1
        actor(&mut e, "op", c::PWR_OPERATE_EXTENSION, 1, [22, 22])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_EXTENSION, &s))?;
        assert_eq!(
            e.get_object(&near_target).unwrap()["store"]["energy"],
            80,
            "预算 0.2 × 400 = 80 全部给距目标最近者"
        );
        assert!(e.get_object(&far_from_target).unwrap()["store"]["energy"].is_null());
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 920);
    }
    Ok(())
}

/// off extension 既不灌能也不计入预算基数（官方 filter 的双重作用）。
/// off 由每 tick 配额重算产生（RCL2 配额 5、第 6 个距控制器最远者失效），
/// 夹具直摆 off 会在指令相前被 calc_spawns 覆写。
#[test]
fn operate_extension_skips_off_extensions_in_fill_and_budget() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, s) = fixture(binary, 2, 1000)?;
        let on = extension_at(&mut e, [21, 20])?;
        for pos in [[20, 21], [21, 21], [19, 20], [20, 19]] {
            extension_at(&mut e, pos)?;
        }
        let off = extension_at(&mut e, [40, 40])?; // 距控制器 [1,1] 最远 → 超配额 off
        actor(&mut e, "op", c::PWR_OPERATE_EXTENSION, 1, [19, 20])?;
        e.run_player("p1", &cast("op", c::PWR_OPERATE_EXTENSION, &s))?;
        // RCL2 每个容量 50、有效 5 个 → 预算 0.2 × 250 = 50 全给距目标
        // 最近的 on；若误计 off（预算 60）断言失败。
        assert_eq!(e.get_object(&on).unwrap()["store"]["energy"], 50);
        assert!(e.get_object(&off).unwrap()["store"]["energy"].is_null());
        assert_eq!(e.get_object(&s).unwrap()["store"]["energy"], 950);
    }
    Ok(())
}
