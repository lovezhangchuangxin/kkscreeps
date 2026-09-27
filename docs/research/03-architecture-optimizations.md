# 03 · 架构优化深挖（第二轮）：01 报告之外的新发现

> 调研日期：2026-09-27
> 定位：`01-screeps-rust-feasibility.md`（宏观可行性）之后的**第二轮架构深挖**。输入材料：五份深挖笔记（`notes/arch-processor.md`、`arch-runner.md`、`arch-xxscreeps.md`、`arch-storage-backend.md`、`arch-community.md`）及其 topOptimizations 汇总。01 已覆盖的宏观点（JSON 序列化层的存在、进程拓扑、全局屏障、每 tick 全量快照的存在）不再重复，本文给出的是**逐条 file:line 级热路径、实测数字与 Rust 对应做法**。
> 结论先行：**本轮把 01 报告"架构重构收益 10x"的判断从推理升级为实测锚定，且未发现动摇该结论的证据。** 三个最大单项：(a) processor 按坐标查询层实测 80× 优化空间【实测】；(b) runner 每用户每 tick 固定开销合计 ≈10–25ms，其中 ≈10–20ms 落在玩家 CPU 计量窗口之外、属纯服务器净损失（构成推导与计费窗口分解见 §2.0；合计区间为【推断】，分量单价部分实测、部分估计）；(c) 私服 storage 是"单一 env 文档 + lokijs 每 10 秒整库 JSON 序列化"（机制为 npm 解包实证），按 100 用户/50 活跃房的**假设规模**推算 env 热数据 ≈40–110MB、每 10s 全量重写即 ≈130ms–1.1s 单核 stringify【推断——机制实测、总量非实测】。另有一条**架构级防错结论**：玩家字段读绝不能走"每次属性读回调宿主"路线，否则比官方现状慢 2–4 个数量级【推断——单次调用单价未实测，但结论对单价误差不敏感，且官方自身两处热路径先例佐证，完整证据链见 §2.6】。

**置信级别**（全文适用）：
- 【实测】= 本轮调研在真实源码 / 本机基准实测（Node v24.11.0，Apple Silicon；命令与输出记录于对应笔记的 §0 段），或对闭源 npm 包解包实证；
- 【文档】= 官方 blog/forum/docs/Steam 公告或作者（laverdet）一手陈述；
- 【推断】= 由实测线性外推或架构推理，未直接测量。

**优先级标尺**：P0 = 架构定型期必须做/必须决策（影响 crate 内部表示、正确性或安全边界，后改成本高）；P1 = 高收益，按里程碑落地；P2 = 锦上添花 / 兼容优化。

**引用抽查说明**：撰写人本轮对笔记中最关键的 8 处 file:line 逐一到源码核对，全部吻合（清单见附录 A）；其余行号按笔记原样引用，未逐一复核。

---

## 0. 跨主题 P0 清单（架构定型期必须处理）

| # | 事项 | 主题 | 一句话理由 |
|---|---|---|---|
| P0-1 | 房间状态 SoA + 2500 格空间索引为 kk-world 第一公民 | §1.1 | 实测 80×，且决定内部数据表示，后改成本极高 |
| P0-2 | per-room blob + SAB 直传 + 增量连接器，取代整包 structured clone | §2.1 | 每用户每 tick 5–10ms【估计】×用户数的最大单项；决定 runner/engine 接口 |
| P0-3 | RoomObject 访问路径写成硬约束：SAB 字节 + 沙箱内 getter，低频操作走常驻原生模块 | §2.6 | 防错结论——选错路线整体劣化 100 倍以上 |
| P0-4 | 存储分层：env 键空间分表（redb/libmdbx），scratch 编排层改进程内原子计数器 + channel | §4.2、§3.1 | 私服最大写热点与最大 IPC 热点，且属存储 trait 定型 |
| P0-5 | 流水线语义：per-room 计数器触发 run/process 重叠 + 确定性房间序（替代全局屏障/LIFO 队列） | §4.5 | tick 时长从 Σ 串行变 max(单房间)；同时是 replay 正确性前提 |
| P0-6 | CPU 计费与隔离内建于沙箱边界（intent 常量表照搬） | §5.2 | 经济兼容 + 根除 CPU stealing 类漏洞；属安全边界 |
| P0-7 | screeps-ok adapter 从 M1 起进 CI 持续跑 parity | §5.1 | 2897 个 vanilla 基准用例是重写期唯一可回归的行为护栏 |

---

## 1. 引擎处理端热路径（processor / intents）

基准锚点【实测】：开发完成的 50-creep 房间（N=400 对象，350 结构 + 50 个 50-part creep），仅"按坐标找对象"一层每房每 tick ≈3.7ms 纯扫描；等价查询用一次性格子索引 0.043ms（笔记 arch-processor.md §0，命令在笔记内）。活跃房 processor 侧热路径（扫描+重算+分发，不含 RPC/DB）实测 ≈5–6ms/房/tick。

### 1.1 按坐标查询统一替换为 2500 格空间索引【P0】

- **现状证据**：四处 O(N) 全表扫描叠加——move intent 目标格 `_.filter`（`engine/src/processor/intents/creeps/move.js:36`）；障碍判定 `_.forEach(roomObjects)`（`engine/src/processor/intents/movement.js:16-39`，本次抽查核对：`checkObstacleAtXY` 对全表按 x/y 过滤，且 `_.contains(C.OBSTACLE_OBJECT_TYPES, i.type)` 数组线性查）；落地格对象（`movement.js:202`）；portal 检查（`processor/intents/creeps/tick.js:81`，本次抽查核对：带 `ageTime` 的 creep——即几乎所有普通 creep——每 tick 全表 `_.find` portal，**不移动也查**；powerCreep 同款 `power-creeps/tick.js:44`）。50 creep 全动 = 4×50×400 = 8 万次谓词回调/房/tick。另有 56 处 `roomObjects` 线性扫描清单（笔记 §3，grep 全量核对）：攻击找 rampart、rangedMassAttack 的 O(N+T×N)、withdraw/build/harvest 同格查找、spawns/tick 每 spawn 全表 filter 等，全部是"按 (x,y) 找同格对象"或"按 type 收集"两种形态。
- **Rust 方案**：`Vec<SmallVec<[ObjectId; 4]>>` 长度 2500 的格子索引，房间加载时一次 O(N) 构建，movement（唯一批量改坐标处）增量维护；叠加"静态障碍结构位图 + 2500 字节地形位图"，`checkObstacleAtXY` 退化为 2 次位测试 + 格内小循环。"按 type 收集"由 §1.6 的类型分桶消灭。
- **预期收益**：【实测】3.7ms → 0.043ms（≈80×）；100 活跃房私服仅此层从 ~0.4s/tick 降到 ~10ms 量级（单线程等价）。
- **注意**：格内对象数 k≈1–4，索引查询 O(k)；对象增删/移动的维护成本由 movement 仲裁点集中承担。

### 1.2 creep body 派生量改为受击/store 变化时失效的缓存【P1】

