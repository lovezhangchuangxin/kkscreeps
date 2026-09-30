# M2B 扩面批：玩家 API ~150→~230（官方全量 340 的可达子集）

> 状态：**已收口（2026-09-30 开批，2026-10-01 P0-P6 全部完成，见 §8）**。
> 批次方向决策归用户，批内切片节奏自主。
> 定位：ADR-013 决策 2 的 M2B 扩面本体。前置批（seam 三件套，ef43f4a 五片）与
> 双房实弹批（外部复审两轮清偿，0b83f1d）均已收口——**本批是扩面前的全部前置
> 关卡走完后的第一扩面批**。已授权序列：本批 → 2B。
> 依据：[m2-playable](m2-playable.md) §1.2/§10-9（API 补全批组）、
> [ADR-013](../decisions/ADR-013-architecture-review-coordinator.md) 决策 2、
> [m2b-prerequisite](m2b-prerequisite.md) §8（同步点全表）、
> [dual-room-live-fire](dual-room-live-fire.md) §7（P2-8 rooms 收敛登记）。

## 0. 目标与总原则

- **官方面全量盘点 → 诚实缺口**：官方 game/ 层 ≈337 面（m2-playable §0-2 实测）。
  全量 340 中 PowerCreep 38 面/market 14 面/power 生态等属 M4/2B 能力门——
  本批交付**不依赖未落地能力**的全部缺口（~+80 面，落点 ~230），余量逐项
  归因（§6）。**不冒充**：无执行器/无数据的面不换真，维持桩+登记。
- **硬边界（用户指令）**：新 getter 数据一律经 seam——具体形态见 §3.1（本批
  评估结论：零新 WorldSource 方法，新面全部经既有投影/trait 查询面的产出）；
  typed FIND 不做；createFlag/createConstructionSite 等 countStub 面转真
  （intent 通道+结算执行器已交付，双房批勘误登记归本批）；
  findClosest\* 照官方形态内部走 room.find（官方 rooms.js:1518-1576 实证：
  findInRange/findClosestByRange 体内即 `room.find(type, opts)`——自动复用
  tick 内 findCache）。
- **逐片完整批次循环**（§5 验证矩阵），统一提交点纪律不可破。

## 1. 官方 API 面全量盘点与缺口清单（2026-09-30 对 HEAD=d875734 实测）

> 官方侧 = `~/Desktop/projects/sources/screeps/engine/src/game/` 逐文件抽取
> （defineGameObjectProperties 字段表键 + prototype 方法 + game/map/market
> 对象字面量键；RawMemory 在 driver/lib/runtime/runtime.js）。
> 我方侧 = crates/kk-runner/src/prelude/30_game/ 逐文件。
> 「面」= getter + 方法（含 toString/toJSON 家族）。**状态**：✅=真实现；
> ⬜=缺（无此面）；🪑=countStub 桩；💎=常量桩（返回固定值）。

### 1.1 Game 命名空间（官方 game.js:146-186）

| 面 | 官方 file:line | 我方现状 | 本批处置 |
|---|---|---|---|
| time/creeps/spawns/structures/flags/constructionSites/rooms/resources/getObjectById | game.js:146-171 | ✅ | — |
| cpu.limit/tickLimit/bucket/getUsed | game.js:147-152 | ✅ | — |
| cpu.getHeapStatistics | game.js:153-155（V8 原生注入） | 🪑 | **P3 真化**（op_kk_get_heap_statistics，D2 遗留「随批补」） |
| cpu.halt | game.js:156（cpuHalt 注入） | 🪑 | **P3 真化**（映射既有看门狗硬停——语义=主动掐断本轮，对照 make.js 超时路径；实现期核对差异登记） |
| cpu.generatePixel/unlock/setShardLimits | 官方 MMO 订阅面 | 🪑💎 | 维持+登记（单机服务端官方同拒） |
| cpuLimit | game.js:145 | ⬜ | **P3 顺手**（= cpu.tickLimit 同源别名） |
| gcl 三 getter | game.js:159-163 | ✅ | — |
| gpl 三 getter | game.js:164-168 | 💎 桩 | 不做（M4 power；官方数据源 user.power） |
| shard | **官方 game 层无此 getter**（grep 证） | 我方有（超集） | 维持现状（既有登记口径） |
| notify | game.js:178-186（`intents.push('notify',{message,groupInterval},20)`） | 🪑 | **P3 真化**：global 特殊键 intent 通道 + 结算记入 PlayerState（查询面官方无玩家可见出口——通知只进官方后台，登记差异）；计费 20 CPU 官方同形 |
| map / market | 见 1.2/1.3 | | |
| survivalInfo | rooms.js:437（Room 实例属性，survival 模式专用） | ⬜ | 不做（官方 survival 模式专用） |

### 1.2 Game.map（官方 map.js）

| 面 | 官方 file:line | 我方现状 | 本批处置 |
|---|---|---|---|
| describeExits | map.js:20-46（读 mapGrid t/b/l/r） | 💎 `{}` | **P3 真化**：世界房名表推导（邻房已注册→出口开）。登记差异：官方 mapGrid 显式出口标记 vs 我方邻房注册推导（等价于官方全开通网格） |
| findRoute | map.js:69-188（describeExits 上 DijkstraFinder + routeCallback/heuristic/avoid） | 💎 `[]` | **P3 真化**（房间图小，手写 Dijkstra 对照官方 pathfinding 语义：cost=1+routeCallback 回值、heuristic=房名线性距） |
| findExit | map.js:188-197（findRoute[0].exit） | 💎 `-1` | **P3 真化**（官方同形转发） |
| getRoomStatus | map.js:209-233 | 💎 normal | **P3 真化**：已注册→normal / 未注册→closed / 非法门名→undefined（官方 novice/respawn 状态无来源，登记） |
| isRoomAvailable | map.js:201-207（deprecated） | ⬜ | **P3 顺手**（= getRoomStatus!=closed 官方同形） |
| getRoomLinearDistance | map.js:265-267（utils.calcRoomsDistance） | 💎 `0` | **P3 真化**（房名坐标算术 + continuous 形态，utils.js 对照） |
| getWorldSize | map.js:269-271（driver.getWorldSize） | 💎 `1` | 维持+登记（官方有限世界 vs 我方注册制世界，语义不可迁移） |
| getTerrainAt / getRoomTerrain | map.js:235-263 / :260 | ✅/✅ | — |
| visual（MapVisual） | map.js:271-380（circle/line/rect/poly/text/size/clear/export/import） | ⬜ | **P6 真化**（收集面+size/export 真形态；消费通道登记 F2 合并期对齐，同 RoomVisual） |

### 1.3 Game.market（官方 market.js）——**本批不做，整体推 2B**

官方 12 面（calcTransactionCost/getAllOrders/getOrderById/createOrder/cancelOrder/deal/changeOrderPrice/extendOrder + credits/orders/incomingTransactions/outgoingTransactions/getHistory 族，market.js:31-191/196-250）。理由：ratchet 市场用例全在 terminal 能力门 skip 池（本批零棘轮收益）；订单簿/成交物流依赖 terminal 结构与跨房资源转移结算（2B 能力批主体）；global intent 通道本批由 notify 小面先行验证形态（为 market 铺路）。terminal.send 同批推 2B。

### 1.4 Room / Terrain / RoomVisual / RoomPosition（官方 rooms.js）

| 面 | 官方 file:line | 我方现状 | 本批处置 |
|---|---|---|---|
| name/memory/controller/energyAvailable/energyCapacityAvailable/mode | rooms.js:429-535 + game.js:325-328 | ✅ | — |
| storage/terminal 回填 | game.js:330+（buildGame 赋结构实例） | ⬜（恒 undefined） | **P2 真化**（makeRoom 单趟扫描时顺带回填，官方同构） |
| toString/toJSON | rooms.js:536-549 | toString ✅ / toJSON ⬜ | **P2 核对补齐** |
| find | rooms.js:584-767 | ✅（findCache 官方 quirk 同形） | — |
| lookAt/lookForAt/lookAtArea/lookForAtArea | rooms.js:768-862 | ✅ | — |
| getEventLog | rooms.js:573-583 | ✅（B3） | — |
| getPositionAt | rooms.js:971-976 | ✅ | — |
| getTerrain | rooms.js:1134-1137 | ✅ | — |
| **createFlag / createConstructionSite** | rooms.js:978-1027 / :1029-1097 | 🪑 countStub | **P2 转真**：预检（配额/覆盖/RCL/房名）+ room 级 intent——通道与结算执行器已交付（m2-playable 4.3 勘正口径）；返回官方 name/id 形态 |
| findPath | rooms.js:863-970（寻路） | ⬜（无此面） | **P2 直线桩**（findPathTo 同款退化形态——有可调用面不冒充寻路，灰区表既有登记延续；真语义归 FFI 批） |
| findExitTo | rooms.js:1130-1132（map.findExit 转发） | ⬜ | **P3 真化**（勘正：P2 盘点误排——依赖 P3 map 族先行，官方同形转发；见 §8 P2 记录） |
| serializePath/deserializePath（静态） | rooms.js:528-530 | ✅ | — |
| Room.Terrain get/getRawBuffer | rooms.js:1223-1241 | ✅（b64 紧凑协议） | — |
| RoomVisual 9 面 | rooms.js:1138-1220 | 🪫 Proxy 桩 | **P6 真化**（收集面官方同形：链式返回 this、getSize/export/import 真值；ops 收集后经 endTick visual 通道带出——消费归 F2，登记） |
| RoomPosition 3 props + 16 方法 | rooms.js:1288-1636 | 10 ✅；findPathTo/findClosestByPath 🪑；createFlag/createConstructionSite 🪫；findClosestByRange 💎 | **P2**：findClosestByRange 转真（官方 :1546-1576：内部 `room.find` + getRangeTo 打擂——自动复用 findCache）；createFlag/createConstructionSite 转真（官方 :1622-1636：转发 room 级）。findPathTo/findClosestByPath 维持桩（依赖寻路，归 FFI 批） |

### 1.5 Creep（官方 creeps.js）与 Store

