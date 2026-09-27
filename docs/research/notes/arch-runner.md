# 玩家侧运行时开销挖掘笔记（runner / engine/src/game）

> 调研日期：2026-09-27。范围：`sources/screeps/engine/src/game/` 全部 + `sources/screeps/driver/lib/runtime/` 全部，对照 xxscreeps 的 runner/sandbox/memory/market 实现。
> 目标：找出「每用户每 tick 的固定开销」中还能消灭的部分，并给出 Rust 重写中的对应做法与量化依据。
> 本文是 `01-screeps-rust-feasibility.md` 的增量，不重复其结论；与 `notes/arch-processor.md`（processor 侧）互补。

---

## 0. 本轮实测数据（自己跑的基准，本机 Node，M 系列 macOS）

| 项目 | 实测值 | 命令 |
|---|---|---|
| `common/lib/constants.js` JSON 序列化体积 | **23,332 字节 / 423 个顶层键** | `node -e "require('./lib/constants.js')"`（在 `sources/screeps/common` 下） |
| constants `JSON.parse(JSON.stringify(C))` 单次 | **≈1.65 ms** | 同上，hrtime 2000 次均值 |
| 500 单市场订单簿（每单 172B JSON）深拷贝 | 88,281 字节 JSON，round-trip **≈4.0 ms** | node hrtime 500 次均值 |
| 106KB Screeps 风格 Memory round-trip（parse+stringify） | **≈9.2 ms** | node hrtime 200 次均值 |
| 单房间 15 个 spatial 寄存器 ×2500 槽位纯分配 | **≈146 µs/房间** | node hrtime 200 次均值 |
| 单条订单 JSON | 172 字节 | 同上 |

换算锚点：官方 tick 3.6s 目标 / 私服 2C2G。下文所有"×100 用户"都以此为量级。

## 0.1 一个此前报告没写的关键事实：这些固定开销不计入玩家 CPU，也不受超时约束

`driver/lib/runtime/runtime.js` 中：

- `_start` 开头记 `startDirtyTime`（runtime.js:59）；
- `game.init(...)`（= makeGameObject：constants 深拷贝、register/spatial/findCache 构建、全部对象 wrapper 实例化）在 runtime.js:216 执行；
- 真正的玩家 `loop` 在返回的闭包里，`startTime = nowCpuTime()` 在 runtime.js:235 —— **在 game.init 之后**；
- `usedTime/usedCleanTime` 按 startTime 差值计算（runtime.js:271-273）。

即：**每 tick 的世界物化成本发生在玩家 CPU 计量窗口之外**（只受 make.js:301-307 的 5s watchdog 约束，`run.apply(..., {timeout})` 只包玩家 loop，make.js:187）。它纯粹是服务器成本，随世界规模和用户数线性增长，却在任何 CPU 统计里都看不见——这与官方 runner 每小时重启"回收内存"（`launcher/lib/start.js:139-141`）的症状互为印证。Rust 重写消灭的每一毫秒都是服务器净收益，不会改变玩家体感公平性（除了 Memory round-trip，见 §3.5）。

---

## 1. 现状全景：一次 `runUser(userId)` 的固定动作清单

按执行顺序（每用户每 tick）：

1. **20 次 DB/env RPC 拼装 runtimeData**（`driver/lib/runtime/data.js:99-264`）；
2. **整包 structured clone 进 isolate**（`driver/lib/runtime/make.js:144-160`：`new ivm.ExternalCopy(data)` → `copyInto()`）；
3. **沙箱内物化**（`engine/src/game/game.js:50-438` makeGameObject）：register/byRoom/spatial(15×2500/房间)/findCache、全部对象 JS wrapper（game.js:260-395）、flags 打包串解析（game.js:397-429）、map 解析（map.js:18）、market 对象（game.js:433）、**constants JSON round-trip**（game.js:435）、Game.resources round-trip（game.js:170）；
4. 玩家 loop（计费窗口）；
5. **Memory stringify + 整包 outMessage ExternalCopy 出沙箱**（runtime.js:246-248, 276, 296）；
6. **结果写回**：`db.users.update`（make.js:214）、pubsub CPU 上报（make.js:263-273）、`saveUserMemory/Segments/Intents`（`engine/src/runner.js:32-44` → `driver/lib/index.js:153-243`，intents 按房间 upsert-merge + `ACTIVE_ROOMS` sadd + `users.intents` insert）。

