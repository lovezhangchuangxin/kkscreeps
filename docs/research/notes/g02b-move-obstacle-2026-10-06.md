# G02.b 仲裁障碍准入交付记录（2026-10-06）

## 范围

开放 `builtin:room.moveObstacle` 房间级障碍槽：准入槽之后、阻塞链取消之前，对全部胜者整房一次进入 JS；输入提供目的格对象/地形摘要与预填时点 native 初判，JS 经 `ctx.obstacle`（`isBlocked`/`block`）**单向追加**阻挡；取消链按 追加阻挡 ∥ 链内 native `check_obstacle_at_xy` 取消，让位链传播（追加取消 A 后 A 阻挡其原格上的依赖请求）保留在引擎链内。放行方向不开放——放行 native 阻挡会让移动者进入未让位对象占据的格、破坏坐标表示有效性（宿主约束）。现行契约见[JS 移动障碍](../../architecture/movement.md#障碍追加)。

意图登记期静态预检 `has_static_obstacle`（intent_move/slot_request_direction 两处同口径）不并入本片——预检在仲裁前逐对象执行、无整房视野，登记为后续子片或并入 G07 预检族。

## 实现结构

- `kk-mod-api`：`obstacle.rs`（`MoveObstacleInput`/`ObstacleMoverInfo`/`ObstacleObjectInfo`）；`SLOT_KEYS` 16→17、描述符（stage=Movement、绑定 `room.moveObstacle.native`）、revision v10；freeze 断言同步 17。
- `kk-engine`：
  - `settle/movement.rs`：`extra_blocks: BTreeSet<ObjId>` 字段；`obstacle_input`（胜者对象摘要：creep 的 yielding=胜者表包含、rampart 公开性、工地 owner、墙地形与路；native 初判=预填时点 `check_obstacle_at_xy`）；`apply_extra_blocks` 接管所有权；`cancel_cell` 判定改为 `extra_blocks.contains ∥ check_obstacle_at_xy(链内)`。
  - `settle/obstacle.rs`：`MoveObstacle` 追加候选（域校验、`is_blocked`=native 初判或已追加、单向 `block`、`into_blocks` 接管）。
  - `settle/contest.rs::arbitrate_room`：竞争 → 排名 → 准入 → **障碍** → 取消链。
  - `contrib.rs`/`contrib/obstacle.rs`/`assemble.rs`/`install.rs`：SlotCall::Obstacle、双路装配、native 绑定清单。
- `kk-mod-host`：`chain/obstacle.rs` 两个 op；ChainHost `obstacle_blocked`/`obstacle_block` 默认拒绝；sdk.js `ctx.obstacle` 工厂注入两处；execution_plan decline 豁免。
- `kk-runtime`：`mod_chain.rs` SlotCall::Obstacle、`obstacle_slot` 目标门、NativeCall::Obstacle 无操作中心、staged/Declined 清单。

## 验收证据

- `cargo test -p kk-engine`：lib **69** + 集成 **285** + doctest 2 全过、0 失败（G02.a 后为 lib 65；本批新增 4 个 `settle::obstacle::tests`：输入要素与 native 初判（含静止/让位 creep 摘要）、墙地形无路判定、追加阻挡沿交换让位链传播（原生对照两步链成立）、候选域与空输入/接管后取空）。
- `cargo test -p kk-runtime --test mod_chain_tests`：**214 通过**（206 既有 + 8 新增 `move_obstacle`：输入完整性与追加传播、原生对照 + stayNorth/noSwap 两独立策略、replace+after 并集组合、拒绝回退、10 种非法写入/越权不可被 decline 掩盖、动作槽能力门、故障停用回退、晚期写回缓存重试恰一次；JSON/KKPV 双通道覆盖传播与重试两例）。
- `cargo test -p kk-cli --test sim_tests`：**35 通过**（33 既有 + 2 新增 `move_obstacle`）：四场景（plain/staynorth/noswap/combined）× 双通道完整 final 与 PlanHash 一致、四个计划身份分别冻结；第 1 tick 保存检查点退出（码 2）、加载续跑与直跑等价（冷恢复）；场景断言核对交换/南下两端与双 namespace 恰一次计数。
- 场景夹具：`compat/fixtures/mod-move-obstacle/`（stay-north replace、no-swap after 两个独立包）与 `compat/fixtures/sim-mod-move-obstacle/`（四场景）。

## G30.d 同功能默认参考（微基准）

`movement_benchmark_native_arbitration`（8 竞争格 ×4 竞争者、release、2000 次迭代，M1 arm64）：

| 时点 | 样本（µs/房） |
|---|---|
| G01 后基线 | 4.56（冷）、1.65、1.61、1.61 |
| G02.a 后 | 2.75（冷）、1.84、1.95、1.89、1.92 |
| G02.b 后（本批） | 1.85、1.83、2.01、1.99 |

障碍槽的默认路径新增仅 `extra_blocks`（BTreeSet，恒空）的 contains 检查与 Movement 克隆时的空集拷贝，在测量噪音之内；输入构造（对象摘要 + native 初判逐胜者重算 `check_obstacle_at_xy`）只在 Hosted 路径付费。JS 策略端到端开销仍由 Mod VM 预算管控。

## 复审

三路只读 subagent 复审（引擎时序与单向性 / 宿主权限与运行时 / 测试与文档），三路均无高危与中级阻断。发现与清偿：

- 引擎路：低级 3 项已清偿——工地摘要补 `structureType`（可空字段，工地填在建类型、其余 null，文档表格同步）；`yielding` 收紧为 胜者 ∧ 准入终值为真（准入被拒的胜者必被链内取消，让位预期不成立，相对最终结果仍是下界）；`apply_extra_blocks` 加 debug_assert 暴露 G03 潜在重入替换。建议 3 项：owner 空串分支冗余已简化、单测悬置草稿注释改为明确断言、安全模式主人未入摘要登记为后续可解释性增强（见剩余）。交付记录计数 63→65 已修正。单向性与"链内只会收紧"经评审独立形式化推演与测试双重确认（winners 链内单调收缩、障碍判定对 winners 单调、房间/地形/索引在链内不变）。
- 宿主权限路：与 admission/contest 槽逐行对照无遗漏（op 故障留存、能力门、sdk.js 注入与 open 生命周期、Declined 链路、序列化字段、冻结面 17/revision v10/NATIVE 清单、realm 注册）。2 项中级均为收口时序问题——台账未更新（本节与台账同步完成）、admission/contest 文档槽计数 16 与 G02.b 措辞未同步（已改为 17 并更新交叉引用）；1 项低级（测试 unused 变量）已清偿。
- 测试与文档路：无高危；3 项中级中 台账未更新（本节与台账同步完成）、admission/contest 文档槽计数与 G02.b 措辞（同宿主路发现，已改 17 并更新交叉引用）、"准入被拒者仍在 movers 列表"声明无测试——已清偿：mod_chain probe 场景加入未配对无件者（准入终值为否仍在输入，movers 数恰为胜者数断言由 3 改 5）。3 项低级已清偿：模块注释 `//!` 截断、传播演示改为只挡 b（直接挡 b → b 取消 → a 失去让位随链取消，真实单向传播）、引擎测试探索性草稿注释（同引擎路 S3）。非法矩阵补 `ctx.objects.get` 越权一条。decline 隔离与 sim 层传播场景为结构性保证/引擎已覆盖，登记不另补测。

## 剩余与后续

- 意图静态预检 `has_static_obstacle`（结构/私有 rampart/自家工地）开放为可替换入口：后续子片或 G07 预检族。
- 放行方向（解除 native 阻挡）不开放：坐标表示有效性属宿主约束；若未来需要（如配合自定义交换语义），须与 G03 获准集合共同设计。
- G03 整房请求视野/获准集合替换、G04 地形代价与执行效果：未开放。