| 面 | 官方 file:line | 我方现状 | 本批处置 |
|---|---|---|---|
| 14 字段 getter + memory | creeps.js:71-120 | ✅（saying 🪫 恒 undefined） | **P1 saying 真化**（actionLog.say 文本+isPublic 投影——B3 tombstone creep_saying 同型载荷） |
| move/moveTo/moveByPath/harvest/drop/transfer/withdraw/pickup/build/repair/upgradeController/suicide/say/signController/pull/cancelOrder | creeps.js:126-588/813-838/977-1028/1050+ | ✅（moveTo 直线桩维持，登记） | — |
| attack/dismantle/heal/rangedHeal | creeps.js:593-733 | ✅（B1） | — |
| claimController/attackController/reserveController/generateSafeMode | creeps.js:839-910 | ✅（B2） | — |
| **rangedAttack / rangedMassAttack** | creeps.js:626-677 | 🪑 countStub | **P1 转真**（**前提勘正（P0 期盘点误判）**：Action 枚举虽在但 settle 分派走 `_ => {}` 静默跳过臂——B1 只做了近战族，ranged 执行器随 P1 新写：rangedAttack.js/rangedMassAttack.js 逐行对照 + apply_damage 复用；盘点表原「执行器已在」系把 intent 通道当成了执行器） |
| **getActiveBodyparts** | creeps.js:589-592 | ⬜（无此面） | **P1 补面**（body 折叠，官方同形） |
| **notifyWhenAttacked** | creeps.js:996-1015 | ⬜ | **P1 补面**（Action::NotifyWhenAttacked 已在——Structure 版真实现有，Creep 版对照移植） |
| CREEP_METHODS 残余桩表 | — | 表含 **非官方面**（spawn/recycleCreep/renewCreep/moveHead 挂在 Creep 原型上） | **P1 清理**：桩表收敛到官方 29 方法口径，非官方面摘除（玩家可见面收缩——棘轮+bot 三基线盯） |
| Store getCapacity/getUsedCapacity/getFreeCapacity + 动态资源下标 | store.js:19-96 | ✅ | — |

### 1.6 Structure 族（官方 structures.js）

基类与已真化类：

| 类 | 状态 |
|---|---|
| Structure（hits/hitsMax/structureType + destroy/notifyWhenAttacked/isActive） | ✅ |
| OwnedStructure（owner/my）、Controller（10 getter + unclaim/activateSafeMode）、Extension、Storage、Extractor（cooldown）、Rampart.isPublic、Source、Mineral、Tombstone、Ruin、Spawning（5 props + setDirections/cancel） | ✅ |
| Spawn（6 getter + memory + spawnCreep/renewCreep/recycleCreep/notifyWhenAttacked） | ✅ |
| Resource（resourceType/amount） | ✅（energy 字段 ⬜ → **P4 顺手**） |

缺口（本批做）——**注意 tower/link/container 仍是 Generic 透传桶（schema 无 typed 变体、结算忽略），真化=变体升级+结算，是本批最重的片**：

| 类 | 面 | 官方 file:line | 本批处置 |
|---|---|---|---|
| StructureTower | energy/energyCapacity/store（else 分支已有）+ attack/heal/repair + cooldown | :760-806/:762 | **P5**：TowerObj typed 变体（追加尾部，B3/B4 先例）+ 结算（官方伤害/治疗/修理公式+距离衰减+TOWER_ENERGY_COST 耗损+TOWER_COOLDOWN 递减）+ API 预检面 |
| StructureLink | energy 族（else 已有）+ cooldown + transferEnergy | :481-530 | **P5**：LinkObj typed 变体 + transferEnergy 结算（LINK_CAPACITY/冷却/距离无损）+ API 面 |
| StructureRampart | setPublic | :651-657 | **P5**：Action::SetPublic + Generic 写回 isPublic（damage.rs notify_when_attacked 写回先例）；ticksToDecay 推 2B（无衰减钟结算） |
| StructureSpawn | canCreateCreep/createCreep（deprecated 对） | :897-940 | **P5 顺手**：官方同形（canCreateCreep=spawnCreep dryRun 判定、createCreep 返回 name） |
| StructureContainer | store/storeCapacity/ticksToDecay | :155-166 | **P4**：ContainerObj typed 变体 + 衰减钟结算 + getter 真化（container 是 bot 主力结构，withdraw 已支持其判定） |
| StructureLab | 读取面（mineralType/mineralAmount/mineralCapacity/cooldown） | :307-318 | **P4 静态折叠**（store_extra 透传折叠；cooldown 无钟恒 0 登记）；**动作族（runReaction/reverseReaction/boostCreep/unboostCreep）推 2B**——boost 生态与战斗系数联动，半面无验收锚 |
| StructureTerminal | storeCapacity/cooldown 读取面 | :708-713 | **P4 静态折叠**；send 推 2B（market 联动） |
| StructurePowerSpawn | power/powerCapacity/energyCapacity | :605-611 | **P4 静态折叠**；processPower 推 M4 |
| StructureNuker | ghodium/ghodiumCapacity/cooldown/energy 族 | :1347-1354 | **P4 静态折叠**；launchNuke 推 2B（Nuke 对象生命周期） |
| StructureFactory | level/storeCapacity/cooldown | :1427-1432 | **P4 静态折叠**；produce 推 2B（commodity 链） |
| StructurePortal | destination/ticksToDecay | :1404-1415 | **P4 静态折叠**（destination 放置面定格） |
| StructureWall | ticksToLive | :839-842 | 折叠恒缺省+登记（墙衰减结算推 2B） |
| StructureRoad | ticksToDecay | :672-676 | **P4 真化**（RoadObj.next_decay_time 已在——投影折叠实现期核对） |
| KeeperLair/PowerBank/InvaderCore | my/owner/ticksToSpawn、power/ticksToDecay/my/owner、level/spawning/ticksToDeploy | :285-291/:585-592/:1480-1485 | **P4 静态折叠**（SK 房/power 生态对象本批不可达，面先齐防 bot 崩） |

### 1.7 其余命名空间

| 命名空间 | 状态/处置 |
|---|---|
| ConstructionSite（6 getter ✅ + remove 🪑） | **P2 转真**（~~对象级~~ **room 级** intent——勘正：官方 construction-sites.js:50-62 走 pushByName('room','removeConstructionSite')，非对象级 Action；RoomIntent::RemoveConstructionSite + 结算摘除+退款） |
| Flag（name/color/secondaryColor/memory ✅；remove/setPosition/setColor 🪫） | **P2 转真**（~~对象级~~ **room 级** intent——勘正同上：flags.js:57-91 三方法均 pushByName('room', 'removeFlag'/'createFlag') 组合；RoomIntent::RemoveFlag + FlagObj 尾追 owner） |
| CostMatrix set/get/clone ✅；serialize/deserialize 🪑 | **P6 真化**（纯 JS 位打包，官方 :40-48 同形；为 FFI 批备好输入形态） |
| PathFinder.search/use | 维持桩（FFI 批）；use 官方 deprecated |
| RawMemory get/set/activate ✅；segments 4 面 🪑（setActiveSegments/getActiveSegments/getSegment/setSegment） | **P6 真化**（官方 runtime.js:122-135/193/250-267：≤10 活跃段/100KB 上限/endTick 带出+宿主持久+下 tick 注入；prelude segmentsBackend 已备） |
| InterShardMemory 2 面 | 不做（M4；官方开源层无对照） |
| Game.shard | 维持我方超集面（官方 game 层无） |
| **PowerCreep 全家（17 getter + 20 方法 + create 静态 ≈38 面）** | **不做（M4 power）** |
| Deposit 4 面 / Nuke 2 面 | 不做（SK 房生态 / nuker 联动，2B/M4） |

### 1.8 数字对账

官方 ≈337 面 ≈ 我方现真面 ~150 + 本批新增 ~80（P1-P6 逐项见上表）+ 不做余量 ~107（PowerCreep 38 / market+terminal 13 / 寻路依赖 6 / lab 动作族 4 / observer+nuker+factory+powerSpawn 动作 4 / deposit+nuke 6 / MMO 订阅面 5 / gpl 3 / wall·rampart 衰减 2 / survivalInfo·worldSize 等登记面）。**「~150→340」的原始表述在本批现实口径下修正为「~150→~230，余量全部逐项归因」**——340 需要 M4 power 与 2B terminal/market 能力先行，属后续批次验收面。

## 2. 搭车登记项定夺（用户指令：方案中逐条定夺）

1. **O2 领域语义模块归属——首片做（P0）**：kk-world 新增 `domain/` 模块族，
   controller_progress_total_opt + controller_levels 表 / store 家族
   （store_positive_entries/apply_store_extra）/ owner 三态
   （controller_owner_value/object_owner）从 snapshot.rs/world_json.rs 纯搬移
   （B1 纪律：零行为，token 级对账——P4 先例）。**本批新增投影折叠
   （ticksToX/cooldown/destination/容量族）一律落 domain 模块**——这就是
   「随 M2B 扩面首片一次性归属到位，免两次搬」的兑现点。
2. **`pub rooms` 收敛——首片做（P0）**：双房批 P2-8/O1 双登记的建议
   （可写面扩大前收敛最省）。形态：`World.rooms` → `pub(crate)` +
   访问器（`rooms()` 只读视图 + 既有 `room()`/`room_mut()`）+ 提交口
   `World::commit_room(name, state)`（**实现期勘正（P0 评审批注）：内部
   insert 但不 bump generation**——「一次成功 tick ⇒ generation 恰 +1」
   的既有不变量（engine 级测试锁定）由调用方必经的 advance_ticks 单点
   承载，commit_room 自行 bump 会令多房 tick 代际超额且破坏
   RoomVersionHandle 身份；失败回退路径不调用本方法）。
   写点适配实测：settle 提交尾 settle/mod.rs:664-677（insert 改经提交口）、
   storage-bench fixtures.rs（读改访问器）、sandbox.rs/settle_tests 多处
   测试直写 time（改经**新增的** `#[doc(hidden)] set_time_for_test`——与
   旧直写逐点等价，不动 player_ticks/generation；advance_ticks 会动
   player_ticks 故不适用）。读面（kk-runtime/kk-engine 的
   keys/get/迭代）全部改走访问器——编译器强制穷尽，漏点零存活。
3. **ObjDir String 的 Arc<str> 化——不做**（后续优化登记维持，双房 O1 口径）。

## 3. 设计要点

### 3.1 「新 getter 写 WorldSource 之上」的本批落法

ADR-013 决策 2 的 seam 纪律在扩面批的具体形态（评估结论）：

- 本批新面的数据**全部流经既有 seam 产物**：prelude getter 消费
  `world_json_for_viewer` 投影（新字段 = kk-world world_json 投影扩展——
  生产侧在 `impl WorldSource for World` 的后端之内）；embed 既有查询面
  （get_object/find_in_room）形状不动。
- **零新 WorldSource trait 方法**（§1 逐项核对：map 族由 prelude 房名算术
  消费既有投影；notify/segments/visual 走 intent/endTick 通道非世界读取）。
  若实现期出现新查询需求：扩 trait，不直连 kk_world::World——本条为评审
  判据（发现直连即打回）。
- kk-runtime/kk-runner 侧新增面（getHeapStatistics/halt op、endTick 的
  visual/segments 字段）均非世界读取，不涉 seam。

### 3.2 typed 变体升级的同步点（m2b-prerequisite §8 全表适用）

