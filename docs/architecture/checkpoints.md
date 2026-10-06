# 基础检查点与冷恢复

当前 `EmbedEngine` 能在无开放轮次时保存完整已提交状态，并在重启后继续运行。`sim run` 提供保存/加载入口。这是显式单文件检查点：尚无定期保存、输入日志、确定性回放、自动迁移、热更新或数据库后端。

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
| Mod 身份 | 全部发现包的 manifest 与全部文件 SHA-256，包含未选择的包；配置、内容选择、冻结选项、VM 预算及自动停用阈值 |
| Mod 执行状态 | 停用集合、连续故障计数、最近故障和自动停用决定；停用不卸载 schema 或有效规则 |
| 协调器/调用方 | 并发数、投影通道、最近 console；任意 JSON metadata 可保存 alias 等调用方状态 |

V8 堆、JS 全局变量、模块缓存、Rust 派生查询/地形编码缓存、性能样本与 profiler 累计值/句柄、测试故障钩子和失败轮候选不保存。Mod 的持久业务状态应写入 `ctx.state` 或对象字段，玩家应写入 Memory/segments。冷恢复会重新注册 Mod、重新安装玩家 API 和重建玩家线程；模块缓存回到初始状态。它不能保证依赖非持久全局变量的玩法与连续运行相同。

世界保存实际种子；新增 RNG 状态、外部输入、账户域或 outbox 时必须扩展此清单，不能仅提升格式号便宣称已有恢复能力。检查点只恢复最后保存的提交状态，不补跑已经丢失的轮次。

## API 与装配身份

- `EmbedEngine::checkpoint(metadata)` / `restore_checkpoint(value, mods)` 导出/装入管理载荷。
- `save_checkpoint(path, metadata)` / `load_checkpoint(path, mods)` 发布/读取文件。
- `kk_runtime::checkpoint::read_checkpoint(path)` 校验信封后只读导出载荷，不创建 VM，缺包时也可使用。

只允许 `Ready` 或 `Committed`，协调器另检查待重试玩家结果、执行故障及待恢复的 Mod VM；失败轮须先重试或显式丢弃，必要时重建 realm。检查点不包含输入缓存，不能用保存再加载来绕过未处置的轮次。

恢复所需 Mod 包与 `RegisterOptions` 由调用方显式提供。检查点不自动访问记录中的目录，也不携带可自行装载的 Mod 源码。缺包、额外包、manifest/源码/配置/选择/预算/规则来源变化均拒绝；相同包可搬到新绝对目录。PlanHash 由重新注册实际产生并再次核对，停用集合须满足完整依赖闭包，冻结规则和玩家常量从原计划再生。只读导出不解除这些可写启动要求。

玩家模块字节直接保存并用于重建。sim 为防止无意换代码，还核验外部玩家模块清单与目录、场景原始字节摘要及 CLI 二进制 SHA-256；修改场景、移动模块目录、切换 debug/release 或更换二进制也会拒绝续跑。修改代码、换包或改规则应走后续显式升级/迁移协议。

恢复先在替代分片装配并验证完整载荷：房名/坐标/地形、全局 ID 唯一性、房间与世界时间/种子/游标一致性、CPU 与 Memory/segments 限额、市场游标、Mod 类型/schema/位置、扩展字段 schema/引用和 namespace 身份。失败不会修改旧分片。所有检查通过后替换现行分片；同线程 Mod realm 遵守 V8 Enter/Exit 栈顺序，存在夹在旧/替代 VM 之间的其他活跃 Mod 分片或同线程玩家 VM 时拒绝切换。

## 文件格式与发布

检查点 v1 是 JSON 信封：`format = kkscreeps-checkpoint`、`version = 1`、`sha256` 和完整 JSON 载荷字符串 `payload`。SHA-256 覆盖载荷的实际 UTF-8 字节，用于发现损坏，不提供来源认证。载荷记录引擎包版本与房间 `SCHEMA_VERSION`，当前只接受相同版本；未知版本拒载，没有自动迁移。

房间使用既有顺序二进制快照，以字节数组保存；扩展字段单独保存。单文件把 manifest 与所有域放在同一完整版本中，无须从多个文件拼接房间。有限浮点采用 serde_json 的 `float_roundtrip` 精确往返，不能因保存而改变 CPU 桶或字段的数值。

输出父目录必须已存在。发布顺序：同目录唯一临时文件 `create_new` → 写完整信封 → 文件 `sync_all` → rename 替换 → 父目录 `sync_all`。替换前的错误保留旧检查点，临时文件不参与恢复；替换后的错误可能已有完整新文件，但尚未确认目录耐久性，调用方不能把这次错误当成“仍是旧版”。读取只使用指定完整文件，检查大小、信封版本、SHA-256 与业务不变式，绝不自动接受残留临时文件。

文件最大 256 MiB，超限拒绝保存/读取。这是第一版管理工具的文件限制，JSON 字节数组和整份候选会额外占用内存，尚未做大型世界恢复成本验收。发布流程针对本地支持 rename 替换与目录 fsync 的文件系统；错误注入和子进程在写完/同步/替换后直接退出证明文件版本完整性，没有实机断电或网络文件系统耐久承诺。没有跨进程写锁，同一路径须由一个宿主串行发布。

没有自动保存间隔。崩溃后的丢失窗口从最后一次确认保存的 tick 算起；只有显式检查点不能承诺零丢失。文件系统损坏时拒载，当前没有自动回退历史副本。

首次发布检查点格式后，改变持久字段、语义或房间变体布局须提升对应版本，并设计旧版读取/迁移；当前没有该迁移实现。格式版本不能替代新增数据域的接线和恢复验收。

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

实际玩家 CPU 费用、Date/Math.random 与墙钟输入本来就可能改变独立运行结果；完整续跑相等测试使用固定停跑回充轮锁定 CPU 状态，真实 bot 比较不要求各次实测 CPU 费用相等。任意 bot 的确定性回放还需要输入日志、玩家执行结果和时间/随机协议，未在本批实现。证据见[基础检查点交付](../research/notes/basic-checkpoints-2026-10-05.md)。
