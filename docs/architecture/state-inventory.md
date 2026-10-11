# 持久化状态域清单与 schema 门禁

本页逐域登记 `EmbedEngine::checkpoint()` 载荷的持久化字段、版本策略、恢复消费者与迁移状态，并说明钉死键树的机器门禁。周期检查点的目录布局（`ckpt-{tick:010}.json`）与 sim 元数据游标（`log_cursor`，随检查点文件 metadata 域写入）是宿主侧持久面（G26.a）：不新增引擎载荷域、不进键树门禁（对门禁是不透明叶子），由 sim 恢复选择器消费。检查点的机制、文件格式与发布协议见[基础检查点](checkpoints.md)；本页是"有哪些域、归谁管、怎么验收"的权威清单。G23 负责迁移协议；JSON 载荷域自 G23.a 起走显式迁移链（见下表"载荷格式版本"行），信封与其余内容域对旧版本仍一律拒绝。

## 机器门禁：钉死键树

`crates/kk-runtime/src/embed/checkpoint/tests.rs` 的 `checkpoint_schema_keys_are_pinned` 提取三个场景真实检查点的键树并集，与 `PINNED_CHECKPOINT_KEYS` 逐项比对：

- **官方包全量场景**：双房/双玩家夹具（JS+二进制模块、Mod 对象、市场订单与交易、通知/segments、停用包），双投影通道各采一份，并给 p1 注入跨 shard 资源余额钉住 `resources` 字段存在性。
- **故障自动停用场景**：mounts 声明包的处理器连续故障跨过停用阈值，`fault_streak`/`last_fault`/`last_auto_disable` 取非空值，并携带已声明 mount 的扩展字段钉住 `extensions` 字段存在性。
- 对象展开全部键；数组折叠为 `路径[]` 并下钻首元素；数据键映射（玩家/房间/包/订单等）折叠为 `.*` 仍下钻内部 schema；调用方或 Mod 自管内容（`metadata`、`mods.options`、manifest、扩展字段载荷、`engine.mod_states` 的 namespace state）作为不透明叶子不再下钻。
- 任何批次给持久化结构**新增或删除**字段，本测试即红。处置顺序：先补齐该字段的保存/恢复与版本策略（当批接线，不能等 G23），再同步本页表格与钉死清单；新增动态映射表或不透明叶子须同步登记 `SCHEMA_DYNAMIC_MAPS` / `SCHEMA_OPAQUE`。
- 可空数组的 null 与数组分支分别记为 `path` 和 `path[]`；工作者模块的 binary 两种形态均登记。Embed 默认 main 是真实 JS 源码（binary=null），显式上传可整体替换；该值域沿用既有模块信封，无新增字段或版本升级。
- 键树只约束 schema 形状，不约束取值；取值等价由既有冷恢复往返相等测试与 sim 双通道验收覆盖。夹具为空的映射表（见下文"覆盖边界"）不会产生键，schema 变化不被本门禁发现，仍靠解码端 `deny_unknown_fields` 兜底。

## 编排层域（`kk-runtime` `embed::checkpoint::Checkpoint`）