TowerObj/LinkObj/ContainerObj 三个新变体走全表：schema 声明（kk_room_object!
  追加尾部，tag 16/17/18——v1 内演化窗口，冻结归 2B 存储批）+ place 归一臂 +
  两投影各一臂 + matches_find 条目 + settle 各面 + movement type_name_of +
  complete_construction 镜像。宏化后机械层=声明一次（P1 宏产 9 块），契约层
  显式 match 由编译器强制。**place 兼容**：既有 Generic 形态放置的 tower/link/
  container（夹具/bot 世界）解码路径不变（tag 区分），新放置走 typed——实现期
  核对 Generic→typed 迁移策略（不做数据迁移，两形态共存至 2B 存储批，登记）。

### 3.3 createFlag/createConstructionSite 转真（P2 核心）

官方四 API 面（Room.createFlag :978-1027 / Room.createConstructionSite
:1029-1097 / RoomPosition 两转发 :1622-1636）。预检族：房名/坐标/配额
（CONTROLLER 结构上限表）/RCL/覆盖冲突（旗互斥工地共存语义）/ERR_NAME_EXISTS
（旗名全局唯一）。intent：RoomIntent 两变体通道已真（intent.rs room 级带
roomName 路由），结算执行器已交付（双房批勘误登记原文「交付的是 intent 通道+
结算执行器，API 面归 M2B 扩面」）——本批补的就是 API 面与预检。返回值：旗名/
site id 官方形态。**overmind 基线形态预告**：其 console 的 stub-flag 报错形态
将消失，三基线重跑须重新归档错 tick 形态（§5）。

### 3.4 map 族真化的语义锚

describeExits/findRoute/findExit/getRoomStatus/getRoomLinearDistance 全部
纯 JS（官方同形：map.js 20-268 逐方法对照移植，房名坐标算术 utils.roomNameToXY
形态）。数据源=投影 rooms 键集（世界全房——不可见房也携地形条目，prelude
gw.world.rooms 即全量表，game.js:347 过滤发生在 Game.rooms 装配层）。差异
登记：出口推导（邻房注册 vs 官方 mapGrid t/b/l/r 标记）；novice/respawn 状态
无数据源恒缺省。

### 3.5 RawMemory segments（P6）

官方 runtime.js:122-135（segments 对象：setActiveSegments≤10 校验/
getActiveSegments/getSegment/setSegment 100KB 上限）+ :193（outMessage.
memorySegments 带出）+ :250-267（宿主侧 10 段/100KB 总量再验）。我方：
prelude segmentsBackend 已在；扩展 endTick 返回（memorySegments 字段，
同 intentsList 通道模式）→ RunResult 扩字段 → 统一提交点持久
（PlayerState.segments）→ 下 tick begin 注入。跨 tick 语义对齐官方
（未 set 段保留旧值）。

### 3.6 RoomVisual/MapVisual（P6）

官方 rooms.js:1138-1220 / map.js:271-380：方法收集绘制指令（链式 this）、
getSize 常量、export 序列化/import 还原。真化=收集语义+endTick visual
通道带出（console.addVisual 官方等价物）——**消费归 F2 渲染线**（合并期
对齐，P3 地形协议同款并行线协调条款）。玩家可见行为=链式/异常面/size/export
形态对齐；不渲染不违约（官方单机版同样只入报文）。

## 4. 切片划分

| 片 | 内容 | 主落点 | 量级 |
|---|---|---|---|
| P0 | O2 领域模块归属 + pub rooms 收敛（§2）+ 方案入库 | kk-world（domain/ 新模块、world.rs、snapshot.rs/world_json.rs 搬移）、settle/mod.rs、storage-bench、sandbox.rs | 小（纯结构） |
| P1 | Creep 尾巴：rangedAttack/rangedMassAttack/getActiveBodyparts/notifyWhenAttacked 转真 + saying 真化 + CREEP_METHODS 桩表清理 | prelude/creep.js/creep_resources.js + saying 投影字段（world_json） | 小 |
| P2 | 创建族与 RoomPosition/Room 尾巴：createFlag/createConstructionSite 四面转真 + Flag.remove/setPosition/setColor + ConstructionSite.remove + findClosestByRange + findPath 直线桩 + room.storage/terminal 回填 + Room.toJSON（findExitTo 顺延 P3——map 依赖，见 §8） | prelude/room.js/room-position.js/sites-flags.js + intent.rs（**room 级** 2 新 RoomIntent + 白名单——§1.7 勘正）+ settle 执行器（createFlag 挂主/removeFlag 域内删/removeConstructionSite 门+退款 + 组序排序） | 中 |
| P3 | Game.map 族（describeExits/findRoute/findExit/getRoomStatus/isRoomAvailable/getRoomLinearDistance）+ Game.notify + cpu.getHeapStatistics/halt + cpuLimit | prelude/game.js/room.js（map 段）+ ops.rs 两 op + intent global 通道 + PlayerState.notifications | 中 |
| P4 | 结构读取面：ContainerObj typed 变体（含衰减结算）+ Generic 静态折叠 getter 族（Lab/Terminal/PowerSpawn/Nuker/Factory/Portal/KeeperLair/PowerBank/InvaderCore/Road.ticksToDecay）+ Resource.energy + 折叠函数归 domain 模块 | schema/room.rs、place、两投影、settle、prelude/structures*.js | 中大 |
| P5 | 结构动作族：TowerObj/LinkObj typed 变体 + tower.attack/heal/repair + link.transferEnergy + rampart.setPublic + spawn.canCreateCreep/createCreep | schema、place、settle（towers/links 执行器+冷却/耗能）、两投影、prelude | 大 |
| P6 | 收尾：RawMemory segments + RoomVisual/MapVisual + CostMatrix.serialize/deserialize + 棘轮地板上调 + 灰区表/架构文档回写 + ADR-012 关联面核对 | prelude/game.js、90_host.js、path-finder.js、RunResult/PlayerState、compat/reports | 中 |

依赖：P0 先行（domain 模块是 P4 折叠函数的落点；rooms 收敛是 P4/P5 结算写面的
前提）。P1/P2/P3 相互独立可序贯。P4 先于 P5（读取面+Container 变体先立，
tower/link 复用其投影/折叠模式）。P6 收尾。**每片一个完整批次循环**（§5），
单一归因提交。

## 5. 验证矩阵（每片完整批次循环）

1. 写规格（片内逐面官方 file:line 对照表随实现落注释——重写而非拷贝纪律）；
2. `cargo test --workspace` 全绿（V8 新测试并入 embed_tests 串行链：根唯一
   `#[test]`、子模块全 0、链体追加不动既有序）；
3. **显式 `cargo build --release` 后**跑棘轮：1405 零未豁免转红（转绿逐用例
   归因；P2/P4/P5 预期显著转绿——地板上调落本片提交或 P6 集中上调，唯一
   真相源 ratchet.json）；跑完 `git checkout -- compat/reports/kkscreeps.json`；
4. bot 三基线：tooangel 2000t console 零错**人工确认**（map 族/createFlag
   真化会改变其行为路径——远征/旗标面重点盯）；overmind 2000t——**P2 后
   stub-flag 既知形态将消失、P6 后 setPublicSegments 形态将消失，错 tick
   形态须重新取证归档**；wasm-bot 10t；skipped_penalty 全 0；
5. perf 同口径 A/B 消融（基线 2p=0.124/0.209/0.032/0.467，绝对值不跨会话，
   >20% 先消融机器态，settle 相金丝雀——P5 tower/link 冷却结算与 P4 容器
   衰减会动结算面，预期 settle 微升须归因；P4 投影新字段动 project 相同理）；
6. ≥3 路 subagent 并行评审（并发 ≤6；视角=官方语义对照/Rust 正确性/
   测试锁值充分性）→ 逐条清偿 + 清偿复审（**复核评审共识本身**——「返回
   本体还是拷贝」类形态主张须 mock 复刻控制流实测取证）；
7. roadmap/本方案 §8 回写 → `pnpm run check` → 单一归因提交
   （subject ≤350 字符且冒号前 ≤48）并 push。

外部复审惯例：批收口后请外部 AI 复审 HEAD（diff 区间/评审视角/file:line
要求/验证命令式提示词）；修复面极小且多层锁定时可提请拍板条件免除
（先例 0edbe5c/08302c1）。

## 6. 不做清单（显式出界，评审按此判）

| 项 | 理由与归因 |
|---|---|
| PowerCreep 全家（~38 面）+ gpl + processPower + InterShardMemory | M4 power（roadmap 既有排期） |
| Game.market 全家 + terminal.send + observer.observeRoom | 2B 能力批（terminal/远视注入/订单簿结算依赖）；本批 notify 先行验证 global intent 通道 |
| lab 动作族（run/reverse/boost/unboost） | 2B：boost 矿物生态与战斗系数联动，半面无验收锚；读取面本批 P4 齐 |
| nuker.launchNuke + Nuke 面、factory.produce（commodity 链）、Wall/Rampart 衰减钟 | 2B 能力批 |
| PathFinder.search 真语义 + findPathTo/findClosestByPath/moveTo/findPath 真寻路 | kk-pathfinder FFI 批（D4 排期 2B 首批；直线桩维持+灰区登记不冒充） |
| Deposit 面 | SK 房生态（2B/M4） |
| typed FIND、seam 收窄、InfraFailure 持久化 | 既有显式决策维持（用户指令 + 双房批 §5） |
| Game.cpu.generatePixel/unlock/setShardLimits、getWorldSize、survivalInfo、Game.shard 收敛 | MMO 订阅面/世界模型差异/我方超集——维持现状+登记 |
| 列式布局/零拷贝、并行调度、存储 | 2B 主体（本批仅 §2 两件结构收敛） |
| 不动 packages/ 与 .worktrees/renderer/（F2 并行线）；不改 snapshot 契约字节形态（schema v1 冻结归 2B；本批变体追加走 v1 内演化窗口，B3/B4 先例） | 边界与隔离（用户指令） |

## 7. 风险与已知坑（本批专项）

1. **P2 行为面放大**：createFlag/createConstructionSite/remove 族转真改变
   bot 可写面——tooangel/overmind 基线形态变化须逐条重新归档（§5-4 预告），
   不可拿旧形态当回归口径。
2. **P4/P5 变体升级**：走 §8 全表（m2b-prerequisite）逐点核对；Generic 与
   typed 两形态共存期的投影一致性（同一 structureType 两种 tag——放置面与
   结算面的判定序）实现期专项；`complete_construction` 与 place 镜像（#29
   登记面）随变体接线。
3. **公式等价测试值落在分歧窗口内**（双房 P0 教训）：tower 伤害距离衰减/
   link 冷却/容量边界/findRoute cost 的锁定测试须选官方与我方实现分歧的
   取值域（边界值+窗口内值双覆盖）。
4. **P0 rooms 收敛的编译器穷尽**：pub(crate) 后全部外 crate 访问点编译期
   显形——逐点改访问器，禁止为图省事开 pub 拉回；`World::reset` 跨分片
   撞键防御注释（P2 前置批登记）随搬移保持。
5. **map 族房名算术**：roomNameToXY 的 W/E/N/S 半开区间语义（官方 utils
   精确对照）；越界/非法房名返回形态逐方法对照（null vs undefined）。
