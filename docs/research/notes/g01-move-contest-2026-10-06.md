# G01 移动竞争优先级策略交付记录（2026-10-06）

基线 `8e28ac4`（功能）之上实施；本批交付 `builtin:room.moveContest` 房间级槽、竞争输入、优先级候选与排序接线，契约见[JS 移动竞争优先级策略](../../architecture/mod-move-contest.md)。

## 交付内容

- **mod-api**：`contest.rs` 有界输入（`MoveContestInput`/`ContestCell`/`ContenderInfo`，含默认四键评分要素；`MAX_PRIORITY` = 2^53−1）。`builtin.rs` 新增槽描述（`Stage::Movement` 首次接线）与 `room.moveContest.native` 绑定，内建槽 14→15，builtin revision v7→v8。
- **引擎**：`Movement::check_impl` 排序键前置优先级（缺项 0），平手回落默认四键与登记序；`Movement::contest_input`/`has_contested_cells` 从原始登记矩阵构造全量竞争输入（不截断）；`settle/contest.rs` 的 `MoveContest` 候选（id 白名单 + ±2^53−1 域校验）与 `arbitrate_room` 编排；`ContributionDispatch` 增加 `ContestSlot`（Native 直调 / Hosted 整槽）并接入两条装配路径与 install 清单；`room_loop` 仲裁点改为释放借用后经贡献面。
- **JS 宿主**：ChainHost 新增 `contest_priority`/`contest_set_priority` 默认拒绝实现；`chain/contest.rs` op 保留浮点整值与域校验、token/binding 归属与故障分类；sdk.js 注入 `ctx.contest`（getPriority/setPriority，JS 侧 `Number.isSafeInteger` 预检）；execution_plan 允许本槽正常拒绝。
- **runtime**：`SlotCall::Contest` 整房一次（无对象目标，占位 id 仅诊断用），native 中心为无操作；staged state 暂存/移交复用受击槽路径，Declined 只丢弃优先级与暂存 state。
- **夹具**：`compat/fixtures/mod-move-policy/{seniority,heaviest}` 两个独立公共 SDK 包；`compat/fixtures/sim-mod-move-policy` 四场景（plain/seniority/heaviest/combined）+ README。

## 行为验收

- 场景：`mid(work,move)`、`light(move,move)`、`heavy(work,work,move)` 同 tick 挤 `(10,11)`。默认 light 胜（功率比 2.0）；seniority（id 最小 +1/其余 −1）mid 胜；heaviest（after 抬 +2）heavy 胜；组合 heavy 胜——三种策略三种胜者，组合语义独立可观察。失败者不移动（无递补）。
- `kk-cli sim_tests/move_policy.rs`：四场景 × JSON/KKPV 完整 final 全等（排除 CPU 桶）、PlanHash 四身份区分；第 0 tick（竞争已结算、观察未写）保存退出码 2，加载续跑 final 等价、namespace `applied` 恰一次。
- `kk-runtime mod_chain_tests/move_contest.rs`（7 用例）：输入完整性与评分要素；优先级重排；decline 回退默认且丢暂存 state；非法数值/外部 ID/越权能力不可被 decline 掩盖（世界不变）；竞争能力在动作槽不可用；负优先级低于默认层；故障停用后健康钩子 + native 回退；晚期 RawMemory 写回失败经缓存重试恰一次提交。
- `kk-engine` 单测：竞争输入登记序与要素、空输入无候选、候选域/白名单、优先级排序与平手回落（`settle/contest/tests.rs`）；既有测试无回归（`cargo test -p kk-engine` 全部通过，workspace 全量 955 通过/12 既有忽略，见下方复审节）。

## G30.d 同功能默认参考（微基准）

`kk-engine` `movement_contest.rs::movement_benchmark_native_arbitration`：8 竞争格 × 每格 4 竞争者，release、2000 次迭代的 `check_impl` 每房耗时（M1 arm64，cargo test --release -- --nocapture）：

| 时点 | 样本（µs/房） |
|---|---|
| 改造前（8e28ac4 + 基准文件） | 4.56（冷）、1.65、1.61、1.61 |
| 改造后（本批，空优先级表路径） | 4.87（冷）、1.61、2.44、1.75 |

结论：默认路径（无 JS、无竞争槽贡献）不进入 realm、不构造输入；排序键前置的空表查找在测量噪音之内，无可测量回归。JS 策略与组合的端到端开销由 Mod VM 结算预算统一管控；密集竞争场景的原生/JS/组合三态对照与 `mod-perf-probe` 移动模式随 G03/G04 或 G30.d 滚动补测，本批不宣称已量化 JS 开销。

