# B3 开发报告：tombstone/ruin 变体 + 死亡事件 + TTL 死亡产墓碑

> 轨 B 批 3（分支 `m2/track-b`，基于 main=74c16d5）。官方源码快照
> `~/Desktop/projects/sources/screeps/`（4.3.2），一切行为主张 file:line 取证。
> 灰区表回写归主会话——本文"灰区登记"节为提案清单。

## 一、实现清单（官方 file:line 对照）

### 1. kk-schema（v1 内演化——只加不减，灰区表既有约定）

| 变更 | 官方出处 |
|---|---|
| `TombstoneObj` 变体（tag 12）：death_time/decay_time/creep_id/creep_name/creep_ticks_to_live/creep_body(部件码)/creep_saying/owner/store+store_extra | _die.js:22-34 的墓碑字段面 |
| `RuinObj` 变体（tag 13）：structure_id/structure_type/structure_hits_max/destroy_time/decay_time/owner/store+store_extra | structures/_destroy.js:22-40 |
| `ResourceStack {resource, amount}`：墓碑/废墟 store 的额外资源槽（boost 矿物等） | 官方 store 是按资源名键的开放对象（_die.js:35/:44-53/:78） |
| `EnergyObj.resource: opt str` 命名资源槽（Some 时 code 无效） | _create-energy.js:31-42 的 resourceType 字符串形态 |
| `RoomSnapshot.events: vec str` 死亡事件缓冲 | saveRoomEventLog 每 tick 整体覆写（processor.js:497 → driver/lib/index.js:583-585） |
| tags 12/13 追加尾部（冻结纪律：新增变体追加、不动已有值） | schema room.rs tags 模块注释约定 |

### 2. kk-world

| 变更 | 官方出处 |
|---|---|
| place.rs：`tombstone`/`ruin` kind 从 Generic 透传切 typed 变体；缺省面 deathTime/destroyTime ?? gameTime、ticksToDecay ?? 500、creepId ?? null、store 额外资源进列表 | vanilla placeTombstone（adapters/vanilla/index.ts:1263-1280）/ placeRuin（:1286-1313） |
| place.rs：掉落资源白名单放开到 RESOURCES_ALL（原仅 energy/power/G） | _create-energy 的任意 resourceType 落地 |
| world_json.rs：tombstone/ruin 世界投影（官方 DB 形态 + ticksToDecay getter 折叠 + creepId/structure.id 字符串化 + user 双落/缺省不落） | tombstones.js:26-46 / _destroy.js:23-38 |
| world_json.rs：tombstone/ruin 的 user 进 object_owner（视野归集） | driver/lib/runtime/data.js:105-116（只跳过 flag/constructionSite） |
| snapshot.rs：tombstone/ruin 查询契约七键面（kind/id/pos/creepName|structureType/deathTime|destroyTime/store/ticksToDecay），ticksToDecay 裸减法、store 全资源正量 | vanilla snapshots.ts:405-414（snapshotTombstone）/:392-402（snapshotRuin）/:36-42（snapStore） |
| world.rs：RoomState.events 缓冲字段 + 快照编解码往返 | 同 schema events 缓冲 |

### 3. kk-engine（结算）