- **现状证据**：同一 creep 每 tick ≥6 次 50-part body 全扫——movement.check 争议格 rates 3 次（`movement.js:116-122`，调 `calcBodyEffectiveness`/`_.filter(body)`/`calcResourcesWeight`，`engine/src/utils.js:623-636`）+ movement.execute 疲劳 2 次（`movement.js:236-239`）+ tick move power 1 次（`processor/intents/creeps/tick.js:105`）。
- **Rust 方案**：`move_power: u16`、`fatigue_base: u16`（非 MOVE/CARRY 部件数）、`carry_weight`（随 store 变化）作为 blob 内缓存字段，受击（`processor/intents/creeps/_recalc-body.js` 路径）与 store 变化时失效；boost 表查找一并消失。
- **预期收益**：【实测】50 creep×6 扫×50 part = 1.5 万次部件访问（实测 rates 0.322ms + 疲劳 0.384ms + move power 0.025ms）→ ≈50 次 O(1) 读，该层 ~300×。

### 1.3 history 写链路改为 tick 内增量 delta【P1】

- **现状证据**：每活跃房每 tick 全量 `JSON.stringify` 快照（`engine/src/processor.js:532`，本次抽查核对；实测简化房间单快照即 100,588 字节）写进 env；每 `historyChunkSize=20` tick（`backend-npm/lib/game/server.js:39`）`upload()` 把 20 份全量快照全部 `JSON.parse` 回来 + `getDiff` 全字段深比较算 19 个 diff（`driver/lib/history.js:11-43`）；且 `currentHistoryPromise` 是**进程级单链**（`processor.js:11,524-534`，本次抽查核对），该 processor 进程内所有房间的历史写严格串行——隐藏串行点。
- **Rust 方案**：processor 结算时天然产出变更集（bulk ops）→ 直接产出本 tick 字段级 delta（rkyv/postcard），20 tick 合并为"1 全量基线 + 19 delta"；房间间历史写并行。
- **预期收益**：【实测+推断】每房每 20 tick 写 ~2MB → ~100KB + delta（经验 <10%），流量 ~20×↓；消灭 20×100KB 重新 parse + 19 次深比较；串行链解除。与 §4.3（默认私服该结果全被丢弃）合并看，是"白烧"路径。

### 1.4 eventLog / mapView 写加脏标记与静态层版本化【P1】

- **现状证据**：`saveRoomEventLog` eventLog 为空也写（`processor.js:497` → `driver/lib/index.js:583-585`，`JSON.stringify([])` + hset RPC）；`mapViewSave` 每房每 tick 全量重建 ~400 次 push + JSON.stringify + env.set（`processor.js:328-337,442-481,493` → `driver/lib/index.js:530-532`），其中墙/路/矿/控制器坐标等静态层只在结构增删时才变。
- **Rust 方案**：eventLog 产生点直接 append ring buffer，空 tick 零写；mapView 拆静态层（结构版本号触发重算）+ 动态层（仅坐标变化）。
- **预期收益**：【实测·源码】空闲/低活动房间该层降为 0 写（现状每房每 tick 无条件 2 次 env 写 RPC）。

### 1.5 NPC AI 共享敌我分类 + CostMatrix 版本化【P1】

- **现状证据**：每个 invader 各做一次全表收集（`processor/intents/creeps/invaders/pretick.js:17-40`；keepers 同款 `keepers/pretick.js:23`）；每个不带 heal 的 invader 依次尝试最多 4 轮 `findClosestByPath`+`moveTo`，失败分支还对每个 spawn 各跑一次 `checkPath` 全寻路（`invaders/findAttack.js:44-86`）；`fake-runtime.findPath` 每次搜索重建 2500 字节 CostMatrix（`processor/common/fake-runtime.js:104-141`）；`findClosestByPath` 把每个候选目标都变 goal（`fake-runtime.js:283`）。
- **Rust 方案**：一次全表敌我分类供全部 NPC 复用；CostMatrix 基底（静态结构+地形）按房间版本化缓存，结构增删才重建；creep 位置层用可复用 overlay（2500 字节 memcpy 后盖 creep 占用）；`reusePath` 语义官方已有（`fake-runtime.js:204-238`），保留。
- **预期收益**：【实测】10 次矩阵构建 0.334ms@N=400；5-invader 房现状 ≈5 次全表收集 + 最多 20–30 次寻路×每次全表矩阵 → 矩阵重建仅结构增删时发生，NPC 房 processor 成本降一个量级【推断】。

### 1.6 结算循环类型分桶（SoA）+ 依赖 DAG + 衰减类到期堆【P1】

- **现状证据**：29 个 `if (object.type == ...)` 类型链 × 两遍全对象 forEach（预处理 `processor.js:43-171` + 结算 `processor.js:342-419`）；`driver.config.emit` 在热路径 per-object 至少 4 次（`processor.js:169,246,318,482`）；实测 29×400 字符串比较 0.23ms@N=400。**顺序承载真实语义**：movement.execute 扣道路磨损（`movement.js:213-221`）必须先于 road 衰减 tick（`processor/intents/roads/tick.js:10-31`）——不能盲目并行。roads/ramparts/containers 的 tick 全是 `nextDecayTime` 判定。
- **Rust 方案**：加载时按 `enum GameObject` 分桶到 per-type `Vec`，枚举 match 取代字符串比较；依赖 DAG 调度（xxscreeps 的"注册式 processor + 拓扑排序 + 冲突位掩码"已验证该形态）；衰减类合并为 BinaryHeap by nextDecayTime——O(到期数) 而非 O(类型总数)；字段不相交的类型组房间内 rayon 并行，确定性靠固定迭代序（保 replay）。
- **预期收益**：【实测】类型分发 11,600 次字符串比较 → 0；【推断】RCL8 房 200 路+100 rampart 每 tick 遍历变为 O(到期数)（平均 <2/tick，该层 ~150×）。

### 1.7 global 跨房结算改脏集合驱动 + 种子化 PRNG【P1】

- **现状证据**：`getInterRoom()` 每 tick 从 DB 拉全部 normal 房间 + **全世界所有 terminal/powerSpawn/powerCreep** + 全部 market orders + 全部 users.intents（`driver/lib/index.js:429-449`），再 `_.indexBy/_.groupBy`（`engine/src/processor/global.js:28-29,45-49`）；market 每 tick 对每条 order 全量重算 active/amount（`processor/global-intents/market.js:497-589`）；`directDeals = _.shuffle(...)` 用 `Math.random`（`market.js:400`）——跨实例/replay 不可复现。`terminalDeals.sort` 比较器里反复 `roomNameToXY`（parseInt 字符串解析，`market.js:289`、`utils.js:374-392`）。
- **Rust 方案**：terminal/powerCreep/order 维护内存注册表 + 变更事件入脏集合，每 tick 只处理脏集合 + 有 intent 的条目；PRNG 全局种子化；房间名在世界加载时预解析为 (i16,i16)。
- **预期收益**：【推断】O(全部 order) → O(活跃 order)，私服百房规模即数百 KB JSON 流/tick 消失；MMO 规模数千 terminal 全表扫同理。shuffle 修复是 replay 正确性问题而非性能问题。

---

