# M2A-A5-lite 接线批（B3 接线 + upgradeController 补漏）

基线 main=e8445ae（A3c+B3+C3 已合）。本批是 A5 的先行子批：点亮钥匙优先。

## 内容

1. **Tombstone/Ruin 玩家类接线**（官方 tombstones.js:41-135 / ruins.js:38-70）：
   defineGameObjectProperties 字段表（deathTime/ticksToDecay 折叠值直读/store
   makeStore/carry 族）+ creep 代理对象（id/name/spawning=false/my/body(hits:0)/
   owner/ticksToLive/carryCapacity/carry/fatigue=0/hits=0/hitsMax/saying——
   官方 defineProperties 覆盖形态同构，Creep 原型链）+ structure 代理对象
   （user 有→OwnedStructure 形含 owner/my，无→Structure 形；structureType 读
   存照 type 键——官方 getter 写 o.structureType 但 DB 键在 structure.type，
   vanilla snapshots.ts:394 同构）；assemblers 透传壳
2. **room.getEventLog(raw)**（官方 rooms.js:573-583）：世界投影 rooms 条目注入
   events（B3 RoomState.events 落账，仅可见房——官方 roomEventLog 按可见房
   data.js:141/:219）；raw=true 经预置 stringify 返回字符串形态
3. **Structure.destroy() 换真**（官方 structures.js:72-86 三门：!room/
   controller 非我/房内敌方 creep·powerCreep——FIND 常量走冻结 C 表防判定门
   失明；room 级 intent 经 pushByName('room','destroyStructure',{roomName,id})
   数组通道——from_json "room" 分支同形）；**STRUCTURE_METHODS 桩列表移除
   destroy（坑⑥复发实证：实例级桩遮蔽基类真方法，症状假 OK 零 intent）**
4. **prelude room.find 补 118/119/122/123 四臂**（FIND_TOMBSTONES/POWER_CREEPS/
   DEPOSITS/RUINS——后两类无对象恒空，臂在保持与 Rust matches_find 同构）
5. **upgradeController 换真**（官方 creeps.js:922-964 十门——shardAccess 门
   hosted 面不可达登记跳过；执行器 GPT boost 批已完善）；transfer(控制器,
   能量) 重定向自此走真门（既有矩阵断言 -12=无 WORK 真门，正路径新测试
   redirect_upgrade_positive_impl）
6. **pushByName 补计费**（make.js:100-106 按元素 0.2——JS 显示面，Rust 结算
   面原有）

## 测试

- sandbox_tests +2：destroy_room_intent_channel（room 通道形态+计费）、
  redirect_upgrade_positive_impl（重定向+直调双 OK）
- embed 冒烟全链：destroy→intent→结算→废墟契约七键→玩家面 Ruin 实例/
  structure 代理/store/ticksToDecay + getEventLog 事件（event=2）；Tombstone
  类/ticksToDecay/store/creep 代理（instanceof/hitsMax/body）
- workspace 全绿 + fmt + clippy 零警告

## 发现与登记

- 坑⑥复发实证（桩覆盖序）——灰区登记
- shardAccess 门不可达——灰区登记
- pushByName 计费差（JS 显示面）——已补

---

# 三路评审之三：Rust 正确性与测试充分性（6093f7b）

评审人独立复跑全部回归并补做未入库的 embed 冒烟复验（临时测试，跑完即删）。
结论：**可合入**（0×P1 / 0×P2 / 6×P3——全部为登记项与测试面建议，无阻断）。

## 一、world_json events 注入（crates/kk-world/src/world_json.rs:65-75）

- **filter_map().ok() 破损行静默跳过的可达性——验证成立，不可达**。`RoomState.events`
  的全部写源：①结算侧 settle/mod.rs:509 `w.events.iter().map(Value::to_string)`
  （serde_json `Value::to_string` 恒产合法 JSON）；②快照装载 world.rs:98（快照
  本身由①的字符串 encode，kk-schema roundtrip 不改内容）。手写世界 JSON 走
  prelude 注入面（sandbox begin_tick），不经过 typed `RoomState`——`events` 键
  无法从外部注入坏行。唯一向量是快照文件磁盘损坏，`.ok()` 跳过在该场景下是
  合理的降级（投影面不 panic）。**P3-1**：建议加 `debug_assert!` 或 console 告知行
  （当前静默，快照损坏时事件凭空消失无诊断线索）；非本批必须。
- **仅可见房注入的判定位置**：注入在 `if visible.contains(name)` 块内（与
  energyAvailable 同一块）✓。纵深验证（embed 实跑）：JS 装配层对不可见房根本
  不构造 Room 对象（game.js:231-236 visibleRooms 过滤）——玩家读
  `Game.rooms['W2N1']` 得 undefined，Rust 侧 events 裁切是不可达时也正确的第二层。
- **None 面（world_json() 全量）行为确认**：visible=全房，events 同样注入。消费面
  grep 全仓仅 snapshot.rs:711 测试引用——与 energyAvailable 的 None 面口径一致
  （"M1 全量投影，测试/内部消费"），一致性成立。空 events 不落键（`is_empty`
  守卫）→ prelude `getEventLog` 的 `(r && r.events) || []` 兜空数组，形状闭环。
- **P3-2（fidelity，非本批引入的新面）**：`getEventLog()` 非 raw 分支返回注入数组
  的**活引用**（room.js:187-190）——玩家 mutate 返回值会污染同 tick 后续读取；
  官方每次调用 `JSON.parse` 返回新鲜拷贝（rooms.js:573-583）。bot 生态无此用法
  （ratchet ROOM-EVENTLOG 全族通过），登记即可。

