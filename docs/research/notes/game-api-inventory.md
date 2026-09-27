# Screeps 玩家可见功能全集盘点（deno_core 宿主能力需求视角）

> 调研日期：2026-09-27。数据来源：本地中文文档站 `/Users/keqing/Desktop/projects/sources/screeps/screeps-cn.github.io/`（下文来源均写相对路径）。共 49 个 API 类页面、约 340+ 方法/属性条目、443 个全局常量。

## 一、API 全集分类清单（按宿主能力需求分 7 类）

### A. 纯数据读取（属性访问，需要：JS 对象图注入/属性 getter）
- **Game**（`api/Game.html`）：`time, gcl{level,progress,progressTotal}, gpl{...}, creeps, spawns, structures, flags, powerCreeps, constructionSites, rooms, resources, shard, cpu(属性), map, market`
- **Game.shard**（`api/Shard.html`）：`name, type, ptr, access, accessTime`
- **Game.cpu 属性**（`api/Cpu.html`）：`limit, tickLimit, bucket, shardLimits, unlocked, unlockedTime`
- **Game.market 属性**（`api/Market.html`）：`credits, orders, incomingTransactions, outgoingTransactions`
- **Room**（`api/Room.html`）：`controller, energyAvailable, energyCapacityAvailable, memory, name, storage, terminal, visual`
- **Creep**（`api/Creep.html`）：`body, carry, carryCapacity, fatigue, hits, hitsMax, id, memory, my, name, owner, saying, spawning, store, ticksToLive, effects, pos, room`
- **RoomObject 基类**（`api/RoomObject.html`）：`pos, room, effects`
- **Structure/OwnedStructure**（`api/Structure.html`、`api/OwnedStructure.html`）：`hits, hitsMax, id, structureType, my, owner` + 各子类属性：
  - Controller：`level, progress, progressTotal, reservation, safeMode, safeModeAvailable, safeModeCooldown, sign, ticksToDowngrade, upgradeBlocked, isPowerEnabled`
  - Spawn：`memory, name, spawning`；Spawning：`name, needTime, remainingTime, spawn, directions`
  - Lab：`cooldown, mineralAmount, mineralType, mineralCapacity`；Link/Nuker/Tower/Observer/Extractor/Factory/InvaderCore/PowerBank/Portal（`destination, ticksToDecay`）/KeeperLair（`ticksToSpawn`）等各自属性
- **Store**（`api/Store.html`）：动态下标 `store[RESOURCE_*]` + `getCapacity/getFreeCapacity/getUsedCapacity`
- **RoomPosition**（`api/RoomPosition.html`）：`x, y, roomName`（可 `new` 构造）
- **静态对象**：Source（`energy, energyCapacity, ticksToRegeneration`）、Mineral（`density, mineralAmount, mineralType, ticksToRegeneration`）、Deposit（`cooldown, depositType, lastCooldown, ticksToDecay`）、Resource（`amount, resourceType`）、Nuke（`launchRoomName, timeToLand`）、Ruin（`destroyTime, store, structure, ticksToDecay`）、Tombstone（`creep, deathTime, store, ticksToDecay`）、Flag（`color, secondaryColor, memory, name`）、ConstructionSite（`progress, progressTotal, my, owner`）

