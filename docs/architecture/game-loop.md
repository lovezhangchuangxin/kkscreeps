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

`usePower` 当前完成 GENERATE_OPS、OPERATE_TOWER、DISRUPT_TOWER、OPERATE_SPAWN、DISRUPT_SPAWN、OPERATE_LAB、OPERATE_POWER、OPERATE_FACTORY、REGEN_SOURCE 和 DISRUPT_SOURCE。tower 两种效果分别通过目标类型、射程、等级覆盖、ops 与冷却门后挂到目标，统一扣 ops、设置冷却、发 EVENT_POWER 和动作日志。塔 attack/heal/repair 先计算距离衰减，再乘两种有效效果的 POWER_INFO 小数倍率，最后一次 floor；塔的能量费用保持固定值。其余 9 个 power 的结算消费者仍待后续切片。

spawn 两种效果复用同一施放、费用、冷却和效果表通路。OPERATE_SPAWN 仅在创建任务时将身体数 × CREEP_SPAWN_TIME 乘以等级倍率并 ceil，不重算已有任务；DISRUPT_SPAWN 每个有效 tick 将 spawnTime 延后一拍，保留 needTime，并暂停出生尝试。两效果可共存，干扰不阻止创建、续命或回收等其他动作。新增 creep 下一轮才参加对象 tick；一拍任务当轮先清除 spawning，creep 下一轮经孤儿出生路径使用默认方向。若一拍任务被同轮干扰延期，任务及指定方向保留，到期后按指定方向出生。公共效果表、任务绝对时钟和 creep 状态随检查点恢复及失败候选一起提交。

OPERATE_LAB 同时提高正反应产量/两份试剂消耗，以及逆反应化合物消耗/两份输出量；玩家预检与原生结算各自按当前有效等级计算整批数量，矿物清零时仍解绑槽容量。公开 effects 的修改只能影响玩家预检，不能伪造原生结算量。OPERATE_POWER 的批量为 min(库存 power, 1 + 等级增量)，能量不足整个批次不生效；processPower 玩家预检仍按基础量 1，最终扣款和账号 power 增量在候选中原子提交。此消费者使用 endTime ≥ gameTime，因此公开效果到期被过滤的当轮仍享受增量，下一轮恢复基础量；lab 使用严格 > 边界。两域均复用当前效果表与检查点布局，不新增状态字段。

OPERATE_FACTORY 首次成功施放将工厂 level 锁定为 power 等级，之后只有同级施放能刷新效果；效果到期不清等级，异级拒绝不扣 ops、不进冷却、不发事件。等级配方仍须工厂 level 与配方 level 相同，并在 endTime ≥ gameTime 时具有同级效果；无等级配方不要求此效果。工厂等级、效果、产出与组件库存均参与整轮候选和冷恢复，沿用现有 Generic level 与公共效果表布局。

REGEN_SOURCE 按 `(endTime - gameTime - 1) % period == 0` 补充能量，当前 period 为 15、等级补充量为 50/100/150/200/250；相位由施放/刷新时刻确定，补充不超过 source 当前容量。普通再生时钟为空或 0 时先重新挂钟。DISRUPT_SOURCE 在未满源的每个有效 tick 将普通 nextRegenerationTime 延后 1，再判断普通再生是否到点；不阻止 REGEN_SOURCE 的周期补充。满源跳过两种再生处理，同轮采集先结算；普通再生及 power 补充均先使用旧容量，再按控制器占有状态调整容量。两种效果都以 endTime > gameTime 为有效期，可共存，同级刷新重置相位；现有 source 时钟与公共效果表共同参与候选回滚、保存恢复和输入回放，不新增检查点字段。

JSON/KKPV 内部记录携带 endTime 和剩余时间，供原始预检使用；Game 对象装配和手动构造均只复制公开四键（effect/power/level/ticksRemaining），过滤到期项。管理查询快照同样只返回活跃效果。检查点 v4 独立保存房间效果表，字段必填、恢复重验引用/等级/时间/同 power 唯一性；完整失败撤销整个候选，局部拆毁拒绝恢复对象及效果，缓存重试不重复运行玩家。

### 仓库资源与 GCL

`transfer`/`withdraw` 除既有类型化仓库外，也读写 Generic 的 `store`；转入容量由 `storeCapacity` 和 `storeCapacityResource` 字段决定，无需扩充结构类型白名单。放置及建造落成的空仓也带原始 `store`，满足玩家 API 的目标预检；固定能量仓与新建仓库带显式 `energy: 0`。共享仓按所有资源的合计库存计算余量，分资源仓按对应槽计算；实验室首次装入矿物时绑定矿物槽，取空后恢复未绑定容量。同轮动作读取先前动作修改后的候选库存，避免竞争时超容。提取保留安全模式、敌方私有 rampart、nuker/powerBank 禁止提取和 terminal 干扰截止时间限制。Generic 仓库的 10 条默认容量已接入[冻结规则表](store-rules.md)，包括实验室槽绑定/释放的数值和终端容量归一；这些特殊动作策略仍由默认 native 执行；[creep 资源槽](creep-resource-slots.md)已开放 before/wrap/replace/after 与局部拒绝；[资源领域能力](mod-resources.md)允许可信 Mod 通过 replace 自定义转账、显式损耗及同房路由。默认特殊策略尚无各自独立入口，原例外依据见[核心经济记录](../research/notes/economy-gcl-2026-10-04.md)。

分片创建支持 `PlayerSpec.gcl = { level, progress? }`。输入按 `ceil(GCL_MULTIPLY × (level−1)^GCL_POW) + progress` 转成累计点数，验证等级回读、整数进度与 JS 安全整数范围；不提供 GCL 时，初始等级为 `max(已拥有房间数+1, 2)`。校验先于回收旧分片。升级的累计点数已由引擎候选更新，JSON 与 KKPV 玩家投影读取同一玩家状态；账号 `power` 可在 `PlayerSpec` 中提供，未提供时为 0；GPL 与 powerCreep 注册表由玩家候选持有。

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
