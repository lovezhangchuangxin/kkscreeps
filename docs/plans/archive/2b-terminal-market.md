> 历史归档：正文保留当时的方案、状态和验证；不得作为当前接手指令。当前工作见[任务台账](../mod-next-batches.md)，现行契约见[架构索引](../../architecture/README.md)。

# 2B 批：terminal / market（订单簿 + 跨房资源转移 + credits）

> 状态：**批已收口（2026-10-01，P0-P2 三片全部完成；地板 1774→1852/2914=63.55%）**。
> 批次方向决策归用户；批内切片节奏自主（同 M2B 扩面先例）。
> 定位：M2B 扩面批（0702235，地板 1774/2914=60.88%）之后授权序列下一站。
> 依据：[m2b-expansion](m2b-expansion.md) §1.3/§6（market 整体推 2B 的登记）、
> [roadmap](../../roadmap.md) 2B 关节（API 补全面）、[ADR-013](../../decisions/ADR-013-architecture-review-coordinator.md)。
> 官方源实读基线：`~/Desktop/projects/sources/screeps/`（engine@pin / common@pin /
> screeps-ok@34a630e / vanilla adapter 报告 `.compat/screeps-ok/reports/vanilla.json`）。

## 0. 目标与总原则

- **交付 terminal + market 全链**：StructureTerminal.send（跨房资源转移）+
  Game.market 12 面（订单簿生命周期/成交/查询/credits/transactions），对齐
  官方 engine 逐字语义。screeps-ok 解锁面合计 **83 用例**（市场族 73
  +INTENT-LIMIT 2+terminal 邻接 8；见 §2/§4 口径），其中本批可达绿 **79 面**
  （扣 MARKET-QUERY-006 官方同红豁免、TERMINAL-SEND-002/004 power 门、
  015 liveWorldSize 门），**MARKET-QUERY-006 官方同红**按「官方语义优先」
  对齐 + ratchet-allow 豁免归因。
- **官方执行序硬锚**（main.js:47-72 实读）：用户执行 → 房间队列结算（terminal
  `send` intent 在房间相**只做校验并落 `object.send` 字段**——
  processor/intents/terminal/send.js 全 36 行）→ commit1 → **global 阶段**
  （processor/global.js:48 先 power 后 market——send 消费/订单簿结算/
  成交都在这里，global-intents/market.js 601 行）→ commit2 → 时间递增。
  我方等价落点：`Engine::tick_with_intents_with_input` 房间 commit 循环之后
  插入 `settle/market.rs` 全局阶段（与 notify 落库同层）。
- **价格单位铁律**（本批最大单位陷阱，官方实树取证）：玩家/API 面 price=
  **credits 浮点**；intent 清洗层 ×1000 成**整数毫 credits**
  （common/lib/system.js:8 `'price': value => parseInt((1000*value).toFixed(0))`）；
  db/订单簿/user.money 全毫；玩家视图（credits getter/orders/getAllOrders/
  getOrderById）÷1000。fee=ceil(毫price×amount×0.05) 毫
  （global-intents/market.js:127）。套件 publicFee
  （20.2-20.4-market.test.ts:22-25）同口径反向验证。**f64 逐位一致**：
  毫整数运算 < 2^53 精确域，Rust i64/f64 混合与 JS number 同结果；MARKET_FEE
  两侧同字面量 0.05。
- **逐片完整批次循环**（§5 验证矩阵），统一提交点纪律不可破：市场结算写入
  PlayerState.money 与 MarketState，随全局阶段在**世界结算成功后**提交——
  房间相失败整体回退时市场相不执行（官方 global 阶段在 rooms 全部成功后
  才跑，同构）。

## 1. 官方语义锚清单（全部行号 grep 实树取证）

### 1.1 Game.market（engine/src/game/market.js:8-253）

| 面 | 行 | 语义要点 |
|---|---|---|
| calcTransactionCost(amount, room1, room2) | :31-34 | `calcRoomsDistance(r1,r2,true)` → `calcTerminalEnergyCost` |
| getAllOrders(filter) | :36-39 | `_getOrders(filter.resourceType)` 快照深拷贝+price÷1000，`_.filter(orders, filter)`（filter 空数组形注意） |
| getHistory(resourceType) | :41-53 | 非法资源名返 `{}`；`'all'` 缺省；history 来自 market.stats（引擎恒空——driver data.js:51-75（:60 初始化 `{all:[]}`）数组、按资源键仅在有数据时存在：合法资源无数据 → `{}` **对象非数组**） |
| getOrderById(id) | :55-66 | 先查 this.orders（自己的，含 inactive，price÷1000 已做）→ 公共 active 池 → null |
| createOrder(type,res,price,total,room) | :68-98 | 对象/参数双形解构；资源名门（RESOURCES_ALL+INTERSHARD）→ type 门 → parseFloat/parseInt → `!price‖≤0‖!totalAmount` → fee>credits 门 → 非 intershard 须 roomName 有**自家 terminal**（ERR_NOT_OWNER）→ `size(orders)+本tick已建 ≥300` ERR_FULL → 计数++ → pushByName('global','createOrder',…)（**无 maxLen**） |
| cancelOrder(orderId) | :100-106 | 门=`this.orders[orderId]`（自己的）否则 ERR_INVALID_ARGS；pushByName(…,50) 返回值**忽略**恒 OK |
| deal(orderId,amount,targetRoom) | :108-153 | order 取**公共 active 池**（runtimeData.market.orders.all）不存在→ERR_INVALID_ARGS；parseInt amount `!amount‖<0` 门；intershard→资源面门；否则 targetRoom 必填→terminal 查找（ERR_NOT_OWNER）→ transferCost=calcTransactionCost→energy<cost ERR_NOT_ENOUGH_RESOURCES→cooldownTime>time ERR_TIRED→BUY 型资源面门（energy 特判 amount+cost）→SELL 型 `user.money<amount*price` 门→pushByName(…,**10**) 满 ERR_FULL |
| changeOrderPrice(id,newPrice) | :155-172 | 门=自己的订单；parseFloat；提价差额费>credits 门；pushByName(…,50) |
| extendOrder(id,addAmount) | :174-191 | 门=自己的订单；parseInt `!add‖≤0`；fee>credits；pushByName(…,50) |
| credits getter | :196-201 | `(user.money‖0)/1000` |
| incoming/outgoingTransactions | :203-233 | runtimeData.transactions 各 100 条 map：transactionId=""+_id、删 _id、sender/recipient→`{username}`或 undefined；**闭包缓存**（每 tick 对象重建=每 tick 首读快照） |
| orders getter | :235-249 | myOrders map：id 串化、删 _id/删 user、price÷1000、indexBy('id')；闭包缓存 |
| 描述符形态 | :194-250 | `Object.defineProperties(market, {credits/incoming/outgoing/orders: {enumerable:true, get}})`——四 getter **只有 get**（无 set/writable，不可配置）；8 个方法=对象字面量 own enumerable writable（P6 描述符镜头：SHAPE-GAME-007 dataProps 恰 4 键） |

