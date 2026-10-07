# G02.c 意图静态预检槽交付记录（2026-10-06，`0f390b5`）

## 范围

G02 收尾子切片：新增房间级槽 `builtin:room.movePrecheck`（内建槽 17→18、revision `…-obstacle-precheck-descriptors-v11`；catalog 槽计数断言同步）。**请求级单向收紧**：全部意图与移动动作槽登记完成后、竞争输入构造之前（`arbitrate_room` 阶段 0）整房一次进 JS；`ctx.precheck.block(id)` 否决请求，被否决请求经 `Movement::apply_precheck_blocks` → `remove_requests` 在竞争输入构造之前移除，**等同从未登记**——不参与竞争评分、不让位、不占格（spawn 放置的 `is_tile_busy` 与准入槽整房请求视野均不含）。放行方向不开放：意图期放行语义惰性（仲裁期 `check_obstacle_at_xy` 对结构/私有 rampart/墙地形一律复检），与障碍槽口径一致。原生 `has_static_obstacle`（`intent_move` / `slot_request_direction` 两个登记入口共享）保持 native 并在登记时点先行生效——被原生挡下的请求不入矩阵、不进槽输入；两个入口的请求汇入同一矩阵，槽在矩阵上统一否决，同源成立。现行契约见[JS 移动预检](../../architecture/movement.md#请求预检)。

## 关键设计判断

- **请求级 ≠ 胜者级**：预检否决强竞争者的**请求**使其不参与竞争，同格弱者独占目的格胜出并移动；障碍槽（G02.b）挡下的是**胜者**，之后无人可动。两级互补可组合——courtesy 夹具的强弱竞争场景即此差异的可观测演示。
- **否决不动 pull 配对**：拉者请求被否决后 `pull_of/pulled_by` 保留，被拉者仍以有效被拉身份通过准入门槛，但拉者不再让位，链内取消——与"拉者发出 pull 但从未登记移动请求"的基线逐步一致（引擎测试逐步对照）。
- `old_fatigue` 残项仅被胜者读取（`unwrap_or(0)` fail-closed），否决后无害。

## 同批接线清单

kk-mod-api：`precheck.rs` 输入结构、`builtin.rs`（槽常量/SLOT_KEYS 18/原生绑定/描述符/revision v11）、catalog 断言 18。kk-engine：`settle/precheck.rs` 候选、`movement.rs` `precheck_input`/`apply_precheck_blocks`、`working.rs` 门面、`contrib/precheck.rs` 槽、contrib/assemble 两条装配路径、`contest.rs` 阶段 0、`install.rs` NATIVE 清单、`SlotCall::Precheck`。kk-mod-host：`chain/precheck.rs` op、ChainHost 默认方法、realm 注册、sdk.js `ctx.precheck` 工厂（simple/inner 两处挂载）、`execution_plan.rs` decline_allowed。kk-runtime：`NativeCall::Precheck`、SlotCall 装配与 args 序列化、`precheck_slot` 目标守卫（房间级槽 null fields）、decline 局部恢复边界、ChainHost 桥接。

## 机器证据

- **引擎单测**（`kk-engine/src/settle/precheck/tests.rs` 4 测试全过）：
  - `precheck_input_lists_requests_in_id_order_with_context`：对象 ID 升序、要素完整（功率/重量/owner/地形标志/占位摘要）、防御值。
  - `blocked_request_never_contests_and_rival_takes_cell`：**请求级语义核心**——否决 strong 后 weak 独占 (40,19) 胜出（原生基线 strong 胜出对照）；准入整房视野不含被否决请求。
  - `blocked_puller_breaks_pulled_chain_at_arbitration`：拉者被否决 → 被拉者门槛仍豁免 → 链内取消，与"拉者从未登记请求"基线 winners 逐步相等。
  - `candidate_validates_domain_and_empty_input`：域外 ID 拒写、快照、空输入不构造候选。
- **运行时链路**（`kk-runtime/tests/mod_chain_tests/move_precheck.rs`，4 + 复审清偿 4 共 8 测试全过）：字段形态 JS 通道校验 + 否决后弱者夺格 + 无请求 tick 不进 JS；decline 整房回退原生（state 不提交）；fenced replace + courtesy after 组合（after 经 `isBlocked` 感知 replace 否决，并集生效，双 namespace 恰计一次）；`ctx.precheck` 在准入/障碍槽调用走能力门故障、不可被 catch+decline 掩盖、世界不变。
- **场景夹具**（[sim-mod-move-precheck](../../../compat/fixtures/sim-mod-move-precheck/README.md)）：plain（原生 strong 胜出、w1 进西区）、fenced（w1 原地）、courtesy（strong 原地、weak 独占竞争格 `(40,19)`）、combined（并集）；四场景 JSON/KKPV 双通道完整 final 一致、四 plan_hash 身份互异（内容选择与组合身份分别冻结；17→18/revision v11 的直接证据是 freeze 槽计数断言与 revision 串本身）；第 1 tick 后保存检查点退出（exit 2）→ 加载续跑（exit 0）final 与一次性参考一致。CLI `sim_tests/move_precheck.rs` 2 测试全过。
- **typegen 新鲜度不受影响**：`mod_types_tests`（3）过——玩家面类型不暴露内建槽清单。
- 回归：kk-engine 285+78、kk-runtime 225+35、kk-mod-host/api 全过。

## 复审与清偿

三路只读复审（正确性/一致性、权限/故障恢复、验收/文档——评审维度纳入 100a859 修复批教训）结论：**无高危逻辑缺陷**，"否决等同从未登记"经逐下游读取点核对成立（竞争评分/预填/配对复核/准入视野/障碍输入/链预演/执行/spawn 放置/old_fatigue/pull 残标全部与基线等价）；能力门、decline 面、故障恢复、检查点身份与 admission/obstacle 先例逐点同构。清偿记录：

- 【C·门禁】movement.rs 达 1010 行超 1000 门槛——按职责拆分：五处槽输入投影（precheck/contest/contender/admission/obstacle 的 input 构造）移入 `settle/movement/inputs.rs` 子模块（363 行），movement.rs 回到 667 行聚焦仲裁状态机；公开 API 与调用方不变。
- 【A·中级】预检槽缺 staged_states：before/wrap 层的 ctx.state 在中心 decline 路径直接提交，与文档"丢弃暂存 state"承诺不符——条件补 `precheck.is_some()`（一行），并新增 `precheck_decline_discards_before_layer_state` 回归。
- 【B·中级】夹具目录命中 `.gitignore` 的 `compat/fixtures/*`——按先例补两条 `!` 白名单（`mod-move-precheck/`、`sim-mod-move-precheck/`），`git check-ignore` 验证不再被吞。
- 【C·中级】移动槽通道（`slot_request_direction`）×预检的同源组合无可观测证据——新增 `precheck_blocks_movement_slot_channel_request`：after 层经 `ctx.movement.request` 改向（before 会被后到的 native 登记覆盖，坑已注记），预检包否决该通道请求，两态恰计一次。
- 【C·中级】预检×障碍槽同房两级组合无测试——新增 `precheck_and_obstacle_compose_in_same_room`：预检否决弱者请求 + 障碍挡胜者 → 目的格无人进入。
- 【C·中级】故障停用回退与晚期重试无预检专项——新增 `precheck_fault_disable_falls_back_and_state_commits_once`（停用后 after 继续组合；RawMemory 写回失败经缓存重试恰一次提交，双通道）。
- 【C·中级】mod-move-contest.md 槽计数史实 16→15（G01 批 14→15）——已改。
- 【C·中级】"17→18 PlanHash 变更实证"措辞强于证据——四互异 hash 只证明内容选择敏感；改为 freeze 断言+revision 串为直接证据（研究记录与夹具 README 同步）。
- 【C·中级+L】"is_tile_busy 不占"的引擎断言恒真——重写为选胜前的矩阵断言（request_cell_of 为 None、竞争格恰剩唯一登记者）。
- 【A/B·低】write_fields 缺预检槽守卫（误导文案）——补显式分支。
- 【C·低】mod-move-precheck.md 首段管线顺序写反（竞争在前）——改为 预检→竞争；检查点拒绝机制表述改为"经可执行文件与包身份间接反映"。
- 【A·建议】TERRAIN_MASK_SWAMP 全限定与导入混用——统一为导入常量。
- 【B·建议】停用槽路径仍为输入构造付费、by_mover 聚合的单请求不变量假设——与 contest/obstacle 既有设计同形，注记保留（G03/G04 开放多请求时重审）。

## 剩余

G02 父任务收口：障碍/预检放行方向按宿主约束或语义惰性关闭（不再挂起）、孵化中门槛归 G13 领域、障碍摘要的安全模式主人列为可解释性增强另登记。玩家面 `move()` 返回值的预检策略属 G07 玩家预检族，另行盘点。G03 剩余（pull 登记策略、障碍槽预演）与 G04 按原队列。
