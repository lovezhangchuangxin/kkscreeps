# 游戏循环

一个 tick 是世界结算的一轮，不是固定的墙钟时长。当前嵌入式引擎由调用方显式请求推进，没有运行完整服务器的定时调度器。本页区分已实现的流程和并行流水线目标，详细约束见 [tick 契约](tick-contract.md)。

## 当前执行流程

真实玩家执行由 `kk-runtime::EmbedEngine::run_players` 协调：

1. 固定本轮的已提交世界版本，逐名玩家准备视野过滤后的 JSON 世界、玩家 Memory 和宿主 CPU 预算。
2. 有界派发玩家任务，完成一个后补入下一个，收集指令、表达式结果、RawMemory、segments、计费候选和日志。每名玩家有独立线程和 V8 实例；回执可乱序到达，结果按玩家身份保存。默认上限为可用处理器数与 2 的较小值，可在轮次之间通过 `set_player_concurrency` 配置为 1..=32。
3. 按注册玩家稳定顺序合并指令，只取各回执中的注册玩家键（覆盖对象、房间、通知、市场及扩展意图），采样 `TickInput`，根据固定用量计算 CPU 桶候选并进入世界结算。引擎依次经过 prepare、settle、market、validate、publish：准备工作状态、串行处理房间、处理市场、验证候选、发布内存状态。
4. 将 CPU 桶、RawMemory、segments 写回合并到玩家候选，随世界一起发布，再返回玩家运行结果。

调用 `run_player` 同样推进一轮，只请求一名玩家；调用 `EmbedEngine::tick(count)` 则不执行玩家代码，仅逐轮结算世界。后者途中失败时，已经成功的 tick 不会撤销。

当前入口不会让房间结算与未完成玩家执行重叠；本轮请求执行的玩家看到同一轮已提交世界，不能因为某玩家先完成就推进其观察到的时间。

正常 CPU 用量与 `Game.cpu.getUsed()` 使用执行线程的 CPU 时钟，不计排队、睡眠或其他线程的用量；指令附加费用保持原规则。墙钟仍限制沙箱活性，硬中断仍按完整预算计费。每条 RunJob 在派发时绑定绝对期限，覆盖排队、内存注入、执行与结果导出；超期结果作为基础设施故障处理。

## 指令如何生效

玩家 API 在 JavaScript 中检查参数、所有权、距离、资源和对象类型等条件，接受动作后收集指令。API 返回 `OK` 表示指令已接受，最终能否生效仍取决于结算时的状态与冲突规则。

同一对象动作可覆盖此前收集的动作；某些指令通过数组累计，不能把所有指令都概括为“只保留最后一条”。指令收集和附加 CPU 费用的具体规则见 [game.js](../../crates/kk-runner/src/prelude/game/game.js) 的 `resetIntents`。

宿主通过 JSON 导出结果，Rust 再次清洗指令字段并绑定注册身份。当前并未使用二进制指令传输。结算后的世界经 `kk-world::world_json` 投影，下一轮才向玩家呈现新状态；查询接口使用独立的 `snapshot` 投影。

包含 JS 贡献的规则槽通过一次 VM 调用完成整条处理链；native 中心经普通 op 修改同一 Rust 候选，默认无扩展槽仍直调 Rust。组合顺序、候选同步和故障语义见 [Mod 处理链执行](mod-execution.md)。

核弹在 `landTime = gameTime + 1` 当轮冲击。该轮意图阶段先取消房内全部新 `createCreep`，不扣孵化能量；对象 tick 阶段清扫已有 creep/孵化任务并结算爆炸伤害。仅取消新孵化，其他 spawn 动作仍按原顺序处理。

lab 动作顺序为 boost → unboost → runReaction，否则 reverseReaction；已登记 runReaction 时即使结算失败也不回落 reverseReaction。

### power 效果链（当前首片）

`RoomState.effects` 按对象 ID 持有 `effect/power/level/end_time`，与对象候选一起克隆、删除和跨房迁移；同 power 施放替换，异 power 共存。常规消费以 `end_time > time` 判断有效，不逐拍改写或清空过期条目；factory 配方门与 powerSpawn 处理量保留官方 `≥` 边界。管理员 Generic 输入中的 effects 在插入或编辑时归一到此表，发布和恢复拒绝双权威或悬空条目。

