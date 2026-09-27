# 引擎处理端热路径挖掘笔记（processor / intents）

> 调研日期：2026-09-27
> 调研对象：`sources/screeps/engine/src/processor.js`、`engine/src/processor/`（全部 intent 实现）、`engine/src/processor/global.js` 及其 driver 侧对应实现（`driver/lib/index.js`、`driver/lib/bulk.js`、`driver/lib/history.js`、`driver/lib/queue.js`）。
> 定位：**上一轮报告（`docs/research/01-screeps-rust-feasibility.md`）未覆盖的算法级 / 数据结构级优化点**。上一轮已覆盖的宏观点（JSON 序列化层、进程拓扑、全局屏障、每 tick 全量快照的"存在性"）不再重复；本轮给出的是逐条 file:line 级热路径、量级估算与 Rust 重写对应做法。
> 基准环境：Node v24.11.0（Apple Silicon），使用 engine 自带 lodash 3.10.1；测量为单房间相对量级，非生产绝对值。

---

## 0. 本轮实测数据（自己跑的基准）

在 `sources/screeps/engine/` 下用 `node -e`（require engine 自带 lodash 3.10.1）构造"开发完成的 RCL8 房间"：N=400 个对象（350 结构 + 50 个 50-part creep），测量各热点层的耗时。两次运行的命令与输出：

```
cd sources/screeps/engine && node -e "<见下述各段脚本>"
```

| 热点层 | 对应源码 | 实测耗时（每房间每 tick） |
|---|---|---|
| 50× portal 全表 `_.find` | `processor/intents/creeps/tick.js:81` | **1.007 ms** |
| 50× 目标格 `_.filter`（movement.execute） | `processor/intents/movement.js:202` | **1.096 ms** |
| 50× 障碍判定全表扫描 | `processor/intents/movement.js:16-39`（check 里 183 调用） | **0.472 ms** |
| 29 类型 if 链 × 400 对象 | `processor.js:342-419` | **0.230 ms** |
| 50× actionLog `_.isEqual` | `processor/intents/creeps/tick.js:98` | 0.031 ms |
| 50× creep 历史快照 `JSON.parse(JSON.stringify())` | `processor.js:432` | **0.325 ms** |
| 1× 全房间 `JSON.stringify`（历史） | `processor.js:532` | **0.175 ms / 100,588 字节** |
| movement.check 争议格 rates（3 次 body 全扫 × 50 creep） | `movement.js:116-122` | **0.322 ms** |
| movement.execute 疲劳计算（2 次 body 全扫 × 50） | `movement.js:236-239` | **0.384 ms** |
| NPC 寻路 CostMatrix 构建 ×10（5 invader × 2 次搜索） | `processor/common/fake-runtime.js:104-141` | **0.334 ms** |
| 50× 单元格索引（Map<u16, list>）构建 + 200 次查询 | （对照组） | **0.043 ms** |

结论锚点：**仅"按坐标找对象"这一层（move intent + 障碍判定 + execute + portal 四项），每房间每 tick ≈ 3.7 ms 纯扫描；同样的查询需求用一次性构建的格子索引只需 0.043 ms，≈ 80×。** 这还不含 intents 自身的扫描（攻击找 rampart、rangedMassAttack、build 等）和输入侧 JSON parse。

---

## 1. 处理端流水线的固定成本（每房间每 tick）

### 1.1 单进程一次一房 + 每房约 10 次 RPC 往返

`processor.js:549-590` 的 `loop()`：`roomsQueue.fetch()` 取**一个**房间 → `q.all` 并发 6 个读（`processor.js:559-566`：intents / objects+users / terrain / gameTime / roomInfo / flags）→ `processRoom` → `bulk.execute()` ×4 + `mapViewSave` + `saveRoomEventLog`（`processor.js:493-497`）→ `clearRoomIntents`（`driver/lib/index.js:322-324`，`removeWhere` 再一次 RPC）→ `markDone`。每个 RPC 帧都是 JSON（`common/lib/rpc.js:26-53`，上一轮已证）。

- 量级：100 活跃房间/tick ≈ 1000 个串行-分批 RPC 帧；每个房间读侧至少要等 1 个网络往返 + 全量对象 JSON parse。
- Rust 做法：进程内 blob（上一轮 §4.2），读侧为内存直读；房间处理天然可 `rayon` 并行（xxscreeps 已证 per-room 重叠可行）。

