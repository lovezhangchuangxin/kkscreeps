# Screeps Rust 重构可行性研究报告

> 调研日期：2026-09-27
> 调研对象：官方开源服务端（engine/common/driver/launcher/renderer）、社区重构版 xxscreeps、npm 补齐的 backend 与 pathfinding、行为一致性测试套件 screeps-ok，以及 Rust 生态选型调研。
> 结论先行：**可行，且预期能将资源开销降低一个数量级（10x 量级）**。但最大收益来自架构重构（消灭 JSON 序列化 / IPC / 存储写放大 / 流水线串行等待），Rust 本身是放大器而非唯一来源；风险最高的是"玩家 JS 沙箱"这一唯一无法纯 Rust 化的模块。

---

## 0. 调研范围与仓库盘点

参考源码位于 `~/Desktop/projects/sources/screeps/`：

| 目录 | 来源 | 状态 |
|---|---|---|
| `engine/` | github.com/screeps/engine（v4.3.2，2026-09 活跃） | 已有 |
| `common/` | github.com/screeps/common（v2.16.1） | 已有 |
| `driver/` | github.com/screeps/driver（v5.3.0，含 C++ 寻路源码） | 已有 |
| `launcher/` | github.com/screeps/launcher（v4.2.0） | 已有 |
| `renderer/` | github.com/screeps/renderer（PixiJS 7 + 全套 SVG 美术） | 已有 |
| `xxscreeps/` | github.com/laverdet/xxscreeps（2026-09 活跃） | 已有 |
| `screeps-room-planner/` | 第三方规划工具 | 已有 |
| `backend-npm/` | npm `@screeps/backend@3.3.0` 解包 | **本次补齐**（GitHub 仓库已私有化） |
| `pathfinding-npm/` | npm `@screeps/pathfinding@0.4.17` 解包 | **本次补齐**（legacy 纯 JS 寻路） |
| `screeps-ok/` | github.com/misterwise/screeps-ok | **本次补齐**（行为一致性测试套件） |

仍然闭源 / 不在本地的内容：
- `@screeps/storage`（storage 进程，闭源 npm 包，但其 RPC 协议可从 `common/lib/storage.js` 完整反推）；
- 官方 MMO 服的运营层（官网、支付、赛季系统）；
- Steam 客户端壳（但 renderer + 美术开源，可自组客户端）。

两处常见误解的澄清（本次调研确认）：
1. **官方 renderer 开源且包含全套美术资源**（`renderer/metadata/images/*.svg`，宽松许可，`renderer/LICENSE.txt`），"美术闭源"的说法不成立；
2. **xxscreeps 当前主线没有用 LMDB**，其默认存储是自研 "Redis-like 接口 + 内存数据结构 + blob 文件 + 周期 checkpoint"（`packages/xxscreeps/engine/db/storage/local/`），可选切真 Redis。

---

## 1. 官方架构剖析（现状基线）

### 1.1 进程拓扑

launcher 拉起 5+ 个独立 Node.js 进程（`launcher/lib/start.js:176-224`）：

```
storage（1）── 单点数据总线：JSON-RPC over TCP/unix socket
   ├── backend（1）   HTTP REST + sockjs WS + CLI + cronjobs
   ├── engine_main（1）tick 编排：users 队列 → rooms 队列 → global → gameTime++
   ├── engine_runner×N（默认1，各 RUNNER_THREADS=2~4 并发）  isolated-vm 跑玩家 JS
   └── engine_processor×N（默认2）  房间结算（intents 执行、状态推进）
```

- 所有进程对 storage 的每一次 db/env/queue/pubsub 调用都是 **4 字节长度前缀 + JSON 帧的 RPC**（`common/lib/rpc.js:26-53`）——没有任何二进制协议。
- 私服默认 storage 落地是 **lokijs 单文件 db.json**（模拟 Mongo/Redis API）；官方 MMO 服是 MongoDB + Redis（`engine/src/config.js:6-10` 残留配置为证）。
- runner/processor 每小时主动重启一次以"回收内存"（`launcher/lib/start.js:139-141`）——官方自己承认长跑内存问题的间接证据。