## 2. 玩家运行时开销（runner / 沙箱物化）

**本主题的奠基性发现**【实测】：每 tick 的世界物化成本（constants 深拷贝、spatial/findCache/wrapper 构建、整包 clone）发生在玩家 CPU 计量窗口**之外**——`game.init` 在 `runtime.js:216` 执行，玩家计时的 `startTime` 在 `runtime.js:235` 才取。即这些是**纯服务器成本，随世界规模与用户数线性增长，却在任何 CPU 统计里都看不见**。Rust 消灭的每一毫秒都是净收益，不改变玩家体感公平性（Memory round-trip 除外，见 §2.2）。

与官方 runner/processor 每小时强制重启（`launcher/lib/start.js:139-141`；storage 笔记 §5 判定为官方对长跑内存问题的默认缓解）的关系需要说清：**两者是同一根源的两个症状，不是同一命题的互证**——每用户每 tick 的全量物化（wrapper/spatial/register 对象图）既是窗口外 CPU 成本、也是主要内存分配源，其累积正是重启所"回收"的压力；而"窗口外"属性使 CPU 侧问题不出现在任何统计中，官方只能靠重启这类粗粒度手段兜底。此因果链为【推断】——官方未公开归因，且重启所缓解的内存问题还包括 q promise 链、lokijs/env 文档膨胀等独立来源（storage 笔记 §5）。

### 2.0 每用户每 tick 固定开销：构成推导与计费窗口

推导基准（arch-runner 笔记 §0/§9 锚点场景）：100 活跃用户、每用户 5 可见房间、500 单市场簿、106KB Memory、世界 400 活跃房间。

| 分量 | 量级 | 置信级别 | 计费窗口 |
|---|---|---|---|
| 20 次 RPC 拼装（§2.5） | 2–6ms | 【推断】（0.1–0.3ms/次为 JSON 帧 RPC 经验单价，未逐次实测） | 窗口外 |
| 整包 ExternalCopy（§2.1） | 5–10ms | 【估计】（500 单订单簿 JSON round-trip 实测 ≈4.0ms；ExternalCopy 同量级或更高为推断） | 窗口外 |
| constants JSON round-trip（§2.3） | 1.65ms | 【实测】（2000 次均值） | 窗口外 |
| spatial/findCache/wrapper 重建（§2.4） | ≥0.7ms | 【实测单价·下限】（5 房间×146µs 纯分配；~2000 次 wrapper 构造与 GC 未计价） | 窗口外 |
| 结果写回（update/pubsub/env.set） | 1–2ms | 【推断】 | 窗口外 |
| **窗口外小计** | **≈10–20ms** | 【推断】 | **服务器净损失** |
| Memory round-trip（§2.2） | 9.2ms@106KB（2MB 外推 ≈180ms） | 【实测】 | **窗口内（计玩家 CPU）** |
| market 访问侧 clone（§2.7） | 0–4ms 视用法 | 【实测单价】 | 窗口内（玩家调用时） |
| **合计** | 小 Memory/小世界用户 ~10ms；106KB Memory 且读市场的 bot ≈20–30ms | 【推断】 | — |

说明：arch-runner 笔记 §9 原表合计"≈10–25ms（世界越大越高）"与本推导相容——下限对应小 Memory/小世界用户，上限对应上表锚点场景。本报告凡引用"10–25ms"区间处一律按【推断】对待（分量单价部分实测）。

### 2.1 消灭"runtimeData 整包 structured clone 进沙箱"【P0】

- **现状证据**：`driver/lib/runtime/make.js:144-160`——每用户每 tick `new ivm.ExternalCopy(data)` 整包深拷贝进 isolate。包内含：可见房间全部对象原始文档、**整本市场订单簿 + 14 天历史 + 玩家挂单**（`driver/lib/runtime/data.js:243-247`）、**200 条交易**（`data.js:213-216`，2×100 条 sort+limit）、每房间事件日志（`data.js:219`）——**从不碰市场的玩家每 tick 也在为这一切做深拷贝**。
- **Rust 方案**：per-room blob（rkyv）双缓冲 + SharedArrayBuffer 直传（官方地形已是该模式：`make.js:20-51` SAB + 每房间 Uint8Array 视图——官方自己证明了可行性，只是只用于地形）；market/用户名/交易/事件日志走增量连接器——xxscreeps 已验证该形态可照抄：每 tick 只读可见房间 blob（`xxscreeps engine/runner/instance.ts:222-248`）、订单只传 `changedThisTick ∪ 未见过`（`mods/mmo/wallstreet/driver.ts:207-226`）、`seenUsers` 用户名跨 tick 缓存。
- **预期收益**：【实测单价+估计】500 单订单簿 JSON round-trip ≈4.0ms（实测）；ExternalCopy 同量级或更高（推断）；加历史/交易/事件日志估计每用户每 tick 5–10ms 且与世界规模正相关（估计）；100 用户即 0.5–1s/tick——与官方 tick 2–5s 同量级，是其直接构成【推断】。增量化后趋近 0。
- **注意**：这决定 kk-runner ↔ kk-engine 的接口形态（blob + SAB），属 P0 定型项。

### 2.2 Memory round-trip 三层增量 + 计费决策点【P1，含显式决策点】

- **现状证据**：三重浪费——存储→宿主 `env.get`（`data.js:132`）→ 随整包 clone 进沙箱 → tick 内首触全量 `JSON.parse`（`engine/src/game/game.js:479-500`）→ tick 末全量 `JSON.stringify`（`driver/lib/runtime/runtime.js:246-248`）→ ExternalCopy 出沙箱（`runtime.js:276,296`）→ `env.set` 写回（`driver/lib/index.js:153-160`）。【文档】官方明文承认 parse 计费给玩家（docs.screeps.com/global-objects.html）。xxscreeps Memhack 已做：整体惰性 parse、跨 tick `previousJson` 复用、UTF-16 SAB 零拷贝（`mods/meta/memory/memory.ts:268-283,315-326`）——但**只读 tick 仍付全量 stringify**（`_parsed` 被设置过 flush 就整体 crunch+stringify）。
- **Rust 方案**（在 Memhack 语义之上再加三层）：(a) 按顶层键（creeps/rooms/…约 5 个）惰性 parse，宿主只做字节区间扫描（memchr 级，比 JSON.parse 快一个量级且零分配）；(b) 写屏障脏键标记，flush 只重序列化被写键、未写键复用上一 tick 字符串切片；(c) 字节常驻宿主 SAB arena，进出沙箱与写回全零拷贝。
- **预期收益**：【实测】106KB round-trip ≈9.2ms，线性外推 2MB ≈180ms/tick（上限用户）；只读 tick → 0（与 Memhack 持平），写 tick → O(脏键)（典型 bot 每 tick 只写几个键）。
- **决策点**：parse/stringify 计入玩家 CPU（官方语义【文档】docs.screeps.com/global-objects.html）。玩家侧参照：知名资深玩家 Dissi 在官方 2017 优化路线图帖回帖称 "It could save the CPU used on the Memory serialization, which is about 4% of my overal script times (higher in global reset ticks)"（screeps.com/forum/topic/85，本次修订时抓取原帖核实；帖内**未披露** bot 规模与 Memory 大小，适用范围未知——仅作量级参照，不作决策主依据）。Rust 宿主可移出计费窗口（宿主付费）——大 Memory 用户相对受益个位数百分比，属公平性变更，需显式决策；保守做法是仍计费但费用因增量化降至增量级。

