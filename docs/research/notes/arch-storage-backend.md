# 存储与后端协议层深度调研（增量）

> 调研日期：2026-09-27。调研员：存储与后端挖掘员。
> 本文是 `docs/research/01-screeps-rust-feasibility.md` 的**增量补充**，不重复其结论，聚焦：storage RPC 协议与队列真实实现（本次用 `npm pack @screeps/storage` 拉到闭源包解包实证）、backend socket diff 推送成本、map-stats 聚合、cronjobs 后台扫描（尤其 roomsForceUpdate）、history chunk 与 replay 缺位、launcher 每小时重启。
> 目的：为 Rust 版 xs-storage / xs-backend 的设计（事件溯源 vs 快照、二进制 WS 协议、增量订阅、休眠感知后台任务、单机嵌入式 KV）提供逐条优化依据。
>
> 源码根：`/Users/keqing/Desktop/projects/sources/screeps/`（下文相对路径均以此为准）。
> 闭源包实证：`npm pack @screeps/storage` → `screeps-storage-5.1.3.tgz` 解包于 `/tmp/screeps-storage-pkg/package/`（980 行 JS，全部可读——"闭源"只是不进 GitHub，npm 上完整可拉）。

---

## 1. 闭源 storage 进程的真实实现（本次实证）

### 1.1 RPC 协议：4 字节长度 + JSON 帧，无批量、无压缩、无二进制

`common/lib/rpc.js`：
- 帧格式 = `UInt32BE 长度 + UTF-8 JSON 字符串`（`rpc.js:26-53`）；每帧 `Buffer.alloc`、`JSON.parse`（`rpc.js:42`）、`JSON.stringify`（`rpc.js:49`）。`_parse` 递归 + `buffer.slice` 拷贝（`rpc.js:35,45`）。
- 请求/响应靠自增 id 匹配（`rpc.js:125-135`）；**没有批量接口**——N 个查询就是 N 帧 N 次 stringify/parse。
- pubsub 订阅帧由 storage 侧**每连接单独序列化一次**（`rpc.js:79-81`：每个订阅连接各 write 一个 makeFrame）。
- 健壮性：`RpcServer._processFrame` 无 try/catch（`rpc.js:68-96`），非法 method 名会同步抛异常进 Writable 流。
- `common/lib/storage.js:91-110`：每个集合包装出 `find/findOne/by/clear/count/ensureIndex/removeWhere/insert/update/bulk/findEx` 全走同一 RPC 通道；`storage.js:114-137` 队列与 env 各方法一一映射 RPC 方法名。集合清单硬编码 21 个（`storage.js:6-27`）。

**每 tick RPC 往返数（私服，U 个活跃用户、R 个活跃房间）**：
- main：`tickStarted` 发布 + `getAllUsers`（`driver/lib/index.js:144-151`，**传输完整 user 文档数组，仅用 `_id`**）+ `usersQueue.addMulti` + `whenAllDone` + `getAllRoomsNames` + `roomsQueue.addMulti` + `whenAllDone` + gameTime++ + 每 20 tick 全量 `updateAccessibleRoomsList`/`updateRoomStatusData`（`driver/lib/index.js:490-524`，扫全 rooms 集合）+ `roomsDone` 发布（`engine/src/main.js:26-92`）。
- runner：每用户 `queue.fetch` + `queue.markDone`（`engine/src/runner.js:68,74`）+ 约 20 个并行数据查询（driver/lib/runtime/data.js，见 01 报告）+ memory `env.set` + cpu pubsub。
- processor：每房间 **9 次往返**——`fetch` + 6 个并行查询（intents/objects/terrain/gameTime/roomInfo/flags，`engine/src/processor.js:555-566`，其中 terrain 是 2500 字符字符串**每 tick 全量重传**）+ `clearRoomIntents` + `markDone`（`processor.js:581-583`）；写侧 `mapViewSave`（每房每 tick `env.set`，`processor.js:493`、`driver/lib/index.js:530-532`）+ bulk + **history 每 tick 全量快照 hmset**（见 §4）。

