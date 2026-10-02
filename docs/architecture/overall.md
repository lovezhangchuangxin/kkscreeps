# 总体形态与模块划分

当前实现是一个可嵌入调用的内存游戏引擎，配套玩家沙箱、兼容性验收和性能采集工具。完整服务器、持久化与前端属于目标架构；本页先说明已经存在的模块和数据流。

## 运行入口

`kk-cli` 构建可执行程序 `kkscreeps`。各命令的实现位于其 `src/main.rs`、`embed_main.rs`、`bot_check.rs` 和 `perf_probe.rs`。

| 命令 | 当前行为 |
|---|---|
| `embed` | 通过 stdin/stdout 接收逐行 JSON 请求，串行处理创建分片、放置对象、执行玩家代码和查询等操作 |
| `bot-check` | 加载 bot 模块，在固定测试世界中运行并输出运行错误和 API 访问统计 |
| `perf-probe` | 在采集、升级和孵化负载下测量投影、玩家执行、结算与整轮耗时 |
| `serve`、`sim`、`test`、`import` | 预留入口，调用时返回“尚未实现”和退出码 2 |

`embed` 使用自定义的 `{id, method, params}` / `{id, ok, result或error}` 消息格式，不是标准 JSON-RPC 2.0，也不提供网络监听。接入方式见 [兼容性验收](../../compat/README.md)。当前没有 `export` 子命令。

目标是让服务器与本地开发工具共用引擎实现，减少规则分叉；不同配置、输入和外部环境仍需分别验证，不能仅凭共用二进制保证运行结果一致。开发工具设计见 [dev-kit.md](../designs/dev-kit.md)。

## 模块职责

| crate | 当前职责与边界 |
|---|---|
| `kk-core` | 游戏常量、房间坐标、地形编码和随机数工具 |
| `kk-mod-api` | 玩法描述、启动校验和冻结计划；尚未接入生产引擎或 JS Mod |
| `kk-schema` | 二进制快照读写、对象字段定义、布局宏和版本升级接口；接口存在不等于已有历史版本迁移 |
| `kk-world` | 内存世界、房间和对象管理、空间索引，以及沙箱数据和查询快照的投影 |
| `kk-engine` | 清洗指令，结算房间和市场规则，验证并提交世界、玩家及市场的内存候选状态 |
| `kk-runner` | V8 沙箱、CommonJS 模块加载、玩家 API 预检查、指令收集、墙钟预算和堆限制 |
| `kk-runtime` | 玩家世界数据注入、执行编排、Memory/segments/CPU 候选提交及失败重试 |
| `kk-pathfinder` | 封装 Screeps C++ 寻路算法与 Rust 回调接口；尚未接入玩家沙箱的 PathFinder 搜索 |
| `kk-storage-bench` | 用合成数据比较 redb 与 libmdbx；结果用于选型研究，不是游戏持久化实现 |
| `kk-storage` | 预留模块，尚无存储接口或数据库后端 |
| `kk-backend` | 预留模块，尚无 HTTP 路由、监听或 WebSocket 服务 |
| `kk-cli` | 命令行参数和嵌入式调用、bot 检查、性能采集入口 |

前端 `apps/game` 和 `packages/*` 目前是占位包。目标职责见 [frontend.md](frontend.md)。`compat` 是验收工具，通过子进程调用引擎；`spikes` 保留独立实验，不属于 Rust workspace。

## 实际依赖

下表仅列 workspace 内的直接依赖，依据各 crate 的 `Cargo.toml`。它描述编译依赖，不表示每项接口都已经接入所有调用路径。

| crate | 直接依赖 |
|---|---|
| `kk-cli` | core、engine、runtime、runner、world |
| `kk-runtime` | core、schema、engine、runner、world |
| `kk-engine` | core、schema、world |
| `kk-world` | core、schema |
| `kk-mod-api` | core |
| `kk-schema` | 无内部依赖 |
| `kk-runner` | core，使用 deno_core 等外部库 |
| `kk-storage-bench` | core、schema、world |
| `kk-core`、`kk-pathfinder`、`kk-storage`、`kk-backend` | 无内部依赖 |

表中的简称均指 `kk-*` crate。实际引擎并不通过 `kk-storage` 读写，也没有依赖 `kk-pathfinder`；完整服务的目标依赖不应代替这张实际依赖表。

## 数据流与状态边界

嵌入式执行的主要路径如下：

1. `kk-runtime::EmbedEngine` 从已提交世界构造按玩家视野过滤的 JSON 数据，注入该玩家的 V8 沙箱。
2. `kk-runner` 执行玩家代码。JavaScript API 检查所有权、距离、资源和参数等条件，接受的动作加入指令收集器；返回 `OK` 表示接受指令，不表示动作已结算成功。
3. 宿主收集指令、RawMemory、segments、计费候选和日志。Rust 指令入口再次清洗字段并绑定注册玩家身份，不信任玩家自报的身份。
4. `kk-engine` 在候选副本中处理房间与市场规则，验证后更新内存世界、玩家和市场状态。
5. 世界结算成功后，协调器提交玩家 Memory、segments 和沙箱 CPU 桶候选。世界结算失败时缓存运行结果及 `TickInput`，重试不再次执行玩家代码。

玩家已修改的 V8 全局变量不随世界回退；线程通信失败、panic 或进程崩溃也不能直接套用内存结算失败的保证。具体边界见 [游戏循环](game-loop.md)和 [tick 契约](tick-contract.md)。

这里有三种不同的数据表示：

| 表示 | 用途 |
|---|---|
| `kk-world::world_json` | 供沙箱装配 `Game` 的原始世界 JSON |
| `kk-world::snapshot` | 供 `getObject` 等查询使用的对象 JSON |
| `RoomState` 与 `kk-schema` 编码 | 房间状态和二进制快照读写，不自行写入磁盘 |

当前沙箱注入和结果导出仍使用 JSON。共享二进制视图、检查点保存和崩溃恢复的设计见 [快照契约](snapshot-contract.md)与 [存储设计](storage.md)。

## 规则与测试边界

玩家 API 预检查由 `kk-runner` 实现，最终规则和状态更新由 `kk-engine` 处理，世界字段投影由 `kk-world` 提供。维护行为时应同时核对预检查、指令清洗、结算和下一轮投影，避免 API 返回成功而实际动作被遗漏。

`kk-engine::Sandbox` 与 `ScriptedSandbox` 用于无 V8 的引擎测试；真实执行由 `kk-runtime::ThreadExecutor` 调用玩家线程。两者共享结果类型，但不能将测试接口描述为真实 V8 接线的全部路径。

`kk-pathfinder` 复用上游算法及其方向和平局规则；输入转换、回调异常和玩家 API 接线仍需单独验证，不能由“复用源码”推导出完整兼容性。

## 目标架构与来源

房间任务并行、玩家与房间结算重叠、共享快照、持久化、网络服务和前端尚需分别实现与验收。详细要求见 [架构索引](README.md)，取舍依据见 [ADR-004](../decisions/ADR-004-core-architecture.md)、[ADR-013](../decisions/ADR-013-architecture-review-coordinator.md)和 [ADR-014](../decisions/ADR-014-contribution-plane.md)。

常量来源为 Screeps common，指令规则和玩家 API 主要对照 Screeps engine，原生寻路对照 Screeps driver。具体版本与符号在相应源码中注明；原始研究见 [研究索引](../research/README.md)。