## G30.a 移动领域覆盖清单（对照本地 xxscreeps `5ff0d1a6`）

对照源：`packages/xxscreeps/engine/processor/movement.ts`（announce/dispatch：power 排序 + 随机平手 + 递归让位 resolver）、`engine/processor/intents/creeps/tick.js`（执行）、`game/pathfinder`（native 寻路）。本仓仲裁模型（上游 intents/movement.js 的矩阵评分选胜 + 阻塞链取消）与 xxscreeps 主干 dispatch 模型结构不同、语义对应，按能力登记：

| 能力 | 本仓状态 | 位置 / 任务 |
|---|---|---|
| move 意图登记（方向/相邻目标/follow） | 已实现 native + JS 改向拒绝槽 | `settle/creep/movement.rs`、[creep-move-slot](../../architecture/creep-move-slot.md) |
| pull/pulled 登记 | 已实现 native；JS 槽未开放 | `Movement::add_pulling`；G03 |
| 竞争排序（power/priority） | **本批开放 JS**（优先级层）；xxscreeps 随机平手未实现（本仓平手=登记序） | [mod-move-contest](../../architecture/mod-move-contest.md)；随机平手如需另行子片 |
| 仲裁选胜后阻塞链取消 | 已实现 native；获准集合替换未开放 | `remove_from_matrix`；G03 |
| 递归让位/交换/循环/拉拽联动（xxscreeps resolver） | 未开放 | G03 |
| 准入（疲劳门槛、MOVE 部件、地形/结构/rampart/安全模式） | 已实现 native；策略面未开放 | `can_move`、`check_obstacle_at_xy`、`has_static_obstacle`；G02（含执行阶段 `can_move` 复核同源） |
| 位移执行/疲劳生成恢复/道路磨损/工地踩踏/跨房过渡 | 已实现 native；策略面未开放 | `movement_execute`、`creep_tick_prepare`；G04 |
| 寻路（CostCallback/原生搜索） | 已实现玩家面（kk-pathfinder） | `crates/kk-pathfinder`；地形代价策略随 G04/G21 |

缺口登记：随机平手打破、让位链 JS 替换、准入与代价策略、pull 策略、寻路代价矩阵 JS 化均未实现，分别由 G02/G03/G04 及后续子片承接，不因本批排序开放而宣称移动域完成。

## 复审与已知限制

三路只读 subagent 复审（仲裁正确性/恢复路径、能力面/数值边界、模块规约/文档一致性；期间 workspace `cargo test --no-fail-fast` 955 通过/1 失败（冻结槽计数 14，见下）/12 既有忽略，该失败项修复后全绿；最终 `pnpm run check` 955 通过/0 失败/12 既有忽略）：仲裁语义、完整原始矩阵评分、decline/fault 传播、RefCell 借用、装配清单四处一致、数值域三道门一致、sdk 注入同型等方面核验无问题；无中/阻断级发现。低级与信息级发现及清偿：

- `kk-mod-api` 冻结目录测试断言 `plan.slots().len()==14` 未随槽 15 同步（阻断 workspace 测试）——已改 15。
- 基准测试缺 `#[ignore]`：`--ignored` 命令会匹配 0 个测试假通过、release 套件每次跑 2000 次迭代——已按仓库惯例补 `#[ignore]` 注明手动命令。
- `reciprocal`/`pulled`/`originRequestCount` 只有负向断言——已补对穿（reciprocal=true 且默认第一键 100 胜出、originRequestCount 数值）与"拉着走"（被拉者第二键胜出、拉者同时让位不被判障碍）两个正向用例，e2e 类型检查补 `movePower`/`originRequestCount`/`originX/Y`/`pulling`。
- `MoveContest::new` 对不可解析 id 曾静默剔除白名单（隐式不变量）——改为构造期故障（fail-fast），签名 `Result<Option<_>>`。
- 竞争槽写 `target.fields` 会落入 link 写面报误导文案——已在 `write_fields` 提前显式拒绝；decline 门控错误文案补齐受击/移动竞争两类。
- 文档 `ctx.target` 措辞精确为"fields 恒 null、id 为诊断占位"。
- 信息级不修改：占位 id `u64::MAX` 与 ID 分配游标的理论碰撞（需 2^64 次分配，不可达，写面不受影响）。

已知限制：竞争槽输入只含竞争格（整房请求视野属 G03）；JS 开销未单独量化（见 G30.d 节）；xxscreeps 的随机平手语义未提供（登记序平手是本仓默认兼容行为）。
