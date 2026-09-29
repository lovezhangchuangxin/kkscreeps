# B4 开发报告：mineral/extractor + 多资源槽

> 轨 B 批 4（分支 `m2/track-b`，基于 main=e8445ae）。官方源码快照
> `~/Desktop/projects/sources/screeps/`（4.3.2），一切行为主张 file:line 取证。
> 灰区表回写归主会话——本文"灰区登记"节为提案清单。
> 范围：结算侧（schema/world/engine/settle_tests）。prelude API 面扩展
> （Mineral/StructureExtractor 类换真）归轨 A，本批未触碰 prelude 任何文件。

## 一、实现清单（官方 file:line 对照）

### 1. kk-schema（v1 内演化——只加不减，冻结纪律同 B3 约定）

| 变更 | 官方出处 |
|---|---|
| `MineralObj` 变体（tag 14）：mineral_type/mineral_amount/density/next_regeneration_time(opt) | minerals/tick.js 读写的持久化面；getter 面 minerals.js:35-40 |
| `ExtractorObj` 变体（tag 15）：hits/owner(opt)/cooldown(u64，0≙官方 undefined)/notify_when_attacked(opt) | build.js:214-220（user/hits/hitsMax）+ extractors/tick.js 计数器 |
| `CreepObj.store_extra: vec ResourceStack`（尾部追加） | 官方 creep store 是按资源名键开放对象（harvest.js:95 `store:{[target.mineralType]}`） |
| `StorageObj.store_extra: vec ResourceStack` | storage 通用商店（capacityForResource 走 `max(0, storeCapacity−Σscr)`，utils.js:671-675）任意资源可入 |
| tags 14/15 追加尾部 + sample_snapshot/冻结测试 | schema room.rs tags 模块约定 |

### 2. kk-world

| 变更 | 官方出处 |
|---|---|
| place.rs：structure 通道 `mineral` → typed（mineralType 必填、amount 缺省 MINERAL_DENSITY[density]、density 缺省 DENSITY_HIGH(3)、ticksToRegeneration 挂绝对钟——只判 !== undefined，无 source 的 >0 过滤） | vanilla placeMineral（adapters/vanilla/index.ts:1209-1221） |
| place.rs：structure 通道 `extractor` → typed（hits 缺省 EXTRACTOR_HITS、cooldown 透传、owner 可缺省） | vanilla placeStructure 的 extractor cooldown 计数器形态（index.ts:1147-1151） |
| world_json.rs：mineral 投影 `type:"structure"+structureType:"mineral"+mineralType/mineralAmount/density/ticksToRegeneration(折叠，None→0)` | 与 source 同款折叠口径（prelude structures.js:300-305 的消费面 + game.js:329-345 独立装配分支） |
| world_json.rs：extractor 投影（hits/hitsMax=EXTRACTOR_HITS/cooldown 计数器原样/owner 仅 Some 落键/notifyWhenAttacked opt） | structures.js:269 `cooldown: o.cooldown \|\| 0`；build.js:214-220 |
| world_json.rs：creep/storage 的 store 投影改 store_with_extra（命名矿物正量并入） | 官方 store getter 按资源名键正量暴露 |
| world_json.rs：object_owner 补 Extractor 臂（有主授房视野）；mineral 无 user 不授 | data.js:105-116 userObjects 归集 |
| snapshot.rs：Mineral 契约七键（kind/id/pos/mineralType/mineralAmount/density/ticksToRegeneration）+ Extractor 契约（base+hits+cooldown，owner 无主读 null）；creep/storage 快照 store 全资源正量 | vanilla snapshots.ts:365-373（snapshotMineral）+ snapshotStructure extractor 分支（...base/...hits + `cooldown: obj.cooldown \|\| 0`） |
| snapshot.rs：Generic 透传 kind 映射保留 mineral/extractor（旧持久化字节与手写透传的遗留路径——放置面已 typed） | B3 同款处理（tombstone/ruin 先例） |
| types.rs：`store_total`/`store_extra_slot`（calcResources 等价物——全槽求和/命名槽读取）、`resources_weight` 携带量并入 extras、`mineral_density` 表查询 | movement.js:41-59 的 `_.sum(creep.store)`；constants.js:310-315 |

### 3. kk-engine（结算）