### 2.3 constants 每用户每 tick 深拷贝直接消灭【P1】

- **现状证据**：`engine/src/game/game.js:435`（本次抽查核对）：`_.extend(globals, jsonParse(jsonStringify(C)))` 每用户每 tick；而 isolate 创建时已一次性注入干净常量（`driver/lib/runtime/user-vm.js:65`），每 tick round-trip 纯粹是防上一 tick 玩家篡改污染。V8 snapshot 机制已存在（`user-vm.js:151-158` 读 runtime.snapshot.bin）。
- **Rust 方案**：整棵 C 放进 isolate 快照并递归 `Object.freeze`（`system-functions.js:20-28` 已有 freezeDeep），每 tick 直接复用引用。兜底：每 isolate 一次 pristine 深拷贝 + 每 tick 浅重绑（423 次属性赋值，µs 级）。
- **预期收益**：【实测】C 序列化 23,332 字节/423 顶层键，round-trip ≈1.65ms/次（2000 次均值）；100 用户即 165ms/tick 纯常量拷贝 → 0。且该成本在玩家 CPU 窗口外，纯服务器净收益。
- **注意**：语义差异需记录——冻结后玩家改 C 从"静默复位"变为"静默失败/抛错"；无官方文档把"可变 C"当特性，screeps-ok 若有对应用例再降级兜底方案。

### 2.4 find/look/spatial 索引与 wrapper：每房间一次构建、观众共享、按访问物化【P0】

- **现状证据**：每用户每可见房间建 ~15×2500 槽 spatial 数组（`game.js:104-110` + 插入 `game.js:41-47`，实测纯分配 ≈146µs/房间）+ **全部对象无条件实例化 JS wrapper 并预填充 findCache**（`game.js:260-395`，开发完成房间 ~400 对象）+ 每房间 13+2 张 lookTypeRegisters（`engine/src/game/rooms.js:485-514`）。N 用户同房间 = N 份独立重建。
- **Rust 方案**：processor 每房间每 tick 构建一次（Rust µs 级），全部观众共享——blob 内 slot 排布即索引；wrapper = `(blob 视图, slot 偏移)` 薄壳，玩家真正读属性才物化（xxscreeps overlay 语义：整 tick 没碰 `creep.body` 就不进 V8 堆）；FIND_MY_* 等用户相关过滤在共享索引上 O(命中数) 现算。跨 tick 持久化仅作增量修补的锦上添花（processor 唯一写者，diff 免费）。
- **预期收益**：【实测+推断】重建次数从 Σ(用户×可见房间)（100 用户×5 房=500 次/tick）降到 Σ活跃房间次；GC 压力随惰性物化大幅下降【文档·作者声称】。

### 2.5 data.js 的每用户每 tick ~20 次 RPC 拼装改增量/合并【P1】

- **现状证据**：`driver/lib/runtime/data.js:99-264` 两批共 ~20 查询（两份笔记分别计 19 与 20，差异在第二批计数口径，量级结论一致）：user 文档双取（`data.js:130` 重复于 `make.js:55`）、`rooms.objects` 对全世界最大集合两次扫描（`data.js:103,136`）、gameTime 专门一次 RPC（`data.js:134`；官方 users 队列只传 userId 字符串，`engine/src/runner.js:68-71`）、console 轮询 + 空 `removeWhere` 无条件写（`data.js:133,149`）、transactions 200 条急切取数（`data.js:138-139`）、roomEventLog 急切取数（`data.js:141`）。每帧 RPC = 4 字节长度前缀 + JSON 帧（`common/lib/rpc.js:26-53`）。
- **Rust 方案**：per-user 可见集索引（processor 写时维护 `userId → 可见房间/拥有对象`）；gameTime 随队列任务下发；user 文档取一次；console 改 pubsub 推送；transactions/事件日志惰性取数（同进程同步宿主调用无 RPC 惩罚）；users.code 按 `(userId, codeTimestamp)` 缓存。
- **预期收益**：【推断，单价有实测支撑】按 0.1–0.3ms/RPC 估，每用户每 tick 省 2–6ms 纯管道成本，100 用户 0.2–0.6s/tick；`rooms.objects` 全集合扫描是 MMO DB 单点饱和主因之一（§5.3 官方危机史），索引化后该类查询消失。

### 2.6 RoomObject 访问路径定型【P0·架构级防错结论】

- **现状证据**：官方形态 = 闭包 `data(id)` + JS 对象属性（`engine/src/utils.js:509-545` eval 生成 getter + per-instance 缓存；`engine/src/game/creeps.js:50-58`），字段读为沙箱内 JS 属性访问，ns 级。
- **"慢 2–4 个数量级"的完整证据链**：
  1. 单次跨界调用成本（deno_core op / ivm Reference.apply）≈1–10µs 为【推断·未实测】——机制依据：跨界调用需参数序列化/克隆（serde_v8 或结构化克隆）+ 两次运行时切换，与官方 JSON-RPC 帧实测单价（0.1–0.3ms/次，含 socket 往返，见 §2.5）同源而更轻。旁证：deno_core 专门提供 `#[op2(fast)]` 走 V8 Fast API 绕过序列化（本轮 spike 笔记 `notes/sandbox-denocore.md` 记载）——deno 自身为常规 op 的序列化成本另辟快通道，恰说明常规路径有价。
  2. **结论对单价误差不敏感**：玩家每 tick 10^4–10^6 次字段读，即使按最乐观的 1µs/次计也需 10ms–1s/tick，相对官方 ns 级现状整体劣化 10^2 以上；单价再偏差一个数量级不改变方向性结论。
  3. **官方反例（官方自己在两条热路径上避开了"每次访问回调宿主"）**：① 地形不走逐次宿主回调，而是 SAB + 沙箱内每房间 `Uint8Array` 视图直读（`make.js:45-49`）；② 寻路不走宿主 RPC，而是把 C++ 编成常驻沙箱的 NativeModule（`user-vm.js:7,64`、`runtime-driver.js:124-125`），CostMatrix 以 Uint8Array 零拷贝直传。
  4. SAB 数据面（零拷贝、宿主与多 isolate 共享同一份内存）已在 deno_core spike 实测确认（`notes/sandbox-denocore.md` §SAB 实测输出 "one allocation shared by 2 isolates + host"）。
- **Rust 方案（硬约束）**：数据走 SAB 零拷贝字节；字段读一律沙箱内原型 getter 读 typed array（编译期偏移）；**绝不走"每次属性读回调宿主"**；低频操作（PathFinder、getOrderById、getObjectById）走常驻原生模块（即官方反例②的形态）；Store 派生对象按访问物化，消灭 Proxy + `C.RESOURCES_ALL.indexOf` 线性扫描（`engine/src/game/store.js:87-96`）。
- **预期收益**：【推断】字段读保持官方同量级、未触碰字段零成本；避免误选路线导致的整体劣化（量级见证据链第 2 条）。此条应写进 kk-runner 设计契约。