下面逐项挖"还能消灭什么"。

---

## 2. data.js 的一次性 20 查询：增量/合并的具体改法

### 2.1 现状清单（data.js:99-264）

第一批 14 项并行查询（data.js:129-144）：

| 查询 | 行号 | 每用户每 tick 是否必要 |
|---|---|---|
| `users.findOne({_id})` | data.js:130 | **重复**：make.js:54-96 `getUserData` 已 findOne 过同一 user（两个完整 user 文档各取一次） |
| `users.code.findOne(activeWorld)` | data.js:131 | 仅代码变更时需要；可用时间戳+pubsub 失效缓存 |
| `env.get(MEMORY+userId)` | data.js:132 | 需要（但可 SAB 共享，见 §3.4） |
| `users.console.find` | data.js:133 | 轮询；可 pubsub 推送 |
| `common.getGametime()` | data.js:134 | `env.get(GAMETIME)` RPC（common/index.js:65-67）；**tick 号本可随任务下发**（官方 users 队列只传 userId 字符串，runner.js:68-71） |
| `rooms.find({$in: roomIds})` | data.js:135 | 房间元数据几乎不变，可缓存 |
| `rooms.objects.find({user:userId})` | data.js:103 | 对**全世界最大集合**按 user 扫描；应被 processor 维护的 per-user 索引替代 |
| `rooms.objects.find({room $in, user $ne})` | data.js:136 | 同上，可见房间内全部对象；与 103 可合并为一次 `room $in` 查询后客户端拆分 |
| `getAccessibleRooms()` | data.js:137 | 已有 60s 进程级缓存（data.js:77-86），OK |
| `transactions.findEx ×2`（sort+limit 100） | data.js:138-139 | **每用户每 tick 拉 200 条交易**，绝大多数 bot 从不读（market.js:203-233 是惰性 getter，取数却急切） |
| `rooms.flags.find` | data.js:140 | 小，可随 per-user 记录走 |
| `env.hmget(ROOM_EVENT_LOG, roomIds)` | data.js:141 | **每房间每用户拉事件日志**；getEventLog 是惰性 parse（rooms.js:573-582）但取数急切 |
| `users.power_creeps.find` | data.js:142 | 小 |
| `getRoomStatusData()` | data.js:143 | 已有 60s 缓存（data.js:88-97），OK |

第二批 6 项（data.js:223-239）：`users.find({$in:userIds})`（224）、`getCachedMarketOrders`（225，进程内按 gameTime 缓存，data.js:31-49）、`getCachedMarketHistory`（226，3h 缓存，data.js:51-75）、`market.orders.find({user})`（227）、segments hmget（228-230）、foreign segment（231-238）。

另有每 tick 无条件发出的写命令：`users.console.removeWhere`（data.js:149）——即使命令队列为空（`$in: []`）也发一次 RPC。

### 2.2 Rust 对应做法与依据

xxscreeps 已给出可照抄的形态（`xxscreeps/packages/xxscreeps/engine/runner/instance.ts:200-248`）：

- **每 tick 只读可见房间的 room blob**（`Promise.all(Fn.map(visibleRooms, roomName => this.shard.loadRoomBlob(...)))`，instance.ts:222-226）——`rooms.objects` 两查合一为"每可见房间一次 blob 读"，blob 内含全部对象，天然带 per-room 空间布局；
- **用户名增量解析**：`seenUsers` 集合跨 tick 缓存，只为新出现用户查一次（instance.ts:227-246）。官方 data.js:224 的 `users.find({$in})` 每用户每 tick 全量拉。

Rust 版（xs-world/xs-runner）具体化：

