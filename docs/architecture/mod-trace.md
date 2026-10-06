# Mod 运行 trace

`sim run --mod-trace`（`mod test --mod-trace` 同义透传，汇总报告记录该旗标）开启**可选、有界**的 Mod 运行事件记录，报告新增 `mod_trace` 字段。此前的诊断面只保留**最后一条**结算故障（`mod_fault`）与不可见的连击/停用状态；正常拒绝（Decline）与整轮丢弃→重建→缓存重试的回退链路对作者完全不可见。trace 在两个汇聚点补齐这一层（`kk-runtime/src/mod_trace.rs`）：

- **槽结果**（`embed/mod_chain.rs`，每次整槽调用）：`SlotCompleted` / `SlotInactive`（停用命名空间的槽被丢弃而非执行——invoke_chain 对 inactive 槽恒返 Completed，真实完成与静默丢弃必须区分）/ `SlotDeclined{binding, code}` / `SlotFault{binding, kind, error}`，条目带 tick（提交前的世界时间）、房间、目标对象 id、槽键与阶段（`object_action`/`object_tick`/`movement`/`market`）。
- **回退生命周期**（`embed/execution.rs`/`embed.rs`）：`SettlementRetry{error}`（引擎实际结算失败，候选缓存待重试，不重跑玩家代码；结算入口前的测试注入/装配失败同样缓存候选但不产生本事件）、`RealmRebuilt`（故障后按固定启动快照重建，身份核验通过）、`AutoDisabled{mod_id, consecutive, disabled}`（连击达阈值自动停用，mod_id 为停用来源、disabled 含依赖闭包；native 回退自此生效）。生命周期事件无槽/目标，对应字段为空串、tick 为当轮世界时间。

故障种类的 `kind` 与 `ModFaultKind` 的 serde snake_case 同名（`handler`/`write_conversion`/…），`mod_trace` 与 `mod_fault` 两个诊断面可按此 join；无结构化来源的基础设施错误统一记 `chain`。事件序即发生序；同一故障轮的重试会再次记录槽事件（重试真实执行了链），一轮内先记停用决定再记缓存重试。

## 边界与纪律

- **有界**：缓冲上限 1024 条（`TRACE_CAP`），超限截尾并累计 `dropped`；单条错误/拒绝文本截断 200 字符（字符边界）。默认关闭；**所有记录点先经 `trace_enabled()` 判空门控**——关闭时无缓冲、无条目构造、无格式化。开启时每槽调用新增少量字符串克隆，相对链调用本身可忽略。
- **与 inspect 活动链一致**：trace 的槽键与故障/拒绝绑定取自同一冻结计划——验收断言"每个事件绑定 ∈ 同报告 `composition` 对应槽的 before/after/wraps/center 活动链"，漂移在测试期暴露。
- **隐私范围**：条目只含定位信息（tick/房间/对象 id/槽键/阶段）与结果摘要；不记录参数、字段值或玩家 Memory。错误文本是结算诊断原文，可能引用被拒参数——属服务端诊断面，不经玩家可见通道输出。
- **不是回放日志**：trace 是诊断快照，不含确定性重放所需的参数、字段值与随机序列。复现材料=场景身份（报告 `scenario_sha256`）+ 包目录与选择集（`plan_hash`、`mods.packages`）+ 同二进制（`executable_sha256`）重跑；报告 `mod_trace.note` 原文注明。sim 场景本身是确定性的，同输入重跑即复现。
- **开启时序与检查点**：`enable_mod_trace` 在 Mod 装配前调用会挂起到装配后生效；检查点恢复用替代引擎整体替换，挂起标志随之转移（续跑保持开启，有回归测试锁定）。trace 缓冲本身**不进检查点**：恢复后的分片从新空缓冲开始记录。teardown 重置挂起标志。
- 槽未进 VM 的早退（"realm 已在执行"/"Mod VM 待恢复"/候选构造失败）不记录——无结果可言。

## 验收

- `kk-runtime`：`mod_trace.rs` 单测（上限/丢弃计数/字符边界截断/故障种类标签与 serde 名一致）；`mod_chain_tests/trace.rs` 端到端——默认关闭无快照且开启后健康链只有完成事件（非空断言 + tick/房间/槽/阶段字段齐）、拒绝事件带绑定与 code、故障→自动停用→缓存重试→重建→新链完成的**完整事件序**锁定（含 kind 名与错误文本）、停用后重试轮自有槽记 `SlotInactive`（恰 3 个、无新故障）、trace 跨 realm 重建保留历史。
- `kk-cli`：`sim_tests/mod_trace.rs` 经真实 CLI——gatekeeper 场景（拒绝+完成，绑定 ∈ composition 活动链，默认关闭时无载荷且最终世界一致）、breaker 场景（`run_failed` 退出 2 且失败归因为结算故障，`slot_fault` 槽/绑定/错误文本 + `settlement_retry`，复现材料字段在报告内）、检查点续跑（保存 2 tick 后 `--load-checkpoint --mod-trace` 续跑继续记录且事件从恢复后的世界时间开始）。
- JS 夹具 [mod-trace](../../compat/fixtures/mod-trace/) 与 [sim-mod-trace](../../compat/fixtures/sim-mod-trace/README.md)：gatekeeper（可通行/可扣押动作对，房间可见对象 + 玩家类 emit）与 breaker（link tick 前置抛错）。

## 当前边界

首片不做：按 tick/槽/绑定过滤的 CLI 参数（缓冲上限内全量输出，过滤由报告消费者做）；自动停用阈值仍只能经 `RegisterOptions` 程序化设置（场景 JSON 不暴露），sim 层的 breaker 场景只覆盖故障与缓存重试，自动停用序列在 kk-runtime 测试（阈值 1）覆盖；`mod inspect` 不输出 trace（它不推进世界，无事件可记）。