粗算：100 用户 + 50 活跃房间的服务器每 tick ≈ 100×(2+20) + 50×(9+3) + main 侧 ~10 ≈ **2800+ 次 JSON-RPC 往返/tick**，全部走同一条 storage TCP 连接的 JSON 帧。这是 01 报告热点表 #1 的具体量化。

### 1.2 队列语义（`/tmp/screeps-storage-pkg/package/lib/queue.js`，闭源包实证）

三个队列 `usersLegacy/usersIvm/rooms` 纯内存数组 + EventEmitter（`queue.js:6-22`）。`'users'` 由 driver 映射为 `'usersIvm'`（`driver/lib/queue.js:20-22`）。

- **`queueFetch` 是长挂 RPC**：队列空时不回调，注册 `emitter.once('add', check)` 等 `queueAdd` 唤醒（`queue.js:27-35`）。每个等待的 runner/processor 线程持一个打开的 RPC 请求。
- **惊群**：`queueAdd` → `emit('add')` 唤醒**所有**等待者，第一个 pop 到任务，其余发现空、重新 `once('add')` 注册（`queue.js:29-33,54-64`）。私服规模小（RUNNER_THREADS×runner 数 + processor 数 ≈ 个位数），惊群可忽略，但语义上每次 add 是 O(等待者)。
- **LIFO**：`pending.pop()`（`queue.js:32`）——后进先出！main 按 activeRooms 顺序 `addMulti`（`engine/src/main.js:50`），单 processor 服务器实际按**反序**处理房间。Rust 复刻时若要行为对齐（screeps-ok 类测试可能敏感）需注意；多 processor 时本就无全序。
- **`queueWhenAllDone` 是 O(等待者×markDone) 轮询**：每次 `markDone` 都 `emit('done')` 唤醒所有 whenAllDone 等待者，逐个检查 `pending.length || processing.length`，未满足再注册（`queue.js:76-92`）。每 tick：U+R 次 emit × 唤醒 main 的 1 个等待者。达成后**发布 `queueDone:<name>` pubsub**（`queue.js:83`）——但 main 用的是 RPC 版 whenAllDone，这个 pubsub 频道实际是给谁听的？`driver/lib/queue.js:67-69` 提供 `createDoneListener`，engine 无人调用（grep 仅此一处定义）。冗余 pubsub。
- **`_.pull(processing, id)` 是 O(n) 数组扫描**（`queue.js:45`，lodash 3）。
- **零持久化**：队列在 storage 进程内存，storage 崩溃 = 进行中的 tick 丢状态（main 的 `mainLoopResetInterval=5000ms` 看门狗 `queue.resetAll()` 兜底，`engine/src/main.js:20-24`、`driver/lib/index.js:22`）。
- 尾巴：`setInterval(()=>{},500)` 空转保持事件循环（`queue.js:107-109`）。

### 1.3 db 层：lokijs + **单一 env 文档** 是全系统最大写热点

`/tmp/screeps-storage-pkg/package/lib/db.js`：

- 配置 `{autosave: true, autosaveInterval: 10000}`（`db.js:15`）——lokijs 每 10s 若脏则 `saveDatabase`。lokijs 实现（`xxscreeps/node_modules/lokijs/src/lokijs.js:2904-2920` autosaveEnable 定时器；`:2780` `saveDatabase(this.filename, this.serialize(), ...)`；`:1334` 默认 `serializationMethod` = **`JSON.stringify(this)` 整库序列化**；`:2437-2443` fs 适配器写临时文件 + rename）。
- **结论：私服 db.json = 每 10 秒全库 JSON.stringify + 全文件重写**。库内含 21 个集合：`rooms.objects`（全部房间的全部对象，不只活跃房）、`users.code`（所有分支玩家代码）、`users`（含徽章 SVG path 字符串，见 `db.js:192-194` 的巨型 badge 示例）……
- **env 是单个 lokijs 文档**：`dbEnvGet/Mget/Set/Hset/Hmset/Sadd/...` 全部读写 `db.getCollection('env').get(1)` 这一份文档（`db.js:539-563` 等），任何 env 写都把整个文档标脏（`env.update(values)`）。
- **env 文档里装了什么**（`common/lib/storage.js:33-52` 键表）：
  - `memory:<userId>` 每用户完整 Memory JSON 字符串（≤2MB/人）；
  - `roomHistory:<roomId>` 每**活跃**房间最近 ≤20 tick 的**全量快照 JSON 字符串**（§4）；
  - `roomVisual:<user>,<room>,<tick>` 每 tick 每房间每次绘制（`driver/lib/runtime/make.js:253-259`，TTL=5s 但清理周期 10s，`db.js:295-315`）；
  - `mapView:<roomId>` 每房每 tick 重写（`processor.js:493`）；
  - `accessibleRooms`（每 20 tick 全量房间名列表 JSON）、`roomStatusData`、`gameTime`（每 tick 写）、`activeRooms`（`activateRoom` = `env.sadd`，`backend-npm/lib/utils.js:311-313`，注意 lokijs 实现里 sadd 用 `_.includes` 去重 O(n)，`db.js:721-748`）。