### 1.2 两次全对象 forEach + per-object 事件钩子

- 预处理遍历：`processor.js:43-171`（每 creep 重建 13 键 actionLog 对象、分类收集 keepers/invaders/spawns/extensions/nukes）。
- 结算遍历：`processor.js:342-483`（29 个 `if (object.type == ...)` 逐个比对 + 事件钩子 `postProcessObject` per object）。
- `driver.config.emit` 在热路径被 per-object 调用至少 4 次（`processor.js:169,246,318,482`），无监听者时也是注册表查找 + 函数调用 ×4N。
- 量级（N=400）：29×400 ≈ 11,600 次字符串比较（实测 0.23 ms）+ 两遍 N 次回调。
- Rust 做法：对象按类型分桶（`enum GameObject` + `Vec per type`，见 §6 SoA），两次遍历合并为类型分桶的一次遍历；钩子改为编译期注册的 Option/空槽跳过。

### 1.3 每房间每 tick 的无条件写

- `mapViewSave`：`processor.js:493` → `driver/lib/index.js:530-532`，每次全量重建 mapView（`processor.js:328-337, 442-481`，约 400 次 push）+ `JSON.stringify` + env.set RPC。**其中 w/r/s/c/m/k/pb/p 这些静态层（墙/路/矿/控制器坐标）只在建造/摧毁时才会变**，每 tick 重复写。
- `saveRoomEventLog`：`processor.js:497` → `driver/lib/index.js:583-585`，**eventLog 为空也写**（`JSON.stringify([])` + hset RPC）。
- `_.isEqual(roomInfo, oldRoomInfo)`（`processor.js:509`）+ `_.clone(roomInfo)`（`processor.js:29`）：深比较换一次条件写，方向正确，但 clone+isEqual 本身每 tick 都跑。
- Rust 做法：脏标记（eventLog 空 → 跳过）；mapView 拆"静态层（结构版本号触发重算）+ 动态层（creep/所属结构坐标）"，静态层只在结构集合变化时重写；进程内共享内存后这两个写本身不再有 RPC 成本。

---

## 2. movement.js 仲裁与障碍判定（本轮最大单项）

### 2.1 四处 O(N) 全表扫描叠加

| # | 扫描 | 位置 | 触发频率 |
|---|---|---|---|
| 1 | move intent 目标格对象 | `processor/intents/creeps/move.js:36`：`_.filter(roomObjects, {x:.., y:..})` | 每个提交 move 的 creep 每 tick |
| 2 | 障碍判定 | `processor/intents/movement.js:16-39`：`_.forEach(roomObjects, ...)` 按 x/y 过滤 | `check()`（`movement.js:183`）对**每个候选移动**调用 |
| 3 | 落地格对象 | `processor/intents/movement.js:202`：`_.filter(roomObjects, i => i.x == move.x && i.y == move.y)` | `execute()` 每个成功移动者 |
| 4 | portal 检查 | `processor/intents/creeps/tick.js:80-85`：`_.find(roomObjects, i => i.type == 'portal' && i.x == .. && i.y == ..)` | **每个非 spawning creep 每 tick 都查**（不管有没有 portal）；powerCreep 同款 `power-creeps/tick.js:44` |

50 creep 全动的房间：4 × 50 × 400 = **80,000 次谓词回调/房间/tick**（实测 ≈ 3.7 ms）。房间对象越多（重路网/重城墙 RCL8 房 N 可到 500+）越糟，因为扫描的是**全部对象而不是 creep**。

补充细节：
- `checkObstacleAtXY` 里对每个对象重复合格判断（safeMode 分支 `movement.js:22`），`_.contains(C.OBSTACLE_OBJECT_TYPES, i.type)` 是数组线性查。
- `check()` 的争议格 rates 计算（`movement.js:116-137`）对每个竞争者做 3 次 body 全扫（`calcBodyEffectiveness` + `_.filter(body)` + `calcResourcesWeight`，`utils.js:623-636, 41-59`）；之后 `execute()` 又做 2 次 body 全扫算疲劳（`movement.js:236-239`），`creeps/tick.js:105` 再做 1 次 move power。**同一个 creep 每 tick 至少 6 次 50-part 全扫**（实测 0.73 ms/50 creep），而 move power / 疲劳基数只会在 body 受损或 store 变化时改变。
- `movement.execute` 落地时顺带做道路磨损 + 敌方工地踩踏（`movement.js:211-233`），同一份 cellObjects 复用，这点是好的；问题只在获取 cellObjects 的方式。