## 二、测试充分性

**新断言推演（均成立）**：

- `destroy_room_intent_channel`（sandbox_tests.rs:2594-2627）：锁三层——
  intentsList 的 `room.destroyStructure[0]` 数组通道形态（与 IntentBatch::from_json
  的 "room" 分支 intent.rs:388-407 同形闭环）、`intentsCpu==0.4`（JS 显示面：
  move 0.2 + pushByName 补计费 0.2；Rust 宿主面 take_endtick_intents 的
  `as_array().map_or(1, len)` 按元素计费同口径兜底，双轨各自验证）、对照组 move
  走对象级通道。断言有效且互不冗余。
- `redirect_upgrade_positive_impl`：WORK+carry 邻接自家 controller 十门全过 → 0，
  加直调同路径 0。与矩阵 -12（见四）构成正负双锁。
- **embed 冒烟声称与入库状态不符（P3-3，报告 vs 树）**：报告"测试"节声称
  "embed 冒烟全链：destroy→intent→结算→废墟契约七键→玩家面 Ruin 实例/
  structure 代理/store/ticksToDecay + getEventLog 事件（event=2）；Tombstone 类/
  creep 代理（instanceof/hitsMax/body）"——**这些没有任何一个进树**（embed_tests.rs
  无 destroy/ruin/getEventLog 玩家面用例；grep 全仓无）。评审人以等价临时测试
  独立复验**全部成立**（含：ruin instanceof Ruin、structureType 读存照 type 键、
  structure 代理 id/structureType、store.getUsedCapacity=120、ticksToDecay 折叠、
  getEventLog 数组/raw 字符串/event=2/data.type=container、Tombstone creep 代理
  instanceof Creep/hitsMax=body×100/body 序/part hits=0/carryCapacity=CARRY×50/
  hits=fatigue=0/spawning=false/owner.username/my、事件覆写口径（隔 tick 读为空）、
  不可见房事件裁切）——即声称的行为真实，但测试工件缺失，声称从仓库不可复核。
  建议把该冒烟固化为 embed_tests 用例（成本约 60 行）。
- **缺口清单与本批是否必须**：Tombstone/Ruin 玩家类 getter 族、getEventLog
  raw/非 raw、events 可见房裁切/None 面，在 cargo 级均无锁值——但 ratchet
  （CI 第四 job）已点亮 TOMBSTONE-004/006-016、RUIN-005/007、SHAPE-RUIN-001、
  ROOM-EVENTLOG-001/003/004/005/027、STRUCTURE-API-001/002/003/007——screeps-ok
  经 embed run_player 走的正是真投影+真类，**行为级验证已成立**（对照 +102
  棘轮点亮）。缺口定性为 cargo 级回归速度/离线可跑性的卫生项（P3），非合入
  阻断；其中 destroy→ruin 玩家面链最值得优先固化（它串联本批三项接线）。

## 三、+102 转绿归因抽查（compat/reports/kkscreeps.json vs e8445ae 基线）

实跑逐用例 diff：1181→1283（+102，零转红、零用例总数漂移）。**91/102 直接
归因**本批四项：CTRL-UPGRADE-002~016（含 013 十门全序矩阵 28 用例）、
TRANSFER-011 重定向、TOMBSTONE/RUIN 衰减与代理族、ROOM-EVENTLOG 全族（含
005/006/027 destroy 事件）、STRUCTURE-API/STRUCTURE-HITS-004、SHAPE-RUIN-001、
CTRL-DOWNGRADE-003。其余 11 条间接归因成立：8 条 UNDOC-STALEARG/STALERECV
（陈旧结构引用拒——依赖 destroy 真移除对象，桩时代对象不消失故拒不了）+
CTRL-DOWNGRADE-012/013（upgrade 每tick restore 记账——依赖真 upgrade intent）。
归因合理，无混入无关转绿。

## 四、既有断言改动（transfer 重定向 -12）

resource_family 矩阵 ：449-451 断言 0→-12：夹具 w1 body=["carry","carry"]
（sandbox_tests.rs:2043）纯 carry——重定向进真 upgradeController 后门③
`countActiveParts(WORK)===0` → ERR_NO_BODYPART(-12)。**-12 本身证明重定向
命中真方法预检链**（桩返回 0 会 fail），是比旧假 OK 更强的锁；旧 0 行为由
redirect_upgrade_positive_impl 正路径重锁。非能量→controller 仍走容量门 -7
（矩阵既有断言未动）✓。改动正确。

## 五、回归面（全部实跑）

- `cargo test --workspace`：全绿（29 个 result: ok，0 failed，含 sandbox/settle/
  embed/pathfinder 全套）
- `cargo fmt --all -- --check`：零输出
- `cargo clippy --workspace --all-targets`：零警告
- embed 冒烟：见二（评审人临时复验 PASS，工件已删，树内状态见 P3-3）

## 六、events 注入对 bot-check 的影响

