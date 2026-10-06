# G07.a 战斗射程规则包

两个互不依赖的独立包，各自只覆盖内建射程规则（无处理器、无状态提交）：

- **reach**（`combat-ranges-reach`）：`builtin:creep.attack.range` 1→3——近战攻击距离放宽到 3。
- **medic**（`combat-ranges-medic`）：`builtin:creep.heal.range` 1→3、`builtin:creep.rangedHeal.range` 3→5——治疗族射程独立放宽。

覆盖值经冻结计划同时驱动 Rust 结算距离与玩家全局常量（`ATTACK_RANGE` 等），场景用法见 [../sim-mod-combat-ranges/](../sim-mod-combat-ranges/)。