1. **per-user 可见集索引**：processor 提交房间 blob 时增量维护 `userId → {可见房间, 拥有对象 id 集}`（写时更新，O(变更对象)）。data.js:103/136 的两次大集合查询彻底消失，变为一次索引读 + N 次 blob 读。依据：可见性只随对象所有权/observer/防守关系变化，每 tick 变更量 ≪ 总量。
2. **tick 号随任务下发**：engine_main 把 `gameTime` 放进 users 队列任务体（官方只传 userId，runner.js:68-71），消灭每用户一次 `env.get(GAMETIME)`（data.js:134）。
3. **user 文档只取一次**：合并 make.js:55 与 data.js:130（同一文档取两次）。
4. **transactions / roomEventLog / market 惰性化**：沙箱内 getter 首次访问时经同步宿主调用取数（Rust 宿主+沙箱同进程，无 RPC 惩罚）；或按"新交易计数器"/pubsub 触发预取。
5. **users.code 缓存**：按 `(userId, codeTimestamp)` 缓存模块映射，代码提交事件失效。runCodeCache 已经在沙箱内做了类似事（game.js:442-468 每用户 globals 复用），宿主侧却没做。

### 2.3 量化

- 消灭的 RPC 次数：每用户每 tick 约 20 → 约 1（per-user 记录）+ 可见房间数次内存内 blob 读。官方每帧 RPC 是"4 字节长度前缀 + JSON 帧"（common/lib/rpc.js:26-53），unix socket 往返 + 双向 stringify/parse；按 0.1–0.3ms/次估，**每用户每 tick 省 2–6ms 纯管道成本**，100 用户即 0.2–0.6s/tick。
- `rooms.objects` 的全集合扫描是 MMO 侧 DB 单点饱和（报告 §3 已引 2017 blog）的主构成之一，索引化后该类查询消失。

---

## 3. 整包深拷贝进沙箱：官方最大的单项浪费

### 3.1 现状证据

- make.js:144-160：`data = await runtimeData.get(userId); dataRef = new ivm.ExternalCopy(data); vm.start.apply(undefined, [dataRef.copyInto()])` —— **runtimeData 整包 structured clone 进 isolate，每用户每 tick**。
- runtimeData 里装了什么：`roomObjects`（可见房间全部对象原始文档）、`rooms`、`flags`、`users`（可见房间里出现过的所有玩家文档）、**`market.orders`（全世界全部活跃订单，按 resourceType 分桶）+ `market.history`（全部 market.stats，约 14 天 × 全资源）+ `myOrders`**（data.js:243-247）、**transactions 2×100 条**（data.js:213-216）、`roomEventLog`（每房间事件日志字符串，data.js:219）、accessibleRooms、roomStatusData、userMemory（≤2MB 字符串，data.js:205）。
- 也就是说：**一个从不碰市场、不看交易的玩家，每 tick 也在为整本市场订单簿 + 全部历史 + 200 条交易做深拷贝**。玩家访问侧再拷一次（§5）。

### 3.2 消除方案（Rust 宿主 + JS 沙箱）

分层：

1. **房间状态**：per-room blob（rkyv/xxscreeps wire format）双缓冲，processor 写 T 版、runner 读 T-1 版，以 SharedArrayBuffer 直传沙箱（官方地形已经这么做了：make.js:20-51 staticTerrainData SAB + runtime.js:45-49 按房间建 Uint8Array 视图——**官方自己已证明该模式可行，只是只用于地形**）。
2. **市场**：增量连接器。xxscreeps wallstreet 的做法可直接照抄：runner 侧常驻 `OrderBlobWatcher`（`mods/mmo/wallstreet/driver.ts:12-74`，订阅 marketChannel，amount 变更走 pubsub 增量 + 本地改 DataView），每 tick 只给 payload 装 `changedThisTick ∪ 未见过` 的订单 blob（driver.ts:207-226：`Fn.filter($$, id => blobLoader.changedThisTick.has(id) || !loadedBlobs.has(id))`）；沙箱侧 `Orders` 类跨 tick 复用旧 blob（`mods/mmo/wallstreet/order.ts:54-78`），订单对象按需 `readOrder(blob)` 物化并 `@cached`；credits 由 `PlayerCreditsWatcher` 以 pubsub 增量维护（driver.ts:82-99）。
3. **用户名/交易/事件日志**：同模式——跨 tick 缓存 + 变更推送（xxscreeps 用户名增量：instance.ts:227-246）。
4. **Memory**：见 §3.4。