**零影响——bot-check 不走 world_json**。kk-cli/bot_check.rs:252 `home_world(t)`
是手写 JSON 直供 prelude（pool.run_player(PLAYER, &home_world(t), ...)），
不经过 typed World 投影；events 注入对 bot-check 载荷/性能面无任何变化。
真实消费面是 embed run_player（screeps-ok 套件）：每玩家每 tick 对每个可见房
做 events 的 from_str + 整世界 re-stringify（sandbox.rs:292）——**P3-4**：
相比官方（roomEventLog 持字符串原样下发、getEventLog 时才 parse）多两次
拷贝，量级 O(房 events 数)，战斗房最坏数百条/秒级 tick 亦在毫秒级；当前
套件跑时无明显变化（ratchet 重跑通过）。远期优化方向：投影期直透字符串、
JS 侧 parse（同时根治 P3-2 的活引用分歧）——登记，非本批必须。

## 结论

| # | 级 | 条目 | 处置 |
|---|----|------|------|
| P3-1 | 3 | events filter_map 静默跳过无诊断线索（不可达已论证） | 建议 debug_assert/告知行 |
| P3-2 | 3 | getEventLog 非 raw 返回活引用（官方每调新鲜 parse） | 灰区登记 |
| P3-3 | 3 | 报告声称的 embed 冒烟未入库（行为经评审人复验为真） | 建议固化为 embed_tests 用例 |
| P3-4 | 3 | events 注入 from_str+re-stringify 双拷贝（对 bot-check 零影响） | 远期直透字符串方案 |
| P3-5 | 3 | Tombstone/Ruin/getEventLog/裁切缺 cargo 级锁值（ratchet 已行为级覆盖） | 建议随收口批补 |
| P3-6 | 3 | ratchet 地板未随批提升（floor 仍 1181，实际 1283） | 收口批一并 bump（B3 先例 3f65942） |

**总评：可合入**。四项接线（B3 类接线 / destroy room 通道 / upgradeController
十门 / pushByName 计费）实现与官方形态对表成立，+102 归因干净零转红，回归面
全绿；全部发现为 P3 登记项，无阻断缺陷。

---

# 三路评审之二：防篡改/装配纪律（6093f7b）

评审人：对抗视角。方法：prelude/Rust 双层代码对照官方源
（~/Desktop/projects/sources/screeps）+ embed 冒烟探针三组（判定门正负路径、
壳属性伪造、代理读写隔离，探针脚本 /tmp/a5_probe{1,2,3}.py，echo RPC 直驱
`kkscreeps embed`；createShard 需 rooms+players）。结论：**可合入
（0×P1 / 1×P2 / 10×P3）**。防篡改纵深完整成立：JS 判定门可绕（壳属性可写，
与官方构造器定格面同形）→ Rust 结算层四门/十门真值复核全部拒伪造——三组
伪造探针（壳 id 嫁接/假房对象/假 pos）零提权。

## 1. 代理对象攻击面（tombstoneCreepProxy/ruinStructureProxy）——通过

- `__Object.setPrototypeOf` 预置捕获 ✓（structures.js:810/:840）。raw 投影
  `o` 闭包捕获、玩家不可达：全部 getter 只返回副本；`__kkp` 装载后删除
  （探针 `typeof globalThis.__kkp === 'undefined'`）、`rawById` 闭包私有。
- **getter 返回值可写性同构论证成立（探针实证）**：`c.body` 每次 `.map`
  新建数组——污染 `c.body[0].type='attack'` 后再读 `.body` 仍得原始
  `[work,carry,move]`；官方 tombstones.js:68-71 `_.map(o.creepBody,...)`
  同为每次新建。`store/carry` 走裸 defineProperty getter（不经
  defineGameObjectProperties 缓存），每次 `makeStore(o)` 新 Store——玩家写
  `store.energy=999` 只落在当次实例。
- `t.creep === t.creep` 为同一实例（defineGameObjectProperties 的
  `if(!this['_creep'])` 缓存）——官方 utils.js:522-527 逐字同款缓存，同构
  非偏差。对缓存代理 `setPrototypeOf(c,{evil:true})` 不污染类原型（探针
  `[c2.evil===undefined, proto===Creep.prototype, Creep.prototype.evil===
  undefined]=[false,false,true]`——被改的是缓存代理自身，类面干净；官方
  缓存实例同样可被玩家改，自害面一致）。
- ruinStructureProxy 的 user 分支形态（有主→OwnedStructure 形含 owner/my，
  无→Structure 形、my undefined）与官方 ruins.js:43-68 逐字段对齐 ✓。
- 继承面 fail-closed：`ruin.structure.destroy()`（经 Structure.prototype
  继承，探针 `typeof s2.destroy==='function'`）读 `this.room`——代理无
  room own 属性 → ERR_INVALID_TARGET（官方 `new globals.Structure()` 无
  room 同款）。
- **P2-1（正确性缺口，非安全）**：tombstoneCreepProxy 缺 `pos`/`room`——
  官方 `new globals.Creep()` 后 `RoomObject.call(creep,o.x,o.y,o.room)`
  挂两者（官方 rooms.js:1648-1655）。探针 `'pos' in c === false`、
  `String(c.pos)==='undefined'`。bot 读 `tombstone.creep.pos`（死亡爬虫
  位置）得 undefined。修法：defs 补
  `pos:{get:()=>new gw.RoomPosition(o.x,o.y,o.room)}`。
- P3-7：缺官方 powerCreepId 分支（tombstones.js:135+ 的 PowerCreep 代理）
  ——power creep M4 未建模，当前世界不可达，随 M4 登记。
- P3-8：官方 owner getter 经 `runtimeData.users[o.user].username`（未知
  用户 throw）；我方直投 `{username:o.user}`——更宽容，无判定面影响。

## 2. destroy 判定门——通过