`usePower` 当前完成 GENERATE_OPS、OPERATE_TOWER、DISRUPT_TOWER、OPERATE_SPAWN、DISRUPT_SPAWN、OPERATE_LAB、OPERATE_POWER、OPERATE_FACTORY、REGEN_SOURCE、DISRUPT_SOURCE、REGEN_MINERAL、OPERATE_STORAGE、OPERATE_EXTENSION、两种 terminal power、OPERATE_CONTROLLER、FORTIFY/SHIELD 与 OPERATE_OBSERVER。tower 两种效果分别通过目标类型、射程、等级覆盖、ops 与冷却门后挂到目标，统一扣 ops、设置冷却、发 EVENT_POWER 和动作日志。塔 attack/heal/repair 先计算距离衰减，再乘两种有效效果的 POWER_INFO 小数倍率，最后一次 floor；塔的能量费用保持固定值。全部 19 个 power 的结算消费者已接通。

spawn 两种效果复用同一施放、费用、冷却和效果表通路。OPERATE_SPAWN 仅在创建任务时将身体数 × CREEP_SPAWN_TIME 乘以等级倍率并 ceil，不重算已有任务；DISRUPT_SPAWN 每个有效 tick 将 spawnTime 延后一拍，保留 needTime，并暂停出生尝试。两效果可共存，干扰不阻止创建、续命或回收等其他动作。新增 creep 下一轮才参加对象 tick；一拍任务当轮先清除 spawning，creep 下一轮经孤儿出生路径使用默认方向。若一拍任务被同轮干扰延期，任务及指定方向保留，到期后按指定方向出生。公共效果表、任务绝对时钟和 creep 状态随检查点恢复及失败候选一起提交。

OPERATE_LAB 同时提高正反应产量/两份试剂消耗，以及逆反应化合物消耗/两份输出量；玩家预检与原生结算各自按当前有效等级计算整批数量，矿物清零时仍解绑槽容量。公开 effects 的修改只能影响玩家预检，不能伪造原生结算量。OPERATE_POWER 的批量为 min(库存 power, 1 + 等级增量)，能量不足整个批次不生效；processPower 玩家预检仍按基础量 1，最终扣款和账号 power 增量在候选中原子提交。此消费者使用 endTime ≥ gameTime，因此公开效果到期被过滤的当轮仍享受增量，下一轮恢复基础量；lab 使用严格 > 边界。两域均复用当前效果表与检查点布局，不新增状态字段。

OPERATE_FACTORY 首次成功施放将工厂 level 锁定为 power 等级，之后只有同级施放能刷新效果；效果到期不清等级，异级拒绝不扣 ops、不进冷却、不发事件。等级配方仍须工厂 level 与配方 level 相同，并在 endTime ≥ gameTime 时具有同级效果；无等级配方不要求此效果。工厂等级、效果、产出与组件库存均参与整轮候选和冷恢复，沿用现有 Generic level 与公共效果表布局。

REGEN_SOURCE 按 `(endTime - gameTime - 1) % period == 0` 补充能量，当前 period 为 15、等级补充量为 50/100/150/200/250；相位由施放/刷新时刻确定，补充不超过 source 当前容量。普通再生时钟为空或 0 时先重新挂钟。DISRUPT_SOURCE 在未满源的每个有效 tick 将普通 nextRegenerationTime 延后 1，再判断普通再生是否到点；不阻止 REGEN_SOURCE 的周期补充。满源跳过两种再生处理，同轮采集先结算；普通再生及 power 补充均先使用旧容量，再按控制器占有状态调整容量。两种效果都以 endTime > gameTime 为有效期，可共存，同级刷新重置相位；现有 source 时钟与公共效果表共同参与候选回滚、保存恢复和输入回放，不新增检查点字段。