### 3.3 量化

- 500 单订单簿 round-trip 实测 ≈4.0ms（§0），且这是"访问侧再拷"的成本；进沙箱的 structured clone 同量级或更高（ExternalCopy 走 V8 序列化器）。加上 history/transactions/roomEventLog，**每用户每 tick 的整包拷贝合理估计 5–10ms**（世界越大越高，与玩家是否使用无关）。
- 100 用户私服即 0.5–1s/tick —— 与官方 tick 2–5s 的实测同量级，是 tick 时长的直接构成。
- 增量化后：常驻用户每 tick 市场流量 ≈ 仅变更订单（通常 <10 单/世界/tick），趋近 0。

### 3.4 Memory round-trip：xxscreeps Memhack 语义之外还能做什么

现状三重浪费（每用户每 tick，≤2MB 字符串）：

1. 存储→宿主：`env.get(MEMORY+userId)`（data.js:132）；
2. 宿主→沙箱：随整包 ExternalCopy 进来（make.js:145,160）；
3. tick 内 `JSON.parse` 全量（game.js:479-500，getter 首触即整体解析）→ tick 末 `JSON.stringify` 全量（runtime.js:246-248）→ 随 outMessage ExternalCopy 出沙箱（runtime.js:276,296）→ `env.set` 写回（driver/lib/index.js:153-160）。

实测 106KB round-trip ≈9.2ms；线性外推 2MB ≈ 180ms/tick（上限用户）。与知名玩家 Dissi "Memory 占玩家 CPU ~4%"（forum topic/85）一致——**注意：parse/stringify 发生在玩家 loop 窗口内，是计入玩家 CPU 的**，与其他固定开销不同。

xxscreeps memory.ts（`mods/meta/memory/memory.ts`）已实现：整体惰性 parse（getIsDefault，memory.ts:219-234）、跨 tick `previousJson` 复用 = Memhack 跳过（memory.ts:284-289）、UTF-16 SharedArrayBuffer 传输零拷贝（memory.ts:315-326, 302-308）、Memhack 篡改钩子（memory.ts:329-334）、`crunch()` 模拟 JSON round-trip 的语义副作用（memory.ts:39-66）。

**增量空间（Memhack 之外）**：

1. **每键惰性 parse**：memory.ts 首触 `Memory` 仍整体 parse。改为按顶层键（`creeps/rooms/spawns/flags/powerCreeps` 就 5 个左右）惰性解析——bot 只读 `Memory.rooms` 时不解析 `Memory.creeps`（往往是大头）。实现：宿主把 memory 字节按顶层键切偏移表（一次扫描产出各键的字节区间，~O(总长) 的 memchr 级扫描，比 JSON.parse 快一个量级且零对象分配），沙箱侧 getter 首触该键才 parse 对应区间。
2. **写屏障脏键序列化**：xxscreeps 只要 `_parsed` 被设置过，flush 就整体 `crunch + JSON.stringify`（memory.ts:268-283）——**只读 tick 也要付全量 stringify**。改为：对被 parse 出的顶层键安装一次性写探针（首写时把该键标记 dirty 并换成普通对象），flush 时只重序列化 dirty 键、未 dirty 键复用上一 tick 的字符串切片，拼接成最终缓冲。读多写少的 bot（常见模式：每 tick 只更新几个计数器）从 O(全量) 降到 O(dirty 键)。
3. **传输零拷贝 + 写回零拷贝**：memory 字节常驻宿主侧 per-user arena（SAB 映射），进出沙箱都不拷贝（xxscreeps 已做到，Rust 版保留即可），写回存储改为脏页追加/checkpoint 而非每 tick 全量 `env.set`。
4. **计费外移（公平性决策点）**：官方把 parse/stringify 计入玩家 CPU（这是玩家实测 4% 的来源）。Rust 宿主可选择把序列化移出计量窗口（宿主付费），使 Memory 大小不再变相惩罚玩家；这会轻微改变体感公平性（大 Memory 用户相对受益 ~4%），需要显式决策。保守做法：仍计费，但因 1+2 的增量化，费用从全量级降到增量级。

