//! 战斗射程规则（builtin:creep.attack/rangedAttack/heal/rangedHeal.range）
//! 端到端：覆盖值经冻结计划注入玩家常量，prelude 射程门与 Rust 结算
//! 同源放宽；rangedMassAttack 面积语义不随单目标射程变化；越界覆盖被拒。
use super::*;
use harness::{enemy, events, fighter, hits};

/// 只覆盖射程规则的观察包（无处理器）；规则键不带 builtin: 前缀。
fn range_pkg(rules: &[(&str, i64)]) -> String {
    let body = rules
        .iter()
        .map(|(rule, value)| format!("api.rules.override('builtin:{rule}',{value});"))
        .collect::<Vec<_>>()
        .join("");
    package(&body)
}

/// 近战射程覆盖：玩家门（-9→OK）、常量注入（ATTACK_RANGE=3）与
/// 结算距离同源放宽；默认引擎对照拒绝且无事件。
#[test]
fn attack_range_override_widens_gate_constants_and_settlement() -> anyhow::Result<()> {
    for binary in [false, true] {
        // 默认：距离 2 的 attack 被玩家面射程门拒绝，无事件无伤害。
        let mut plain = engine(&[("baseline", package(""))], binary)?;
        let _a0 = fighter(&mut plain, "a0", 10, 10, &["attack"])?;
        let e0 = enemy(&mut plain, "e0", 12, 10, &["move"])?;
        let rc = plain.run_player(
            "p1",
            &format!("Game.creeps.a0.attack(Game.getObjectById('{e0}'))"),
        )?;
        assert_eq!(rc.expr["value"], -9, "{binary}: 默认射程 1 拒");
        assert_eq!(hits(&plain, &e0), 100);
        assert!(events(&plain, kk_core::constants::EVENT_ATTACK).is_empty());

        // 覆盖 attack.range=3：常量平铺、门放行、结算命中。
        let source = range_pkg(&[("creep.attack.range", 3)]);
        let mut e = engine(&[("reach", source)], binary)?;
        let a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
        let e1 = enemy(&mut e, "e1", 12, 10, &["move"])?;
        let seen = e.run_player("p1", "ATTACK_RANGE")?;
        assert_eq!(seen.expr["value"], 3, "{binary}: 生效值平铺到玩家全局");
        let rc = e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))"),
        )?;
        assert_eq!(rc.expr["value"], 0, "{binary}: 覆盖后门放行");
        assert_eq!(hits(&e, &e1), 70, "{binary}: 结算按放宽射程命中");
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), 1, "{binary}");
        assert_eq!(attacks[0]["objectId"], json!(a1));
        assert_eq!(attacks[0]["data"]["damage"], json!(30));
    }
    Ok(())
}