### 1.2 Tick 流水线（串行屏障是最大结构性问题）

`engine/src/main.js:11-127`，每 tick：

1. `tickStarted` pubsub；
2. **所有玩家代码**（users 队列，runner 消费）——`whenAllDone` 阻塞等待；
3. **所有房间处理**（rooms 队列，processor 消费）——再次阻塞等待；
4. global 阶段（跨房移动 / power / market）；
5. `gameTime+1`，`roomsDone` pubsub。

即：**官方引擎必须等全部玩家脚本跑完才开始处理任何房间**。xxscreeps 的 README 直接点名了这一条的浪费（"the processor service just sits around doing nothing until *all* player scripts are done"）。

房间内处理顺序（`engine/src/processor.js:16-520`）：对象预处理 → NPC intents（Keeper/Invader/Stronghold）→ 玩家 intents（按 user → objectId 插入序）→ movement 仲裁（四级 rate 排序）→ 逐对象按**固定类型顺序** tick（invaderCore → energy → source → … → storage，`processor.js:342-419`）→ 快照/历史/事件日志 → bulk 写回。

### 1.3 Intent 系统

- 沙箱内 `intents.set/push`（每个 intent 计 0.2 CPU，`driver/lib/runtime/runtime.js:60-61`）→ `ivm.ExternalCopy` 拷出 → runner 重组为 `{room: {objectId: {...}}}` JSON → 白名单 sanitize（`common/lib/system.js:16-140`，字段类型表 + 截断 + price 定点化）→ 写 `rooms.intents` 集合（`$merge` upsert）→ processor `findOne` 读出执行后清除。
- 跨房移动 / market / power 走 `users.intents`，global 阶段统一结算。
- 顺序保证 = 三重循环自然顺序 + movement 统一仲裁；没有优先级子系统。

### 1.4 玩家代码沙箱（isolated-vm）

- 每用户一个持久 V8 Isolate（按 userId 缓存，代码变更才重建），内存上限 `256MB + 地形共享缓冲`（`driver/lib/runtime/user-vm.js:30`），空闲 3 分钟回收。
- 沙箱由 V8 snapshot + webpack 打包的 `runtime.bundle.js` 构建（webpack 3，2017 年）。
- **每 tick 把玩家可见的全部世界状态 `ivm.ExternalCopy` 深拷贝进 isolate**（`driver/lib/runtime/make.js:144-160`）——约 20 个并行 DB/env 查询拼装（`runtime/data.js:99-264`）。
- CPU 计量 = `isolate.cpuTime` 纳秒累计 + 0.2×intents（`runtime.js:202-204`）；预算 `min(cpuAvailable + user.cpu, 500)`，bucket 10000；强制手段 = 模块级 `runSync(timeout)` + 外层 5s watchdog 销毁 isolate。

### 1.5 寻路

- 正式路径是 driver 内置 **C++ 原生模块**（`driver/native/src/pf.cc`，613 行，JPS/A*，laverdet 写）；CostMatrix 以 `Uint8Array(2500)` 零拷贝直传，地形经 SharedArrayBuffer 全图共享。
- `@screeps/pathfinding`（纯 JS）只是 `PathFinder.use(false)` 时的 deprecated 后备。

### 1.6 序列化与性能热点清单（Rust 重构的靶子）

| # | 热点 | 位置 |
|---|---|---|
| 1 | 全部跨进程 RPC 走 JSON 帧 stringify/parse | `common/lib/rpc.js:42,48-53` |
| 2 | 每 tick 每 active 房间全量 `JSON.stringify` 快照进 history | `engine/src/processor.js:532` |
| 3 | creep/powerCreep 每 tick `JSON.parse(JSON.stringify())` 历史快照 | `processor.js:431-439` |
| 4 | 玩家可见世界每 tick structured clone 深拷贝进沙箱 | `driver/lib/runtime/make.js:145-160` |
| 5 | 整个 constants 表**每用户每 tick** `jsonParse(jsonStringify(C))` | `engine/src/game/game.js:435` |
| 6 | market orders/history 每次访问深拷贝 | `game/market.js:21,50,57,61` |
| 7 | 玩家 Memory 每 tick JSON round-trip（≤2MB） | `game/game.js:484-486`、`runtime.js:246-248` |
| 8 | bulk diff 用 lodash `cloneDeep` | `driver/lib/bulk.js:38,65` |
| 9 | processor 侧无空间索引，O(n) 全对象扫描（movement 障碍判定等） | `movement.js:16-39, 202`、`utils.js:479-504` |
| 10 | q promise 链 + 每秒轮询 unhandled rejection 遍布热路径 | `main.js:144-150` 等 |