### 1.2 StructureTerminal（engine/src/game/structures.js:701-747）

- send(resourceType, amount, targetRoomName, description) :714-745 **校验序逐字**：
  `!this.my`→ERR_NOT_OWNER → `checkStructureAgainstController`（P5 已移植）→
  ERR_RCL_NOT_ENOUGH → `/^(W|E)\d+(N|S)\d+$/` 房名→ERR_INVALID_ARGS →
  RESOURCES_ALL 资源名→ERR_INVALID_ARGS → store 面 `!store‖!store[rt]‖<amount`
  →ERR_NOT_ENOUGH_RESOURCES → `cooldownTime>time`→ERR_TIRED → 能量成本
  （非 energy：`energy<cost`；energy：`energy<amount+cost`）→
  ERR_NOT_ENOUGH_RESOURCES → description（非 string‖>100）→ERR_INVALID_ARGS →
  `intents.set(id,'send',{…})` → OK。**无最小量门**（TERMINAL_MIN_SEND 常量
  存在但 send.js 不消费——TERMINAL-SEND-014 锁 amount=1 可发）。
- prototype getter（:711-713）：store/storeCapacity/**cooldown**
  （`cooldownTime && cooldownTime>time ? 差 : 0`——P4 折叠 getter 已在
  structures_spawn.js:785 cooldownFold 同形）。

### 1.3 房间相 intent（processor/intents/terminal/send.js 全 36 行 + tick.js 全 15 行）

- send.js：房名 regex/资源名/`amount≤store` 复验（无 my/RCL 复验——已在 API
  相做过；官方房间相不再查 cooldown）→ PWR_OPERATE_TERMINAL 效果折成本
  （power 批不做——我方结构无 effects 数据，分支死码，登记）→
  `bulk.update(object, {send: intent})`——**只落字段不执行**。
- tick.js（phase 8 每房每 terminal）：storeCapacity =
  `controller.level>0 && controller.user==terminal.user && CONTROLLER_STRUCTURES.terminal[level] ? 300000 : 0`。

### 1.4 global 阶段（processor/global-intents/market.js:7-600，执行序逐段）

1. **send 消费**（:71-95）：全部 terminal 按 `!!i.send` 过滤——先
   `bulk.update(terminal,{send:null})`（**无条件清**）→ `cooldownTime>time`
   return → 目标房无 terminal 或无 user return → executeTransfer 成功才
   `cooldownTime=gameTime+TERMINAL_COOLDOWN`。
2. **executeTransfer**（:15-69）：from/to/feeTerminal 缺一 false；from.user
   资源面复验 false；to.user 空间钳 `min(amount, storeCapacity−calcResources)`
   （**钳后 ≤0 静默 return undefined——注意非 false**）；成本重算（feeTerminal
   ===from：非 energy `energy<cost`/energy `<amount+cost`；===to：
   `to.energy<cost`）→ 三方 store 写回 → bulkTransactions.insert
   `{time,sender,recipient,resourceType,amount,from,to,+附加}`。
3. **createOrder**（:109-165）：复验 price/totalAmount/资源名/terminal 归属
   （intershard 免 terminal）→ fee=ceil(毫price×total×0.05) `user.money<fee`
   return → money−=fee → 订单插入
   `{createdTimestamp:Date.now(),user,active:false,type,intent 字段,amount:0,remainingAmount:totalAmount,created:gameTime(非 intershard)}`
   → 资金流水（users.money 日志——我方无该集合，**只保 money 数值**，流水面登记不做）。
4. **changeOrderPrice**（:168-212）：订单存在+归属 → newPrice 门 →
   `newPrice≠price` 置 `_skip`（**同 tick 后续 deal 跳过该订单**）→ 提价差额费
   → `bulk.inc(order,'price',diff)`。
5. **extendOrder**（:215-252）：归属/正量/fee 门 → remainingAmount+totalAmount
   双加。
6. **cancelOrder**（:256-263）：归属 → `remainingAmount=0`+`_cancelled` →
   尾段 remove。
7. **deal 收集**（:265-285）：订单存在且非 _skip、amount>0；intershard→
   directDeals；否则 targetRoom 归属复验 → terminalDeals。
8. **terminalDeals**（:289-398）：**按目标房→订单房距离升序排序**后逐条：
   双 terminal 存在、target 冷却门 → BUY/SELL 买卖双方定向 →
   `amount=min(deal.amount, remaining)` → seller 资源钳 → buyer 空间钳 →
   buyer.money 钳（`dealCost=min(dealCost,money); amount=floor(dealCost/price);
   dealCost=amount*price; !amount return`）→ executeTransfer(fee=target
   terminal) → seller/buyer money 双向落账（**NPC 侧 user 为空跳过**）→
   订单 amount/remainingAmount 双减 → target cooldown。
9. **directDeals**（intershard，:400-495）：shuffle（官方非确定——我方稳定
   序，登记）→ 用户 resources+money 直接划转。套件不覆盖；PlayerState.resources
   落最简实现（BTreeMap，缺省空）不冒充。
10. **订单生命周期尾段**（:497-589，每 tick 全量订单）：
    - `_cancelled` → remove；`createdTimestamp+30d(墙钟)<now` → 退
      `remaining×毫price×0.05` → remove（MARKET-ORDER-009 用 adapter 注入
      createdTimestamp 过去值）。
    - SELL：可用量=min(terminal/intershard 资源, remaining)；active 且
      无量→active=false,amount=0；active 且量变→amount 同步；inactive 且
      有量→active=true+amount。
    - BUY：newAmount=min(floor(money/毫price), remaining, terminal 空间)；
      `newActive=isOwner&&newAmount>0`；与现值不同才写。

### 1.5 数据供给（driver/lib/runtime/data.js 实读）

- runtimeData.market.orders（:20-38）：`db market.orders find({active:true})`
  **按 gameTime 跨玩家缓存**；分桶 `{all:{}, [resourceType]:{}}`，id 串化。
- runtimeData.market.myOrders（:227/:243-246）：`find({user:userId})` 全量
  （含 inactive）。
- runtimeData.market.history（:40-60）：market.stats 恒空 → `{all:[]}`。
- runtimeData.transactions（:138-139/:213-218）：`findEx({sender:userId},
  {sort:{time:-1},limit:100})`+incoming 同构；:189-190 交易对端进 userIds
  集（username 解析用）。
- user.money（:199 credits getter 消费）：毫。
- vanilla adapter 种子（adapters/vanilla/index.ts:950）：
  `money: 10000000000`（10M credits）——**套件 INITIAL_CREDITS=10_000_000
  锚**，我方 createShard 同种子。
- placeMarketOrder（vanilla adapter :1415-1434）：
  `{createdTimestamp:Date.now(), created:gameTime, user, active:spec.active??true,
  type, resourceType, price:round(spec.price*1000), amount:0,
  remainingAmount:totalAmount, totalAmount, roomName}` → 返回 id 串。

### 1.6 常量（common/lib/constants.js 实读）

TERMINAL_CAPACITY=300000（:333）/ TERMINAL_COOLDOWN=10（:337）/
ORDER_SELL="sell"/ORDER_BUY="buy"（:360-361）/ MARKET_FEE=0.05（:372）/
MARKET_MAX_ORDERS=300（:374）/ MARKET_ORDER_LIFE_TIME=30d 毫（:375）/
INTERSHARD_RESOURCES=[token,cpuUnlock,pixel,accessKey]（:1138-1143）。
**TERMINAL_SEND_COST/TERMINAL_MIN_SEND（:335-336）为死常量**（engine 源零消费）
——玩家 realm 常量面照挂（官方 globals 挂全量 C）。MARKET_MAX_DEALS
**官方常量不存在**（INTENT-LIMIT-001 `typeof` 容忍 null；deal 限 10 的真身=
market.js:149 pushByName maxLen）。

## 2. 范围表（官方面 × 套件用例 × 我方现状）

套件面=screeps-ok 市场族 73 用例（TERMINAL-SEND 50+MARKET-ORDER 10+
MARKET-DEAL 5+MARKET-QUERY 7+SHAPE-GAME-007 1；vanilla 基准实测 72 绿 1 红）
+INTENT-LIMIT 2+terminal 邻接 8=83；我方现状=83 全部 skipped（四能力键
terminal/marketBasics/market/terminalSend 均 false，compat/src/capabilities.ts）。

| 族 | 用例数 | 能力键 | 我方现状缺口 | 本批预期 |
|---|---|---|---|---|
| TERMINAL-SEND 001-015（013 矩阵 36 变体） | 50 | terminalSend | send API/intent/双相执行/transactions 全无 | **47 绿**；002/004（powerCreeps+powerEffects）/015（liveWorldSize）三键维持 skip |
| MARKET-ORDER 001-009（002 双形态=10 条） | 10 | market | 全无（countStub 桩） | 10 绿 |
| MARKET-DEAL 001-005 | 5 | market | 全无 | 5 绿 |
| MARKET-QUERY 001-007 | 7 | market/marketBasics | 全无 | 6 绿；**006 官方同红**（vanilla failed——官方 getHistory 返 {} 对象，套件期望数组）→ 我方对齐官方语义后保持红 + ratchet-allow 豁免归因 |
| SHAPE-GAME-007 | 1 | marketBasics | market 桩形状（4 getter 缺） | 1 绿（**用例只断言 dataProps 键集**——键集可过；getter 三标志/get-only 严格赋值面 P1 已按官方形态装配但该用例不锁定，P2 e2e 补描述符断言锁） |
| INTENT-LIMIT 001/002 | 2 | market | MARKET_MAX_ORDERS 常量已在 kk-core；MARKET_MAX_DEALS 不存在（容忍） | 2 绿 |
| terminal 邻接：CTRL-STRUCTLIMIT-002:terminal×2 / ROOM-STRUCTURE-001:terminal / STRUCTURE-HITS-001:terminal / CONSTRUCTION-COST-001:terminal / STORE-OPEN-001/002:terminal / SHAPE-STRUCT-001:terminal | 8 | terminal | 读取面 P4 已齐（折叠 getter/room.terminal 快捷/place store 面）；缺 terminal/tick storeCapacity RCL 归一与 hits=3000 初始化核对 | **P1 实收 7 绿**；SHAPE-STRUCT-001:terminal skip→failed 归既有 Object Shape 红家族（装配器 isStructure 实例标记+ELSE_FIELDS 实例级 energy/energyCapacity+原型多键——家族整体归 Object Shape 批，非本片回归） |
| **合计** | **83**（50+10+5+7+1+2+8） | | | **78 绿上限**（P1 实收修正：79−SHAPE-STRUCT-001:terminal 归家族红；83−006 官方红−002/004/015 三键−terminal 形状族红） |

## 3. 切片划分（每片一个完整批次循环）

### P0 地基片：域态 + intent 通道 + 全局阶段挂点（零玩家可见面变化）

- `PlayerState` 增 `money: i64`（毫，缺省 0）+ `resources:
  BTreeMap<String,i64>`（intershard，缺省空）。
- `Engine` 增 `market: MarketState`（orders: BTreeMap<String,MarketOrder> /
  transactions: VecDeque<Transaction>（容量 1000 截断，登记有界差异——官方
  db 无界、玩家视图各 100 条）/ next_order_seq）+ `Transaction`/`MarketOrder`
  结构（毫 price、amount/remaining/total、active、created tick、
  created_timestamp 墙钟毫）。
- `intent.rs`：`UserIntents` 增 `market: Vec<MarketIntent>`（枚举
  CreateOrder/CancelOrder/ChangeOrderPrice/ExtendOrder/Deal）——清洗逐字对齐
  common/lib/system.js 白名单+transform（price=`parseInt((1000*v).toFixed(0))`
  的 Rust 等价 `(1000.0*v).round()`——toFixed 恰半取大 n ≡ round 远零半舍
  （node 实测钉测），≥1e21 指数形首位怪癖保真实现（清偿复审裁决）；number=
  parseInt 截断；string=""+v）。
  from_json 的 'global' 容器臂（对照 notify 臂先例）。
- `settle/market.rs` 域态模块（MarketState/MarketOrder/Transaction + 容量
  纪律 + id 生成器）。
- **挂点与注入通道 re-slice（P0 评审后定音，随 P1 落地）**：
  ①全局阶段挂点——P0 评审确认官方 global 阶段（main.js:63-65）无 throw
  路径（逐 intent 静默跳过），故我方 `settle_market` 设计为**不可失败阶段**，
  在 `tick_with_intents_with_input` 的**房间 commit 循环之前**对已结算工作
  副本（`settled` Vec）运行——失败原子性与「一次成功 tick ⇒ generation 恰
  +1」不变量（room_mut/get_mut 保守 bump 与之冲突）双双保住，官方
  「commit1 后读已结算态」的可观测序等价（§7-5 措辞随此勘正）；②市场数据
  注入——**不走新 V8 host 通道**（原设想的 set_market_state），改在 embed
  层对 `world_json_for_viewer` 产出合并 per-viewer `market` 键（引擎态→
  投影合并；gcl 参数先例同位；segments 需要 host 通道因其是沙箱闭包态，
  市场是投影态——归因不同）。两项均随首个消费者 P1 落地，P0 保持零玩家
  可见面。
- 验收：workspace 全绿（新增单测：intent 清洗五键/price 毫变换边界/
  MarketState 容量与 id 单调）；棘轮 **零变化**（玩家面未动）；三基线/perf
  照跑。

### P1 终端片：terminal/tick + send 全链 + transactions（+55 面）

- **房间相**：`Action::Send` + structures.rs 执行器（send.js 逐字：regex/
  资源名/amount≤store/能量成本复验——**官方房间相不查 cooldown/my/RCL**）
  → 写 Generic data `"send"`；economy.rs phase 8 增 terminal 分支
  （storeCapacity RCL 归一，tick.js 逐字）。
- **全局相**（settle/market.rs）：terminal 扫描消费 `data.send`（无条件清→
  cooldown 门→目标 terminal 归属门→executeTransfer→cooldownTime=time+10→
  Transaction 入账）；executeTransfer 移植（空间钳/成本门/三方写回/
  transaction 字段）。
- **API 面**：prelude StructureTerminal.send（校验序 §1.2 逐字，8 拒因+
  组合序矩阵 36 变体的门序就是官方源序）；cooldown 折叠 getter 已在
  （P4）；transactions 注入（每玩家 outgoing/incoming 各 100 条投影）+
  Game.market.incoming/outgoingTransactions getters 真化（闭包缓存+
  transactionId 串化+sender/recipient→{username}）。**Game.market 其余面
  维持桩**（credits 桩恒 0、orders 桩 {}——不发订单无订单簿，桩面无害）。
- **capability**：`terminal: true` + `terminalSend: true`。
- 验收锚：TERMINAL-SEND 47 绿（001/003/005-014 含 013 全 36 变体、
  011 无目标 terminal OK 无转移无冷却、012 送达量断言）；邻接 **7 绿**
  （STORE-OPEN 依赖 tick.js 归一；STRUCTURE-HITS-001 place 面 hits=3000
  ✓；SHAPE-STRUCT-001:terminal 归 Object Shape 家族红——P1 实收 +54）。

### P2 市场片：Game.market 12 面 + 订单簿全生命周期（+24 面）

- **prelude Game.market 真化**（market.js:8-253 逐字，替换 countStub 桩）：
  8 方法+4 getter（defineProperties 形态逐字——P6 描述符镜头）；getHistory
  官方形状（'all'→[] / 非法→{} / 合法无数据→{}——**006 保持红对齐官方**）；
  gw.intents 全局通道 pushByName('global',…) 形态（maxLen：deal 10 →满
  ERR_FULL；cancel/change/extend 50 返回值忽略；createOrder 无 cap）。
- **全局相订单簿**：§1.4 段 3-10 全量（createOrder 复验/fee/插入形状；
  changePrice _skip；extend 双加；cancel；deal 收集+距离排序+买卖定向+
  三重钳+executeTransfer+订单双减+cooldown；生命周期尾段同步/过期退费/
  移除；directDeals 稳定序登记）。
- **adapter/RPC**：embed `placeMarketOrder`（vanilla adapter :1415 形状）+
  adapter.ts 接线；createShard money 种子 10_000_000_000 毫；
  `market: true`+`marketBasics: true`。
- **注入**：set_market_state 消费——{credits_milli, orders_active 快照,
  my_orders, transactions}（price÷1000 在 prelude getter 层做，与官方
  _getOrders 同位）。
- 验收锚：MARKET-ORDER 10/DEAL 5/QUERY 6/SHAPE-GAME-007/INTENT-LIMIT 2；
  ratchet-allow 增 MARKET-QUERY-006（vanilla 同红取证归因）。
- **地板预期**：1774+54（P1 实收）+23（P2：MARKET-ORDER 10+DEAL 5+
  QUERY 6+SHAPE-GAME-007 1+INTENT-LIMIT 2−002 的 ERR_FULL 双形态中
  001 计一次等——以实跑为准）≈**1851-1852/2914≈63.5%**（P1 后 1828；
  精确数以 capability 翻真实跑归因）。

## 4. 地板目标口径

- 每片棘轮以 `node compat/scripts/ratchet.mjs`（先 `cargo build --release
  -p kk-cli`）为准；**P1 +55 / P2 +24 为目标口径**，实跑偏差逐用例归因
  （先查套件门是否如预期解锁——capability 翻真后 skip→执行）。
- 转红门禁：零未豁免转红；MARKET-QUERY-006 skip→failed 的翻转走
  ratchet-allow 显式豁免（同提交归因，ROOM-EVENTLOG-027 官方同红先例）。
- P0 片棘轮**零变化**是设计约束（无玩家面变化）——若有翻转即实现泄漏，
  回查。
- 新 kkscreeps.json 报告随批入库（地板变动批先例）；三基线刷新（P6 后
  形态：overmind withErr=1 / tooangel 3 形态 / wasm 干净——**P2 实测勘误
  本行初稿「send/市场面 bot 不消费」的市场面半句：tooangel 每 tick 消费
  Game.market.getAllOrders**（P1 桩探针 `getAllOrders: 2000` 在 P2 撤桩后
  消失、真面接手 console 零错——良性，收口已归因）；perf 同日 A/B
  （bots 后冷却复跑）。

## 5. 验证矩阵（每片完整批次循环，不可裁剪）

1. `cargo build --release -p kk-cli` 后 workspace 全绿（`cargo test --workspace`）。
2. 棘轮零未豁免转红（§4 口径）；报告入库。
3. bot 三基线（tooangel 2000t console 零错人工确认 / overmind-dist 2000t
   / wasm-bot 10t）——形态对照 P6 后基线。
4. perf 同日 A/B（`perf-probe --ticks 1000 --warmup 100 --players 2`，p50
   对照；>20% 变化先消融机器态）。
5. ≥3 路 subagent 并行评审（官方语义/Rust 正确性/测试与归因三视角，
   并发 ≤6）——**评审 prompt 必含两个镜头**：①跨子系统执行时序（send 的
   房间相→全局相同 tick 链；回调/getter 内再操作其它子系统）②属性描述符
  /accessor 语义（market 四 getter 三标志、普通赋值 vs defineProperties、
  严格赋值面）。凡玩家可见对象挂载与 tick 尾回收，逐 API 对照官方挂载
  形态与执行序。
6. 逐条清偿+清偿复审（复核评审共识本身——多路矛盾主张实测裁决）。
7. 灰区表（compatibility.md）/roadmap/plan §8 回写 → `pnpm run check` →
   单一归因提交（subject ≤350 字符、冒号前 ≤48）并 push。

## 6. 不做清单（显式出界，评审按此判）

| 项 | 理由与归因 |
|---|---|
| PWR_OPERATE_TERMINAL/DISRUPT 效果（TERMINAL-SEND-002/004、POWER-OPERATE-005 等） | power 批（powerCreeps/powerEffects 能力键维持 false；结构 effects 数据模型随该批） |
| TERMINAL-SEND-015 worldSize 环绕计费 | liveWorldSize 能力键（我方世界模型 getWorldSize=1 登记面；calcRoomsDistance 的 continuous 环绕分支实装但世界尺寸语义归后续批） |
| Game.cpu.generatePixel/unlock 与 pixel/token 经济流 | MMO 订阅面维持现状+登记（m2b §6 同判）；PlayerState.resources 只承载 deal 划转不伪造来源 |
| users.money 资金流水（market.fee/buy/sell 日志集合） | 官方 bulkUsersMoney 是账户流水集合（页面消费），引擎行为面只保 money 数值；套件不覆盖；登记不做 |
| market.stats 历史（getHistory 有数据形态） | 官方由 backend cron 填充、引擎内恒空——getHistory 官方空形态逐字（'all'→[] 等）即完整语义 |
| NPC 订单生成（NPC roomName 订单、KEY/energy 官方 NPC 做市） | 官方 backend 任务，engine 仓库无此逻辑；订单簿只含玩家/adapter 注入订单 |
| lab/factory/nuker/observer/deposit/safe-mode/controller-mechanics/PathFinder 族红 | 各自归属批（m2b §6/roadmap 既有归因；剩余 153 红家族归批方向待用户拍板——本批只动市场族） |
| IntershardMemory/multiShard | M4（interShardMemory 能力键维持 false） |
| 存储/列式/并行调度 | 2B 里程碑的存储落盘面独立批（MarketState/PlayerState 为引擎内存态+快照契约不动——v1 冻结边界，F2 隔离） |
| 不动 packages/ 与 .worktrees/renderer/；不改 world_json/snapshot 契约字节形态 | F2 并行线隔离（用户指令） |

## 7. 风险与已知坑（本批专项）

1. **价格毫/credits 双单位**是全批最大陷阱：任何一处漏 ×1000/÷1000 都是
   1000 倍错且多数断言（toBeCloseTo 5 位）必炸——fee/credits/dealCost/
   changeOrderPrice 差额/过期退费五处公式以 §1 行号逐字对齐；套件
   publicFee/publicDealCost 双 helper 是反向锁。
2. **toJSON 式闭包缓存时序**：market orders/incoming/outgoing getter 是
   make 闭包缓存（同 tick 首读快照）——prelude 装配期缓存 vs 首访缓存的
   玩家可观测差（getter 调用次数）——按官方「装配后 lazy 首读」实现并配
   锚测（P6 惰性三读同款教训）。
3. **deal 排序**：terminalDeals 距离升序是玩家可观测执行序（同 tick 多
   deal 争同一订单时谁先扣 remaining）；Rust sort 稳定性对齐官方
   `Array.prototype.sort`（并列距离时官方插入序——Rust sort_by 稳定序同形）。
4. **同 tick 链**：send intent 房间相落字段→全局相消费——**同 tick 送达**
   （TERMINAL-SEND-012 断言目标 store 同 tick+1 可见——夹具 tick 时序
   注意 createShard 预热 tick 的 off-by-one，套件 fixture 已处理）。
5. **两阶段回退交互（P0 评审定音）**：官方 global 阶段（market 处理器）
   **无 throw 路径**——逐 intent 静默跳过（getInterRoom 后的处理体全部
   early-return 语义），故我方 `settle_market` 为不可失败阶段，挂在房间
   commit **之前**对已结算工作副本运行：失败原子性（任一房 Err 全弃）与
   「generation 恰 +1」（room_mut/get_mut 保守 bump 与 commit 后二次变异
   冲突）双不变量原样保持；官方「房间结算全部完成后才跑 global」的因果
   序等价（读到的都是本 tick 结算后状态）。P1 挂点落地时评审专项核。
6. **capability 翻真的涟漪**：terminal=true 会解锁 CTRL-STRUCTLIMIT 等已有
   实装面的用例（预期绿）——若翻红说明 P4/P5 读取面有暗坑，逐条归因不
   许豁免洗白。
7. **countStub 桩撤除的探针面**：Game.market 桩现有 probe/countStub（drain
   消费）——真化后撤桩，P6 撤五桩先例（官方全源 grep+bot 零消费）同流程。
8. 工具链坑全表适用（memory 清单）：python 补丁锚点含 `#[test]` 属性行 /
   zsh `===` / ratchet 前 release 重建 / 跑完 kkscreeps.json 入库 /
   commit subject 限制 / V8 测试并入串行链不开新根 / 官方注释行号 grep
   实树取号。
9. **测试墙钟耦合（P2 评审 3 F-G 登记）**：市场订单 created_timestamp 与
   `TickInput::now_ms` 两关注点共用 BENCH_NOW_MS=1.7e12 常量；裸
   `tick_with_intents` 缺省真实墙钟（settle/mod.rs:151）——任何
   place_order+裸 tick 组合会把订单当 2.7 年旧物静默过期（失败形态响亮
   但触发前不可见）。后续批市场测试一律走 market_tick/固定 input 助手。

## 8. 执行记录（随片回写）
### P2（市场片：订单簿全生命周期 + Game.market 真化）——已收口（含三路评审清偿+清偿复审）

- **交付**：settle/market.rs process_market_intents（**分节消费**官方段序
  createOrder→changeOrderPrice→extendOrder→cancelOrder→deal 收集——稳定
  划分保段内注册序；跨玩家序=名字序↔db 插入序已登记）+execute_terminal_deal
  （**买卖定向 SELL→seller=订单房/buyer=目标房**——初版写反致 DEAL 族四红，
  实测勘正；三重钳+money 钳链 deal_cost=min(毫×量,buyer money)→floor 回推）
  +execute_direct_deal（intershard 资源+钱直转）+order_lifecycle（**只见
  tick 前存量** pre_tick_ids 过滤——官方尾段遍历 getInterRoom 快照，同 tick
  建单延迟落库下一拍才同步；_cancelled 移除/过期退 remaining×毫price×FEE
  截断 i64【亚毫登记】/SELL active·amount 同步/BUY newAmount 三取 min）；
  intent.rs MarketIntent 五变体+to_price_milli（≥1e21 指数首位 quirk；
  [2^63,1e21) 表示强制 None 登记可达偏差）；prelude makeMarket 真化（9 方法
  全门+四 getter Object.defineProperties {enumerable,get}——严格赋值抛
  TypeError/方法字面量三 true/orders 惰性 indexBy 同 tick 同对象/
  cachedOrders 原型继承键官方怪癖保留；getHistory {} 形态官方同红保真）；
  embed money 种子 10M 毫（vanilla adapter 同位）+market_view（orders 分组
  原始毫价/my_orders 含 inactive/history {all:[]}）+placeMarketOrder RPC+
  adapter；capability market/marketBasics 翻 true；countStub 市场桩撤除
  （getAllOrders 真面接手——tooangel 每 tick 消费，console 零错）。
- **实测**：workspace 353 绿（settle m2b2 族 22 测）；棘轮 **1828→1852
  （+78 绿零未豁免转红，155=154+1）**——转绿分布【评审 3 勘正原 73+2+3
  分解】：市场族 69（TERMINAL-SEND 47+MARKET-ORDER 10+MARKET-DEAL 5+
  MARKET-QUERY 6+SHAPE-GAME-007 1）+terminal 邻接 7+INTENT-LIMIT 2；
  新红恰 1=MARKET-QUERY-006 **官方同红**（vanilla.json failed 实证——
  getHistory 非法资源返 {} 非数组，套件期望数组；note 落 ratchet-allow
  不入 allow[]——skip→failed 不触发翻转门，入内成永惰性条目）。三基线
  与 P1 形态逐字一致（tooangel 3 形态/overmind withErr=1 Memory.flags
  随机后缀/wasm 干净；唯一增量=getAllOrders 桩探针消失，§4 勘误归因）；
  perf 同日 A/B：project +6~8%（market_view 每_viewer 投影新字段——空
  orders 态 ≈3µs/Viewer 分配上限附近归因 plausible；**非空态 O(订单×
  Viewer) 缩放登记待评估**），run/settle 持平，wall ≤+3%，workload 锚
  3328/2836/2366 逐字节一致。
- **三路评审（官方语义+镜头①/Rust+镜头②/测试与归因）**：评 1 判 FAIL、
  评 2/3 PASS with conditions——**镜头①再立功**（费门活值/尾段快照/段序
  三项全是跨子系统时序）；镜头②描述符零瑕疵（四 getter 三标志+闭包缓存
  逐位对照官方 market.js:194-250）。清偿清单（每项官方源 grep 实证后修）：
  ①**费门活值读**（评 1 F1 P0=评 2 F1：官方 user.money 逐 intent 重读+
  bulk.inc 立改内存 bulk.js:90-97，我方循环头快照会让 Σfee>credits 全额
  扣款打负 money——改逐门 get 活值+测试 money 恰够单笔时次笔拒且不为负）；
  ②**尾段快照过滤**（评 1 F2=评 2 F2=评 3 F-B：遍历全集使同 tick 建单提前
  一拍进公共池——pre_tick_ids 过滤+新测「创建 tick active=false/amount=0、
  下一拍同步」；原 same_tick_invisible 测试注释「尾段也不跑」与实现相反
  一并勘正）；③**负改价门 `<=0`**（评 1 F3=评 2 F2：仅拒 ==0 会落负价反转
  deal 资金流向；负改价不置 _skip 官方先于 skip 段 return——deal 按旧价照常
  成交新测锁定）；④**分节消费**（评 1 F4=评 2 F3：单趟声明序 cancel 先行
  把「提价+取消」的费算成 0 吞掉——sort_by_key 稳定划分按官方段序，新测
  fee=50000 按 remaining=1000 收取；评 2 F3b「跨玩家全局分节」主张过强，
  官方外层 userIntents.forEach 按用户循环——跨用户可见性由用户序定，我方
  按官方结构同构实现）；⑤**_skip 测试判别力**（评 1 F5=评 3 F-A：原夹具
  dealer money=0 绿在 money 钳、且名字序 dealer 先于 owner 令 skip 不可达
  ——夹具对调 owner/dealer+dealer 注资+无扣款断言，变异存活归零）；
  ⑥饱和加三处（评 2 F5：注入域巨价 refund/deal_cost 溢出 panic/回绕）；
  ⑦deal 后目标 terminal 冷却断言（评 3 F-C 原零锁——删 write_cooldown
  全链不红）；⑧floor 非整除例 304000/2500=121（评 3 F-D：原整除例
  round/ceil 同值无判别）；⑨墙钟常量 BENCH_NOW_MS 抽取（评 3 F-G）。
  **变异取证**：①③④及尾段过滤四变异全部击杀对应新测，复原后 203 绿。
  登记项（评 2）：orders BTreeMap 字典序 vs 官方插入序（纯序面 "mkt10"<
  "mkt9"，逻辑面按 id 直查不依赖）/price=0 注入订单 BUY 激活分歧（官方
  money/0=Infinity→active；deal 面两侧同收敛）/无主 terminal NPC 分支
  （官方 if(seller.user) 含无买家钱凭空入账怪癖，我方整单中断保守侧）/
  game.js deal 能量门缺键 `||0` 收紧（store 恒含 energy 键不可达）——
  四项注释登记+灰区表收编；ExtendOrder/CancelOrder 引擎臂外部独锁与
  intershard 拒因三闸无锁（评 3 F-E）登记灰区表。
- **清偿后全门禁复跑**：workspace 353 绿/clippy 0/棘轮 **1852/155/907
  零用例位移**（四项行为修复零套件位移——官方对齐修复与套件无交集的
  再证）/三基线全等/perf 持平（project 0.0809/run 0.1686/settle 0.0200/
  wall 0.3023——首轮机器态偏高回落）。**评审 3 流程事故登记**：其对测试
  文件 git checkout 抹掉未提交 P2 改动后按自读内容恢复，清偿编辑在其
  恢复版上干净叠加（22 测+四变异击杀均在叠加后验证）——终态全量门禁
  覆盖该风险；教训=评审 agent 不得对未提交工作树做 checkout 类破坏性
  回滚（后续评审 prompt 加禁令）。

- **交付**：Action::Send（白名单 system.js:69）+IntentData target_room_name/
  description；structures.rs terminal 臂房间相执行器（send.js 逐字——只校验
  落 data.send 字段）；settle/market.rs settle_market 全局阶段（send 消费链
  market.js:71-95+executeTransfer :15-69——**空串 owner 的 JS falsy 门**
  【m2b2 测试抓出】：官方 `!terminalsByRoom[x].user` 对空串同 undefined 拒，
  我方 is_some 误放行→filter(!is_empty) 清偿）；calc_rooms_distance（**不环绕**
  plain Chebyshev——官方 MMO 经 getWorldSize 环绕，登记差异；liveWorldSize
  键维持 false）/calc_terminal_energy_cost/is_valid_room_name；挂点=commit
  循环前对 settled Vec（失败原子性+generation 恰+1 双不变量保住，官方
  commit1→global 因果序等价）；economy terminal_capacity_tick（tick.js RCL
  归一）；prelude StructureTerminal.send 九步校验序（013 矩阵 36 变体锁序）
  +makeMarket 官方形装配（market.js:29-253：9 方法+四 getter 描述符逐字；
  calcTransactionCost 真化+transactions getter 真化+闭包缓存）；embed
  market_view per-viewer 投影合并（world JSON market 键——投影态不走 V8
  host 通道）；capability terminal/terminalSend 翻 true。
- **实测**：workspace 339 绿；棘轮 **1774→1828（+54 绿零未豁免转红）**——
  TERMINAL-SEND 47 全绿（002/004 power 双门、015 liveWorldSize 维持 skip）
  +邻接 7 绿；**SHAPE-STRUCT-001:terminal skip→failed 归既有 Object Shape
  红家族**（家族 12 条同因【terminal 在内】：装配器 `base.isStructure`
  实例标记+ELSE_FIELDS 实例级 energy/energyCapacity 兜底+原型链多键——
  家族整体归 Object Shape 批处理，
  非本片回归；§4 预告 +55 面勘正为 +54+1 家族红）；三基线形态一致；perf
  同日 A/B project −0.1%/run −1.0%/**settle +3.9%（+0.0008ms 绝对——市场
  阶段每 tick 全房 Generic 扫描的预期成本）**/wall −0.6%，确定性锚逐字节一致
  （progress_delta 3328/source 2366 全等）。