6. **segments/visual 的 endTick 通道扩展**：与 intentsList 同模式（一次性
   结构化带出）；上行闸门面评估（segments 100KB 官方硬限即天然闸）。
7. 工具链坑全表适用（记忆清单）：python 补丁锚点含 `#[test]` 属性行 /
   cargo fmt 重排破替换锚点 / zsh `===` 与词分裂 / ratchet 前显式 release
   重建 / 跑完还原 kkscreeps.json / commit subject 限制 / V8 测试串行链。
8. **世界版本纪律**（P2 前置批钉）：P0 收敛后整房写回经 commit_room（自身
   **不 bump**——见 §2 勘正），代际仍由 advance_ticks 单点承载；「直写
   rooms 后必经 advance_ticks」的旧纪律由可见性收口强制（跨 crate 写面
   仅剩 commit_room/register_room/room_mut，各有其 bump 纪律）。

## 8. 执行记录（随片回写）

### P6（收尾：RawMemory segments + RoomVisual/MapVisual + CostMatrix + creep 存活门清偿）——已收口（含三路评审清偿）

- **范围与官方基准**：driver/lib/runtime/runtime.js:122-200（segments 五
  API）+ runtime.js:250-269（tick 尾回收校验；评审 2 勘正——初记
  make.js，该处实为 publicSegments env.set）；game/console.js:57-108
  （visual 存储四函数）；game/rooms.js:1144-1218（RoomVisual）、map.js:
  275-374（MapVisual）；game/path-finder.js:18-47（CostMatrix）；套件
  25-memory/31-visuals/COSTMATRIX 族。
- **CostMatrix 真化**：原桩三处全非官方——索引反置（y*50+x，官方
  **x*50+y**）、serialize 返回 {bits:2500}（官方是 **Uint32 视图 625 元素
  普通数组**）、无 clone/set 钳 0..255/公开 _bits 面。逐字重写
  （path-finder.js）；COSTMATRIX-003/004/005/007/008 + UNDOC-_bits 三条转绿。
- **RawMemory segments 全链**：①prelude 五 API 逐字（ setActiveSegments
  的 10 段/id 0-99/非数组 **throw**（官方文案）；setActiveForeignSegment
  (null) 清除；未知名不 throw 下 tick 无交付）；②**后端在协调器
  PlayerState**（segments 全量 100 槽 + active_request/public_ids/
  default_public/foreign_request 跨 tick 持久——官方 db 持久
  （users.activeSegments 等）的等价物；**官方 runtime 闭包每 tick 重置**
  （make 侧每 tick 重跑 _start）、「闭包同形」为评审 2 P2-2 勘误的
  失实前提）；
  ③管线：宿主新通道 setMemoryState（raw+窗口+foreign，与 set_raw_memory
  同时序约束）注入、endTick 信封 segmentsOut（窗口写回+四请求**全量带出**）
  带出、commit 合并；④tick 尾校验（≤10 槽/0-99/字符串/≤100KB）违例整轮
  runtime error（官方 make throw 同义）；⑤foreign 授权解析每 tick 重跑
  ——显式 id 须在公开集；**默认段同样须在公开集内（FOREIGN-008：残留
  default 不构成授权——实现期抓到的真 bug，探针取证后修）**；⑥未写过的
  请求槽物化空串（RAWMEMORY-003「请求即出现」）；⑦publicSegments 空列表
  no-op（官方 truthy-check 弃空 quirk，FOREIGN-008 注释实证）；⑧五个
  非官方桩面（getActiveSegments/getSegment/setSegment/setInterShardMemory/
  getInterShardMemory）撤除（官方无此面，bot 实测零消费）。
- **Visuals**：visual 存储（console.js:57-108 逐字——每玩家按房名累积
  JSON 行字符串；map 1000KB/房 500KB 超限 throw 官方文案）落 prelude
  gw 桥（room.js）；**每 tick 重置**（官方后端每 tick getVisual drain 同
  语义——绘制只活在当 tick）；RoomVisual 六方法逐字（坐标/位置双形、
  poly 位置数组归一、import 字符串原样）；MapVisual 惰性单例（世界坐标
  RoomPosition 强制+房名 n/n1/n2、style 缺省 {}）；sharedBuffer=存储键
  共享（同房两实例天然同缓冲）。console.addVisual 玩家面不挂（官方
  fakeConsole 有——套件不消费，登记）。
- **creep 战斗族存活门清偿（P5 登记-销账勘误）**：P5 评审曾登记「官方
  套件无 UNDOC-STALEARG creep 面用例」——**误**：套件有五条
  （creepAttackCreep/creepRangedAttack/creepHeal/creepRangedHeal/
  creepRepair）。P6 顺手清偿：isKnownCreep/isKnownStructure 提 gw 桥
  （structures_spawn.js 定义、creep.js 五 API 接线——官方 creeps.js:607/
  :640/:689/:717/:748 的 register 判定同形）。
- **验收与验证**：e2e 守卫（segments 激活/写回/10 段/越界/100KB±1/
  foreign 撤回-清除-未知名-未写槽（全矩阵主体在套件层 FOREIGN 族）/
  visuals 记账-sharedBuffer-房 500KB 上限-clear（map 1000KB 在套件层
  VISUAL-SIZE-004）/CostMatrix 往返-clone-钳-_bits）；workspace **322 绿**/
  clippy 0/fmt 净；棘轮 **1774（P6 累计 +32：segments 13/visuals 8/
  CostMatrix 6/creep 存活门 5）、转红 0**；地板 1742→**1774（60.88%，
  破六成）**；bot 三基线重跑（**overmind setPublicSegments 形态消失——
  P3 起的 2000 错/tick 基线归零**，tooangel 形态 4→3，均零新增形态）；
  perf 复跑（首轮 bots 后机器态偏高，冷却回落；工件随批刷新入库）。
- **三路评审与清偿（全 PASS with conditions 零 P0，当日清偿）**：
  评审 1（官方语义）/评审 2（Rust 正确性）/评审 3（测试与归因）。
  **P1 三项**：①setActiveSegments([]) 清空（1/2 同报——原 emit 带
  length 门成 no-op；官方 runtime.js:287 无守卫 emit、[] truthy 经
  make 侧 $set 清库；**与 public 的 [] no-op 是两个官方形状**——public
  经 join(',') 变空串才被 truthy-check 弃）改 `!== null` 门+e2e 锁定；
  ②tick 尾三文案逐字化（`"X" is not a valid memory segment ID`/
  `Memory segment #X is not a string`/`Memory segment #X has exceeded
  100 KB length limit`——runtime.js:259/262/265，grep 实证后改）；
  ③overmind 剩余错误归因勘正（评审 3 五帧栈对齐 bundle——
  DirectiveHarvest.create 的 log.debug 读 `Memory.flags[result]`，col 76
  是计算键非 lookFor 调用；overmind bundle 无 Memory.flags 初始化、
  首 tick 空 Memory 必抛，**官方服务器大概率同形**——不归 room-look 批，
  台账两处已改）。**P2 顺手修四项**：foreignSegment 未写公开槽 data=
  null（官方 hget 缺失 null——与活动槽 `|| ""` 刻意不对称）+e2e；
  CostMatrix.set 去多余 return this（官方无返回值）；commit_segments
  的 as u8 截断防御（0..=99 门——畸形值截断成 0 会静默污染 0 号槽）；
  四处「官方闭包同形」失实注释勘正（game.js/settle mod.rs/embed.rs——
  P1① 的根源正是该错误类推，评审 2 P2-2）。**P2 登记不修**：校验顺序
  quirk（官方先 >10 再逐 id）、中途 throw 前缀残留 quirk、falsy-0 官方
  bug 形态刻意保留（id 0 正常交付）、foreign 默认段解析时点（官方 API
  调用 tick 钉死不重解析）、MapVisual 跨 tick 身份、assertTargetObject
  五处既有面（随后续批）、powerCreeps 臂（M4）、username 无界。均落
  compatibility.md。清偿后全门禁复跑：workspace 322 绿/clippy 0/fmt 净/
  棘轮 1774 零转红（+32 归因不变——修复零用例位移）/三基线与 perf 以
  清偿后二进制重跑刷新入库。
- **收尾插曲（夹具完整性事故，当日恢复）**：清偿后重刷 tooangel 基线
  发现 2000 tick 全错 `Unknown module 'main'`——排查：夹具是未跟踪的
  TooAngel/screeps clone（HEAD 87645a0，clone 本身 git status 干净）+
  **根目录 src/*.js 扁平拷贝**（90 文件，screeps 官方部署形态——require
  扁平命名空间，`./require` 解析为模块名 `require` 而非 `src/require`；
  图根 shim `require('./src/main')` 方案因此不可行），扁平拷贝被会话外
  进程（评审期）删除且**未跟踪文件不进 git status** 故静默。恢复
  `cp src/*.js .` 后 30 tick 冒烟（stub 流/两条 console 形态对上）→
  2000t 全量：0 错/0 中断/形态 3 与旧基线逐字全等。夹具根留
  KKScreeps-FIXTURE-NOTE.md（恢复命令+形态说明）。**教训登记**：
  compat/fixtures/ 整目录未跟踪是单点风险（overmind-dist/wasm 同理），
  基线异常先查夹具完整性；评审/外部进程动过机器后未跟踪工件须核验。
  另登记（清偿复审 P2-1）：tooangel 基线存在**非确定性噪声底**（bot 侧
  Math.random/CPU 桶——两次重跑可有 ±2 个偶发形态/主形态计数 1999↔2000/
  stub 计数 ±4 漂移），基线 diff 判定按「典型形态集+计数容差」口径，不逐字。

### P5（结构动作族：tower 三动作 + link.transferEnergy + rampart.setPublic + spawn deprecated 对）——已收口待评审清偿

- **范围与官方基准**（逐文件实读 ~/Desktop/projects/sources/screeps/）：
  towers/attack.js、heal.js、repair.js、intents.js、tick.js；
  links/transfer.js、intents.js；ramparts/set-public.js；
  game/structures.js Tower.attack/heal/repair（:764-812）、Link.transferEnergy
  （:488-531）、Rampart.setPublic（:652-657）、Spawn.canCreateCreep/createCreep
  （:898-1051）；processor.js:227-330 分派序；utils.js checkStructureAgainstController
  （:456-505）；game/names.js（createCreep 自动名）。
- **关键勘误（对 §4 表）**：①官方**无 TOWER_COOLDOWN**（常量表无此键、
  towers/tick.js 只持久化 actionLog 不递减）——塔限速=TOWER_ENERGY_COST 能量 +
  intents.js:3-8 的 **else-if 单动作序（heal>repair>attack）**；②§4「距离无损」
  勘正：transfer 损耗 `ceil(amount×LINK_LOSS_RATIO 0.03)` 恒发生 + 冷却
  `+=LINK_COOLDOWN(1)×本格坐标 Chebyshev 距离`，且 API 面**同房限定**
  （跨房 ERR_NOT_IN_RANGE，structures.js:526-528）。
- **形态决策（P4 镜头延续）**：tower/link 继续 Generic 透传（结算写面=
  data JSON 字段级写回，link 冷却递减先例已在；TowerObj/LinkObj typed 化
  无玩家可见收益，偏差登记更新）。
- **通道**：复用 Action::Attack/Heal/Repair/Transfer（官方白名单同形
  {id,x,y}/{id,amount,resourceType}，按对象类型路由到不同执行器=官方
  processor.js 同构）；新增 Action::SetPublic（{isPublic:boolean}，
  system.js:72）。settle 阶段 5 加 Generic 分派臂（所有权门=官方
  `object.user && != userId` 跳过——**无主对象不跳**，官方同形）。
- **tower 三执行器**：attack=官方守卫链（塔+store/目标在/spawning creep 拒/
  hits>0/能量≥10）→ rampart 同格重定向（复用 rampart_at）→ 距离衰减
  （power 600/400/800、optimal 5、falloff 20/0.75、range 钳 20、floor）→
  apply_damage（推广为 creep/Generic 攻击者双形态——creep 目标延迟
  damage_to_apply/结构目标立即+击毁废墟；P1 模型复用）→ 扣 10 能量。
  heal=creep 目标+延迟 heal_to_apply（同 tick 阶段 8 落地=官方
  creeps/tick.js:125-127 序）+EVENT_HEAL(healType=RANGED)；repair=
  CONSTRUCTION_COST 门（可修结构）+hits<hitsMax+EVENT_REPAIR。
  actionLog/受击通知/sendAttackingNotification 维持 P1/P4 登记（无驱动管线，
  Generic 透传写 actionLog 会泄漏到玩家投影面——官方剥离，我方不写=玩家面
  等价）。
- **link transfer 执行器**：守卫链（link/store.energy≥amount/amount≥0/
  目标是 link/源 cooldown>0 拒/**checkStructureAgainstController（Rust 移植
  :456-505——含同类型更近者配额计数与等距先扫到算更近的 id 序并列规则）**/
  目标无容量键或满仓拒）→ amount 钳目标余量 → 双侧记账+3% 损耗 →
  cooldown += 距离 → EVENT_TRANSFER。amount=0 官方奇行保留（仍挂冷却）。