### 2.2 Rust 对应做法与量化

- **格子索引**：`Vec<SmallVec<[ObjectId; 4]>>` 长度 2500（或 u16 key 的 HashMap），房间加载时一次 O(N) 构建，对象增删/移动时增量维护（movement 本来就是唯一批量改坐标的地方）。按坐标查询 O(格内对象数 k≈1~4)。
- 80,000 次谓词回调 → 200 次 × k 次数组访问 ≈ **~800 次内存访问（~80× 减少）**；实测对照 3.7 ms → 0.043 ms。
- **障碍位图**：地形是静态 2500 字节（`utils.checkTerrain`，`utils.js:333-336`）；静态障碍结构（wall/非公共 rampart 归属判定除外）可再叠一层"结构占用位图"，`checkObstacleAtXY` 变成 2 次位测试 + 格内列表小循环。
- **body 派生量缓存**：`move_power: u16`、`fatigue_base: u16`（非 MOVE/CARRY 部件数）、`carry_weight`（随 store 变化）。受击（`_recalc-body`，`processor/intents/creeps/_recalc-body.js`）与 store 变化时失效。6 次全扫 → O(1) 读，15,000 次部件访问（50 creep × 6 × 50）→ ~50 次。

---

## 3. 各 intent 的全对象线性扫描清单（grep 全量核对）

对 `engine/src` 执行 `grep -rn "roomObjects" processor/ processor.js | grep -E "_\.(find|filter|forEach|any|some|...)\("`，共 56 处命中。热路径上的（每 tick 高频触发）：

| Intent | 扫描 | 位置 | 频率 |
|---|---|---|---|
| attack / rangedAttack / dismantle / towers-attack | 目标格 rampart 查找 | `creeps/attack.js:33`、`rangedAttack.js:33`、`dismantle.js:27`、`towers/attack.js:27` | 每次攻击 intent |
| rangedMassAttack | 先全表过滤 7×7 目标，再**每个目标**全表查 rampart | `creeps/rangedMassAttack.js:26-38` | 每 RMA intent：O(N + T×N) |
| `_damage` | melee 反击前全表查自身格 rampart | `_damage.js:17` | 每次近战伤害 |
| withdraw | 全表查目标格敌方 rampart | `creeps/withdraw.js:25` | 每 withdraw |
| build | 全表收集目标格对象 | `creeps/build.js:29-35` | 每 build intent |
| harvest(mineral) | 全表查同格 extractor | `creeps/harvest.js:73` | 每矿物采集 |
| `_create-energy` | 全表查 container + 全表查同格能量堆 | `_create-energy.js:20,35` | 每次 drop/衰减结算（tombstone/ruin/container 衰减都会走到） |
| `_die` | 全表查同格 container | `creeps/_die.js:37` | 每次死亡 |
| spawns/tick | 全表 filter spawn（RCL 数量门控）| `spawns/tick.js:35` | **每 spawn 每 tick** |
| spawns 出生 | 出生点占用检查全表 any×2 | `spawns/_born-creep.js:28-38,60` | 每次出生结算 |
| keeper-lairs/tick | 全表按名字查 keeper ×2 | `keeper-lairs/tick.js:13,20` | 每 lair（nextSpawnTime 为空时每 tick） |
| creeps/tick（spawning 分支） | 全表查出生点 | `creeps/tick.js:38` | 每 spawning creep 每 tick |
| `checkStructureAgainstController` | 全表按距离逐个数同类结构 | `utils.js:479-504` | 链路传输 / 采集矿物门控时（RCL 降级后每 tick 常态化） |
| `checkControllerAvailability` | 全表 filter 同类结构计数 | `utils.js:350`、调用点 `room/create-construction-site.js:25` | 每建 site intent |

这些全部是"按 (x,y) 找同格对象"或"按 type 收集"两种形态——前者被 §2 的格子索引消灭，后者被 §6 的类型分桶消灭。