- **三路评审（官方语义/Rust 正确性/测试与归因）全 PASS with conditions 零 P0**，
  清偿清单：
  - 评审 1 P1-1/P1-2：fmt+clippy 硬门（collapsible_if 两处——market.rs 接收方
    写回/economy.rs capacity 写回塌缩为 let 链）；workspace fmt/clippy 零告警。
  - 评审 1 P2-1（**玩家可达**）：空串 description 交易记录——官方 :91 falsy
    无键，我方 Some("") 投影泄漏 → `.filter(!is_empty())` 清偿。
  - 评审 2 P1-1/评审 3 P1：测试判别力变异存活两处——全局相能量门（补
    energy=cost−1 拒收测试）与冷却门（夹具补 H 资源使冷却门成唯一拦截）。
  - 评审 2 P2-2/评审 1 P2-4：terminal_capacity_tick 无主早退偏离官方
    （官方无 object.user 存在门、`undefined != 0` 写 0）→ 重写为官方语义
    （Option 相等比较含 None==None 真；storeCapacity 键缺失≠0 写回）+
    异主/同主 RCL 8 判别用例。
  - 评审 1 P2-2/评审 2 P2-4：structures.js C 表 RESOURCES_ALL 重复键（后键
    击穿 A4-i 私有冻结拷贝纪律）删除。
  - 评审 2 P3-1：pass1 send 收集 filter(is_object) → 官方 `!!i.send` JS
    truthy 语义（非 false/0/""/null 均收集）。
  - 评审 3 P1：e2e 最小集（embed 串行链 +m2b2_terminal_send_e2e：九步
    错误码链/跨 tick TIRED/成功链/transactions 视图映射/description 转义/
    credits getter——P1 交付面原零覆盖清偿；冷却账 embed 侧 runPlayer=
    执行+结算+推进一拍，实测对齐）。
  - 评审 3 P1/评 2 P2-3：归因勘正包——floor 1828、§8 家族计数 13→12、
    §2/§3 数字协调（79→78 上限）、system.js:80→:69、SHAPE-GAME-007
    描述符「过度声明」措辞降级（键集可过、三标志 P2 e2e 补锁）、夹具
    冗余块坍缩、m2b2 宽松析取收紧、capacity docstring 与夹具不符勘正。
  - 评审 3 P2：run-suite.sh **dist 新鲜度守卫**（src 新于 dist 即重建——
    陈旧 capability 跑门禁的 P4 同类事故面）；skip→failed 翻转盲区本轮人工
    差分（153→154 恰 1 条 SHAPE-STRUCT-001:terminal）——flip 语义维持
    （基线绿保护），解锁面 failed 池差分入收尾清单。
  - **登记（不清偿）**：无主 from-terminal 全局相复验分歧（官方 `from.user
    &&` 前置跳过复验可写负 store——裸 intent 注入域可达，prelude+processor
    双门使玩家面不可达）；room 相负 amount+缺键角落（终态等价中间态差）；
    交易视图键序/NPC undefined 键形态（P2 deal 链）；Transaction.id 裸十进制
    串；裸 Math 家族债（send cost 算式——Link.transferEnergy 同族，随家族
    批统一）；market_view O(1000)×2×玩家扫描（perf 实测包络内，P2 订单簿
    批评估 per-player 索引）；NaN 因果注释勘正；game.js roomNameToXY 注释
    勘正（宽松 NaN 不抛）。