### 1.7 代码规模与技术债

| 模块 | 行数 |
|---|---|
| engine/src（游戏逻辑本体） | 16,559（其中 game/ 沙箱 API 7,119，processor/ 7,773） |
| driver/lib（不含 C++） | 2,481 |
| driver/native（C++ 寻路） | 1,011 |
| common（constants 数值表 1,591 行 + 存储/RPC/intent schema） | 2,566 |

技术债：q 1.x（2014）+ lodash 3.10.1（2015）+ webpack 3（2017）+ isolated-vm 锁死 commit；`eval()` 动态生成访问器；大段死代码；错误处理粗糙（房间处理失败仅 log 后照常 markDone）；Babel es2015 全量转译。**规模属于"小到中等"——1.7 万行游戏逻辑，对 Rust 重写是可控的范围。**

### 1.8 可直接复用的资产

- `common/lib/constants.js`：全部数值表（BODYPART_COST、BOOSTS、REACTIONS、POWER_INFO、MARKET 费率…），纯数据，可直接转 Rust 常量模块；
- `common/lib/system.js` 的 `intentTypes` 白名单 schema（校验规则逐条照抄）；
- `common/index.js` 的房间名编解码 / 地形编解码 / `getDiff` 差分算法；
- `common/lib/strongholds.js` 的据点布局数据；
- `driver/native/src/pf.cc`：C++ 寻路可经 FFI 直接复用或参照重写；
- `renderer/`：PixiJS 7 渲染引擎 + 全套 SVG（宽松许可），客户端可复用。

---

## 2. xxscreeps 架构分析（最重要的参照系）

xxscreeps（原作者 laverdet 的 TypeScript 重写，pnpm monorepo，2026-09 仍活跃）证明了官方玩法可以在一套现代化架构上 1:1 复刻。其核心思想一句话：

> **把每个房间变成一个自描述、可版本化、可懒读的小二进制镜像（room blob），所有服务都是这个镜像的无状态读者 / 单写者，用 pubsub + 计数器协调流水线。**

### 2.1 关键设计

1. **Room blob + 双缓冲**：房间全状态编码为 250B–60KB 二进制（wall 44B、满编 creep ≈274B），key 为 `room{time%2}/<name>`，runner 读 T-1、processor 写 T，天然无锁并行；未变化房间 copy-forward 几乎零成本（`engine/db/shard.ts:153-168`）。落盘是周期 checkpoint（默认 2 分钟），热路径全在内存。
2. **自研二进制 schema（wire format）**：类 C 内存布局（struct/variant/vector/optional/enumerated），字段自动对齐打包，字符串 Latin-1/UTF-16 自适应；schema 归档存 DB，**版本不匹配自动升级重写，免停服迁移**（`engine/schema/`）。
3. **零拷贝 + 懒物化**：blob 以 SharedArrayBuffer 直接进玩家 isolate，游戏对象是 `(BufferView, offset)` 薄壳，字段经原型 getter 按需读取——玩家整 tick 没碰 `creep.body`，它就从未进入 V8 堆，GC 压力大减（`schema/overlay.ts`、`README.md:29-34`）。
4. **run/process 重叠**：不再等全部玩家跑完；per-room 计数器（zset `ZINCRBY -1`）归零即触发该房间处理（`engine/processor/model.ts:107-131`）。
5. **休眠房间**：无玩家房间进 sleepingRooms，整 tick 只 copy-forward——稀疏世界几乎零成本。
6. **单进程模式**：`launcher.singleThreaded: true` 时四服务同进程（玩家沙箱仍是独立 isolate）；多进程时经 worker_threads/unix socket。
7. **Intent 架构升级**：注册式 processor，启动时拓扑排序出确定性顺序 + 冲突位掩码；runner 侧与 processor 侧用**同一个 check 函数双重校验**（runner 不可信，processor 是唯一权威写者）。
8. **存储抽象**：`KeyValProvider/PubSubProvider` trait（刻意模仿 Redis 命令面），本地实现与真 Redis 实现可互换——单机与 MMO 共用一套原子语义。
9. **Mod = 贡献合并**：11 类 provide 点（game/processor/schema/main/terrain…）拓扑排序注册，连 `Creep` 都是 mod。
10. **C++26 寻路**：NAPI 二进制 + isolated-vm NativeModule 双形态，玩家调 `PathFinder.search` 不跨 isolate；LTO/ICF/PGO 全开。