- `C.FIND_HOSTILE_CREEPS/FIND_HOSTILE_POWER_CREEPS` 模块加载期数值捕获
  （constants.json 有 103/121 两键）✓。探针：玩家覆写全局
  `FIND_HOSTILE_CREEPS=999` 后 API 判定不受影响——面板可写是官方同形
  （官方 game 层 C 亦闭包自 driver.constants，玩家覆写全局不影响判定）。
- `this.room.find` 的 MY/HOSTILE 臂经 `gw.viewerName` 闭包 + worldObjs
  索引迭代 ✓。
- `this.room.name/this.id` 是 probe 壳上的可写 own 数据属性（wrapObject
  base 构造定格面；官方构造器赋值同形可写）——篡改面存在，纵深在结算。
  **三组伪造探针全部结算层拒**：
  1. 假房对象整体替换（`controller:{my:true}`、`find:()=>[]`——JS 三门
     全绕）+ `roomName='W2N2'`（敌房）+ `id=敌房 container` → JS 返回
     OK，结算 controller 归属门拒（getObject 对象仍在）；
  2. 同上但 `roomName='W1N1'`（自家房）+ 敌房对象 id → 结算在本房
     `objects.get(&id)` 查无此 id → 拒（跨房 id 嫁接不通）；
  3. 正负锁：敌 controller 房 destroy=-1（NOT_OWNER）、房内敌 creep
     destroy=-4（BUSY）、自家正路径=0 且对象消亡+EVENT_OBJECT_DESTROYED
     下一 tick 落事件日志（`{"event":2,...,"data":{"type":"container"}}`）。
- 结算四门（settle/room.rs:56-84）：对象存在 + CONSTRUCTION_COST 类型 +
  controller 归本人 + 房内无敌 creep——对官方 destroy-structure.js；
  process_room_intents 的 `room == &w.room.name` 匹配使跨房 intent 落空。
- P3-9（注释失实）：structures.js:772-773 称 "controller/source/mineral
  经原型链继承 destroy"——source/mineral 我方与官方都 extend RoomObject、
  无 destroy（探针 `typeof source.destroy==='undefined'`、
  `Source.prototype→RoomObject.prototype` ✓——行为两面对齐，注释错）。

## 3. room intent 通道防伪——通过

- `pushByName` 仅经 `gw.intents`（resetIntents 闭包私有，官方 runtime.js
  :60-100 同形）；`__kkp` 删除后不可达；探针 `__kk` 冻结（赋值后
  `__kk.x===null`、`Object.isFrozen===true`、`beginTick` 仍在）——宿主
  通道不可换。
- 出口：endTick 信封 nullEnv + `sanitizedStringify`（根原型 toJSON 摘除
  窗口覆盖 room 数组通道，90_host.js:159-172）；`pushByName` 内
  `setPrototypeOf(data,null)` 逐条剥原型——继承 toJSON 不可入载荷。
- `IntentBatch::from_json` "room" 分支：as_object/as_array 双形检查、
  destroyStructure 白名单 `{roomName:'string',id:'string'}`（官方
  system.js:20 同形——roomName 官方即不截断）、`to_id` 须 ObjId 可解析、
  未知名/非对象 'room' 键丢弃（from_json_adversarial_inputs 锁值）✓。
- P3-10（不可达不对称）：`intents.remove('room',name)` 若被调用只退 0.2
  而删整数组（N 元素）——唯一调用方 activateSafeMode 是对象级，结构性
  不可达。

## 4. getEventLog 出口——通过

- `__jsonStringify` 在 room.js:23 模块加载期捕获（IIFE 头部，先于任何
  玩家代码）✓——raw 分支不经可换全局。
- 注入面：world_json.rs:59-75 events 只在 `visible.contains(name)` 分支
  内——敌房/不可见房不注入；run_player 走 `world_json_for_viewer`。可见
  敌房（有自家 creep）的事件按官方 roomEventLog 可见房注入语义
  （data.js:141）——视野门内，非泄漏。
- 事件窗口=上一 tick 覆写口径（探针：destroy 的下一 tick 读得 event=2，
  再下一 tick 空）——官方单 tick 窗口一致。
- 同 tick 别名（非 raw 返回 `r.events` 活引用）与评审三 P3-2 交叉确认
  （同一发现，不重复计）。
- `setPrototypeOf(gw.world,null)` 只剥顶层、`r.events` 内层不剥——与官方
  注入侧"只剥顶层"一致（runtime.js:56），非偏差。

## 5. 桩覆盖序修复完备性（坑⑥清偿）——通过

- 全 prelude grep `mkStubMethod` 挂载点：creep.js:1048（creep 族桩）、
  sites-flags.js:38/55-57（site.remove/flag 族）、structures.js:642/756/
  760（setActive 桩）——**destroy 桩挂载点清零** ✓。
- 探针锁值：spawn/container/controller 的 `destroy ===
  Structure.prototype.destroy`（无遮蔽）；`setActive` 桩返回 0（A4-iv
  待换真面，既有登记）；继承链 StructureSpawn/StructureController→
  OwnedStructure→Structure、Tombstone/Source→RoomObject 全对 ✓。
- this.room 回填：buildGame 仅 `rooms[o.room]` 存在才回填——不可见房
  room=undefined → destroy INVALID_TARGET，fail-closed ✓。
- P3-11（陈旧注释）：structures.js:14 头部块注释仍写 "destroy/setActive/
  notifyWhenAttacked 保持桩"——destroy 本批已换真。