---

## 4. NPC AI 的寻路风暴（invader/keeper/stronghold 房）

- `creeps/invaders/pretick.js:17-40`：**每个 invader 各做一次全表 forEach** 收集 creeps/hostiles/fortifications（5 个 invader = 5 × N）；`keepers/pretick.js:23` 同款。
- `creeps/invaders/findAttack.js:44-77`：每个不带 heal 的 invader 依次尝试**最多 4 轮** `findClosestByPath` + `moveTo`（不同 costCallback 组合），失败分支还对**每个 spawn 各跑一次 `checkPath` 全寻路**（`findAttack.js:83-86`）。
- `fake-runtime.findPath` 每次搜索都重建 2500 字节 CostMatrix：`defaultCostMatrix` 全表扫描 roomObjects（`fake-runtime.js:104-141`，实测 10 次构建 0.33 ms @N=400）+ 原生 search。
- `findClosestByPath` 把**每个候选目标都变成 goal**（`fake-runtime.js:283`），一次搜索的 goals 数 = 敌方 creep 数。
- 量级：一个 5-invader 房间每 tick ≈ 5 × (1 次全表收集 + 最多 4-6 次寻路 × 每次全表 CostMatrix)。相比玩家 creep 的移动 intents，NPC 房的 processor 成本高一个量级。
- Rust 对应做法：CostMatrix 基底（静态结构 + 地形）按房间缓存、版本号失效（结构增删才重建）；creep 位置层单独一个可复用 overlay（2500 字节 memcpy 后盖上 creep 占用）；invader 之间共享同一份"敌我分类"结果（一次全表收集供全部 invader 复用，官方代码每个 invader 各自扫一遍）；`reusePath` 语义官方已有（`fake-runtime.js:204-238`），保留即可。

---

## 5. 逐 tick 重复计算 → 增量化机会汇总

| 重复计算 | 位置 | 增量化做法 |
|---|---|---|
| body 派生量 6 次全扫/creep/tick | `movement.js:116-122,236-239`、`creeps/tick.js:105`、各攻击 intent 的 `calcBodyEffectiveness` | 受击/store 变化时失效的缓存字段（§2.2） |
| actionLog：重建 13 键对象 + `_.isEqual` 深比较决定是否写库 | `processor.js:51-65`、`creeps/tick.js:98-100` | bitflag 记录本 tick 是否写过任一 action；没写过直接跳过比较与 bulk.update |
| `energyAvailable` 全表 filter+sum | `processor.js:326` | 在 spawn/extension 的 store 变更处累加维护（intents 里所有改动 store 的点都集中在本文件内） |
| mapView 全量重建+序列化 | `processor.js:328-337,442-481` | 静态层版本化；动态层只序列化坐标变化 |
| 全房快照 `JSON.stringify` | `processor.js:532` | tick 内已知变更集（bulk 里就有）→ 直接产出 delta；见 §7 |
| creep 历史快照深拷贝 | `processor.js:431-439` | 同上；blob 内存表示 + 按需序列化 |
| `roomInfo` clone + isEqual | `processor.js:29,509` | 字段级脏标记 |
| spawn/extension RCL 数量门控（排序） | `_calc_spawns.js:9-40`（预处理已算一次）但 `spawns/tick.js:35-41` 每 spawn 再 filter+sort 一遍 | 预处理结果复用（同一 tick 内不变量） |
| `terminalDeals.sort` 比较器里反复 `calcRoomsDistance`→`roomNameToXY`（parseInt 字符串解析） | `global-intents/market.js:289`、`utils.js:374-392` | 房间名预解析为 (i16,i16)（世界加载时一次） |

---

## 6. 类型固定顺序 → SoA / 并行调度

现状：结算阶段单遍 forEach、29 个类型 if 链（`processor.js:342-419`），类型顺序硬编码（invaderCore → energy → source → … → storage）。语义上这个顺序承载了真实依赖（例：creep tick 里的 `movement.execute` 先扣道路 `nextDecayTime`（`movement.js:213-221`），随后 road tick 才结算衰减（`roads/tick.js:10-31`）；tower 攻击要先于目标 creep 的 tick 应用伤害？——实测顺序：tower(18) 在 controller(19) 之前、creep(6) 之后，creep 的 `_damageToApply` 在自己 tick 内结算（`creeps/tick.js:120-136`），所以跨类型依赖是"同 tick 内前序类型的写对后序类型可见"）。

