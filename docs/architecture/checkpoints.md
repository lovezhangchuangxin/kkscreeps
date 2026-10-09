# 基础检查点与冷恢复

当前 `EmbedEngine` 能在无开放轮次时保存完整已提交状态，并在重启后继续运行。`sim run` 提供保存/加载入口。这是显式单文件检查点：输入日志的写入与校验读取自 G25.a 起可用（[契约](input-log.md)；写序在提交之后，检查点不得领先日志头），结算回放核对自 G25.b 起可用（[契约](input-log.md)「回放」节），周期保存与恢复选择自 G26.a 起可用（见下文[周期保存与恢复选择](#周期保存与恢复选择g26a)），热更新与数据库后端尚无；JSON 载荷域自 G23.a 起有显式迁移链（见下文迁移框架），信封与二进制身份域仍不做自动迁移。

## 已保存的数据清单

逐域字段路径、版本策略与迁移状态见[状态域清单](state-inventory.md)；本页聚焦机制与协议。

| 数据域 | 持久内容 |
|---|---|
| 世界 | time、seed、player_ticks、全局 next_obj_id、generation、terrain_epoch、完整房间集合 |
| 房间 | 地形、全部原生/Generic/Mod 对象、绝对计时字段、事件，以及房间二进制快照之外的扩展字段 |
| 玩家账户 | CPU 桶全部字段、GCL、所属房间、RawMemory、segments、活动/公开/默认/外部段请求、通知、余额和跨 shard 资源余额 |
| 市场 | 订单、交易及两类分配游标 |
| 引擎 | 自身 ID 游标、最近成功事件、每个 Mod 的已提交 namespace state、冻结 PlanHash |
| 玩家工作者 | 工作线程注册子集、JS/二进制模块字节、堆/CPU/安装预算及常量配置；账户不必均有线程 |
| 格式版本 | `checkpoint_format_version`（当前 6，必填——缺失即拒，不做缺键猜测）：JSON 载荷域布局版本，经注册迁移链升入当前版本 |
| 宿主玩法协议 | `host_protocol`，当前 UTC Date/RNG/SDK 语义身份，恢复严格核验 |
| Mod 身份 | 全部发现包的 manifest 与全部文件 SHA-256，包含未选择的包；配置、内容选择、冻结选项、VM 预算及自动停用阈值 |
| 执行策略身份 | 组合报告的域分隔摘要：冻结 PlanHash + 停用闭包 + 每槽派生执行面（键/阶段/模式/活动链）；引用 PlanHash 而非复制，诊断字段不入摘要 |
| Mod 执行状态 | 停用集合、连续故障计数、最近故障和自动停用决定；停用不卸载 schema 或有效规则 |
| 协调器/调用方 | 并发数、投影通道、最近 console；任意 JSON metadata 可保存 alias 等调用方状态 |

V8 堆、JS 全局变量、模块缓存、Rust 派生查询/地形编码缓存、性能样本与 profiler 累计值/句柄、测试故障钩子和失败轮候选不保存。Mod 的持久业务状态应写入 `ctx.state` 或对象字段，玩家应写入 Memory/segments。冷恢复会重新注册 Mod、重新安装玩家 API 和重建玩家线程；模块缓存回到初始状态。它不能保证依赖非持久全局变量的玩法与连续运行相同。

世界保存实际种子；Mod 面随机流自 G22.b 起由种子无状态推导（[确定性玩法时间与随机协议](gameplay-determinism.md)），不新增独立 RNG 持久域；新增外部输入、账户域或 outbox 时必须扩展此清单，不能仅提升格式号便宣称已有恢复能力。检查点只恢复最后保存的提交状态，不补跑已经丢失的轮次。

## API 与装配身份

- `EmbedEngine::checkpoint(metadata)` / `restore_checkpoint(value, mods)` 导出/装入管理载荷。
- `save_checkpoint(path, metadata)` / `load_checkpoint(path, mods)` 发布/读取文件。
- `kk_runtime::checkpoint::read_checkpoint(path)` 校验信封后只读导出载荷，不创建 VM，缺包时也可使用。

只允许 `Ready` 或 `Committed`，协调器另检查待重试玩家结果、执行故障及待恢复的 Mod VM；失败轮须先重试或显式丢弃，必要时重建 realm。检查点不包含输入缓存，不能用保存再加载来绕过未处置的轮次。

恢复所需 Mod 包与 `RegisterOptions` 由调用方显式提供。检查点不自动访问记录中的目录，也不携带可自行装载的 Mod 源码。缺包、额外包、manifest/源码/配置/选择/预算/规则来源变化均拒绝；相同包可搬到新绝对目录。PlanHash 由重新注册实际产生并再次核对，停用集合须满足完整依赖闭包，冻结规则和玩家常量从原计划再生。只读导出不解除这些可写启动要求。

玩家模块字节直接保存并用于重建。sim 为防止无意换代码，还核验外部玩家模块清单与目录、场景原始字节摘要及 CLI 二进制 SHA-256；修改场景、移动模块目录、切换 debug/release 或更换二进制也会拒绝续跑。修改代码、换包或改规则应走后续显式升级/迁移协议。

**版本职权分五项**：信封 `version`（线格式：结构、sha256 机制，严格等值）；`checkpoint_format_version`（JSON 载荷域布局，必填，经迁移链升入）；`engine_version`/`room_schema_version`（发行版本与房间 schema 门；不是源码摘要）；`host_protocol`（SDK 时间/RNG/候选语义身份，严格等值）。

**JSON 载荷迁移框架**：迁移在 `deny_unknown_fields` 解码**之前**对原始 JSON 执行——链上第 n 项（0 起算）把版本 n+1 升到 n+2，从 `raw - 1` 项起依次执行到当前版本，版本号在链执行完毕后统一改写；版本 0、未来版本、缺失或非法版本值拒绝；任一步失败整体拒绝（旧分片由替代分片架构保证完整，不自动修补不一致载荷）。已注册步骤：v1→v2 归一 `last_auto_disable` 旧形态（来源从故障绑定命名空间派生，mod_id 与来源不一致即畸形）。v2→v3 为缺协议身份载荷补旧语义标记，宿主协议门明确拒绝恢复旧时区历史。v3→v4 引入必填房间 effects 表，缺该表的载荷明确拒绝，不补造空表。携带当前协议身份与效果域的显式 v1 演练载荷按 legacy 语义宽容执行策略身份缺失（v1 先于该键存在）；**v2 起 Mod 检查点必须携带该键**。

冷恢复的身份门（ModIdentity/plan_hash/执行策略等值）要求新旧包集逐字节一致；运行中**更换**包集合不走检查点往返，走[无开放轮次的包切换](mod-switching.md)（替代分片模式换装执行层，切换后保存的检查点自然携带新身份）。

恢复核对链按序执行：载荷迁移与格式版本路由 → 版本双硬门 → Mod 身份（包/配置）→ 原生检查点不得含 Mod 执行状态 → 重装配与停用重放（闭包等值）→ **执行策略身份等值**（重装配产物重算摘要与保存值比对，见 [组合与停用](mod-composition.md)）→ 引擎检查点（engine.plan_hash 等值）→ 故障三域回填。`ModVm::recover` 的 PlanHash/可执行绑定/策略身份三重核对属**故障重建**路径（健康 VM 冷恢复时短路），两条路径汇入同一组合派生函数。

恢复先在替代分片装配并验证完整载荷：房名/坐标/地形、全局 ID 唯一性、房间与世界时间/种子/游标一致性、CPU 与 Memory/segments 限额、市场游标、Mod 类型/schema/位置、扩展字段 schema/引用和 namespace 身份。失败不会修改旧分片。所有检查通过后替换现行分片；同线程 Mod realm 遵守 V8 Enter/Exit 栈顺序，存在夹在旧/替代 VM 之间的其他活跃 Mod 分片或同线程玩家 VM 时拒绝切换。

## 文件格式与发布

检查点 v1 是 JSON 信封：`format = kkscreeps-checkpoint`、`version = 1`、`sha256` 和完整 JSON 载荷字符串 `payload`。SHA-256 覆盖载荷的实际 UTF-8 字节，用于发现损坏，不提供来源认证。载荷记录引擎包版本与房间 `SCHEMA_VERSION`，当前只接受相同版本；未知版本拒载，没有自动迁移。

房间使用顺序二进制快照（当前 schema v2），以字节数组保存；v2 将 storage 权威容量纳入对象载荷，v1 无法无损还原截止拍容量窗口，显式拒绝，不自动补造。扩展字段单独保存。单文件把 manifest 与所有域放在同一完整版本中，无须从多个文件拼接房间。有限浮点采用 serde_json 的 `float_roundtrip` 精确往返，不能因保存而改变 CPU 桶或字段的数值。

输出父目录必须已存在。发布顺序：同目录唯一临时文件 `create_new` → 写完整信封 → 文件 `sync_all` → rename 替换 → 父目录 `sync_all`。替换前的错误保留旧检查点，临时文件不参与恢复；替换后的错误可能已有完整新文件，但尚未确认目录耐久性，调用方不能把这次错误当成“仍是旧版”。读取只使用指定完整文件，检查大小、信封版本、SHA-256 与业务不变式，绝不自动接受残留临时文件。

文件最大 256 MiB，超限拒绝保存/读取。这是第一版管理工具的文件限制，JSON 字节数组和整份候选会额外占用内存，尚未做大型世界恢复成本验收。发布流程针对本地支持 rename 替换与目录 fsync 的文件系统；错误注入和子进程在写完/同步/替换后直接退出证明文件版本完整性，没有实机断电或网络文件系统耐久承诺。没有跨进程写锁，同一路径须由一个宿主串行发布。

没有自动保存间隔——周期保存由 `sim run --checkpoint-every` 提供（G26.a，见[周期保存与恢复选择](#周期保存与恢复选择g26a)）：目录内逐份原子发布、按 tick 降序「最新可用」恢复，坏文件跳次新。仅显式检查点不能承诺零丢失；周期检查点 + 输入日志恢复的丢失窗口见 RPO 语义。

## 周期保存与恢复选择（G26.a）

- **保存**：`sim run --checkpoint-every N --checkpoint-dir DIR`。保存点在每轮成功提交且无失败玩家之后（按提交后世界时间为 N 整倍数对齐——非整倍数边界恢复续跑时首轮即可能保存）——已提交边界、日志段已写、无开放轮次三条件同时成立，「检查点不得领先日志头」天然满足。文件名 `ckpt-{tick:010}.json`（tick 定宽=恢复优先序），逐份独立走既有原子发布（临时文件→fsync→rename→父目录 fsync）；`--checkpoint-keep K` 保留最新 K 份（缺省全部；清点与删除失败都不致命，仅记入报告 `delete_errors`）。保存 I/O 失败即运行失败（`checkpoint_failed`、退出 1、旧版不被替换）——耐久承诺未达成不静默继续。周期保存不摘除运行期日志。
- **元数据**：sim 元数据新增 `log_cursor{last_seq,last_tick,sha256}`（保存时的日志游标，未挂日志为 null）；`applied_switches` 在周期保存点恒为已生效子集（mods_switch 场景与周期保存互斥拒绝——切换摘除日志后检查点无日志覆盖，组合恢复属后续切片）。
- **恢复**：`--load-checkpoint` 指向目录时按 tick 降序选「最新可用」：逐份信封+SHA 校验（坏文件跳次新并计数），与 `--input-log`（必需）通过 runtime 哈希游标配对——游标 seq/哈希必须命中整条日志链，游标 last_tick+1 必须等于检查点时间；只重放该 seq 后的连续后缀，后缀首 tick 必须等于检查点时间。零后缀仍须核验游标历史，是合法终态（保存后即崩的形态）。配对失败（缺口/接缝/历史哈希不匹配/日志不覆盖游标）是日志与检查点的关系问题，硬错误不降级；撕裂日志在读取处失败并携带前缀段数，不自动截断文件（人工修复后重试）。只有规范命名（`ckpt-{tick:010}.json`）的文件参与选择与保留清理——手工放置的其他文件被忽略，既不删除也不计入坏份。报告 v10 顶层 `recovery` 记录所选文件/检查点时间/保存时游标/重放段数/跳过坏份数（目录形态启用时含 setup_failed 路径非 null），`auto_checkpoint` 记录间隔/份数/保留策略/保存耗时样本与 RPO 声明。注意差一：`auto_checkpoint.last_tick` 是保存时刻世界时间，`recovery.log_cursor.last_tick` 是所选份对应日志末段的 tick（= 前者 −1）。
- **守卫**：周期检查点目录与其他所有输入/输出两两互斥且不得互相包含（含 `--load-checkpoint` 指向同一目录的「恢复+继续保存」组合——恢复与续存同目录属后续切片的放宽项），目录不得位于 Mod 包根内——目录内文件由自动保留策略管理。

首次发布检查点格式后，改变持久字段、语义或房间变体布局须提升对应版本，并设计旧版读取/迁移：JSON 载荷域（`checkpoint_format_version`）的显式迁移链自 G23.a 起已落地（见上文迁移框架），房间二进制快照变体布局尚无迁移实现。格式版本不能替代新增数据域的接线和恢复验收。

## sim 保存与续跑

在仓库根目录构建 release CLI，准备已存在的输出目录：

```bash
cargo build --release -p kk-cli
mkdir -p /tmp/kkscreeps-checkpoints
target/release/kkscreeps sim run \
  --scenario compat/fixtures/sim-mod-objects/scenario.json \
  --ticks 20 --report /tmp/kkscreeps-checkpoints/first.json \
  --save-checkpoint /tmp/kkscreeps-checkpoints/world.json

target/release/kkscreeps sim run \
  --scenario compat/fixtures/sim-mod-objects/scenario.json \
  --ticks 80 --report /tmp/kkscreeps-checkpoints/resumed.json \
  --load-checkpoint /tmp/kkscreeps-checkpoints/world.json
```

`--ticks` 在续跑时表示额外推进的轮数，报告的 `world_ticks_committed` 也只计本次新增轮数；`start_time/end_time` 记录绝对时间。原始 `initial`、alias 和断言起点一并恢复，`eq/gte` 检查续跑最终值，`delta_gte` 比较首次创建时的初始值。续跑不会重新放置对象或重置 Memory。并发数和 JSON/KKPV 通道可由此次 CLI 参数重新选择。

可同时传 `--load-checkpoint` 与 `--save-checkpoint`，包括同一路径。运行全部轮次且没有玩家/运行故障后保存；最终断言未满足仍可保存合法已提交状态，CLI 仍返回 2。中间轮数的原场景断言可能尚未满足，不能把这次退出码 2 当作检查点写入失败。报告的 `checkpoint_saved` 记录确认保存的路径。

检查点输出不能覆盖场景、报告、玩家或 Mod 源码目录；报告不能覆盖加载的检查点。保存 I/O 失败返回 1，报告 `checkpoint_failed` 并保留实际已提交轮数和最终状态；恢复/身份校验失败在运行前返回 1。`checkpoint_saved = null` 表示没有确认发布，替换后同步失败须结合错误判断磁盘上的完整版本。

## 验证边界

验收覆盖两房两玩家、多个独立 JS 内容包、自主生成/删除对象、内容选择、停用掩码、有效规则、命名资源、私有扩展、Memory/segments/通知、市场和分配游标。完整清单在磁盘 JSON 往返后逐项相等；真实 CLI 子进程退出再恢复的玩法状态与连续运行参考一致，两种投影通道均验证。

实际玩家 CPU 费用、玩家面 Date/Math.random 与墙钟输入本来就可能改变独立运行结果；完整续跑相等测试使用固定停跑回充轮锁定 CPU 状态，真实 bot 比较不要求各次实测 CPU 费用相等。玩法 realm（Mod 规则代码）的时间/随机自 G22.b 起确定性化（[协议](gameplay-determinism.md)）；任意 bot 的确定性回放需要输入日志与玩家执行结果——输入日志写入面自 G25.a 起交付、结算回放核对自 G25.b 起交付（[契约](input-log.md)「回放」节，`sim run --verify-replay`）。证据见[基础检查点交付](../research/notes/basic-checkpoints-2026-10-05.md)。

加载检查点目录作为输入整棵受保护：报告、显式保存和日志路径（含符号链接解析后的路径）不得落入其中，守卫先于执行及失败报告写入。重复冷恢复继承 `applied_switches` 并仅追加新成功切换；会话报告只展示当次切换。缺少带哈希游标的旧目录检查点拒绝自动配对，不凭 tick 猜测历史。

普通 Invader 移动记忆自载荷 v6 保存于每房 `invader_memory`，可空缓存与 last_move 必填；v5_to_v6 对缺域房间显式拒绝。该域参与完整状态回放、候选发布、对象删除与恢复，校验及消费者见[游戏循环](game-loop.md#普通-invader-行为当前实现)和[状态清单](state-inventory.md)。

Invader raid 房间状态自载荷 v7 保存于每房 `raid`（阈值覆盖/采集预算/活跃/房态四键必填）；v6_to_v7 对缺域房间显式拒绝（不补造——活跃房静默变为可生成是不可接受的丢失）。生成器消费者与随机流约定见[游戏循环](game-loop.md#invader-raid-调度当前首片)。

powerBank 生成调度自载荷 v8 保存于每房 `power_bank_time`（Option<u64>，null 或到期 tick）；v7_to_v8 对缺键房间显式拒绝（不补造——已调度房静默回到首轮形态是不可接受的丢失）。生成器与消费者见[游戏循环](game-loop.md#powerbank-生成与消费者当前首片)。

房间公共状态种子自载荷 v9 保存于每房 `room_status`（Option<{status,timestamp}>，null 为 normal；novice/respawn/closed + 墙钟毫秒时间戳）；v8_to_v9 对缺键房间显式拒绝（种子房静默回到 normal 是不可接受的丢失）。玩家面 getRoomStatus、launchNuke 房间状态门与传送机制见[游戏循环](game-loop.md#portal房间状态与世界边缘当前首片)。
