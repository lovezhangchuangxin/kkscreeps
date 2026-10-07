# G02.a 移动准入策略交付记录（2026-10-06）

## 范围

开放 `builtin:creep.moveAdmission` 房间级移动准入槽：仲裁选胜后对全部获胜执行者整房一次进入 JS，候选初值为原生 `can_move` 结果（疲劳 0 且有存活 MOVE，或有效被拉），JS 经 `ctx.admission`（`getAdmitted`/`setAdmitted`）读初值、覆盖终值；准入终值**同源**消费于两处——阻塞链取消（`cancel_blocked`）与执行阶段（`movement_execute`），执行阶段不再按当前身体二次 native 复核，缺项一律未获准（fail-closed）。现行契约见[JS 移动准入](../../architecture/movement.md#准入与预演)。

障碍准入（`has_static_obstacle`/`check_obstacle_at_xy` 的地形/结构/rampart/安全模式判定）、孵化中拒绝与越界/坐标仍为原生或宿主约束，登记为 G02.b 及后续子片。

## 实现结构

- `kk-mod-api`：`admission.rs`（`MoveAdmissionInput`/`MoverInfo`，camelCase 序列化）；`SLOT_KEYS` 15→16，新描述符（stage=Movement、native 绑定 `builtin:creep.moveAdmission.native`），builtin revision 升 v9；freeze_cases 断言同步。
- `kk-engine`：
  - `settle/movement.rs` 拆分仲裁：`rank_winners`（优先级+四键选胜）→ `prefill_admissions`（拉拽配对复核 + 原生 can_move 预填终值表；配对复核时序与旧 `remove_from_matrix` 逐格复核等价，由自由函数 `validate_pull_pair` 以拆分字段借用实现，无整表拷贝）→ `admission_input`（仅 Hosted 路径构造，含字符串分配；原生路径不为其付费）→ `apply_admissions`（槽终值整体替换）→ `cancel_blocked`（读终值，`admitted` fail-closed）。`check_impl` 保留为原生整链组合。
  - `settle/admission.rs`：`MoveAdmission` 终值候选（原生值初始化、域校验、不可解析 ID 按构造故障）。
  - `settle/contest.rs::arbitrate_room`：竞争槽 → 排名 → 准入槽（预填恒发生、输入仅 Hosted）→ 取消链。
  - `settle/contrib.rs`/`contrib/admission.rs`/`assemble.rs`/`install.rs`：SlotCall::Admission、双路装配、native 绑定清单。
  - `settle/creep/movement.rs::movement_execute`：复核改读 `movement.admitted(id)`。
- `kk-mod-host`：`chain/admission.rs` 两个 op（读/写，token/binding 归属与故障留存）；ChainHost `admission_admitted`/`admission_set_admitted` 默认拒绝；sdk.js `ctx.admission` 工厂注入 simple/inner 两处（不可枚举、open 生命周期同 contest）；execution_plan decline 豁免加入本槽。
- `kk-runtime`：`mod_chain.rs` SlotCall::Admission 接线、`admission_slot` 目标门（fields 恒 null、写面提前拒绝）、staged/Declined 允许清单、NativeCall::Admission 无操作中心。

## 验收证据

- `cargo test -p kk-engine`（本批后全量）：**292 通过**/2 既有忽略（285 既有 + 7 新增 `settle::admission::tests`：输入要素完整性、原生整链等价、覆盖放行且取消链消费、候选域/空输入/fail-closed、有效被拉配对时序含破裂态、放行不绕过被占目的格、非 creep 胜者防御值与域外读取 fail-closed）。
- `cargo test -p kk-runtime --test mod_chain_tests`：**206 通过**（196 既有 + 10 新增 `move_admission`：输入完整性与放行、有效配对原生正对照（真实 pull 意图链）、拒绝回退、收紧策略+原生对照、11 种非法写入/越权不可被 decline 掩盖、动作槽能力门、故障停用回退、晚期写回缓存重试恰一次提交、同包竞争排序×准入放行组合解耦、移动槽能力 request 通道与意图通道同源吃准入终值；JSON/KKPV 双通道覆盖放行与重试两例）。
- `cargo test -p kk-cli --test sim_tests`：**33 通过**（31 既有 + 2 新增 `move_admission`）：四场景（plain/tireless/strict/combined）× 双通道完整 final 与 PlanHash 一致、四个计划身份分别冻结；每场景第 1 tick 保存检查点退出（码 2）、加载续跑与直跑 final 等价（冷恢复）；场景断言核对放行/收紧两端与双 namespace 恰一次计数。
- 场景夹具：`compat/fixtures/mod-move-admission/`（tireless replace 放行、strict after 收缩两个独立包）与 `compat/fixtures/sim-mod-move-admission/`（四场景，未选包 mod_states 投影为 null）。
- 关键路径发现：玩家面方向形态 `move()` 预检疲劳与 MOVE 部件（同官方 creeps.js），疲劳/无件移动者经朝向相邻 creep 的 `move(target)` 形态到达仲裁——验收场景与正对照（真实 `pull` 意图 + 配对完好原生放行）均建立在该通道上。

## G30.d 同功能默认参考（微基准）

`movement_benchmark_native_arbitration`（8 竞争格 ×4 竞争者、release、2000 次迭代，M1 arm64；首轮冷值单列）：

| 时点 | 样本（µs/房） |
|---|---|
| G01 后基线（9c8687d） | 4.56（冷）、1.65、1.61、1.61 |
| 本批后 | 2.75（冷）、1.84、1.95、1.89、1.92 |

密集 32 移动者场景每房增加约 0.2–0.3µs，来自准入终值表维护（配对复核 + 逐胜者插入 + 取消/执行同源读表）；输入构造（含字符串分配）已收敛到 Hosted 路径，原生路径不付费。开发机并发编译时测得的 4–8µs/房读数系测量污染，静置复测后如上。JS 策略端到端开销仍由 Mod VM 预算管控，随 G30.d 滚动补测。

## 复审

三路只读 subagent 复审（引擎时序与等价 / 宿主权限与运行时 / 测试与文档），三路均无高危。发现与清偿：

- 宿主权限路：3 处低级项已清偿——decline 文案补列移动准入（chain.rs）、unused mut（admission/tests.rs）、陈旧注释引用改名方法（admission.rs）。能力门、sdk.js 注入、Declined 回退、恢复边界、序列化字段、冻结面（16 槽/revision/NATIVE 清单）逐项核验通过。
- 引擎时序路：核验 movement_execute 改读终值与旧复核等价（伤害/治疗在目标自身 tick 的移动之后施加，旧复核本为冗余；死亡由 take_move/creep 查找双闸兜住）、fail-closed 无跨 tick 残留、候选域不可绕过、检查点只含已提交世界。1 项中级（配对复核遍历顺序改为胜者对象 ID 升序，与旧目的格访问序在"同一拉者多目标残标"病态输入下有差异——现有意图流不可达，G03/G04 开放 pull 登记时可达）：已在代码注释与架构文档改述"集合等价、顺序语义与不可达依据"，并登记重审条件。2 项低级已清偿：准入终值表克隆三次减为一次（apply_admissions/into_finals 接管所有权）；Hosted 触发面宽于竞争槽已在架构文档性能口径点名。
- 测试与文档路：1 项高危（交付记录死链——本文件当时未写）已随本文件补齐清偿，文档链接门禁复测通过。2 项中级已清偿：补"同包竞争排序×准入放行组合"与"移动槽能力 request 通道同源吃终值"两个测试；2 项低级已清偿：补"放行不绕过被占目的格"与非 creep 防御值/fail-closed 读取测试；域外 ID 直接断言一并补入。冷恢复、夹具、.gitignore 白名单、链接与数字一致性逐项核验通过。

## 剩余与后续

- G02.b：障碍准入——`has_static_obstacle`（意图静态预检）与 `check_obstacle_at_xy`（仲裁期动态障碍，与让位链耦合）开放为可替换入口，须处理取消链交互。
- 孵化中/spawning 门槛与越界/坐标：登记为宿主约束，未开放。
- G03 整房请求视野、G04 移动代价与执行效果：未开放，准入输入只含胜者。
