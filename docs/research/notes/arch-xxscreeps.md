# xxscreeps 增量设计深挖笔记

> 调研日期：2026-09-27。对象：`/Users/keqing/Desktop/projects/sources/screeps/xxscreeps`（含完整 git 历史，1147 commits）。
> 定位：**01 可行性报告（`docs/research/01-screeps-rust-feasibility.md` §2）之外的增量发现**，不重复报告已有结论。
> 所有行号以当前工作区 HEAD 为准；git 提交以 `git show <hash>` 现场查看为证。

---

## 1. `.changeset/` 与 git 历史中的性能设计演进（148 个 changeset + git log 全量扫描）

`git log --all --oneline | wc -l` = 1147。用
`git log --all --oneline | grep -iE 'fast|perf|optimi|speed|slow|leak|cache|batch|copy|overhead|efficient|lazy|sleep|wake|scratch|diff|bench|pgo|lto|icf|profil'`
筛出约 40 条性能相关提交，结合 `.changeset/*.md` 正文逐条核对。

### 1.1 "faster pathfinder" changeset 的真身

`.changeset/lovely-sites-tap.md` 正文就一行 "faster pathfinder"，对应提交
`06d8cbfa 2026-06-03 engine: update pathfinder`（`git log --all -- .changeset/lovely-sites-tap.md` 确认）。diff 核心（`packages/xxscreeps/driver/pathfinder.ts`，-76/+67 行）：

1. **删除 pf.version===11 的运行时双路径兼容**：旧代码按 `pf.version === 11` 在两个 `makePositionIn`/`makeCompletePath` 实现间选择（每处带 `@ts-ignore`）；新代码只保留一种。
2. 位置打包为 `(yy << 16) | xx` 的单 u32（现 `driver/pathfinder/pathfinder.ts:12-20`）。
3. 路径以**拐点压缩**从 native 返回，JS 侧生成器展开为逐格 `RoomPosition`（`pathfinder.ts:22-23` 的 `makePositionOut` + 展开逻辑）。

**Rust 对应**：位置直接 `u32`（`packed: u32` newtype），拐点压缩路径在 FFI 边界零成本传输——xxscreeps 已验证这套 ABI 是性能瓶颈最小的形态，照抄即可。

### 1.2 pathfinder 性能提交时间线（2020 → 2026 的完整演进）

| 提交 | 日期 | 内容（`git show --stat` 核实） |
|---|---|---|
| `d6aa7676` | 2020-04-07 | "Re-add PF PGO build. **It's not actually any faster now.**" —— PGO 首次尝试无收益 |
| `04978d6b` | 2020-04-01 | schema 构建开销削减（9 文件） |
| `35409c9a` | 2020-04-15 | creep 移动 cached path 功能（+213 行，`game/objects/creep.ts` 主体） |
| `d8038e54` | 2021-04-12 | Schema cache + Room schema 私有 key |
| `85926e9c` | 2021-07-01 | Batch local responder messages：responder 请求/响应改为 `process.nextTick` 微批成数组再 postMessage（diff 见 §4.3） |
| `4fd2b895` | 2021-06-04 | `Store` 拆成 3 种 shape/实现（单一 store 泛型拆为多形态） |
| `452e55cf` | 2021-08-29 | Optimized sorted set consumer：`consumeSortedSetMembers` 由逐成员 `ZPopByScore` 改为 **zmscore 批量探测（16 个/批）**再单点 zrem |
| `53d3a436` | 2021-08-28 | memory segments 载入改用 `SharedArrayBuffer`（`utf16ToBuffer(string, SharedArrayBuffer)`），免拷贝进 isolate |
| `54c090f6` | 2026-06 | pf: performance-guided optimization（PGO 重新引入） |
| `c6bceed2` | 2026-06-02 | pf: room index optimization（pf.cc 240 行重写 + position.cc 101 行改） |
| `f1df8be5` | 2026-06 | pf: position modernization & optimization |
| `1080e822` | 2026-06 | pf: additional modernization and optimizations |
| `65a8ea9f` | 2026-06-30 | pf: **heuristicWeight == 1 时改用 A\***（pf.cc:256-264，注释明说 JPS 在非均匀代价网格上会给出次优路径） |
| `3d136abb` | 2026-06-30 | pf: heap scores 惰性更新（lazy deletion） |
| `65a14182` | 2026-06-30 | pf: heap sift 改进（bottom-up heapsort 式 sift） |
| `b3682bd3` | 2026-07 | pf: PGO for `@isolated-vm/experimental` binaries |
| `41614e49` | 2026-07-13 | pf: fix linux PGO |
| `c23bcb98` | 2026-08-16 | pf: **dial's heap**（heap.cc 新增 107 行桶式堆） |
| `7852d4cc` | 2026-06-03 | 修复 isolate 内存泄漏（#242）：runtime `ivm.Reference` 未 `release: true` + memoryLimit 单位 bug（`.length` → `byteLength >> 20`） |