- **rampart.setPublic**：执行器 `isPublic: !!intent`（Generic data 写回）；
  放置面补 rampart isPublic:false 缺省（vanilla getStructureDefaults——
  同时清偿 P4 登记的快照契约面 isPublic 缺口）。
- **spawn deprecated 对**：canCreateCreep=官方 :898-929 独立预检序（!my→
  BUSY→body 校验→room.energyAvailable→off（raw 回查）→NAME_EXISTS→
  名长>100）；createCreep=:932-1051（memory 移位参/预检透传/names.js
  自动名 100+100 名池逐字移植/Memory.creeps 显式写/幽灵壳/spawning 同 tick
  Game.creeps/intents.set 'createCreep' {name,body}/**返回 name**）。
- **验收**：settle 家族（衰减公式三分支/重定向/延迟序/else-if 单动作/
  transfer 全链/setPublic）+embed e2e（API 错误码全表/跨房/自动名/
  同 tick 读回）+五重验证+三路评审。
- **落地补记（实现与规格的差异/勘正）**：
  ①**tower API 的 RCL 门用 `!this.isActive()`**（官方直调
  checkStructureAgainstController）——四门逐门等价（无主/表外/无房无
  controller/距离计数），野房无 controller 分支官方 `data(undefined.id)`
  抛错不可达、我方走 ③ 门返 RCL 错（合意外延）；②**结构 API 目标门补
  register 存活判定**（isKnownStructure/isKnownCreep——陈旧壳拒绝，
  UNDOC-STALEARG 面；首跑棘轮 3 条转红即此，补后归零）；③**spawnPair
  Memory 懒触发勘正**：原 spawnCreep/createCreep 直调 gw.ensureMemory()
  强制重解析+重固化，把玩家同 tick 早先的 Memory 写入清掉——官方读
  globals.Memory（getter 首访解析后固化为值属性，同 tick 后续读写同一
  对象）；改 `globalThis.Memory` 后 e2e 锁定「Memory.creeps.X 同 tick 可见」；
  ④spawnGhostCreep 抽取共用（spawnCreep 原内联块=createCreep 壳，逐字段
  同形）；⑤rampart isPublic:false 放置面缺省（P4 快照契约缺口清偿）。
- **实现期取证沉淀**：同名 intent 后写覆盖先写（官方 intents.set 同语义
  ——同 tick 两座 spawn 各自提交互不干扰、同对象同名覆盖）；RCL 配额在
  测试世界构建时的干扰（rcl5 spawn 配额 1——第二座 spawn 的 createCreep
  被 isActive 结算门拒，测试世界须 rcl7）；link 冷却挂钟当 tick 即被
  phase 8 递减（官方 intent 期挂钟→phase 8 减的净效果——e2e 冷却净读按
  结算 tick 数推算）；**f64 衰减公式官方 JS 是减法赋值、我方一次性表达式
  ——逐点数值核对（600×0.75×k/15 全整数族）无分歧**。
- **验证**：workspace **322 绿**（+12 settle 家族——含清偿补 3 测）/clippy 0/fmt 净；release
  重建后棘轮 **1742、转红 0**——**口径勘正（评审 1/3 双报）：本批真实增量
  =HEAD→工作树 +116**（地板 1626→1742），棘轮门禁自身的「转绿 149」是
  HEAD~1（P3 地板 1593）→工作树口径（含 P4 已提交的 +33）；本批 +116 分布
  StructureLink 57/StructureTower 46/StructureRampart 5+8（PROTECT 族与
  intent 序）/Tower 5/SAFEMODE-COMBAT/STRUCTURE-ACTIVE-002/ROOM-EVENTLOG-002
  等（Store 9/adapter 7/Road 5/Container 3 经实证全在 HEAD 已绿——属 P4
  交付面，评审 3 逐条抽查归因干净）；**首跑 3 条转红（UNDOC-STALEARG
  tower 三条——目标存活判定缺失）当轮修复归零**；地板 1626→**1742
  （59.78%）**；
  bot 三基线（tooangel 2000t 0 错误/overmind 形态集与基线全等 10=10/
  wasm 干净——中断 0 跳过罚 0 缺失 {}）；perf 四相 0.127/0.222/0.032/
  0.487 持平（对照 P4 收口 0.125/0.217/0.031/0.480——phase 5 新增 Generic
  分派与 check_structure_against_controller 仅在结构 intent 提交时触达）。

- **三路评审（官方语义对照/Rust 正确性/测试与归因）均 PASS with conditions，
  零 P0；清偿当日完成**：
  ①**link 自指转移收敛**（评审 2 P1-1，行为修复）——官方三步序对同一引用
  净收敛 E−ceil(3%)、我方独立写回覆写为 E−amount；`target_id==id` 分支按
  官方序收敛 + 测试 `link_transfer_self_target_converges`（397/冷却 0）；
  ②**reject_gates 三拒因断言空转修复**（评审 3 P1）——双根因：Fixture
  controller 只 insert 不替换 + Working::controller() 按 ObjId 升序取首个
  → 同 Fixture 二次 f.controller 形成 rcl4 阴影（配额门接盘全部拒因）；
  且满仓用例源能量 0 在能量门即拒。重写：rcl4/rcl8 分 Fixture + 三个拒因
  各自独占（目标满门全仓库首次获得结算层判别覆盖）；
  ③补三测：repair 钳 hitsMax/满血拒（评审 3 P2-2——全仓库首次覆盖）、
  heal>repair else-if 半边（P2-3，in-repo 补；全序另由 compat
  TOWER-INTENT-002 外部锁定）、pending_order 注释勘正（纯顺序反转数值
  同 100——真实判别面是「伤害-治疗间夹死亡判定」形态，评审 3 P2-1）；
  ④四登记：creep 战斗族 UNDOC-STALEARG 剩余面（isKnown* 提共享 helper 归
  后续批）、tower 自指判等方向（我方保守拒绝）、roomInfo.lastPvpTime 并入
  通知跳过行、isKnownStructure 域注释（source/mineral 靠 instanceof 合取
  排除）；⑤**代码健康**：checkStructureAgainstController 前奏三行与
  extractor_active 抽 `structure_rcl_prologue` 单源（评审 2 P2-2）、
  spawnCreep 注释归属勘正（Memory 写在 API 层 structures.js 非结算层，
  P3-N1）；⑥plan 棘轮口径勘正（+116/+149 双基线写清）。

### P4（结构读取面：setup 契约 + 衰减结算三件 + Generic 折叠 getter 族 + Resource.energy）——已收口

- **落地**：①place 通道（place_structure.rs）——Generic 臂收
  ticksToDecay/cooldown 相对值折算绝对钟（nextDecayTime/cooldownTime
  =time+N；缺省窗对齐官方 tick.js 重置窗：container 有主房 500/野房
  100、rampart 100）、road 臂 spec 覆写、container/factory 平面
  storeCapacity 缺省（官方 DB 形态）；②快照（snapshot.rs Generic 臂）
  ——绝对钟折叠差值（remaining 家族/cooldown `>time?差:0`），绝对键
  移除；③prelude（structures_spawn.js）——11 类静态折叠 getter 表
  （container store/storeCapacity/ticksToDecay、rampart 衰减钟、
  link/terminal/lab/factory cooldown、lab 矿物双 getter、powerSpawn/
  nuker 能量族、portal destination 双形态/keeperLair/powerBank/
  invaderCore、road/wall ticksToDecay）+Spawning 经 gw 桥复用+
  Resource.energy 别名；④投影（world_json.rs）——energy 堆补
  energy=amount 别名键；⑤结算（economy.rs）——衰减三件
  （roads/containers/ramparts tick.js 逐字：地形系数窗/rampart hitsMax
  rcl 同步/容器死亡 hits 先减后落地——同格承接门拒收官方次序）。
- **偏差登记（compatibility.md）**：ContainerObj typed 变体以
  Generic+绝对钟交付（官方 DB 同形——typed 化无玩家可见差异，P5
  tower/link 评估同镜头）；domain 折叠函数单消费者不迁移。
- **实现期取证**：M1 期夹具缺衰减钟（官方 DB 不变量：container/
  rampart 恒带钟——缺钟=官方立即衰减）曾致 5 处回归，按不变量补钟；
  死亡掉落与同格承接的次序依赖（hits 先减使承接门拒收）；getObjectById
  对不可见房恒 null（野房容器默认值走快照通道断言）。
- **验证**：workspace 305 绿（+6：引擎衰减三件；embed 1 场景——setup
  契约/折叠 getter/死亡落地全链）/clippy 0/fmt 过/release+棘轮
  **1593→1626（+33：实现批 +20、复审修复 +5、清偿修复 +5、主线复核补
  +3[STORE-BIND-002 H/O/G]，零未豁免转红）**——setup 契约 7（覆写×3/默认×3
  /link cooldown）/衰减结算 10（CONTAINER×3/ROAD×5/RAMPART×2）/
  STORE-OPEN×2/SHAPE-RESOURCE-001；地板上调 1626+README 55.80%+报告随批
  入库；bot 三基线（tooangel console 零错/overmind 既知形态复验——
  衰减面零新增错误/wasm/skipped_penalty 全 0，三判定通过）；perf 四相
  持平（两跑 0.127/0.228/0.033/0.496、0.126/0.222/0.032/0.488——对
  P3 日内 0.125-0.126/0.220-0.222/0.031-0.032/0.483-0.484；settle
  金丝雀 0.032 稳——phase 8 衰减分派对 container/rampart 增一次
  Generic data JSON 解析，当前量级噪声内）。
- **提交前多路独立复审（官方语义对照 / 测试判别力 / 跨层键名一致性）修复 4 项**：
  ①**建造落成入口漏建钟**——官方 build.js 落成即挂 nextDecayTime（rampart
  :167、container :261）并写平面 storeCapacity；初版只覆盖放置入口，而
  phase 8 把缺钟（nextDecayTime=0）判为「已到点」，建成 rampart
  （hits=RAMPART_HITS=1）在**落成同一 tick** 被 300 打光移除、container
  首 tick 误掉 5000。修复：build 落成臂复用
  `kk_world::place::{built_decay_clock, built_store_capacity}`（两入口单源）。
  ②**rampart 的 hitsMax 同步位置**——官方在衰减钟门**之外**每 tick 执行
  （ramparts/tick.js:19-24 在 :26 的门之前），初版放进门内 → rcl 变化要等
  下一个衰减窗才生效；同时改回官方精确查表（原「向下取最近键」与
  damage.rs 自查不一致）。③**link 的 cooldown 形态**——官方两形态
  （vanilla index.ts:1145-1158）：link/extractor 用每 tick 计数器
  `o.cooldown`（getter 直读），lab/terminal/nuker/factory 才用 `cooldownTime`
  折算；初版对 link 也折 cooldownTime（注释误套 lab 式）。④**wall 撤除官方
  不存在的 ticksToDecay**（官方 structures.js:839-841 只挂 ticksToLive），
  road 补官方 decayTime 兜底分支。
  **验证**：守卫测试 +4（建造建钟 rampart/container、hitsMax 门外同步、
  road 重挂钟）+ container 落地断言强化为逐键校量（原仅校「存在一个堆」）；
  变异实验（把 hitsMax 同步移回门内）确认唯一转红的是新守卫——该分歧点
  此前无任何测试覆盖。workspace 绿（0 failed）——**数口径勘误**：本轮复核
  以 `cargo test --workspace | grep '^test result:'` 逐行求和实测，加本轮
  4 条新守卫前为 **306**（原记 309 未复现，按实测更正）。
- **清偿复审（对上述修复自身的独立复核）修复 3 项 + 补 1 项**：
  ⑤**容量平面/per-resource 未成对**——`Store.getCapacity()` 无参是
  `storeCapacityResource ? null : storeCapacity || null`（store.js:31），
  上轮只给 lab/container/terminal/factory/powerSpawn/nuker 补了平面
  `storeCapacity`，lab 的 `getCapacity()` 遂由官方 null 变 5000。修复：
  新增 `built_store_capacity_resource`（lab/powerSpawn/nuker/link/tower），
  放置与建造**两入口成对落**；平面表补 link/tower 对齐 vanilla
  `getStructureDefaults` 权威表（官方 build.js 是它的子集，差异不可观测）。
  ⑥**装配兜底遮蔽 energyCapacity 的取数**——实例级 `ELSE_STRUCTURE_FIELDS`
  会遮蔽 `StructureLab/PowerSpawn/Nuker/Link/Tower.prototype` 的同名 getter，
  且原读**原始键** `o.energyCapacity`（官方 DB 从无该键）→ 该五类的
  `energyCapacity` 恒 0（官方 lab 2000/link 800/tower 1000/powerSpawn 100/
  nuker 300000）。方案 §4 P4 行本已声明 PowerSpawn 的 `energyCapacity`
  交付，修前该声明并不成立。修复：兜底改读官方口径
  `storeCapacityResource.energy`（无平面回退分支）。⑦**link 冷却无结算面**
  ——place 面已按计数器形态落键，但 phase 8 无递减（官方 processor.js:378
  `links/tick.js`）；旧 e2e 只在放置同 tick 读一次（恒 7），恰把「不递减」
  读成预期值。修复：`economy::link_cooldown_tick` + phase 8 分派（Generic
  按 `structureType` 取，官方按 `type` 取——我方透传桶 `type` 恒 "structure"）。
  补 1 项：build 落成臂补 `storeCapacityResource` 与 nuker 的 `cooldownTime`
  （官方 build.js:269-272）；官方另落的 `store:{energy:0}` 与 link/lab/factory
  的 `cooldown:0` 经核对**读取面等价**，不落并写明理由。
  **验证**：新增守卫 5 条（settle 4：lab 容量对、nuker 冷却钟、link 递减、
  link 钳 0；e2e 3 条断言并入既有场景）＋**四条变异实验逐条确认杀伤力**
  （兜底取数回退 → 0 vs 2000；link 分派摘除 → 7 vs 5/6；per-resource 摘除
  → null vs 2000 与 5000 vs null）。workspace **310 绿 / 0 failed**；棘轮
  **+5 转绿 1618→1623、转红 0**。**读数勘误**：本批初记「计数不变、维持
  1618」——系陈旧 release 二进制所致（棘轮前置 `cargo build --release -p
  kk-cli` 未随批重建，`compat/scripts/run-suite.sh` 头注已明列）；第三轮
  复核以逐条消融实证（装配兜底/link 分派 2 项占 +1、容量成对落与 build
  臂占 +4），并以二进制 md5 核对构建确实随代码更新。
- **bot 三基线复跑（本轮补做，含一处基线陈旧发现）**：`compat/reports/`
  三条基线最后一次入库分别是 **tooangel/wasm 停在 A6（2026-09-29）、
  overmind 停在 P2**——即 P1/P3/P4 各批记录里的「三基线通过」用的是
  **未入库的本地跑**（默认 report 路径 `bot-check-report.json` 未跟踪），
  入库基线因此陈旧三轮。本轮按基线口径重跑并刷新入库：
  tooangel（`fixtures/tooangel/src` 2000t）**2000/0 中断/0 跳过罚/0 console
  错误/缺失 {}**；overmind（`fixtures/overmind-dist` 2000t）2000/0/0/**2000
  console 错误——归一化比对与旧基线集的**错误类型集全等**（唯一条数 10=10，
  首条仍是已知的 `RawMemory.setPublicSegments is not a function` ×2000，归
  P6），无新增错误类型**/缺失 {}；wasm（`fixtures/wasm-bot` 10t）10/0/0/0
  错误/缺失 {}。三判定（中断/缺失/跳过罚）全过。
  - **轨迹位移的消融归因**：刷新后 tooangel 的 `stub_calls_top` 相对陈旧
    基线整体位移（`PathFinder.search` 10000 登顶、room.find 行号集改变）。
    做消融（回退本轮两条 bot 可见改动——`ELSE_STRUCTURE_FIELDS.energyCapacity`
    取数与 link 冷却分派）后重跑，`stub_calls_top` 与错误集**全等**
    （唯一差异是首行 `Load: 5/6` 的模块加载计数噪声）→ **位移归因于 A6→P4
    的累计引擎变化，与本轮清偿修复无关**。
  - **残留登记**：入库 bot 基线自 A6 起实则未随批刷新（P1/P3/P4 三轮），
    属「验收声明与入库证据脱节」——建议把三基线刷新纳入各批收尾清单。
