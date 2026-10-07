# 玩家预检规则

玩家预检规则是**规则驱动双端同源**的契约族：一条内建规则既决定 Rust 结算的判定值，也以玩家全局常量的形式进入玩家 API 预检，两端在装配期从同一冻结计划取值。已开放：战斗射程四条（G07.a）、扩展意图压制声明（G07.b，见[可扩展意图盘点](#可扩展意图盘点g07b)）；EXECUTION_ORDER 策略化与参数清洗策略仍是后续子片，契约按本文件演进。

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
- 覆盖只影响射程判定，不改变 vanilla 动作互斥（硬编码压制表保持官方行为）、延迟伤害落地（对象 tick）与事件形状；扩展意图的压制声明面自 G07.b 起开放（见下节）。

## 可扩展意图盘点（G07.b）

预检契约族四项的现状与去向。共同不变量：策略只能经 `freeze → plan_hash → 装配快照` 单通道生效——绕过计划身份直改引擎常量会造成检查点/回放静默漂移，被排除。

- **动作互斥**：vanilla 侧是硬编码压制表（`kk-engine/src/intent.rs` 的 `suppressed_by` + `Action::executes` 存在性判定，结算期静默跳过、玩家 API 记录即返 OK——官方同形；`EXECUTION_ORDER` 为编译期 23 动作常量，唯一消费点在 `settle/creep/mod.rs`）。**开放面**：Mod 扩展意图可声明压制——`api.intents.define(..., { suppressedBy: ['<同接收者动作键>'] })`；冻结校验要求压制动作同 scope 同接收者且在计划内（未知键拒；**互压环拒绝**——A 压 B、B 压 A 在存在性判定下齐发会双双静默丢弃，vanilla 压制表是严格优先级 DAG，环不属于同语义开放面）；链式压制（A 压 B、B 压 C）允许，逐环独立存在性判定——压制者的键存在即压制，不看压制者自身是否执行。引擎在 ext 派发点按**键存在性**判定（同接收者同 tick，只看存在不看压制者自身成败——其参数越界被丢弃仍压制；跨玩家/跨对象的意图表天然隔离），与 vanilla `Action::executes` 同语义。压制与 Declined（进槽后处理器拒绝）是两条独立通道。描述符在 contributions 内，plan_hash/检查点等值/回放段身份自动覆盖。vanilla 压制表与 EXECUTION_ORDER 本身的策略化需要键引用型计划新形态（现规则域只有数值），未开放。
- **EXECUTION_ORDER**：编译期常量、默认与官方一致（测试采样钉死 drop 最先、harvest<say<suicide 位次与压制关系，非 23 位全序逐位断言）。策略化会改变结算序=改变 Mod 处理器调用序=G22.b 抽值 index 分配，且需新的计划数据形态——列为独立子片（G07.c 候选），先出设计再动。
- **参数策略**：清洗层是并集字段读取+JS 变换模拟（`sanitize_object_intent`），执行层校验硬编码；**参数表示检查保持原生**（边界不变）。`ArgumentConversion::Native(Key)`（Mod 意图复用 vanilla 清洗变换）未接线，声明即拒。
- **可扩展意图的可执行范围**（声明生成与引擎装配共用的 `intent_gate` 口径）：对象域 / `Merge::Last` / 严格参数转换 / 玩家授权 / Mod 类型接收者 / 压制可声明（G07.b 起）。未接线形态（声明即拒）：非对象域（房间/玩家/世界）、`Merge::Queue`、非严格转换、`ServerOnly` 授权、内建类型接收者（如 `builtin:link`——freeze/player_surface 能通过，但 ext 派发只认 Mod 对象，装配门显式拒绝而非留作静默死码）。既成事实：ext 派发序=BTreeMap 键字典序，且在每个玩家的 vanilla 动作之后按对象序执行。

## 验收

引擎（默认≡硬编码、扩程命中、零程拒绝、heal/rangedHeal 独立、mass 面积不受影响）见 `settle_tests/combat_ranges.rs`；runner 常量默认/覆盖平铺与四门读常量见 `sandbox_tests`；运行时端到端（常量注入、门与结算同源、越界拒绝、mass 排除）见 `mod_chain_tests/combat_ranges.rs`；reach/medic 两独立包 + 原生 + 组合四场景 × JSON/KKPV 双通道与检查点冷恢复见 [sim-mod-combat-ranges](../../compat/fixtures/sim-mod-combat-ranges/README.md)。证据见[交付记录](../research/notes/g07a-combat-ranges-2026-10-06.md)。压制（G07.b）：引擎语义矩阵（齐发/单发/存在性/跨对象/链式逐环/跨玩家隔离）见 `settle_tests/c3_ext_intent_family/suppression.rs`，冻结校验（同接收者/互压环）见 kk-mod-api freeze 用例，转换形态（非数组/非字符串/null 同缺省）见 kk-mod-host register 用例；SDK 声明→冻结→执行→冷恢复见 `kk-runtime` c3 压制族；suppress/free 独立包五场景（含组合选择）× 双通道 × 冷恢复见 [sim-mod-intent-suppression](../../compat/fixtures/sim-mod-intent-suppression/README.md)（原生对照由 c3「无 Mod 意图分片不装配发射原语」用例承担），证据见[交付记录](../research/notes/g07b-ext-intent-suppression-2026-10-07.md)。

## 移动静态放行与玩家预检边界

G02.d1 的 [静态移动放行](mod-move-precheck.md)在结算请求登记及仲裁同源生效。玩家 `move(direction)` 原本不预检墙和结构，正常身体/疲劳时返回 OK 并进入该策略。`moveTo` 与 PathFinder 仍按原生障碍寻路，不会自动规划穿墙；疲劳/MOVE 的玩家门仍需 G02.d2 盘点并贯通，不可据结算侧放行声明完整玩家预检已交付。
