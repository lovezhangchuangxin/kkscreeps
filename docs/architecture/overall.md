# 总体形态与模块划分

当前实现是一个可嵌入调用的内存游戏引擎，配套玩家沙箱、兼容性验收和性能采集工具。现已提供显式完整状态[检查点与冷恢复](checkpoints.md)；完整服务器、数据库/日志持久化与前端属于目标架构；本页先说明已经存在的模块和数据流。

## 运行入口

`kk-cli` 构建可执行程序 `kkscreeps`。命令分派在 `src/main.rs`，各命令实现位于 `src/` 下对应模块（`sim/`、`export_fixtures/` 等目录模块随子命令增长）。

| 命令 | 当前行为 |
|---|---|
| `embed` | 通过 stdin/stdout 接收逐行 JSON 请求，串行处理创建分片、放置对象、执行玩家代码和查询等操作 |
| `bot-check` | 加载 bot 模块，在固定测试世界中运行并输出运行错误和 API 访问统计 |
| `perf-probe` | 在采集、升级和孵化负载下测量投影、玩家执行、结算与整轮耗时 |
| `mod-perf-probe` | 同一 link 冷却负载的 native、空 Mod 与完整处理链纯结算对照；逐对象验证状态与链位计数 |
| `sim run` | 加载版本化 JSON 场景与玩家模块，推进真实世界、检查断言并写报告，见[本地场景模拟](local-simulation.md) |
| `export-fixtures` | 只读导出渲染器夹具：固定 seed 的脚本世界推进 N tick，逐 tick 落 world_json（JSONL，确定性输出，前端 build-fixtures 消费） |
| `serve`、`test`、`import` | 预留入口，调用时返回“尚未实现”和退出码 2 |

`embed` 使用自定义的 `{id, method, params}` / `{id, ok, result或error}` 消息格式，不是标准 JSON-RPC 2.0，也不提供网络监听。接入方式见 [兼容性验收](../../compat/README.md)。

目标是让服务器与本地开发工具共用引擎实现，减少规则分叉；不同配置、输入和外部环境仍需分别验证，不能仅凭共用二进制保证运行结果一致。开发工具设计见 [dev-kit.md](../designs/dev-kit.md)。

## 模块职责

| crate | 当前职责与边界 |
|---|---|
| `kk-core` | 游戏常量、房间坐标、地形编码和随机数工具 |
| `kk-vm` | 玩家与玩法 VM 共用的执行线程 CPU 时钟、墙钟看门狗、V8 安全点资源检查；不包含游戏规则或两类脚本的 API |
| `kk-mod-api` | 玩法描述、启动校验、冻结计划与玩家面目录；已接入 link 规则与对象域 Mod 意图，不代表所有默认玩法已迁移 |
| `kk-mod-host` | 可信 JS 包装载、注册冻结与整槽处理链执行；通过 ChainHost 访问候选，不依赖引擎或玩家 VM |
| `kk-schema` | 二进制快照读写、对象字段定义、布局宏和版本升级接口；接口存在不等于已有历史版本迁移 |
| `kk-world` | 内存世界、房间和对象管理、空间索引，以及沙箱数据和查询快照的投影 |
| `kk-engine` | 清洗指令，结算房间和市场规则，验证并提交世界、玩家及市场的内存候选状态 |
| `kk-runner` | V8 沙箱、CommonJS 模块加载、玩家 API 预检查、指令收集、墙钟预算和堆限制 |
| `kk-runtime` | 玩家世界数据注入、执行编排、Memory/segments/CPU 候选提交及失败重试 |
| `kk-pathfinder` | 封装 Screeps C++ 寻路算法、宿主回调与取消接口；玩家面经 kk-runner 同步执行玩家 roomCallback、代价矩阵与嵌套搜索，引擎面供 NPC 结算（Source Keeper 巡守）Rust 直调 |
| `kk-storage-bench` | 用合成数据比较 redb 与 libmdbx；结果用于选型研究，不是游戏持久化实现 |
| `kk-storage` | 预留模块，尚无存储接口或数据库后端 |
| `kk-backend` | 预留模块，尚无 HTTP 路由、监听或 WebSocket 服务 |
| `kk-cli` | 命令行参数和嵌入式调用、bot 检查、性能采集入口 |

前端 `apps/game` 和 `packages/*` 目前是占位包。目标职责见 [frontend.md](frontend.md)。`compat` 是验收工具，通过子进程调用引擎；`spikes` 保留独立实验，不属于 Rust workspace。

## 实际依赖

下表仅列 workspace 内的直接依赖，依据各 crate 的 `Cargo.toml`。它描述编译依赖，不表示每项接口都已经接入所有调用路径。

| crate | 直接依赖 |
|---|---|
| `kk-cli` | core、engine、runtime、mod-host、mod-api、runner、world |
| `kk-runtime` | core、schema、mod-api、mod-host、engine、runner、world |
| `kk-engine` | core、mod-api、pathfinder、schema、world |
| `kk-world` | core、mod-api、schema |
| `kk-mod-api` | core |
| `kk-mod-host` | core、mod-api、vm |
| `kk-schema` | 无内部依赖 |
| `kk-runner` | core、vm、pathfinder、schema，world（仅测试），使用 deno_core 等外部库 |
| `kk-vm` | 无内部依赖，使用 deno_core 与 libc |
| `kk-storage-bench` | core、schema、world |
| `kk-core`、`kk-pathfinder`、`kk-storage`、`kk-backend` | 无内部依赖 |