- **perf 观测点复跑**（`perf-probe --ticks 1000 --warmup 100 --players 2`）：
  四相 p50 project 0.1254→**0.1244**、run 0.2087→**0.2099**、settle
  0.0322→**0.0309**、wall 0.4665→**0.4610** ms——**持平**（对照入库基线
  `compat/reports/perf-probe.json`，B0-6 口径）。负载自检三项
  （progress_delta 3328 / source_energy_min 2836→2366）**完全相同**，
  即夹具行为无漂移；phase 8 新增的 link 分派只对 link 对象多一次
  `structureType` 取值（原 P4 版的 match guard 已对每个 Generic 解析一次
  data JSON，本轮的 `settle::generic_structure_type` 抽取不增成本）。
- **主线复核补（对外部评审报告的逐条复核，新发现 1 项并修复）**：
  **lab 矿物槽的容量绑定**——vanilla placeStructure :1122-1128 对
  spec.store 带矿物的 lab 给 storeCapacityResource 补缺失矿物
  LAB_MINERAL_CAPACITY。不补则未存矿物的 `getCapacity` 错读 3000（官方
  0→null，**槽绑定语义的玩家可观测面**）。修复：place 面 lab 臂填充。
  **勘误记录**：初判「填充与回退式恒等、仅 DB 形态保真」——恒等只对
  已存矿物成立（flat 5000−energy 2000=3000 表定义使然），槽绑定面经
  消融实测纠正（禁用填充重跑棘轮：1626→1623，差 3 条即
  STORE-BIND-002 H/O/G）。守卫：e2e 双锚（玩家面 `getCapacity('O')`=null
  + 快照 storeCapacityResource 含 U/L 键集），变异实验双锚各自转红。
  **主线复核实测**（修复后终值）：workspace **310 绿**/clippy 0/fmt 净；
  release 重建后棘轮 **1626（+3 转绿、转红 0）**，地板 1623→**1626
  （55.80%）**；bot 三基线复跑三判定全过（tooangel 2000t 0 错误/
  overmind 形态集与基线全等 10=10/wasm 干净——tooangel 有一条 A6 基线
  没有的**日志**形态 `Executing room failed…lookFor`，系 bot 自身
  `getAdjacentPosition` 出界返 false 的 catch 日志非 console error，且
  bot 夹具无 lab（填充机械不可达），归 A6→P4 累计位移同外部报告消融
  结论）；perf 四相 0.125/0.217/0.031/0.480 持平。

### P3（Game.map 族 + Game.notify 全局通道 + cpu 三面）——已收口