| 域 | 字段路径 | 版本策略 | 恢复消费者 | 迁移状态 |
|---|---|---|---|---|
| 版本门 | `engine_version`、`room_schema_version` | 与当前二进制硬等值，不等即拒 | `restore_checkpoint` 入口检查 | 拒绝不迁移，G23 负责 |
| 玩家工作者 | `workers.*`（`heap_limit_bytes`、`cpu_limit`、`bucket_cap`、`max_per_tick`、`initial_bucket`、`install_wall_limit`、`constant_overrides.*`、`modules.*.js`、`modules.*.binary`（null 或字节数组）） | 随 `engine_version` 整体硬门；schema 另被键树钉死 | 工作者注册与模块重装；配置重验后重建玩家线程 | 拒绝不迁移，G23 负责 |
| Mod 包身份 | `mods.packages.*.manifest`、`mods.packages.*.files_sha256`（不透明） | 身份核验：恢复时与调用方提供的包 manifest 与全部文件 SHA-256 逐一比对，含未选择包 | `ModIdentity::verify`；相同包可换目录，缺包/多包/内容变化拒绝 | 拒绝不迁移，G23 负责 |
| Mod 配置身份 | `mods.options`（不透明） | 与 `RegisterOptions` 重建身份核验（内容选择、逐包配置、冻结选项、VM 预算、profiler 开关、停用阈值） | `ModIdentity::verify` | 拒绝不迁移，G23 负责 |
| 宿主玩法协议 | `host_protocol` | 当前 `kk-gameplay-v2-utc-date`，严格等值 | 替代分片装配前核验；日志头段同源 | 旧时区语义明确拒绝，无自动内容迁移 |
| 载荷格式版本 | `checkpoint_format_version` | 当前 9、必填（缺失即拒）；v1→v2（last_auto_disable 归一）、v2→v3（缺 host_protocol 标旧语义）、v3→v4（缺房间 effects 拒绝）、v4→v5（缺房间 keeper_memory 拒绝）、v5→v6（缺房间 invader_memory 拒绝）、v6→v7（缺房间 raid 拒绝）、v7→v8（缺房间 power_bank_time 拒绝）、v8→v9（缺房间 room_status 拒绝）已注册 | `migrate::migrate_to_current` 解码前路由 | 显式迁移链（G23.a 起生效；后续格式变化逐链追加） |
| Mod 执行状态 | `disabled_mods[]`、`execution_strategy`、`fault_streak`、`last_fault`、`last_auto_disable` | 无独立版本；停用集合须满足依赖闭包，恢复后停用包 schema/规则仍有效；`execution_strategy` 为组合摘要（plan_hash+停用闭包+每槽派生执行面），恢复在 realm 重建后重算等值核对，v2 起 Mod 检查点必填（v1 载荷宽容） | realm 重建后回填故障与停用状态，策略身份以重注册产物为准 | 拒绝不迁移，G23 负责（last_auto_disable 旧形态已收编为 v1→v2 迁移） |
| 协调器 | `concurrency`、`kkpv` | 无独立版本 | 恢复后并发与投影通道初值（sim 可由 CLI 参数覆盖） | 拒绝不迁移，G23 负责 |
| 最近 console | `last_console.*[]` | 无独立版本 | 恢复后 console 续接 | 拒绝不迁移，G23 负责 |
| 调用方 metadata | `metadata`（不透明，调用方自管） | 宿主不解释内容、无宿主侧 schema，因而不设迁移（域本身的存在仍被键树钉死）；sim 用于 `initial`/alias/断言起点 | 原样返回给保存方 | 调用方自理 |

## 引擎层域（`kk-engine` `settle::checkpoint::EngineCheckpoint`）