- P3-12（坑⑥同族隐患，基线已有非本批引入）：notifyWhenAttacked 双重
  定义（structures.js:608-633 与 704-731 逐字重复，后定义胜出）——当前
  无害，但正是"后挂遮蔽前挂"形态的样本，建议下批去重防再犯。

## 6. upgradeController 可交换全局面——通过

- `instanceof globalThis.StructureController`：类全局 A3c 非可写
  defineProperty + wrapObject 类表加载期定格 ✓。伪造 target 需原型嫁接；
  探针 `Object.create(StructureController.prototype)+真 id` 过门（结算按
  真对象处理，无放大面）。
- `countActiveParts` 模块级预置（creep.js:164）、`C.WORK` 数值捕获 ✓。
- **伪造 pos 探针（纵深实证）**：远距 creep `pos={inRangeTo:()=>true}` →
  JS 十门过返回 OK；结算 `intent_upgrade_controller` 按真位置拒
  （progress 不变）；邻接真路径 progress 递增——引擎侧距离/归属/
  spawning/能量全重验（settle/creep.rs:426-473）。
- P3-13：缺官方 `register.structures[target.id]` 检查 → 未知/过期 id 的
  target 在 `upgradeBlocked` getter 处抛 "Could not find an object with
  ID ..."（官方回 ERR_INVALID_TARGET -7，creeps.js:936）；同缺
  `assertTargetObject`（Memory 序列化 24 字符 id 的辅助报错——
  renewCreep/recycleCreep 有）。探针实证
  `['throw','Could not find an object with ID kk0000000099999999']`。
  过期 controller 引用跨 tick 的 bot 得异常而非 -7。
- P3-14：`countActiveParts` 用 for...of（可交换 Symbol.iterator）且全量
  计数 vs 官方 `_hasActiveBodypart` 尾扫+首死即断（creeps.js:20-29）——
  中段死部件时我方更宽容 + 迭代器污染可喂假计数；结算侧真值复核兜底
  （helper 非本批引入，登记备查）。

## 7. pushByName 计费增补——通过

- JS 显示面 0.2/元素（game.js:377）+ `Game.cpu.getUsed` 含 intents.cpu
  （:257）✓；Rust 结算面 `take_endtick_intents` 对数组通道按 `Vec::len`
  计 0.2/元素、排除 say/pull（sandbox.rs:421-433）——两侧同口径
  （destroy_room_intent_channel 锁 intentsCpu=0.4 = move 0.2+destroy 0.2）。
- 上行闸门：MAX_INTENT_OBJECTS=50k 按对象键截断；room 通道是单键数组、
  不受对象数闸门约束——但生成受玩家执行窗口（看门狗+堆上限）物理约束、
  计费线性跟随、结算幂等（探针：同结构 destroy×5 全返回 0、对象只消亡
  一次）——官方 make.js 同无元素上限，无绕过面。
- 玩家不可直调 pushByName（同第 3 条闭包论证）。

## 顺手发现（本批接线面相邻）

- P3-15：`Ruin` 的 toString 读 `o.structure.type`（_registry.js
  OFFICIAL_TOSTRING）但 `o.structure` 是代理（暴露 structureType 键）→
  探针 `String(ruin)==="[ruin (undefined) #kk...]"`；官方 ruins.js:69 读
  `this.structure.structureType`。B3 清偿把投影键改 `type` 时同步了
  toString，但 toString 消费代理层非投影层——修一行即愈（结构代理自身
  `String(s2)==="[structure (spawn) #kk...]"` 正确）。
- P3-16：FIND_NUKES(117)/FIND_MY_POWER_CREEPS(120) 两臂两层（prelude
  room.find / matches_find）都未实装——批内声明范围外，两层一致恒空，
  随 nuke/power 批次登记。
- 测试缺口与评审三 P3-3/P3-5 交叉确认同一发现：Tombstone/Ruin 玩家面
  接线无 cargo 级回归锁。P2-1 修复合并时建议一并固化（本评审探针场景可
  直搬：body 污染隔离 + creep 代理全字段 + getEventLog raw/数组双形态）。

## 总评（评审 2）

**可合入（0×P1 / 1×P2 / 10×P3，编号 P2-1 与 P3-7..P3-16）。** 三条主线
（destroy 换真+room 数组通道、坑⑥桩覆盖清偿、upgradeController 十门）在
防篡改维度全部成立：JS 层判定门与官方同为"可绕的预检层"，安全边界在 Rust
结算层的真值复核——探针实证壳 id 嫁接、假房对象、假 pos 三类伪造全被拒，
无提权路径；room intent 通道出口净化（nullEnv+sanitizedStringify+逐条剥
原型）与入口清洗（白名单+to_id 钳制+双形检查）双向闭合。P2-1
（tombstoneCreepProxy 缺 pos/room）是官方形状缺口、影响 bot 兼容性但不
影响安全，建议随下一接线批清偿。

---

# 三路评审之一：官方语义对照（6093f7b，基线 e8445ae）

对照源：官方 4.3.2 快照 `~/Desktop/projects/sources/screeps/`（行号允许漂移）。
我方行号均为 6093f7b 工作区。锚点缩写：官 `engine/src/game/`、官驱
`driver/lib/runtime/`。逐门/getter 对照 + `cargo test --workspace` 实跑复验。

## 1. Tombstone/Ruin 字段表（structures.js:785-855 ↔ 官 tombstones.js:41-135 / ruins.js:38-70）

