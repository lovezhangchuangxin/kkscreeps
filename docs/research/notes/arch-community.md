# 社区与官方情报调研笔记（Screeps 引擎性能史 / xxscreeps / screeps-ok / 2024-2026 官方动向）

> 调研日期：2026-09-27
> 调研员：社区情报调研员（本轮）
> 调研对象：官方 blog/forum/Steam 公告、docs.screeps.com、laverdet（xxscreeps 作者）公开陈述、screeps-ok 行为一致性测试套件、社区 CPU 优化讨论。
> 本地参照源码根目录：`sources/screeps/`（含官方 backend/driver/engine/pathfinding、xxscreeps、screeps-ok 三方代码，均为 2026-09 快照）。
> 姊妹篇：`docs/research/notes/arch-processor.md`（processor/intents 热路径实测）、`arch-runner.md`（runner 侧）。

**置信级别标注约定**（全文适用）：
- 【官方】= 官方开发者（artch/o4kapuk）在 blog/forum/Steam/官方文档中的一手陈述，附 URL；
- 【源码】= 本地源码验证，附 `file:line`；
- 【作者声称】= laverdet 在 xxscreeps 仓库中的自述（一手但无独立基准数据）；
- 【社区经验】= 玩家社区讨论，可信但非官方，附 URL。

---

## 1. 官方优化史完整时间线（artch 团队）

这是理解"官方引擎为什么长这样"的主线。每一代性能危机 → 每一次官方应对，都直接塑造了当前架构，也是 Rust 重写要逐一拆掉的补丁。

