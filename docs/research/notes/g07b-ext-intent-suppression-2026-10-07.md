# G07.b ext 意图压制接线与可扩展意图盘点——登记与交付记录

## 前置审计结论（2026-10-07，两路后台只读 Explore）

**引擎侧（意图清洗与结算结构）**：

- 清洗器单文件 `kk-engine/src/intent.rs`；`EXECUTION_ORDER` 是编译期硬编码 `pub const`（23 个 creep 动作，intent.rs:216-240，来源上游 creepActions），唯一消费点 `settle/creep/mod.rs:60-63`；不进任何计划/规则/身份域。同 tick 多意图=清洗期同键 last-wins+不同键全量保留，结算期按固定序逐个尝试；互斥在结算期判定。
- 互斥=硬编码压制表 `suppressed_by`（intent.rs:161-205）+ `Action::executes` 存在性判定（intent.rs:207-210，只看存在不看成败）；压制静默跳过无拒绝事件、玩家 API 记录即返 OK（官方同形）。Declined 与压制是两条独立通道（Declined=进槽后处理器拒绝；压制=结算前不进）。
- **ext 意图半成品**：`IntentDescriptor.suppressed_by`（definition.rs:212）已声明；冻结校验（同 scope+同接收者、未知压制动作拒，freeze.rs:279-292）已实现；执行装配门 `executable_gate` 把非空 suppressed_by 一律拒（intent_gate.rs:41-46，文案「压制判定的执行装配未接线」）；ext 派发点 `settle/mod_intents.rs` 的 `process_object_ext_intents` 对 list 内全部键逐一执行（BTreeMap 键字典序，room_loop.rs:145-154 在 vanilla 之后按对象序派发）——**声明面齐、执行差一步，是现成最小切片**。
- 身份交互：意图描述符在 contributions → plan_hash（freeze.rs:548-554 canonical_hash）→ 检查点等值门与回放段身份自动覆盖；策略化只能走 freeze→plan_hash→from_plan 快照单通道。EXECUTION_ORDER/互斥若策略化需**键引用型计划新形态**（RuleDescriptor 只有数值域 definition.rs:221-227）；顺序变化会改 Mod 处理器调用序→G22.b 抽值 index 分配（跨计划对比必须意识到）。
- 测试先例：`combat_ranges.rs` 的计划驱动快照注入模板（combat_of/combat_plane/settle_with_plane）；压制行为测试先例 controller_safemode.rs:526；c3_ext_intent_family/installation.rs:195-225 门矩阵（含「压制关系」拒绝断言，接线时翻转）；intent/tests.rs:23-39 executes 判定、from_json_tests.rs:285-289 官方位次钉死。

**玩家面与契约**：

- player-precheck-rules.md 仅一句总承诺（:3 四项为 G07 后续子片）；:29「参数表示检查保持原生」、:31「不改变动作互斥」为显式边界句，G07.b 开放 ext 压制须改写 :31 口径（vanilla 表不变、ext 声明面开放）；:14 同源不变量（「不会玩家面放行结算拒绝或反向撕裂」）是四项共同验收母句。
- 玩家侧完全感知不到顺序/互斥（无 EXECUTION_ORDER 类常量、记录器只 last-wins+计费、无冲突报错）；「API 返 OK≠执行」只写在 Rust 注释。sdk/rules.js installRules 值仅限有限数（:15）——顺序/互斥表是非数值，现通道装不下（键引用形态缺口与引擎侧结论互证）。
- 常量流程五件套（constants.json→gen-constants→kk-core 键数门 427→catalog 逐键→prelude C 表+setup 双通道）；typegen 无 constants 引用（G07.a 核实），但 **intent_gate 是 typegen 与装配共用门**——放開 suppressed_by 后 typegen 侧同口径放开；typegen.rs 零 suppressed_by 引用（实测），.d.ts 产物不变，`--check` 字节门不红。
- 设计承诺主句：台账 :168「清洗器的 EXECUTION_ORDER、动作互斥、参数策略和可扩展意图也要逐项盘点，不能只改 API 返回值」+ :102「G07 负责通用化」；mod-complete-gameplay :88「默认玩家预检与动作互斥仍生效，其策略替换…继续登记未完成」。
- 文档挂点：player-precheck-rules.md（盘点篇+:3/:29/:31）+ 三索引 + note + 台账；条件性五槽文档注记（creep-combat-slot.md:3,7 等「原 EXECUTION_ORDER 保持生效」句——本批不动 vanilla 表，仅补 ext 开放注记）；state-inventory/checkpoints/mod-types 均不需要（无新持久域、typegen 不枚举常量）。

## 范围拍板（登记定案，详见台账当前任务行）

① ext `suppressed_by` 执行接线（装配门删拒绝臂+派发点存在性判定，vanilla `Action::executes` 同语义）；② player-precheck-rules.md 可扩展意图盘点篇（四项现状+缺口清单+ext 派发序既成事实）。不做：EXECUTION_ORDER 策略化（G07.c 候选+设计前置）、vanilla 压制表策略化、参数策略/Native 接线、射程族第二片（领域首片职责）、玩家只读常量暴露（破坏同源不变量）。

## 验收计划

- kk-mod-api：freeze 既有压制校验测试（未知压制动作/非同接收者）保持绿；executable_gate 放开后 typegen/装配同口径（gate 单测同步）。
- kk-engine：c3 门矩阵「压制关系」断言翻转（同接收者双意图+压制关系=装配通过）；ext 压制行为测试（齐发只执行未压制者/压制者缺席则执行/压制只看存在不看成败）。
- kk-runtime：声明压制的独立包（fixture）+原生+组合×JSON/KKPV 双通道×第 2 tick 冷恢复。
- kk-cli sim：夹具场景端到端（压制生效的世界级可观测差异）。
- 文档：player-precheck-rules.md 盘点篇+三索引+本 note；typegen `--check` 不红。

## 交付记录

（实现完成后回填。）