| 域 | 字段路径 | 版本策略 | 恢复消费者 | 迁移状态 |
|---|---|---|---|---|
| 世界标量 | `engine.world.time`、`seed`、`player_ticks`、`next_obj_id`、`generation`、`terrain_epoch` | 随 `engine_version` 硬门；装配时校验时间/种子/游标一致性 | 世界重建；`player_ticks` 回填按显式推进轮计数（createShard 装配热身拍只推进 time，不计入该值） | 拒绝不迁移，G23 负责 |
| 房间快照 | `engine.world.rooms.*.snapshot[]`（顺序二进制） | `room_schema_version`（kk-schema `SCHEMA_VERSION`，当前 2）硬门；v2 新增 storage `store_capacity` 权威容量，v1 缺状态显式拒绝 | 房间对象（含 storage 容量）/地形/事件整体解码；房名、坐标、地形、全局 ID 唯一性与 deposit 绝对时钟/累计量、keeper lair nextSpawnTime 安全整数校验；keeper lair 沿用 Generic 字符串载荷、Source Keeper 沿用 CreepObj，无新增二进制字段或版本升级 | 拒绝不迁移，G23 负责 |
| 房间对象效果 | `engine.world.rooms.*.effects.*[]`（effect/power/level/end_time；effect 可空——PWR_SHIELD 引擎施放形态（官方专用分支无 effect 键），载荷中为 null，管理放置 ingress 缺省 effect=power；v4 内放宽，旧读取器不认 null，无存量检查点） | 载荷 v4 必填；缺失不补造，引用、等级、时间与同 power 唯一性重验 | `World::from_checkpoint` 重建表；投影/玩法读取绝对时间，删除/跨房与宿主同行 | 缺该域的旧布局拒绝，当前格式冷恢复与失败重试已覆盖 |
| 房间扩展字段 | `engine.world.rooms.*.extensions`（动态映射，载荷不透明） | 扩展字段按 schema/引用校验 | Mod 对象扩展字段回填 | 拒绝不迁移，G23 负责 |
| NPC 巡守记忆 | `engine.world.rooms.*.keeper_memory`（动态映射：`source_id`、可空 `move_cache{destination[]、time、path[][]}`） | `checkpoint_format_version` v5 起必填（v4_to_v5 对缺域房间显式拒绝，不补造）；编辑/恢复/候选发布共用值域校验（执行者为 Source Keeper 的 creep、目标 ID 在已分配范围、坐标 0..49、时钟安全整数、路径 ≤2500 且逐步相邻） | Keeper 巡守目标与 50 tick 路径缓存重建；对象删除/候选回滚随宿主同清理或恢复，不进玩家投影 | 拒绝不迁移，G23 负责 |
| Invader 移动记忆 | `engine.world.rooms.*.invader_memory`（动态映射：必填可空 `move_cache{destination[]、time、path[][]}` 与 `last_move`） | 载荷 v6 起必填，v5_to_v6 缺域显式拒绝；执行者为 Invader creep、坐标 0..49、时钟安全整数、路径 ≤2500 且逐步相邻 | moveTo 5 tick 缓存与生成移动方向的 lastMove；对象删除/链拒绝恢复/整轮候选同步，不进玩家投影 | 缺状态拒绝，不猜测历史路径 |
| powerBank 生成调度 | `engine.world.rooms.*.power_bank_time`（必填可空：null 或下次到期 tick） | 载荷 v8 起必填，v7_to_v8 缺键显式拒绝；种子命令键白名单 powerBankTime、非负安全整数或 null | genPowerBanks 生成器的调度输入（首轮只写时间、到期生成并重置）；不进任何玩家投影 | 缺调度拒绝，不补造默认（已调度房静默回到首轮形态不可接受） |
| 房间公共状态种子 | `engine.world.rooms.*.room_status`（必填可空：null 或 {status,timestamp}） | 载荷 v9 起必填，v8_to_v9 缺键显式拒绝；createShard rooms[].status 种子（normal/novice/respawn/closed，时间戳=未来 1 小时墙钟毫秒），域校验限四态枚举与正安全整数 | 玩家面 getRoomStatus、launchNuke 门、核弹保护区移除；status/statusTimestamp 随房投影（公共地图数据不按视野裁切） | 缺种子拒绝，不补造默认（种子房静默回到 normal 不可接受） |
| Invader raid 房间状态 | `engine.world.rooms.*.raid`（必填 `invader_goal` 可空、`harvested_energy`、`active`、`status`） | 载荷 v7 起必填，v6_to_v7 缺域显式拒绝；种子命令校验非负整数预算/阈值、非空房态、预定属主须为本分片非 NPC 玩家 | raid 生成器的阈值/活跃/房态输入，harvest 结算逐次累加预算；不进任何玩家或管理快照 | 缺状态拒绝，不补造默认（活跃位丢失会静默放行生成） |
| 玩家账户 | `engine.players.*`：`cpu.*`（桶 5 字段）、`gcl`、`rooms[]`、`raw_memory`、`segments.*`、`active_request[]`、`public_ids[]`、`default_public`、`foreign_request`、`money`、`resources.*`（动态映射） | 随 `engine_version` 硬门；CPU 与 Memory/segments 限额在装配时校验 | 玩家状态重建、下一次执行的注入与授权 | 拒绝不迁移，G23 负责 |
| power 账号域 | `engine.players.*.power`（账号 power，GPL 源）、`power_creeps.*`（动态映射：注册表记录 8 字段，`powers.*` 为等级表；`spawn_cooldown_time`/`delete_time` 为墙钟毫秒） | 随 `engine_version` 硬门；旧检查点缺失按 serde 默认回填（0/空表） | powerCreep 注册表重建、GPL 配额与冷却判定 | 拒绝不迁移，G23 负责 |
| 玩家通知 | `engine.players.*.notifications[]`（`message`、`tick`、`group_interval`） | 无独立版本 | 跨 tick 最近 100 条内存记录 | 拒绝不迁移，G23 负责 |
| InterShardMemory 本地段 | `engine.players.*.ism_local`（字符串或 null，≤100×1024 UTF-16 码元） | 随 `engine_version` 硬门；旧检查点缺失按 serde 默认回填 null | 每玩家跨 tick 的 ISM 本地段注入与写回（setLocal 触发拍携带候选） | 拒绝不迁移，G23 负责 |
| 市场 | `engine.market.orders.*`（订单 12 字段，动态映射）、`transactions[]`、`next_order_seq`、`next_transaction_id` | 随 `engine_version` 硬门；订单生命周期按墙钟判过期，装配时校验市场游标 | 市场状态重建与游标续发 | 拒绝不迁移，G23 负责 |
| 事件 | `engine.last_events[]` | 无独立版本 | 最近成功轮事件回填 | 拒绝不迁移，G23 负责 |
| 动作记录导出 | `engine.last_action_log`（房间 → 条目：id/type/structureType/name/x/y/actionLog 动作键→载荷；与 last_events 同为跨拍读面） | 无独立版本 | embed `captureActionLog` 读面（上一成功结算拍的 room-history 形状产物；同拍死亡对象不进导出、非公开 say 裁剪） | 拒绝不迁移，G23 负责 |
| Mod 提交状态 | `engine.mod_states`（动态映射，载荷不透明——namespace state 是 Mod 自管 JSON） | 无独立版本；与包身份一致性由 realm 重建核对 | Mod namespace state 回填 | 拒绝不迁移，G23 负责 |
| 引擎杂项 | `engine.next_id`、`engine.plan_hash` | `plan_hash` 恢复时由重新注册实际产生并等值核对，不等即拒 | 对象 ID 游标续发；冻结计划一致性 | 拒绝不迁移，G23 负责 |