### 2.2 尚存的问题（Rust 版可以进一步解决的）

- 仍是 V8：每 isolate 数 MB、GC 抢玩家 CPU、webpack/babel 构建链复杂；
- 仍是 JS 引擎侧的运行时布局计算、JSON diff 推送、`JSON.parse` 双重 stringify 的包管理；
- 无 replay 系统（官方 standalone 同样把 replay 留给 mod）；
- 无正式 benchmark 数字（README 只有定性声明 + 一条 Reddit 实测：12 个并行世界 × 512 steps 几秒内完成，"官方引擎要数天"）。

---

## 3. 性能开销的定量证据

| 事实 | 数字 | 来源 |
|---|---|---|
| 官方 DB 单点瓶颈 | 2017 年"托管商最强数据库服务器 100% 饱和"，被迫引入 sharding | blog.screeps.com/2017/06/optimizations |
| 官方 tick 时长 | 2–5s/shard；shard0 实测 4.5–5.5s（目标 ~3.6s） | Steam 页面 / forum topic/3070 |
| 私服推荐配置 | **2 核 + 2GB**（空/中等世界持续占用） | docs.screeps.com/private-server |
| 私服已知问题 | 内存泄漏 + CPU 30 分钟爬满整机（2020） | forum topic/2940 |
| 玩家 Memory JSON 开销 | 占玩家 CPU ~4%（知名玩家 Dissi 实测） | forum topic/85 |
| xxscreeps 房间 blob | 250B–60KB/room | xxscreeps README |
| xxscreeps 实测 | RL 用例 12 并行世界 × 512 steps 几秒完成 vs 官方引擎数天 | Reddit r/screeps 1vrb5o8 |
| rkyv vs JSON | 序列化快 ~14x（212µs vs 3.02ms），体积 ~1/3，零拷贝字段访问 ~1ns | djkoloski/rust_serialization_benchmark |
| QuickJS vs V8 | 内存 ~200KB vs 数 MB/isolate；速度慢 5–10x（无 JIT） | 多来源（见 §5.1） |

**结论：官方栈的开销主要来自架构（JSON 序列化 + Mongo 写放大 + Redis/RPC 往返 + 全局屏障等待 + 每 tick 全量快照），而非游戏逻辑本身。** 这正是为什么 xxscreeps 仅靠改架构（仍用 V8）就能拿到数量级提升，也是 Rust 重构的收益上限所在。

---

## 4. Rust 技术栈是否成立：逐组件选型

### 4.1 玩家 JS 沙箱（唯一无法纯 Rust 的模块，风险最高）

| 方案 | 速度 | 内存/isolate | CPU 计量与打断 | 兼容性 | 判断 |
|---|---|---|---|---|---|
| **rquickjs（quickjs-ng）** | 慢 5–10x | **~200KB** | `JS_SetInterruptHandler` 字节码级原生支持 + `JS_SetMemoryLimit` 硬限 | ES2023 几乎完整（async/generators 均可） | 内存与计量最优，速度是代价 |
| **deno_core（rusty_v8）** | 最快（JIT） | 数 MB + 运行时 ~30MB | 需自建 watchdog 线程 + `TerminateExecution` | 完整（Chrome 同款） | 速度最优，计量与内存工程量大 |
| wasmtime + Javy | 最慢 | 线性内存硬限 | epoch/fuel 最强 | eval/new Function 受限，不兼容 | 不适合 Screeps 语义 |

