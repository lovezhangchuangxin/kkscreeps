# 本地场景模拟

`kkscreeps sim run` 使用 `kk-runtime::EmbedEngine` 加载场景、运行玩家模块和推进真实结算。每轮所有玩家读取同一已提交视图，结果经现有协调器提交。当前实现 JSON v1 场景、默认玩法和显式 Mod 包加载；已提供显式[检查点保存与冷恢复](checkpoints.md)与 sim 级周期保存/目录恢复（见[检查点](checkpoints.md)「周期保存与恢复选择」）；热更新、观战和内容预设尚未实现；场景 seed 字段自 G22.b 起可用（见上表）。

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run \
  --scenario compat/fixtures/sim-economy/scenario.json \
  --ticks 2000 --report /tmp/sim-economy.json
```

`--ticks` 默认 1000，必须大于零；`--concurrency` 默认 2，可选 1–32；`--kkpv` 选择二进制玩家投影，默认 JSON；`--mod-trace` 在报告附有界的 Mod 运行事件（槽结果、拒绝、故障与回退；默认关闭，见[运行 trace](mod-trace.md)）；`--input-log` 每轮成功提交后向指定文件追加一段可回放输入（默认关闭，完整契约见[输入日志](input-log.md)——运行期被切换/恢复摘除时报告如实标注 `detached`，损坏日志拒绝续接）；`--verify-replay` 运行成功后从起点快照按日志逐段重放并核对终态（需 `--input-log`，报告顶层 `verify_replay`）；`--checkpoint-every N --checkpoint-dir DIR` 在提交后世界时间为 N 整倍数的轮次保存完整检查点（按世界时间对齐，非整倍数边界恢复后首轮即可能保存）（`--checkpoint-keep K` 保留最新 K 份，报告顶层 `auto_checkpoint` 含 RPO 声明），`--load-checkpoint` 指向目录时按「最新可用」恢复并重放日志段到尾（报告 `recovery`）。路径相对当前目录定位场景，场景里的模块目录相对场景所在目录定位。报告必须使用 `.json` 后缀，且不能覆盖场景或装载检查点；报告、保存检查点与输入日志均须在场景声明的玩家模块与 Mod 包根之外，且输入日志不得与报告、场景或检查点同路径（原子替换写会互相覆盖）。守卫在装载/执行和失败报告写入之前运行，逐组件解析符号链接，拒绝后不创建输出目录。`--save-checkpoint` 保存无开放轮次的完整状态，`--load-checkpoint` 从同一场景/模块身份恢复，`--ticks` 表示额外推进轮数；父目录须已存在，完整契约与示例见[检查点](checkpoints.md)。

## 作者包模板

`kkscreeps mod init --root <目录> --id <包ID>` 生成一个可独立运行的最小公共 SDK 包（落在 `<目录>/<包ID>/`）：manifest、声明每 tick 自增计数对象的服务端处理器、绑定玩家类与 FIND 查询的玩家面、玩家脚本、引用本包并带断言的场景，以及包含组合检查、运行与保存恢复命令的 README。目标目录已存在时拒绝，通过原子创建占用包目录，不覆盖现有内容；`builtin` 是保留命名空间，不能用作模板 ID。玩家类及查询常量采用无损 ID 转义：下划线→双下划线、连字符→`_H`，因此 `foo-bar` 与 `foo_bar` 等不同 ID 组合时不会撞名。此规则只应用于新模板，已有包不自动改名。

```bash
target/release/kkscreeps mod init --root /tmp/my-mods --id starter
target/release/kkscreeps sim run --scenario /tmp/my-mods/starter/scenario.json --ticks 3 --report /tmp/starter.json
```

模板场景断言 3 轮后的对象计数值、玩家读取值与 namespace state；`--save-checkpoint`/`--load-checkpoint` 续跑等价，作为作者的保存恢复示例。生成物只依赖公共 SDK 与 CLI，宿主没有按包 ID 分派的分支。

`kkscreeps mod test --root <目录> --report <汇总.json>` 对根下每个自带 `scenario.json` 的包按同一 `sim run` 路径运行断言并汇总：无场景的包记 skipped；`--ticks` 默认 3（匹配模板断言），`--kkpv`/`--mod-trace` 与 `sim run` 同义。退出码沿用 sim 语义（0 全过、1 场景/装载失败、2 运行或断言失败），多包失败取最严重退出码；汇总报告内联各场景的机器结果（含 plan_hash 与断言状态），装载期失败也落盘 `setup_failed` 汇总。汇总报告必须在包根之外，守卫拒绝时不写失败报告；各场景中间报告使用原子占用的私有临时目录，读取失败不会被计为通过，结束后清理。

`kkscreeps mod types --root <目录> --out <声明.d.ts> --report <报告.json>` 从冻结计划生成玩家面 TypeScript 声明（安装 API 窄化签名、对象投影 interface、意图参数与资源联合），`--select`/`--exclude`/`--configs` 与 sim/inspect 共用展开语义；`--check` 与磁盘声明按字节对比校验新鲜（过期或缺失退出 2，只读不改文件）。输出与报告都必须在包根之外（`.d.ts`/`.json` 后缀）——包内文件字节计入计划指纹，被守卫拒绝的路径不落任何文件。完整契约见[玩家类型生成](mod-types.md)。

## 场景输入

完整示例见[经济夹具](../../compat/fixtures/sim-economy/scenario.json)。场景的顶层、shard、bot、对象声明和断言声明拒绝未知字段；房间和对象 spec 复用已有嵌入接口的字段校验及默认值。

| 字段 | 含义 |
|---|---|
| `version` | 当前仅接受整数 1 |
| `seed` | 可省略（默认 0）；世界种子——玩法随机流与确定性时间的派生根（[协议](gameplay-determinism.md)），同种子同输入同抽值 |
| `shard.players` | 非空且 handle 唯一的玩家列表，支持字符串或现有 PlayerSpec 对象 |
| `shard.rooms` | 现有 RoomSpec 列表；包括 name、owner、rcl、地形等字段 |
| `bots` | 每个玩家必须恰有一个声明：player、modules、entry（默认 main）、memory（默认空对象） |
| `mods` | 可省略；root 为 Mod 根目录，configs 为按包 ID 分组的可选配置 |
| `mods_switch` | 可省略；提交完第 `after_tick` 轮后在同一 root 上整体替换 configs/selection，见[包切换](mod-switching.md) |
| `objects` | 可省略；按顺序放置 alias、room、kind、spec，alias 必须非空且唯一 |
| `assertions` | 可省略；最终状态的 JSON Pointer、op 和 value |

模块加载沿用[玩家沙箱](sandbox.md)的模块规则，入口必须是 JS 模块并导出 `loop` 函数；每轮调用一次，返回值不作为断言。此入口不等待 Promise。模块顶层代码在首轮执行，而非场景初始化时执行。

对象 alias 用于宿主报告和断言，不增加玩家可见的命名 API。初始化使用真实引擎放置接口，不运行隐藏 tick；初始 Memory 必须是对象，并遵守 RawMemory 的 UTF-16 长度限制。模块或 Memory 不能在待重试轮次中被替换。控制器初始位置沿用现有 RoomSpec 的 `(1,1)`。

### 显式加载 Mod

`mods.root` 必须为非空路径，相对场景目录解析，也可使用绝对路径；装载其直接子目录的所有包。包按 manifest 依赖拓扑注册，缺包、版本不符、冲突或注册/装配失败均在运行前拒绝。`mods` 拒绝未知字段，`configs` 的键必须是已加载的包 ID；配置值的业务校验由包负责。省略 mods 保留原生默认创建路径，不加载官方或第三方 JS。`mods.selection` 支持显式 include/exclude 与 manifest 依赖展开，见[内容选择](mod-content-selection.md)；没有依赖自动下载。

自有初始对象使用 `kind: "mod"`，spec 严格包含 `typeKey` 和 `fields`，例如 `{"typeKey":"clockwork:counter","fields":{"x":10,"y":12}}`。类型必须在已装配计划中，字段和默认值沿同一 schema 校验。声明了 tick 的对象从第一轮结算开始执行；玩家首轮看到初始化值。完整示例见[官方生命周期场景](../../compat/fixtures/sim-mod-tick/scenario.json)及[对象 tick 契约](mod-object-tick.md)。

### 中途切换包集合

`mods_switch` 声明一次[无开放轮次的包切换](mod-switching.md)：提交完第 `after_tick` 轮后，在同一 `mods.root` 上按本字段（整体替换初始 configs/selection，不是合并）重新装载并换装执行层。切换拒绝（残留、同包内容变更、开放轮次或切换装载本身的失败）不使运行失败——旧计划继续结算，报告 v6 顶层 `mods_switches` 记录每次切换：`applied` 含前后 plan_hash、执行策略身份与增删包清单，`rejected` 含错误。`--load-checkpoint` 续跑按检查点世界时间**与实际生效的切换**（保存进检查点元数据的 `applied_switches`）选择恢复用包集合——被拒绝的切换不改变身份，越过边界不等于携带切换后集合；续跑不重放已生效的切换，时间已越过边界的被拒切换也不再重试。报告顶层 `mods` 始终记录**初始装载输入**；切换后的实际身份以 `composition`/`execution_strategy` 顶层域为准。完整示例见[切换夹具](../../compat/fixtures/sim-mod-switch/)。

## 状态与断言

初始和最终观察都读取已提交状态，包含 `time`、`players`、`objects`、`rooms`、`world`、`mod_objects`、`mod_states`。常用指针如下：

| 指针示例 | 读取内容 |
|---|---|
| `/players/bot/gcl` | 实际玩家 GCL 累计点数，包含等级所需的累计基数 |
| `/players/bot/memory/loops` | 解析后的玩家 Memory；非法 JSON 时为 null |
| `/objects/container/store/energy` | alias 对应对象当前能量 |
| `/rooms/W1N1/controller/progress` | 房间控制器进度 |
| `/objects/first/fields/value` | Mod 对象声明字段 |
| `/mod_objects/0/fields/age` | 全部 Mod 对象的管理快照，包括自主生成且无 alias 的对象 |
| `/mod_states/clockwork/visits` | 包命名空间已提交 state；未初始化为 null |

alias 或玩家名中的 `/`、`~` 分别转义为 `~1`、`~0`。对象死亡后 alias 对应 null，`missing` 同时接受缺失指针和 null；`exists` 要求值存在且非 null。

`eq` 比较最终 JSON 值，缺失指针不能与 null 相等；`gte` 检查最终数值下限；`delta_gte` 检查最终值减初始值的下限。整数比较和差值保持精度；混合浮点比较拒绝把超出 JS 安全整数范围的整数转为浮点。差值超出 JSON 的 64 位整数范围时，以十进制字符串报告，判断仍使用精确整数。

没有断言的场景仅检查执行成功和完成轮数。经济夹具至少运行 500 tick，要求实际升级、孵化替补及旧 creep 死亡，不能通过少运行几轮获得同等验收结论。

## 报告与失败

报告记录场景字节、模块内容及 CLI 二进制 SHA-256、模块目录与清单、投影通道、并发配置、请求/尝试/已提交轮数、失败轮次、各玩家计数、断言及初始/最终状态。控制台和错误各保留前 20 条。当前运行时控制台仅提供文本，捕获异常后打印日志不会自动判为执行失败；开始执行后的 CPU 硬中断按现有协议记录为 runtime 错误，未进入求值的轮次另计。

Mod 场景另记录实际加载包的目录、版本、SDK 要求、依赖、服务端代码/玩家入口/配置 SHA-256，以及引擎实际安装的 `plan_hash` 和 `disabled_mods`，顶层 composition 保存[冻结/活动组合诊断](mod-composition.md)，`execution_strategy` 保存[执行策略身份](mod-composition.md#执行策略身份)摘要。无 JS Mod VM 时两者为 null。未声明 mods 时包清单为空，仍记录原生计划身份。停用集合单独记录，因为 PlanHash 不包含执行掩码；报告本身仍是诊断数据；`--save-checkpoint` 另存完整恢复载荷，报告不能替代它。输入回放面：`--input-log` 自 G25.a 起记录每轮提交的可回放输入（报告顶层 `input_log` 摘要），`--verify-replay` 自 G25.b 起做结算重放核对（不重跑玩家沙箱）。

`players` 中的 GCL 累计点数、Memory 和 CPU 桶来自实际 PlayerState；GCL 不是玩家 `Game.gcl` 的等级/进度对象。`world` 是宿主诊断投影，其公共 gcl/cpu 字段不是玩家状态。`world` 沿原生 JSON 诊断形态，不包含 Mod 对象；`mod_objects` 独立列出按房间名、ObjId 排序的全部已提交 Mod 对象及 server-only 字段，不能作为玩家可见性接口。报告不保存完整运行状态，也不能作为检查点恢复。

玩家异常或硬中断使场景失败，但当前轮次仍可能提交合法指令和 Memory，报告必须保留实际已提交状态。基础设施或结算错误停止在最后已提交轮次；此工具不自动重试。Mod 调用故障另保留 mod_fault（tick、room、target 和宿主结构化错误，包括槽/绑定/阶段与故障种类）；其他故障为 null。失败轮重试契约见[tick 契约](tick-contract.md)。

| 退出码 | 含义 |
|---|---|
| 0 | 完成全部轮次、玩家执行成功且所有断言通过 |
| 2 | 玩家/运行失败或断言失败；报告保留失败详情与已提交状态 |
| 1 | 场景、模块、初始化、检查点或报告 I/O 失败；运行前失败为 setup_failed，保存失败为 checkpoint_failed 并保留实际提交状态 |

CLI 参数解析失败沿用 Clap 的退出码 2，不写报告。报告通过同目录临时文件写入后重命名，配置失败也会替换旧报告，避免误读上次成功结果；非法报告路径直接拒绝。该操作不是完整世界持久化协议。

`run_wall_ms` 仅包含推进循环，不含加载、指纹计算、最终状态观察和报告写入。`phases` 使用运行时已有的累计阶段计时，是诊断数据，不能直接作为正式跨实现基准。玩家 Date、Math.random、CPU 查询保持生产行为，因此固定场景和源码不保证任意 bot 完全确定；Mod 规则代码的时间/随机则自 G22.b 起确定性（[协议](gameplay-determinism.md)）。复现要求见[开发套件设计](../designs/dev-kit.md)。

原生场景验收见[初始交付记录](../research/notes/sim-run-2026-10-04.md)，Mod 场景验收见[生命周期交付记录](../research/notes/n1a-object-tick-2026-10-05.md)和[对象操作交付记录](../research/notes/n1b-mod-objects-2026-10-05.md)。