- **落地**：①map 族真化（game.js map 段 IIFE——官方 map.js:11-268 逐方法
  对照）：describeExits（在场房四向全开——官方 mapGrid t/b/l/r 标记的
  单机近似，邻房可不在世界、off-world 返 null；键=方向常量 1/3/5/7）/
  findRoute（A* 逐字：kRouteGrid=30 搜索界、启发式曼哈顿、cost
  `Number(cb(...)) || 1` 的 0→1 归一、Infinity 剪枝、route 回溯四向
  判别；堆=二叉堆+惰性删除替代官方 Heap 类 decrease-key——行为等价）/
  findExit/findExitTo（rooms.js:1128-1131 两参转发）/getRoomStatus
  （normal/closed 两臂+非法名 undefined——正则 .test 强转语义；三态
  数据源缺登记）/isRoomAvailable（deprecated 提示不仿真）/getRoomLinear
  Distance（utils.js calcRoomsDistance Chebyshev 逐字）；roomNameToXY/
  getRoomNameFromXY 经 gw 桥导出（段序：room-position 先于 game）；
  常量 13 个加载期捕获。②notify 全链：prelude intents.push 全局通道
  （官方 runtime.js:78-86 逐字——list 顶层按动作名键、maxLen 帽、0.2
  计费）+ Game.notify（maxLen 20，game.js:174-180）+ intent.rs 顶层
  'notify' 键解析（清洗 schema common/lib/system.js:17：message
  userText/groupInterval number）+ settle 落账 PlayerState.notifications
  （房循环外每玩家恰一次；有界 100 drop-oldest）。③cpu 三面：cpuLimit
  （=tickLimit，game.js:145）/getHeapStatistics（begin_tick 控制线程
  采样真 V8 统计经 cpu_json 注入——snake_case 键面 overmind/tooangel
  消费形实证；externally_allocated_size 恒 0 登记）/halt（op_kk_cpu_
  halt：置 halt_requested+terminate——官方 user-vm.js:68-70 isolate
  dispose 的长寿命沙箱等价；run_loop 据标志分叉 after_halt=实际用量
  计费，硬停其余面与 terminate 同构）。
- **实现期取证**：W 房坐标向中心递减（W1N1 东邻 W0N1——e2e 首版预期
  写反即被抓）；JSON 序列化自然丢 undefined 键（notify 无参调用落库
  容空串）；deno_core v8 重导出可达（v8_isolate().get_heap_statistics()
  返回值式 API——HeapStatistics::default()+&mut 参数形态不存在）。
- **验证**：workspace 299 绿（+2：引擎 notify 解析/落账帽；embed 2 场景
  入串行链——map 族全表/notify 帽+落账+cpu 三面+halt 硬停后沙箱复用）/
  clippy 0/fmt 过/release+棘轮 **1578→1593（+15 转绿零未豁免转红）**
  ——MAP 族 10/GAME-NOTIFY 2/CPU-HEAP-001/ROOM-API-002/适配器 setup
  涟漪（default rooms all four exits open）；地板上调 ratchet.json+README
  （54.67%）+报告随批入库；bot 三基线（tooangel console 零错/overmind 既知形态复验——halt 路径
  未触及零新增错误/wasm/skipped_penalty 全 0，三判定通过）；perf 四相
  持平（两跑 0.126/0.220/0.032/0.484、0.126/0.222/0.032/0.483——对
  P2 日内 0.125/0.210-0.212/0.031-0.032/0.467-0.471，run 相 +4.8% 在
  当日噪声带内、settle 金丝雀稳定；begin_tick 新增 heap 采样为控制
  线程单次 v8 调用、buildGame map IIFE 闭包构建为亚微秒级）。
- **评审三路（官方语义/Rust+JS 正确性/测试与工程）1 FAIL 2 PASS 带条件，
  清偿当日完成**：**P1-1（评1）** notify message 误走 userString(100) 帽
  （官方 userText(1000)，common/lib/system.js:13）→ to_user_text 新助手
  （官方 `("" + (value||"")).substring(0,1000)` 强转语义：falsy 归空/数组
  join/对象 [object Object]）。**P1-2（三路共识）** count_billed 漏顶层数组
  通道（notify 权威账零计费——显示面与落账口径分裂，官方 make.js:98-107
  对数组逐元素计）→ 补 Value::Array 分支。**P1-3（评1）** halt「实际用量
  计费」与登记「官方不可直读」双失实——官方 make.js:280-284 抛出**先于**
  :194-212 计费块=**零计费**，runner.js:26-28 补 "CPU halted" console
  错误行 → after_halt 改候选丢弃（pending_bill=None）+三通道统一分叉
  （run_loop/run_expr/end_tick 写回——评2 P2-2）+错误行同款+e2e 桶零
  变动断言。**P2-1（评1）** A* 自写堆在并列 fcost 的弹出序与官方分岔
  （实测 8/11 tie 案例房间序列不同——玩家可观测）→ 换 path-utils.js
  Heap/OpenClosed **逐字移植**（冒泡 `<=` 即换/pop `>=`/update 线性扫三处
  序敏感语义），多跳 tie 序（西行先）+真实绕行（封锁官方首选）+kRouteGrid
  界外三把 e2e 锁。**其余**：Engine::tick 假沙箱合并点补 notify take（评3
  P2-2）；embed.rs doc/hidden 归位（评2/3）；game.js 段补 TERRAIN_MASK_*
  捕获（getTerrainAt 官方走闭包 C）+依赖行；测试注释 3+20 勘正；compat
  六登记改写/新增九行（halt 改写/A* 降级勘正/userText/getWorldSize/
  deprecated 提示/heap 相位补注/奇异输入）。
- **清偿复审（复核评审共识本身）**：P1-2/P1-3/P2-1 及其余全部实证
  通过——**官方零计费取证链独立实读成立**（user-vm dispose→make.js
  :280-284 抛出先于 :194-212 计费块→runner.js:26-28 错误行）、**A\*
  tie 序独立追溯吻合**（W2N1 后入堆经 `<=` 冒泡登顶先弹=西行先——
  e2e 锁的正是该序敏感语义）。**唯一阻断项（当日修复）**：js_array_
  to_string 元素强转把顶层 falsy 折叠误用到 join 元素（[42]→
  "[object Object]" 应为 "42"）→ 元素分支改普通 ToString+三条数组
  消息断言锁定。nit 三条登记（UTF-16 帽/halt 末语句竞态/findExitTo
  经 currentGame().map 的硬化不一致）。

### P2（创建族：createFlag/createConstructionSite 四面 + Flag/Site 增删改 + findClosestByRange + findPath 桩 + storage/terminal 回填 + toJSON）——已收口

- **落地**：①schema——FlagObj 尾追 owner（v1 内演化；官方 flags 按玩家
  私有——data.js:140 `rooms.flags` 按 user 全局查询，两玩家可同名旗）；
  ②intent.rs——RoomIntent::RemoveFlag{name}/RemoveConstructionSite{id}
  （**room 级**——§1.7 勘正：官方三文件全走 pushByName('room',…)）+
  parse 白名单；③settle/room.rs——组序稳定排序（room_intent_rank 五序
  对齐 room/intents.js:9-36，销账 compatibility.md「执行序」登记）+
  remove_flag（按 owner 域内删+$VLINE$/$TILDE$ 转义）+
  remove_construction_site（所有权门 site.owner∨controller.owner +
  progress>1 退 floor(progress/2) 能量）+ create_flag 挂主；④prelude——
  room.createFlag/createConstructionSite 全预检面（fetchXY/边界/FLAGS_LIMIT/
  色白名单/名自增/COST 门/RCL 配额/对象地形双占位/总量帽，utils.js:128-190/
  338-354 适配原始投影形态）、RoomPosition 两面委托、Flag.remove/
  setPosition/setColor 与 ConstructionSite.remove（组合 intent）、
  findClosestByRange 真化（room.find+getRangeTo 严格 <，官方 :1546-1576）、
  findPath 直线退化桩（:863-970 形态面）、room.toJSON（:540-549）、
  storage/terminal 回填；⑤投影——他人旗滤除+旗 owner 键。
- **评审三路（官方语义/Rust+JS 正确性/测试与归因）三 PASS 带条件，清偿
  当日完成**：**P1-1（评审 1，成立）**——createFlag 缺官方 :1022 本地
  Game.flags[name]=new Flag(...) 装配（同 tick 旗不可读，注释先行谎称已有）
  → 补 wrapObject 合成壳（name/color/secondaryColor 定格、无 id——官方旗
  无 getObjectById 面）+ e2e 同 tick 读锁。**P2 共识五项**——①不可见房
  自家旗蒸发（官方旗 user 全局集合 vs 我方 !room_visible 全裁）→
  object_owner 补 Flag 臂 + 破例导出（与工地同款两语义正交）+ 视野归集
  过滤显式跳过 Flag（data.js:113-116——加臂后必须，原「无 owner 天然不授」
  失效）+ game.js 兜底分支同款跳过 + e2e 双断言锁（旗存活+房不进
  Game.rooms）；②findPath 缺 toPos.roomName 门（:904-906）→ 补齐；
  ③裸全局常量 19 个（FLAGS_LIMIT/COLORS_ALL/CONSTRUCTION_COST/TOP 族/
  ERR 族——官方闭包 C 免疫玩家覆写）→ 加载期捕获；④extractor 互斥过用
  （官方 :178-181 在互斥前提前返回——我方循环内互斥先杀）+ 引擎缺矿物门
  与免墙检 → prelude continue 跳过+引擎 has_mineral 门+`!=extractor` 免墙；
  ⑤角格边界优先序（官方 :132-135 顺序 if——y 边后写胜出；我方三元链
  x 边优先）→ 两侧改顺序赋值逐字复刻。**P1-1（评审 3，主张
  不成立——清偿复审以棘轮实证纠偏）**——评审 3 主张「+170 归因错误、
  应为 121（1575−1454）」：其前提拿 P1 地板当基线，而棘轮基线取
  HEAD~1=P0 提交（报告 1405）——**初记 +170 本就是对的 flips 计数**
  （1405+170=1575 恰合）；清偿期曾据评审 3 误改 note 为 +121，棘轮
  复跑（转绿 173）实证后纠正。教训：归因数字以门禁自身 flips 计数
  为准，评审的算术主张同样要走实测取证（「复核评审共识本身」纪律
  的又一实例）。**其余**：spawn 名域跨房
  自家工地（roomObjs 单房 → Game.constructionSites 全房，官方
  register.constructionSites 域；敌方 spawn 分量缺失登记 compatibility.md）；
  dispatch-order 测试勘正（原异名 new/old 任何序都绿不锁序——改同名单名
  重建=真序敏感杀伤线）；陈注释四处（room.js:667 谎称装配/sites-flags A4
  桩块/setPosition 越界主张不实——官方仅 undefined 拦截，越界结算静默丢/
  world_json 「flag 无 owner」）；compatibility.md 两销账（Game.flags 成员
  资格/room intent 执行序）+ 七新登记（findPath 桩/findClosestByRange 平价/
  敌 spawn 名分量/引擎旗名单房域/转义不还原/双删退款形态/无主旗兜底）；
  findExitTo 顺延 P3 归档（map 依赖）；overmind 基线形态复取证（stub-flag
  错误消失，仅剩 setPublicSegments ×2000——P6 消失）。