建议：**trait 抽象 + 双后端**。默认 deno_core（V8，兼容性与玩家体验对齐官方），提供 rquickjs 后端作为低内存档位。理由：
- Screeps 玩家生态（tooangel 等开源 bot、lodash3 全局 `_`）按 V8 语义写；
- CPU 预算制下"玩家代码慢 5-10x"意味着同样预算内做的事变少，会改变玩家体感公平性——迁移期保持 V8 语义最稳；
- 内存优势可以靠"单进程内多 isolate 复用 + 持久 isolate"（官方已是每用户持久 isolate，Rust 版省掉的是 Node 进程壳与 IPC，而不是 isolate 本身）。
- 生产先例：Deno Subhosting（V8 多租户不可信 JS + WatchDog 终止）、AWS LLRT（Rust+QuickJS）。**"Rust 服务端内嵌 JS 跑不可信玩家代码的完整 MMO"无先例——本项目将是首个公开先例，风险与机会并存。**

必须复刻的沙箱语义：CPU = isolate CPU 时间 + 0.2/intent；bucket 10000 / 上限 500；模块级 timeout 强制中断；内存 256MB+地形；RawMemory 2MB；segments 100KB×10；lodash3 全局 `_`；`Game.cpu.getUsed()/halt()`。

### 4.2 存储与 IPC

- **单机/私服形态（首要目标）**：进程内直连，消灭 IPC。世界状态 = per-room blob（xxscreeps 模式），活跃内存 slab + 双缓冲，周期 checkpoint 落盘。落盘引擎选型：**redb**（纯 Rust ACID B+树，单文件）或 **libmdbx**（mmap COW，随机读极强）；写密集大世界可选 **Fjall**（LSM）。跨机 MMO 形态才需要 FoundationDB / Redis 兼容层。
- blob 编码用 **rkyv**（零拷贝字段访问，schema 声明宏生成偏移常量）或直接移植 xxscreeps 的 wire format 设计（variant/vector/Latin-1 字符串），布局计算放编译期。
- intent 流用 **postcard/bincode**；对外 REST/WS 保持 JSON 兼容（客户端生态依赖）。

### 4.3 并行与确定性

- 玩家代码只产生 intents、processor 统一结算的语义天然利好并行：**rayon per-room 并行** + run/process 重叠（per-room 计数器触发）。
- 确定性服务于 replay 与跨实例一致：固定房间处理顺序（reduce 阶段按确定性顺序）、`BTreeMap/IndexMap` 固定迭代序、定点数或约束浮点用法、固定种子 PRNG。

### 4.4 网络/API 与前端

- **axum** + tokio（WebSocket、tower 中间件生态）替代 Node backend；
- API 兼容面（复用官方客户端 / xxscreeps client / screeps-ok 的依据）：`/api/version`（protocol 14 + serverData.renderer 注入渲染元数据）、`/api/auth/*`（含 HMAC token，兼容私服工具）、`/api/user/*`（code/memory/console/branches…）、`/api/game/*`（room-terrain/map-stats/place-spawn/add-object-intent…）、sockjs `/socket` 文本协议（`auth/subscribe/gzip`）与 `room:<name>`、`user:<id>/*` 频道；
- 前端直接复用 `renderer/`（PixiJS 7 + SVG 全开源，宽松许可），或走 xxscreeps client 路线从 Steam 目录 serve 官方客户端。

### 4.5 推荐组件映射总表

