//! G02.d2：移动玩家门规则端到端——覆盖值经冻结计划同时驱动玩家面
//! move()/moveTo() 预检常量与仲裁原生准入预填，单一 override 双端生效；
//! 默认引擎保持官方门槛（疲劳动一停一、无 MOVE 拒绝）。
use super::*;
use harness::{creep_at, pos_in};

/// 只覆盖移动门规则的观察包（无处理器）。
fn gate_pkg(rules: &[(&str, i64)]) -> String {
    let body = rules
        .iter()
        .map(|(rule, value)| format!("api.rules.override('builtin:{rule}',{value});"))
        .collect::<Vec<_>>()
        .join("");
    package(&body)
}

/// 疲劳上界覆盖：玩家门（-11→OK）、常量平铺与仲裁原生预填同源放宽，
/// [move,work,work] 移动者从官方「动一停一」变为逐 tick 连续移动；
/// 无 MOVE 者不受该门影响仍拒绝。
#[test]
fn fatigue_limit_override_lets_tired_movers_run_every_tick() -> anyhow::Result<()> {
    for binary in [false, true] {
        // 默认引擎对照：疲劳 2 时玩家门拒绝方向形态（返回码钉死）。
        let mut plain = engine(&[("baseline", package(""))], binary)?;
        let t0 = creep_at(
            &mut plain,
            "W1N1",
            "t0",
            "p1",
            20,
            20,
            &["move", "work", "work"],
        )?;
        // 首轮疲劳 0：移动并产生疲劳；下一轮被玩家门拦截（动一停一）。
        let rc = plain.run_player("p1", "Game.creeps.t0.move(BOTTOM)")?;
        assert_eq!(rc.expr["value"], 0, "{binary}: 首轮疲劳 0 放行");
        let rc = plain.run_player("p1", "Game.creeps.t0.move(BOTTOM)")?;
        assert_eq!(
            rc.expr["value"], -11,
            "{binary}: 默认门疲劳拒绝（动一停一）"
        );
        assert_eq!(
            pos_in(&plain, "W1N1", &t0),
            (20, 21),
            "{binary}: 仅首 tick 移动"
        );

        // 覆盖 fatigueLimit=u32::MAX：常量平铺、门放行、仲裁预填同源，
        // 连续四个 tick 逐格南移不再停歇。
        let source = gate_pkg(&[("creep.move.fatigueLimit", i64::from(u32::MAX))]);
        let mut e = engine(&[("marathon", source)], binary)?;
        let t1 = creep_at(
            &mut e,
            "W1N1",
            "t1",
            "p1",
            20,
            20,
            &["move", "work", "work"],
        )?;
        let seen = e.run_player("p1", "MOVE_FATIGUE_LIMIT")?;
        assert_eq!(
            seen.expr["value"],
            json!(u32::MAX),
            "{binary}: 生效值平铺到玩家全局"
        );
        for tick in 1..=4u8 {
            let rc = e.run_player("p1", "Game.creeps.t1.move(BOTTOM)")?;
            assert_eq!(rc.expr["value"], 0, "{binary}: 第 {tick} tick 门放行");
            assert_eq!(
                pos_in(&e, "W1N1", &t1),
                (20, 20 + i64::from(tick)),
                "{binary}: 第 {tick} tick 位移落地"
            );
        }

        // 无 MOVE 者不受疲劳门覆盖影响（另一门独立）。
        let l0 = creep_at(&mut e, "W1N1", "l0", "p1", 25, 20, &["work"])?;
        let rc = e.run_player("p1", "Game.creeps.l0.move(BOTTOM)")?;
        assert_eq!(rc.expr["value"], -12, "{binary}: minActiveMoves 仍为官方 1");
        assert_eq!(pos_in(&e, "W1N1", &l0), (25, 20));
    }
    Ok(())
}