OPERATE_TERMINAL（[8] duration 1000/cooldown 500/range 3/ops 100/effect [0.9..0.5]）挂在 terminal 上：terminal send 的意图相费用乘倍率 ceil 且边界 endTime >= time（官方 send.js），全局 market 相的转账费用乘承担方 terminal 倍率 ceil、send/deal 冷却 round(10×倍率) 且边界 endTime > time（官方 market.js）——截止拍两相按官方分叉（意图相享折扣、全局相收全费；意图相折扣被更严的玩家面预检支配，经玩家链不可独立观测）。deal 的费用端与冷却端恒为 dealer 终端（官方 executeTransfer 第五参 targetTerminal：SELL 单为接收端、BUY 单为发出端）。玩家面 send/deal 预检不乘倍率（官方 game 层同形，预检更严）。DISRUPT_TERMINAL（[15] duration 10/cooldown 8/range 50/ops 按级 [50..10]）封锁 creep/powerCreep 从 terminal withdraw（严格 > 边界，玩家面预检同门返回 ERR_INVALID_TARGET），并拒绝 OPERATE_EXTENSION 对该目标的灌能。订单生命周期每 tick 同步订单可成交量与 active：SELL 按 terminal 存量、BUY 按玩家 money 与 terminal 空余容量钳制，订单房缺失订单主的 terminal 时 active 置 false（不删除，市场投影只列 active 订单，本人订单另含非活动）。

OPERATE_STORAGE 效果挂在 storage 上，权威 `store_capacity` 保存在类型化对象中：对象 tick 在意图相之后按官方 storages/tick.js 的控制器归属/RCL 门更新（不匹配或 RCL<4 为 0），效果增量 [500k..7M] 仅在基础量 >0 时叠加、endTime > time 严格有效；无控制器时保留已保存容量。放置及建造初值为 1M。creep/powerCreep 的 transfer、Mod 精确转账检查、玩家投影与查询快照消费同一已提交值（withdraw 只出不进不消费容量）。截止拍的意图相仍读上一轮保存的加成容量，该拍对象 tick 才回基础量；已超存库存不钳、仅拒绝继续转入。容量随候选原子回滚/重试，随房间二进制快照保存与恢复；不从恢复时的世界时间重新派生。OPERATE_EXTENSION 为即时动作不落效果表：目标须为带 store 的 storage/terminal/factory/container 且有主时与房主一致、目标带活跃 DISRUPT_TERMINAL 效果拒绝；向房内同主非 off extension 按到目标的 Chebyshev 距离升序灌能，预算 min(目标 energy, effect[level-1] × Σextension 容量)（小数倍率，50 倍数容量下恒整），未移动能量整次静默拒绝（不扣 ops、不进冷却、不发事件）。

REGEN_MINERAL 施放门要求目标为非耗尽且无待再生时钟的矿物（nextRegenerationTime 真值判，0 与未设置等价），否则结算静默拒绝。消费者与 source 同用 `(endTime - gameTime - 1) % period` 相位（period 10、等级补量 2/4/6/8/10，无 ops 费用），补量无上限；效果门读当前存量与时钟——官方 bulk.update 以对象引用调用时 `_.merge` 回写内存，自然再生当拍回填后的新值参与判定、相位命中即叠加（该状态正常施放不可达，由装填/导入数据构造并有引擎级测试锁定）。耗尽矿物停补并交给普通再生时钟接管。普通再生对存量 0 的 Some(0) 时钟同样按官方 falsy 视为未设置——不立即回填，重新挂 50000 时钟。

OPERATE_CONTROLLER（[18] duration 1000/cooldown 800/range 3/ops 200/effect [10..50]）挂在控制器上，唯一消费者是 8 级控制器的升级意图（官方 upgradeController.js）：每 tick 升级限额从 CONTROLLER_MAX_UPGRADE_PER_TICK=15 抬升 effect 值，本轮多次升级按未强化量累计钳制，边界 endTime >= time（与 factory/powerSpawn 及 terminal 的 send 意图相同形；terminal 仅 market 费用/冷却为严格 >）；限额只钳未强化量，boost 增量不受限（既有语义）。8 级升级不写控制器 progress 只涨 GCL，效果在 8 级以下惰性（可施放、无消费）。玩家面 upgradeController 预检不涉限额（官方同形，纯结算侧）。