| 变更 | 官方出处 |
|---|---|
| `mineral_tick`（economy.rs）：`!mineralAmount` 门内挂 50000 tick 钟 → 到点（`time >= next-1`）按**旧** density 查表回填 + 清钟；LOW/ULTRA 强制轮转、其余 5%；do-while 拒同值；概率映射 r≤0.1→1/≤0.5→2/≤0.9→3/else→4 | minerals/tick.js:6-37（MINERAL_DENSITY_PROBABILITY 的 for-in 升序键序） |
| 随机源：官方 Math.random 真随机不可复现（灰区表既有口径）；replay 确定性用无状态 SplitMix64——hash(房名,对象id,tick) 播种，同世界状态必得同序列 | 本批决策（报告灰区 1） |
| `extractor_tick`（economy.rs）：cooldown>0 递减（钳 0）→ 瞬态 `_cooldown` 同 tick 持久化覆盖（先减后盖，官方同序） | extractors/tick.js:7-25 |
| Working 瞬态 `extractor_cooldown_pending: BTreeMap<ObjId,u64>`（官方 `extractor._cooldown` 的两文件通道，与 upgrade_block_pending 同族） | harvest.js:108 写 / extractors/tick.js:21-25 读 |
| `intent_harvest` 重构为官方三分派：公共预检（spawning/目标存在/Chebyshev 邻接先于类型分支）→ source/mineral 两分支执行器 | harvest.js:10-23 |
| `intent_harvest_mineral`：矿量门→同格 extractor（index.at ≡ `_.find` 坐标语义）→敌主拒/无主放行→checkStructureAgainstController（RCL 门）→cooldown 拒→HARVEST_MINERAL_POWER×WORK→扣矿→产出入矿种命名槽→超容按矿种 drop→actionLog→`_cooldown=5`→事件 | harvest.js:67-112 全链 |
| 事件 amount 是**未钳制**的 harvestAmount（mineral 分支 :110）vs source 分支 :63 钳后 amount——官方 quirk 原样保留并锁值 | harvest.js:110 vs :63 |
| `extractor_active`（checkStructureAgainstController 的 extractor 面）：无主恒 active；有主须 controller level≥1 同主且配额>0（quota[8]===1 的"唯一配额即本尊"提前返回——距离计数分支对该类型不可达 ⇒ RCL≥6 才 active） | utils.js:456-495（:457-460/:463-465/:467-476） |
| 多资源槽消费面：transfer/withdraw/drop 白名单 energy/G → **RESOURCES_ALL**（与官方 transfer.js:8/withdraw.js:8/drop.js:10 逐字一致——power 名义在表内但无进 creep 的建模路径，见灰区 3）；creep/storage 槽读写经 slots_add/slots_sub（命名键留 0 不删——官方 `store[res]=0` 落库同构）；容量口径：creep/storage 全槽求和、spawn/extension per-resource（非 energy 得 0 拒） | transfer.js:8-45、withdraw.js:8-25、drop.js:10-26、utils.js:671-675 |
| drop 落地路径 `create_resource`（任意资源名——命名堆 + container 承接） | drop.js:24 的 `_create-energy(amount, resourceType)` |
| pickup：命名矿物堆放行入槽（B3"矿物堆拒拾"登记随本批清偿）；power 堆维持拒绝 | pickup.js:23-30；B3 灰区登记的清偿条款"随矿物槽扩展放开" |
| die_creep 第二循环补 store_extra（携带矿物入墓碑命名槽） | _die.js:78-91 逐键循环 |
| `complete_construction` 补 extractor 臂（user/hits=EXTRACTOR_HITS/notify=true/cooldown=0） | build.js:111-117/:214-220 |
| phase 8 分派补 Mineral/Extractor 两臂 | processor.js:356-357/:380-381 |
| movement.rs：type_name_of 补两臂；resources_weight 携带量含 extras（重量/疲劳可观察） | movement.js:41-59 |
| damage.rs：object_hits/structure_hits_max/set_structure_hits 补 Extractor 臂（mineral 无 hits——攻击门天然拒）；destroy_structure 的废墟承接带 storage store_extra | build.js:217-218；_destroy.js:37 `store \|\| {}` 开放对象 |
| notify_when_attacked 通用段 + object_owner_of 补 Extractor 臂 | processor.js:313-314；CONSTRUCTION_COST 含 extractor(5) |

## 二、测试清单

**settle_tests 130 → 137（+7 新增，1 个 B3 锁值用例按 B4 语义改写）**