### B. 需要宿主原生计算的同步调用（需要：Rust op 注入，同步返回）
- **PathFinder.search**（`api/PathFinder.html`）——官方标注"原生高性能 C++ 实现"，opts：`plainCost, swampCost, flee, maxOps(默认2000, 1 op≈0.001 CPU), maxRooms(16/64), maxCost, heuristicWeight(1.2)`；返回 `{path, ops, cost, incomplete}`
- **Room**（`api/Room.html`）：`findPath`(JPS/A*)、`find`(FIND_*，结果按房间+类型缓存到 tick 末)、`lookAt / lookAtArea / lookForAt / lookForAtArea`、`getEventLog([raw])`（前一 tick 事件 JSON，raw 免 parse）、`getPositionAt`、`getTerrain`、`serializePath / deserializePath`
- **RoomPosition**：`findPathTo, findClosestByPath, findClosestByRange, findInRange, getRangeTo, getDirectionTo, isEqualTo, isNearTo, inRangeTo, look, lookFor`
- **Creep.moveTo** = `pos.findPathTo() + move()` 简写（opts：`reusePath(默认5, 存 memory._move), serializeMemory, noPathFinding`）；`Creep.getActiveBodyparts(type)`
- **Game.map**（`api/Map.html`）：`describeExits, findExit, findRoute, getRoomLinearDistance([continuous]), getRoomTerrain, getTerrainAt(弃用), getWorldSize, isRoomAvailable(弃用), getRoomStatus → {status: normal|closed|novice|respawn, timestamp}`
- **Game.getObjectById(id)**（仅可见房间对象）
- **Game.market**：`calcTransactionCost`（`ceil(amount*(1-exp(-dist/30)))`）、`getAllOrders([filter])`、`getOrderById`、`getHistory`（14 天日线）
- **Game.cpu**：`getUsed()`（毫秒级浮点）、`getHeapStatistics()`（≈v8.getHeapStatistics + `externally_allocated_size`）

### C. 玩家回调进宿主计算（需要：Rust→JS 同步回调，性能敏感）
- **`PathFinder.search` 的 `roomCallback(roomName)`**：搜索中**每房间只调用一次**（每当搜索需要展开一个新房间时触发）；返回 `CostMatrix`（非零值替代默认地形成本，0=用默认，≥255=不可通行）或 **`false` 表示该房间不搜索且不计入 maxRooms**；返回 undefined 则用默认。文档建议玩家自行缓存 CostMatrix。
- **`Room.findPath / RoomPosition.findPathTo / moveTo / findClosestByPath` 的 `costCallback(roomName, costMatrix)`**：宿主传入**内置缓存矩阵实例**供玩家就地 `set()` 修改；**若回调返回新矩阵则替换缓存矩阵**（与 roomCallback 返回值语义不同的关键点）。
- **`Game.map.findRoute` 的 `routeCallback(roomName, fromRoomName)`**：返回进入该房间的浮点开销，`Infinity`=不可进入。
- `room.find(type, {filter})`：filter 为 lodash filter 谓词（对象/函数/字符串），在 JS 侧执行，非宿主计算。
- `createConstructionSite` **没有回调**（仅参数校验返回 ERR 码，如 `ERR_FULL` 工地上限 100、`ERR_RCL_NOT_ENOUGH`）。

### D. 大缓冲/二进制（需要：大字符串/TypedArray 高效传输）
- **RawMemory**（`api/RawMemory.html`）：`get()/set()`（主内存原始字符串，上限 2MB）；`segments`（ID 0–99 共 100 段 × 100KB，**同时最多激活 10 段**，`setActiveSegments` 下一 tick 生效、tick 末自动保存）；`foreignSegment{username,id,data}`（同时只 1 个外部分段，配合 `setActiveForeignSegment/setPublicSegments/setDefaultPublicSegment`）；`interShardSegment`（已弃用）
- **InterShardMemory**（`api/InterShardMemory.html`）：`getLocal/setLocal/getRemote(shard)`，每 shard 100KB，本 shard 独占写、其他只读
- **Room.Terrain.getRawBuffer([destinationArray])**（`api/Room.Terrain.html`）：返回 2500 字节 `Uint8Array`（按行存储，位掩码 `TERRAIN_MASK_WALL=1/SWAMP=2`）；可写入玩家指定数组视图（含 WASM heap），失败返回 `ERR_INVALID_ARGS`；`get(x,y)` 返回整数免字符串操作
- **PathFinder.CostMatrix**：`serialize()/deserialize()`（Uint8Array[2500]）、`set/get/clone`
- **RoomVisual.export()/import()**（每房 500KB 上限）与 **MapVisual.export()/import()**（1000KB 上限）：紧凑字符串，存 Memory 回放
- **二进制模块**（`modules.html`）：`require('xxx')` 返回 `.wasm` 内容的 **ArrayBuffer** → 玩家自行 `new WebAssembly.Module/Instance`（deno_core 需支持 WASM）