### 2.7 market 访问侧深拷贝消除【P2】

- **现状证据**：`_getOrders` 整桶 `JSON.parse(JSON.stringify())` + `price /= 1000` 原地改（`engine/src/game/market.js:21-24`，price 定点化逼出拷贝）；`getHistory` 同款（`:50`）；`getOrderById` 单条也 clone（`:57,61`）；`getAllOrders` 先整桶 clone 再 filter（`:36-39`）；而 `deal()` 内部直接读不拷（`:109`）——证明拷贝仅为防篡改视图，不影响结算。
- **Rust 方案**：订单整数毫信用存储 + getter 换算；getAllOrders 迭代时逐个物化；getOrderById 走 blob O(1) 查找（xxscreeps `order.ts:74-78` 已验证 `readOrder(blob)` 无拷贝）。
- **预期收益**：【实测】500 单访问侧 clone ≈4.0ms/次 → 只为 filter 命中项分配；配合 §2.1，从不读市场的玩家市场成本为 0。

---

## 3. xxscreeps 增量设计：可径直吸收的决策与必须重做的部分

### 3.1 scratch 编排层内联为进程内原子操作【P0，Rust 单进程自然获得】

- **现状证据**：run/process 重叠靠 scratch KV 的 zset 原子计数——runner 每发布一房 intents `ZINCRBY -1`（`xxscreeps engine/processor/model.ts:107-131`）；每 tick 主循环六连 zset 操作（`model.ts:167-202`）；队列消费逐成员 `sPop`/Lua pop（`engine/db/async.ts:6-15,46-80`）；多进程时每次操作是 unix socket + v8 结构化克隆往返（`engine/db/storage/local/responder.ts:13-30`）。作者两次专门为该层打补丁（git `452e55cf` 批量探测、`85926e9c` 微批），佐证其成本。
- **Rust 方案**：单进程内 `AtomicI32` fetch_sub 归零 + channel 投递 room task；语义（房间在全部玩家完成后即可处理）1:1 保留。
- **预期收益**：【推断】xxscreeps 每 tick 每房间 10+ 次 scratch 操作、万房间量级 ~10 万次 IPC/tick → 0（进程内原子操作）。

### 3.2 intents 传输换编译期二进制（postcard/rkyv）【P1】

- **现状证据**：xxscreeps 残留的最后一段 JSON 热点——runner 侧 `JSON.stringify({userId, intents})` rPush、processor 侧 `lRange + vDel + JSON.parse`（`engine/processor/model.ts:91,146-156`）。
- **Rust 方案**：intent 枚举 + serde 二进制。
- **预期收益**：【实测·引用】rkyv vs JSON 快 ~14x（01 报告 §3 的 rust_serialization_benchmark），另消除字符串分配的 GC 压力。

### 3.3 blob schema 布局与版本计算移到编译期【P1】

- **现状证据**：xxscreeps 每个服务启动都要运行时 getLayout/build + SHA-1 版本计算（`schema/build.ts:21-28`）+ 归档源码存 DB（`engine/schema/build/index.ts:16-17,32-60`）+ 升级时 `restoreLayout` 重编译旧 reader（`:66-92`）；作者 2020–2021 两次专提削减此开销（git `04978d6b`、`d8038e54`）。已知脆点：issue #294 kMagic 变更炸全部旧 blob。
- **Rust 方案**：rkyv 声明宏 + 构建脚本生成版本常量，运行时只保留透明升级器（读侧仅一次 u32 比较的机制照抄；升级器只留 materialize→rewrite 一条路径）。
- **预期收益**：【实测·源码】全部启动开销与归档源码驻留消除；免停服迁移语义保留。

### 3.4 backend 房间推送改引擎侧结构感知增量 + 渲染产物缓存【P1】

- **现状证据**：xxscreeps 的共享 load 与 `didUpdate` 跳读机制良好（`backend/sockets/room.ts:45-53,100-104`，throttle 125ms 见 `config/config.ts:286`），但 diff 仍是**每订阅者** JS 递归逐键比较 + 每订阅者各跑一遍 `Render` + `JSON.stringify`（`room.ts:17-38,168-198`）。
- **Rust 方案**：diff 移到 blob 双缓冲层做字段级增量；渲染产物按 (room, tick) 缓存后按订阅者权限过滤扇出。
- **预期收益**：【推断】多观察者热门房间 CPU 从 O(订阅者×对象数) 降数倍（共享 load/didUpdate 部分已验证可行【文档·源码】）。

### 3.5 寻路 crate 直接移植已调优算法组合【P1】

- **现状证据**：作者 2026 年 5+ 提交调优出的结论——dial's heap（256 桶 + 溢出堆 + 惰性删除，`packages/pathfinder/src/heap.cc:93-198`）、`heuristicWeight==1` 切 A*（`src/pf.cc:256-264`，本次抽查核对，注释明说 JPS 在非均匀代价网格出次优路径）、拐点压缩 ABI（u32 打包坐标）、profile.ts 的 552 对固定兴趣点 + SHA-256 输出哈希回归 + criterion 式计时（`driver/pathfinder/profile.ts:15-27,95-127,205-228`）。git `c23bcb98/65a8ea9f/3d136abb/65a14182`。
- **Rust 方案**：算法组合与回归方法论整体移植为 Rust crate + 测试资产，保证行为逐位一致。
- **预期收益**：【推断】避免重踩 6 年调优弯路；输出哈希回归保证移植正确性可机器验证。

### 3.6 runner 沙箱不做跨进程迁移【P1】

- **现状证据**：xxscreeps 迁移 = 50ms 租约超时后新 runner 建全新 isolate、旧实例丢弃（`engine/service/runner.ts:74-91,100-104,121-127`；config 默认 50ms）；玩家付硬 reset 代价（global 丢失、code/memory 重载、跳一 tick，`engine/runner/instance.ts:172-198`）；每 isolate 都要 `compileScript` 一次 runtime bundle，无 V8 snapshot（`driver/sandbox/isolated/index.ts:64-117`）。
- **Rust 方案**：单进程 isolate 池；若多进程，一致性哈希固定归属（或持久租约）。
- **预期收益**：【推断】每次迁移省一次 isolate 全量重建 + 玩家一次 tick 损失；单进程形态天然无此成本。

### 3.7 构建策略：默认 LTO thin + codegen-units=1，PGO 仅作可选发布步骤【P2】

- **现状证据**：作者 2020 年实测 PGO "not actually any faster"（git `d6aa7676` 提交信息原文）；2026 年重新引入（`54c090f6/41614e49/b3682bd3`）更多是工程完备；CMake 全套开关见 `packages/pathfinder/CMakeLists.txt:41-61`。
- **Rust 方案**：确定性收益（LTO/单 codegen unit/内联 hint）做默认；PGO 用 criterion 负载训练列为可选项。
- **预期收益**：【文档】避免在低收益项上耗工期——作者 6 年两次 PGO 尝试是最直接的证据。