- **验证终态**：workspace **342 绿**（+11 m2b2/+1 e2e 串行链）；棘轮
  **1828/154/932 零未豁免转红**两轮复跑稳定；三基线与 P6 后形态一致；
  perf 同日 A/B 四相位噪声级（settle +3.9%=+0.0008ms 市场阶段扫描成本）
  确定性锚逐字节一致。

### P0（地基：域态 + intent 全局市场族）——已收口（含三路评审清偿）

- **交付**：`settle/market.rs`（MarketState 订单簿/交易流水——TRANSACTION_CAP
  1000 有界截断【登记差异：官方 db 无界+视图各 100】、`mktN` 十进制 id
  【登记：官方 ObjectId，套件不透明消费】、next_order_id/alloc_transaction_id
  生成器、transactions_for 方向过滤+入账倒序+limit【同刻序官方 db 未定义，
  我方入账倒序钉死，登记】）+ `PlayerState.money`（毫 credits，缺省 0——
  官方引擎不种子、vanilla adapter 种 10M=套件层职责）与 `resources`
  （intershard 划转面，缺省空不伪造来源）+ `Engine.market` 域态字段 +
  `intent.rs` MarketIntent 五变体/UserIntents.market/from_json 'global' 容器
  臂（迭代序=官方 intentTypes 声明序 cancelOrder→changeOrderPrice→
  createOrder→deal→extendOrder【跨动作序不可观测：处理器按分节序消费，
  system.js:23-29 与处理器分节序锚】）+ to_plain_string/to_price_milli
  （toFixed(0) 取大 n ≡ round() 远零半舍，node 实测 0.0005→1/−0.0025→−3
  双侧吻合）/parse_market_intent 三 helper。
