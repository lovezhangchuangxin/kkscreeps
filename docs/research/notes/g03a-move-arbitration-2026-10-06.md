# G03.a 整房请求视野与链预演交付记录（2026-10-06）

## 范围

G03 首个子切片：准入槽（`builtin:creep.moveAdmission`）输入扩展 + 只读链预演，**不新增槽**。三件事：

1. `ctx.args.requests`——仲裁前的完整原始请求矩阵（含竞争落选者 `won=false`），按 目的格→格内登记序 排列，逐条 `moverId/x/y/index/originX/originY/won`。原始矩阵在 `rank_winners` 以 `mem::take` 零拷贝暂存（`Movement::original_requests`）；`Working` 每房每 tick 重建（`settle/room_loop.rs::settle_room`），天然瞬态、不进检查点。
2. `movers[].pulledBy/pulling`——预填配对复核后的有效拉拽配对对象 ID（`Option<String>`，无配对 null）；布尔 `pulled` 保留。
3. `ctx.admission.evaluate()`——链预演：`Movement::simulate_survivors` 克隆自身、替换准入终值为候选快照（`MoveAdmission::finals_snapshot`）、运行与正式仲裁**完全相同**的 `cancel_blocked` 代码路径，返回幸存胜者对象 ID 升序数组。只读；调用时点障碍槽追加集必为空（槽序 admission→obstacle 固定），预演结果是最终取消结果的上界（追加只会收紧）。

获准集合选择因此闭环：暂存候选（JS 布尔终值）→ 宿主链验证（预演/正式取消同代码）→ 终值整体替换原生预填；宿主不重跑默认门槛覆盖 JS 结果。拒绝仍是整房回退原生预填（G02.a 语义），不复用单执行者边界。现行契约见[JS 移动准入策略](../../architecture/mod-move-admission.md)。

## 调用链

`sdk.js ctx.admission.evaluate()` → op `op_mod_chain_admission_evaluate`（token/binding 校验、`ModFaultKind::Read` 归类、profiler 计时）→ `ChainHost::admission_evaluate`（默认 Err 能力门）→ `ModChainContext`（kk-runtime）：`candidate.finals_snapshot()` + `ctx.borrow().movement_simulate_survivors(&finals)`（安全模式主人与 `finish_movement` 同源计算）。无越权面：其他槽调用得到"本次槽没有移动准入能力"故障，沿既有 outcome 路径不可被 catch+decline 掩盖。

## 机器证据

- **引擎单测**（`kk-engine/src/settle/admission/tests.rs`，7 旧 + 5 新函数全过，新增函数合计覆盖 11 个拓扑案）：
  - `requests_view_covers_losers_and_pull_ids`：竞争落选者可见且 `won=false`、格内登记序保留、胜者输入不含落选者、拉拽双方 ID 互见。
  - `simulate_matches_chain_on_swap_ring_chain_and_blockage`：交换（双方存活）/三环旋转（全存活）/扣住环上一员（整环级联取消为空集）/链式让位+占位第三者（占位者门槛获准但链内取消）——每个拓扑断言 预演幸存集 == 实际取消链幸存集（`assert_simulate_matches_chain` 先预演再应用同一终值比较）。
  - `simulate_matches_chain_with_overrides_and_pull_pair`：覆盖疲劳者放行后预演按覆盖终值预测；拉拽配对完好双存活、扣住拉者后被拉者级联取消。
  - `simulate_matches_chain_under_safe_mode`：安全模式主人穿过未让位敌方 creep 的分支（无安全模式双空幸存、有安全模式双存活）预演/正式一致——`Working::active_safe_mode_user` 抽取后三处（障碍输入/预演/正式链）同源。
  - `finals_snapshot_reflects_candidate_overrides`：快照含原生初始项与覆盖项、不消费候选。
- **场景夹具**（[sim-mod-move-arbitration](../../../compat/fixtures/sim-mod-move-arbitration/README.md)，六移动者：交换对 a/b、占位对 stranger/parked、竞争对 strong/weak）：
  - `scenario-plain.json` 原生：交换成立（a→(10,9)、b→(10,10)）、stranger 链内取消原地、strong 胜出移动 (30,29)、weak 落选原地。
  - `scenario-wave.json` wave 包 replace：`evaluate()` 幸存 3 < 获准 4（stranger 级联损失只有预演可见——准入输入不含占位信息）→ 全房停下，所有人原地。
  - `scenario-calm.json` calm 包 after：落选者（requests 视野专属信息）可见 → 扣住同格胜者 strong；交换不受影响。
  - `scenario-combined.json`：wave replace + calm after 并集，两 namespace `applied` 各恰计 1。
  - 四场景 JSON/KKPV 双通道完整 final 一致（`stable_final` 剥离 cpu_bucket）；plan_hash 四身份互异（内容选择分别冻结）。
  - **冷恢复**：每场景每通道第 1 tick 后 `--save-checkpoint` 退出（断言未观测，exit 2）→ `--load-checkpoint` 续跑至第 3 tick（exit 0），续跑 final 与一次性 3 tick 参考完全一致（位置与 namespace 计数恰一次延续），plan_hash/composition 相同。