## 服务目录宿主侧域（`kk-runtime` `service`，S1.1 现行）

与 sim 宿主侧持久面平行的服务目录域；不进引擎键树门禁（同样是 `metadata` 不透明叶子内的新键树），由服务选择器严格解码消费。

| 域 | 字段/文件 | 版本策略 | 恢复消费者 | 迁移状态 |
|---|---|---|---|---|
| 目录身份 | `identity.json`：`service_format_version`、`world_id`、`world_seed`、`scenario_sha256`、`initialized` | 信封 `kkscreeps-service-identity` v1 严格等值；`initialized` 必须 true；服务格式版本当前 1，v2 随 S2 切片升级 | `service::identity::read`（run 入口）；身份缺失/损坏拒绝运行 | 新域首版，无历史迁移 |
| 检查点服务元数据 | `metadata.service`：`service_format_version`、`world_id`、`publish_seq`、`tick`、`world_seed`、`scenario_sha256`、`entries.*` | v1 严格 `deny_unknown_fields`；载荷序号须与文件名一致，world_id/种子/内容身份/格式与目录身份一致，tick/种子与引擎载荷交叉一致；保留策略按完整版本计数 | `service::checkpoints::select_latest`（无日志恢复选择）与 `enforce_keep` | 新域首版，无历史迁移 |
| 服务检查点文件 | `checkpoints/cp-<publishSeq>.json`（复用检查点信封与引擎载荷） | 信封同基础检查点；发布序号目录唯一分配、不复用 | 选择器降序取首个完整份；撕裂/哈希损坏跳过并计数 | 随基础检查点 |
| 单写者租约 | `writer.lock`（无内容语义） | 无版本；固定 inode、持锁期间不删除 | `kk-storage::WriterLease`（进程级互斥，崩溃由 OS 释放） | 不适用 |