新增：
1. `mineral_regeneration_clock_and_density_rotation`——耗尽首见挂钟（time+50000）、差一 tick 不回填、到点按旧 density 回填（35000/15000）+ 清钟、LOW 强制轮转必变、**replay 确定性**（同世界状态两次运行同结果——官方 Math.random 不可复现口径的确定性替身锁值）。
2. `harvest_mineral_named_slot_and_cooldown_cycle`——2 WORK×HARVEST_MINERAL_POWER(1)=2 入 U 命名槽、矿量扣减、事件 amount=2、extractor cooldown 同 tick 持久化 5、T1-T5 冷却拒采递减 5→0、T6 再采（六 tick 周期）、**未钳制事件 quirk**（矿量 1/功率 2：实采 1、事件 2）。
3. `harvest_mineral_gates`——门矩阵：无 extractor/敌主/RCL 5（配额 0）/矿量耗尽 拒；**无主 extractor + 无 controller 放行**（utils.js:457-460 无主恒 active）。
4. `harvest_mineral_overflow_drops_named_pile`——满 energy 双 WORK 采 2 U：sum 52>50 → drop 2 U 成命名堆（当 tick 衰减 1 的既有锁值口径）、creep U 键留 0、energy 不动。
5. `mineral_multislot_transfer_storage_and_drop_chain`——U transfer 入 storage 命名槽 → transfer 到 spawn 拒（per-resource 容量）→ withdraw 取回 → drop 落命名堆；转出后键留 0（官方 store[U]=0 落库同构）。
6. `movement_weight_counts_named_minerals`——[Carry,Carry,Move] 携 energy 50+U 60=110 → weight 2：净 +2 疲劳卡住（canMove 需 fatigue==0）、空转恢复归 0；对照组无 U（weight 1）连续移动净 0。
7. `creep_death_carries_named_minerals_to_tombstone`——suicide 后墓碑承接部件回收（125）+ 携带 energy 20 + XUHO2 5（store_extra）。

改写：`pickup_rejects_named_mineral_pile` → `pickup_mineral_pile_into_named_slot_and_power_still_rejected`（B3 锁"拒拾"随矿物槽清偿：XUHO2 堆整堆拾入命名槽、power 堆维持拒绝——B3 登记口径不变）。

**其他**：kk-schema 7→8（`b4_variant_tags_and_multislot_roundtrip`——tag 14/15 冻结 + 变体往返 + sample_snapshot 补 mineral/extractor/creep extra）；kk-world 37→40（`mineral_and_extractor_placement_defaults`、`mineral_and_extractor_projection_and_find`、`extractor_snapshot_and_creep_named_slot`；`snapshot_generic_kind_mapping` 改写为 `snapshot_mineral_and_tombstone_kind_mapping`——typed 化后的七键契约 + FIND_STRUCTURES 不含 mineral）。

门禁：`cargo test --workspace` 29 个测试目标全绿；`cargo clippy --workspace --all-targets` 零警告；`cargo fmt --all` 过；embed 冒烟通过（createShard rooms+players → placeObject type:"structure" mineral/extractor → tick → findInRoom 116 契约七键 → runPlayer：`Game.rooms.W1N1.find(FIND_MINERALS)[0]` 得 Mineral 实例（mineralType=U/amount=5000/density=3）、FIND_STRUCTURES 含 extractor、`creep.harvest(mineral)` 返回 OK、下一 run 读 `store.getUsedCapacity('U')`==2——**玩家 API → 结算 → Store 读数全链贯通**）。

## 三、灰区登记提案（回写归主会话）