| 官方组件 | 现状 | Rust 替代 |
|---|---|---|
| engine main/processor | Node 多进程 + q promise | 单进程 tokio + rayon per-room 并行 |
| runner（isolated-vm） | V8 isolate + Node 进程壳 | deno_core isolate 池（进程内）/ rquickjs 档位 |
| storage 进程 + Mongo + Redis + JSON-RPC | 单点总线 + lokijs/Mongo | 进程内 blob store + redb/libmdbx checkpoint；跨机时 Redis/FDB trait 实现 |
| 全 JSON 序列化 | 每帧 stringify/parse | rkyv blob（编译期布局）+ postcard intents + 对外 JSON 兼容层 |
| C++ pathfinder | nan addon | 直接 FFI 复用 pf.cc / 移植为 crate（JPS + u32 打包坐标 + 只返回拐点） |
| backend（express/sockjs） | npm 闭源包 | axum + sockjs 兼容协议 |
| renderer | PixiJS 7 + SVG（开源） | 原样复用，喂官方格式 state |
| 验收 | 无 | **screeps-ok 行为一致性套件**（vanilla 2897 tests / xxscreeps 2642 passing，可插 adapter） |

### 4.6 Monorepo 布局建议（cargo workspace）

```
kkscreeps/
├── crates/
│   ├── kk-core        # constants（从 common/lib/constants.js 生成）、房间名/地形编解码、PRNG
│   ├── kk-schema      # blob wire format：声明宏 → 编译期布局 + reader/writer + 版本归档/升级
│   ├── kk-world       # 世界模型：room blob、对象 variant、空间索引、休眠房间
│   ├── kk-engine      # tick pipeline：intent registry（拓扑排序+冲突位掩码）、movement 仲裁、global 阶段
│   ├── kk-runner      # 沙箱抽象 trait + deno_core 后端 + rquickjs 后端 + CPU 计量
│   ├── kk-storage     # KeyVal/PubSub trait + 进程内实现 + redb/libmdbx checkpoint + redis 后端
│   ├── kk-pathfinder  # pf.cc 移植/FFI + CostMatrix 语义
│   ├── kk-backend     # axum REST + sockjs WS + auth + cronjobs
│   └── kk-cli         # import/export/generate-room/单进程启动
├── compat/            # screeps-ok adapter、官方 API 兼容测试、导入器
├── docs/
└── Cargo.toml         # workspace
```

---

## 5. 预期收益评估（回答"能否极大降低开销"）

**能，量化推理如下**（对比基线：私服 2C2G、tick ~1s+、内存随玩家数线性膨胀）：

1. **消灭 JSON 序列化层**（RPC 帧/历史快照/constants 深拷贝/Memory round-trip）：rkyv 快 ~14x、体积 ~1/3、字段零拷贝。官方每 tick 全部跨进程流量都在这条路上，收益直接乘在 tick 主路径。
2. **消灭 IPC 与存储中间件**：MongoDB+Redis+storage 进程 → 进程内 blob + 周期 checkpoint。私服形态下 2GB 内存预算里的大头（Mongo/Redis/多 Node 进程壳）直接消失；磁盘从"每 tick 增量文档写 + 全量快照"变为"每 tick 内存双缓冲 + 2 分钟 blob checkpoint"。
3. **消灭全局屏障**：run/process 重叠 + rayon per-room 并行（xxscreeps 已证明语义可行），tick 时长从"所有玩家串行等待"变为"受最慢房间约束"。
4. **玩家 isolate 内存**：官方每用户 V8 isolate 数 MB 且带 Node 进程壳；Rust 版单进程内持久 isolate 复用（deno_core），或 QuickJS 档位每用户 ~200KB 级。百名玩家量级的私服，内存从 GB 级降到百 MB 级是合理预期。
5. **无 GC 引擎侧**：processor 侧 Rust 原生代码，官方 processor.js 的 O(n) 扫描换成空间索引（官方沙箱侧本有 spatial 索引，processor 侧没有——`game/game.js:41-47` vs `movement.js:16-39`）。
6. **休眠房间 copy-forward**：稀疏世界（私服常态）几乎零成本。

