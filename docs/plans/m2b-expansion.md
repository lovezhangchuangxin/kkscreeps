# M2B 扩面批：玩家 API ~150→~230（官方全量 340 的可达子集）

> 状态：**已确认（2026-09-30 用户批准开批）**——P0 已收口（见 §8）；
> P1-P6 待续。批次方向决策归用户，批内切片节奏自主。
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
| findExitTo | rooms.js:1130-1132（map.findExit 转发） | ⬜ | **P2 真化**（依赖 P3 map 族，官方同形转发） |
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
| ConstructionSite（6 getter ✅ + remove 🪑） | **P2 转真**（Action::RemoveSite 对象级 intent + 结算摘除） |
| Flag（name/color/secondaryColor/memory ✅；remove/setPosition/setColor 🪫） | **P2 转真**（对象级 intent + 结算——FlagObj 直改） |
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
| P2 | 创建族与 RoomPosition/Room 尾巴：createFlag/createConstructionSite 四面转真 + Flag.remove/setPosition/setColor + ConstructionSite.remove + findClosestByRange + findExitTo + findPath 直线桩 + room.storage/terminal 回填 + Room.toJSON | prelude/room.js/room-position.js/sites-flags.js + intent.rs（对象级 3 新 Action + 白名单）+ settle 执行器 4 小面 | 中 |
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