- **量化写放大**：100 用户×100KB memory + 50 活跃房×30KB 快照×20 tick 历史 ≈ **40–50MB 的热数据塞在一个 env 文档里，每 10 秒整库 re-stringify + 全文件重写一次**（V8 JSON.stringify 吞吐约 100–300MB/s，即每 10s 花 150–500ms 单核 CPU + 写 50MB 临时文件）。且随用户/房间数线性恶化——与论坛 "私服 30 分钟 CPU 爬满"（01 报告 §3 forum topic/2940）现象吻合。
- env 过期清理：每 10s 全量遍历 expiration 表（`db.js:295-315`）。
- `dbResetAllData` 每次 `require('../db.original')` 解析 520KB 基线 JSON（`db.js:766-770`）；基线世界 = 121 房间（11×11）、340 对象、7 用户（本次解包 `db.original.json` 实测：collections 统计 env:1/users:7/rooms:121/rooms.objects:340/rooms.terrain:121）——这是私服默认世界的**真实规模锚点**。

### 1.4 pubsub：进程内 EventEmitter 转发

`/tmp/screeps-storage-pkg/package/lib/pubsub.js`：storage 进程内一个全局 EventEmitter，每连接一个 `create()`，`publish` → `emit(channel)` + `emit('*')`（`pubsub.js:9-13`）。发布的数据**本身已是 JSON 字符串**（如 cpu 事件 `JSON.stringify({...})`，`driver/lib/runtime/make.js:262-273`），backend 收到后再 `JSON.parse`（`backend-npm/lib/game/socket/user.js:36`）再 `JSON.stringify` 发给客户端（`socket/server.js:60-63`）——**同一数据三次序列化**（engine→storage 帧、storage→backend 帧、backend→客户端帧）。

### 1.5 为 Rust 版挖出的具体设计依据

| 官方痛点（证据） | Rust 做法 | 预期收益 |
|---|---|---|
| 每 tick ~3×10³ 次 JSON-RPC 往返（§1.1 量化） | 单进程内函数调用（trait `Storage`），跨机形态才走 bincode/postcard 帧 + 批量 `MultiGet` | 序列化 CPU 按 01 报告 rkyv 基准 ~14x；消除 4 层 stringify/parse（rpc+pubsub+socket） |
| 队列惊群 + O(n) pull + 零持久化（`queue.js:32,45`） | per-room 计数器（xxscreeps `ZINCRBY -1` 模式）或 `crossbeam` mpsc + 无锁环形缓冲；流水线状态本是内存态，无需持久化，但崩溃恢复要靠 tick 边界 checkpoint | whenAllDone 全局屏障直接消失（run/process 重叠，01 报告 §2.1.4 已证可行） |
| env 单文档写热点（§1.3） | 键空间分片：memory/history/visual/mapView 各归独立的 KV 表或带 TTL 的内存 slab；`redb` 多表或 `libmdbx` 多 dbi | 写放大从"整库 10s 全量序列化"降为"改哪个键写哪个键"；50MB/10s → KB 级增量 |
| LIFO 房间处理序（`queue.js:32`） | 固定确定性房间序（BTreeMap/索引序）服务 replay 与跨实例一致 | 免费获得确定性；对齐 screeps-ok 验证 |
| `smembers+del` 非原子丢唤醒（`driver/lib/index.js:170-174`） | 原子 drain（`swap` 取走旧值） | 消除罕见但真实的房间唤醒丢失竞态 |