| 变更 | 官方出处 |
|---|---|
| `die_creep` 全量重写（creep.rs）：移除本体 → 造价回收 + 携带 store 双循环（同格 container 先承接）→ 墓碑落库 → EVENT_OBJECT_DESTROYED {type:'creep'} | _die.js:6-101 |
| — decayTime = body.len() × TOMBSTONE_DECAY_PER_PART(5) | _die.js:16-17 |
| — _ticksToLive = ageTime − gameTime | processor.js:50（tick 首拍维护） |
| — lifeRate = dropRate × ticksToLive / lifeTime（lifeTime 按 body 含 CLAIM → 600） | _die.js:40-42（_.any 不看 hits） |
| — 造价回收：每件能量 min(CREEP_PART_MAX_ENERGY=125, cost×lifeRate)；boosted 件加 LAB_BOOST_MINERAL(30)×lifeRate 矿物 + LAB_BOOST_ENERGY(20)×lifeRate 能量；**资源总额 floor 后逐键入账** | _die.js:43-53/:61-76 |
| — 两段 container 承接（body 回收与携带各过一次门，容量 = CONTAINER_CAPACITY − calcResources） | _die.js:36/:45-59/:79-90 |
| — dropRate 参数化：TTL/暴亡/suicide 走 0.2 缺省，**recycleCreep 走 1.0** | tick.js:93-95/:134-136、suicide.js:14、recycle-creep.js:23 |
| `destroy_structure` 全量重写（damage.rs）：雏形移除 → 废墟（结构存照 {id,hits:0,hitsMax,user} + store 全量承接）→ 移除本体 | structures/_destroy.js:6-48 |
| — 衰减钟 RUIN_DECAY_STRUCTURES[type](powerBank 10) \|\| RUIN_DECAY(500)；user 仅真值落（空串 falsy） | _destroy.js:31/:37-38 |
| 击毁事件：apply_damage 在 destroy 后 push EVENT_OBJECT_DESTROYED {type: 官方对象 type}，先于反击/主攻击事件 | _damage.js:46-53/:89/:93 |
| `tombstone_tick`/`ruin_tick`（economy.rs）：`!decayTime \|\| gameTime >= decayTime-1` 到点 → 全 store 逐资源落地 → 移除 | tombstones/tick.js:5-16 / ruins/tick.js:5-16 |
| `create_resource`（mod.rs）：_create-energy 全量——container 先承接（含 store 缺键现场建）+ 同格同资源并堆；create_energy_typed 收敛为其 code 别名（**drop/衰减/踩工地退费等全部掉落路径现在官方同式过 container**） | _create-energy.js:6-42 |
| `DestroyStructure` room intent（intent.rs 白名单 + room.rs 结算）：可建类型 + controller 归本人 + 房内无敌方 creep → _destroy；system.js:20 白名单 `{roomName:'string', id:'string'}` | room/destroy-structure.js:6-24 |
| settle_room 尾部落 `room.events`（每 tick 覆写）；phase 8 分派补 Tombstone/Ruin 两臂 | processor.js:379-382/:497 |
| type_name_of/object_owner_of/notify 门补 tombstone/ruin 臂（CONSTRUCTION_COST 表外——攻击/修理/建造门天然拒） | 官方 type 名 'tombstone'/'ruin' |

## 二、测试清单

**settle_tests 119 → 125（+6 新增，7 个 B1 时代用例改写为官方口径）**

新增：
1. `tombstone_body_recovery_and_boost_minerals`——boost 部件回收锁值：XUHO2=30×0.2=6、energy=20×0.2+min(125,100×0.2)+min(125,50×0.2)=34、携带并槽 44、ticksToLive/decayTime。
2. `tombstone_container_deposit_first`——container 余量竞争：1900/2000 起点，body 回收 125 吃 100、携带 150 全进墓碑 ⇒ 175。
3. `tombstone_decay_dumps_store_per_resource`——到点三资源（energy/G/XUHO2）各成一堆（同格异资源不并堆）+ 移除 + 当 tick 新掉落不衰减。
4. `ruin_decay_dumps_store_and_removes`——同形（energy+LH）。
5. `destroy_structure_intent_paths`——自家可毁（废墟+store 承接+事件）/ 敌 creep 在房拒 / 非 controller 主人拒 / source 无 cost 拒。
6. `room_events_buffer_overwritten_per_tick`——事件序列化落账 + 无事件 tick 覆写为空。

改写（M1 灰区"资源掉地/无事件"口径 → B3 官方墓碑/废墟口径）：`recycle_creep_settles`（dropRate 1.0 的 175=50+125）、`creep_dies_at_age_time`（墓碑 30 + 事件）、`attack_kills_creep_into_tombstone`（原 drops_store；290=40+250、事件序 ATTACK→DESTROYED）、`attack_kills_structure_and_spawn_chick`（废墟存照+251 承接）、`dismantle_reflux_and_destroy`（事件序 DESTROYED→ATTACK + 废墟）、`dismantle_redirects_to_rampart`（rampart 废墟 + 事件序）、`suicide_intent_kills_creep_same_tick`（290 + 事件销账）。

