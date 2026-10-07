> 历史归档：正文保留当时的方案、状态和验证；不得作为当前接手指令。当前工作见[任务台账](../mod-next-batches.md)，现行契约见[架构索引](../../architecture/README.md)。

# B0 运行协调层与正确性清偿批方案

> 状态：**已确认**（2026-09-29，四项裁决经用户拍板，[ADR-013](../../decisions/ADR-013-architecture-review-coordinator.md)）；
> **B0 批已执行完毕（2026-09-29，切片执行与实现偏差见 [b0-slices.md](b0-slices.md)）**——
> 验收关卡 1-4 全部达成（复现场景三组回归全绿、workspace/clippy 全绿、
> kk-engine 零 kk-runner 依赖、棘轮零转红、bot 三基线绿、perf-probe 同口径
> 四相无显著劣化且 run 相 -52.8%）。
> 排期：**A6 关段之后执行**——此前依次为在飞清偿提交收口（棘轮+bot+地板+push）→ A5-core → A6 原范围关段（2A 关卡达成）。
> 硬约束与 [m2-playable.md](m2-playable.md) 头注相同：①玩家侧体验与官方一致；②玩家不可越狱；在此之上按性能与确定性最优设计。
> 依据：[ADR-013](../../decisions/ADR-013-architecture-review-coordinator.md)、[tick 契约](../../architecture/tick-contract.md)、
> [sandbox.md](../../architecture/sandbox.md)、[架构复审笔记](../../research/notes/architecture-review-2026-09.md)。

## 0. 背景：五项已验证问题

| # | 问题 | 证据位点 | 严重度 |
|---|---|---|---|
| 1 | **部分提交**：玩家持久状态（Memory/CPU 桶）在各 `end_tick` 即时结算无暂存；世界结算失败回退时先执行玩家状态已泄漏（复现：p2 注入失败 → 整轮 Err、时间不动，p1 `Memory.n` 已 0→1） | embed.rs `run_players` 循环 `?`；sandbox.rs `end_tick` 内 `budget.settle`；tick-contract"暂存到提交点"未落实 | P0 |
| 2 | **全局 LIFO 误判**：rusty_v8 Enter/Exit 为线程局部栈，跨线程无全局逆序要求（探针实证不崩）；`evict_idle` 被错误假设绑架只能栈顶逐出 | proto.rs:230；sandbox.rs `Drop` 全局注册栈；spike-deno_core.md:174 证据出自单线程叠实例 | P1 |
| 3 | **宿主面玩家可达**：`globalThis.__kk` 冻结但可调用——可清自己指令、替换内部世界视图使 `Game.time` 失真（官方无此通道） | 90_host.js:43；compatibility.md 已登记欠账，外部复审复现 | P1 |
| 4 | **墙钟落盘**：`signController` 结算内 `SystemTime::now()` 直写世界状态，破坏重放确定性（两次执行时间戳不同）；且伴随自相矛盾的双注释 | settle/creep.rs:1482 一带 | P1 |
| 5 | **embed 编排混入引擎 crate**：kk-engine 编译期拖上 deno_core/V8/tokio；Sandbox trait 真路径架空（真实现绕开 trait） | kk-engine/Cargo.toml → kk-runner；embed.rs:15 | P1 |

## 1. 范围与验收

**五个工作包**：WP1 运行协调层（含部分提交修复与 embed 上移）、WP2 LIFO 纠偏、WP3 TickInput 非确定性显式化、WP4 宿主面收口、WP5 文档回写。

**验收关卡（机器可判定）**：

1. 复审复现场景全部转为回归测试且全绿：部分提交三连（注入失败轮后先执行玩家的 Memory/CPU 候选与世界版本均不变）、LIFO 乱序销毁（多线程交错销毁 + 存活实例正常执行 + 干净退出）、重放一致性（同初始世界+同指令+同 TickInput 两次结算快照逐字节一致，含 sign）；
2. `cargo test --workspace` 全绿、clippy（workspace deny）全绿；
3. **kk-engine 不再依赖 kk-runner**（V8 退出规则引擎编译面），真/假沙箱实现同一执行接口；
4. 棘轮零转红（五项均不改玩家可见行为）；意外转红按 ratchet-allow 逐字豁免 + 同提交归因留痕。