---

## 2. backend socket diff 推送：每 200ms 全量读 + 每订阅者全量 diff

### 2.1 room 频道（`backend-npm/lib/game/socket/rooms.js`）

- 触发：**每次 `roomsDone` pubsub**（= 每 tick）进入 `_.throttle(..., 200)`（`rooms.js:22,139`；`socketUpdateThrottle: 200` 定义于 `socket/server.js:24`）。
- 成本 1——**无脏检查的全量读**：对每个有订阅者的房间执行 `db['rooms.objects'].find({room})` + `getGametime` + `db['rooms.flags'].find({room})`（`rooms.js:54-58`）——房间**没变也照读**，全量对象数组经 JSON-RPC 从 storage 搬到 backend。terrain/flags 同理。
- 成本 2——**每订阅者各算一遍 diff**：`common.getDiff(i.objects, roomObjects)`（`rooms.js:75`）。`common/index.js:69-136` 的 getDiff 是 O(对象数×字段数) 的 `_.isEqual` 深比较（第二层以下子对象整体替换，`index.js:98-99`——不是真深 diff）。同房间 3 个订阅者 = 同一份 diff 算 3 遍（flags/visuals 才因用户而异）。
- 成本 3——**visuals 每 tick 每订阅者 2 个 env.mget**（`rooms.js:81-92`），即使没画任何东西也要往返；命中时传输完整 visual JSON 字符串。
- 每连接持有 `i.objects = roomObjects` 上次全量快照（`rooms.js:95`）——backend 内存里每订阅一份房间世界。
- 订阅限制 `USER_LIMIT = 2`（每用户同时订 2 个 room 频道，`rooms.js:9,35-45`）；初始订阅 = `getDiff([], roomObjects)` 全量（`rooms.js:204`）。
- 房间处理顺序 `_.shuffle` 随机（`rooms.js:26`）。
- **关键洞察：processor 明知本 tick 改了什么**（bulk update/insert/remove 就是变更集，`driver/lib/bulk.js`），但存储 API 没有 change feed，backend 只能"全量重读 + 反推 diff"。**官方架构把引擎已知的信息扔掉再花 CPU 买回来。**

### 2.2 user memory / resources / mapVisual 频道（`socket/user.js`）

- 每 roomsDone（throttle 200ms）：对每个有 memory 订阅的用户 `env.get(memory:<id>)` **整条 memory 字符串过 RPC**（≤2MB），backend `JSON.parse` 整个 memory，再按订阅路径逐个走指针（最多 50 条路径，超出**静默丢弃**，`user.js:52-84`）。看 1 个 `memory.creeps.xxx.hit` 路径 = 每 200ms 全量 parse 一次 2MB JSON。
- resources：每 200ms 对所有订阅用户 `db.users.find({_id:{$in}})` 拉全量 user 文档取 `resources/money` 两字段（`user.js:86-95`）。
- cpu 频道：engine 每用户每 tick publish JSON 字符串 → backend throttle 200ms 后 `JSON.parse` 再 `JSON.stringify`（`user.js:35-37` + `make.js:262-273`，三重序列化见 §1.4）。

### 2.3 roomMap2 频道（`socket/map.js`）

- 每 roomsDone（throttle 200ms）：`env.mget([mapView:<room>...])` 把**所有**订阅房间的 mapView JSON 全量拉回（`map.js:15-44`），不管变没变，然后每房间拼字符串 `["roomMap2:<room>",<json>]` 广播。没有版本号/ETag/增量。
- mapView 本身是 processor 每 tick 每房间 `env.set` 重写的（§1.3）——即使房间静止（copy-forward 场景不存在，官方每 tick 重写同值）。

### 2.4 传输编码（`socket/server.js`）