保守综合估计：**同等世界规模下 CPU 约 1/5–1/10、内存约 1/5–1/10、运维面从 5+ 进程 + 2 中间件变为单二进制**。对照锚点：xxscreeps 仅完成架构现代化（仍 V8）已拿到数量级提升（Reddit 实测），Rust 版在其之上再叠加无 GC、无 Node 壳、编译期布局三项。

**唯一可能变慢的点**：若选 QuickJS 档位，玩家 JS 执行慢 5–10x（CPU 预算制下=玩家能做的事变少）。这就是建议默认 V8（deno_core）档位的原因。

---

## 6. 风险与关键决策点

| 风险 | 等级 | 缓解 |
|---|---|---|
| JS 沙箱 CPU 计量/硬中断在 deno_core 上无现成方案 | **高** | 参照 Deno Subhosting WatchDog 模式自建；isolated-vm 的做法（laverdet 已为 V8 打过同样的补丁）可参考；先做 spike 验证再定后端 |
| 玩家 API 语义兼容面巨大（game/ 7,119 行沙箱 API） | 高 | screeps-ok 2897 项行为测试做验收关卡；constants/intent schema 直接搬运；对照官方源码逐模块移植 |
| 工程量（1.7 万行逻辑 + backend + 客户端兼容） | 高 | monorepo 按模块推进（见 §7 里程碑）；玩法拆 crate 可插拔（xxscreeps mod 思路） |
| blob schema 演进 | 中 | 照搬 xxscreeps 的布局归档 + 自动 upgrader 设计 |
| replay 系统官方本就缺位 | 低 | 事件溯源（intent 日志 + blob 双缓冲）天然支持，反而成为新实现的优势 |
| 确定性漂移（浮点/迭代序） | 中 | 定点数或约束浮点、BTreeMap、固定房间序、replay 校验 |
| 单人长期维护风险 | 中 | 里程碑严格垂直切分，每个阶段都有可运行交付物 |

---

## 7. 建议路线图

1. **M0 spike（1–2 周量级）**：deno_core 内嵌 V8 跑一段不可信 JS，实现 CPU 时间计量 + watchdog 终止 + 内存上限 + snapshot 启动 —— 全项目最高风险项先行验证；
2. **M1**：kk-core + kk-schema + 单房间 tick（无沙箱，脚本内置 intents），跑通 movement/combat/spawn 核心循环 + screeps-ok 子集；
3. **M2**：kk-runner（沙箱 + 完整玩家 API）+ kk-storage（blob + checkpoint）→ 单机可玩私服形态；
4. **M3**：kk-backend（axum + sockjs 兼容）+ 复用 renderer → 浏览器可玩；
5. **M4**：market/power/inter-shard、import 官方世界数据、多 shard、Redis/FDB 分布式后端；
6. **M5**：性能基准与官方/xxscreeps 对照报告，QuickJS 档位，运维工具。

---

## 8. 参考文件索引（本地）

- tick 主循环：`sources/screeps/engine/src/{main,runner,processor}.js`、`engine/src/processor/global.js`
- 沙箱与运行时：`sources/screeps/driver/lib/runtime/{make,user-vm,runtime,data}.js`
- 存储 RPC 协议：`sources/screeps/common/lib/{storage,rpc}.js`
- intent 白名单：`sources/screeps/common/lib/system.js`；数值表：`common/lib/constants.js`
- 进程编排：`sources/screeps/launcher/lib/start.js`、`launcher/init_dist/.screepsrc`
- API 面：`sources/screeps/backend-npm/lib/game/{server,api/*,socket/*}.js`
- xxscreeps 架构文档：`sources/screeps/xxscreeps/docs/architecture.md`；schema：`packages/xxscreeps/engine/schema/`
- 验收套件：`sources/screeps/screeps-ok/`（adapter 可插）
- 渲染器：`sources/screeps/renderer/`（engine/ + metadata/ 含全套 SVG）

主要网络来源：blog.screeps.com（2017 Optimizations、2018 renderer 开源公告）、docs.screeps.com/private-server、screeps.com/forum topic/85、3070、2940、djkoloski/rust_serialization_benchmark、quickjs-ng / rquickjs / deno_core 文档、Reddit r/screeps 1vrb5o8。
