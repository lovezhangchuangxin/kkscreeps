# 玩家侧体验一致性红线（Compatibility Charter）

> 2026-09-27 确立。约束：**内部可以任意优化，但玩家侧体验必须与官方一致，不得出现游戏脚本 API 的变动或差异。**
> 本文是所有 crate 设计与验收的硬约束，优先级高于一切性能优化。
>
> **适用范围**：本红线约束**玩家侧**（玩家脚本可见的 API 与行为）。服务器管理侧不在范围内：官方私服的 JS mod 生态（screepsmod-* 等）依赖官方 Node.js 进程内部与数据库集合，本项目不做官方 mod 兼容（[ADR-007](../decisions/ADR-007-mod-system.md)），自建 mod 机制（见本目录 README 的规划清单）。

## 一、完全不可变的部分（1:1 复刻，逐项验收）

1. **API 面**：约 340+ 方法/属性、443 个全局常量（盘点清单见 `../research/notes/game-api-inventory.md`）——名称、签名、返回值、错误码（`OK/ERR_*`）逐个对照官方实现移植；数值表直接从 `common/lib/constants.js` 生成，不手抄。
2. **运行时语义**：游戏循环的分阶段推进（官方模型为三阶段，本项目内部拆为五步，见 [game-loop.md](../architecture/game-loop.md)）、intents 延迟结算、同 tick 冲突动作优先级图（被覆盖动作仍返回 OK）、对象每 tick 重建、global 跨 tick 持久与 reset 时机、`Game.cpu.halt()` 行为。
3. **CPU 经济**：`tickLimit = min(500, limit + bucket)`、bucket 上限 10000 / 每 tick +limit、0.2 CPU/intent 计费（say/pull 免费、幂等、可退费）、硬超时按 tickLimit 计费且桶可为负、桶为负时下一 tick 停跑一次并回充（官方 `cpuAvailable` 语义；skipTicksPenalty 是官方 runtime 硬重置专用，不用于普通超时）、`getUsed()` 计量公式与计费窗口（含 Memory parse 计入玩家 CPU 的官方语义）。
4. **数据语义**：Memory/RawMemory（2MB 上限、惰性 parse、tick 末写回）、segments（100KB×100、激活 10 段）、InterShardMemory、`creep.memory` 别名机制。
5. **模块系统**：CommonJS 多模块（require/module.exports、循环 require 半成品语义、模块缓存跨 tick、代码更新重建）、lodash 3.10.1 全局 `_`、`.wasm` 二进制模块。
6. **沙箱环境**：V8 语义（已选 deno_core）、无 setTimeout/网络/文件、`WebAssembly` 可用性同官方。
7. **寻路结果**：路径必须与官方逐点一致（含 tie-breaking）——通过 FFI 复用官方 `driver/native/src/pf.cc` 源码保证，不自行实现可能产生等价但不同路径的算法。

## 二、内部替换但玩家不可感知的部分（优化主战场）

序列化格式（从 JSON 文本改为直接的二进制编码，思路来自 rkyv——一个 Rust 的零拷贝序列化库）、存储引擎（从 lokijs/MongoDB 改为嵌入式键值库 redb/libmdbx）、进程拓扑（5 个以上进程改为单个二进制）、按坐标的空间索引、对象按类型分桶存放（SoA，即同类数据连续排布的内存布局）、按房间的流水线推进、增量推送——玩家看到的仍是同样的 JS 对象图、同样的 sockjs 协议（官方客户端使用的 WebSocket 兼容协议）、同样的渲染。判据：**任何玩家脚本在官方服务器与本实现的输出序列（console、intent 序列、结果状态）不可区分。**

## 三、灰区与默认决策（发现新灰区须在此登记）