- sockjs **文本帧**：`JSON.stringify([channel, data])`（`server.js:60-63`）；握手是裸文本行 `time <ts>` / `protocol 14` / `auth <token>` / `subscribe <ch>` / `gzip on`（`server.js:92-145`）。
- 可选 gzip：**每条消息单独 `zlib.deflate(level 1)` → `'gz:'+base64'`**（`server.js:45-58`）——base64 膨胀 33%，抵消部分压缩收益；无共享字典、无跨 tick 合帧（同 tick 多房间 = 多帧多次 deflate）。
- 订阅扇出：backend 内部 EventEmitter per channel per connection（`server.js:77-88`），每连接每消息一次 stringify。

### 2.5 为 Rust 版挖出的依据

1. **增量订阅推送（最大项）**：processor 结算时天然产出变更集（intents 应用后的 bulk ops）。Rust 设计 `RoomChangeSet`（per-room postcard 编码的字段级 delta），引擎发布一次，backend 按订阅者可见性（flags/visuals/视野）过滤后扇出——**消灭"每 200ms 全量重读 + 每订阅者重算 diff"两笔成本**。量级：50 订阅房间 × 30KB 对象 × 5 次/s = 7.5MB/s RPC 传输 + 5×50 次/s getDiff 深比较 → 降为 0（只在真变更时发 KB 级 delta）。memory 频道同理：runner 写 memory 时已知变更路径/字节区间，按订阅路径做指针定位服务端提取，免去全量 parse。
2. **二进制 WS 协议**：兼容层保留官方 sockjs 文本协议（renderer 生态依赖，01 报告 §4.4），新增原生二进制通道（permessage-deflate 或自定义 frame：`[channel-id u16][tick u32][rkyv delta]`），channel-id 用握手期字典分配。对比现状：省掉每消息 deflate→base64 膨胀与每连接重复 stringify；同 tick 变更合帧批量发。
3. **订阅状态不复制**：官方每连接缓存整房间快照（`rooms.js:95`）；Rust 侧 diff 在引擎侧算好后按引用扇出，backend 无需 per-conn 世界缓存（内存从 O(订阅×房间大小) → O(订阅×delta)）。

---

## 3. cronjobs：与休眠无关的全量扫描后台任务

调度：`setInterval(cronjobs.run, 1000)` 每秒检查（`backend-npm/lib/index.js:20`），11 个任务（`backend-npm/lib/cronjobs.js:14-26`）。

### 3.1 roomsForceUpdate（每 20s，`cronjobs.js:178-192`）——"反休眠"机制

- 逻辑：`getGametime` + `getActiveRooms`（env.smembers）+ **`db.rooms.find({status:{$ne:'out of borders'}, _id:{$nin: activeRooms}})` 全量 rooms 文档经 RPC 搬到 backend**（`cronjobs.js:181`，lokijs 对 `$nin` 无索引，全集合扫描），逐房判断 `gameTime >= room.nextForceUpdateTime` 则 `activateRoom`（env.sadd）并把下唤醒点设为 `gameTime + 90 + rand(20)`（`cronjobs.js:183-189`）。
- **语义：世界上每个房间每 90–110 游戏刻被强制唤醒处理一次，与有没有玩家完全无关。**官方架构没有真正的休眠房间——"空闲"房间的 sources 重生、tombstone 衰减等都要靠跑完整 room tick 来推进。
- 量化（默认 121 房世界、tick 1s 下限 `driver/lib/index.js:21`）：每房间每 ~100s 一次全处理 → 持续 ~1.2 房间 tick/s 的底线负载，每次 = §1.1 的 9 次 RPC + 全量对象读写 + **history 全量快照 hmset**（§4）+ mapView 重写。空服也永不闲置。10k 房间世界（官方 shard 规模）≈ 100 房间 tick/s 纯后台维持成本。
- 每 20s 一次的全量 rooms 文档传输（121 房 ≈ 每房几百字节元数据；大世界数 MB）也是周期性 storage 抖动源。

### 3.2 其他扫描型任务