### E. 产生 intent 的动作（需要：intent 收集器 + 错误码同步返回）
- **Creep**（25 个）：`move(方向|相邻creep), moveByPath(数组|序列化串), moveTo, harvest, build, repair, dismantle, upgradeController, claimController, reserveController, attackController, attack, rangedAttack, rangedMassAttack, heal, rangedHeal, transfer, withdraw, drop, pickup, pull, say, signController, suicide, cancelOrder, notifyWhenAttacked, generateSafeMode`
- **PowerCreep**（`api/PowerCreep.html`）：静态 `create`；实例 `usePower(PWR_*, [target])`（**同 tick 仅一次、后调覆盖前调**）、`enableRoom(controller)`、`rename, upgrade, delete(24h), spawn(于 PowerSpawn, 8h 冷却), renew(母巢/宝库旁), suicide, cancelOrder` + 移动/资源类同 Creep
- **Structure 通用**：`destroy(), notifyWhenAttacked(v)`
- **分建筑**：Spawn `spawnCreep(body,name,{memory,energyStructures,dryRun,directions})`（dryRun 只校验）、`recycleCreep, renewCreep`；Spawning `cancel(), setDirections()`；Tower `attack/heal/repair`；Link `transferEnergy`；Lab `runReaction, reverseReaction, boostCreep, unboostCreep`；Terminal `send(资源,数量,房间,备注)`；Observer `observeRoom`（获得下一 tick 视野）；Nuker `launchNuke`；PowerSpawn `processPower`；Factory `produce`（commodities 配方）；Rampart `setPublic`；Controller `activateSafeMode, unclaim`
- **世界编辑**：`Room.createConstructionSite / RoomPosition.createConstructionSite`、`Room.createFlag / RoomPosition.createFlag`、`Flag.remove/setColor/setPosition`、`ConstructionSite.remove`
- **Market**：`createOrder({type:ORDER_SELL|ORDER_BUY,resourceType,price,totalAmount,roomName}), cancelOrder, changeOrderPrice, extendOrder, deal(orderId,amount,room)`（**≤10 笔/tick**，300 单上限，5% 手续费；账户绑定资源免 roomName）
- **账户级**：`Game.cpu.setShardLimits`（12h 冷却）、`unlock`（24h）、`generatePixel`（bucket 10000→1 PIXEL）、`halt()`（**重置运行环境并擦除堆内存**）、`Game.shard.activateAccess`（消耗 ACCESS_KEY）、`Game.notify(msg,[groupInterval])`（邮件，上限 20 条排队，模拟模式不可用）

### F. 视觉/绘制（需要：绘制原语序列化 + 每房容量统计，不计 CPU）
- **RoomVisual**（`api/RoomVisual.html`，链式调用，坐标可小数）：`line, circle, rect, poly, text(支持 emoji/字号字体/背景/对齐), clear, getSize(≤512000B), export, import`；`new RoomVisual([roomName])` 无视野也可画，不传名=全房间
- **MapVisual**（`api/MapVisual.html`）：`line, circle, rect, poly, text, clear, getSize(≤1000KB), export, import`，全局坐标（跨房间）
- 共性：不进数据库、只保留 1 tick、零 CPU 计费