- **PlanHash 稳定实证**：`git stash` 切回基线构建与 G03.a 构建分别运行旧夹具 `sim-mod-move-admission/scenario-combined.json`，plan_hash 均为 `72d231e2ab9ac1aaa5bcc99afee866ee6dea628da6517e7dfed0f68035e700ec`——本批不改变计划身份、检查点身份与旧场景兼容性。
- **运行时链路**（`kk-runtime/tests/mod_chain_tests/move_admission.rs`，9 旧 + 3 新全过）：
  - `admission_evaluate_sees_overrides_and_requests_view`：覆盖→预演端到端闭环——原生终值预演幸存 1（未配对无件者被链取消），`setAdmitted` 放行后预演幸存 2（预演按候选当前终值即时计算，非快照）；requests 视野 JS 通道字段形态校验（moverId/x/y/index/originX/originY/won）且含拉者与被拉者两条登记；最终位置随候选落地。
  - `admission_evaluate_is_unavailable_in_obstacle_and_contest_slots`：evaluate 在障碍槽/竞争槽的处理器内调用走宿主能力门故障，不能被 catch+decline 掩盖，整轮候选失败且世界不变；动作槽能力门测试同步加入 evaluate 探针。
- **CLI 回归**（`kk-cli/tests/sim_tests/move_arbitration.rs` 2 测试过）：上述双通道/冷恢复/身份断言 + 位置门控断言（8 组 expected）。
- **性能优化（复审清偿）**：`simulate_survivors` 克隆瘦身为只复制取消链消费的字段（matrix/winners/extra_blocks/buildings + 替换 admissions；拉拽表与原始请求矩阵不拷贝），避免迭代式策略多次预演放大深拷贝成本；单次预演即一次完整取消链（`check_obstacle_at_xy` 对 Generic 结构逐判定 JSON 解析为既有成本），调用频次由 Mod VM 预算统一管控（与既有 op 一致，无逐调用上限）。

## 复审与清偿

三路只读复审（正确性/一致性、权限/故障恢复、验收/文档）结论：**三路均无高危，正确性/一致性无中级发现**——预演与正式取消链同一代码路径、克隆完备性、`original_requests` 生命周期（Working 每房每 tick 重建）、`won` 判定、拉拽标记时点、借用安全、`#[serde]` Vec<String> 形态逐项核对通过，且复审独立推演未找到"预演幸存集 ≠ 实际链幸存集"的可证伪拓扑。清偿记录：

- 【中级·B】夹具目录命中 `.gitignore` 的 `compat/fixtures/*` 规则——提交时 `git add -f compat/fixtures/mod-move-arbitration compat/fixtures/sim-mod-move-arbitration`（与既有夹具同做法），并核对入库文件齐全。
- 【中级·B/C】evaluate 能力门无测试——补 `admission_evaluate_is_unavailable_in_obstacle_and_contest_slots` + 动作槽探针（见上）。
- 【中级·C】setAdmitted→evaluate 组合缺端到端——补 `admission_evaluate_sees_overrides_and_requests_view`（见上）。
- 【低级·A】evaluate 全量克隆含链不读字段——克隆瘦身（见上"性能优化"段）。
- 【低级·A/B】`extra_blocks` 空集守卫仅 debug_assert——保留断言并补注双重保障依据（`arbitrate_room` 槽序固定 + 预演能力随准入槽 chain 令牌过期失效）；复审确认当前不可达。
- 【低级·A】等价性未覆盖安全模式分支——补 `simulate_matches_chain_under_safe_mode`。
- 【低级·A/B/C】安全模式主人计算三处复制——抽取 `Working::active_safe_mode_user` 私有助手，三处共用。
- 【低级·B/C】`pulling` 拉者侧语义偏宽（目标落选时残标不清理，为登记态而非有效态）——不改引擎复核语义，字段注释与架构文档收敛为"登记的拉动目标；目标落选本轮不生效"（`pulledBy`/`pulled` 保持有效态语义不变）。
- 【低级·C】台账等价性拓扑清单误列"竞争落选"（落选者不进缩减矩阵，不参与取消链）——清单改为实际交付拓扑并注明落选者可见性另行专测。
- 【低级·C】"PlanHash 验收实证"措辞强于证据——改为"基线对照构建实测一致"并指向本记录的 stash 对照实验；结构论证（builtin.rs 零改动、revision v10）同记。
- 【低级·C】sdk.js 注释超长行——拆行并补"非快照"语义。
- 【低级·C】夹具 README"六个移动者"计数——改为"五个移动请求 + 一个静止占位"。
- 【建议·C】测试计数口径（"5 旧 + 6 新"失准）——按函数/案数分别写清（7 旧 + 5 新函数）。
- 【建议·A】evaluate 文档补"预演后继续覆盖会使结果失效"——已并入架构文档 evaluate 行与 sdk.js 注释。
- 【建议·C】JS 通道仅消费 requests 部分 字段——`admission_evaluate_sees_overrides_and_requests_view` 的 shape 校验补齐全字段断言（x/y/moverId/won/index/originX/originY）。

## 剩余

- pull 登记策略开放（须重审 `validate_pull_pair` 胜者 ID 升序遍历的同拉者多目标顺序语义）。
- 障碍槽自身无预演点（准入槽 evaluate 已开放；障碍追加后无第二预演，文档已注明）。
- 障碍放行方向（表示有效性宿主约束）、意图静态预检 `has_static_obstacle`、G04 代价与执行效果。
- 密集移动下 evaluate 的调用频次由 Mod VM 预算管控；无单独调用上限（与既有 op 一致）。