| 时间 | 事件 | 关键陈述/数字 | 级别 |
|---|---|---|---|
| 2016-02-19 | **C++ 原生寻路器发布**（`PathFinder` 模块） | "It is implemented in fully native C++ code and makes use of JPS algorithm which drastically improves performance"。官方未给出具体倍数 | 【官方】[blog 2016-02-19](https://blog.screeps.com/2016/02/Changelog-2016-02-19/) |
| 2016-04-13 | 新 PathFinder 默认启用（`PathFinder.use(false)` 可回退），`Game.map.findRoute` 完全重写，`heuristicWeight` 默认 1.2 | — | 【官方】[blog 2016-04-13](https://blog.screeps.com/2016/04/Changelog-2016-04-13/) |
| 2017-06-12 | **Optimizations roadmap**：承认 tick 明显变慢；**数据库是最大瓶颈**——"the most powerful machine available from our hoster currently runs our database at 100% of its capacity"；试过 DB sharding/replication（"unacceptable overhead"）、试过更换 DBMS（"didn't work out as well"）；宣布三条路线：isolated-vm（laverdet 为 Screeps 专门写的 V8 isolate 隔离库，堆可整体序列化跨节点迁移，消灭 reset storm、GC 互相影响）、世界分片、WebGL 渲染器。**新 shard 目标 2 秒 tick** | — | 【官方】[blog 2017-06](https://blog.screeps.com/2017/06/optimizations/) |
| 2017-08-03 | **World Shards 上线**：从"星型"（中央 DB + 边缘 runtime 服务器）改为"多个松耦合星型"，每 shard 独立 DB/地图/runtime；shard0 tick 5 秒，shard1 更快 | — | 【官方】[blog 2017-08](https://blog.screeps.com/2017/08/shards/) |
| 2018-01-18 → 03-05 | isolated-vm 进 PTR 后上线（私服 v2.13.0），玩家脚本获得完全隔离堆 | — | 【官方】[forum tid 2073](https://screeps.com/forum/topic/2073)、[blog 2018-03-05](https://blog.screeps.com/2018/03/changelog-2018-03-05/) |
| 2019-09-30 | **集群整体迁移**：新集群用 **Intel i9-9900K**，实测**用户 CPU 提升 15–30%**、**tick 快 20%（shard3 达 2.2s/tick）**；架构改为 Kubernetes 容器池，一键克隆生产集群做测试；runtime 服务器升级 Node.js 10 | — | 【官方】[forum tid 2800](https://screeps.com/forum/topic/2800) |
| 2021-01-08 | 托管商基础设施故障 → 全 shard tick 变慢数日（应急降速） | — | 【官方】[forum tid 3140](https://screeps.com/forum/topic/3140) |
| 2024-01-03 | **重大社区事件**：发现"CPU stealing"漏洞（部分玩家窃取超额算力）；封号潮暴露 10–15 个/人的多账号；后续遭遇 review bombing、账号被黑、**对官方基础设施的 DoS、游戏代码泄露**（"the leaking of stolen game code"） | — | 【官方】[forum tid 3285](https://screeps.com/forum/topic/3285) |
| 2025-12-11 | **论坛转入归档模式**，官方沟通全部转移到 Discord（artch 发帖） | — | 【官方】[forum tid 3637](https://screeps.com/forum/topic/3637) |
| 2025-12-15 | **Arena and World Roadmap 2026**：团队"从全国性制裁中恢复并迁出俄罗斯"后重拾 World 开发；Arena 采用"每场一个代码实例、无 Memory、直接对象引用"模式 | — | 【官方】[Steam 新闻 2025-12-15](https://store.steampowered.com/app/464350)（见 §5） |
| 2026-03-17 / 04-01 | **Node.js v10 → v24 全量升级**（runtime 服务器自 2019 年起停留在 Node 10 长达 7 年）；官方原话 "The current version v10 is long overdue"、"This upgrade will give your scripts a noticeable performance boost"；客户端 Pixi v7；私服二进制同步更新 | — | 【官方】[Steam 新闻 2026-03-17 / 2026-04-01](https://store.steampowered.com/app/464350) |
| 2026-04-23 | **新 shard "shardX"**：tick 快 2 倍——"All standard shards are now set to **1000 ticks per hour**, and shardX will have **2000 ticks per hour**"（即标准 shard 3.6s/tick、shardX 1.8s/tick）；Access Key 访问制（无 key 禁 claimController/upgradeController/reserveController，新错误码 ERR_ACCESS_DENIED）；API 新增 `Game.shard.access/accessTime/activateAccess` | — | 【官方】[Steam 新闻 2026-04-23](https://store.steampowered.com/app/464350) |
| 2026-05-28 | 十周年（Steam EA 2016-06-20）；shard0/E11S52 有 2016-12-29 出生至今未死的 creep "Mia"；Season 10 免费；Season 赛制固定为每 3 个月一届（Season 11 于 2026-09-01 开） | — | 【官方】[Steam 新闻 2026-05-28 / 2026-08-25](https://store.steampowered.com/app/464350) |

官方 engine 仓库本身仍活跃：`screeps/engine` v4.3.2（2026-06-01），2026-04-19 "restricted shards support"（即 shardX 访问制）、2026-09-23 仍在迁移测试框架。【源码】GitHub API：`repos/screeps/engine` pushed_at=2026-09-23。

**对 Rust 重写的直接启示**：
1. MongoDB 文档模型 + 星型 DB 是官方两次架构危机的根源（2017 分片、被迫用"世界分裂"来解决）。官方自己给的最强机器都扛不住 → 房间本地化状态 + 可线性扩展存储是第一优先级。
2. 官方性能演进 = "换更快的 CPU + 空间分裂"（i9-9900K +15~30%、分片、shardX 2x tick）——从未动过单进程 JS 引擎本身的热路径。Rust 重写直接吃掉这条未走的路。
3. CPU stealing 漏洞 + 2024 泄码事件说明：计费与隔离必须内建于引擎边界，不能依赖 JS 层约定。

---

## 2. 官方架构与 tick 耗时构成的泄露信息

### 2.1 官方架构页数字（docs.screeps.com/architecture.html）【官方】

来源：<https://docs.screeps.com/architecture.html>（描述约 2017-2019 时代的集群）：

- 栈：Node.js（当时 8.9.3，2019 后为 10，2026-04 起为 24）+ MongoDB + Redis；"20k lines of server side JavaScript code"。
- 算力：**40 台四核专用服务器（OVH），共 160 个 Intel Xeon E3-1231 v3 核心**；"the number of parallel processes strictly corresponds to the CPU cores number"——每核同步处理一个房间或一个玩家，"rules out various race conditions"（用并行度换无锁，官方明说是设计选择）。
- 数据库：**"MongoDB for each shard runs on 24-core machine with 128 GB of RAM and handles 30k update requests per second"**；WiredTiger 文档级并发；**"All the game data is stored in MongoDB. Each game object is a separate database document"**（每个游戏对象一个文档！）；更新在 tick 处理完后 bulk 批量执行；磁盘 flush 每分钟一次。
- Tick 两阶段：玩家脚本计算 → 命令处理；Redis List 做任务队列，runtime 服务器从队列拉任务、"request the DB data the player needs"、跑脚本、收集命令。玩家运行时"optimized to work in memory and does not make any hard drive or database requests"（数据预载入 RAM）。
- 沙箱：`vm.runInContext`（该页较旧，2018 后为 isolated-vm）；失控脚本连 fork 一起杀掉重建。

### 2.2 tick 耗时构成的一手泄露【源码】

官方从未公布过生产 tick 耗时分解，但私服代码把阶段结构完整暴露了：

- **tick 阶段序列**（`sources/screeps/engine/src/main.js:28-91`）：`getUsers → addUsersToQueue → waitForUsers → getRooms → addRoomsToQueue → waitForRooms → commit1 → global → commit2 → incrementGameTime → ...`。**`waitForUsers`（`main.js:30-33`，`usersQueue.whenAllDone()`）是全局屏障：所有玩家脚本跑完之前，房间处理一行都不开始**。每个阶段都 emit `mainLoopStage` 事件（`main.js:16` 起），可被外部 stats 工具订阅——阶段计时钩子是官方留给社区的。
- **tick 节奏**：`main.js:118-120` — `usedTime < mainLoopMinDuration` 时补齐延迟，`console.log('Tick done in X ms, next tick in Y ms')`；默认 `mainLoopMinDuration: 1000`（`sources/screeps/driver/lib/index.js:21`），私服即 1s/tick，可经 env `MAIN_LOOP_MIN_DURATION` 调（`driver/lib/index.js:121-125`）。
- **生产侧已知 tick 长度**（时间线汇总）：shard0 ~5s（2017）→ ~6s（2020，社区帖）；shard3 2.2s（2019-09，官方）；shard1 目标 2s（2017-06，官方规划）；标准 shard 3.6s、shardX 1.8s（2026-04，官方）。【官方/社区混合，各条见上表】

### 2.3 CPU 计费模型（官方文档 + 源码）

- 【官方】CPU 定义为"duration of time in milliseconds"：<https://docs.screeps.com/cpu-limit.html>；bucket 上限 10,000、单 tick 最多透支 500；`Game.cpu.tickLimit` ≥ `Game.cpu.limit`。
- 【源码】**intent 常量计费**：`sources/screeps/driver/lib/runtime/runtime.js:63` — `intentCpu = 0.2`；`runtime.js:64` — `say`/`pull` 为 `freeMethods`；`runtime.js:67-70` — 同一对象同名 intent 重复提交不重复计费（`if (!freeMethods[name] && !this.list[id][name]) this.cpu += intentCpu`）。
- 【源码】玩家 CPU = 沙箱墙钟 + intent 费用：`runtime.js:203-205` — `getUsedCpu = nowCpuTime() + intents.cpu - startTime`。
- 【官方】**Memory 的 JSON 解析/序列化计入玩家 CPU**："The Memory object is stored in the stringified form and is parsed each time upon the first in the tick access... The CPU cost of this method execution is counted as your script expense"（<https://docs.screeps.com/global-objects.html>）。即官方把存储层成本直接转嫁为玩家预算。

---

## 3. 源码级证据：官方引擎的四大结构性成本

（processor 内部热路径已在 `arch-processor.md` 实测覆盖，此处只列"社区/官方情报视角"能补强的结构性事实。）

### 3.1 Memory 全量 JSON 往返【源码】

- 首次访问 `Memory` getter：`sources/screeps/engine/src/game/game.js:484-486` — `JSON.parse(runCodeCache[userId].memory.get() || "{}")`（整段字符串全量解析）。
- tick 结束全量写回：`sources/screeps/driver/lib/runtime/runtime.js:247` — `data.userMemory.data = systemFunctions.JSON.stringify(rawMemory._parsed)`。
- 官方文档确认这笔账算在玩家头上（§2.3）。

### 3.2 每玩家每 tick ~19 次数据库/env 查询【源码】

`sources/screeps/driver/lib/runtime/data.js:85`（`exports.get`）：第一批 `q.all` 共 **14 个查询**（`data.js:131-145`）：`rooms.objects.find({user})`、`users.findOne`、`users.code.findOne`、`env.get(MEMORY+userId)`、`users.console.find`、`getGametime`、`rooms.find`、`rooms.objects.find`（同房间他人对象）、`accessibleRooms`、`transactions` ×2（各取最近 100 条）、`rooms.flags.find`、`roomEventLog` hmget、`users.power_creeps.find`、`roomStatusData`；第二批再 5+ 个（`data.js:225-231`：`users.find`、market orders/history、玩家挂单、active segments）。这就是 laverdet 所说"several costly serialization and deserialization steps"的具体形状。

### 3.3 `Room.find` 的 per-tick 缓存【源码】

`sources/screeps/engine/src/game/rooms.js:584-648`：`register.findCache[type][roomName]` 每 tick 缓存未过滤结果（`rooms.js:587-589`），但 **`opts.filter` 每次调用都重跑**（`rooms.js:639-646`），无 filter 时也要 `_.clone` 整个结果数组（`rooms.js:643`）。FIND_EXIT 系列直接 4 次 find 拼接（`rooms.js:594-597`）。社区"find 很贵"的体感即来自 filter+clone 的叠加。

### 3.4 历史快照的 JSON 成本【源码】

- 活跃房间每 tick 全量 stringify 进历史：`sources/screeps/engine/src/processor.js:531-534` — `var data = JSON.stringify(objects); ... driver.history.saveTick(roomId, gameTime, data)`（实测约 100KB/房间/次，见 `arch-processor.md` §0）。
- creep/powerCreep 逐个深拷贝：`processor.js:431-432` — `objectsToHistory[object._id] = JSON.parse(JSON.stringify(object))`。

---

## 4. laverdet（xxscreeps 作者）公开性能陈述全集

laverdet = 官方 C++ 寻路器（`driver/native/src/pf.cc:1` 署名 "Author: Marcel Laverdet"）与 isolated-vm 的作者，xxscreeps 是其"从头重写 Screeps 后端"项目。以下全部来自其一手文本（本地 2026-09 快照 + GitHub）。

### 4.1 README 性能主张【作者声称】

来源：`sources/screeps/xxscreeps/README.md`（GitHub: <https://github.com/laverdet/xxscreeps>）：

- **Blob 二进制房间格式**（`README.md:13-18`）：每房间一个 250B–60KB 的 blob；`StructureWall` 44 字节、满配 50 部件 creep 约 274 字节——"游戏状态像 C 程序一样表示"。
- **免序列化传输**（`README.md:22-27`）：游戏状态以 `ArrayBuffer` 读入，可"*instantly* transferred into the player's runtime for free"；对比 vanilla"requesting game state from the database and loading it into the runner requires several costly serialization and deserialization steps"（即 §3.2/3.3 的直接指控）。
- **惰性物化 overlay**（`README.md:29-34`）：getter 直接从 blob 读字段，整 tick 未访问的属性（如 `creep.body`）永不进 V8 堆——"This greatly reduces the burden of the garbage collector which robs every player of a significant portion of CPU"。
- **可线性扩展存储**（`README.md:36-38`）：房间 blob "no different than a very small JPEG"，可按 processor worker 数量线性扩容。
- **runner/processor 并行**（`README.md:40-44`）："room processing can begin as soon as all player scripts in a single room have completed... **Right now in vanilla Screeps the processor service just sits around doing nothing until *all* player scripts are done**"（对应 `main.js:30-33` 的全局屏障）。

### 4.2 架构文档补充【作者声称】

来源：`sources/screeps/xxscreeps/docs/architecture.md`：

- 四服务（main/runner/processor/backend）各持独立 JS 堆，除 main 外都可多实例（`architecture.md:52-65`）；房间 blob 双缓冲 `room0/room1` 按 `time % 2` 键控（`architecture.md:234-236`）。
- **runner best-effort / processor 权威**（`architecture.md:107-109`）：慢 runner 的 intents 可被放弃以保住 tick 节奏；处理器必须处理所有房间。
- **Intent 双重验证**（`architecture.md:135-166`）：runner 侧校验给玩家返回值、processor 侧重跑同一 `check*`；"The player's CPU budget pays only for the runner-side check; processor re-validation is on the server's time. **The duplicated work is the price of the untrusted-runner architecture**"——Rust 重写若沿用该模型要为这 2x 校验留预算。
- 沙箱两种模式：isolated-vm（默认）与 `sandbox: unsafe`（node `vm`，"This is faster and useful when you're only running your own code"，`architecture.md:102-106`）。

### 4.3 项目活跃度与 2026 新寻路包【源码/GitHub】

- 仓库活跃：`repos/laverdet/xxscreeps` pushed_at = **2026-09-25**，93 stars（2026-09-27 查）。
- **2026-08~09 laverdet 又重写了寻路器**：`sources/screeps/xxscreeps/packages/pathfinder/`（`@xxscreeps/pathfinder` v0.4.6）——C++20 modules 实现（`src/astar.cc`、`src/jps.cc`、`src/heap.cc`…），napi 预编译 6 平台（`package.json` napi.targets），带 isolated-vm 直绑（`iv.ts`）。提交序列：`pf: dial's heap`（2026-08-16）→ `pf: version package` → **`engine: use the new pathfinder`（2026-08-18 与 2026-09-25 两次）**。即 vanilla 2016 的 pf.cc 又被其原作者迭代了一轮，方向 = 模块化 C++ + 分发预编译二进制 + IV 零开销绑定。
- 真实部署细节（PR #369，merged 2026-08-17）：默认并发改为 `os.availableParallelism()`；作者实测环境为 **Kubernetes pod、20 线程宿主机（6 P-core + 8 E-core）`taskset -c 0-12` 亲和**——xxscreeps 已被容器化部署跑真实世界。
- 注意：README 的性能主张（§4.1）**没有附带公开基准数字**，属【作者声称】级；但方向与 §3 的源码证据完全互洽。

---

## 5. 2024-2026 官方服务器/版本动向汇总【官方】

来源：Steam 新闻 API（app 464350 "Screeps: World"，2026-09-27 抓取）+ 论坛归档。排序后时间线：

1. **2024-01-19** Season 6 开（2 个月）；**2024-08-13** Season 7 开。Season 规则：全员 GCL/GPL 1、100 CPU、无市场、无 portal。
2. **2024-01-03** CPU stealing 漏洞 → 封号潮 → DoS/泄码报复（§1 表）。官方开始修订 ToS、起草 Fair Play Expectations。
3. **2025-12-11** 论坛归档（只读），官方沟通全面转向 Discord。
4. **2025-12-15** Roadmap 2026：团队迁出俄罗斯；Arena 收尾两更新（Trophies、Decorations）后"fully shifting our focus back to World"；World 2026 计划 = 常规 Season（每 3 个月）、装饰系统改进、新 Terminal Nodes 机制、**两个新 Power Creep 职业**、新 Screeps IDE。
5. **2026-02** Season 8 起固定 3 个月一届；console.log 拆分 `console.log`（服务端过滤 HTML）/`console.logUnsafe`。
6. **2026-04-01** Node.js v24 上线（v10 → v24，7 年欠账一次还清，官方称将给玩家脚本"noticeable performance boost"）；Pixi v7 + Arena 渲染优化回移；私服二进制/Electron 更新（含 ARM macOS 修复）。
7. **2026-04-23** shardX 上线（2x tick，1000→2000 ticks/hour；Access Key 门禁 + `Game.shard.access` API + ERR_ACCESS_DENIED）；私服 engine 4.3.1 "restricted shards support"（2026-04-19）对应。
8. **2026-05-28** 十周年；Season 10 免费（2026-06-01 开）；**2026-08-25** Season 11（2026-09-01 开，11 月 1 日结束）。

**判断**（本调研员观点）：官方 2026 年处于"重启活跃开发但资源主要在 Arena"的状态；性能策略仍是空间换时间（shardX 双倍 tick 靠新增 shard 而非引擎提速），唯一引擎级大动作是 Node 24 升级。这为 Rust 重写留出了明确的差异化窗口。注意官方还持有未发布的官方引擎闭源部分（backend），2024 年已发生过泄露事件。

---

## 6. 社区 CPU 优化战争故事（对引擎设计有启发的部分）

### 6.1 moveTo/PathFinder 成本【社区经验】

- [forum tid 3005 "Is moveTo the enemy of cpu?"（2020-06）](https://screeps.com/forum/topic/3005)：
  - likeafox："moveTo is inconsistent in cpu costs; **It blows up sometimes**… I believe this has to do with the cached matrices and they being recalculated or cleaned up. The more rooms you are moving through, the more matrices that are created"（跨 3 个房间 = 3 份 CostMatrix）。玩家已经把 vanilla 寻路的成本抖动归因到矩阵重建。
  - ayrtep：把 PathFinder 产出的路径串（每步 1 字符）缓存进 Memory/segments 供 moveByPath 复用后，"**CPU went from around 2\*creep numbers to 0.4 creep numbers**"——寻路+移动占总 CPU 的 ~80% 的间接证据。
  - 社区方案谱系：`reusePath`（默认 5 tick）、Traveler 库、20 个 RawMemory segments 存路径 + heap 常驻。
- `arch-processor.md` 的本地实测与上述互洽（50 creep 房间 movement 层 ~2.3ms/tick，其中矩阵构建 0.33ms）。

### 6.2 find()/API 微观成本【社区经验】

- [forum tid 3179 "CPU tips and tricks"（2021-06）](https://screeps.com/forum/topic/3179)：最大单项节省来自猴补 `Structure.prototype.isActive`（对 container/road 恒真、RCL8 恒真并自记忆化）——说明哪怕官方 API 内部的一次属性计算在玩家侧都可感知。`isActive` 语义见 `engine/src/game/structures.js`（processor 侧 `structureIsActive`）。
- `Room.find` 的 filter 重跑 + `_.clone`（§3.3）是社区"别在循环里 find"经验帖的普遍背景（散见于 tid 3179 等；未找到单独量化帖——如实记录：无官方数字）。

### 6.3 慢 tick 的政治经济学【社区经验】

- [forum tid 3070（2020-09）](https://screeps.com/forum/topic/3070)：shard0 tick ~6s、11,339 可占房间（vs shard3 的 7,076）、无 CPU 上限（老玩家 300 CPU）——玩家算出订阅价值被慢 tick 蒸发 33%；官方 2018 年曾整圈停用 shard0 外环房间以提速（引 <https://twitter.com/ScreepsGame/status/994137796574089216>）。
- [forum tid 3262（2022-10）](https://screeps.com/forum/topic/3262)：shard3 平均 3s、某日突增到 11s；deft-code 称"I think the Dev's are targeting a tick duration of 5 seconds"（社区推测，非官方）。
- 启示：tick 速率是付费公平性问题（CPU/订阅按 tick 计价），Rust 引擎若把 tick 做快，还顺带修复了官方一直没解决的经济公平性。

### 6.4 Memory 成本【官方+社区】

- 官方文档明说 Memory 解析计玩家 CPU（§2.3）+ 源码全量往返（§3.1）；社区对策是迁往 RawMemory segments（10 个活跃 × 100KB 上限）。任何"Memory 瘦身"都是新手劝退帖的固定主题（无单一权威帖，散布于 help 版块；如实记录）。

---

## 7. screeps-ok：覆盖范围与用法细节

来源：`sources/screeps/screeps-ok/`（GitHub: <https://github.com/misterwise/screeps-ok>，MIT；created 2026-04-01，pushed 2026-09-27，极活跃）。

### 7.1 定位与覆盖【源码】

- 口号："_If your engine agrees, it's Screeps._"——**一次编写、对任意引擎运行的行为一致性测试套件**（`README.md:10-11`）。
- 当前进度（`docs/status.md`，2026-09-27 00:03 UTC 全量跑）：**vanilla 适配器 2897 passing / 10 expected-fail / 7 skipped**；**xxscreeps 适配器 2642 passing / 77 expected-fail / 195 skipped**（`README.md:6`、`docs/status.md` 表格）。expected-fail = 已登记的 parity gap，全部有理由说明。
- 测试目录 34 个领域（`ls tests/`）：movement、pathfinding、harvesting、resource-transfer、construction-repair、controller、combat、boosts、spawning-lifecycle、structures-×5、room-mechanics、source-mineral-deposit、game-objects、power、market、map、roomposition、store-api、intent-resolution、**memory、object-shapes、undocumented、deprecation** 等。
- 行为目录 `behaviors.md`：条目格式 `ID` + `behavior|matrix` + oracle（`verified_vanilla` / `needs_vanilla_verification`），规则明确要求"catalog 覆盖到与 vanilla 行为不可区分"。

### 7.2 适配器契约（Rust 引擎接入方式）【源码】

- 必须实现 `ScreepsOkAdapter`（`docs/adapter-spec.md:44-69`）：`capabilities`、`createShard`、typed placement helpers、`placeObject`、`setTerrain`、`runPlayer(s)`、`tick`、`getObject`、`findInRoom`、`getGameTime`、`captureActionLog`、invader-raid 三件套、`getControllerPos`、`captureConsoleLogs`、`teardown`——共 ~20 个方法。
- 核心不变量（`adapter-spec.md:26-42`）：每测试一个隔离新世界；玩家句柄（`p1`/`p2`/`sk`/`srcKeeper`）不透明；setup 走 typed spec、玩家代码是 `PlayerCode` 字符串、检查走纯 JSON snapshot；**适配器不得重演游戏语义、不得掩盖引擎 bug**（`adapter-spec.md:18-24`）。
- 现有两个适配器：xxscreeps（进程内包 `simulate()` API，快）与 vanilla（spawns `screeps-server-mockup` 引擎子进程，串行）。运行方式：`npm test -- xxscreeps tests/05-construction-repair/5.1-build.test.ts`、过滤 `-t "..."`、`npm run parity` 全量双引擎对拍（`README.md:40-81`）。
- alpha 期消费方式：clone 仓库就地跑；npm 发布是 beta 门槛（`README.md:97-104`）。**对 Rust 重写**：写一个 Rust adapter（进程内暴露同等 JSON 面即可，参照 xx 适配器路径而非 vanilla 子进程路径），把 2897 个 vanilla 基准用例当作重写期间的行为护栏。

### 7.3 副产品：已定案的 vanilla bug 清单【源码】

`docs/status.md` 登记 8 个 vanilla parity gap，每个都附上游 PR 引用——Rust 重写可直接"修正"并注明与 vanilla 的分歧：
1. `pull` 蔓延疲劳在 puller TTL 死亡时滞留被拉 creep（顺序依赖 quirk；上游 screeps/engine#156 open）。
2. `Structure.destroy()` 不发 `EVENT_OBJECT_DESTROYED`（引擎 `_damage.js:51`/`_die.js:97` 才发）。
3. power creep 名字静默截断到 50 字符（`global-intents/power/createPowerCreep.js:21`、`renamePowerCreep.js:16`，而 API 声称 100）。
4. 未孵化 power creep `ticksToLive` 返回 NaN 而非 undefined（`power-creeps.js:72`；engine#148）。
5. `Game.market.getHistory` 空 resource 返回类型错误（`market.js:41-52`；engine#131）。
6. `findClosestByPath` 忽略 `opts.range`（`rooms.js:304-374` 硬编码 range 1；engine#121）。
7. 疲劳 + `visualizePathStyle` 时 moveTo 重算路径（`creeps.js:173,241-302`；engine#152）。
8. 全路线被堵时 moveTo 仍走进 creep（engine#63）。

---

## 8. 对 Rust 重写的优化启示（汇总表）

| # | 优化点 | 官方/社区证据 | Rust 对应做法 | 预期收益（依据与量级） |
|---|---|---|---|---|
| 1 | 消灭 Memory 全量 JSON 往返 | `game.js:485` + `runtime.js:247`【源码】；官方计玩家 CPU【官方】 | 结构化内存（bincode/列存 arena）+ 脏页增量持久化；或照抄 blob+overlay 惰性物化 | 社区普遍经验 Memory 为新手 CPU 大头（无官方分解）；xxscreeps 声称消除后 GC 负担"significant portion"级下降【作者声称】 |
| 2 | runner/processor 按房间流水线并行 | `main.js:30-33` 全局屏障【源码】；laverdet 点名【作者声称】 | 每房间 actor（tokio task），intents 齐即处理；runner 超时弃单保 tick | tick 时长从 `Σrun+Σprocess` 变 `max(per-room run+process)`；xxscreeps 已验证该拓扑可行 |
| 3 | 房间状态 blob + 零拷贝视图 | `data.js:131-231` 每玩家 19 查询【源码】；"每对象一文档" 30k 更新/s【官方】 | 房间级二进制状态 + mmap/共享内存分发；SoA 字段布局 | 2017 官方 DB 打满 100% 被迫分片【官方】→ 彻底绕开文档型 DB；blob 44B/墙、274B/creep【作者声称】 |
| 4 | 原生寻路为一等公民 | pf.cc C++ JPS【源码】；官方"drastically improves"【官方】；laverdet 2026 再重写【源码】 | Rust 内置 JPS+A*（对齐 CostMatrix/portal 语义），零 FFI | 社区：路径缓存后 2.0→0.4 CPU/creep【社区经验】→ 寻路占移动 CPU ~80% |
| 5 | find/look 空间索引 + 跨 tick 缓存 | `rooms.js:584-648` filter/clone 每 tick 重跑【源码】；isActive 猴补故事【社区经验】 | 每类型空间网格/位图，对象增删时失效；filter 结果不 clone | 消除玩家侧重复扫描（无官方数字；方向性收益） |
| 6 | 计费与隔离内建边界 | intentCpu=0.2、去重、freeMethods【源码】；2024 CPU stealing【官方】 | CPU 计量在 sandbox 边界（外部时钟），intent 计费表照搬保持经济兼容 | 杜绝 CPU stealing 类漏洞；兼容现有 bot 经济 |
| 7 | 行为一致性护栏先行 | screeps-ok 2897 用例 + 20 方法 adapter 契约【源码】 | Rust 引擎实现 ScreepsOkAdapter，CI 跑 parity | 把"行为等价"变成可回归的量化门禁（直接复用现成 34 领域用例） |

---

## 9. 来源清单

**官方（blog/forum/Steam/docs）：**
- https://blog.screeps.com/2016/02/Changelog-2016-02-19/ （PathFinder C++ 发布）
- https://blog.screeps.com/2016/04/Changelog-2016-04-13/ （PathFinder 默认启用）
- https://blog.screeps.com/2017/06/optimizations/ （优化路线图：DB 瓶颈、isolated-vm、分片计划）
- https://blog.screeps.com/2017/08/shards/ （分片上线）
- https://blog.screeps.com/2018/03/changelog-2018-03-05/ （isolated-vm 上线）
- https://screeps.com/forum/topic/2800 （2019 新集群：i9-9900K、+15-30% CPU、2.2s tick、K8s）
- https://screeps.com/forum/topic/3140 （2021 tick 降速事故）
- https://screeps.com/forum/topic/3285 （2024 CPU stealing/封号/泄码事件）
- https://screeps.com/forum/topic/3637 （2025-12 论坛归档）
- https://screeps.com/forum/category/5/news-announcements （公告索引，NodeBB API 分页抓取）
- Steam 新闻（app 464350）：2025-12-15 Roadmap、2026-03-17/04-01 Node 24、2026-04-23 shardX、2026-05-28 十周年、2026-08-25 Season 11（经 `ISteamNews/GetNewsForApp` API 抓取，正文见 Steam 社区公告）
- https://docs.screeps.com/architecture.html （160 核/40 机、MongoDB 24 核 128GB 30k 更新/s）
- https://docs.screeps.com/cpu-limit.html （CPU/bucket/tickLimit 语义）
- https://docs.screeps.com/global-objects.html （Memory 解析计费）

**laverdet / xxscreeps：**
- https://github.com/laverdet/xxscreeps （README、docs/architecture.md）
- https://github.com/laverdet/xxscreeps/pull/369 （availableParallelism、K8s 部署实测）
- https://github.com/laverdet/xxscreeps/pull/52 （2021 roadmap，历史）
- 本地 `sources/screeps/xxscreeps/packages/pathfinder/`（2026 新 C++20 寻路包）

**社区：**
- https://screeps.com/forum/topic/3005 （moveTo CPU 抖动与路径缓存 2.0→0.4）
- https://screeps.com/forum/topic/3179 （CPU tips：isActive 猴补等）
- https://screeps.com/forum/topic/3070 （shard0 慢 tick 公平性；2018 外环停用推文）
- https://screeps.com/forum/topic/3262 （shard3 tick 突增 3s→11s）
- https://twitter.com/ScreepsGame/status/994137796574089216 （shard0 整圈房间停用，2020 帖内引用）

**本地源码（file:line 速查）：**
- `sources/screeps/engine/src/game/game.js:484-486`（Memory JSON.parse）
- `sources/screeps/driver/lib/runtime/runtime.js:63-90,203-205,247`（intent 计费、getUsedCpu、Memory stringify）
- `sources/screeps/driver/lib/runtime/data.js:131-145,225-231`（每玩家 19 查询）
- `sources/screeps/engine/src/game/rooms.js:584-648`（Room.find 缓存/filter/clone）
- `sources/screeps/engine/src/main.js:16-121`（阶段事件、全局屏障、tick 日志）
- `sources/screeps/driver/lib/index.js:21`（mainLoopMinDuration=1000）
- `sources/screeps/engine/src/processor.js:431-434,531-534`（历史快照 JSON）
- `sources/screeps/driver/native/src/pf.cc:1`（C++ JPS 作者署名）
- `sources/screeps/xxscreeps/README.md:13-44`（blob/免序列化/overlay/并行四主张）
- `sources/screeps/xxscreeps/docs/architecture.md:74-166,210-241`（tick 流、双重验证、blob 细节）
- `sources/screeps/screeps-ok/README.md:6-104`、`docs/adapter-spec.md:26-69`、`docs/status.md`、`behaviors.md`

**未能完成/未找到的信息（如实记录）：**
- 生产服务器 tick 耗时的官方分解（各阶段毫秒数）从未被公开；本笔记只有私服阶段结构（§2.2）与零散 tick 长度数字。
- 2016 PathFinder 重写的具体加速倍数：官方原话仅"drastically improves performance"，无数值。
- laverdet 关于 xxscreeps 与 vanilla 的量化对比基准：README/issue/PR 中均未见公开数字（Discord #xxscreeps 有讨论但无法抓取）。
- `Room.find` 单调用官方成本：无官方数字，只有结构证据（§3.3）。
- Web 搜索工具本轮被限流（2026-09-29 恢复），全部网络证据改用直连 URL/API 抓取，未影响覆盖面。