/// minActiveMoves=0：无 MOVE 部件的方向形态移动过玩家门并真实位移
/// （仲裁预填同源获准、执行无二次复核）；默认引擎同一移动者被玩家
/// 门拒绝。moveTo 同门。
#[test]
fn min_active_moves_zero_lets_legless_movers_walk() -> anyhow::Result<()> {
    for binary in [false, true] {
        // 默认：无 MOVE 方向形态被玩家门拒绝。
        let mut plain = engine(&[("baseline", package(""))], binary)?;
        let l1 = creep_at(&mut plain, "W1N1", "l1", "p1", 10, 10, &["work", "carry"])?;
        let rc = plain.run_player("p1", "Game.creeps.l1.move(BOTTOM)")?;
        assert_eq!(rc.expr["value"], -12, "{binary}: 默认无 MOVE 拒绝");
        assert_eq!(pos_in(&plain, "W1N1", &l1), (10, 10));

        // 覆盖 minActiveMoves=0：门放行、位移落地、疲劳照常累积。
        let source = gate_pkg(&[("creep.move.minActiveMoves", 0)]);
        let mut e = engine(&[("legless", source)], binary)?;
        let l2 = creep_at(&mut e, "W1N1", "l2", "p1", 10, 10, &["work", "carry"])?;
        let seen = e.run_player("p1", "MOVE_MIN_ACTIVE_MOVES")?;
        assert_eq!(seen.expr["value"], 0, "{binary}: 生效值平铺到玩家全局");
        let rc = e.run_player("p1", "Game.creeps.l2.move(BOTTOM)")?;
        assert_eq!(rc.expr["value"], 0, "{binary}: 覆盖后门放行");
        assert_eq!(pos_in(&e, "W1N1", &l2), (10, 11), "{binary}: 无件位移落地");
        let obj = e.get_object(&l2).unwrap();
        assert!(
            obj["fatigue"].as_i64().unwrap_or(0) > 0,
            "{binary}: 无件移动疲劳照常累积（永不恢复，玩法自担）"
        );

        // moveTo 同门：用未积累疲劳的第二个无件 creep 验证（l2 已带
        // 永不恢复的疲劳，fatigueLimit 未覆盖时 moveTo 正确拒绝）。
        let rc = e.run_player("p1", "Game.creeps.l2.moveTo(10, 15, { reusePath: 0 })")?;
        assert_eq!(rc.expr["value"], -11, "{binary}: 疲劳门独立，未覆盖仍拒绝");
        let l3 = creep_at(&mut e, "W1N1", "l3", "p1", 12, 10, &["work", "carry"])?;
        let rc = e.run_player("p1", "Game.creeps.l3.moveTo(12, 15, { reusePath: 0 })")?;
        assert_eq!(rc.expr["value"], 0, "{binary}: moveTo 同门放行");
        assert_eq!(
            pos_in(&e, "W1N1", &l3),
            (12, 11),
            "{binary}: moveTo 首步位移"
        );
    }
    Ok(())
}

/// 两规则组合覆盖：fatigueLimit 与 minActiveMoves 同时放宽，双门独立
/// 叠加生效（既疲劳又无件的对象在组合门下可行走）。
#[test]
fn combined_gate_overrides_stack_independently() -> anyhow::Result<()> {
    for binary in [false, true] {
        let source = gate_pkg(&[
            ("creep.move.fatigueLimit", 1000),
            ("creep.move.minActiveMoves", 0),
        ]);
        let mut e = engine(&[("both", source)], binary)?;
        let c = creep_at(&mut e, "W1N1", "c", "p1", 10, 10, &["work", "work"])?;
        // 先制造疲劳：minActiveMoves=0 下先走一步（无件，疲劳不恢复）。
        e.run_player("p1", "Game.creeps.c.move(BOTTOM)")?;
        assert_eq!(pos_in(&e, "W1N1", &c), (10, 11), "{binary}: 无件先走一步");
        // 疲劳非零仍连续行走：双门同时放开。
        for tick in 2..=3u8 {
            let rc = e.run_player("p1", "Game.creeps.c.move(BOTTOM)")?;
            assert_eq!(rc.expr["value"], 0, "{binary}: 第 {tick} tick 双门放行");
            assert_eq!(pos_in(&e, "W1N1", &c), (10, 10 + i64::from(tick)));
        }
    }
    Ok(())
}
