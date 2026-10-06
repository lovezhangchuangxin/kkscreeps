# 玩家预检规则

玩家预检规则是**规则驱动双端同源**的契约族：一条内建规则既决定 Rust 结算的判定值，也以玩家全局常量的形式进入玩家 API 预检，两端在装配期从同一冻结计划取值。首篇开放战斗射程四条（G07.a）；EXECUTION_ORDER、动作互斥、参数清洗策略与可扩展意图的预检盘点是 G07 后续子片，契约按本文件演进。

## 通用契约

规则的生效路径分四段，各段只做一次解析或读取：

1. **声明**：内建贡献（`builtin.rs` 的 rules）声明规则键、数值域与默认值。默认值与玩家常量表（`kk-runner/assets/constants.json`）同源锁定——`kk-mod-api` 的 freeze catalog 测试逐键比对，常量表经 `scripts/gen-constants.mjs` 生成 `kk-core` 常量。注意常量表是「上游 dump + 本地新增键」的混合：射程四键为本仓库新增（上游 `common/lib/constants.js` 无此名字），重新 dump 会丢键——由 kk-core 键数门、catalog 比对与 `c::*_RANGE` 编译期引用三道门拦住。Mod 用 `api.rules.override('builtin:<键>', 值)` 覆盖，值域外的覆盖在冻结时拒绝。
2. **装配快照**：引擎装配期（`install_plan_with_ext`/`install_hosted_plan`）以 `from_plan` 把有效规则解析成 `Copy` 快照，存于引擎并随每房每 tick 进入 `Plane`；结算热路径按字段读取，不再触碰键解析。无计划的默认引擎用 `kk-world::place::builtin_*` 一次性冻结默认计划取同一快照。
3. **结算消费**：意图处理器读 `w.plane.<快照>` 判定，替代原先的硬编码常量；默认值与官方行为逐位等价（引擎测试钉死默认臂）。
4. **玩家面注入**：`kk-runtime` 装配时把同一快照写入 `constant_overrides`（canary 预检沙箱与真实玩家沙箱两条通道都注入），`constants_script` 校验键在常量表后平铺到玩家全局。玩家 API 的 prelude 常量表（`C`）在安装期捕获生效值，预检门读常量而非字面量。

同源不变量：**同一生效值驱动门与结算**。不会出现"玩家面放行但结算拒绝"或反向撕裂——两端的值都出自同一冻结计划快照，检查点恢复后计划指纹（plan_hash）等值核对继续兜底。玩家自己覆写 `globalThis` 常量不影响 API 判定（C 表安装期捕获，与官方 driver.constants 注入同形）。

## 战斗射程（首篇）

四条单目标射程规则，值域 0..=50（0 即实效禁用该档射程；50 为房宽上界——宿主表示约束）：

| 规则 | 默认 | 结算消费 | 玩家常量 | prelude 门 |
|---|---|---|---|---|
| `builtin:creep.attack.range` | 1 | `intent_attack` 切比雪夫判定 | `ATTACK_RANGE` | `attack` 的 `inRangeTo` 门 |
| `builtin:creep.rangedAttack.range` | 3 | `intent_ranged_attack` | `RANGED_ATTACK_RANGE` | `rangedAttack` |
| `builtin:creep.heal.range` | 1 | `intent_heal`（非远程档） | `HEAL_RANGE` | `heal` |
| `builtin:creep.rangedHeal.range` | 3 | `intent_heal`（远程档） | `RANGED_HEAL_RANGE` | `rangedHeal` |

边界：

- **不在规则面内**：身份/归属/安全模式/参数表示检查保持原生；`rangedMassAttack` 是三格面积语义（距离倍率 1/0.4/0.1），不随 `rangedAttack.range` 变化（引擎与运行时测试钉死）；塔、power creep 等其他射程族未开放。
- 近战两门原为 `isNearTo`（等价 `inRangeTo(·,1)`），首片一并改读常量——默认行为不变，覆盖后语义连续；门保持官方接收者形态（`target.pos.inRangeTo(this.pos, C.*)`）。
- 覆盖只影响射程判定，不改变动作互斥、延迟伤害落地（对象 tick）与事件形状。

## 验收

引擎（默认≡硬编码、扩程命中、零程拒绝、heal/rangedHeal 独立、mass 面积不受影响）见 `settle_tests/combat_ranges.rs`；runner 常量默认/覆盖平铺与四门读常量见 `sandbox_tests`；运行时端到端（常量注入、门与结算同源、越界拒绝、mass 排除）见 `mod_chain_tests/combat_ranges.rs`；reach/medic 两独立包 + 原生 + 组合四场景 × JSON/KKPV 双通道与检查点冷恢复见 [sim-mod-combat-ranges](../../compat/fixtures/sim-mod-combat-ranges/README.md)。证据见[交付记录](../research/notes/g07a-combat-ranges-2026-10-06.md)。