1. **mineral 密度轮转的随机源**——官方 Math.random()（真随机、不可复现；灰区表"非确定性源"既有口径）。本批用无状态 SplitMix64（hash(房名,对象id,tick) 播种）保证 replay 确定性；与官方的分布差异只在低 53 位均匀性（概率映射阈值同表），玩家不可区分"是否真随机"。若未来官方 replay 需逐值对齐，此处换种子源即可（单点：MineralRng::new）。
2. **phase 5 掉落在出生 tick 衰减（既有，非本批引入）**——官方 `bulk.insert` 不进内存 roomObjects，phase 8 的 forEach 不见 intent 期新掉落堆 ⇒ 官方出生 tick 不衰减；我方 phase 8 ids 快照在 phase 5 后收集 ⇒ 出生 tick 衰减 ceil(amount/1000)（amount=1 的堆直接蒸发）。既有锁值测试（harvest_overflow_drops"同 tick 衰减 1"）已把该差异钉为预期。本批新测试沿用该口径。**与官方有 1/tick·堆 的可观察差**——清偿方向：phase 8 ids 快照改为与官方 lodash 键快照同语义（settle 开始时收集）。本批未动（会翻转既有锁值，独立小批处理）。
3. **transfer/withdraw/drop 白名单含 power 名义**——本批执行器白名单与官方逐字一致（RESOURCES_ALL 含 "power"）；power 无任何进 creep store 的建模路径（pickup 拒 power 堆——B3 登记、harvest 不产、结构 store 无 power 槽），故放行面不可达。唯一的 pickup power 拒绝是既有登记（随 power 批放开）。
4. **mineral 分支无 source 的 controller 归属/预定门**——官方 harvest.js:31-34 的门只在 source 分支；mineral 分支只查 extractor 自身归属与 RCL。敌主房里采自有 extractor 不被房主身份拦（extractor 归属门已足够）。非分歧、防误抄登记。
5. **mineral 再生事件的 amount quirk**——harvest mineral 事件 amount=harvestAmount（未钳制），矿量余 1/功率 2 时事件报 2、实采 1（source 分支报钳后值）。官方 quirk 已复刻并锁值（见测试 2）。
6. **checkStructureAgainstController 的距离计数分支**——只对 quota[8]>1 的类型可达（spawn/tower 等）；extractor quota[8]===1 走"唯一配额即本尊"提前返回。本批只实现 extractor 面；多配额类型的 isActive 面随对应结构批。
7. **extractor cooldown 的 API 读数（轨 A 缺口）**——prelude StructureExtractor 仍是 M0 桩类（无 cooldown getter）：冷却期玩家读 undefined（应读 5..1）→ harvest 预检 `extractor.cooldown` 恒 falsy → API 返回 OK 但结算拒绝（应 ERR_TIRED）。行为差只在返回码，世界终态一致（结算侧真值门）。投影侧 cooldown 键已就位，轨 A 接线 getter 即消。

## 四、轨 A 待接清单

1. `StructureExtractor` 类换真：cooldown getter（`o.cooldown || 0`，structures.js:269）——投影键已就位（world_json 的 extractor 投影带 cooldown 计数器）。
2. `Mineral` 类防御分支收口：structures.js:300-305 的 fallback（`o.mineralType || 'XUH2O'` 等）在 typed 投影下恒有真值，可撤防御值。
3. `deposit` 分支（harvest.js:114-139 与 FIND_DEPOSITS 玩家面）——schema 无 deposit 变体（仍 Generic 透传），随对应批。
4. effects 面（PWR_REGEN_MINERAL 的 minerals/tick.js:39-47）随 power 批（M2 无 effects 面）。

## 五、未尽事项与轨 B 后续

1. withdraw/pickup 以 tombstone/ruin 为目标（B3"轨 B 后续"第 1 条）——仍不在（本批只做了资源维度扩展，目标白名单维持 spawn/extension/storage）。
2. road 衰减 tick（B3"轨 B 后续"第 2 条）——未动。
3. lab/terminal/factory 等其余多资源结构（lab 的 storeCapacityResource per-resource 容量族）——lab 随 2B 压缩批，terminal/factory 随对应批。
4. `withdraw` 的 safeMode/rampart 门（官方 withdraw.js:25-31）——B1 起既有缺口（本批只扩资源维度），随 rampart 批。
5. Game.resources 形态、market/像素面——M2B 用户资源面（既有登记）。

## 六、风险自评

- **store_extra 是 v1 内演化**（Creep/Storage 尾部追加）：尚无落盘字节在流通（既有登记口径）；首个持久化格式发布起 bump 纪律生效。
- **slots 键留 0 不删**：官方 store[res]=0 落库同构；消费面（投影/快照/calcResources 族）全走正量/求和口径，0 键无行为面。若未来 schema 换正式开放 store 形态时一并收口。
- **phase 8 ids 快照语义**（灰区 2）是本批测试口径的依赖——若清偿该差异，本批 4/5 号测试的"当 tick 衰减 1"断言需随转（转红即提示）。
- 棘轮影响：mineral/extractor 旧放置路径（Generic 透传）→ typed 后 FIND/快照形状变化——快照契约七键比透传更窄（透传曾带 structureType 键），screeps-ok 契约面以七键为准（vanilla snapshotMineral 同构），预期棘轮不转红（本批 CI 全绿；全量棘轮回归归主会话门禁）。