**其他**：kk-schema 6→7（`b3_variant_tags_and_named_resources_roundtrip`——tag 12/13 冻结 + 命名槽/ResourceStack 往返；sample_snapshot 补 tombstone/ruin/events 进 roundtrip）；kk-world 31→34（`tombstone_and_ruin_placement_defaults`、`tombstone_and_ruin_world_projection_and_vision`、`tombstone_and_ruin_snapshot_contract`；`unknown_types_go_generic_passthrough` 的 tombstone 示例换 deposit）；embed_tests 的 real_settlement/spawn_family/move_misc 三段死亡断言改墓碑口径（含 ticksToDecay=死亡 tick 折叠、契约七键面）。

门禁：`cargo test --workspace` 26 个测试目标全绿；`cargo clippy --workspace --all-targets` 零警告；`cargo fmt --all` 过；embed 二进制冒烟（createShard/placeObject tombstone/findInRoom FIND_TOMBSTONES 契约形状回读）通过。

## 三、灰区登记提案（回写归主会话）

1. **destroyStructure 的事件：OSS 引擎与套件分歧（本批按套件）**——OSS 4.3.2 的 room/destroy-structure.js 与 structures/_destroy.js 均无 eventLog.push（OBJECT_DESTROYED 只在 _damage.js:51 与 _die.js:97 两处）；vanilla 与 screeps-ok ROOM-EVENTLOG-027 期望 `Structure.destroy()` 产事件。我方在 room.rs 结算侧补 push（objectId=被毁结构、data.type=structureType）。建议灰区表登记"以套件为准"的裁决。
2. **eventLog 保存窗口：单 tick 覆写**——OSS saveRoomEventLog 每 tick hset 整体覆写（driver/lib/index.js:583-585）；本地官方文档镜像（screeps-cn Room.html）表述为"前一个 tick 的事件"与覆写一致。评审 1 勘误：原"官方文档 100 tick 窗口"主张未取证成立（ROOM_EVENT_LOG_LENGTH 常量在 4.3.2 快照不存在），已删。我方 RoomState.events 按单 tick 覆写（同 OSS/套件/文档镜像三方一致）。
3. **墓碑的 creepSaying 恒无**——官方 creepSaying 取死亡 tick actionLog.say 的公开遗言（_die.js:33）；我方 u16 位图 action_log 无文本载荷（既有 say 登记的延伸）——tombstone.creep.saying 玩家面读 undefined。
4. **ruin 的 hitsMax None 边缘**——无 controller 房的 Generic rampart 击毁时 hitsMax 计算得 None（官方 ramparts/tick.js:17 归一后恒有数值）→ 投影 undefined。放置面 structureHitsMax 可显式给值。
5. **_create-energy 容器承接的行为变化（清偿）**——M1 的掉落路径无 container 分支；B3 补齐后 drop/墓碑废墟衰减/踩工地退费等全部掉落官方同式先进同格 container（hits>0，容量 2000−全槽）。灰区表 49 行"其余矿物仍拒"同步清偿（place 掉落白名单放开到 RESOURCES_ALL）。
6. **NPC/宿主体系不建模面**——tombstoneDecay 覆盖键（_die.js:18，OSS 全库无写入点）、userSummoned/strongholdId 回收跳过门（:38）、Source Keeper 的 dropRate=0（suicide.js:14 user=='2'）、stats.creepsLost（:99-101）。核弹抑制分支（_die.js:21/_destroy.js:21 的 attackType==NUKE 不产墓碑/废墟）随核弹批。

## 四、未尽事项与轨 A 待接

**轨 A（玩家 API 面）**：
1. `Tombstone`/`Ruin` 全局类（tombstones.js:26-46/ruins.js:33-63 的 getter 族：deathTime/ticksToDecay/store/creep 代理对象（hits=0、body 全灭、ticksToLive=creepTicksToLive、carryCapacity=ΣCARRY×50）/structure 代理）——_registry.js 已有 'tombstone'→'Tombstone' 映射，类本体缺。
2. `room.getEventLog()/getEventLog(true)`——消费 RoomState.events（需 world_json/beginTick 通道把事件注入 runtimeData.roomEventLog 等价物）；B2 登记的"EVENT 封套空 data"（attackController）在该接线时一并消化。
3. 世界投影的 tombstone/ruin 已透出（DB 形态 + ticksToDecay 折叠）——prelude 薄壳 getter 化时按官方类面对照。
4. FIND_TOMBSTONES/FIND_RUINS 两层（查询面 matches_find + prelude room.find）已通——prelude 类点亮后即完整。
5. `Structure.destroy()` prelude 仍是桩（structures.js）——引擎侧 DestroyStructure room intent 已实装（评审 1 F3 补列）；套件 ROOM-EVENTLOG-027 经该 API 走通，接线时点亮。