表中的简称均指 `kk-*` crate。实际引擎并不通过 `kk-storage` 读写；`kk-pathfinder` 有两个消费面——玩家沙箱经 kk-runner 的 op 通道执行玩家 roomCallback，引擎 NPC 结算（Source Keeper 巡守）在 Rust 侧直调寻路、不依赖 V8；完整服务的目标依赖不应代替这张实际依赖表。

## 数据流与状态边界

嵌入式执行的主要路径如下：

1. `kk-runtime::EmbedEngine` 从已提交世界构造按玩家视野过滤的 JSON 或 KKPV 数据，注入该玩家的 V8 沙箱。
2. `kk-runner` 执行玩家代码。JavaScript API 检查所有权、距离、资源和参数等条件，接受的动作加入指令收集器；返回 `OK` 表示接受指令，不表示动作已结算成功。
3. 宿主收集指令、RawMemory、segments、计费候选和日志。Rust 指令入口再次清洗字段并绑定注册玩家身份，不信任玩家自报的身份。
4. 协调器构造玩家 Memory、segments 和 CPU 桶写回候选，交给 `kk-engine` 随房间与市场规则在副本中处理。无扩展内建槽直调 Rust；已接线的 JS 贡献槽由 runtime 一次进入可信 Mod realm，JS 组合处理链、Rust 校验并读写同一候选。
5. 引擎验证后统一发布世界、玩家和市场状态，协调器返回运行结果。世界结算失败时缓存运行结果及 `TickInput`，重试不再次执行玩家代码。

玩家已修改的 V8 全局变量不随世界回退；线程通信失败、panic 或进程崩溃也不能直接套用内存结算失败的保证。具体边界见 [游戏循环](game-loop.md)和 [tick 契约](tick-contract.md)。

可信 Mod VM 在整轮结算返回错误后销毁，恢复按分片原包快照与配置重新注册并核验原计划，保留世界和玩家候选；恢复失败阻止新执行。Mod 模块全局随重建丢弃，持久状态仍由 Rust 候选与发布点负责。

这里有三种不同的数据表示：

| 表示 | 用途 |
|---|---|
| `kk-world::world_json` | 供沙箱装配 `Game` 的原始世界 JSON |
| `kk-world::snapshot` | 供 `getObject` 等查询使用的对象 JSON |
| `RoomState` 与 `kk-schema` 编码 | 房间状态和二进制快照读写，不自行写入磁盘 |

沙箱注入默认使用 JSON，也可显式选择 KKPV 二进制块束；结果导出仍使用 JSON。KKPV 已有玩家通路与等价验证，不等于所有对象零复制；完整状态的磁盘保存由独立检查点入口负责。块生命周期、检查点保存和崩溃恢复见 [快照契约](snapshot-contract.md)与 [存储设计](storage.md)。

## 当前采用的系统边界

项目适合继续采用单进程的分层内核：玩家读取已提交投影，沙箱只产出意图与写回候选，Rust 结算和协调器共同完成整轮验证与发布。房间是局部工作单元，市场、跨房行为和玩家写回仍须遵守全轮提交边界；不能仅凭房间划分就独立提交。玩家线程已可并发执行，房间结算并行与流水线仍是后续能力。

玩法扩展以启动冻结计划描述对象、规则和槽；可信玩法 VM 与不可信玩家 VM 分开。组合 JS 函数留在玩法 VM，权威状态与领域校验留在 Rust，避免每层 wrap 都重新借用整个 VM。默认规则继续保留 native 路径；当前不为未开放的规则提供虚假的通用扩展保证。机制见 [Mod 执行](mod-execution.md)，取舍见 [ADR-015](../decisions/ADR-015-mod-chain-execution.md)。

先完成这一读写、提交与失败契约，再扩大规则写面、补齐 VM 预算与故障处置，之后接持久化、网络与客户端。现有 crate 边界足以支撑这条路线；目前没有负载证据要求改成微服务、引入统一 ECS 或把所有规则改写为 JS。后续并行优化、存储选型与进程隔离分别用真实负载和故障验收决定。

## 规则与测试边界

玩家 API 预检查由 `kk-runner` 实现，最终规则和状态更新由 `kk-engine` 处理，世界字段投影由 `kk-world` 提供。维护行为时应同时核对预检查、指令清洗、结算和下一轮投影，避免 API 返回成功而实际动作被遗漏。

玩家执行参数与结果 `RunSpec`、`RunOutcome`、`RunResult` 定义在 `kk-runtime`，由 `ThreadExecutor` 调用玩家线程产出。`kk-engine` 不持有玩家沙箱，不再提供 `Engine::tick` 或测试沙箱接口；只接收显式指令和写回候选。无 V8 的结算测试直接使用测试夹具提交 `IntentBatch`。执行与重试边界见 [tick 契约](tick-contract.md#玩家执行与结算接口)。

`kk-pathfinder` 复用上游算法及其方向和平局规则；输入转换、回调异常和玩家 API 接线仍需单独验证，不能由“复用源码”推导出完整兼容性。

## 目标架构与来源

房间任务并行、玩家与房间结算重叠、共享快照、数据库与输入日志持久化、网络服务和前端尚需分别实现与验收。详细要求见 [架构索引](README.md)，取舍依据见 [ADR-004](../decisions/ADR-004-core-architecture.md)、[ADR-013](../decisions/ADR-013-architecture-review-coordinator.md)和 [ADR-014](../decisions/ADR-014-contribution-plane.md)。

常量来源为 Screeps common，指令规则和玩家 API 主要对照 Screeps engine，原生寻路对照 Screeps driver。具体版本与符号在相应源码中注明；原始研究见 [研究索引](../research/README.md)。