- **genInvaders（每 5min，`cronjobs.js:377-440`）**：对**每个非活跃 normal 房间串行 await** `db['rooms.objects'].find({room, type:{$in:['source','controller','creep']}})`（`cronjobs.js:380`）——121 房 = 121 次串行 RPC 往返（每次含 storage JSON 序列化 + 传输）；大世界数千次。且有 `$regex` 部门查询（`cronjobs.js:394`）lokijs 全扫。
- **genPowerBanks（每 5min，`cronjobs.js:194-243`）**：qSequence 串行每 terminal 一次 `db.rooms.findOne`（`cronjobs.js:62`）。
- **purgeTransactions（每 1h，`cronjobs.js:443-470`）**：`db.transactions.find()` **全表载入**内存 JS 排序裁剪。
- **calcMarketStats（每 1h，`cronjobs.js:472-523`）**：载入 14 天全部 `users.money` 市场记录聚合。
- **recreateNpcOrders（每 5min，`cronjobs.js:38-122`）**：全量 terminal 扫描 + 每房 3 次串行写。

### 3.3 为 Rust 版挖出的依据：休眠感知的后台任务

- 把"源重生/衰减计时"改为**离线结算**：房间状态里存时间戳，唤醒时按 `Δt` 一次性推进（xxscreeps 休眠房间 copy-forward + 少量 decay 计算的思路），`roomsForceUpdate` 整个消失或退化为低频巡逻。收益量化：默认世界省 ~1.2 房间 tick/s；10k 房省 ~100 房间 tick/s（每次含 9 RPC + 全量对象读写 + 快照写）。
- 背景扫描全部改为**索引下推 + 批量**：invader/powerBank 生成按 sector 分区键索引查询（`WHERE next_event_time <= now LIMIT n` 风格），一次批量取到期房间，而非每房一往返。121→1 次查询；串行 await → 并行。
- 事务清理/市场统计：增量游标（按 time 字段索引范围扫描）替代全表载入。
- cron 与 tick 解耦：官方 cron 挂在 backend 进程、每秒 setInterval（`index.js:20`），与引擎时钟（gameTime）不同步（任务内部才去取 gameTime）；Rust 版统一到 gameTime 驱动的调度器，天然支持暂停/倍速。

---

## 4. history chunk 与 replay：双倍成本 + 默认丢弃 + API 未实现

### 4.1 写侧：每活跃房每 tick 一次全量 JSON 快照进 env 文档

- `engine/src/processor.js:499-507`：房间保持活跃时调用 `saveRoomHistory(roomId, objectsToHistory, gameTime)`；`processor.js:431-439`：creep/powerCreep 先 `JSON.parse(JSON.stringify(object))` 深拷贝清洗（01 报告热点 #3）；`processor.js:532`：`JSON.stringify(objects)` **整房快照**；`processor.js:533` → `driver/lib/history.js:7-9` `env.hmset(roomHistory:<room>, {<tick>: <全量快照字符串>})`——写进 §1.3 那个单文档 env。
- 快照经 `currentHistoryPromise` 串行链**逐 tick 顺序排队**（`processor.js:524-534`），同一房间的写不并行但跨 tick 链式 then 累积。

### 4.2 chunk 上传：每 20 tick 双倍计算后默认扔掉

`driver/lib/history.js:11-43`（historyChunkSize=20，`driver/lib/index.js:29`、`backend-npm/lib/game/server.js:39`）：
- 每 20 tick `upload`：`env.get` 取回 **20 份全量快照**（又经 RPC 全量传输），逐份 `JSON.parse`（`history.js:20,32`），对相邻两份跑 `common.getDiff` 深比较得到 19 个 delta（`history.js:33-34`），拼 `{base, ticks:{t0:全量, t1..t19:delta}}`，然后 `config.engine.emit('saveRoomHistory', ...)`（`history.js:39`）。
- **默认没有任何监听者**：全仓库 grep `saveRoomHistory` 仅命中 driver 发射点 + launcher 的**示例 mod**（`launcher/init_dist/example-mods/room-history.js:13-31`，把 chunk 写成无上限的本地 JSON 文件，无保留策略）。即默认私服：每活跃房每 tick 付全量快照写 + 每 20 tick 付 20 次 parse + 19 次 diff，**结果全部丢弃**。
- 读侧：`/room-history` HTTP 端点默认 `callback('not implemented')`（`backend-npm/lib/game/server.js:34-36,178-187`）——**官方 standalone 没有可用的 replay 查询 API**；示例 mod 的文件版实现也只是裸 JSON 串。