FORTIFY（[17] duration [1..5]/cooldown 5/range 3/ops 5）挂在 constructedWall/rampart 上：非核弹伤害被完全免疫（官方 _damage.js，严格 > 边界，免疫含扣血/摧毁/反击/事件整段）；dismantle 的能量回收被 SHIELD/FORTIFY/INVULNERABILITY 归零（官方 dismantle.js，>= 边界，伤害照常走 _damage 门）；rangedMassAttack 跳过 FORTIFY/INVULNERABILITY 目标（>= 边界）；玩家面 attack/rangedAttack/dismantle 预检对 FORTIFY/INVULNERABILITY 目标即拒 ERR_INVALID_TARGET（ticksRemaining > 0，SHIELD 不在预检门）。SHIELD（[12] effect [5000..25000]/duration 50/cooldown 20，效果条目无 effect 键——官方专用分支只写 {power, level, endTime}，通用路径才 effect==power）无 ops 无目标：先删自身位置 constructionSite，过对象/地形放置门（同类 rampart 不重叠、本格非墙、贴边时三块边界格全墙——官方 checkConstructionSite 的 rampart 子集）后原地插入临时 rampart（hits=effect、hitsMax=0、nextDecayTime=endTime、携带 PWR_SHIELD 效果）；护盾本体可正常受伤，衰减 tick 跳过 hitsMax 同步与衰减，endTime <= time 时由衰减门删除；POWER_INFO.energy 字段官方无消费者，不扣。EFFECT_INVULNERABILITY（1001）与 FORTIFY 共享全部消费门，其生产者属后续 invader core/NPC 域。

OPERATE_OBSERVER（[7] duration [200..1000]/cooldown 400/range 3/ops 10，无 effect 数组）挂在 observer 结构上，消费门在 observeRoom 预检：效果活跃（严格 > 边界）完全跳过射程检查——官方无 10+N 公式，效果即无限射程；无效目标（非 observer）结算侧静默丢弃不扣费。observeRoom 门序 NOT_OWNER→房名 regex→RCL（isActive）→射程（逐轴绝对差 ≤ OBSERVER_RANGE=10）；结算侧复验同门防直注意图注入后把目标房名写进 observer 的 data.observeRoom 标记，房间结算预意图阶段清零（官方 processor.js 同形）——观察视野恰一拍：下一拍投影可见、无重复意图的再下一拍消失。可见性在世界级预扫（`observed_rooms_of`）并入两通道可见集（JSON 投影与 KKPV 信封/私有块同源），被观察房的能量统计、事件与对象导出与己方对象房同口径；目标房未注册时不进 Game.rooms（官方查询自然落空）。标记随 Generic data 走既有快照/检查点通道，无新持久字段。放置侧：无公开 cooldown 的结构类型（observer 等）拒绝 cooldown spec（适配器契约）。

事件条目形与官方一致：attackController 等无 payload 事件省略 data 键（玩家面读 undefined 而非空对象）。JSON/KKPV 内部记录携带 endTime 和剩余时间，供原始预检使用；Game 对象装配和手动构造均只复制公开四键（effect/power/level/ticksRemaining——SHIELD 条目无 effect 键），过滤到期项。管理查询快照同样只返回活跃效果。检查点 v4 独立保存房间效果表，effect 可为 null（SHIELD 引擎施放形；管理放置缺省仍回填 power），其余字段必填、恢复重验引用/等级/时间/同 power 唯一性；完整失败撤销整个候选，局部拆毁拒绝恢复对象及效果，缓存重试不重复运行玩家。

### 仓库资源与 GCL

`transfer`/`withdraw` 除既有类型化仓库外，也读写 Generic 的 `store`；转入容量由 `storeCapacity` 和 `storeCapacityResource` 字段决定，无需扩充结构类型白名单。放置及建造落成的空仓也带原始 `store`，满足玩家 API 的目标预检；固定能量仓与新建仓库带显式 `energy: 0`。共享仓按所有资源的合计库存计算余量，分资源仓按对应槽计算；实验室首次装入矿物时绑定矿物槽，取空后恢复未绑定容量。同轮动作读取先前动作修改后的候选库存，避免竞争时超容。提取保留安全模式、敌方私有 rampart、nuker/powerBank 禁止提取和 terminal 干扰截止时间限制。Generic 仓库的 10 条默认容量已接入[冻结规则表](store-rules.md)，包括实验室槽绑定/释放的数值和终端容量归一；这些特殊动作策略仍由默认 native 执行；[creep 资源槽](creep-resource-slots.md)已开放 before/wrap/replace/after 与局部拒绝；[资源领域能力](mod-resources.md)允许可信 Mod 通过 replace 自定义转账、显式损耗及同房路由。默认特殊策略尚无各自独立入口，原例外依据见[核心经济记录](../research/notes/economy-gcl-2026-10-04.md)。