## 不持久化域

| 域 | 不保存的原因 |
|---|---|
| V8 堆与 JS 全局变量 | 冷恢复重建 VM；玩法持久状态必须写 `ctx.state` 或对象字段，玩家写 Memory/segments |
| 后台服务 VM、队列与探针 | 独立进程态，不进游戏检查点；后台冷启动重新注册/start，未完成请求不恢复，G27.b 耐久/outbox 尚未实现。见[后台服务](background-services.md) |
| 模块编译缓存 | 恢复后按保存的模块字节重新安装，回到初始缓存状态 |
| 失败轮候选 | 检查点只含已提交状态；失败轮须先重试或显式丢弃才能保存 |
| 计时样本与 profiler 累计值 | 过程观测数据，不参与状态等价 |
| 地形编码缓存 | Rust 派生查询缓存，恢复后按地形重建 |
| Mod 运行 trace 缓冲 | 有界观测缓冲，非回放日志；trace 开启意愿经引擎标志跨恢复保留 |
| 测试故障钩子 | 仅测试使用的注入开关，不属于运行状态 |

## 覆盖边界与身份门现状

- **夹具覆盖**：订单表、交易、账户（含跨 shard 资源与全部标量域）、工作者（含常量覆盖与二进制模块）、Mod 身份、故障三域（含自动停用记录展开）、房间效果与扩展字段均在键树内。`fault_streak`/`last_fault`/`last_auto_disable` 是持久化链上没有 `deny_unknown_fields` 的少数结构（自动停用记录为宽松自定义反序列化，未知字段被静默忽略），其 schema 变化只能由故障场景的键树发现——解码端不会拒绝。
- **残余边界**：数组只下钻首元素，`engine.last_events[]` 为自由 `Value`，事件变体的形状只按夹具当时首位钉死；不透明叶子的内部形状不设防；取值等价仍由冷恢复往返相等测试与 sim 双通道验收负责。持久输入日志的写入与校验读取自 G25.a 落地、结算回放核对自 G25.b 落地（[输入日志](input-log.md)，日志自身独立于检查点载荷、不进版本门；回放不新增持久域）。
- **版本门**：`engine_version`（kk-runtime 包版本）与 `room_schema_version` 双硬门，未知版本拒载；信封另有 `format=kkscreeps-checkpoint` / `version=1` 与 SHA-256 完整性校验。
- **sim 附加身份**（不进载荷）：CLI 二进制 SHA-256、场景原始字节摘要、外部玩家模块清单与目录核验，防止无意换代码续跑。
- **RNG**：随机协议自 G22.b 落地（[确定性玩法时间与随机协议](gameplay-determinism.md)）：Mod 面抽值由（世界种子、流域、tick、域内序号）无状态推导，序号按结算轮重置——持久化随机状态就是世界种子本身，不新增独立域；引擎结算墙钟仍按 ADR-013 记录型外部输入处理（每轮输入由[输入日志](input-log.md)逐轮记录并支持跨进程续接；G25.b 的回放核对是 sim 进程内旗标 `--verify-replay`，独立跨进程 replay 子命令未实现）。

sim 的日志游标现为 `{last_seq,last_tick,sha256}`，由 runtime 导出末段真实链摘要，并由 `InputLogData::segments_after_cursor` 消费。它不进入引擎键树（metadata 为不透明叶子），但目录恢复端到端测试覆盖零段错历史、较旧份重放与分支后缀。Mod 建筑的已存在自定义字段在编辑、恢复、发布三处验证；缺默认键的手放记录仍允许存在，整集写回须补齐。