### G. 全局工具与常量（需要：全局环境注入）
- **全局对象**：`Game, Memory, RawMemory, InterShardMemory, PathFinder, Game.map.visual, _`（lodash，`require('lodash')` 可省）、`console.log`（转发浏览器/服务器控制台）
- **443 个全局常量**（`api/constants.html`）：`OK/ERR_*`(19)、`FIND_*`(28)、`LOOK_*`(14)、`COLOR_*`(11)、方向 `TOP..TOP_LEFT`(8)、身体部件 `MOVE..CLAIM`+`BODYPART_COST`+`BODYPARTS_ALL`、`TERRAIN_MASK_WALL/SWAMP/LAVA`、`STRUCTURE_*`(26)、`RESOURCE_*`(~90，含基础矿/化合物/commodities/`OPS`)+`REACTIONS/BOOSTS/REACTION_TIME`、`ORDER_SELL/BUY、MARKET_*`、`EVENT_*`(18)+`EVENT_ATTACK_TYPE_*`、`POWER_CLASS/OPERATOR、PWR_*`(17)+`POWER_INFO`、`EFFECT_INVULNERABILITY/COLLAPSE_TIMER`、数值常量（`CREEP_LIFE_TIME, CREEP_CLAIM_LIFE_TIME, MAX_CREEP_SIZE=50, MAX_CONSTRUCTION_SITES=100, FLAGS_LIMIT, CONTROLLER_*, GCL_*, TOWER_*, TERMINAL_*, NUKER_*, POWER_*` 等）、`MODE_SIMULATION/MODE_WORLD, SYSTEM_USERNAME, SIGN_*, OBSTACLE_OBJECT_TYPES, RESOURCES_ALL, COLORS_ALL, INTERSHARD_RESOURCES, COMMODITIES`
- **require/module.exports** 模块系统（含跨 tick 模块缓存、global reset 后失效）

## 二、运行时语义要点

1. **游戏循环三阶段**（`game-loop.html`）：初始（服务器**重建全部游戏对象**与状态）→ 执行（跑玩家 `main` 及模块；**改变状态的指令不立即生效**，如先 move 后 attack，攻击按移动前位置结算）→ 结束（服务器处理 intents，冲突按优先级执行；不同玩家指令可同时生效）。tick 长度=所有玩家跑完。
2. **对象每 tick 重建**："所有运行游戏对象及变量在每 tick 后都会被清除重建"——JS 侧缓存的引用下一 tick 失效；跨 tick 持久化只有 `Memory`/`RawMemory`/global（global 周期性 reset，`Game.cpu.halt()` 可主动触发）。
3. **控制台指令**在 main 之后、同规则执行（`debugging.html`）。
4. **CPU 计费**（`cpu-limit.html` + `api/Cpu.html`）：毫秒计费；默认 20 CPU；bucket 上限 10000、tickLimit ≤500 且 ≥limit；超时**强杀**；`JSON.parse(Memory)` 计入玩家 CPU；`room.find` 结果自动缓存（同 tick 同 key 免费复用）；寻路 1 op ≈ 0.001 CPU；RoomVisual/MapVisual **不计费**；`getUsed()` 模拟模式恒 0。
5. **Memory ↔ RawMemory**（`global-objects.html`）：`Memory` 上限 2MB、tick 内**首次访问时惰性 JSON.parse**、tick 间 JSON.stringify 传递；`creep.memory`/`room.memory`/`spawn.memory` 是 `Memory.creeps[name]` 等的**别名**；可弃用 Memory 直用 `RawMemory.get/set` 自管序列化。不能存活动对象引用（应存 id + `Game.getObjectById`）。
6. **同步动作优先级**（`simultaneous-actions.html`）：同一 creep 冲突动作按优先级图右侧覆盖左侧，**但被覆盖动作仍返回 OK**；相同动作最后一个生效；无冲突动作（moveTo+rangedMassAttack+heal+transfer+drop+pickup+claim）可同 tick 叠加；能量类动作资源不够时按优先级裁决。
7. **错误处理**：动作以 `ERR_*` 返回码报错而非异常；`console.log`、`debugger`（浏览器模式）、Memory watch 面板调试；文档**未提 source map**，仅提错误码与 console。
8. **文档未提及 async/await、Promise、setTimeout/setInterval、网络、文件**——纯同步模型，宿主不应注入这些（全站 grep 无命中）。模块系统 = CommonJS `require/module.exports` + 内置 lodash + WASM 二进制模块。
9. **Actions vs 数据**：玩家代码只通过方法下达指令，直接改 `Game` 对象属性无效。