- **deathTime/destroyTime/ticksToDecay/store 等价成立**。ticksToDecay 折叠
  投影（world_json.rs:515/:560 `decay_time.saturating_sub(time)`）对官方
  getter `decayTime - runtimeData.time`（tombstones.js:44/ruins.js:41）——
  `saturating` 仅在"已过衰减未扫除"不可达窗口与官方负值不同。store 两层
  `new Store(o)` 同形（官 tombstones.js:8-13/ruins.js:8-10 ↔
  structures.js:817/:848）。
- **creep 代理 14 getter 中 12 条逐字对上**（structures.js:791-812 ↔ 官
  tombstones.js:45-133）：id/name/spawning=false/my/body({type,hits:0})/
  owner（undefined·null→undefined）/ticksToLive/carryCapacity(ΣCARRY×50)/
  fatigue=0/hits=0/hitsMax(len×100)/saying。body 形态：官方 `_die.js:31`
  存 `_.map(body,b=>b.type)` 字符串数组 ↔ 投影 world_json.rs:522-524 同形。
  instanceof 面：官方 `new Creep()` 空壳真实例（creeps.js:59-63 no-id
  no-op）+ own getter 遮原型 ↔ 我方 plain object + setPrototypeOf
  （structures.js:810）同构。pos/room 缺失 = 评审 2 P2-1，官方面确认：
  官 tombstones.js:47-48 `RoomObject.call(creep,o.x,o.y,o.room)` 挂
  room/pos（rooms.js:1648-1655），我方代理两者皆无——P2 维持。
- **P3-A（本评审新发现：Tombstone 级 carry/carryCapacity 是官方没有的
  额外面）**：官方 Tombstone 原型字段表仅四条 deathTime/store/ticksToDecay/
  creep（tombstones.js:41-44）——`tombstone.carry` 官方 undefined；我方多挂
  两个 enumerable 原型 getter（structures.js:818-819），for-in/toJSON 枚举
  面随之加宽。bot 按 official 写 `tombstone.carry.energy` 官方炸、我方出值。
  建议删。
- **P3-B（本评审新发现：creep 代理 carry/store 与官方 quirk 反向）**：官方
  creep 代理的 carry/store 是 `get: _storeGetter` **无参调用**
  （tombstones.js:101-108）→ 走 `_storeGetter` 的 `!o` 回退（:8-13）→
  `{store:{}, storeCapacity: this.carryCapacity}`——即官方
  `tombstone.creep.carry/store` 恒为**空 Store**、`getCapacity()`=ΣCARRY×50、
  任何资源键 undefined（上游 quirk，非意图语义）。我方 `gw.makeStore(o)`
  （structures.js:802-803）给**真库存**且 `getCapacity()`=null——内容与
  容量双反。bot 生态实际读 `tombstone.store`，代理 carry 罕用，P3；对齐
  官方需改空 Store 回退形。
- **P3-C（=评审 2 P3-7，交叉确认）**：官方 `creep` getter 的 powerCreepId
  分支（tombstones.js:137-232，PowerCreep 代理 16 getter）我方未实装
  （structures.js:820 仅 creepId 臂）——世界模型无 powerCreep 恒不可达，
  登记。
- **structure 代理逐字段成立**（structures.js:826-842 ↔ 官 ruins.js:43-65）：
  user 有→OwnedStructure 形六 getter（id/hits/hitsMax/structureType/owner/my）、
  无→Structure 形四 getter；官方 `new OwnedStructure()/new Structure()` 空壳
  （官 structures.js:38-40/128-131 no-id no-op）↔ setPrototypeOf 同构；
  hits:0/hitsMax/user 双落（官 `_destroy.js:22-27` 两处同写 ↔
  world_json.rs:543-553/:563-565）对上。