### 4.3 为 Rust 版挖出的依据：事件溯源 vs 快照的取舍

- 官方同时付了两种范式的成本（全量快照存储 + 事后 diff 计算）却两者都没用好。Rust 建议分层：
  1. **权威变更流**：processor 的 bulk ops 本身就是事件源（insert/update/remove + 字段级旧值新值），追加写入 per-room 分段日志（postcard 编码），天然 O(变更) 而非 O(房间)；
  2. **周期基线**：每 N tick（可配，默认 20 对齐官方 chunk 语义）落一份 rkyv blob 全量（xxscreeps 房间 blob 250B–60KB，01 报告 §2.1.1），replay = 基线 + 重放 delta；
  3. **对外 chunk API 兼容**：`/room-history?room=&time=` 返回官方 `{base, ticks}` JSON 结构，从事件流现算，客户端（renderer 时间轴）无感。
- 收益量化：写侧从"每 tick 全量 stringify（30KB×50 房×1/s ≈ 1.5MB/s）"降为"每 tick 变更集追加（通常 <1KB/房）"，约 30–100x；每 20 tick 的 20 次 parse + 19 次 diff 全部消失（这些工作在默认私服里本来就是白做的）。
- 保留策略官方缺位（示例 mod 无上限写盘）：Rust 版按 `(room, tick)` 段文件 + 大小/时长双阈值 GC，热数据 mmap 冷数据归档。

---

## 5. launcher 每小时重启：官方承认的内存问题

- `launcher/lib/start.js:26` 默认 `restart_interval: 3600`；仅 runner 与 processor 传了该参数（`start.js:206-224`，backend/engine_main/storage 不重启）；实现 = `setTimeout(() => process.kill(), 3600s)` + exit 后 1ms 重启（`start.js:139-149`）。
- 每 runner/processor 每小时硬重启 = **官方对长跑内存泄漏（isolated-vm 沙箱残留、q promise 链、lokijs/env 文档膨胀、bulk cloneDeep 等）的默认缓解**。代价：重启窗口内该进程的并发槽位消失（tick 变慢一拍）+ isolate 冷启动（V8 snapshot 重新反序列化 + 玩家代码重新编译）。
- storage/backend 不重启：env 文档与 lokijs 缓存只会涨——这与"私服内存泄漏"论坛报告（topic/2940）一致。
- Rust 版：无需进程级重启——isolate 池按 LRU + 空闲回收（官方 user-vm 3min 空闲回收语义保留），世界状态在受控内存 slab 中有上界，跨 tick 无 GC 堆累积。每用户持久 isolate 的内存收益已在 01 报告 §5.4 评估；本条新增依据是：**官方把"每小时重启"写进默认配置这一事实本身，就是架构级内存问题的自认**。

---

## 6. 其他增量发现（01 报告未覆盖）

