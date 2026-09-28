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
| `Game.structures` 收录范围（M0 桩环境；M2A-A2.1 已清偿） | 已按官方只收自家 owned 结构（game.js:302-307 OwnedStructure + userObjects；无 owner 的 neutral 结构不入）；Game.creeps/spawns/constructionSites 同批切 per-viewer（game.js:273-280/:313-318/:366-372） | my/FIND_MY_* 按 world.user 判定；视野数据面裁切见下方新条目 |
| 验收仪表/宿主 tick 通道的防篡改深度（M0 桩环境） | `__kk` 已整体 freeze（防属性覆盖）；玩家直接调用 `drainMissing()` 抢先清空仪表的深层防御未做 | 根治需宿主侧闭包/独立 op，归 M2 玩家 API 层重写范围 |
| 房间名解析强度（kk-core RoomName） | **强类型构造即校验**：方向位非法/符号消费错位/Unicode 数字一律 Err；官方 roomNameToXY（utils.js:374-392）对方向位只判 W/w 与 N/n、其余一律当 E/S（"X1N1"→[1,-2]、"E1X1"→[1,1]、"W-100N5"→[99,0] 静默别名到其他房间），parseInt 还认 Unicode 数字 | 官方这些形态得到"合法但错误"的坐标；我们不产出静默别名坐标。embed 边界吃外部房间名时该差异玩家可见（screeps-ok 用例若依赖官方宽松态会显形） |
| creep TTL 死亡的产物（M1 引擎） | **资源直接掉地为 energy 对象，不产 tombstone**；官方 _die.js:16-33 产 tombstone 承接全部资源与 body 造价回收 | M1 范围无 tombstone 变体（kk-schema v1）；M2 加 tombstone 变体走 schema 版本升级。M1 内资源守恒语义一致（总量不变），缺的是墓碑对象与 decay 链 |
| flag 的存储位置（M1 引擎） | flag 折进 RoomObject 变体（tag 8）；官方 flag 是独立集合（processor.js:22 bulkFlags，不参与 roomObjects 循环与结算） | M1 单房间结算 flag 无规则行为差异；投影层（world_json → prelude）输出官方形状即可 |
| 地形输入校验强度（kk-core Terrain） | 长度≠2500、未知位（& !0b111）、数字串形态的 '8'/'9' 一律 Err；官方 checkTerrain 对字符串形态照单全收（'9' 判为 wall） | 正确性-7/8 的清偿方向：非法输入前置拒绝而非静默截短/误读；官方吃 '9' 是无校验不是语义 |
| spawn.spawning 投影形态（M1 typed world；查询面已清偿 M2A-A1） | 世界投影（prelude 消费面）发绝对 `spawnTime`；查询面快照（getObject）已按官方 Spawning 形态发 `name/needTime/remainingTime`（structures.js:1302-1310，remainingTime=spawnTime−time；vanilla snapSpawning 同构）；directions 字段契约快照不含 | prelude 玩家面消费随 M2A-A4 动作族接线 |
| source/controller 的 None→0 投影（M1 typed world；查询面已切 null 形态 M2A-A1） | 世界投影（prelude 消费面）无钟时发 0；查询面快照已按契约发 null（`t ? t−time : null`；safeMode 用 `t>time` 的 active 形态）——两消费面各自契约 | 玩家面仍是 0 形态（inspection "preserves undefined as null" 用例红）——M2B 快照直读时玩家面收拢到同族 null 形态 |
| 放置的 owner 宽松接受面（M1 place 归一化） | typed 变体不强制 owner（缺省空串）；vanilla 对 REQUIRING_OWNER 类型与 placeCreep/placeSite/placeFlag 强制 owner 并抛错 | M0 哑世界同宽松；强制化随 M2 玩家 API 层（届时违规放置显形） |
| source 容量三分支缺 reservation（M1 place 归一化） | ControllerObj 无 reservation 字段，被预定（无主）房间 source 容量判 1500；vanilla 读 user || reservation 判 3000 | 当前放置 spec 无法表达 reservation 不触发；reserveController intent 实装时补字段 |
| extension 能量容量（M1，深评清偿） | transfer 钳制与 energyAvailable/energyCapacityAvailable 投影均按 `EXTENSION_ENERGY_CAPACITY[rcl]`（extensions/tick.js:10-17）；无 controller 房按 0 拒收（官方 capacityForResource undefined ⇒ 拒） | ExtensionObj 仍无容量字段（结算/投影实时查表）——多资源槽（storeCapacityResource 全量）M2 schema 扩展时一并定型 |
| 非 energy 掉落资源（M1 place；GPT 复审清偿更新） | 已支持 power(1)/G(2)（resource_type 编码；G 掉落同格同资源并堆、全资源同式衰减 energy/tick.js:12）；其余矿物仍拒 | 矿物种类随 schema 资源槽扩展放开 |
| 未知类型工地（M1 place，深评收严） | 无 CONSTRUCTION_COST 的类型 Err（与 vanilla getProgressTotal:1873 抛错对齐；原先宽松接受、progressTotal 派生 0） | build 结算同样拒绝无 cost 目标（build.js:19-21），两侧一致 |
| creep 放置 ttl 无上限钳制（M1 place） | `time + ttl` 原样（可超 1500）；官方 renew 才有 1500 上限语义（拒绝式非钳制） | 三方评审裁决跟 vanilla 放置语义（index.ts:1087），2026-09-28 |
| 玩家内 room intents 先于对象 intents（M1 结算） | createConstructionSite/createFlag 全部先于 creep/spawn 对象指令执行 | 官方单层 objects map 中 'room' 键按插入序交错；结果序无关（房间指令与对象指令无共享状态面），登记迭代序差异 |
| move intent 的 id 形态（M1 结算登记；A4-iv 核对时确认已清偿） | intent_move 双形态（direction/id）与 prelude move 的朝相邻 creep 形态均已实装（creep.js move :137-141 / creep.rs intent_move :86-103） | 登记条目滞后未销——A4-iv 的 pull 位移依赖该形态（被拉 creep 记 `move(拉者)`），embed/settle 锁值测试已钉死 |
| creep 死亡事件与墓碑（M1 结算） | 无 EVENT_OBJECT_DESTROYED、无墓碑（资源直接掉地已另登记） | 随墓碑变体（M2 schema 升级）一并补事件 |
| spawn/extension 的 off 标记生效时序（M1 结算） | `_calc_spawns` 直接改工作副本 → 同 tick 即时生效 | 官方经 bulk.update 延迟落库 → 同 tick 读到旧值；仅配额翻转 tick 可观测（多一个 spawn 被计费的窗口） |
| 踩敌方工地的 safeMode 门（M1 结算） | 未实现（M1 无 safeMode 激活路径） | 官方 movement.js:224 要求 controller 非敌 safeMode；activateSafeMode 实装时补 |
| 无 controller 房间的建造配额（M1 结算） | 完全跳过配额检查 | 官方按 rcl=0 计（spawn/extension 配额 0 → 拒、road 2500 → 允许）；M1 测试房间恒有 controller |
| createCreep 空 energyStructures（M1 结算） | `[]` 落回全房间扣费（与缺省同路径） | 官方空数组走显式列表模式（可用 0 → 失败）；根因是 sanitize 后 None/[] 不可区分，M2 intent 通道定型时修 |
| harvest 0 个 WORK 部件的事件形态（M1 结算，深评清偿） | 已对齐官方：`work_power<=0` 整体跳过（harvest.js:44 `if(harvestAmount)` 包住 actionLog+eventLog），不产 amount=0 事件 | API 层不可达（ERR_NO_BODYPART 拦截）；引擎侧零量事件原则已与官方一致 |
| Engine 沙箱的玩家身份（M1，外部复审提前清偿） | **结构性强制**：intent 只归本沙箱注册键，冒名键整批丢弃 | M2 真沙箱接入时该不变量直接复用；沙箱内多玩家形态（如观察者）届时另议 |
| controller 的 hits/hitsMax 投影（M1 typed world） | 不发（prelude 兜 0） | M0 哑世界发 10000 手抄值；官方 getter 直读 DB 字段、vanilla 放置不带 hits → 官方为 undefined。0 vs undefined 是 prelude 残差，M2 玩家 API 层对齐 |
| typed 变体丢弃 spec 的非 energy 资源与私有时序字段（M1 place） | creep/spawn/extension/storage 的 store 仅保 energy；ticksToDecay/cooldown 等不透传（road 恒 ROAD_DECAY_TIME） | M1 schema 资源单槽；多资源种类随 M2 schema 扩展放开 |
| 未注册房间放置（M1 embed） | 报错（房间不存在） | M0 哑世界自动建房、vanilla 不校验——显式拒绝优于静默建出无地形房间；screeps-ok 套件先 createShard 后放置，不可达 |
| 多房间世界 JSON 的 objects 序（M1 typed world） | 房间名序 × 房内 id 升序 | M0 为全局 id 序（单房间等价）；多房间下 Game.creeps 等键插入序可能不同——若出现顺序敏感用例以此归因 |
| controller safeMode 族投影（M1 typed world，外部复审清偿） | 世界投影发绝对 tick（safeMode/downgradeTime），prelude 按官方 getter（structures.js:174-186）换算剩余值：safeMode/ticksToDowngrade=绝对−time、safeModeAvailable 透传 | 无值/过期归 0（None→0 归一口径）；safeModeCooldown 仍桩 0（无该状态字段，M2 激活链路时补） |
| movement 四键完全平手的决胜序（M1 结算，深评登记） | 矩阵登记序 = 对象 id 升序；官方 = 玩家 intent 到达序（processor.js:236 for...in 插入序） | 四键全平时胜者可观察分歧；官方到达序本身依赖 DB 查询序（弱保证），id 序是更强的不变量——顺序敏感用例出现时以此归因（D4-1 的 intent 序侧补充） |
| sanitize 层未接生产（M1 结算，深评登记） | `sanitize_object_intent` 只有单测消费（M1 唯一 intent 源是 typed 的 ScriptedSandbox，无不可信输入面） | M2 真沙箱 intent 通道定型时在入口接线，否则为死代码 |
| to_int 的数值域上限（M1 sanitize，代码质量轮清偿） | parseInt 语义已全量仿真（前缀截断 `"12abc"`→12、`"0x10"`→16、`"1e2"`→1、指数形态 Number 取首位 `1e-7`→1）；与官方的剩余差异仅 i64 域外：官方 parseInt 返回大数（如 1e20），我们返回 None（整体拒绝/哨兵） | 差异只在荒谬输入面可达：\|值\|≥~9.2e18（数值路径）、字符串 >2^53 我方比官方更精确（i128 不舍入）、数组元素形态（官方 String([5])="5"→5，我们 None）、空白集边界（U+0085/U+FEFF 两侧 trim 集不同）；M2 intent 通道接线如有对齐需要再议 |
| userString 截断单位（M1 sanitize，深评登记） | 按 Unicode chars 截 100；官方 substring 按 UTF-16 码元（100 单位 = 50 个 CJK 字符） | ASCII 名字无差异；M2 玩家名/旗名面显形时对齐 |
| energyAvailable 投影的房主过滤（M1 typed world，深评登记） | 房间合计全部非 off spawn/extension；官方 game.js:325-328 按查看者过滤自家结构 | runPlayer 已按 viewer 注入（M2A-A2.1），房间能量值的 per-viewer 过滤随 A3c 收尾（当前单主测试房等价） |
| directions 显式空数组（M1 结算，深评登记） | `[]` 折为"无限制"（缺省 1..8）；官方存 `[]`（truthy）→ _born-creep 零方向循环、出生坐标 undefined 的病态行为 | 官方自身行为即坏，不作对照基线；JS API 层 `directions: []` 官方就判 ERR_INVALID_ARGS，intent 层不可达 |
| build 的"同格 swamp/wall 对象"系数分支（M1 结算，深评登记） | 不查（typed 世界无 swamp/wall 对象类型，地形走 Terrain）；官方 create-construction-site.js:39-44 对同格对象也乘系数 | 当前状态空间不可达；M2 若经 Generic 通道合成此类对象需补判定 |
| schema v1 内字段追加（M1 kk-schema，深评登记） | ControllerObj 尾部加 `is_power_enabled`、ConstructionSiteObj 中部加 `progress_total`，未 bump SCHEMA_VERSION | 版本冻结纪律保护的是已发布的持久字节流；M1 快照尚无任何落盘数据在流通，v1 内演化无损。首个持久化格式发布（M2 存储）起 bump 纪律生效，升级链挂载点已在 `UPGRADE_CHAIN` 占位 |
| 查询面快照投影独立于 prelude 世界投影（M2A-A1） | embed getObject/findInRoom 出 screeps-ok 契约形状（`kind` 字段族 + getter 值形态：剩余 tick/正量 store/storeCapacity），snapshot.rs 单源；prelude 世界投影（world_json.rs）维持 M0 契约形状 | 两个消费面（验收查询 vs 玩家世界视图）契约不同；一致性由 embed_tests 对表断言锁定（M0 第七轮层间一致教训）；M2B 快照直读时玩家面收拢到同族 getter 形态 |
| FIND_STRUCTURES/FIND_MY_STRUCTURES 的 source/mineral 排除（M2A-A1 修正） | matches_find 与 prelude room.find 两层同步排除 source/mineral（官方 game.js:229-251 structureTypes 表无此三类、:329-345 独立装配分支；vanilla 查询面 findTypeMap 同构 snapshots.ts:478-491） | 修正前 source 会漏进 FIND_STRUCTURES（M0 以来两层同错）；FIND_MY_* 的 owner 过滤仍缺（M2A-A3c per-viewer） |
| 查询面 storeCapacity 的 null 形态（M2A-A1，评审三轮定裁） | creep 发 `>0` 数值（活跃 hits>0 CARRY × 50 × boost 乘数，vanilla `storeCapacity || null` 同构——0/无 CARRY 读 null）；spawn/extension 恒 null（官方落库带 storeCapacityResource——build.js:117-135，无参 getCapacity() 为 null，store.js:28-33；adapter-spec.md 明文 "null for a store with per-resource limits such as a spawn's"）；storage 发 STORAGE_CAPACITY 数值 | kk 单资源槽 schema 无 storeCapacityResource 概念，数值容量经 energyCapacityAvailable/结算面消费；storeCapacityByResource 面随 M2 schema 扩展 |
| 查询面剩余值的 saturating 钳 0（M2A-A1） | 契约头注明 "unclamped"（官方过期挂钟发负数），我方 saturating_sub 钳 0 | 正常路径不可达（钟到点被结算消费清掉）；仅缺结算路径（如 road decay 未实装）或构造性放置可观察，消费方按 <=0 过期等价 |
| createShard 种子钟 +1 与无 warm-up tick 失配（M2A-A1 评审登记，既有根因） | kk createShard 不消耗 vanilla 的 warm-up tick（种子 +1 的前提，vanilla index.ts:968-975），engine time 停 0 而 kk embed.rs 种子公式含 +1 | inspection 的 controller timer relativity 两用例差 1（基线红）——归 createShard 语义轨道（A3c 或 createShard 补 warm-up tick 时清偿） |
| Generic 透传快照不折算 getter 形态（M2A-A1） | 透传对象（mineral/tombstone 等）的计时字段（decayTime/cooldown/lastCooldown 公式）不做剩余值换算，只保底 kind/id/pos | 契约相关用例（deposit/tombstone 族）本就在能力门 skip 池；随 B3/B4 对应能力批补折算 |
| intent 通道的编码与键覆盖（M2A-A2） | intentsList 以 JSON 编码经 endTick 返回值带出（官方为 ExternalCopy 结构化拷出，runtime.js:271-296——玩家不可见面）；'room' 键数组通道已实装，'global'（market/powerCreep）与 'notify' 键未实装（随对应能力批） | 二进制编码（ADR-004 决策 5）随 M2B 性能基线评估；未实装键当前无生产者 |
| Memory 写回失败时 intents 的处置（M2A-A2，套件裁决后对齐官方） | 写回失败（ok:false）→ intents 同步丢弃、Memory 保留上一 tick（UNDOC-MEMJSON-005 锁定）；错误分类仍归玩家 runtime（M1 六轮定谳的契约不变） | 官方 stringify 抛错传播为 isolate 级故障故 intents 整轮丢（make.js:279-294）——我方分类降级为软错误但 intents 处置跟官方；计费维持正常结算路径（套件未锁该面） |
| cancelOrder 退费不查免费表（M2A-A2） | remove 对 say/pull 也退 0.2（官方 runtime.js:95 无条件减——可把 intents.cpu 减为负的官方 quirk） | 原样保留（红线第 30 条官方 quirk 复刻）；prelude 收集器同实现 |
| prelude harvest 预检的 reservation 缺失（M2A-A2） | API 层预检只判 controller.owner（被预定无主房官方判 reservation 归属）；schema 无 reservation 字段 | 与"source 容量三分支缺 reservation"同根，B2 controller 结算族补字段时一并清偿 |
| API 实现读预置常量而非全局（M2A-A2） | prelude 动作方法经闭包捕获的预置常量表判定错误码/部件名；玩家覆盖全局常量不影响 API 判定 | 官方 game 层的 C 来自 driver.constants 注入对象不可篡改（game.js 头部），同款防篡改语义 |
| room 级 intent 的计费形态（M2A-A2 登记；宿主计费半边已于 61d99c8 修齐） | 宿主 count_billed 已按数组长度计（make.js:105 对照）；仍缺：prelude pushByName 不加 0.2（官方 runtime.js:89 每次 push 计费）且无 maxLen 参数、官方 push 方法（notify 通道，上限 20）缺席 | room/global API 全是桩（A4 接线）——当前无生产者不可见；接线时补 pushByName 计费/maxLen 与 push |
| 预检的伪实例面（M2A-A2） | prelude harvest 等只判 instanceof（官方 register.sources 等注册表成员校验 + assertTargetObject）；伪 Source 形对象可过预检（计 0.2、结算侧真身检查丢弃） | 玩家影响 = 白付 0.2 与说谎的返回码，无法推动世界；wrapper 世界全量来自投影、伪造面有限，A3b 薄壳化时随注册表语义收紧 |
| 视野过滤的数据面裁切（M2A-A2.1，外部复审 P1-3 清偿；自审补工地键） | runPlayer 注入世界按查看者裁切：只注入其**非工地非旗**自有对象所在房（官方 data.js:113-116 归集显式跳过 flag/constructionSite——自审修复：工地曾错误授予房视野），可见房内敌方对象保留（官方同构）；observer 授 observeRoom 房视野（data.js:117-119）随对应能力批；不可见房 rooms 条目 terrain-only（Game.map 面保留全量地形） | 官方按 userObjects 归集房间——M2A 近似不含"仅 observer 可见"等形态；world.user 字段是官方 runtimeData.user 等价物；controller sign/reservation 不授视野（官方仅进用户名解析，data.js:123-125） |
| 结算侧工地总量帽（M2A-A2.1，外部复审 P1-2 纵深） | 官方 MAX_CONSTRUCTION_SITES=100 检查在 game 层（rooms.js:1071-1073），结算侧不复查；我方结算按（玩家,房间）现存数复核 | 正规路径不可达（API 层先挡）；per-room 近似跨房总量随多房引擎；意图是伪造/绕过 API 面的兜底 |
| 宿主消息的预置 JSON（M2A-A2.1 外部复审 P1-1 + 自审 P1/P2 全链清偿） | 编码全走加载期捕获的预置 stringify（官方 systemFunctions.JSON 同款）；**解码侧同批预置化**：beginTick 世界解码与 runExpr 值规范化往返的 parse 均捕获引用（官方 runtime.js:207 对宿主数据同款）；Memory 懒解析保持全局 parse（官方 game.js:482 逐字一致，玩家只能伪造自己的 Memory 视图） | 官方 ExternalCopy 亦不经玩家全局；对抗探针锁定（覆写后伪造 room intent 不进通道） |
| 宿主信封与出口净化的结构位（自审 P1 + 对抗评审 P1/P2 清偿） | 三层：①宿主信封（endTick/runLoop/runExpr/drain）null 原型构造（官方 runtime.js:214 outMessage 同款）+ 收集器 data/注入世界顶层剥原型（官方 :56 对注入侧剥顶层的等价扩展）；②**出口净化窗口**（sanitizedStringify）：宿主消息序列化期间摘除 Object/Array 根原型的 toJSON（save/restore，摘除容错——configurable:false 降级赋 undefined、freeze 走出口 catch 的字面量信封兜底）——官方出口 ExternalCopy 对玩家 toJSON 不可见（runtime.js:296），摘除=等价；③预置捕获全家（JSON.stringify/parse、Object、Array、eval）——`globalThis.Object` 交换曾实证绕过 null-proto 防线整体（假 assign/create 产出真原型信封） | Memory 写回**不**净化（官方 systemFunctions.JSON.stringify 是原生语义、玩家 Memory 的 toJSON 参与自己的写回——runtime.js:247）；runExpr 探针值序列化净化（官方控制台走 ivm 不调 toJSON，净化更贴官方）；两枚对抗探针锁定（toJSON 污染 + Object 交换组合：真实 intent 存活、伪造 room intent 不进通道、探针值不被改写） |
| Game.structures 收录自家 spawn（自审 P3 清偿） | spawn 同时进 Game.spawns 与 Game.structures（官方 structureTypes 表含 spawn，先经 structures 装配再进 spawns，game.js:298-318；StructureSpawn 是 OwnedStructure 子类，structures.js:854） | 修复前只进 spawns（else-if 链漏收）；Game.structures 的其余收录边界不变（自家 owned、不含 source/mineral/road/wall） |
| `__kk` 通道的可调用面（自审 P2 登记，M2B op 下沉根治） | 玩家直调 `__kk.beginTick(伪造世界)` 可清空已记 intents 计费/注入伪造世界/反复 rebuild（DoS）；`__kk` 冻结只防改写不防调用。**JS 侧相位守卫已评估并否决**：硬中断 tick 的 Rust 善后路径（sandbox.rs:340-348/:383-392）不执行 JS endTick，相位会永久卡 'begun' 使世界停更——功能级回归风险大于纵深收益 | 危害有界（已验证闭合）：伪造 id 只能命中不存在对象（settle 丢弃）或敌方对象（所有权校验丢弃），不存在"藏在自己不可见房的自有对象"可被唤醒；官方结构性免疫（玩家侧无宿主 tick 函数，runtime 在 driver 进程）；根治=宿主窗口标志下沉 op（begin 窗口外 no-op），随 M2B 通道 op 化；与 drainMissing 抢调（:39）同属"冻结防改写不防调用"类 |
| Game.flags 成员资格双分歧（自审 P2 登记，随 schema 批清偿） | 官方 Game.flags = 本人 flag 全集（按 user 全量拉取与视野无关，data.js:140 → game.js:397-421），FIND_FLAGS 才是可见房限定；我方 = 可见房内一切 flag（含敌方）且不可见房自家 flag 消失 | 根因 FlagObj 无 owner 字段（M1 schema）；清偿需 schema 补 flag owner + 装配改官方形状（本人全集全局注入），随 B3/B4 schema 演化批 |
| query 面 FIND_MY_* 的 owner 过滤缺失（层间不一致，A3c 清偿） | embed matches_find 的 FIND_MY_CREEPS/MY_STRUCTURES/MY_SPAWNS 与对应 FIND_* 同臂无 owner 过滤；prelude room.find 已按 viewerName 过滤——两层语义现已分歧（M0 第七轮层间一致教训的现存实例） | screeps-ok 经 embed oracle 验 MY_* 存在假绿面；A3c per-viewer 批将 viewer 贯穿 find_in_room 签名链时一并清偿 |
| 预置常量的写入面（自审 P3 登记） | C 表对象常量冻结共享引用（freezeConst）：判定面与官方等价（覆写不影响 API 判定），但玩家写入严格模式抛 TypeError——官方 C 是每 tick 深拷贝（game.js:435 `_.extend(globals, jsonParse(jsonStringify(C)))`），玩家写自己全局静默成功且下 tick 重置 | 极端边缘（无 bot 写常量全局）；如需收尾改 C 表捕获时深拷贝即可；sandbox.rs 常量平铺不 freeze 的待验证注释同此归并 |
| 寻路回调内 load_terrain 的拒绝（C2，我方结构性防护） | `search` 回调内再调 `load_terrain` 返回 Err（重入锁拒绝）而非死锁 | 官方无此面（地形仅启动期载入，driver/lib/path-finder.js:23-46，回调内无 loadTerrain 调用点）；玩家不可达（宿主 Rust API），登记备查 |
| sign 的 datetime 墙钟字段（B2 登记；GPT 复审灰区纠错已清偿） | 官方结算时落一次 Date.now()（signController.js:23）、getter 从持久值构造——已对齐：schema Sign.datetime(opt u64 毫秒)+结算落墙钟+双投影 Some 带键+getter new Date(o.sign.datetime)；缺省回落当前墙钟仅手写世界无 datetime 可达（官方不可达） | 已清偿销账 |
| reserve 超窗的"幻影预约"（B2 登记） | 官方 reserveController.js:35-39 先写内存对象再于 :43-45 拒（幻影仅本 tick 后续处理器可见、不入库）；我方超窗整票拒绝 | 同 tick 多 reserve 且首条恰好超窗时可观察分歧；与"结算期直接写工作副本"类既有登记同族，顺序敏感用例显形时归因 |
| 饱和截断的官方负数等价族（B2 登记） | attack 削减 endTime/downgradeTime 官方可得负数（null−effect / 削穿 0）；我方 u64 饱和 0 | 两实现终态等价（负值与 0 均被同 tick 清除/降级链消费，清除条件同真）——锁值测试钉死（null downgradeTime 当 tick失主权、削穿即清除） |
| EVENT_ATTACK_CONTROLLER 的 data 键（B2 登记） | 官方事件无 data 键（attackController.js:52）；我方 push_event 封套恒带空 data | 投影层形状，轨 A eventLog 投影接线时消化 |
| controller 族的 GCL 门读数（B2 清偿记录） | 结算期读上一 tick 的 PlayerState.rooms 定集（官方 bulkUsers.addToSet 不改内存对象，bulk.js:93-101）；Engine tick 末按世界真值重建 | 同 tick 双 claim 都按旧定集放行（官方 quirk，锁值测试钉死——settle 期若误更新 rooms 即转红） |
| unclaim 未实现（B2 范围决策；A4-iii 清偿记录） | intent 白名单/结算器已补（intent.rs Action::Unclaim + controller.rs intent_unclaim 对照 unclaim.js）；**不清 reservation/sign/upgradeBlocked**（官方 bulk update 键表无此三项——与降级归零路径的官方刻意差异，锁值测试钉死） | API 面（prelude unclaim 方法）与执行器均已接线 |
| to_user_string 的 falsy 折叠角落（B2 登记，承 M1） | 官方 `("" + (value\|\|""))` 将 0/false/NaN 折叠为 ""（→sign 清除语义）；我方 Number(0)→"0"、Bool(false)→"false" | 仅畸形 intent 输入面可达（API 层预检先挡）；与 UTF-16 截断同条登记的姊妹差异 |
| 类原型 toJSON 的出口窗口（A3b.1 登记，评审 2） | sanitizedStringify 窗口只摘 Object/Array 根原型 toJSON；A3b 起各游戏类原型挂官方同形 toJSON（可覆写）——runExpr 探针值通道上玩家可伪造嵌套壳的序列化形态 | 官方 ivm 出口对玩家 toJSON 不可见（runtime.js:296）；intents/Memory 宿主消息面无游戏对象（实证）；出口窗口扩类原型清单归 M2B 出口批，验收探针侧先读 id/原语 |
| beginTick 装配路径的裸全局查找族（A3b.1 登记，评审 2） | 装配期 `new Map/Set/Proxy`、`Object.setPrototypeOf`、兜底 `Object.keys/defineProperties` 经可交换全局——玩家交换后下一 tick 自家装配崩（自害面，封顶于视野/仪表） | makeStore 已用预置 `gw.__Object`；其余统一预置捕获归 A3c/M2B 装配批（对抗评审"预置捕获全家桶"原则的贯彻项） |
| isStructure/isActive 超集进 toJSON（A3b.1 登记） | 我方壳 own 挂 isStructure:true/isActive（官方无此字段）——进 Object.keys 与 toJSON 输出 | M0 起既有暴露面；A4 Store 族清偿时随官方字段面收敛 |
| CREEP_METHODS 超集/缺集（A3b.1 登记，承 A2） | 桩列表含 spawn/recycleCreep/renewCreep/moveHead（非官方 Creep 方法）；controller 族五方法（claim/reserve/attack/sign/generateSafeMode）A4-iii 已换真下桩 | 剩余超集项随 A4 方法族换真按官方 creeps.js 全集对表收敛 |
| Memory 破损态的优雅降级（A3b.1 登记，评审 1） | `Memory.creeps = 5` 类破损后官方读 undefined/写抛专用错（creeps.js:94-120 的 _.isObject 特判）；我方 strict 下写属性 TypeError | 玩家自伤边角，文案差异；随 A4 memory 面批次对齐 |
| upgradeBlocked 未暴露（A3b.1 登记） | 官方 controller getter 有（structures.js:147）；我方 Controller 字段表未列（投影有 upgrade_blocked） | A4-iii controller API 面连同 reservation/sign/safeModeCooldown 投影一起补 |
| prelude lodash 共享实例（A4-i 评审 2 P2-2 登记，承 M0） | 官方把 engine bundle 编进玩家 isolate、内建共享，**唯独 lodash 私有**（user-vm.js:38 + game.js:478-480 的 runInContext 独立实例）；我方 `__kkp.lodash === globalThis._` 同一实例——玩家改写 _.contains/_.isArray 等可骗 API 判定门 | 全部状态改变经 Rust 结算真值裁决（from_json body 白名单/directions range/calcCreepCost 消毒后计费——谎言只作用于攻击者自己的返回码）；私有 lodash 副本（runInContext 同构）为轨道级结构项归 M2B |
| spawn 族数组载荷的出口注入面（A4-i 评审 2 P3 登记） | body/directions 数组经引用入 intents，出口 sanitizedStringify 只摘两根原型 toJSON——数组**自有** toJSON 可在序列化窗口改写载荷 | 结算吸收链完备（BodyKind 白名单滤空→body.is_empty 早退/MAX_CREEP_SIZE 截断/消毒后计费+真值扣费/越界整单拒）——注入更贵 body→扣费失败、更便宜→等价合法传参；代码注释"数组元素经净化窗口兜底"措辞仅对继承面成立（已修正认知） |
| storage 容量的 RCL 归零（A4-i 评审 1 P3-4 登记） | 官方 storages/tick.js:8 按 RCL/归属把 storeCapacity 归零（controller 失效→0）；我方投影恒 1e6 | M2 无结构停用路径不可达（isActive/RCL 门未接线）；随结构停用批清偿 |
| 资源族判定门的预置捕获清偿（A4-ii 评审 2 清偿记录） | 缺省钳制路径裸 Math.min/max（交换可产负缺省绕 amount<0 前置门）、白名单裸 Array.prototype.includes、容量折算的 for...of 迭代（Symbol.iterator 可污染）——结算真值复核均兜住（最坏说谎码+0.2），判定门纪律统一预置（__MathMin/__MathMax/__arrayIncludes）+ 索引迭代 ×4 处 | 家族登记项：STRUCTURE_REGISTER_TYPES.has（Set.prototype.has）/.find/.some 为视野或自害路径不预置；beginTick/buildGame 裸 new Map/Set、既有 harvest/build 扫描归 A3c 装配批统一；intents 载荷对象 amount 的自有 toJSON 出口差异（官方 ExternalCopy 不调、我方序列化窗口可换值——结算四路钳死）归 M2B 出口批 |
| upgradeBlocked 未暴露（A3b.1 登记；A4-iii 清偿记录） | 官方 controller getter 有（structures.js:147）；我方 Controller 字段表未列（投影有 upgrade_blocked） | 已换真：四 getter（reservation/sign/upgradeBlocked/safeModeCooldown）+ 双投影（world_json 原始形态 / snapshot 契约形态）一并落地 |
| activateSafeMode 门③ 的无钟房返回码（A4-iii 登记） | 官方读 `this.ticksToDowngrade`（undefined 参与比较为假——门过，intent 记录后结算侧 processors/activateSafeMode.js:20 用原始 downgradeTime null→0 同拒）；我方 ticksToDowngrade 维持 None→0 口径 → 0 < 阈值为真 → API 直接 ERR_TIRED | 仅返回码差异、终态一致（两边 safeMode 都不激活）；随 ticksToDowngrade getter 换官方 undefined 形态的批次消除 |
| claimController 预检的 GCL 读数（A4-iii 登记，A3c 清偿） | 官方读 `runtimeData.user.gcl` 点数；我方按 Game.gcl 逆变换近似（level/progress → 点数）——Game.gcl 现状硬编码 {level:1,progress:0} → 0 点：首房（claimed=0）门短路放行、已有房/同 tick 二 claim 时 ERR_GCL_NOT_ENOUGH | 已占房计数按注入世界真值即时算（= 官方 runtimeData.user.rooms 的上一 tick 定集，投影 tick 内静态——同形）；结算侧 claimController.js:41-43 用 PlayerState 真值复核，本门最坏是说谎返回码 + 0.2 计费；Game.gcl 真值接线归 A3c per-viewer 批 |
| 查询面 controller 快照的超额键 upgradeBlocked（A4-iii 登记） | vanilla ControllerSnapshot 契约无 upgradeBlocked 键（common.ts:44-54 的字段表止于 safeModeCooldown/reservation/sign）；我方快照额外出 active_remaining 形态的该键 | 契约消费方结构校验不受额外键影响；prelude attackController 门⑦ 需要该面（读原始投影，不经快照）——保留供调试/对表，随契约演化定夺 |
| creep 世界投影 store 的 ghodium 键（A4-iii 登记） | 正量才投影（0 值无 "G" 键——官方 store getter 正量暴露口径/vanilla snapStore 同构）；energy 维持恒投影的既有形状 | generateSafeMode 预检读原始投影 `store['G']`（官方 data(this.id).store 同位）——缺键得 undefined 判 NOT_ENOUGH_RESOURCES，语义同官方空 store |
| API 判定门路径的缺失键原型污染族（A4-iii 评审 2 登记） | 玩家 tick 内执行的判定门读 `store['G']`/`C.CONTROLLER_DOWNGRADE[投毒 _level]` 等——缺键经 Object.prototype 原型链可被喂假值（gclPoints 读可改写的 Game.gcl 同族） | 均被结算侧世界真值复核兜住（最坏说谎返回码 + 0.2 计费，无世界推进——伪实例面既有登记族）；myClaimedRoomCount 的 Set 已预置捕获（评审必修）、门路径扫描已改索引迭代；彻底消除归 API 门路径统一预置/纯量化批 |
| activateSafeMode 门④ 的可信侧兜底（A4-iii 评审 2 必修清偿记录） | 官方 game 层跑 isolated-vm 可信 realm，门④（任一自家 controller safeMode 生效 → ERR_BUSY）处理器不复核；我方 prelude 与玩家共享 realm——原型迭代器污染可令门④扫描失明 → 双房 safeMode 并存（官方不可能态） | Engine 层 tick_with_intents 按世界真值兜底（玩家已有生效 safeMode → 其 ActivateSafeMode intent 全部丢弃）；prelude 门④扫描改索引迭代（纵深防御）；同 tick 多 controller 去重由收集器同名覆盖+lastActivateSafeMode 结构性保证 |
| moveTo 寻路退化（A4-iv 登记，**本批最大灰区**） | **直线桩形态**：预检段完整对照官方（creeps.js:158-257），寻路退化为 `getDirectionTo` 直线一步；`memory._move` 路径缓存（reusePath=5/serializeMemory 面）不落 | 行为差异面：①绕障——被墙/结构围住的目标官方 A* 绕行或 ERR_NO_PATH，我方恒朝直线方向发 move intent（返回 OK）、结算侧被障碍静默拦截；②对角穿墙——官方代价/拓扑约束下不走的对角步我方可能发出；③路径复用——官方 5 tick 内沿缓存路径走、我方每 tick 重算方向（十字震荡目标下逐 tick 之字形同 Official 直线趋势，但贴墙绕行场景路径逐点不同）；④跨房——官方可寻至邻房（含出口格语义），我方按世界坐标给直线方向、move intent 在边缘格被钳。归 M2B FFI 批（红线一·7：官方 pf.cc 逐点一致）；01-movement 的 moveTo 族用例（MOVE-* 直线可达子集外）预期仍红 |
| say 的 actionLog 文本载荷（A4-iv 登记，B1.1 healType 同口径） | u16 位图只落事件位 bit9（ACTION_SAY）；官方 actionLog.say 是 `{message: 截 10 字, isPublic}` 对象 | `creep.saying` 读不到文本（恒 undefined——官方无 say 记录时同值，有记录时官方返回消息）；EVENT 层无 say 事件（官方亦无）；schema 换正式 actionLog 形态时恢复双键与文本 |
| creep 侧 notifyWhenAttacked 落库（A4-iv 登记） | 官方 creep 出生即 `notifyWhenAttacked: true`（create-creep.js:93）且 processor 通用段可写（processor.js:313 的 creep 分支）；我方 CreepObj 无该字段 | Creep API 面本就无 notifyWhenAttacked 方法（官方也只在结构类暴露）、通知面未接线——不可观测；damage 通知接线时若需 creep 侧读数随 schema 补字段 |
| notifyWhenAttacked 的通知发送面（A4-iv 登记） | 只持久化标志（typed 五对象 + Generic data JSON + build 落成即 true 对齐 build.js:116）；`_damage.js:78`/`towers/attack.js:63` 的受击 Game.notify 发送不发生 | M2 通知面（notify 通道 + 上线推送）未接线；结构/塔被攻击的玩家提醒缺失，flag 值本身已可读写并可经投影观察到 |
| suicide 后序 intent 的幽灵语义（A4-iv 评审 1/3 双确认登记） | 官方处理器持死引用快照继续执行且对其他对象的效果落地（processor.js:241 先取引用、_die.js:12-13 只删集合、claim/upgrade/…/pull 处理器无 actor 存活复检）——`suicide(); upgradeController(c)` 同 tick 官方升级照常生效；我方对象消亡后全早退 | 有意分歧：处理器持 pre-death 快照侵入面大、真实 bot 罕见该组合；次级边角同根——死者矩阵项我方零 rate 必输、官方幽灵按真 body 竞争可赢格；已入矩阵的 move 不落格两侧同形（官方 execute 不遍历死对象） |
| moveTo 越界坐标（A4-iv 评审 1 登记） | 官方 RoomPosition 构造器对 x/y∉[0,49] 抛 'Invalid arguments...'（rooms.js:1280-1287）；我方构造无校验，moveTo(60,25) 静默直线 | 目标解析段的未复刻抛错面；随 RoomPosition 构造器校验批次决定（bot 生态罕见传越界）；fetchXY 的 Number.isNaN 类型门 vs 官方强转 isNaN 的返回码差同族（'ab' 类目标我方放行到寻路段） |
| say 的 isPublic 出口折叠（A4-iv 评审 2 清偿记录） | 官方 system.js:8 的 boolean transform 在 JS 边界 !!value；我方出口序列化不摘对象自有 toJSON——对象形态可携大载荷过通道（结算 to_bool 终值兜住） | 已清偿：出口前 `!!isPublic` 折叠对齐官方边界；同通道的 message 为字符串字面量拼接免疫 |