### 3.5 结论

Memory 的增量上限：只读 tick 成本 → 0（与 Memhack 持平）；写 tick 成本 → O(被写键) 而非 O(全量)；2MB 上限用户从 ~180ms/tick 降到 µs–ms 级。

---

## 4. constants 每用户每 tick 深拷贝：直接消灭

### 4.1 现状证据

- game.js:435：`_.extend(globals, jsonParse(jsonStringify(C)));` —— 每用户每 tick，在玩家 CPU 窗口外（§0.1）。
- C 从哪来：isolate 创建时一次性 `ExternalCopy(index.constants)` 注入（user-vm.js:65），runtime.js:6 `driver.constants = _constants`。即**每 isolate 已经有一份干净常量**，每 tick 再 JSON round-trip 纯粹是为了给玩家一个"未被上一 tick 玩家篡改污染"的副本。
- 实测：23,332 字节 / 423 顶层键，round-trip ≈1.65ms（§0）。100 用户 = **165ms/tick 纯常量拷贝**。
- 同类小项：`Game.resources = jsonParse(jsonStringify(user.resources))`（game.js:170）；`game.rooms = _.clone(register.rooms)`（game.js:227）。

### 4.2 Rust 对应做法

1. **快照 + 深冻结**：runtime.bundle 已支持 V8 snapshot（user-vm.js:151-158 读 `runtime.snapshot.bin`）。把整棵 C 放进 isolate 快照并递归 `Object.freeze`（system-functions.js:20-28 已有 freezeDeep 可复用），每 tick 直接复用引用，成本从 1.65ms → 0。deno_core 等价物：启动 snapshot 或首次 isolate 初始化时一次物化 + 冻结。
2. **语义差异要记录**：官方语义是"玩家可改 C，下一 tick 复位"。冻结后玩家修改会静默失败（非严格模式）或抛错（严格模式）。没有任何官方文档把"可变 C"当特性；screeps-ok 若有对应用例再降级为方案 3。
3. **兜底方案**（若必须保留可变性）：每 isolate 创建时做**一次** pristine 深拷贝（1.65ms 一次性），每 tick 浅 `_.extend(globals, pristine)`（423 次属性赋值，≈µs 级）。代价：玩家改嵌套表会污染 pristine（浅绑不隔离嵌套对象），因此只对玩家实际可能碰的浅层键生效——比 JSON round-trip 弱但便宜 1000 倍；或检测到篡改（对比冻结副本引用）才整体重建。

### 4.3 收益

每用户每 tick 净省 ≈1.65ms（100 用户 165ms/tick），无任何游戏语义损失（方案 1 下仅"玩家改 C"这一未定义行为变化）。

---

## 5. market 访问侧的深拷贝：与 §3.2 配套消灭

### 5.1 现状证据（market.js）

- `_getOrders(resourceType)`：`JSON.parse(JSON.stringify(orders))` 整桶拷贝 + `price /= 1000` 原地改（market.js:21-24）——price 定点化逼出了这次拷贝；
- `getHistory`：同样整桶拷贝（market.js:50）；
- `getOrderById`：取单条也要 clone（market.js:57,61）——`Game.market.deal()` 内部直接读 `runtimeData.market.orders.all[orderId]` 不拷（market.js:109），说明**拷贝只为防玩家篡改视图，不影响结算**；
- `incoming/outgoingTransactions`/`orders` 是惰性 getter（market.js:203-249），但底层数据早已急切取回并入沙箱（§3.1）。