deposit 是 `type=deposit` 的 Generic 世界对象；倒计时、绝对时钟与 harvested 只接受非负 JS 安全整数，放置时钟换算检查加法及结果范围，非法值返回错误并保持世界不变。保存状态的数值域在插入、编辑、候选发布与检查点恢复时同源校验。放置归一把相对 cooldown/ticksToDecay 换算为绝对 cooldownTime/decayTime（与 nuke 的 timeToLand 同形；绝对键直传）；玩家面 Deposit 类提供 depositType/cooldown/lastCooldown/ticksToDecay（官方 game/deposits.js 派生：rawData 投影通道透传绝对 cooldownTime 由 getter 折算，查询快照通道直接给剩余 cooldown、派生 lastCooldown 并把 decayTime 折算为 ticksToDecay）。采集走 harvest 的 Deposit 分支：预检 isNearTo 与 cooldown→ERR_TIRED；结算量 = WORK 有效部件 × HARVEST_DEPOSIT_POWER(1)（harvest 类 boost 经部件效能表放大；官方 deposit 分支无 amount 门，WORK 全熄的 amount=0 仍落动作日志、按当前累计重算冷却并刷新寿命），直接入 creep 共享容量仓，超容部分掉落地堆（资源堆落成当拍即按 ceil(amount/ENERGY_DECAY) 衰减）；不发 EVENT_HARVEST、无库存上限。每次成功采集累计 harvested（含 boost 口径）并把 decayTime 刷新为 time+DEPOSIT_DECAY_TIME(50000)。冷却 = ceil(DEPOSIT_EXHAUST_MULTIPLY × harvested^DEPOSIT_EXHAUST_POW)（>1 才挂），经 pending 表在结算对象相落成 cooldownTime——同 tick 后续采集读旧值仍成功（官方 _cooldown 临时字段 + 对象 tick 落成同构）；寿命移除门在对象 tick 用 `time >= decayTime-1`（processor.js 移除相为实际边界，deposits/tick.js 的严格 > 为不可达兜底）。

### keeper lair（当前首片）

keeper lair 由 Generic 结构记录保存 `structureType=keeperLair` 与绝对 `nextSpawnTime`；管理放置的 `ticksToSpawn` 先校验非负安全整数再换算，插入/编辑/发布/恢复共用时钟校验。查询快照给剩余 `ticksToSpawn`，JSON/KKPV 玩家投影保留绝对键，由现有 StructureKeeperLair getter 折算（缺键、null、0 为未计时）；NPC 归属固定 `Source Keeper`，该用户名不能注册为玩家；lair 属于 FIND_STRUCTURES/FIND_HOSTILE_STRUCTURES，不进入玩家 Game.structures。对象 tick 在同房查找 `Keeper{lairId}` 且归属为 Source Keeper 的 creep：缺失或 hits<5000、当前无倒计时时开始 ENERGY_REGEN_TIME(300)；到 `time >= nextSpawnTime-1` 直接移除旧 keeper 并在 lair 本格生成 50 部件、5000 hits 的新 keeper（17 TOUGH、13 MOVE、10 组 ATTACK/RANGED_ATTACK，零仓容量、无初始寿命），随后计时置 null。替换不产墓碑/死亡事件；新 creep 下一轮初始化普通寿命，满血同名 keeper 抑制新倒计时。Source Keeper 不执行边缘格跨房过渡；keeper 巡守/战斗见下节，普通 Invader 行为见下节，raid/stronghold 与 keeper 地图生成仍未实现。来源为锁定官方 `processor/intents/keeper-lairs/tick.js`、`creeps/tick.js` 与 `game/structures.js` 的 StructureKeeperLair。

### Source Keeper 巡守与战斗（当前首片）

房间在玩家动作前对每个归属 Source Keeper 的 creep（无孵化过滤，同官方收集条件）生成预意图，与玩家动作共用有序派发、显式同名动作优先（官方 processor.js 的 `assign(ii, existing)` 覆盖规则）。巡守目标先读私有记忆 `source_id`（对象仍存在时有效），否则按对象表顺序（ID 升序即插入序）取同房原生 Source/Mineral 变体中距离 ≤5 的第一个并写入记忆；目标删除后暂留旧 ID，下一轮重选。距离 >1 时按官方 fake-runtime `moveTo` 同构移动：单房原生寻路（复用 kk-pathfinder，engine 不依赖 V8），goal range 1、路径缓存 50 tick 复用；代价矩阵按官方 `defaultCostMatrix`——障碍类型与 portal、非公有他人 rampart、自家障碍工地写入阻挡值，道路无条件覆盖先前写入（官方 Uint8 矩阵钳 255 仍小于 Infinity，按稳定对象序后写者赢）；creep 已在路径尾、或不在路径上且与首格切比雪夫距离 >1 时不发移动并清空路径缓存（官方 `nextDirectionByPath` 返回 0 同形）。