- **挂点/注入通道 re-slice 至 P1**（§3 已勘正：不可失败阶段+提交前跑；
  world JSON 投影合并替代 V8 host 通道）。
- **验证**：workspace 331 绿×2 轮；release+棘轮 **1774/153/987 零翻转**
  （P0 设计约束「零玩家可见面变化」成立——perf 同日 A/B workload 确定性锚
  逐字节一致【progress_delta 3328/source energies 2836·2366 全等】，各相位
  非劣 −3~−13%）；三基线与 P6 后形态一致（tooangel console 零错 2000t/
  overmind withErr=1 tick1 Memory.flags【仅随机 directive id 后缀与 PathFinder
  计数 ±4 漂移——bot 自身非确定性】/wasm 干净）。
- **三路评审（官方语义/Rust 正确性/测试与归因）全 PASS with conditions
  零 P0/P1 阻断**，清偿清单：
  - 评审 2 P1-1：to_price_milli 负向下界对称守卫（|milli|≥2^63 饱和截断
    →None，与 to_int 先例对齐）+边界测试（−0.5→−500/−0.0025→−3/0→Some(0)/
    true→1000/−1e19→None）。
  - 评审 1 P2-1/P2-2：MarketIntent 文档勘误——order_type=None 在 P2 处理器
    **必须直接拒不得默认 buy/sell**（官方 :139 物化串默认落 ORDER_BUY，
    等价性纯靠 prelude :75 不可达）；deal.amount NaN 首门是比较门
    （`amount<=0` 对 NaN 假）非 falsy 门——P1/P2 处理器注释契约。
  - 评审 1 P2-3：'global' 容器官方还承载 power-creep 六键（power 处理器
    先于 market，processor/global.js:41-49）——M4 power 批进驻本分支的
    注释锚。
  - 评审 1 P2-4/评审 2 P2-2~P2-4/评审 3 P1-3 合流→**清偿复审 P1-a 再裁
    决**：to_price_milli 分歧域终口径——①ToNumber 分歧串真不可达
    （parseFloat 门）钉 None；②[2^63,1e21) **表示域强制 None 且玩家可达**
    （remaining=0 订单 changeOrderPrice 费恒 0 放行任意 newPrice——官方改价
    生效/我方跳过，已知偏差登记，套件零覆盖；初判「fee 门钳死不可达」被
    复审推翻：费公式含 ×remainingAmount，remaining=0 费恒 0）；③≥1e21
    toFixed 指数串被 parseInt 截首数字——同洞玩家可达，**保真实现**（node
    实测 1e18→1/2.5e18→2/−1e18→−1，边界测试锁定）；to_plain_string 数组/
    对象 ToString 分歧登记。
  - 评审 1 P2-5：九处官方行号 citation 勘正（storeIntents global 分支
    :421-124/system.js :23/:24/:29/:5/:8/data.js getCachedMarketHistory
    :51-75 等）。
  - 评审 3 P1-1/P2：方案 §0/§2/§4 计数勘正（82→83、74→73+2+8、63.55→
    63.59）；extend_from 合并序锚定、transactions_for 同刻序/limit=0/空
    ledger 测试、MARKET-ORDER 002 双形态标签注。
  - 评审 2 P2-5：alloc_transaction_id 生成器（消只写死字段）+单调测试。
  - 评审 2 P2-8：非数组真值动作键（官方物化退化 intent 后下游门全跳 vs
    我方整键丢弃——pushByName 恒数组双侧不可达）登记于 parse_market_intent
    注释链。
- **灰区表登记**（compatibility.md P0 行）：TRANSACTION_CAP 有界、同刻交易
  序、'mktN' id 形态、sender/recipient 玩家名键制（官方 user _id，视图层
  适配）、intershard 订单 roomName None vs 官方 "undefined" 字面串、
  to_price_milli 三分歧域、非数组动作键。