## 四、验收关卡（不做完不算兼容）

1. **screeps-ok 行为一致性套件**（`sources/screeps/screeps-ok`，vanilla 2897 用例）从 M1 起进 CI 持续跑 parity；
2. 真实开源 bot 回归（tooangel/overmind 类）在 deno_core + CJS 垫片 + lodash3 底座上原样可跑；
3. 官方客户端/renderer 可正常连接游玩（API/sockjs 协议兼容）；
4. 对照移植检查表：每个 API 移植时登记"官方实现 file:line ↔ 本实现 file:line"映射。
| G 资源执行器丢弃（GPT 复审 P1-1 清偿记录） | A4-ii prelude 按 RESOURCES_ALL 放行而执行器仅 energy（API 返回 OK 结算静默丢弃+计费）——已参数化：transfer/withdraw/drop 的 energy/G 两槽结算（Store 槽助手+per-resource 容量分岔+G 掉落 resource_type=2+同格同资源并堆+全资源同式衰减） | 矿物等其余资源随 schema 槽扩展；A4-ii 报告的执行器分歧 1/2/4/5（safeMode 拒/敌 rampart/container 先入桶/Generic 目标族）仍登记待轨 B |
| 部件强化效能统一（GPT 复审 P1-2 清偿记录） | 容量投影算 boost 而移动重量/疲劳恢复/仲裁 moves 不算（boosted CARRY/KH 重量虚高、ZO 恢复虚低）——已统一：kk_world::types::calc_body_effectiveness（官方 utils.js:623-636 的 OnceLock 查表版）单源供 12 处消费（容量/重量/仲裁/恢复/build/upgrade/attack/repair/dismantle/heal×2/harvest）；canMove 保持 _.some 计数（官方 movement.js:11-14 同款） | rangedAttack/rangedMassAttack 执行器接线时按 BOOSTS.ranged_attack 键接效能；claim/reserve/attackController 官方纯计数不接（BOOSTS 表无 claim 键） |
| progress/progressTotal 的边界等级（GPT 复审 P2-4 清偿记录） | 我方 `|| 0` 折叠使 RCL 0 的 progress/progressTotal 与 RCL 8 的 progressTotal 恒 0；官方（structures.js:180-181）分别为 undefined——已修：getter 加 level 门（level>0 / level>0&&level<8） | 已清偿；ticksToDowngrade/safeMode 的 None→0 口径仍维持（既有登记，API 返回码差异随换真批次消除） |
