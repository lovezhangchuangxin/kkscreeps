# kkscreeps

用 Rust 从零重构 [Screeps](https://screeps.com)（编程 MMO）的服务端。目标：**玩家侧体验与官方完全一致**的前提下，把服务器资源开销降低一个数量级，并提供官方架构给不了的本地开发体验。

> **给没玩过 Screeps 的读者**：Screeps 是一个"写代码玩游戏"的 MMO——玩家用 JavaScript 编写 AI，控制名为爬爬虫（creep）的单位在分成一个个 50×50 格房间的世界里采集能量、建造、战斗。服务器按固定节拍（tick）推进世界；玩家的代码每 tick 在服务器上运行一次，消耗的"CPU"是一种按 tick 发放、可攒可透支的配额货币（约等于毫秒）。玩家代码不直接改世界，而是下达"指令"（intent），由服务器统一结算。

**当前状态（2026-09-28）：M1（引擎骨架）已完成并经深度评审收口**——M0 已收官（八轮代码评审、screeps-ok 上游基准独立复现零偏差 vanilla 2897 / xxscreeps 2642、[ADR-010](docs/decisions/ADR-010-screeps-ok-adapter-bridge.md) 一实例一线程原型验证通过）。M1 交付：kk-core/kk-schema/kk-world typed world/kk-engine 单房间结算（movement 仲裁/资源经济/孵化链）、**embed 通道切真实结算**（screeps-ok 适配器经 typed world + settle_room 推进世界）、**通过率棘轮进 CI**（第四 job，地板见 [compat/reports/ratchet.json](compat/reports/ratchet.json) 唯一真相源；M0 起点为 237；架构结论见 [ADR-011](docs/decisions/ADR-011-engine-skeleton.md)）；收官后 5 路 subagent 深评修复 4 high + 13 medium + 3 low（详见[台账追记](compat/reports/m0-code-review.md)）。bot 验收：tooangel 房间管线完整执行（300 tick）、overmind 推进到 RoomIntel 全量执行（2000 tick 零中断、缺失 API 清零，bot-check 退出码机器可判定）。下一步 M2：真沙箱接引擎与玩家 API 层。

## 技术定案摘要

| 决策点 | 结论 | 定案记录 |
|---|---|---|
| 语言与工程 | Rust + cargo workspace（单仓库） | [ADR-001](docs/decisions/ADR-001-rust-monorepo.md) |
| 玩家代码沙箱 | deno_core（V8 引擎），唯一后端，无备选 | [ADR-002](docs/decisions/ADR-002-deno-core-sandbox.md) |
| 玩家侧兼容性 | 游戏脚本 API 零变动零差异，优先级最高 | [ADR-003](docs/decisions/ADR-003-player-compatibility.md) |
| 核心架构 | 房间快照 + 流水线 + 索引 + 分层存储 | [ADR-004](docs/decisions/ADR-004-core-architecture.md) |
| 本地开发套件 | 模拟器与服务器同一个二进制 | [ADR-005](docs/decisions/ADR-005-dev-kit.md) |
| 兼容验收 | screeps-ok 行为测试套件进 CI | [ADR-006](docs/decisions/ADR-006-screeps-ok-ci.md) |
| 预期收益 | CPU 约 1/5~1/10、内存约 1/5~1/10；5 个以上进程 + 2 个中间件（MongoDB、Redis）→ 单个二进制 | [调研底稿](docs/research/) |

## 文档目录

| 目录 | 内容 |
|---|---|
| [docs/designs/](docs/designs/) | **功能设计**：玩家可见行为与工具的设计（兼容性红线、开发套件……） |
| [docs/architecture/](docs/architecture/) | **技术架构**：模块划分、游戏循环、沙箱、存储与传输 |
| [docs/research/](docs/research/) | **调研底稿**：四份主报告 + 15 份笔记（另有 1 份实验输出存档），全部决策的证据链 |
| [docs/decisions/](docs/decisions/) | **决策记录**：十条 ADR 与否决方案清单 |
| [docs/roadmap.md](docs/roadmap.md) | **路线图**：里程碑 M0–M5 的内容与验收关卡 |

新读者建议顺序：[decisions](docs/decisions/)（为什么这么做）→ [architecture](docs/architecture/)（要建什么）→ [designs/compatibility.md](docs/designs/compatibility.md)（什么不能动）→ 按需查 [research](docs/research/)。

## 仓库布局

```
docs/            文档（见上表）
compat/          兼容性验收（TS 包）：screeps-ok 适配器（经 kkscreeps embed sidecar）、
                 bot 验收工具与验收报告——见 compat/README.md
crates/          Rust 引擎工作区（九个 kk-* crate，见 docs/architecture/overall.md）
apps/ packages/  JS 工作区：Vue3 客户端 + 渲染器/API/编辑器/共享类型库（M3 前为占位包），
                  见 docs/architecture/frontend.md
scripts/         仓库工具脚本（screeps-ok pin 克隆、bot 拉取、文档链接检查）
spikes/          技术验证工程（cargo crate，使命已完成，保留作证据）
├── deno-spike/    ★ 主力验证：沙箱基础 / 游戏 API 交互模式 / 指令回调 / 预算桶仿真
│                  运行：cargo run --release [--example game_api | op_callback | bucket_sim]
├── rquickjs-spike/（评估后淘汰，保留作证据）
└── boa-spike/     （评估后淘汰，保留作证据）
```

## 路线图

里程碑 M0–M5 的内容与验收关卡见独立文档 [docs/roadmap.md](docs/roadmap.md)。要点：只有范围和验收、没有时间估算；排序风险驱动（最大的不确定性最先消灭）；screeps-ok 通过率棘轮贯穿全程。

## 参考源码位置

官方与社区源码克隆在 `~/Desktop/projects/sources/screeps/`：官方 engine / common / driver / launcher / renderer、社区重写项目 xxscreeps（最重要的架构参照）、backend-npm 与 pathfinding-npm（官方仓库已私有化，从 npm 发布包解包补齐）、screeps-cn.github.io（官方中文文档，功能面盘点来源）、screeps-ok（行为一致性测试套件）。

## 附录：术语速查

| 术语 | 含义 |
|---|---|
| tick | 游戏的一个节拍。服务器按 tick 推进世界；官方服一个 tick 约 2~5 秒 |
| creep（爬爬虫） | 玩家代码控制的基本游戏单位，由最多 50 个部件组成 |
| intent（指令） | 玩家代码下达的动作指令（如移动、采集），由服务器在 tick 末统一结算 |
| CPU | 玩家每 tick 分到的计算配额（约等于毫秒），可攒（桶上限 10000）、可透支（单 tick 上限 500） |
| isolate | V8 引擎内部的隔离运行环境；每个玩家一个，互不可见 |
| 房间快照 | 一个房间的全部状态编码成的一段连续二进制数据（本项目内部数据格式） |
| screeps-ok | 以官方服务器为基准的 Screeps 行为一致性测试套件（2897 个用例），本项目的兼容验收关卡 |
| ADR | 架构决策记录（Architecture Decision Record），见 docs/decisions/ |

> 单机承载量（并发玩家数）暂不定指标：待 M2 里程碑有可运行系统后以基准实测设定。