战斗分类与官方 `keepers/pretick.js` 一致：creep（含孵化雏形，与 Invader 退场的 `!spawning` 分类不同）与 powerCreep 且归属非 NPC（官方 `user != 2 && user != 3`）均视为敌方。距离 ≤1 桶按最低 hits 发 `attack`；≤3 桶按 `[10,10,4,1]`（索引即距离）估计 rangedMassAttack 收益，严格 >13 发 `rangedMassAttack`，否则对最低 hits 目标发 `rangedAttack`；两个目标独立选取。入桶不读 hits：手工检查点注入缺失 hits 的 powerCreep 仍参与 mass 估计，最低 hits 选择跳过它、全部缺失时不发（官方 `_.min` 跳过 undefined、全缺得 Infinity 发废意图由结算丢弃，kk 直接不发，可观察等价）。mass 估计是固定查表，不读实际身体效能或 rampart 减伤。

私有巡守记忆是房间级新保存域 `keeper_memory`（`source_id` 与可空 `move_cache`：目的地、绝对时钟、按行走顺序的同房路径），随整轮候选提交、随对象删除清理并参与候选回滚（对象恢复时记忆同恢复）；不进入任何玩家投影。编辑、恢复与候选发布共用值域校验：执行者必须是 Source Keeper 的 creep、目标 ID 在已分配范围、坐标 0..49、时钟为安全整数、路径 ≤2500 格且逐步相邻；载荷格式 v5 起必填（v4_to_v5 对缺域房间显式拒绝），不补造记忆。来源：锁定官方 `processor/intents/creeps/keepers/pretick.js` 与 `processor/common/fake-runtime.js`。已知对齐边界：官方 moveTo 成功时写 `memory_move.lastMove`（唯一消费者是 walkTo 的 NPC 推挤判定），Keeper 暂不保存；普通 Invader 已独立保存该时钟，walkTo 仅用于 stronghold，Keeper 时钟与推挤随该片补齐；NPC 预意图在 kk 中先于玩家派发（官方玩家在前、仲裁按对象 ID 重排洗平登记序），官方 roomObjects 取回序按 _id 升序是外部行为假设。keeper 跨房移动、raid/stronghold 仍未实现。

分片创建支持 `PlayerSpec.gcl = { level, progress? }`。输入按 `ceil(GCL_MULTIPLY × (level−1)^GCL_POW) + progress` 转成累计点数，验证等级回读、整数进度与 JS 安全整数范围；不提供 GCL 时，初始等级为 `max(已拥有房间数+1, 2)`。校验先于回收旧分片。升级的累计点数已由引擎候选更新，JSON 与 KKPV 玩家投影读取同一玩家状态；账号 `power` 可在 `PlayerSpec` 中提供，未提供时为 0；GPL 与 powerCreep 注册表由玩家候选持有。

### 普通 Invader 行为（当前实现）

NPC 预意图按稳定对象序遍历普通 Invader：身体含 HEAL（包括损毁）走 healer，否则走 findAttack，最后独立执行 shootAtWill。敌方桶为非 Invader / Source Keeper 的已出生 creep 与 powerCreep（后者不读取 spawning）。所有动作并入共用 batch，显式同名动作优先，交由既有移动、战斗、Mod 槽链、事件及候选发布结算；AI 不直接扣血或修改坐标。官方 roomObjects 顺序等于 _id 升序仍是外部假设。

findAttack 在无有效 ATTACK、有有效 RANGED_ATTACK 时先对距离 <3 的敌方执行 flee；严重受伤、无有效 ATTACK 则尝试靠近活跃友方 healer。近战先预选首个相邻敌人，最终被 findClosestByPath 结果覆盖；零步同格优先首个，否则相邻桶最后一个，不按 hits 评分。其余目标依次尝试忽略道路/creep、忽略 fortification、忽略可毁建筑三轮寻路，保留 incomplete 路径的末格选择语义。路径缓存首格上的非 spawn、属于 CONTROLLER_STRUCTURES 的建筑可改向 attack 或 WORK dismantle；shootAtWill 随后可覆盖远程建筑目标。零活跃 ATTACK 的最终 attack 仍可能产生零伤害事件，与官方一致。