### 5.2 Rust 对应做法

- 订单以 blob 常驻（增量同步，§3.2），`getOrderById` = blob 表 O(1) 查找 + 惰性物化（xxscreeps order.ts:74-78 `get(id)` 直接 `readOrder(blob)`，无拷贝）；
- `getAllOrders(filter)`：filter 是玩家 JS 函数，必须物化对象——但改为**迭代时逐个物化**而非先整桶 clone 再 `_.filter`（market.js:36-39）；price 以整数毫信用存储、getter 里除以 1000，消除"因单位转换而拷贝"的动机；
- `Game.market.orders`/credits 同理走增量连接器。

### 5.3 收益

- 500 单订单簿访问侧 clone ≈4.0ms（实测 §0）→ 迭代物化 ≈ 只为 filter 命中项分配对象；
- 配合 §3.2，"从不读市场的玩家为市场付的成本"从每 tick 毫秒级 → 0。

---

## 6. find/look/spatial 索引与 wrapper：从"每用户每 tick 重建"到"每房间每 tick 一次、跨用户共享、按访问物化"

### 6.1 现状证据

- **spatial 寄存器**：populateRegister 给每个可见房间建 ~15 个类型 × 2500 槽的稀疏数组（game.js:104-110），对象循环里逐个插入（game.js:38-48 addObjectToRegister）。实测纯分配 ≈146µs/房间，加上插入与 GC 压力。
- **findCache 预构建**：对象循环里无条件把每个对象塞进 1–3 个 find 桶（game.js:282-381 各类 addObjectToFindCache 调用）——不管玩家这个 tick 是否 find。FIND_EXIT 系列则首次 find 时按地形现算并缓存（rooms.js:592-644）。
- **wrapper 全量实例化**：可见房间内**每个对象**都 `new globals.Creep/Structure.../...`（game.js:260-395）——玩家从不访问的对象也建 wrapper（含闭包、原型链挂接）。构造后逐属性 getter 惰性求值并有 per-instance 缓存（utils.js:509-545，eval 生成），`data(id)` 每次经 `runtimeData.roomObjects[id]` 闭包查找（creeps.js:50-58）；`store` 每个 wrapper 首访问再建 `new Store` + Proxy，Proxy miss 走 `C.RESOURCES_ALL.indexOf` 线性扫描（store.js:87-96）。
- **flags**：打包字符串每 tick split + 正则替换解析（game.js:397-429）。
- **map**：`makeMap` 每 tick `JSON.parse(runtimeData.accessibleRooms)`（map.js:18），而 WorldMapGrid 在 isolate 生命周期内已 parse 过一次（runtime.js:206-208）——同一份数据每 isolate parse 一次 + 每用户每 tick 再 parse 一次。
- **room status / lookTypeRegisters**：每房间建 13+2 张查找表（rooms.js:485-514）。
- **N 用户同房间 = N 份**：以上全部在每用户沙箱内独立执行。同房间的 10 个玩家 = 同一房间的 spatial/findCache/wrapper 重建 10 次。

### 6.2 Rust 对应做法

1. **每房间每 tick 一次构建（processor 侧，Rust 原生），全部观众共享**：空间索引用 flat `Vec<u16>`（2500 槽 → 对象在 blob 内的 slot 下标），随房间 blob 提交产出或由 blob 直接编码（对象在 blob 内本就按 slot 存储，索引即 O(1) 映射）。重建成本 Rust 侧 µs 级，即使不跨 tick 持久化也远比官方每用户 JS 重建便宜；跨 tick 持久化只在"blob diff 已知变更对象"时增量修补即可（processor 是唯一写者，diff 免费获得）。
   ——回应该 ask 的"持久化可能"：**可以做，但正确形态是"processor 每 tick 一次 + 共享"，持久化是锦上添花**；收益主体来自 N 用户去重（N→1）与语言差（JS 对象图 → flat 数组）。
