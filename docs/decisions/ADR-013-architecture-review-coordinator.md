# ADR-013 架构复审决策：运行协调层、直读节奏、LIFO 纠偏与宿主面收口

- **日期**：2026-09-29
- **状态**：已接受（四项裁决经用户拍板）；**B0 批已全部落实（2026-09-29）**——
  裁决 1 运行协调层（kk-runtime crate + RunResult 统一提交点，部分提交三连
  回归全绿）、裁决 3 LIFO 每线程（乱序销毁压力测试）、裁决 4 宿主面收口
  （`__kk` 私有句柄化）、裁决 5 TickInput（重放一致性回归）；**裁决 2 seam
  已随 M2B 前置批落实（2026-09-30，[方案](../plans/m2b-prerequisite.md)）**：
  WorldSource seam + 世界版本面（generation/terrain_epoch/RoomVersionHandle，
  snapshot 契约加现状标注）+ 地形按版本缓存（紧凑载荷，project 相 −42% 实测）
  + 投影/编解码宏化；列式布局与零拷贝冻结仍由端到端测量决定（对象投影
  Value 级共享实测负结果回退登记——共享收益须换表示）；决策 6 批构成中
  的双房双玩家实弹验证另出方案（M2B 扩面前），不在该批；**B0-6 外部评审
  清偿（2026-09-30，[方案](../plans/b0-6-external-review-clearance.md)）**：
  统一提交点三处实现洞补全——被拒的重试请求不再销毁 pending 缓存（原实现
  销毁后原请求重试会重跑玩家代码）、重试复用随失败轮缓存的 TickInput
  （裁决 5「重放用记录值」在协调器侧落实）、负桶停跑轮回充候选化

> 编号说明：ADR-012 由 A6 的 M2A 架构结论升格预留（[m2-playable.md](../plans/m2-playable.md) 头注），
> 本 ADR 因架构复审先行创建，编号跳过 012 在所难免。该预留位已于 A6 收口时补齐
> （[ADR-012](ADR-012-m2a-sandbox-runtime.md)）。

## 背景

M2 三轨中 B（引擎结算）、C（pathfinder/存储）已完成、A 轨剩 A5-core+A6 之际，对全项目做了一次
架构复审：内部六路并行深查 + 外部（GPT）独立复审交叉对比，外部全部可验证主张经逐条复核
（含探针重跑）。完整证据存档见
[架构复审笔记](../research/notes/architecture-review-2026-09.md)。

复审总判定：核心决策链正确、无系统性偏离，但暴露五项正确性问题与一项节奏风险——最重的两项：

1. **部分提交 bug**：玩家持久状态（Memory/CPU 桶）在各自 `end_tick` 即时结算，无暂存；世界结算
   失败回退时先执行玩家的状态已泄漏（复现：一轮失败后世界时间不动，但先执行玩家的 `Memory.n`
   已 +1）。[tick 契约](../architecture/tick-contract.md)的提交协议目标未落实。
2. **全局 LIFO 销毁约束是错误假设**：rusty_v8 的 isolate Enter/Exit 是**线程局部**栈，无跨线程
   全局逆序要求（探针验证：两线程乱序销毁，V8 不崩、存活实例正常执行）；原 spike 证据出自单线程
   叠多 runtime 的场景，被过度泛化，导致 `evict_idle` 只能从栈顶逐出。

## 决策

1. **引入运行协调层（正式形态）**：玩家执行编排（派发、报告收集、失败分类、重试、提交）从
   kk-engine 的 embed.rs 上移至独立的协调层；玩家执行只产出候选 `RunResult`（指令 / Memory /
   CPU / 运行结局 / 控制台），世界与玩家持久状态在**统一提交点**一次切换；玩家级失败走玩家级
   恢复，不再冒泡为整轮泄漏。Sandbox trait 扩为统一执行接口，真实沙箱与测试假沙箱实现同一
   接口，kk-engine 不再编译期依赖 deno_core/V8。落点推荐新 crate `kk-runtime`（备选先移
   kk-cli、后续升格），细节见 [B0 方案](../plans/b0-runtime-coordinator.md)。