/// 治疗族射程独立覆盖：先以近战打伤己方，远距 heal/rangedHeal 在默认
/// 射程下被门拒绝；放宽后门放行且 EVENT_HEAL 结算落地。
#[test]
fn heal_range_overrides_widen_gate_and_settlement() -> anyhow::Result<()> {
    // ally[tough] 100 血；medic(10,10) 与 rmedic(14,12) 分别距离 2/4 于 ally(10,12)。
    fn wounded_ally(e: &mut EmbedEngine) -> anyhow::Result<(String, String, String, String)> {
        let ally = fighter(e, "ally", 10, 12, &["tough"])?;
        let near = fighter(e, "medic", 10, 10, &["heal"])?;
        let far = fighter(e, "rmedic", 14, 12, &["heal"])?;
        let hitter = fighter(e, "hitter", 11, 12, &["attack"])?;
        // 先近战打掉 30（同 tick 打与治会相互作用，分轮更可归因）。
        e.run_player(
            "p1",
            &format!("Game.creeps.hitter.attack(Game.getObjectById('{ally}'))"),
        )?;
        Ok((ally, near, far, hitter))
    }

    for binary in [false, true] {
        // 默认：heal 距离 2、rangedHeal 距离 4 均被玩家面拒绝。
        let mut plain = engine(&[("baseline", package(""))], binary)?;
        let (ally, _near, _far, _hitter) = wounded_ally(&mut plain)?;
        assert_eq!(hits(&plain, &ally), 70, "{binary}: 预打伤 30");
        let rc = plain.run_player(
            "p1",
            &format!(
                "[Game.creeps.medic.heal(Game.getObjectById('{ally}')),\
                 Game.creeps.rmedic.rangedHeal(Game.getObjectById('{ally}'))].join(',')"
            ),
        )?;
        assert_eq!(rc.expr["value"], "-9,-9", "{binary}: 默认射程 1/3 拒");

        // 覆盖 heal.range=3、rangedHeal.range=5：同距离放行且治疗落地。
        let source = range_pkg(&[("creep.heal.range", 3), ("creep.rangedHeal.range", 5)]);
        let mut e = engine(&[("medic", source)], binary)?;
        let (ally, _near, _far, _hitter) = wounded_ally(&mut e)?;
        assert_eq!(hits(&e, &ally), 70, "{binary}: 预打伤 30");
        let seen = e.run_player("p1", "[HEAL_RANGE, RANGED_HEAL_RANGE].join(',')")?;
        assert_eq!(seen.expr["value"], "3,5", "{binary}: 生效值平铺");
        // heal 与 rangedHeal 互斥（官方压制表），分轮各证一次。
        let rc = e.run_player(
            "p1",
            &format!("Game.creeps.medic.heal(Game.getObjectById('{ally}'))"),
        )?;
        assert_eq!(rc.expr["value"], 0, "{binary}: heal 距离 2 放行");
        assert_eq!(hits(&e, &ally), 82, "{binary}: +12 治疗落地");
        let heals = events(&e, kk_core::constants::EVENT_HEAL);
        assert_eq!(heals.len(), 1, "{binary}");
        assert_eq!(heals[0]["data"]["amount"], json!(12));
        let rc = e.run_player(
            "p1",
            &format!("Game.creeps.rmedic.rangedHeal(Game.getObjectById('{ally}'))"),
        )?;
        assert_eq!(rc.expr["value"], 0, "{binary}: rangedHeal 距离 4 放行");
        assert_eq!(hits(&e, &ally), 86, "{binary}: +4 治疗落地");
    }
    Ok(())
}

/// rangedMassAttack 是三格面积语义：rangedAttack.range 压到 1 后，
/// 距离 2 的面积目标仍按 0.4 档命中（单目标射程规则不外溢）。
#[test]
fn ranged_mass_attack_keeps_area_semantics_under_range_override() -> anyhow::Result<()> {
    for binary in [false, true] {
        let source = range_pkg(&[("creep.rangedAttack.range", 1)]);
        let mut e = engine(&[("reach", source)], binary)?;
        let _gun = fighter(&mut e, "gun", 10, 10, &["ranged_attack"])?;
        let e1 = enemy(&mut e, "e1", 12, 10, &["move"])?;
        let seen = e.run_player("p1", "RANGED_ATTACK_RANGE")?;
        assert_eq!(seen.expr["value"], 1, "{binary}: 单目标射程已压到 1");
        e.run_player("p1", "Game.creeps.gun.rangedMassAttack()")?;
        assert_eq!(hits(&e, &e1), 96, "{binary}: 面积档 10×0.4 不受覆盖影响");
    }
    Ok(())
}

/// 射程规则值域 0..=50：越界覆盖在冻结时拒绝，不留半装配引擎。
#[test]
fn out_of_bounds_range_override_is_rejected_at_freeze() -> anyhow::Result<()> {
    for bad in [51i64, -1] {
        let source = range_pkg(&[("creep.attack.range", bad)]);
        let err = match engine(&[("reach", source)], false) {
            Err(err) => err,
            Ok(_) => panic!("越界覆盖 {bad} 必须在冻结时被拒绝"),
        };
        let message = err.to_string();
        assert!(
            message.contains("creep.attack.range") && message.contains("越界"),
            "越界 {bad} 须点名规则与原因：{message}"
        );
    }
    Ok(())
}