2. **wrapper 惰性物化**：对象 = `(blob 视图, slot 偏移)` 薄壳，原型 getter 按编译期偏移读 typed array（xxscreeps overlay 语义：`packages/xxscreeps/schema/overlay.ts`，README.md:29-34 "玩家整 tick 没碰 creep.body，它就从未进入 V8 堆"）。`Game.creeps`/`find()` 返回的也是薄壳数组，只有玩家真正读属性才有字段级成本。`getObjectById` 用宿主侧 id→slot 哈希。
3. **FIND_MY_* 类过滤**依赖 userId 的部分（findCacheFn，game.js:15-28）不能跨用户共享——但那是纯谓词过滤，在共享索引上 O(命中数) 现算即可，无需预建桶。
4. **flags/accessibleRooms**：flags 进房间 blob（xxscreeps 即如此）；accessibleRooms 每进程解析一次常驻（消灭 map.js:18 的每 tick parse）。
5. **Store**：不建 Proxy 包装对象——`store` getter 直接从 blob 读资源计数字段（定长 u32×资源种类或稀疏 map），`getCapacity*` 为纯函数。只有玩家枚举 `for (const res in creep.store)` 时才物化普通对象。

### 6.3 量化

- 索引重建次数：从 `Σ_users Σ_可见房间 1`（实测分配 146µs/房间 + 插入）降到 `Σ_活跃房间 1`（Rust µs 级）。100 用户 × 5 可见房间 = 500 次/tick → 活跃房间数（≤ 世界房间数）次。
- wrapper：一个开发完成的房间 ~400 对象（arch-processor.md §9 的 N≈400），每用户每可见房间 400 次 JS 构造 + 后续 GC；惰性物化后只为玩家实际触碰的对象付费（典型 bot 每 tick 触碰 ≪ 全量）。
- CPU 计费不变：这些在玩家 CPU 窗口外（§0.1），全是服务器净收益。

---

## 7. RoomObject 访问路径在「Rust 宿主 + JS 沙箱」下的最优形态

结论先行：**blob 视图 + 沙箱内原型 getter（编译期偏移）**，宿主只供字节，不参与热路径调用。理由：

- 玩家代码属性读取频率极高（一个 bot 每 tick 10^4–10^6 次字段读）。若每次读取跨沙箱调宿主（deno_core op / ivm Reference.apply），单次 ≈1–10µs，比官方现状（闭包 + JS 对象属性，≈ns 级）**慢 2–4 个数量级**——绝不可走"每次访问回调 Rust"路线。
- 官方自己的两个先例证明正确形态：
  - 地形：SAB + 每房间 `new Uint8Array(buffer, offset, 2500)` 视图（make.js:45-49），沙箱内直接索引，无跨界调用；
  - 寻路：C++ 以 isolated-vm NativeModule 常驻沙箱（user-vm.js:7,39-40,64；runtime-driver.js:124-125），`PathFinder.search` 数据不跨界（CostMatrix Uint8Array 零拷贝）。
- 因此：**数据走 SAB 零拷贝，行为走常驻原生模块**（Rust 场景即 NAPI/deno NativeModule 形态：寻路、市场查询、getObjectById 这类"每次调用一次跨界但调用频率低"的操作；字段读一律沙箱内 getter 读 typed array）。
- 派生对象（Store、owner {username}、RoomPosition）按访问物化并缓存在 wrapper 实例上（沿用 utils.js:516-531 的 per-instance 缓存思想，但数据源从 `runtimeData.roomObjects[id]` 换成 blob 偏移读）。
- CPU 计量锚点保持官方语义：isolate CPU 时间 + 0.2×intent（runtime.js:60-100, 202-204；make.js:193-194），wrapper 物化若发生在玩家 loop 内则自然计费（与官方一致），loop 外的世界物化归服务器（§0.1）。

---

## 8. 其余可顺手消灭的小项（汇总）