2. **性能第二形态的节奏 = seam 先行**：M2B 玩家 API 大扩面（~150→340 方法）**之前**落地三件：
   世界注入来源接口化（seam）、地形按世界版本缓存、不可变房间版本句柄；M2B 新命名空间的
   getter 写在 seam 抽象之上。列式布局与零拷贝冻结**仍由端到端测量决定**
   （[snapshot 契约](../architecture/snapshot-contract.md)"冻结格式前的验收"条款不变）。
   本决策修正 [m2-playable D3](../plans/m2-playable.md) 的时间线（方向不变）。
3. **LIFO 纪律纠偏为每线程**：isolate 销毁顺序约束按 rusty_v8 实际语义（线程局部 Enter/Exit
   栈）执行；一实例一线程模型下每线程恰一个实例，天然满足。空闲回收（`evict_idle`）解除
   栈顶限制、支持任意实例回收；进程退出 teardown 默认仍按创建逆序走（稳妥），并补乱序销毁
   压力测试。[sandbox.md](../architecture/sandbox.md) 原表述已随本批评正。
4. **宿主控制面收口**：`globalThis.__kk`（beginTick/endTick/runLoop/诊断清理）从玩家全局移除，
   生命周期由 Rust 侧持有的私有 V8 函数句柄驱动；玩家全局只暴露游戏 API。消灭"官方没有的
   玩家可达控制通道"这一规则破坏面（复现实锤：玩家可清自己刚发的指令、替换内部世界视图），
   并根治单 realm 共享导致的永久性打地鼠。
5. **非确定性输入显式化（TickInput）**：墙钟等外部输入经 `TickInput` 注入结算，首次执行记录
   实际值、重放使用记录值（signController 的 `datetime` 字段保持玩家可见的墙钟语义不变）。
   对齐 [tick 契约](../architecture/tick-contract.md)"回放以已收集的指令和运行结果为输入"的要求。
6. **排期**：先关 2A——在飞清偿提交收口（棘轮+bot+地板+push）→ A5-core → A6 原范围收口，
   随后立即开 **B0 正确性清偿批**（上述 1/3/4/5 + 文档回写），再进入 M2B 前置批（seam 三件套、
   投影宏化、双房双玩家合并规则实弹验证），然后 M2B 扩面。另：**线程 CPU 时钟切换钉在 M3
   多用户承载前**（不在 B0——多玩家争核下墙钟会双向误计费，机制不变只换表）。

## 三原则符合性

玩家体验一致 / 玩家不可越狱 / Rust 最优且不照搬官方——逐项校验结论：上述决策无一以玩家可见
行为换性能（seam 为纯传输层；协调层是官方不存在的、比官方更严格的自有组件；LIFO 纯资源管理；
TickInput 不改玩家可见值；宿主面收口反而消除官方没有的玩家可达面）。整 tick 原子回退 vs
官方"出错跳过继续推进"属刻意分歧（确定性目标所迫），已随本批登记至
[兼容灰区表](../designs/compatibility.md)（B0 落实时销账）。

## 证据

- [架构复审笔记](../research/notes/architecture-review-2026-09.md)（六路深查摘要、外部增量发现
  与逐条复核、探针重跑输出、双方收敛/分歧与折中）
- 关键代码位点：embed.rs `run_players` 循环 `?`；sandbox.rs `end_tick` 即时 `budget.settle`；
  proto.rs `evict_idle` 栈顶限制；sandbox.rs `Drop` 全局 LIFO 注册栈；settle/creep.rs
  `SystemTime::now()`；settle/mod.rs `Working.next_id/players` 可变全局面
- 复现与探针产物：部分提交复现序列、跨线程乱序销毁探针（本人重跑：exit 0，仅项目自家告警）