**明确不在本批**：线程 CPU 时钟切换（钉在 M3 多用户前）、列式布局与零拷贝冻结（测量后，M2B 前置批的 seam 只做接口与句柄）、跨房移动落地（2B）、capability 面扩充、双房双玩家实弹验证（M2B 前置批另出方案）、bot-check 接入真结算。

## 2. 关键决策（均已裁决，ADR-013）

| # | 决策 | 裁决 |
|---|---|---|
| D1 | 部分提交修复形态 | **正式协调层**（非 embed 内最小修复）——一步到位落实 tick 契约提交协议，避免二次搬家 |
| D2 | 协调层落点 | **新 crate `kk-runtime`**（依赖 kk-engine + kk-runner，被 kk-cli 依赖）；备选先移 kk-cli 再升格（若批内工程量超限） |
| D3 | LIFO 纪律 | **每线程**：同线程多实例仍逆序（一实例一线程下天然满足）；跨线程无约束；空闲回收支持任意实例 |
| D4 | 宿主面 | **全量收口**进 Rust 私有 V8 函数句柄，玩家全局零宿主通道 |
| D5 | 非确定性输入 | **TickInput** 注入（首跑记录实际值、重放用记录值），不改玩家可见语义 |

## 3. 工作包详设

### WP1 运行协调层（批内最后执行，吸收前序形态）

目标数据流（tick 契约阶段 1–7 的单机最小形态）：

```
协调器 tick():
  ①固定输入（V_t、P_t、玩家名单、TickInput）
  ②逐玩家执行 → 各自候选 RunResult（无任何持久副作用）
      玩家运行结局（executed/软异常/terminated/注入失败）＝有定义结局 → 继续
      基础设施故障（线程失联/协议损坏）→ shard 停顿（返回错误，不伪装玩家异常）
  ③④世界结算 tick_with_intents(全批 intents)
      失败 → 整轮回退，重试只消费已缓存 RunResult，玩家代码不再执行
  ⑤⑥统一提交：世界版本 + 玩家持久状态（RawMemory/CPU 桶/Memory）一次切换
  ⑦发布：控制台/事件/快照查询面
```

- **RunResult 结构**（对齐 tick-contract"状态与可见性"）：玩家身份、运行结局（枚举区分玩家结局/基础设施故障）、intents 载荷、RawMemory 候选、CPU 计费候选、控制台缓冲。现有 `EvalReport`/`PlayerRun` 归并为该类型的投影层。
- **Sandbox trait 统一执行接口**：`collect_intents(&WorldView, &str) -> IntentBatch` 演进为 `run(&RunContext) -> RunResult`；假沙箱实现同接口返回构造结果（现有 fake sandbox 测试面迁移）。
- **embed.rs 拆分清单**——移交 kk-runtime：`create_shard` 编排、`run_player`/`run_players`、`finish_player_run`、`settle_tick_with_reports`、`last_console`；kk-engine 保留：`Engine`/settle 面/快照查询面（适配器的 snapshot 查询经协调器转发）。
- **对外契约不变**：`kkscreeps embed` 的 JSON-RPC 方法面与 `PlayerRun` 返回形状维持（适配器零改动），内部改走协调器。
- **CPU 计费暂存**：`end_tick` 不再即时 `budget.settle`，产出候选值；提交点统一落桶（写回失败等玩家结局的 intents 处置语义不变，沿 UNDOC-MEMJSON-005 锁定形态）。

### WP2 LIFO 纠偏

- 全局 LIFO 注册栈降级为存活计数与测试探针；顺序约束改 debug 级**每线程**断言；`lifo_violations` 计数器语义同步（全局序不再是违规）。
- `evict_idle` 扫描全表回收任意超时实例（解除栈顶限制——后创建的活跃玩家不再挡住前面空闲玩家的回收）。
- `shutdown_all` 默认仍按创建逆序（稳妥），新增乱序销毁压力测试（≥3 线程交错创建/销毁 + 存活实例执行 + 进程干净退出）。

### WP3 TickInput