Rust 对应做法：
1. **类型分桶（SoA）**：加载时一遍把对象分发到 `Vec<Creep>`、`Vec<Structure>`、`Vec<Road>`…，枚举 match 分发取代 29 次字符串比较（11,600 次 → 0）。
2. **依赖 DAG 而非线性序**：xxscreeps 已用"注册式 processor + 拓扑排序 + 冲突位掩码"（上轮报告 §2.1.7）。可并行的组：不同类型且字段不相交的 tick（如 road/rampart/container 衰减组、source/mineral/deposit 重生组、lab/link/factory 组）；必须串行的锚点：movement 仲裁（改坐标）→ 依赖坐标的所有判定。房间内 `rayon` scope，房间间天然并行。
3. 确定性：分桶后同类型内按 objectId（或房间内稳定序）迭代，`BTreeMap`/排序保序，replay 可重放。
4. 批量化红利：衰减类 tick（road/rampart/container/tombstone/ruin 全是 `nextDecayTime` 比较 + 一次减法 + bulk.update）可合并为"到期堆（BinaryHeap by nextDecayTime）"——O(到期数) 而非 O(该类型总数)。RCL8 房 200 条路 + 100 条 rampart 每 tick 的遍历变成只弹出到期项（每 tick 平均 <1 条）。

---

## 7. eventLog / history / mapView 写放大（量化）

- **history 每 tick 全量快照 ×20 再压缩**：`processor.js:522-536` 把 `JSON.stringify(objectsToHistory)`（实测 100KB@简化对象，真实重房间 200-400KB）`hmset` 进 `roomHistory:<room>`；每 `historyChunkSize=20` tick（`backend-npm/lib/game/server.js:39`）`upload()`（`driver/lib/history.js:11-43`）把 20 份全量快照**全部 JSON.parse 回来**、用 `common.getDiff`（`common/index.js:69` 起，全字段 `_.isEqual` 深比较）算 19 份 diff 上传，然后删 key。即：**写放大 20×，读回来再 parse 20 × 100KB+，diff 计算又是 19 × O(N×字段数) 深比较**。
- **历史写全局串行**：`currentHistoryPromise` 是进程级单链（`processor.js:11,524-534`），该 processor 进程内所有房间的 stringify+写盘严格串行——多房间时这是隐藏的串行点。
- Rust 对应做法：tick 内已有 bulk 变更集 → 直接增量产出本 tick delta（字段级，rkyv/postcard 编码），20 tick 合并即历史块；写放大从 20×全量降为 20×（1 全量 + 19 delta）。100KB×20 ≈ 2MB/房间/块 → 约 100KB + delta（经验 <10%）；同时消灭 upload 时的重新 parse + getDiff（≈ 20×100KB parse + 19 次深比较，每 20 tick 每房间）。
- eventLog/mapView：见 §1.3，空/未变也写。Rust 下 eventLog 事件在产生点直接 append 到 ring buffer，只在非空 tick 落盘。

---

## 8. global.js 跨房结算的批处理机会

- **数据装载是全世界扫描**：`driver/lib/index.js:429-449` `getInterRoom()` 每 tick 从 DB 拉：全部 `status:'normal'` 房间、**全世界所有 terminal/powerSpawn/powerCreep**（行 435）、全部 market orders、全部 users.intents，再 `_.indexBy(users)`/`_.groupBy(roomObjects,'type')`（`global.js:28-29`）。MMO 规模（数万房间、数千 terminal）下这是每 tick 的全表 JSON 流；私服规模（百房）也是数百 KB 级。
- **market 每 tick 全量重算**：`global-intents/market.js:497-589` 对**每一条 order** 每 tick 重算 active/amount（卖单看 terminal 库存、买单看钱和终端空间）——绝大多数 order 这两项每 tick 都没变。买单分支还每次 `calcTerminalEnergyCost`/`calcResources`。
- Rust 对应做法：terminal/powerSpawn/powerCreep/order 维护**内存注册表 + 变更事件驱动**（terminal store 变化、order 增删改、用户余额变化入脏集合）；每 tick 只处理脏集合 + 有 intent 的条目。O(全部 order) → O(活跃 order)。
- 顺带的确定性问题：`directDeals = _.shuffle(...)`（`market.js:400`）用 `Math.random`，**跨实例/重放不可复现**；Rust 版应换成种子化 PRNG（上轮报告 §4.3 已提确定性总则，这里是具体命中点之一）。
- powerProcessor 与 marketProcessor 共享同一份装载（`global.js:45-49`），批处理改造只需做一次。