**轨 B 后续/其他**：
1. withdraw/pickup 以 tombstone/ruin 为目标（官方 withdraw.js 对有 store 的对象放行；墓碑/废墟的资源回收 API）——本批未接（结算侧 intent_withdraw 目标白名单仍 spawn/extension/storage）。
2. road 衰减 tick 未实装（既有登记——字段与磨损已有，decay 到点移除缺）。
3. effects/collapse（power 体系）随 M4。

## 五、风险自评

- **事件序锁定**：OBJECT_DESTROYED 先于 EVENT_ATTACK（_damage.js:51 vs :93）已按官方序落账并锁值——embed/棘轮侧若有依赖旧事件序的用例会显形（本批全绿）。
- **store_extra 与 EnergyObj.resource 是 v1 内演化**：尚无落盘字节在流通（既有登记口径）；首个持久化格式发布起 bump 纪律生效。
- **掉落路径过 container 的行为变化**：放置面造的 container（Generic）现在会吃掉落——screeps-ok 用例如预设掉落堆旁有 container 可能显形（本批 CI 全绿，棘轮全量回归归主会话合入门禁）。

## 六、三路评审清偿记录

三路结论：评审 1（官方语义）可合入 1×P2+4×P3；评审 2（Rust）需修复后合入
1×P2+3×P3（pickup 矿物洗白 embed 实证）；评审 3（测试/契约）需修复后合入
1×P1+2×P2+7×P3。pickup 命名矿物堆被评审 2/3 独立交叉确认。全部清偿：

- **P1/P2-1 pickup 命名矿物堆洗白**（两路实证）：slot_resource 改走
  energy_resource_name 单源（code 与命名槽双态折算后仅 energy/G 放行）；
  power 堆同根覆盖（create_energy_typed(1) 新形态 (0,Some("power"))）；
  锁值 pickup_rejects_named_mineral_pile（含当 tick 衰减 −1 口径）
- **P2 废墟投影 structure 内键名 structureType→type**（评审 1 F1/评审 3-10
  交叉确认，官方四源：_destroy.js:29/ruins.js 代理/vanilla placeRuin+
  snapshots.ts:394/xxscreeps schema）+ 测试断言与 prelude Ruin toString 同步
- **P2 测试缺口两条**（评审 3-2/3-3）：tombstone_container_cross_resource_
  competition（energy 先吃余量、矿物全进墓碑的键序可观察面）+ ruin_inherits_
  mixed_store_and_dumps_on_decay（Generic 混合 store 击毁→store_extra→衰减落地
  全链闭环）
- **P3 顺手清偿**：deposit_into_container 非整数槽按 0 折算（原 filter_map
  跳过使 headroom 高估+taken 不入槽资源灭失）；place.rs 两处衰减钟裸加法改
  saturating_add；CLAIM 600 分母锁值（claim_body_life_time_denominator，
  130 vs 错分母 52 可分辨）；!decayTime falsy 立即到点锁值；事件缓冲序断言
  （buffered DESTROYED 先于 ATTACK）；两处陈旧注释更新
- **P3 登记项**（灰区表）：room intent 提交序 vs 官方分型批次序
  （removeFlag→createFlag→createSite→removeSite→destroyStructure）；power 掉落
  双表示（place (1,None) vs 引擎 (0,Some)——energy_resource_name 折算后玩家
  面无差）；world_json store energy:0 恒投影扩展至 tombstone/ruin（M0 契约
  外延，空墓碑我方 energy:0 vs 官方 {}，快照正量口径一致）；placeRuin 缺省
  面（hitsMax None 缺键 vs vanilla 0、structureId 解析失败归零）；Generic
  透传臂的 tombstone/ruin 无 ticksToDecay 折叠（遗留路径：旧持久化/
  structureType 通道）