| 项 | 证据 | Rust 做法 |
|---|---|---|
| user 文档取两次 | make.js:55 vs data.js:130 | 合并为一次 |
| gameTime 每 用户一次 RPC | data.js:134, common/index.js:65-67 | 随任务下发 |
| console 命令轮询 + 空 removeWhere 写 | data.js:133,149 | pubsub 推送，读侧无写 |
| accessibleRooms 每 tick JSON.parse | map.js:18 vs runtime.js:206-208 | 进程内常驻（每进程一次） |
| Game.resources 深拷贝 | game.js:170 | 冻结共享/惰性 |
| `_.clone(register.rooms)` | game.js:227 | 直接引用（房间枚举不随玩家变） |
| 每 tick `db.users.update` + 2×pubsub CPU 上报 | make.js:214, 263-273 | 内存记账，tick 末批量落盘/上报 |
| outMessage 整包 ExternalCopy（含 console/visual/memory） | runtime.js:276-296 | memory 走 SAB（§3.4）；console/visual 增量 |
| q promise 链 + 每秒 unhandled rejection 轮询 | runner.js:81-87 | 无（tokio/async fn） |

---

## 9. 量化总表：每用户每 tick 固定开销（官方 vs Rust 目标形态）

以"100 活跃用户、每用户 5 可见房间、500 单市场簿、106KB Memory、世界 400 活跃房间"为参照：

| 开销项 | 官方（每用户每 tick） | Rust 目标形态 | 依据 |
|---|---|---|---|
| 20 次 RPC 拼装 | ~2–6ms（0.1–0.3ms×20） | ≈0（索引读 + blob 读，进程内） | §2 |
| 整包 ExternalCopy（市场簿+历史+交易+事件日志+全部对象） | ~5–10ms | ≈0（SAB + 增量连接器） | §3.1-3.3，实测 500 单 ≈4ms |
| constants JSON round-trip | **1.65ms**（实测） | 0（快照+冻结） | §4 |
| Memory round-trip（106KB） | ~9.2ms（实测，计入玩家 CPU） | 只读 tick 0；写 tick O(脏键) | §3.4 |
| spatial/findCache/wrapper 重建 | ~0.7ms+（5 房间×146µs 分配）+ 2000 次 wrapper 构造 + GC | 每房间每 tick 一次共享（µs×400 房间）；wrapper 按访问 | §6 |
| market 访问侧 clone | 0–4ms（视用法） | 迭代物化，≈0 | §5 |
| 结果写回（update/pubsub/env.set×N） | ~1–2ms | 内存批量 | §8 |

**合计：官方每用户每 tick 固定成本 ≈10–25ms（世界越大越高），Rust 形态 ≈0–1ms + 按访问付费。** 100 用户即每 tick 1–2.5s → 0.1s 以内，与可行性报告"tick 受最慢房间约束"的目标吻合。

---

## 10. 相对已有报告的增量点清单

1. **固定开销在玩家 CPU 计量窗口外**（runtime.js:216 vs :235）——首次定位；说明消灭它们不影响玩家公平性（Memory 除外）。
2. data.js 20 查询的逐条清单与重复项（user 双取、gameTime RPC、空 removeWhere、transactions/roomEventLog 急切取数），及 xxscreeps 的增量连接器形态对照（instance.ts:222-248、wallstreet driver.ts:207-226）。
3. constants 深拷贝的实测量化（23,332B/423 键/1.65ms）与"快照+深冻结"具体方案及语义差异说明（此前报告只列了热点行号）。
4. Memory 惰性化的三层增量（每键惰性 parse、脏键序列化、计费外移决策点）——明确超出 xxscreeps Memhack 语义的部分。
5. find/look 索引的正确目标形态论证：**每房间每 tick 一次共享 > 跨 tick 持久化**（processor 唯一写者、diff 免费；持久化仅锦上添花）。
6. RoomObject 访问路径的量化否决："每次属性读回调宿主"慢 2–4 个数量级，正确形态 = SAB 字节 + 沙箱内编译期偏移 getter + 低频操作走原生模块（官方地形与寻路两个先例）。
7. market 价格定点化（price/1000）是拷贝的直接动机之一（market.js:23,62），整数存储 + getter 换算可消除。