- **Ruin.structureType 口径——报告主张成立，独立考据补充**：官方 getter 写
  `o => o.structureType`（ruins.js:39），但 DB ruin 对象**无顶层
  structureType 键**——唯一 ruin 工厂 `_destroy.js:20-35` 只写
  `structure.type`，driver data.js 原样透传无变换（:209 mapById 直装）→
  官方字面运行值 undefined；而官方 toString（ruins.js:69）经 structure 代理
  getter 读 `o.structure.type`（:50/:62）可用。生态裁量一致取"被毁结构
  类型"：xxscreeps ruin.ts:28 `get structureType() { return
  this['#structure'].type }`、screeps-ok vanilla snapshots.ts:391-397 读
  `obj.structure.type` 带显式注释。我方读存照 type 键（structures.js:833/
  :845）= 生态共识值、偏离官方字面 getter——P3 维持登记（报告引
  snapshots.ts:394 实为 :391/:397，行漂移可接受）。评审 2 P3-15 的
  toString 消费面错键是同一考据的代理层对偶（fmt 应读代理的
  structureType 而非 type）。

## 2. getEventLog（room.js:185-191 ↔ 官 rooms.js:573-583）

- raw truthy → 字符串、否则数组；缺日志 `'[]'`/`[]` 兜底 ✓。
- **勘误（对评审 3 P3-2 的官方面前提）**：官方非 raw **并非**"每次调用
  新鲜 parse"——`register.roomEventLogCache` 每 run 初始化（官 game.js:73），
  首调 `JSON.parse` 后按房名缓存、重复调用返回**同一数组实例**
  （rooms.js:577-581；空数组 `[]` 真值同样命中缓存）。我方返回 `r.events`
  同 tick 同引用——**与官方一致**，玩家 mutate 污染后续读取在官方同样
  发生。P3-2 建议从登记表撤下（不可达分歧）。
- 上一 tick 覆写等价：官方每房整体覆写（processor.js:497 → 官驱
  index.js:583-585 `hset(..., JSON.stringify(eventLog))`）↔ settle/mod.rs:509
  全量替换、次 tick 可见。
- 仅可见房等价：官方 `hmget(ROOM_EVENT_LOG, roomIds)`（官驱 data.js:141，
  roomIds=视野归集；:219 装载）↔ world_json.rs:59-75 visible 分支注入 ✓
  （observer/预定等更宽视野源为既有灰区口径）。
- **P3-D（raw 字符串字节面）**：serde_json 无 preserve_order → 事件对象键
  字母序（data,event,objectId）≠ 官方插入序（event,objectId,data）；仅
  raw=true 直存/比对处可观测，语义等价。`filter_map(...ok())` 静默丢与
  评审 3 P3-1 交叉确认（写源恒合法，不可达）。

## 3. Structure.destroy（structures.js:774-783 ↔ 官 structures.js:72-86）

- 三门全序逐字：!room→INVALID_TARGET（:73-75↔:775）；!controller||!my→
  NOT_OWNER（:77-79↔:776）；房内敌方 creep/powerCreep→BUSY（:81-84↔
  :777-779；官方 findCache `!i.spawning && !i.my`（game.js:18-20）↔ 我方
  find 103/121 臂既语义）✓。
- FIND 走冻结 C 表 ✓（structures.js:76-77；值 103/121 与官方
  common/lib/constants.js:28/:46 一致——判定门不随玩家改全局常量失明）。
- intent 载荷 `{roomName, id}` 逐键同形（:85↔:781）；room 级数组通道消费
  闭环（intent.rs:390-407 → parse_room_intent :353-357，B3 批已有）✓。
- 两层同判主张成立：引擎侧四门（官 destroy-structure.js:9-19）↔
  settle/room.rs:56-84；controller 非可建类型双方同 no-op
  （CONSTRUCTION_COST 无 controller 键）。
- 坑⑥清偿核实：STRUCTURE_METHODS=['setActive']（structures.js:636），全
  prelude 无残留 destroy 桩 ✓。structures.js:14 头部旧注释失实与
  structures.js:772-773 注释口径问题 = 评审 2 P3-11/P3-9，交叉确认。

## 4. upgradeController（creep.js:931-946 ↔ 官 creeps.js:922-964）

- **十门全序等价**：①!my（:925↔:932）②spawning（:928↔:933）③无活跃
  WORK（:931 `_hasActiveBodypart`↔countActiveParts——官尾部断扫 vs 全量
  计数，官方伤害模型死件恒后缀故可达态等价；for...of 面与评审 2 P3-14
  交叉确认）④shardAccess（:934-936）⑤!carry.energy（:937——官方 Store
  缺键 undefined/我方 Proxy 补 0，同 falsy 判）⑥目标无效/非
  StructureController（:940-943↔:936-939）⑦upgradeBlocked 相对值门
  （:944-946↔:940-946；我方 getter 投影绝对 tick−now 相对化
  structures.js:345-346 与官方 structures.js:183 同形，`x && x>0` 逐字）⑧
  !inRangeTo(3)（:947-949↔:947-949）⑨!target.my（:950-952↔:950-952——
  OwnedStructure my 三态两构，官 structures.js:137-139）⑩!level||!owner
  （:953-955↔:953-955）→ intents.set({id})（:958↔:958）✓。
- **shardAccess 跳过论证成立**：`runtimeData.user.shardAccess === false`
  是 hosted 静态控制字段，OSS 4.3.2 源内无置 false 路径——灰区登记恰当。
- **P3-E（=评审 2 P3-13 前半，交叉确认）**：⑥门缺官方
  `register.structures[target.id]` 成员检查（官方四合取 :940-943 实装三）。
  陈旧壳官方返 INVALID_TARGET、我方 getter 经 rawData 抛错；与 repo 既有
  controller 族 doctrine 一致（harvest/claim/attack 同形，creep.js:541-544
  注释），但 transfer 族已实装 structureRegisterEntry（creep.js:569-575）
  ——族内不一致，登记择期统一。
- **P3-F（=评审 2 P3-13 后半，交叉确认）**：拒绝分支缺
  `register.assertTargetObject(target)`（官方 creeps.js:941-942 在 return
  前调用——plain object+24 位 id 抛专用错，game.js:122-126）。transfer 族
  有（creep.js:737 等），controller 族（claim/attack/reserve/upgrade）
  A4-iii 起全缺——官方错误契约面（Memory 序列化对象反模式提示）家族性
  遗留，登记。

## 5. find 118-123 四臂（room.js:153-158 ↔ 官 game.js 装配族）

- 常量值 118/119/122/123 与官方一致（common/lib/constants.js:43-48 ↔
  assets/constants.json）✓。
- 官方四类装配（tombstone game.js:355-359 / ruin :360-364 / deposit
  :340-344 / powerCreep :286-296）在 findCacheFn 表（:17-29）**均无过滤键**
  → 全收录；我方 `o.type === '…'` 无过滤臂同构 ✓。
- Rust matches_find 同四臂同谓词（world_json.rs:709-712）——JS/Rust 两层
  同构 ✓。120/121 两层均缺（官方 :19-20 有过滤臂）= 评审 2 P3-16 交叉
  确认，无对象不可达，不另开条。

## 6. pushByName 计费（game.js:369-378 ↔ 官驱 runtime.js:60-99 + make.js:98-111/193-194）

- **双轨对齐成立**：沙箱显示面每元素 +0.2、无免费表豁免（官方
  runtime.js:89——免费表只在 set :66-70；我方 game.js:377 同）；宿主结算面
  count_billed（sandbox.rs:420-433：对象级 1/数组按长度/排除 say-pull）=
  intentsCount（make.js:100-109），0.2×count 并入 settle（sandbox.rs:392）↔
  make.js:193-194 ✓。destroy_room_intent_channel 的 intentsCpu=0.4 锁定
  结算面 ✓。
- **P3-G（maxLen 未实装）**：官方 pushByName 有 maxLen 参（runtime.js:85-87
  满返 false 不计费）；我方签名无（game.js:369）。现无调用方传参——
  market(deal,10)/powerCreep(50) 批接线时必须补，登记。
- **P3-H（微差注记）**：remove 门槛官方 truthy 检查（runtime.js:93）↔ 我方
  `!== undefined`（game.js:380）——intent 载荷恒真值对象/数组，差异
  不可达。

## 7. 灰区三条与报告主张对照

1. 坑⑥复发（桩覆盖序）——核实成立（§3 尾）。
2. pushByName 计费差——主张与官方一致（引 make.js:100-106 实为 98-111，
   行漂移可接受）。
3. shardAccess 门——不可达论证成立（§4）。

## 评审独立发现（P2×1；对评审 3 结论的一处实证修正）

- **P2-I（测试基建 flake——"workspace 全绿"不可复现，评审 3 的全绿结论是
  幸存样本）**：`destroy_room_intent_channel` 以独立 `#[test]`
  （sandbox_tests.rs:2590 起）加入该测试二进制，打破其"单 #[test]"不变量
  ——LIFO_REGISTRY 是进程级全局栈（sandbox.rs:121-126），Rust 默认并行
  测试线程下与 m0_sandbox_suite 的 create/drop 交错会假记 LIFO 违规。实测
  `cargo test -p kk-runner --test sandbox_tests` **5 跑 2 挂**（
  "全流程 LIFO 纪律零违规" left:1），两测试各自单跑恒绿；`cargo test
  --workspace` 首跑即挂、复跑绿——非确定性，合入即埋 flaky CI。修复：
  并入 m0_sandbox_suite 顺序调用（redirect_upgrade_positive_impl 同款
  模式，本批已有正面先例）或加串行约束。