- `TickInput { now_ms: u64 }`：首跑取 `SystemTime`，重放由调用方注入记录值；`Working` 或结算入口携带。
- `signController` 的 `datetime` 改读 `TickInput`；清理 creep.rs:1469-1490 的矛盾双注释（保留"官方 signController.js:23 墙钟语义 + 重放经记录值"单一口径）。
- 重放一致性回归测试（验收关卡 1 第三项）。

### WP4 宿主面收口

- prelude 安装完成后，Rust 侧以 `v8::Global<v8::Function>` 捕获 beginTick/endTick/runLoop/resetModules/诊断清理句柄存于 `Sandbox`，随后**删除 `globalThis.__kk`**；宿主侧生命周期改经私有句柄调用。
- 90_host.js：生命周期函数注册进 `__kkp` 私有容器（段末已随段删除），不再上全局。
- 玩家全局白名单测试更新：`__kk` 从"冻结但存在"变为"不存在"（`getOwnPropertyNames` 白名单比对同步）。
- 先全仓 grep `__kk` 依赖清单（prelude/宿主/测试三面）再动刀；A3c/A4"预置捕获"登记项中依赖玩家抢先面的残余（如 drainMissing）随收口销账。
- 适配器零影响确认：runPlayer 通道走宿主侧注入与取回，不经玩家全局。

### WP5 文档回写

- 本批文档已先行（ADR-013/sandbox.md/roadmap/灰区表）；随代码合入回写实现现状：tick-contract 状态注（单机协调器已落地、并行调度仍为目标）、sandbox.md 对应条目、灰区表 B0 条目销账、ADR-013 标注落实状态。

## 4. 测试计划

| 测试 | 锁定内容 |
|---|---|
| 部分提交回归（三场景） | 注入失败轮：先执行玩家的 Memory/CPU/世界版本全不变；结算失败重试不重跑玩家代码（执行次数=1）；玩家结局不中止整轮 |
| LIFO 压测 | 乱序销毁不崩、存活实例正常求值、退出干净；每线程逆序断言仍有效 |
| 重放一致性 | 同输入+记录 TickInput 两次结算逐字节一致（含 sign.datetime） |
| 宿主面 | 玩家代码访问 `__kk` 得 undefined；全局面白名单比对通过；原 GPT 复现序列（清指令/改 Game.time）不再可达 |
| 协调层接口 | 真/假沙箱同接口；基础设施故障分类停顿语义；RunResult 归并后 `PlayerRun` 投影等价 |
| 棘轮 | 零转红预期；异常转红逐字豁免+归因 |

## 5. 风险与预登记

- **WP1 批量最大**：内部切步（类型与接口先行 → 编排迁移 → embed 削减），每步独立可合入、三方评审；embed RPC 冒烟测试随步跑。
- **RunResult 与 tick 契约并行演进**：以契约为准绳，实现偏差必须回写契约，不允许静默分叉（复审教训：ADR 落后机制）。
- **宿主面隐性依赖**：动刀前完成 `__kk` 全仓依赖清单；若发现 prelude 运行期依赖玩家全局 `__kk`（不应存在），当场改私有容器。
- 顺带收敛：本批触碰文件内 kk-runner 的 unwrap/expect（12.8/千行，全仓最高）不新增、可减不增；不扩面到未触碰文件。

## 6. 实施顺序与纪律

**WP2 → WP3 → WP4 → WP1 → WP5**：先局部修复、后结构重构——协调层落地时直接吸收 RunResult/重试/失败分类的最终形态，避免在其上做二次搬家。每步：≥3 subagent 并行评审 → 串行合入 → 棘轮零转红 → 批后外部复审 HEAD（沿用既定流程）。

## 7. 本批之后

M2B 前置批（seam 三件套/投影宏化/双房双玩家，另出方案）→ M2B 扩面（API 写在 seam 之上）→ 测量驱动深化（列式/零拷贝冻结、并行流水线、线程 CPU 时钟、存储落盘）。

复审其余发现的去向登记：`Working` 结果表达改造（`next_id`/`players` 移出房间工作者可变面）是双房双玩家批的前置；看门狗线程收敛（当前每玩家≈2 线程）随 M2B 前置批或性能批评估；kk-runner unwrap 密度收敛随触碰文件渐进。