---

## 4. 存储与后端协议

### 4.1 增量订阅推送：变更集直发替代全量重读 + 每订阅者 diff【P1，backend 最大单项】

- **现状证据**：每次 `roomsDone`（每 tick）throttle 200ms 后，对每个订阅房间**无脏检查全量** `db['rooms.objects'].find({room})`（`backend-npm/lib/game/socket/rooms.js:22,54-58`），每订阅者各跑一遍 `getDiff` 深比较（`rooms.js:75`；`common/index.js:69-136`，O(对象×字段) `_.isEqual`），每连接缓存整房间快照（`rooms.js:95`）。**关键洞察：processor 明知本 tick 改了什么（bulk ops 即变更集，`driver/lib/bulk.js`），但存储 API 没有 change feed，官方把引擎已知的信息扔掉再花 CPU 买回来。**
- **Rust 方案**：`RoomChangeSet`（per-room 字段级 delta）由 processor 结算时发布一次，backend 按订阅者可见性过滤扇出；订阅端状态从 O(订阅×全量) 降为 O(订阅×delta)。
- **预期收益**：【实测·推算】50 订阅房×30KB×5 次/s ≈ 7.5MB/s RPC 传输 + 5×50 次/s 深比较 → 只在真变更时发 KB 级 delta，两项同时归零。

### 4.2 消灭 env 单文档 + lokijs 整库序列化【P0，私服最大写热点】

- **现状证据**【实测·npm 解包】：`@screeps/storage@5.1.3` 解包（/tmp/screeps-storage-pkg）——`lib/db.js:15` autosave 10s（本次抽查核对）；lokijs 每 10s `saveDatabase(this.serialize())` = **JSON.stringify(this) 整库序列化**（`xxscreeps/node_modules/lokijs/src/lokijs.js:2780,1334`）+ 全文件重写；**env 是单个 lokijs 文档**（`db.js:539-563`，全部 env get/set/hmset 读写 `get(1)` 这一份），任何 env 写把整文档标脏。env 键表（`common/lib/storage.js:33-52`）装着：每用户完整 Memory（≤2MB）、每活跃房 20 tick 全量历史快照、visuals、mapView、gameTime、activeRooms。
- **Rust 方案**：键空间分表——memory/history/visual/mapView 各归独立 KV 表（redb 多表或 libmdbx 多 dbi）或带 TTL 内存 slab；per-room blob 增量写（01 §4.2 方向的落地细化）。
- **预期收益**：机制为【实测·npm 解包】（env 单文档 + autosave 10s + 整库 JSON.stringify——`db.js:15` 本次抽查核对）；**总量为【推断·依赖假设规模】**，且两份笔记的历史快照单房大小锚点不一致，如实呈现：arch-storage 笔记按 30KB/房计（100 用户×100KB memory + 50 活跃房×30KB×20 tick ≈ 40–50MB）；arch-processor 笔记实测简化重房间（N=400）单快照 100,588 字节（≈100KB/房），按后者计历史项即 100MB、总量 ≈110MB。取区间 **40–110MB**（真实世界按房间对象构成落于两锚点之间），每 10s 全量 stringify（V8 吞吐按 100–300MB/s 估）≈ **130ms–1.1s 单核** + 全文件重写；增量写后降为 KB 级/变更。与论坛"私服 30 分钟 CPU 爬满"报告（topic/2940）现象吻合。

### 4.3 history 改事件溯源 + 补 replay API【P1】

- **现状证据**：官方同时付两种范式成本却都没用好——每 tick 全量快照写（`processor.js:522-536`）+ 每 20 tick 20 次 parse + 19 次 diff（`driver/lib/history.js:7-43`），而 `saveRoomHistory` 监听者**默认不存在**（全仓库 grep 仅 launcher 示例 mod `launcher/init_dist/example-mods/room-history.js:13`，且无保留策略）；`/room-history` 端点默认 `callback('not implemented')`（`backend-npm/lib/game/server.js:34-36`）——官方 standalone 无可用 replay API。
- **Rust 方案**：权威变更流（bulk ops 追加写 per-room 分段日志）+ 周期基线 blob（默认 20 tick 对齐官方 chunk）；对外保留官方 `{base, ticks}` JSON 结构从事件流现算；按 (room, tick) 段文件 + 双阈值 GC。
- **预期收益**：【实测·推算】写侧 30–100×（每 tick 全量 stringify → <1KB/房变更追加）；20 次 parse + 19 次 diff 全消（默认私服本就是白做）；replay 成为新实现的优势能力而非兼容负担。

### 4.4 休眠感知后台任务【P1】

- **现状证据**：`roomsForceUpdate` 每 20s 全量拉非活跃房间文档（`backend-npm/lib/cronjobs.js:178-192`，lokijs `$nin` 全扫），每房间每 90–110 游戏刻强制唤醒跑完整 room tick（9 次 RPC + 全量对象读写 + 快照写，`engine/src/processor.js:555-583`）——**官方架构没有真正的休眠房间**；genInvaders 每 5min 对每非活跃房串行 await 一次 RPC（`cronjobs.js:377-390`）。
- **Rust 方案**：时间戳离线结算（唤醒时按 Δt 一次性推进 sources 重生/衰减，xxscreeps 休眠房间思路）；后台扫描改索引下推批量（`WHERE next_event_time <= now LIMIT n`）；cron 统一到 gameTime 驱动的调度器。
- **预期收益**：【实测参数·推算结论】推导：每房每 90–110 游戏刻（`cronjobs.js:183-189`，均值 ≈100）被强制唤醒一次全量处理 → 每游戏刻平均唤醒 121/100 ≈ 1.2 个房间（121 为 `db.original.json` 解包实测的默认世界房间数）；私服 tick 下限 1s（`mainLoopMinDuration: 1000`，`driver/lib/index.js:21`）→ **空服 ≈1.2 房间 tick/s 底线负载**；10k 房世界同理 10000/100 = 100 房间 tick/s 纯维持成本。离线结算后空闲世界趋近零。

### 4.5 队列/流水线语义重设计【P0】

- **现状证据**【实测·解包】：官方 `queueFetch` 是挂起 RPC + 惊群（每次 add 唤醒全部等待者，`queue.js:27-35`）+ `pending.pop()` **LIFO 反序处理房间**（`queue.js:32`，本次抽查核对）+ `whenAllDone` 每次 markDone 唤醒重查（`queue.js:76-92`）；`smembers+del` 非原子丢唤醒（`driver/lib/index.js:170-174`）；main 双重 `whenAllDone` 全局屏障（`engine/src/main.js:35-55`）。每 tick 每用户/房间 2 次队列 RPC（fetch+markDone）随规模线性增长。
- **Rust 方案**：per-room 计数器触发 run/process 重叠（xxscreeps 已证语义可行）+ 原子 drain + **确定性房间序**（BTreeMap/索引序）。
- **预期收益**：【推断】tick 时长从"全部串行等待"（Σ玩家+Σ房间处理）变为受最慢房间约束（max 单房间 run+process），100 用户+50 房 ≈300 次/tick 队列 RPC 消失；免费获得 replay 所需固定处理序。**注意**：官方 LIFO 反序是行为事实——若 screeps-ok 对处理顺序敏感需对齐验证。