- 报告"测试"节的玩家面声称与树内工件不符 = 评审 3 P3-3 交叉确认（其以
  临时测试独立复验行为为真）；语义风险已被该复验+ratchet +102 消化，维持
  P3 处置（固化用例），不另立 P2。

## 验证（评审 1 实跑）

- `cargo test --workspace`：除 P2-I 偶发 flake 外全绿（25 个 result: ok；
  sandbox 二进制并行时 ~40% 失败率复现，其余二进制全过）。
- 静态对照全部基于 6093f7b 工作区与官方 4.3.2 快照逐行核对（本节全部
  file:line 双侧锚定）。

## 结论（评审 1）

**需修复后合入**。语义主干全部成立——destroy 三门/pushByName 双轨计费/
find 四臂两层同构/getEventLog 时序与可见面/upgradeController 十门与官方
对照无发现返工项。必修二条：**P2-I**（flake 并入套件，一行级改动）与
评审 2 **P2-1**（creep 代理补 pos/room，defs 两行）。建议随批顺手：
P3-A/P3-B（Tombstone 字段表两处小改）。其余 P3（本节 C/D/E/F/G/H 及交叉
确认项）登记后随后续批次清偿。评审 3 的 P3-2 依据本节勘误撤下。

## 三路评审清偿记录

三路结论：评审 1 需修复后合入（1×P2 + 官方 quirk 对照）；评审 2 可合入
（1×P2 + 10×P3，探针实证丰富）；评审 3 可合入（0×P2/6×P3，+102 归因逐用例
确认 91 直接 + 11 间接）。全部清偿：

- **P2-I（评审 1，硬阻断）**：destroy_room_intent_channel 独立 #[test] 打破
  sandbox 单测试不变量——LIFO 全局栈并行假记违规（5 跑 2 挂 flake）→ 并入
  m0_sandbox_suite 顺序调用
- **P2-1（评审 2）**：tombstoneCreepProxy 补 pos（RoomPosition）/room
  （官方 tombstones.js:47-48 RoomObject.call 挂两者）
- **P3-A/B（评审 1）**：Tombstone 原型删多挂的 carry/carryCapacity（官方
  原型无此键）；creep 代理 carry/store 对齐官方上游 quirk（_storeGetter
  无参回退 → 恒空 Store + 容量=ΣCARRY×50——死 creep 不透传资源，墓碑才是
  资源面）
- **坑⑥同族（评审 2）**：notifyWhenAttacked 基类/spawn 双版各重复定义两份
  （逐字相同，后挂覆盖无行为差）→ 删重复份
- **Ruin toString（评审 2 探针实证）**：读 structure 代理的 structureType
  getter（原读 .type 键得 undefined——代理无该键）
- **陈旧注释**×2 清理；评审 1 对评审 3 的勘误采信（官方 getEventLog 的
  roomEventLogCache 每 run 缓存同引用——活引用行为与官方一致，非差异）