| 事项 | 默认决策 | 说明 |
|---|---|---|
| 增量化后 Memory parse 的计费值 | **按官方等价工作量虚拟计费**，不因实现变快而少计 | 官方按实际耗时计费；我们增量化后实际耗时趋近 0，若照实计费会改变经济。虚拟计费保持玩家体感公平 |
| tick 速率变快 | 允许（这正是优化目标） | 官方不同 shard tick 速率本就不同；CPU 是预算制，玩家能做的事不变 |
| CPU 读数逐 tick 数值 | 不保证与官方逐毫秒相同 | 官方跨机器也不会相同；保证的是公式与口径一致 |
| 附加能力（如 ESM 入口、附加 API） | **默认关闭，开关显式开启** | 严格模式下零差异；开关开启的行为属于本实现扩展，不算官方兼容性破坏 |
| 官方 bug | screeps-ok 覆盖的全部复刻；未覆盖的官方 bug 若被证明有玩家依赖再评估复刻 | 例如官方 market `_.shuffle` 用 Math.random 破坏确定性——replay 需要确定性时以"官方行为"为准还是"官方文档"为准，逐例登记 |
| `Math.random` 等非确定性源 | 保留官方行为（真实随机） | 官方本就不可复现；内部结算的确定性（replay 用）与玩家可见随机性分开处理 |
| 循环 require 的行为（M0 桩环境） | **Node 语义**（循环点拿半成品 exports）；官方为抛 "Circular reference to module" | M2 玩家 API 层对照官方 game.js 拍板跟谁——翻转前带循环依赖的 bot 在 kk 通过≠官方通过（验收反向假绿风险） |
| 模块包装形态（M0 桩环境） | **Node 形态**：require 为包装参数、模块 `this = module.exports`；官方（runtime-driver.js evalCode）为 `(function __module(module,exports){…})`、require 是全局变量、非严格模块 `this` 绑定全局对象 | 与循环 require 同批（M2 玩家 API 层）拍板；`this` 用法在官方 bot 生态极罕见 |
| RoomPosition 参数解包宽松度（M0 桩环境） | **宽松**：裸 `{x,y}` 字面量按同房间坐标计算；官方 fetchXYArguments（utils.js:36-63）只认 RoomPosition 实例或带其实例 pos 的对象，裸字面量得 NaN | 官方 inRangeTo 两参形态对无 roomName 目标恒 false（utils.js 官方自身与 getRangeTo 不一致）——M2 精确移植时逐条对齐官方源码，含官方自不一致处 |
| RoomPosition.__packedPos（M0 桩环境） | 未实现（官方 RoomPosition 有 `__packedPos` own 属性，isEqualTo 走 packed 快路径 rooms.js:1577）；我们按坐标比较等价 | M2 packed 坐标移植（screeps-ok 兼容）时一并实现，玩家读该属性可感知差异 |
| Memory 官方解析语义三细节（M0 桩环境） | M0：解析失败重置 `{}`、非对象 JSON 值重置 `{}`、顶层保留 Object 原型。官方（game.js:470-487）：解析失败 `Memory = null` 且不写回；合法 JSON 非对象值（如 `"5"`）保留原值；解析成功后顶层 `__proto__ = null`（无原型对象，hasOwnProperty 等不可用） | 属 M2-2A Memory 正式实现（与台账 JS语义-7 同批）对照移植范围；M0 桩 RawMemory 由宿主注入、无损坏输入路径 |
| Game 全局可写性（M0 桩环境） | M0 用 getter-only defineProperty（每 tick 重挂新实例）；官方 `globals.Game = makeGameObject(...)` 为普通可写属性、玩家可覆盖 | 极端边缘（无 bot 覆盖 Game）；M2 玩家 API 层按官方形态重挂 |
| `Game.structures` 收录范围（M0 桩环境） | M0 桩收录全部结构；官方只收自家 owned 结构（含 spawn、自家 controller），source/mineral/road/wall/container 不入 | M2 玩家 API 层对齐官方 game.js:298-339 的分册逻辑 |
| 验收仪表/宿主 tick 通道的防篡改深度（M0 桩环境） | `__kk` 已整体 freeze（防属性覆盖）；玩家直接调用 `drainMissing()` 抢先清空仪表的深层防御未做 | 根治需宿主侧闭包/独立 op，归 M2 玩家 API 层重写范围 |
| 房间名解析强度（kk-core RoomName） | **强类型构造即校验**：方向位非法/符号消费错位/Unicode 数字一律 Err；官方 roomNameToXY（utils.js:374-392）对方向位只判 W/w 与 N/n、其余一律当 E/S（"X1N1"→[1,-2]、"E1X1"→[1,1]、"W-100N5"→[99,0] 静默别名到其他房间），parseInt 还认 Unicode 数字 | 官方这些形态得到"合法但错误"的坐标；我们不产出静默别名坐标。embed 边界吃外部房间名时该差异玩家可见（screeps-ok 用例若依赖官方宽松态会显形） |
| creep TTL 死亡的产物（M1 引擎） | **资源直接掉地为 energy 对象，不产 tombstone**；官方 _die.js:16-33 产 tombstone 承接全部资源与 body 造价回收 | M1 范围无 tombstone 变体（kk-schema v1）；M2 加 tombstone 变体走 schema 版本升级。M1 内资源守恒语义一致（总量不变），缺的是墓碑对象与 decay 链 |
| flag 的存储位置（M1 引擎） | flag 折进 RoomObject 变体（tag 8）；官方 flag 是独立集合（processor.js:22 bulkFlags，不参与 roomObjects 循环与结算） | M1 单房间结算 flag 无规则行为差异；投影层（world_json → prelude）输出官方形状即可 |
| 地形输入校验强度（kk-core Terrain） | 长度≠2500、未知位（& !0b111）、数字串形态的 '8'/'9' 一律 Err；官方 checkTerrain 对字符串形态照单全收（'9' 判为 wall） | 正确性-7/8 的清偿方向：非法输入前置拒绝而非静默截短/误读；官方吃 '9' 是无校验不是语义 |
| spawn.spawning 投影形态（M1 typed world） | 发绝对 `spawnTime`；官方 StructureSpawn.Spawning 暴露 `name/needTime/remainingTime/directions`（structures.js:1302-1310，remainingTime=spawnTime−time） | prelude 尚未消费（硬编码 null）；embed 接线暴露给玩家时改 remainingTime 形态 |
| source/controller 的 None→0 投影（M1 typed world） | `ticksToRegeneration`/`safeMode` 无钟时发 0；官方 getter 与 vanilla 快照为 undefined/null | 0 vs null 仅 getObject 断言面可见；prelude `?? 0` 归一，玩家面等价 |
| 放置的 owner 宽松接受面（M1 place 归一化） | typed 变体不强制 owner（缺省空串）；vanilla 对 REQUIRING_OWNER 类型与 placeCreep/placeSite/placeFlag 强制 owner 并抛错 | M0 哑世界同宽松；强制化随 M2 玩家 API 层（届时违规放置显形） |
| source 容量三分支缺 reservation（M1 place 归一化） | ControllerObj 无 reservation 字段，被预定（无主）房间 source 容量判 1500；vanilla 读 user || reservation 判 3000 | 当前放置 spec 无法表达 reservation 不触发；reserveController intent 实装时补字段 |
| extension 能量容量字段缺失（M1） | ExtensionObj 无 per-RCL 容量（EXTENSION_ENERGY_CAPACITY[rcl] 50..200）；prelude 兜 0 | M1 结算不消费（_calc_spawns 只合计 spawn/extension 存量）；energyAvailable 语义实装时补 |
| 非 energy 掉落资源（M1 place） | 显式拒绝（Err）；vanilla 支持 power/矿物 | M2 扩展 schema 资源种类时放开 |
| 未知类型工地（M1 place） | 接受、progressTotal 派生 0；vanilla placeSite 对无 CONSTRUCTION_COST 类型抛错 | M0 宽松接受面决策；build 结算按官方 build.js:19-21 拒绝无 cost 目标 |
| creep 放置 ttl 无上限钳制（M1 place） | `time + ttl` 原样（可超 1500）；官方 renew 才有 1500 上限语义（拒绝式非钳制） | 三方评审裁决跟 vanilla 放置语义（index.ts:1087），2026-09-28 |
| 玩家内 room intents 先于对象 intents（M1 结算） | createConstructionSite/createFlag 全部先于 creep/spawn 对象指令执行 | 官方单层 objects map 中 'room' 键按插入序交错；结果序无关（房间指令与对象指令无共享状态面），登记迭代序差异 |
| move intent 的 id 形态（M1 结算） | 仅支持 direction 形态；官方 move.js:15-21 还支持"朝相邻 creep 移动"（id 字段） | M2 玩家 API 层补（真沙箱才会产生该形态） |
| creep 死亡事件与墓碑（M1 结算） | 无 EVENT_OBJECT_DESTROYED、无墓碑（资源直接掉地已另登记） | 随墓碑变体（M2 schema 升级）一并补事件 |
| spawn/extension 的 off 标记生效时序（M1 结算） | `_calc_spawns` 直接改工作副本 → 同 tick 即时生效 | 官方经 bulk.update 延迟落库 → 同 tick 读到旧值；仅配额翻转 tick 可观测（多一个 spawn 被计费的窗口） |
| 踩敌方工地的 safeMode 门（M1 结算） | 未实现（M1 无 safeMode 激活路径） | 官方 movement.js:224 要求 controller 非敌 safeMode；activateSafeMode 实装时补 |
| 无 controller 房间的建造配额（M1 结算） | 完全跳过配额检查 | 官方按 rcl=0 计（spawn/extension 配额 0 → 拒、road 2500 → 允许）；M1 测试房间恒有 controller |
| createCreep 空 energyStructures（M1 结算） | `[]` 落回全房间扣费（与缺省同路径） | 官方空数组走显式列表模式（可用 0 → 失败）；根因是 sanitize 后 None/[] 不可区分，M2 intent 通道定型时修 |
| harvest 0 个 WORK 部件的事件形态（M1 结算） | 推 EVENT_HARVEST(amount=0) | 官方 harvest.js:44 `if(harvestAmount)` 整体跳过；upgrade/build 我们在 effect=0 时跳过（与官方反向）——M2 统一零量事件原则时对齐 |
| Engine 沙箱的玩家身份信任（M1） | 按沙箱自报的玩家名合并 intents | M1 沙箱是自家 ScriptedSandbox；M2 真沙箱接入时按注册名强制（防冒名 room intent） |
| controller 的 hits/hitsMax 投影（M1 typed world） | 不发（prelude 兜 0） | M0 哑世界发 10000 手抄值；官方 getter 直读 DB 字段、vanilla 放置不带 hits → 官方为 undefined。0 vs undefined 是 prelude 残差，M2 玩家 API 层对齐 |
| typed 变体丢弃 spec 的非 energy 资源与私有时序字段（M1 place） | creep/spawn/extension/storage 的 store 仅保 energy；ticksToDecay/cooldown 等不透传（road 恒 ROAD_DECAY_TIME） | M1 schema 资源单槽；多资源种类随 M2 schema 扩展放开 |
| 未注册房间放置（M1 embed） | 报错（房间不存在） | M0 哑世界自动建房、vanilla 不校验——显式拒绝优于静默建出无地形房间；screeps-ok 套件先 createShard 后放置，不可达 |
| 多房间世界 JSON 的 objects 序（M1 typed world） | 房间名序 × 房内 id 升序 | M0 为全局 id 序（单房间等价）；多房间下 Game.creeps 等键插入序可能不同——若出现顺序敏感用例以此归因 |
| controller safeMode 投影发绝对 tick（M1 typed world） | 玩家面应发剩余值（官方 structures.js:187 getter） | prelude 硬编码 0 不消费、M1 无激活路径（activateSafeMode 是 M2）；激活实装时改 remainingTime 形态（同 spawn.spawning 条目） |

## 四、验收关卡（不做完不算兼容）

1. **screeps-ok 行为一致性套件**（`sources/screeps/screeps-ok`，vanilla 2897 用例）从 M1 起进 CI 持续跑 parity；
2. 真实开源 bot 回归（tooangel/overmind 类）在 deno_core + CJS 垫片 + lodash3 底座上原样可跑；
3. 官方客户端/renderer 可正常连接游玩（API/sockjs 协议兼容）；
4. 对照移植检查表：每个 API 移植时登记"官方实现 file:line ↔ 本实现 file:line"映射。
