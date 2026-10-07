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

（2026-10-07 实现完成后回填；提交 ID 见 git log。）

实现落点：

- `kk-mod-api/src/intent_gate.rs`：删除「压制关系」拒绝臂（typegen 与装配共用门同口径放开，压制自本批可声明可执行）；typegen.rs 零 suppressed_by 引用，.d.ts 产物与 `--check` 字节门不变（mod-types.md:14 门描述同步）。
- `kk-engine/src/settle/mod_intents.rs`：ext 派发循环在 builtin 槽防御跳过之后加存在性压制判定——压制者键存在于本对象意图表即 `continue`；与 vanilla `Action::executes` 同语义（只看存在不看压制者成败）。
- SDK 面：`kk-mod-host/src/sdk/registration.js` 的 `intents.define` 暴露 `suppressedBy`（字符串数组）；`kk-mod-host/src/convert.rs` 解析为 `BTreeSet<Key>`（any_key 支持跨命名空间压制者，非数组/非字符串元素拒绝；语义校验仍在冻结期）。
- 测试：kk-engine `c3_ext_intent_family/suppression.rs` 四用例（齐发只执行压制者/压制者缺席则执行/存在性语义——压制者参数越界被丢弃仍压制/跨对象意图表隔离）+ 门矩阵「压制关系」断言翻转为装配通过（带 noop process 越过处理器存在性检查）；kk-runtime c3 压制族两用例（SDK suppressedBy 端到端 × JSON/KKPV 双通道 × 带包冷恢复；未知压制动作经 SDK 面拒绝注册）；kk-cli `sim_tests/intent_suppression.rs` 两用例（四场景 × 双通道 × 第 2 tick 检查点冷恢复续跑等值 + suppress 三场景同计划身份/free 独立身份；期望值逐场景表）。
- 夹具：`compat/fixtures/mod-intent-suppression/{suppressed,free}`（suppress:bump 置 amount + suppress:mark 置 mark 且声明被 bump 压制；free 同形无压制——两意图都设绝对值使期望每 tick 稳定）与 `compat/fixtures/sim-mod-intent-suppression/`（both=3/alone=7/invalid=0/free=7 四场景 + main.js 按 memory mode/prefix 发射）。
- 文档：player-precheck-rules.md 新「可扩展意图盘点（G07.b）」节（四项现状：互斥开放面+vanilla 保持、EXECUTION_ORDER 编译期常量+策略化耦合 G22.b 抽值序列 G07.c 设计前置、参数策略边界不变+Native 未接线、可执行五门+缺口清单）+ :3 导语与 :31 边界句口径更新 + 验收节补压制验收；mod-types.md:14 门描述；architecture README 索引行；research README 索引。

实现期修正：夹具首版 increment 用累加语义——both 场景每 tick +3 期望不稳定（实测 2 tick 得 6）；改为两意图都设绝对值（bump/mark）后四场景期望 3/7/0/7 每 tick 稳定。门矩阵翻转用例初版沿用共享 install 闭包（不带 ext 处理器）——门放行后在处理器存在性检查处失败，改为内联提供 noop process。

复审清偿期增补（详见下节记录）：executable_gate 补「内建类型接收者」拒绝（P1）；freeze 补互压环拒绝；convert 对 suppressedBy:null 与缺省同判；测试补 convert 形态三路、freeze 同接收者/环、引擎链式逐环与跨玩家隔离、门矩阵 builtin 接收者与 Queue 优先级、runtime 冷恢复断言改顺序敏感+发射原语返 true、sim 组合选择场景（第五场景，suppress/free 同计划共存）与夹具 README。

验收：完整 `pnpm run check` 见收口提交。

## 复审记录（2026-10-07，三路后台只读复审，全部清偿）

分工：协议与语义正确性 / 测试充分性与边界 / 文档与台账一致性。结论：0 P0；5 P1 全部修复；P2 逐条处置如下。

**P1 清偿**：

1. 盘点篇把「builtin 对象上的扩展意图」写成「声明即拒」与实现相反（freeze/player_surface 能通过，派发静默死码）→ 双修：executable_gate 补内建类型接收者拒绝（fail-loud 优于死码，正合该门「拦截未接线形态」职责；同族边——Mod 意图声明 `suppressedBy` 指向 builtin 意图——被同接收者校验+新门共同封死）+ 盘点篇五门/缺口清单改写。player_surface 既有 builtin 接收者测试只走 freeze+from_plan、不经门，不受影响。
2. 登记验收「kk-runtime 原生+组合」缩窄 → 组合补齐：sim 第五场景 combined（suppress+free 同计划、双类型对象、压制按包内声明各自生效，三计划身份互异；free 包玩家类名改 FreeCounter、查询全局改 FIND_FREE_COUNTERS 避组合撞唯一绑定）；原生臂登记偏差——压制语义无原生对照可测，由 c3「无 Mod 意图分片不装配发射原语」既有用例承担对照。
3. 台账 G07 父行滞后 → 收口提交同步（.b 交付、互斥拆「ext 声明面开放/vanilla 表未动」）。
4. convert 新增 suppressedBy 解析错误分支零直接测试 → kk-mod-host register_failures 补非数组/非字符串两案 + suppressedBy:null 注册成功用例（null 与缺省同判，见下）。
5. runtime 冷恢复断言 json!(1) 对压制丢失不敏感 → 改顺序敏感形态：恢复后发 increment(4)+reset(mark=99)——压制保持则值不变 12、丢失则 16。

**P2 清偿（修复类）**：freeze 拒互压环（存在性判定下齐发双静默丢弃，vanilla 是严格优先级 DAG）+ freeze 用例补同接收者/环两分支；convert suppressedBy:null 与缺省同判（与 queueMax/mode 容忍一致，空数组才是显式空）；typegen.rs:73 陈旧注释（压制不再是门拒绝项、补接收者域）；盘点篇「测试钉死位次」收敛为采样钉死表述、「跨接收者」改「跨对象」；引擎补链式逐环（三键链：高存在→链下全静默；高层缺席→中层执行且其存在继续压低层）与跨玩家隔离用例；门矩阵补 builtin 接收者与 Queue+压制报错优先级两案；发射原语返 true 显式断言（官方记录即 OK）；sim 测试注释意图名对齐夹具（bump/mark）；suppress-bad 错误路径临时目录清理；c3 文档注释 `*/` 残留；两夹具目录补 README（mod-intent-suppression 新增）。

**P2 处置（记录不改，附理由）**：

- 「三索引」计数：实际更新 architecture README + research README 两处；docs/README.md:20 为通用行（已含玩家预检规则链接）无需随批改——本条即澄清。
- mod-system.md:281-307 设计示例字段名与现行 SDK 漂移（targetTypes/ownership/replace-last/phase vs target/mode:last|queue）：该文档自声明「拟议公共 SDK 形状，不代表仓库已有」，属既有状态非本批引入，不在本批追改。
- 跨包压制者（freeze 目录是全计划合并、允许声明）无专门用例：SDK/convert 面已按 any_key 支持跨命名空间，语义由同接收者+存在性结构覆盖；跨包场景待真实用例出现时随批补。
- mod-intent-suppression 包目录名（suppressed/）与包 id（suppress）不一致：selection 按 id 匹配功能正确，README 已写明对应关系；目录名保留（改名会动 selection 与既有场景）。