**演进结论**：作者 6 年间在 pf 上反复优化的排序是 算法(JPS/A\* 切换) > 数据结构(dial's heap) > 构建(LTO/PGO)。PGO 两次尝试（2020 无收益、2026 重引入并修到 linux 能跑）说明构建期优化只是锦上添花。

### 1.3 未发布的 pending changeset（`.changeset/` 当前驻留 148 个文件）

性能相关的 pending 项：
- `blob-cas.md`：blob `set` 支持 CAS（`if` 条件）——local 与 redis provider 都实现（原子写语义补齐）。
- `keyval-zrank.md`：keyval 新增 `zRank`，local range reads 尊重 `rev`。
- `available-parallelism-concurrency.md`：processor/runner 默认并发从 `os.cpus().length` 改为 `os.availableParallelism()`——**CPU affinity 场景不再超订**（对应 `config/config.ts:292,296` 的 `os.availableParallelism() + 1`）。
- `schema-blob-upgrade.md`（= PR #313, `7070f67e`）：load 时把所有持久 blob 升级到当前格式版本。
- `pathfinder-goal-lifetime.md`（= PR #317, `47dc2ffa`）：多目标搜索中 goal 存储的 use-after-free——native 持有 JS 传入数据的生命周期问题。
- `isolated-vm-memory-leak.md`（= #242, `7852d4cc`）。
- `backend-room-render-tick-coherence.md`：room-socket 更新按 blob 被加载的那个 tick 渲染——修 decay 字段（rampart `#nextDecayTime`）被 later tick 渲染时 overdue 抛 `Invalid expiry time`（代码注释 `backend/sockets/room.ts:96-99`）。
- `room-socket-missed-spawn-update.md`：first-spawn 竞态跳过的 room-socket 更新在窗口关闭后重放。
- `spicy-donuts-refuse.md`：不可序列化 `Memory` 跳过 save 而非每 tick 报错。
- `main-loop-reread.md`：每 tick 重读 `main.loop`，玩家重赋 `module.exports.loop` 生效。

其余 changeset 是玩法/工具向（decorations、stronghold、power-creep 等），与本调研无关。

---

## 2. packages/pathfinder：CMake 全量优化开关 + benchmark 细节

### 2.1 `packages/pathfinder/CMakeLists.txt`（170 行）

- **C++26 + modules**：`set(CMAKE_CXX_MODULE_STD ON)` / `CMAKE_CXX_STANDARD 26`（:25-26），源码是 `export module screeps:heap;` 形式的 C++20 modules（`src/heap.cc:1-5`）。
- **LTO**：`set(CMAKE_INTERPROCEDURAL_OPTIMIZATION TRUE)`（:42）。
- **链接器裁剪**：`CMAKE_LINKER_TYPE LLD`（:45）、hidden visibility（:46）、`-fdata-sections -ffunction-sections`（:47）、`-Wl,--icf=all`（ICF 等码折叠，:48）、linux 上再加 `--gc-sections`（:49-52）。
- **PGO 钩子**：`PGO_OUT`（-fprofile-generate）/ `PGO_IN`（-fprofile-use）（:54-61）。
- **双 target 同源**：`pf.<triplet>.node`（napi binding，`src/main.cc`）与 `iv.<triplet>.node`（`@isolated-vm/experimental` NativeModule，前缀 `isolated_`，`src/iv.cc`）共享全部算法模块（:94-108 与 :151-165 的 `target_sources` 列表相同）。即**同一份 C++ 同时编成 nodejs addon 和 isolated-vm 原生模块**，后者让玩家沙箱内 `PathFinder.search` 完全不跨 isolate（CMake 注释 :139-141 解释 AUTO_JS_PREFIX 技巧）。

### 2.2 dial's heap（`src/heap.cc:93-198`）

`bucket_heap_t<Type, Projection, Capacity, Window = 256>`：
- 256 个桶按 `key % Window` 环形分布，`base_` 单调推进，`settle()` 线性找下一个非空桶（:186-190）。
- 节点池 `inplace_vector<node_t, Capacity>` + 空闲链表，**1-based 索引、0 作哨兵**（"which compares faster"，:172-174）。
- 分数落在 `[base_, base_+256)` 窗口外（heuristic weight 造成下溢/上溢）时回退到 `heap_t` overflow 堆（:150-155），top/pop 时优先弹 overflow 中更小的（:119-148）。
- `heap_t` 本体是**惰性删除**的优先队列（:60-91 注释 "lazy deletion for score updates"）+ bottom-up heapsort 式 sift（:23-42 引 Wikipedia）。

### 2.3 A*/JPS 分派（`src/pf.cc:256-264`）

```cpp
if (delegate.heuristic_weight == 1) {
    // jps can sometimes produce suboptimal paths with non-uniform cost grids even with the added
    // forced neighbor heuristic. so, for heuristicWeight == 1 we will use astar for the best paths.
    dispatch(astar);
} else {
    dispatch(jps);
}
```
`ops_remaining` 预算来自 `max_ops`（:185），结果带 `.ops = options.max_ops - ops_remaining`（:276）。

### 2.4 benchmark / profile 脚本（`packages/xxscreeps/driver/pathfinder/profile.ts`，230 行）

自述（:19-23）："standalone test for the path finder. It runs a whole bunch of path finding operations on real terrain data from a screeps server. It also verifies that the results are the same as previous runs (kind of). **This is also used for profile-guided optimization builds.**"

- 负载：24 个固定 RoomPosition 兴趣点（真实世界地形文件 `terrain`）两两搜索 552 对（`dispatch`，:95-127），`maxRooms: 64`（注释点名某对会打满 64 房上限），`heuristicWeight` 按 `ii % 7` 混 1 与 1.2，一半启用 `roomCallback` 返回确定性 CostMatrix（`ii % 3` 代价 + `ii % 67` 墙，:64-92），外加一个递归嵌套 search。
- **正确性回归**：全部结果 JSON.stringify 后喂 SHA-256，取前 8 hex 与常量 `expectedResult = 'cb11d874'` 比对（:17, :205-209, :223-228）——算法改动必须保持输出逐位一致，否则 exit 1。
- `--with-sandbox` 模式（:129-209）：用 `@isolated-vm/experimental` Agent 起隔离 realm，同一 dispatch 函数 `String()` 序列化后注入模块执行，验证沙箱内路径（iv binding）与裸 native 一致。
- 计时 `process.hrtime`，迭代次数取 argv 尾参；`7e7b85e9` 特意把 profiler 的 heuristicWeight 调 1.0（覆盖 A* 分支）。

**Rust 对应**：benchmark 方法论可直接照搬——固定兴趣点集 + 确定性 CostMatrix + 输出哈希回归 + criterion 计时；这比泛泛"跑分"更能锁算法行为。

### 2.5 引擎侧驱动（`packages/xxscreeps/driver/pathfinder/pathfinder.ts`，75 行）

- `CostMatrix` 底层就是 `Uint8Array(2500)`（`game/pathfinder/cost-matrix.ts:16`），传 native 时直接给 `_bits`（`pathfinder.ts:59-68`），零转换。
- `roomCallback` 由 native 回调 JS（`packages/pathfinder/src/iv.cc:34-51` 的 `napi_room_callback::operator()`）——每进入一个新房间一次 JS 回调。
- 地形经 `loadTerrain` 一次性灌给 native（`pathfinder.ts:29-37`），进程内共享。

---

## 3. scratch 存储：生命周期与真实成本

### 3.1 scratch 是什么

`Shard` 持有三个 provider：`data`（持久）/ `pubsub` / `scratch`（易失），scratch 是**独立 KeyValProvider**（`engine/db/shard.ts:29,38-46`；默认 URI `local://shard0?socket=./screeps/.shard0.scratch`，`config/config.ts:312-317`，注释即 "Temporary storage provider"，:115-118）。

### 3.2 生命周期（不是"每 tick 重建整个 scratch"）

- **进程启动时清一次**：main 服务启动 `shard.scratch.flushdb()` 后把全部房间塞进 `initializeRooms`（`engine/service/main.ts:46-52`）。`flushdb` = 内存 Map clear + blob flush（`engine/db/storage/local/keyval.ts:813-817`）。
- **每 tick 创建与销毁 per-tick 键**：`tick{N}/processRooms`（zset，成员=房间，score=待归零的用户倒数）、`tick{N}/processRoomsPending`、`tick{N}/processedRooms`、`tick{N}/finalizedRoomsPending`（键名 `engine/processor/model.ts:37-46`）。计数归零时逐个 `vDel`（`model.ts:197,215-217,228-229`）。
- **常驻键**：`processor/activeRooms`（zset，score=intent 用户数）、`processor/inactiveRooms`（sleeping，score=唤醒时间）、`user/<id>/{intentRooms,presenceRooms,visibleRooms}`（set）、`rooms/<name>/intents`（list，用后即删）、`activeUsers`。

### 3.3 成本结构（Rust 重写的靶子）

1. **run/process 重叠的编排走 zset 原子计数**：runner 每发布一房 intents 就 `ZINCRBY -1`（`model.ts:113` `zAdd(..., { if: 'XX', incr: true })`），归零立即向 processor channel publish 该房间（`model.ts:117-130`）——语义上是"每房间完成即处理"，但实现是**每房间每用户一次 scratch RPC**。
2. **每 tick 主循环一轮 zset 大操作**：`begetRoomProcessQueue`（`model.ts:167-202`）做 `zRangeStore + zUnionStore + copy + zRemRange + ZCardStore + ZSetToSet`（六步，靠 `KeyvalScript.load` 预载的原子脚本保证原子性）。
3. **队列消费是逐成员 RPC**：`consumeSet`/`consumeSortedSet` 每弹一个成员一次 `sPop`/Lua `ZPopByScore`（`engine/db/async.ts:6-15,46-80`）；`consumeSetMembers` 已优化为 zmscore 16 个/批（§1.2 `452e55cf`）。processor/runner 各自用它们拉房间/用户队列（`engine/service/processor.ts:86-113`、`engine/service/runner.ts:74-77`）。
4. **intents 仍走 JSON**：runner 侧 `JSON.stringify({userId, intents})` rPush（`model.ts:91`），processor 侧 `lRange + vDel + JSON.parse`（`model.ts:146-156`）——这是 xxscreeps 里残留的最后一段 JSON 热点（好消息：不落盘、本地 provider 同进程时是函数直调）。
5. **多进程时的传输**：`local://` provider = 谁抢到 `FileSystemLock`（`config.database.lock`）谁当 host 起 unix socket，其余 sibling 进程连接（`engine/db/storage/local/responder.ts:13-30`）；请求-响应用 **v8 结构化克隆序列化，SAB out-of-band**（`engine/db/storage/local/port.ts:39` 注释），并按 nextTick 微批（`85926e9c`）。即多进程部署下 scratch 每次 zAdd/sPop 是一次 socket 往返。
6. **scratch 永不落盘**：`LocalKeyValResponder.create` 只在 `file:` 协议时才准备 data.json 读写（`keyval.ts:112-123`），`local://` 时 `this.url === undefined` → `save()` no-op（:818）；BlobStorage 在非 file 协议下 `path=null` 纯内存（`engine/db/storage/local/blob.ts:41-47`）。`shard.save()`（`shard.ts:88-90`）每 `saveInterval`=2 分钟（`config.ts:310`）调用，对 scratch 实际是空调用——**易失性是设计而非意外**，重启由 main 的 flushdb+initializeRooms 重建（`main.ts:46-52`）。

---

## 4. blob schema 版本升级机制（实现细节）

### 4.1 版本号 = 布局源码的内容哈希

`schema/build.ts:21-28`：把内存布局 `archiveLayout()` 成归档源码文本，`sha1` 后取前 4 字节 u32 当版本号。blob 头 16 字节 = `kPointerSize*4`（`schema/layout.ts:9`），magic `0x00fff35a`（`layout.ts:10`，校验在 `schema/read.ts:287` 与 `engine/schema/keyval.ts:39`）。

### 4.2 归档的存放与物化

- 启动时 `saveSchemaArchives` 把每个 package 的归档源码 `hSet` 到 `schema/<name>/<versionId>`（versionId 是 hex 字节反转串，`engine/schema/build/index.ts:16-17`），带 `created`(NX)/`seen` 时间戳；配置 `schemaArchive` 目录时**同步写 .js + .ksy（Kaitai 描述）到文件系统**，供 ide.kaitai.io 可视化调试（:32-60）。
- **升级路径** `makeUpgrader`（`engine/schema/build/index.ts:66-92`）：读 blob 头版本 → 相同直接返回原 buffer（一次 u32 比较）；不同则从 DB `hGet` 旧版本归档源码 → `restoreLayout` 重建 reader → **materialize 全量读出旧值（未指定字段取默认）→ 用新 write() 重写**。reader 懒加载缓存（`load ??=`），并 `hSet 'used'` 记录。每个读 blob 的服务透明执行（room blob：`shard.loadRoomBlob → RoomSchema.upgrade`，`engine/db/shard.ts:123-126`；terrain：`loadUpgradedWithWriteBack`，`shard.ts:96-107` + `engine/schema/keyval.ts:7-24`）。
- PR #313（`7070f67e`，changeset `schema-blob-upgrade.md`）把"load 时升级所有持久 blob"定为正式行为。
- issue #294 曾报 `kMagic` 变更导致全部旧 blob "Corrupted blob"（已关闭）——**magic/版本兼容是此机制的已知脆点**。

### 4.3 原子字段级 blob 更新（不走完整解码）

`UpdateSchemaBlob` KeyvalScript（`engine/schema/keyval.ts:31-100`）：本地实现用 DataView 直接在 blob 字节上做 `min/max/incr/set`（校验 magic+版本后，`offset = relativeOffset + 16`）；Redis 实现是等价 Lua（`struct.unpack/pack + SETRANGE`）。这是"存储端直接操纵 schema blob"的通用原语（提交 `05be3b2e`）。
`KeyvalScript` 体系 = 每个 script 双实现（本地 JS 函数 + Redis Lua），`model.ts:52-88` 的 `SCardStore/ZCardStore/ZSetToSet` 同款。

---

## 5. 休眠房间：唤醒条件与成本

### 5.1 入睡

`RoomProcessor.finalize`（`engine/processor/room.ts:198-247`）：
- `hasNoPlayers`：`#users.intents` 全是系统用户（`userId.length <= 2`，`room.ts:227`；NPC id 判据同 `model.ts:237`）。
- 且 `nextUpdate !== nextTime`（本 tick 没有任何 `wakeAt(nextTime)` 发生，`room.ts:243`）。
- 满足则 `sleepRoomUntil(shard, name, nextTime, nextUpdate - 1)`（`room.ts:245`）：**先把 blob copy 到双缓冲两侧**（防回滚覆盖，`shard.ts:149-155` `copyRoomFromPreviousTick`）→ `zRem activeRooms` → `zAdd sleepingRooms wakeTime` 且 `up: 'LT'`（**只提前不推迟**，`model.ts:285-295`）。

`wakeAt` 只能取 min（`room.ts:289-296`，传比当前 tick小的值直接 throw）。`setActive() = didUpdate + wakeAt(nextTime)`（`room.ts:280-283`）。

### 5.2 唤醒

每 tick `begetRoomProcessQueue`（`model.ts:167-202`）：`zRangeStore(tmp, sleepingRooms, 0, time, BY SCORE)` 把到期房间捞出 → `zUnionStore` 并进 activeRooms → 从 sleeping `zRemRange(0, time)`。**成本 = 每 tick 一次 zset 范围扫描（全服）+ 唤醒房间重新进入正常处理**。跨房 intent 会把接收房间拉回 active（`room.ts:240` 的 zAdd score 0 与 finalizeExtraRooms 集，`model.ts:133-144`）。
历史 bug：`9e90a0f3` "fix inter-room intents with incorrect wake"、`nuker-flight-wake-time.md`（在飞 nuke 的非法 wake time 直接崩 processor）——wake 语义是易错区。

### 5.3 睡眠期间的每 tick 成本 ≈ 0

不进 processRooms zset、不被 processor 触碰；blob copy-forward 在本地 provider 下是内存引用级 `data.copy`（`keyval.ts:129-147`）。processor worker 还有**跨 tick 房间对象缓存**：`roomCache/nextRoomCache` 双 Map，时间推进时滚动复用已物化的 Room（`engine/processor/worker.ts:42-45,69-80`），同 worker 亲和的活跃房间也不必每 tick 重新 loadRoom。

---

## 6. backend 按订阅者 diff 推送

`backend/sockets/room.ts`（206 行）：

1. **全局共享订阅层**：`globalSubscriptionsByRoom` 每 roomName 一个共享 task（:45,53）——N 个观察者只 load 一次 blob、只挂一次 channel 监听；`subscribeToRoom` 返回的 listener 广播 `(room, time, didUpdate)`。
2. **跳过未变房间**：processor 存 blob 时 publish `didUpdate`（`shard.ts:141-147`），backend 记 `didUpdate` 标志；tick 事件到来时若 `didUpdate` 为 false **不重新 loadRoom**，只把 `gameTime` 推给订阅者（:100-104）。throttle 默认 125ms（`config.backend.socketThrottle`，`config.ts:286`；`room.ts:91,106`）。
3. **每订阅者渲染 + diff**：socket 端点各自维护 `previous` 对象表（:127,168-171）；`didUpdate` 时在 `runOneShot`（以观察者身份）里对每个对象跑 `Render` symbol，产出 plain object，与 previous 做**递归逐键 diff**（:17-38，仅标量变化/增删键进入 payload），`JSON.stringify` 后 `this.send`（:198）。新增可见用户才去查用户信息（:178-185）。
4. 渲染 tick 一致性（§1.3 `backend-room-render-tick-coherence`）：diff/渲染按加载 blob 的那个 renderTime，防 decay 字段 overdue（:95-99 注释）。

**残余成本**：diff 与渲染都是 JS 引擎侧每订阅者重算（`O(subscribers × objects)`），且输出 JSON 文本——Rust 版可做 blob 级/字段级增量（结构感知 diff 直接消费 blob 双缓冲）+ 渲染产物按 (room, tick) 缓存共享、按订阅者权限过滤。

---

## 7. runner 沙箱迁移机制

`engine/service/runner.ts`（133 行）+ `engine/runner/instance.ts`（319 行）：

- **每用户持久 PlayerInstance**（含 isolate）缓存在 runner 进程内（`service/runner.ts:55-57`），代码更新（branch switch/update 消息）只置 `stale`（`instance.ts:129-144`）。
- **租约式迁移，不是状态搬迁**：每 tick runner 消费 `runnerUsersSetKey(time)`（main 从 activeUsers 拷贝，`service/main.ts:106`）。先走 **affinity 迭代器**（只认领本 runner 已有实例的玩家，`consumeSetMembers`，`service/runner.ts:74-76`）；若队列仍有剩余（`sCard > 0`）且 `migrationTimeout > 0`，则 `Promise.race([tickFinished, setTimeout(50ms)])`——**给拥有该玩家的 runner 50ms 优先窗口**（:79-91，`config.ts:186-189,302` 默认 50）。超时才走 fallback 迭代器认领 → 新 runner `PlayerInstance.create`（全新 isolate），旧 runner 在 tick 末把未 seen 的实例 `disconnect`（:121-127 "Throwaway migrated player sandboxes"）。
- **迁移代价 = 玩家硬 reset**：玩家 global 丢失、code/memory 重新载入、当 tick 跳过（`instance.ts:172-198` 的 `wasStale → reset + return`）；沙箱重建成本 = 新 `ivm.Isolate(memoryLimit 256MB + terrain MB)` + createContext + compileScript(runtime bundle) + pathfinder NativeModule 实例 + ivmInspect（`driver/sandbox/isolated/index.ts:64-117`）。runtime bundle 源码每 runner 进程只编译一次（`runOnce`，:24-57），但**每个 isolate 都要 compileScript 一次**（:82-87）——没有用 V8 snapshot。
- **三档沙箱后端**可配（`config.runner.sandbox`，`config.ts:192-199`；`driver/sandbox/index.ts:44-68` 分派）：`isolated`（isolated-vm，默认）/ `experimental`（@isolated-vm/experimental）/ `unsafe`（node:vm 同 isolate，最快但玩家可窥探引擎态，architecture.md:103-107）。
- **每 tick 执行**：`tick.apply(..., { arguments: { copy: true }, timeout: tickLimit(500ms) })`（`isolated/index.ts:129-137`）。roomBlobs 从 blob 存读出时就是 `new SharedArrayBuffer`（`blob.ts:127,161`）——v8 结构化克隆对 SAB 是共享而非拷贝，这就是"blob 零拷贝进沙箱"的落地机制。CPU = `isolate.cpuTime` 差值（:139-141）；超时/disposed 各有专门 completion（:146-162，含 memory-limit reaper 与反序列化竞态的兜底）。
- **tick 掉队保护**：`time !== shard.time` 直接 throw "has been left behind"——`intentAbandonTimeout`(5s) 对 `tickLimit×concurrency×玩家数×runner 数` 不够时宁可硬 reset（`instance.ts:213-218` 注释）。

---

## 8. docs / issue 中的性能声明与已知缺口

- `docs/architecture.md` 是唯一架构文档（README 之外），性能相关仅：零值默认设计原则（"Schema fields cost real bytes... booleans should be designed so the common case is represented by `0`"，:240-273，`optional('bool')` 多 1 字节、~500KB payload 会累积）、runner best-effort / processor authoritative（:109-110）。
- main loop 每 tick 打 `Tick N ran in Xms; avg Yms`（100 tick 滑动平均，`service/main.ts:43,178-179`；`1217190d` 曾缩短平均窗口）——**仓库内没有任何正式 benchmark 数字**，性能证据只有 profile.ts 的正确性哈希 + 社区实测（01 报告已录）。
- **已知设计缺口（issue）**：
  - #291（OPEN）：keyval 存不下"不适合进内存 KV 的数据"（11 年消息史、月度 room stats）——存储抽象的扩展点讨论，最后以 mod 可替换 MessageStore 收场；
  - #241/#242：isolated-vm 内存泄漏（已修，见 §1.2）；
  - #267：多容器部署死锁（processor 初始化 0 房间、永不 tick，已修）；
  - #294：kMagic 变更炸全部旧 blob（schema 版本机制的兼容脆点，已修）；
  - #52：2021 年的 roadmap issue，表中大量 ❌（power/nuker/stronghold/market…）如今已由 changeset 历史补齐——该 issue 已过时，不可作为现状依据；
  - #328（OPEN）：isolated 沙箱把 runtime `module`/`exports` 全局泄漏进玩家作用域。
- `makePackagesModule` 的注释坦白一处刻意的双重 stringify（`engine/schema/build/index.ts:103-105`："JSON parsing at runtime is faster than JavaScript parsing"）——schema 版本表注入玩家沙箱的方式。

---

## 9. 汇总：Rust 重写可径直吸收的决策与必须重做的部分

| xxscreeps 已验证的决策 | Rust 做法 |
|---|---|
| scratch 全易失 + main 启动重建（`main.ts:46-52`） | 同样分层：persistent(blob file) / ephemeral(进程内 Map)，但**编排计数器改为进程内原子 i32 + channel**，消灭 §3.3 的逐成员 RPC 与 zset 六连操作 |
| run/process 重叠 = 每房间用户计数归零触发（`model.ts:107-131`） | 语义照抄：`AtomicI32` fetch_sub 归零 → 投递 room task 给 rayon |
| blob 双缓冲 `room{time%2}` + copy-forward + didUpdate 跳读 | rkyv 双 slab 同构；copy-forward 是指针交换 |
| schema 版本 = 布局哈希 + 归档 DB + 读时透明升级（§4） | 编译期布局 + build 脚本生成的版本常量；升级器= 旧版本 reader 编译期生成，运行时只做 materialize→rewrite |
| 休眠房间 zset wakeTime + LT-only（§5） | 进程内 BTreeMap<wakeTime, room> 或 timing wheel；语义（只提前、跨房拉活）照抄 |
| per-subscriber 渲染 diff + 共享 load + didUpdate 跳过（§6） | 引擎侧结构感知增量 + 渲染缓存；订阅者只做权限过滤 |
| runner 亲和 + 50ms 迁移租约 = 硬 reset（§7） | 单进程 isolate 池无需迁移；若多进程，用一致性哈希固定归属避免硬 reset |
| dial's heap / A*切换 / 拐点 ABI / checksum 回归（§2） | pf crate 直接移植；profile.ts 方法论变成 criterion+哈希回归测试 |
| LTO/ICF/PGO（2020 无收益→2026 复活，§1.2） | cargo 默认 `lto = "thin"`、`codegen-units = 1`；PGO 列为可选发布步骤，不指望它救命 |

## 10. 本次未做 / 未能验证的事项

- 未实际编译 pathfinder（CMake 4.4 + clang modules 工具链在本机不保证可用），LTO/ICF/PGO 的量化收益未测——只有作者提交信息（2020 年"not actually any faster"）可引。
- 未运行 profile.ts benchmark（需要真实 `terrain` 世界文件，仓库不含）。
- GitHub issues 只翻了前 100 条 state=all 的列表 + 精读 #52/#291；PR 讨论未逐条排查。
- packages/client、packages/redis（xxscreeps-redis）未深挖（不在任务清单内）。