### 4.6 WS 传输层：二进制通道 + 文本协议兼容层【P2】

- **现状证据**：sockjs 文本帧，每消息单独 `zlib.deflate(level 1)` → `'gz:'+base64`（膨胀 33%，`backend-npm/lib/game/socket/server.js:45-63`，本次抽查核对）；cpu/console 等事件三重序列化（engine stringify → storage 转发 → backend parse 再 stringify，`driver/lib/runtime/make.js:262-273` + `backend-npm/lib/game/socket/user.js:35-37`）。
- **Rust 方案**：保留官方文本协议兼容层（renderer 生态依赖，握手 `time/protocol 14/auth/subscribe/gzip` 精确时序需复刻）；新增二进制通道（channel-id 字典 + tick 合帧 + rkyv delta）；事件结构体进程内传递只在出口序列化一次。
- **预期收益**：【推断】对 100 在线客户端每 tick 每人数 KB 推送，CPU 与带宽均约降一半以上（deflate level1 对重复键名 JSON 压缩有限，base64 又抵消部分）。

### 4.7 map-stats / getAllUsers 等读路径投影化【P2】

- **现状证据**：map-stats 每请求 2 次 `rooms.objects` `$in` 扫描后只 pick 两三个字段（`backend-npm/lib/game/api/game.js:186-257`）；main 每 tick `getAllUsers` 拉全部完整 user 文档（含徽章 SVG path，`driver/lib/index.js:144-151`、storage 包 `db.js:192-194`）只用 `_id` 排序。
- **Rust 方案**：per-room/per-user 轻量投影随 tick 更新，请求变字典取值。
- **预期收益**：【推断】开一次世界地图从对象级集合扫描 + 全量 JSON-RPC 传输 → O(请求房间数)；main 每 tick 全量用户文档传输（100 用户×数 KB）→ userId 列表迭代。

---

## 5. 社区与官方情报（工程护栏与经济兼容）

> 本主题多数优化点与 §1–§4 互洽交叉印证，只展开**新增工程决策**两项；其余以对照表收束。

### 5.1 screeps-ok 行为一致性套件作为重写期护栏【P0】

- **现状证据**【文档·源码】：screeps-ok（2026-04 创建、2026-09-27 仍在全量跑）34 个测试领域，vanilla 适配器 **2897 passing**（xxscreeps 2642 passing/77 expected-fail）；适配器契约 ~20 方法（`docs/adapter-spec.md:26-69`），不变量明确"不得重演游戏语义、不得掩盖引擎 bug"；附 8 个已定案 vanilla bug（含 engine#63/#121/#131/#148/#152/#156 上游 PR 引用）。
- **Rust 方案**：为 Rust 引擎实现 ScreepsOkAdapter（参照 xxscreeps 进程内适配器路径而非 vanilla 子进程路径），CI 持续跑 parity；8 个已知 bug 逐条"修正并声明分歧"。
- **预期收益**：把"行为等价"从 code review 变成可回归的量化门禁——重写期唯一的行为护栏，应尽早接入。

### 5.2 CPU 计费内建于沙箱边界【P0】

- **现状证据**【实测·源码核对】：intent 常量计费——0.2/intent、`say`/`pull` 免费、同对象同 intent 去重（`driver/lib/runtime/runtime.js:60-70`，本次抽查核对）；`getUsedCpu = 墙钟 + intents.cpu`（`runtime.js:203-205`）；bucket 10000/单 tick 500（docs.screeps.com/cpu-limit.html）。2024 年官方遭遇 "CPU stealing" 漏洞（窃取超额算力→封号潮→DoS 与代码泄露，forum topic/3285）。
- **Rust 方案**：计费做成单一受控模块（沙箱边界、引擎侧时钟），intent 计费表照搬保持 bot 经济完全兼容；不做散落 JS 约定。
- **预期收益**：玩家 CPU 预算语义不变（迁移期公平性前提）+ 根除 CPU stealing 类漏洞的架构土壤。

### 5.3 交叉印证表（社区/官方证据 ↔ 本报告条目）

| 官方/社区证据 | 级别 | 印证的条目 |
|---|---|---|
| Memory parse 计玩家 CPU（官方文档承认）；社区"Memory 是新手 CPU 大头"普遍经验；Dissi 自述约占其脚本 CPU 4%（forum topic/85 原帖本轮抓取核实；未披露 bot 规模/Memory 大小，仅量级参照） | 【文档】 | §2.2 |
| laverdet 点名 vanilla "processor 坐等全部玩家脚本"；xxscreeps 已验证 run/process 重叠拓扑 | 【文档·作者声称】 | §4.5 |
| 官方 2017 "最强数据库 100% 饱和"被迫分片；架构页"每对象一文档、24 核 128GB 扛 30k 更新/s" | 【文档】 | §2.5、§4.2——blob 化不是优化而是对官方两次架构危机根源的根治 |
| 官方 2016 寻路重写 C++ "drastically improves"；社区路径缓存后 2.0→0.4 CPU/creep（寻路≈移动 CPU 80%）；laverdet 2026-08/09 又重写 pf | 【文档·社区经验】 | §3.5 |
| 社区猴补 `isActive` 省 CPU 的战争故事；`Room.find` filter 重跑 + `_.clone`（`rooms.js:584-648`） | 【社区经验·源码】 | §2.4 |
| 官方 2016→2026 性能策略 = 换硬件（i9-9900K +15–30%）+ 空间分裂（shardX 2x tick）+ Node 24 升级，**从未动单进程引擎热路径与架构串行点** | 【文档】 | 全文——Rust 重写的差异化窗口就在这条未走的路 |

---

## 6. 对 01 报告架构方案的修订建议

深挖总体**强化**而非动摇 01 的结论（可行、10x 量级、风险集中在沙箱）。以下为具体修订：

**R1 · §1.6 热点表补三行（01 低估了 storage 进程内部与队列语义）**：① storage 私服实现 = env 单文档 + lokijs 10s 整库序列化（§4.2，本轮解包实证——01 只列了 RPC JSON 帧）；② 每用户每 tick 固定物化开销（constants/spatial/wrapper/整包 clone）在玩家 CPU 窗口外（§2 奠基发现，runtime.js:216 vs 235）；③ 队列 LIFO 反序 + 惊群 + 非原子丢唤醒（§4.5）。

**R2 · §4.1/§4.5 沙箱选型新增硬约束条款**：RoomObject 字段读不得跨沙箱回调宿主（慢 2–4 个数量级，§2.6）；数据走 SAB、行为走常驻原生模块；constants 用 V8 snapshot + 递归 freeze（§2.3）。kk-runner 的 trait 设计应把这三条写成设计契约，而非实现细节。