---

## 9. 量化总表：50-creep 开发完成房间（N≈400 对象）每 tick 操作数对比

| 层 | 现状（官方 JS） | Rust 优化后 | 倍数 |
|---|---|---|---|
| 按坐标查询层（move/障碍/落地/portal） | ~80,000 次谓词回调 ≈ 3.7 ms | 一次 O(N) 建格索引 + 200×k 次数组访问 ≈ 0.04 ms | **~80×** |
| body 派生量 | 6 次全扫 × 50 creep × 50 part = 15,000 次部件访问 ≈ 0.73 ms | 缓存读 ≈ 50 次 | ~300×（该层） |
| 类型分发 | 29 × 400 = 11,600 次字符串比较 ≈ 0.23 ms | 枚举 match ≈ 0 | ∞ |
| 衰减类 tick（200 路 + 100 rampart） | 每对象每 tick 判一次到期 | 到期堆，O(到期数)（≈0-2/tick） | ~150×（该层） |
| 历史写放大 | 每 tick 全量 100-400KB × 20 份 + 每 20 tick 重新 parse 20 份 + 19 次全字段深 diff | 1 全量 + 19 份字段级 delta | ~20×（流量）+ 消灭 parse/diff |
| eventLog/mapView | 每 tick 每 2 次无条件 RPC 写（含空 eventLog、静态 mapView） | 脏标记，空闲 tick 0 写 | 空闲房间 ∞ |
| 房间读入 | 每房 ~7 个 JSON-RPC 往返 + 全量 parse | 进程内 blob 直读 | 消灭 |
| global 阶段 | 每 tick 全世界 terminal/order 装载 + 全量 order 重算 | 脏集合驱动 | 依世界规模 10-100× |

综合：一个活跃房间的 processor 侧热路径（扫描 + 重算 + 分发，不含 RPC/DB）实测 ≈ **5-6 ms**；上述改造后同负载的等价计算量 ≈ **0.1-0.2 ms 量级**，再叠加 Rust 无 GC/无 lodash 回调开销。100 活跃房间的私服，仅处理端扫描层就从 ~0.4 s/tick 降到 ~10 ms 量级（单线程等价），tick 瓶颈将完全转移出 processor。

---

## 10. 相对上一轮报告的增量点清单

上一轮（§1.6 表）只笼统记了一条"processor 侧无空间索引，O(n) 全对象扫描"。本轮新增的增量发现：

1. portal 检查是**每个 creep 每 tick** 的全表扫描（`creeps/tick.js:81`），与移动无关也照扫——单房最大单项之一。
2. 同一 creep 每 tick ≥6 次 body 全扫（movement.check rates / execute 疲劳 / tick move power / 各攻击 intent），可脏标记缓存。
3. `currentHistoryPromise` 进程级串行链（`processor.js:11,524`）——历史写是隐藏串行点，多房间下互相阻塞。
4. historyChunkSize=20 的 20× 写放大 + upload 时 20 份全量重新 parse + getDiff 深比较（`driver/lib/history.js:11-43`）。
5. eventLog 空也写、mapView 静态层每 tick 重写（`processor.js:493,497`）。
6. NPC AI 每 invader 一次全表收集 + 每次寻路重建 CostMatrix + 最多 4 轮搜索（§4），NPC 房成本被放大。
7. 衰减类结构 tick 可用到期堆增量化（§6.4）。
8. `getInterRoom` 每 tick 全世界 terminal/order 扫描 + market 全量 order 重算（§8）+ `_.shuffle` 非确定性。
9. 类型 if 链与两次全对象 forEach 的 SoA/依赖 DAG 改造路径（§6），包括顺序承载的语义依赖示例（movement.execute 先于 road tick）。
10. 基准数据（§0）把"扫描层 3.7 ms → 0.043 ms"落到实测，支撑 Rust 收益量化。