## 三、特殊/边角能力

- **Game.shard / 受限 shard**：`activateAccess()` 消耗 ACCESS_KEY 开 30 天访问（`api/Shard.html`）
- **Pixel 经济**：`Game.cpu.generatePixel()`（bucket 10000→1 PIXEL 账户资源）；`Game.resources` 存 pixel/cpu unlock/access key；`PIXEL_CPU_COST`
- **跨 shard**：InterShardMemory（100KB 双向只读写分离）、Portal（`destination` 可跨 shard）、市场交易 `INTERSHARD_RESOURCES`（订阅代币等免 terminal）
- **Market 细节**（`market.html` + `api/Market.html`）：NPC 终端挂单（十字路口房间）；订阅代币；deal 距离近者优先；取消单扣 5%；`getHistory` 14 天日线
- **Power 全流程**（`power.html`）：PowerBank（反弹 50% 伤害）→ 8 级房 PowerSpawn.processPower（50 能量+1 power→GPL）→ PowerCreep（0–25 级，Operator 17 个 PWR_*；Commander/Executor 未实装）→ `enableRoom` 后才能在 RCL 房用 power；试用 token 30 次
- **NPC 入侵者/要塞**（`invaders.html`）：房间开采计数器 ~100k 能量触发入侵；Stronghold=`StructureInvaderCore(level)`+`EFFECT_COLLAPSE_TIMER`，废墟保留保护计时
- **Respawn/安全模式**（`respawn.html`）：初始 20000 tick safemode；重生 180s 冷却，GCL 不降；`activateSafeMode`、`signController`
- **事件日志**：`room.getEventLog()` 全量 EVENT_*（攻击/建造/治疗/传送/exit 等）
- **CPU unlock**：24h 全量 CPU，消耗 cpuUnlock 资源；`setShardLimits` 12h 冷却
- **弃用项**（兼容但勿新做）：`PathFinder.use`, `Game.map.getTerrainAt/isRoomAvailable`, `RawMemory.interShardSegment`, `StructureSpawn.createCreep/canCreateCreep`, `creep.carry/carryCapacity`, `StructureContainer.storeCapacity` 等

## 四、deno_core 宿主能力映射（逐类一句话）

| 类别 | 需要的宿主能力 |
|---|---|
| A 纯数据读取 | 每 tick 向 isolate 注入/重建整棵游戏对象图（快照式或带 JS 包装类的 op-proxy） |
| B 宿主同步计算 | 同步 Rust op（`Deno.core.ops`）：寻路、find/look、market 查询、getUsed 等，需精确 CPU 计时 |
| C 回调进宿主 | Rust 计算循环中**同步回调 JS 函数**（roomCallback/costCallback/routeCallback），要求 reentrancy 与极低调用开销，CostMatrix 双语义（就地改/返回替换/false 屏蔽） |
| D 大缓冲 | 大字符串零拷贝（`String`/`v8::String` externalized 或 `FastString`）与 `Uint8Array` 共享/拷贝通道（terrain 2500B、memory 2MB、visual 500KB–1MB） |
| E intent | per-tick intent 列表的 op 落账（op 校验+记录，tick 末结算），含 dryRun 类"只读探测" |
| F 视觉 | 纯 JS 侧累积 + tick 末导出序列化字符串回宿主（几乎零原生需求，但 getSize/export 需字节数统计） |
| G 全局工具 | 全局对象/443 常量/lodash 注入 + CommonJS `require` 模块加载器（含 WASM ArrayBuffer 二进制模块） |

**最依赖宿主能力的 Top 5**：① PathFinder.search 原生寻路 + roomCallback 反复重入回调；② RawMemory/segments/InterShardMemory 大字符串通道；③ 每 tick 全对象图重建（数百对象 × 属性访问性能）；④ CPU 计费/硬中断与 getUsed 毫秒精度（已验证，见 02-sandbox-evaluation.md）；⑤ Terrain.getRawBuffer/CostMatrix/Visual export 的 TypedArray 与序列化数据传输。