healer 对三格内同归属 creep/powerCreep 按最大缺血量选择（平手首个、包括自己与满血），相邻 heal 否则 rangedHeal；跟随目标另按最近路径选择伤员，不能把治疗评分目标当作跟随目标。严重受伤先治疗再对距离 <4 的敌方逃离，否则靠近另一活跃 healer；健康 healer 无伤员时逃离或跟随非治疗同伴，无目标自杀。shootAtWill 仅在有效 RANGED_ATTACK 时对三格内最低 hits 敌方发单体攻击，不切换 mass；缺 hits 目标仍在近战桶，远程最低值选择跳过，全缺时不发有效远程攻击。

NPC 寻路共用原生 kk-pathfinder 与官方 fake-runtime 代价规则；计划级墙、静态对象/私有 rampart 通行与 Mod 建筑 obstacle/road 从同房冻结快照消费，见[静态通行契约](movement.md#静态对象通行声明g02d3-后续)。ignoreRoads 对应 plain=1/swamp=5、默认 plain=2/swamp=10。Invader moveTo 使用同房目的地和 5 tick 缓存，成功生成移动方向即记录 lastMove（不是实际移动成功后才记录）；flee 和 healer 邻格直接 move 不写缓存。对象表快照中的孵化、射程、安全模式、boost、rampart、压制、死亡仍沿现有结算链。

新增房间保存域 `invader_memory`（可空 move_cache：destination/time/path、可空 last_move），载荷 v6 必填；v5_to_v6 对缺域房间显式拒绝，不猜测旧路径。编辑、恢复和候选发布共用坐标/时钟/逐步相邻与路径 ≤2500 校验；执行者必须为 Invader creep，可空键必须存在。对象删除、链拒绝恢复和整轮失败均同步清理或恢复记忆，不进入玩家投影。Source Keeper 继续使用原 keeper_memory 与 50 tick 缓存。

来源：锁定 `@screeps/engine` 4.3.0 的 invaders/pretick.js、findAttack.js、healer.js、flee.js、shootAtWill.js 与 common/fake-runtime.js。本片只覆盖普通 Invader，不提供其地图生成/raid 调度、userSummoned/strongholdId 特殊状态和 stronghold 战术。walkTo 的唯一当前上游消费者是 invader-core/stronghold/creeps.js，NPC 推挤与 Keeper lastMove 保存随 stronghold 后续接入，不是普通 Invader 的移动前置。

### Invader 退场（当前首片）

NPC 归属使用精确保留用户名 `Invader` / `Source Keeper`，不能注册为玩家；普通短用户名不对应官方数据库短 ID。适配器将 screeps-ok 的 sk/srcKeeper 放置别名转换为这两个公开用户名，管理快照再映射回套件 handle；玩家 VM 中 owner.username 保持公开用户名，核心不依赖测试 handle。两种 NPC 均不执行边缘格跨房；Invader 控制器按官方用户 2 的规则跳过降级时钟。

房间在玩家动作前，基于同一候选生成非治疗型 Invader 的无目标自杀预意图。findAttack 三轮目标搜索均无目标时，检查 spawn 可达性；无不可达 spawn 且控制器有主才发自杀。损毁 HEAL 仍属 healer 分支，其无目标退场见上节；孵化对象的动作由既有门拒绝。Source Keeper 不视为 Invader 的敌方。严重受伤靠近 healer、多轮可达性与治疗分支已接入，普通 Invader 不再使用旧首片的保守保留门。

自动动作按官方 `processor.js` 的覆盖规则并入玩家 batch，显式同名动作优先，然后走既有所有权检查、动作压制、候选提交和事件路径。只有 Invader 的 suicide 用 drop_rate=0：仍创建墓碑并发死亡事件，但身体、boost 和携带资源都不向容器或墓碑转移；TTL/战斗死亡继续普通回收。owner、身体和墓碑沿既有 CreepObj/TombstoneObj 保存，移动记忆沿 v6 独立域保存；预意图按确定性房间输入重建，晚期失败不发布候选，冷恢复可重建退场与墓碑衰减。来源：锁定官方 invaders/pretick.js、findAttack.js、creeps/suicide.js 与 _die.js；userSummoned/strongholdId 特殊分支尚未开放。

## 候选提交与失败

世界结算在工作副本中处理房间、玩家及市场状态，验证通过后更新内存。结算返回错误时，已提交世界和时间保留旧值；引擎结算段失败会记录 `NeedsRecovery`，不能把这种错误描述成已完成的 tick。

`run_players` 缓存失败轮的运行结果和 `TickInput`。重试只再次结算这些候选，忽略新代码；玩家 RawMemory、segments 和 CPU 桶候选随世界统一发布。恢复期间修改世界或市场受到入口检查限制，显式放弃通过 `discard_pending_round` 处理。

Mod realm 在整轮结算返回错误时销毁，下次执行前按原启动快照重建并核验计划；恢复失败不取走玩家缓存、不重新运行玩家。显式丢弃轮次仍须完成 Mod VM 恢复，机制见 [Mod 执行](mod-execution.md#vm-故障恢复)。

玩家的 V8 全局变量不会随工作副本回退。执行线程或通信故障发生在结果确认之前时，也不能假定宿主已经完整收集候选；它不具备与普通结算失败相同的重试保证。可通过 `rebuild_player_workers` 显式放弃执行故障轮并重建全部沙箱；重建保留已提交玩家状态并清空全局变量。panic 和进程崩溃另有边界，见 [tick 契约](tick-contract.md)。

这些是内存内的行为，目前没有检查点或数据库恢复通路。

## 确定性与重试输入

当前世界和玩家结构使用稳定的遍历顺序，重试复用首次采样的 `TickInput`，避免签名时间等输入随重试改变。确定性要求针对相同已提交状态、指令和显式输入的结算；不能据此保证重新运行玩家代码得到相同结果。

玩家 `Math.random()`、墙钟执行预算和其他沙箱副作用不属于已提交世界的完整回放日志。磁盘级重放还需要保存输入、版本和恢复信息，要求见 [快照契约](snapshot-contract.md)。

## 空间索引

`kk-world::RoomIndex` 提供格子索引，供结算查找位置相关对象。索引需要构建和维护，性能收益须包括这些成本，不能用单次查询实验推导完整 tick 的提升。

当前房间结算仍有逐对象处理过程，没有完整的房间休眠补算或到期事件堆调度。若引入这些优化，资源再生、衰减和跨房输入仍须与逐 tick 参考执行对照。

## 并行流水线目标

玩家已采用有界并发。后续目标是在一个房间的全部潜在指令来源完成后开始该房间结算，重叠无关房间的工作。依赖集合必须是保守上界；无法证明完整时，该房间等待全部玩家。

房间任务输出私有候选，由协调器按确定规则合并；任务完成的墙钟顺序不能改变对象 ID、事件或玩家状态。RoomPatch、依赖推导、跨房合并和单线程对照要求见 [tick 契约](tick-contract.md)。房间工作者及玩家执行与房间结算的重叠尚未实现。

服务器 tick 速率控制、最慢玩家处置、提交后网络推送和后台检查点还属于目标服务的设计。收益和资源上限须按 [性能验收](performance-acceptance.md)测量。

## 分片装配热身拍

`EmbedEngine::create_shard`（含 Mod 创建入口）在房间、控制器、玩家工作者与计划装配后执行一次空意图世界结算，与锁定的 screeps-ok vanilla 适配器 `createShard` 末尾 `guardedTick` 对齐。它不求值玩家代码、不写玩家 Memory/CPU 候选，但运行正常世界与 Mod 结算；成功创建的世界时间为 1。RoomSpec 的相对 ticksToDowngrade/safeMode 在种子时加一拍补偿，创建完成读回种子值。场景对象在此后放置，不受热身拍影响。

热身输入 `TickInput.warmup=true` 只影响发布时的计数：推进 time/generation，保持 player_ticks 不变，因此 setTerrain 在首个显式推进轮之前仍可用。普通玩家、外部意图、空推进与回放轮均为 false。createShard 先摘除旧输入日志，热身拍不产生日志段；日志起点是热身后状态。检查点保存既有 time/player_ticks 标量，恢复直接装配已提交状态，不重复热身。装配热身失败沿用创建后段的失败契约：旧分片已回收，调用方须重新创建。