**R3 · §2.2 "xxscreeps 尚存问题"清单扩充**：本轮实证的具体残留——scratch 逐成员 RPC（§3.1）、intents 仍走 JSON（§3.2）、schema 运行时构建与归档驻留（§3.3）、backend per-subscriber diff（§3.4）、50ms 租约迁移 = 玩家硬 reset（§3.6）、Memory 只读 tick 仍全量 stringify（§2.2）、PGO 两次尝试 2020 无收益（§3.7）。结论：**"照抄 xxscreeps"不够**——Rust 版要超越的部分有了逐条清单与行号。

**R4 · §4.2 存储分层细化**：env 键空间按用途分表（memory/history/visual/mapView/gameTime/activeRooms 各归独立表）；scratch 层不是 KV 而是进程内易失结构（原子计数器 + channel + BTreeMap 唤醒队列）；跨机形态才落回 KeyVal/PubSub trait。

**R5 · §4.3 确定性总则落到具体命中点**：`market.js:400` 的 `Math.random` shuffle、storage 队列 LIFO 反序（`queue.js:32`）、backend 订阅序 `_.shuffle`（`rooms.js:26`）——三处都是 replay/跨实例不可复现源，固定序 + 种子化 PRNG 需在 kk-engine/kk-backend 显式落地，并过 screeps-ok 验证（处理顺序可能影响可见行为）。

**R6 · §7 路线图两处调整**：① screeps-ok adapter 从"验收阶段"提前到 **M1 起持续跑 parity**（2897 用例是渐进护栏，不是终检）；② kk-world 的内部表示第一公民 = SoA 分桶 + 格子索引 + blob 布局（§1.1/§1.6/§2.4）——它们决定所有 intent 实现的写法，M1 定型后不再改。寻路输出哈希回归资产（§3.5）同步进 M1。

**R7 · §5 收益量化从"推理"升级为"实测锚定"**：processor 扫描层 80×（实测）、每用户固定开销 ≈10–25ms → <1ms（构成推导见 §2.0：窗口外 ≈10–20ms【推断】+ Memory 9.2ms@106KB【实测】）、lokijs 每 10s 整库序列化 40–110MB@假设规模 → KB 级/变更（机制实测解包、总量推算）、历史写 20× 放大消灭（实测）。综合"CPU 约 1/5–1/10"的判断不变，且证据等级提高一档。

**R8 · 新增两个显式决策点登记**（01 未提，需项目维护者拍板）：① Memory parse/stringify 是否移出玩家 CPU 计费窗口（公平性 vs 服务器成本，§2.2；量化依据以官方文档确认计费 + 本轮实测绝对成本 9.2ms@106KB 为主，Dissi 的 ~4% 自述因测量条件未披露仅作社区参照，不作主依据）；② 玩家可变 `C`（constants）语义是否放弃（冻结方案，§2.3）。两者都建议先用保守方案 + screeps-ok 用例验证。

**R9 · 官方动向校正（补充而非矛盾）**：官方 2026-04 已完成 Node v10→v24 升级、shardX 2x tick——"官方栈停在旧运行时"的印象需更新；但其性能策略仍是空间换时间（新硬件/新 shard），单进程引擎热路径与架构串行点（全局屏障、JSON RPC、每对象一文档）从未动过。Rust 重写的差异化窗口定位在**架构层**，该判断不变。

---

## 附录 A：撰写人本轮引用抽查记录

对笔记关键引用逐一到源码核对（Read 工具，2026-09-27），8/8 吻合：

| 引用 | 核对结果 |
|---|---|
| `engine/src/processor/intents/movement.js:16-39` | ✓ `checkObstacleAtXY` 全表 `_.forEach` + `_.contains(C.OBSTACLE_OBJECT_TYPES,...)` 线性查 |
| `engine/src/processor.js:522-536` | ✓ `currentHistoryPromise` 串行链 + `JSON.stringify(objects)` 全量快照 + `historyChunkSize` 分块 |
| `engine/src/game/game.js:435` | ✓ `_.extend(globals, jsonParse(jsonStringify(C)))` |
| `driver/lib/runtime/runtime.js:60-70` | ✓ `intentCpu = 0.2`、`freeMethods`（say/pull）、per-object 去重计费 |
| `engine/src/processor/intents/creeps/tick.js:81` | ✓ 带 `ageTime` 的 creep 每 tick 全表 `_.find` portal（与移动无关） |
| `backend-npm/lib/game/socket/server.js:45-63` | ✓ 每消息 `zlib.deflate(level 1)` → `'gz:'+base64`；`JSON.stringify([channel, data])` |
| `xxscreeps/packages/pathfinder/src/pf.cc:256-264` | ✓ `heuristic_weight == 1` 分派 astar，注释点名 JPS 次优路径问题 |
| `/tmp/screeps-storage-pkg/package/lib/{queue,db}.js` | ✓ `queue.js:32` `pending.pop()`（LIFO）；`db.js:15` `autosaveInterval: 10000` |

其余 file:line 引用按笔记原样转录，未逐一复核。两处如实说明：① 每玩家每 tick 查询次数两份笔记分别记 19 与 20（`data.js` 两批 14+6 vs 14+5 计数口径差异，量级结论不受影响）；② 笔记中 lokijs 行号（`lokijs.js:2780,1334`）、xxscreeps git 提交 hash、profile.ts 未运行（需真实 terrain 世界文件）、PGO/LTO 量化未测——均引自笔记原文，置信级别见各条标注。

**第二轮修订新增核查**（2026-09-27，随审读意见逐条修订时执行）：
- WebFetch 抓取 `https://screeps.com/forum/topic/85` 原帖：确认 Dissi 原话 "It could save the CPU used on the Memory serialization, which is about 4% of my overal script times (higher in global reset ticks)"；帖内未披露 bot 规模与 Memory 大小（该帖主题为官方 2017 优化路线图，Memory 只是回帖中一点）。
- grep 复查 spike 笔记（`notes/sandbox-denocore.md`、`notes/spike-rquickjs.md`、`notes/rquickjs-run-output.txt`）：**无跨界调用（op/Reference.apply）单次延时的实测数据**——§2.6 的 1–10µs 维持【推断】，以"结论对单价误差不敏感"论证替代；spike 中可用的实测仅有 SAB 零拷贝共享（"one allocation shared by 2 isolates + host"）与 op2 fast 机制记载。

## 附录 B：未验证/未完成事项汇总（沿笔记如实转录）

- 跨界调用（deno_core op / ivm Reference.apply）单次延时未实测（§2.6 以机制推理 + 误差不敏感论证替代）；SAB 零拷贝共享已有 spike 实测，但"blob 视图 + 沙箱内 getter"字段读路径的端到端成本需在 M0 spike 补测。
- xxscreeps pathfinder 未实际编译（CMake 4.4 + clang modules 工具链），LTO/ICF/PGO 量化收益未测，只有作者提交信息可引（含 2020 年 "not actually any faster"）【文档】。
- 生产服务器 tick 耗时的官方阶段分解从未公开；本报告所有"×100 用户"换算均以私服实测单价线性外推【推断】。
- ExternalCopy（V8 序列化器）与 JSON round-trip 的等价性为"同量级或更高"的推断，未单独实测【推断】。
- 官方 MMO 侧（MongoDB + Redis）行为由 2017 blog + 架构页 + 私服包反推，无生产侧直接观测【文档+推断】。