- **验证**：workspace 297 绿（+6：引擎 6=旗主域/按主域删/换色移位组合/
  退款/所有权门/组序；embed 3 场景入串行链——创建+双观察者私有性/门与
  helpers/回填）/clippy 0/fmt 过/release+棘轮——实现批 **1405→1575
  （+170 转绿零未豁免转红）**+清偿批 **1575→1578（+3：FLAG-001/002/003
  =本地 Game.flags 装配族）**，合计对 HEAD~1 基线 **+173 转绿**、64 工地
  族/21 旗族/13 toJSON/8 旧路桩收 ripple/4 roomposition 族（评审 3 复核
  家族分布）；ratchet-allow 惰性条目清理（inter-room transition 自 P0
  基线吸收后恒不命中，用例保持 failed 属既定缺口）；**「过半」达成（
  1578/2914=54.15%）**；地板上调 ratchet.json+README 同步+报告随批入库
  （A6 先例）；
  bot 三基线（tooangel console error tick=0 人工确认/overmind 形态复取证
  归档——stub-flag 错误消失、colony 初始化深入浮出鸭子类型探针噪声
  （旗 .ref/spawn.storeCapacity，官方亦无此面）→ probe 增类级定向免计
  PROBE_IGNORE_SCOPED 保验收口径、missing_api 归零/wasm/skipped_penalty
  全 0，三判定均通过）；perf 四相当日 A/B 持平（两跑 0.125/0.210/0.031/
  0.467、0.125/0.212/0.032/0.471——对基线 0.124/0.209/0.032/0.467，
  settle 金丝雀稳定；一次整机抬升离群经复跑甄别为机器态）。
- **清偿复审（复核评审共识本身，独立官方实读+复跑）**：13 项 12 PASS
  +1 PARTIAL——PARTIAL 即本地旗壳 .room 恒 undefined（官方 RoomObject.call
  rooms.js:1651 挂 Room 实例）→ 当日一行修复+e2e shellroom 锁；行号漂移
  三处勘正（utils.js 四条 if :132-135/extractor 对象分支 :178-181/spawn
  名域 :1048·:1084-1087）；旗类缺 RoomObject.prototype 中间层（先在面，
  M0 桩类期）登记 compatibility.md 随类层级批次；**评3「+170 应为 121」
  主张经基线取证（ratchet.mjs:136 HEAD~1=30f6c2d 报告 1405）确认不成立**。
  复审独立复跑：297 绿/clippy 0/fmt 过。

### P1（Creep 尾巴：ranged 族 + getActiveBodyparts + notifyWhenAttacked + saying + 桩表清理）——已收口

- **落地**（含执行器——见 §1.5 前提勘正）：①intent_ranged_attack /
  intent_ranged_mass_attack（combat.rs，rangedAttack.js / rangedMassAttack.js
  逐行对照：重检序/box 射程 3/rampart 单体重定向 vs mass 跳过/距离衰减
  ×1/×0.4/×0.1 round/功率 0 早退仅 mass/尾部 actionLog 位）+ damage.rs 的
  ACTION_RANGED_ATTACK 位（_damage.js:66-71）+ u16 位图 bit10/11；
  ②prelude 四 API 面（rangedAttack/rangedMassAttack/getActiveBodyparts
  （官方尾扫 break 语义）/notifyWhenAttacked（宽松不等判重））；
  ③saying 真化——CreepObj 尾部追加 say_message/say_public/
  notify_when_attacked 三字段（v1 内演化，B3/B4 先例），say.js 截 10 UTF-16
  码元（from_utf16_lossy 折叠孤立代理项为 U+FFFD——分歧登记）、per-tick 随
  action_log 重置、墓碑公开遗言（_die.js:33）承接、出生即
  notify_when_attacked: Some(true)（spawns/create-creep.js:93）；④投影三键
  （say/sayPublic 成对、notifyWhenAttacked）+ prelude saying getter 三态折叠
  （creeps.js:84-92）；⑤CREEP_METHODS 残余桩表摘除——官方 29 方法全真，
  非官方残留面（spawn/recycleCreep/renewCreep/moveHead）清除。
- **评审三路（官方语义/Rust 正确性/测试与工程面）两 PASS 一 FAIL**：
  **P1-1（评审 2，成立，当日修复）**——calc_body_effectiveness 的 part_key
  误传 'rangedAttack'/'rangedMassAttack'（应 BOOSTS 顶层键 'ranged_attack'）
  → boost 静默 ×1；评审 1 与评审 2 此项结论**互相矛盾**，清偿复审实读
  types.rs 查表链 + kk-core BOOSTS 表独立判定评审 2 正确（教训又一例：
  多路评审同一处会得出相反结论，矛盾主张必须实测取证）；修复 + XKHO2 ×4
  锁定测试。清偿：powerBank 反击（POWER_BANK_HIT_BACK ×0.5）未建模经 mass
  新近可达——compatibility.md 登记（归 power/SK 生态前置批，含 i64 分数
  承载语义）；出生默认值 Some(true)（评审 1 P2-2）；settle/mod.rs 陈旧注释
  （「Creep 无 notifyWhenAttacked 方法」已被本片证伪）+ compatibility.md
  两行销账（say 文本载荷/creep notify 落库——后者「官方只在结构类暴露」
  系事实错误一并勘正）；注释官方行号系统性勘正（初版按记忆引行号整体漂移，
  两轮共 ~20 处对 ~/Desktop/projects/sources 实树归一）；测试补强四项
  （box 边界 (13,13)/(14,10)、反击判别 victim 带 ATTACK、零目标有功率位照置
  /零功率不置、boost 乘数锁定）。
- **验证**：workspace 291 绿（+8：引擎 7+boost 1，embed 3 场景入串行链）/
  clippy 0 / fmt 过 / release+棘轮 **1405→1454（+49 转绿零未豁免转红）**
  ——49 条逐条归因全落 ranged（COMBAT-RANGED/RMA/EVENTLOG-015/016/
  INTENT 族）/getActiveBodyparts（BODYPART-003/STALERECV-002）/saying
  （CREEP-SAY-001~003/TOMBSTONE-018）族，3 条经 instrument 间接转绿（评审 3
  独立复核）；地板上调 ratchet.json + README 同步（49.90%）+ **1454 报告
  随批入库**（A6 地板变动批先例——评审 3 P1：不入库则 49 条新绿下批失去
  逐用例转红臂保护）；bot 三基线（tooangel console error tick=0 人工确认、
  overmind 既知形态、wasm、skipped_penalty 全 0）；perf 四相持平
  （0.122/0.205/0.032/0.456 两跑 + 清偿后 0.127/0.211/0.032/0.472——
  settle 金丝雀 0.032 全程稳定）。
- **登记**：prelude 目标活性检查省略（register 成员检查——in-tick 世界冻结
  下不可达，attack/heal 既有同形口径）；mass 全房扫描每 Generic 双 JSON
  解析（perf 观察项，四相持平已验）；round-vs-trunc 在 attackPower 恒为
  10 倍数域内不可观测（代码审查锁定）；powerBank 反击缺口（上行清偿条）；
  INVALID_TARGET API 面由外部 COMBAT-RANGED-005 转绿兜底（内部 e2e 未单列）。

### P0（O2 领域模块归属 + pub rooms/time 收敛 + 方案入库）——已收口

- **落地**：kk-world 新增 `domain/{mod,controller,store,owner}.rs`——五函数
  纯搬移（controller_progress_total_opt / store_positive_entries /
  apply_store_extra / object_owner / controller_owner_value，原居 snapshot.rs
  与 world_json.rs），两投影与 kk-engine 结算侧消费点全部改径
  crate::domain:: / kk_world::domain::（无 re-export 捷径——P2-1 前置批
  清偿定夺口径延续；domain/mod.rs 的 use 再导出仅服务于 crate 内短路径）。
  `World.rooms`/`time` → pub(crate) + `rooms()`/`time()`/
  `commit_room()`（**不 bump 代际**，§2 勘正）/`#[doc(hidden)]
  set_time_for_test()`；跨 crate 访问点全数适配（settle 四处、embed 两处、
  world_source 两处、storage-bench 四处、settle_tests 系列八处、sandbox
  三处）——编译器穷尽，无绕过通道（unsafe/解构/字面量更新零命中）。
- **token 级对账**：五函数与 HEAD 版本 token 流一致（作者与评审 1 双向
  独立核对）；可见性逐函数保序（pub→pub、pub(crate)→pub(crate)）。
- **评审三路（搬移等价/Rust 收敛正确性/测试锁值与工程面）全 PASS 零
  P0/P1 代码缺陷**。清偿：①方案 §2/§7-8 与实现矛盾（commit_room 是否
  bump——实现按不变量方解决，方案文本勘正+本记录落账，三路同报）；
  ②object_json doc 块+allow 错挂（P4 期 pre-existing，搬移后换宿主）归位
  +「每 tick」措辞还原；③Fixture::settle 的 get_mut→room（零漂移化——
  原 room_mut 改法多一次保守 bump，评审 1 P2-4）；④set_time_for_test
  负向语义锁定测试（不动 generation/player_ticks——评审 3 P2-1）；
  ⑤world.rs 五处 doc 措辞随收敛更新（time 字段 crate 内直写的不对称如实、
  advance_ticks/commit_room/bump_generation 的过时口径）；
  obj_dir_rebuilds 测试改经 commit_room（与真实提交尾同语义）；
  domain/controller.rs rustdoc 链接全限定；§1.1 三处官方行号勘正
  （survivalInfo=rooms.js:437 且为 Room 级、gpl 164-168、cpuLimit
  game.js:145）。
- **验证**：workspace 283 全绿（+1 锁定测试）/ clippy 零告警 / fmt 过 /
  release+棘轮 1405 零未豁免转红零转绿 / bot 三基线（tooangel 2000t
  console error tick=0 人工确认、overmind 2000t 既知形态 setPublicSegments/
  stub-flag×2000、wasm 10t 零错、skipped_penalty 全 0）/ perf 四相同口径
  持平（0.122/0.205/0.032/0.456 与 0.123/0.207/0.032/0.461 两跑，
  基线 0.124/0.209/0.032/0.467 噪声内——settle 金丝雀 0.032 稳定）。
- **登记**：kk-world crate 内测试仍直写 world.time（pub(crate) 放行，
  构造期无害——与跨 crate 旋钮化的刻意不对称）；`rooms()` 返回具体
  BTreeMap 类型（表示泄漏 + 有序迭代序成对外承诺——settle 房名序恰依赖
  它，2B 列式迁移时签名破坏面归彼批）；misc_and_notify pull_pair 用例的
  room_mut 保守 bump（安全方向改善，HEAD 直写本违 O1 硬纪律）；
  Fixture::settle 每次 +1 代际不变（room+commit_room 零 bump +
  advance_ticks +1，与 HEAD 逐点同）。