1. **`getAllUsers` 每 tick 全量用户文档传输**（`driver/lib/index.js:144-151`），main 只用 `_id` 排序——Rust：进程内只需迭代 userId 列表。
2. **`getAllRoomsNames` 的 smembers+del 非原子**（`driver/lib/index.js:170-174`）：两 RPC 之间 processor 的 `activateRoom` 会丢唤醒。Rust：原子 drain。
3. **cpu/console/code 等事件三重序列化**（§1.4）：pubsub 载荷本身是 JSON 字符串。Rust：事件结构体进程内传递，仅在出口序列化一次。
4. **`getDiff` 只做两层深**（`common/index.js:93-108`）：第三层起子对象整体替换。官方客户端能容忍这种 delta 粒度 → Rust 的字段级 delta 可以做得更细但不必更粗，兼容无虞。
5. **storage 侧 `queueFetch`/`whenAllDone` 都是挂起 RPC**：storage 进程重启时所有挂起请求的连接收到 error → `storage.js:150-161` 自动重连（1s 退避）但**挂起中的 fetch 请求结果丢失**，runner 靠 engine main 的 reset 兜底。Rust：进程内无此问题；跨机形态需请求级幂等重试。
6. **map-stats 无缓存**（`backend-npm/lib/game/api/game.js:186-257`）：每次请求 2 次 `rooms.objects` `$in` 扫描（controller/invaderCore + mineral）+ users 查询，全量文档传输后才 `_.pick` 两三个字段。客户端开一次世界地图 = 全量可见房间的对象级扫描。Rust：维护 per-room 投影缓存（controller owner/level/sign、mineral type/density 随 tick 更新），map-stats 变成 O(请求房间数) 的字典取值。
7. **`dbBulk` 无事务**（`/tmp/.../db.js:480-513`）：逐 op 顺序执行，中途异常已写部分不回滚（catch 只回传错误）。Rust：redb/libmdbx 原生事务，per-room tick 写入原子提交，崩溃恢复到 tick 边界。
8. **`env.sadd`/`hmget` 等 Redis 风格 API 是存储抽象的表面**：lokijs 实现下这些全是同一文档的字段操作。xxscreeps 把这套面提炼成 `KeyValProvider/PubSubProvider` trait（01 报告 §2.1.8）——Rust 版 xs-storage 沿用该 trait 面，进程内实现零成本直通，跨机换 Redis/FDB 实现。
9. **默认协议握手细节**（`socket/server.js:92-93`）：连接建立即发 `time <ts>` 与 `protocol 14`，auth 前仅允许非 `user:` 频道订阅（`server.js:98-100`）——兼容层需复刻这两行的精确时序。

---

## 7. 优化点汇总（按价值降序，供结果提交）

1. **增量订阅推送：用 processor 已知的变更集替代 backend 每 200ms 全量读+diff**（§2.1/2.5.1）——最大单项，消灭常驻 O(订阅房间×全量对象) 的重复劳动与传输。
2. **存储层：消灭 env 单文档 + lokijs 10s 整库序列化，改 per-room blob + 嵌入式 KV（redb/libmdbx）增量写**（§1.3/1.5）——写放大从数十 MB/10s 降到 KB 级/变更。
3. **事件溯源式 history：变更流追加 + 周期基线，替换每 tick 全量快照 + 事后 diff + 默认丢弃**（§4）——官方在此白烧 CPU；同时补上 replay API 官方缺位。
4. **休眠感知后台任务：离线结算替代 roomsForceUpdate 每 ~100 tick 强制唤醒 + 全量扫描 cron**（§3）——空闲世界负载归零，大世界省 ~100 房间 tick/s。
5. **二进制/合并帧 WS 协议 + 兼容文本层**（§2.4/2.5.2）——省 per-message deflate+base64 膨胀与 per-conn stringify。
6. **队列语义重设计：per-room 计数器触发 + 原子 drain + 确定性房间序**（§1.2/1.5）——支撑 run/process 重叠与确定性 replay。
7. **map-stats 投影缓存**（§6.6）——世界地图请求从对象级扫描降为字典取值。

## 8. 证据文件索引（本次新增阅读）

- 闭源包（npm pack @screeps/storage@5.1.3 解包于 /tmp/screeps-storage-pkg/package/）：`lib/queue.js`、`lib/db.js`、`lib/pubsub.js`、`lib/index.js`、`db.original.json`
- `common/lib/rpc.js`、`common/lib/storage.js`、`common/index.js`（getDiff）
- `engine/src/main.js`、`engine/src/runner.js`、`engine/src/processor.js`
- `driver/lib/index.js`、`driver/lib/queue.js`、`driver/lib/history.js`、`driver/lib/runtime/make.js`
- `backend-npm/lib/index.js`、`cronjobs.js`、`utils.js`、`game/server.js`、`game/socket/{server,rooms,user,map}.js`、`game/api/game.js`
- `launcher/lib/start.js`、`launcher/init_dist/example-mods/room-history.js`
- lokijs 行为证据：`xxscreeps/node_modules/lokijs/src/lokijs.js`（autosaveEnable/serialize/saveDatabase）
