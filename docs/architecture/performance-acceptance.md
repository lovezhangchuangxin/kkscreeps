# 端到端性能验收

本文定义端到端对照的测量方法和验收目标。`perf-probe` 提供本项目固定负载的阶段耗时，`scripts/benchmark/compare.py` 提供本项目与本地 xxscreeps 的共同负载对照。官方私服尚未接入，完整三方场景矩阵仍未完成。局部对照、微基准和性能预估不能作为整体达标结论。

## 当前工具与测量边界

在仓库根构建 release 二进制后运行：

```bash
cargo build --release -p kk-cli
target/release/kkscreeps perf-probe --players 2 --warmup 100 --ticks 1000 --report /tmp/kkscreeps-perf.json
```

当前工具接受 1..32 名玩家，每玩家独立房间，使用采集、升级和孵化脚本。超出范围报错，不静默改变请求的负载。它不覆盖共享房间竞争、跨房调度、数据库写盘或网络推送。

`EmbedEngine` 记录的都是墙钟时间，单位毫秒：

| 字段 | 现有采样范围 |
|---|---|
| `project` | 玩家世界 JSON 与市场投影构造 |
| `run` | 派发与回收窗口扣除宿主投影时间 |
| `settle` | 玩家候选生成、世界结算与统一发布 |
| `wall` | 从 run_players 开始到玩家状态提交后的采样点，不含最后的返回结果装配 |

P1 的 `run` 是整个派发与回收窗口扣除宿主投影时间，包含调度和回执检查，不是各玩家 CPU 用量之和；宿主为后续玩家投影时，已派发玩家可以同时执行。`settle` 包含玩家候选生成与统一发布。四项不能简单相加解释为完整时间分解。重试复用缓存时不重新投影和执行，其对应采样为 0；采样只在完成结算后记录。报告提供 count、p50、p95、p99、最大值和总耗时，分位数按排序样本的 `round((N−1)×q)` 下标取值。

macOS/Linux 上，报告的 `process` 部分另测采集窗口内当前进程全部线程的 CPU core-ms，以及采集前和每 tick 结束时的 RSS。CPU 使用 [getrusage(RUSAGE_SELF)](https://www.man7.org/linux/man-pages/man2/getrusage.2.html)，包括协调器、玩家线程和后台线程；它与玩家 `Game.cpu` 账单分列，不包含其他进程。RSS 使用 macOS 的进程 resident size 或 Linux `/proc/self/statm`，单位为字节。`rss_bytes_sampled_peak` 是采样点最大值，可能漏掉 tick 内瞬时峰值；p50 只保留前 50000 个观测点，最大值仍覆盖全部观测点。

进程窗口在预热与初始负载检查后开始，在采集循环后结束；包括窗口内的负载检查与 RSS 采样开销，报告单列 `rss_sampling_wall_ms`。阶段计时不包含这些循环外部操作，不能将两种口径直接相减作为某一业务阶段耗时。元数据记录运行系统、架构、逻辑 CPU 数、运行环境中的 rustc、源码提交及工作区状态；`debug_assertions` 用于识别 debug 构建。尚未采集 GC、复制字节数或独立网络后台工作。

独立重复运行工具见 [perf-baseline.py](../../scripts/perf-baseline.py)：默认按 1/4/8/32 玩家交替顺序，各运行五次，预热 500 tick、采集 3000 tick，并保存全部原始报告和二进制 SHA-256。此结果只作为本仓基线，不代替三方对照。

预热后清空样本，最多保留 50000 个采集样本；其后的 tick 继续运行但不加入统计。工具只在首房检查控制器进度增加，并在足够长的窗口中检查采集迹象，不能据此保证每房全部动作正确。长窗口会出现 creep 死亡、重新孵化和墓碑衰减，工作负载并非始终不变。

[早期采集记录](../../compat/reports/perf-a6.md)包含空转负载的历史结果，不作为有效工作量的性能基线。当前负载自检降低空转风险，正式对照仍须逐实现核实动作和关键状态。

## 玩家与 Mod 连续闭环采集

`mod-player-probe` 使用入库的 [workload 包](../../compat/fixtures/mod-player/workload/mod.json)，每玩家独立房间，放置一个 Mod counter、一个 source 和一个 worker。玩家每轮通过 Mod 类与 FIND/LOOK 查询 counter、发出递增指令，并按 worker 能量循环采集和升级；同时写回 Memory 执行序号。worker 的合成寿命延长至窗口之后，整个有限窗口保留同一对象，不覆盖死亡与孵化。

```sh
target/release/kkscreeps mod-player-probe --players 2 --concurrency 2 --warmup 500 --ticks 3000 --report /tmp/mod-player-json.json
target/release/kkscreeps mod-player-probe --players 2 --concurrency 2 --warmup 500 --ticks 3000 --kkpv --report /tmp/mod-player-kkpv.json
```

玩家数为 1..320（房间按 20 列网格分布避免世界越界），每玩家房数 1..8 且总房数 ≤320，并发数为 1..32，采集窗口为 1..50000 tick，预热为 0..50000。多房负载下每间自有房各放置一套 counter/source/worker 状态机，逐房核验。报告含里程碑 RSS（建分片前/后、放置后、热身后）与每玩家边际足迹，供满员容量评估（基线证据见 [capacity-isolate-probe](../../compat/reports/capacity-isolate-probe/summary.json)）。默认玩家及 Mod 预算不变（玩家 V8 堆上限随 `kk_runner::DEFAULT_HEAP_LIMIT`，报告 player_budget 记录），报告记录两者配置、包和玩家脚本 SHA-256。每轮核验全部玩家实际执行且无异常、查询与动作返回值、VM/Memory 序号、全部 counter 的提交值、worker 原生动作后的能量、控制器累计进度、source 消耗与再生、namespace 累计次数以及世界 tick/generation；任何不符使命令失败。固定 RCL 4 与单 WORK 在最多 100000 轮内不达到升级阈值，控制器进度应等于累计升级次数。短窗口允许未进入采集分支，报告准确列出采集窗口内每玩家的升级与采集次数，不将预热动作计入。

当前报告为 v2，新增原生动作目标的逐轮验证；进程 CPU 成本不能与仅检查 worker 能量的 v1 窗口混合汇总。采集脚本的门禁始终执行，Python 优化模式也不禁用；校验完整阶段集合、指标类型与采样数、边界动作和对象效果，再比较各窗口身份与完整快照。归档逐次采用同目录原子替换，后续失败保留已经验收的窗口；复审修复与 v2 原始采集见[复审记录](../research/notes/mod-player-review-2026-10-04.md)。

报告保留初始、预热后、采集后三个边界的全部房间对象状态和逐玩家动作次数。阶段计时沿用 `EmbedEngine`，清除预热样本；进程 CPU/RSS 窗口额外包含每轮状态检查与采样开销。这是有实际玩家投影和原生/Mod 指令的合成负载，仍不覆盖共享房竞争、真实 bot、持久化或网络，也不能和纯结算基准直接相减归因。连续故障恢复另由 runtime 集成测试验证，不在性能窗口注入故障。

首次连续运行及通道状态等价性见[闭环采集记录](../research/notes/mod-player-workload-2026-10-04.md)。桌面上的重复窗口只提供本机基线，不构成完整架构达标或通道加速结论。

## 密集 Mod 纯结算基准

`mod-perf-probe` 用同一合成 link 世界比较三个模式：`native` 没有玩法 VM；`empty` 注册空包但没有处理器；`chain` 为每个 link 安装 before、两层 wrap、native 中心与 after。链模式额外维护四个 namespace 计数，用于核实各链位每对象每 tick 恰好执行一次，不能把差值解释为纯 JS 调用成本。

```bash
target/release/kkscreeps mod-perf-probe --mode chain --rooms 1 --links-per-room 400 --warmup 500 --ticks 3000 --report /tmp/mod-perf.json
python3 scripts/mod-perf-baseline.py --output /tmp/mod-perf-baseline.json
```

房间数为 1..4、每房 link 为 1..400。每个 link 的初始冷却等于预热与采集 tick 数之和，因此整个窗口每 tick 都有一次 native 字段写入。此初值是合成负载，不模拟正常冷却上限。启动与三个边界的验证不计入采集窗口；工具验证全部 link 的完整 JSON 快照、世界 tick/generation 和 namespace 四个计数，报告 link 状态 SHA-256。没有玩家运行、世界投影、存储或网络测量；保留的玩家工作者仍计入进程 RSS。

独立运行脚本默认覆盖单房 100/400 个 link，三个模式各五次，预热 500、采集 3000 tick，并轮换模式顺序。`--scenarios 1600` 可探测四房密集负载，超出默认预算时响亮失败，保留已有 partial 记录，不把失败样本当作加速结果。脚本检查模式间对象状态一致、每次实际负载、回调次数、release 构建、包/预算身份和二进制 SHA-256。Mod 模式采用默认注册/结算预算；native 报告中的 `vm_limits` 仅记录对照配置，它不创建 Mod VM，也不受该预算约束。

wall 分位数覆盖整个 `EmbedEngine.tick(1)` 调用；进程 CPU/RSS 沿用上述采样器，不能把进程 CPU 当作 Mod 执行线程账单。本机结果、预算边界和后续优化方向见 [Mod 预算复审与密集负载记录](../research/notes/mod-budget-review-and-perf-2026-10-03.md)。这份局部基准不替代真实玩家闭环或跨引擎共同负载验收。

### 可选成本归因

CLI 和重复采集脚本均支持 `--profile`。默认关闭：不安装 Deno op metrics 回调，不采集阶段墙钟。开启后，`RegisterOptions.profiler` 的线程亲和句柄记录整槽、JS 调用、预算启动/结束、op read/write/native/fault 和候选 host read/write/native。只累加次数和纳秒，不保留逐对象样本。分片恢复沿用同一句柄；它记录全部尝试而非已提交 tick，采集方须显式 reset 划定窗口。CLI 在预热和边界验证完成后 reset。

```sh
python3 scripts/mod-perf-baseline.py --profile --scenarios 400 --warmup 100 --ticks 500 --repetitions 3 --output /tmp/mod-profile.json
```

op 计时来自 deno_core 的 dispatch/completed/error 事件，包含参数/返回转换与 host 方法；host 计时包含候选访问、字段解析/校验与 namespace 写回。JS 调用包含 op，整槽包含 JS 和预算，不能将所有阶段相加；op 与 host 的差值仍包含绑定检查、包装和插桩开销，不能解释为纯 JSON 成本。op 的 errors 来自错误事件，其他阶段只记录次数与耗时。故障上报的来源认领与诊断文本补充均计入 op_fault，一次处理器故障可对应多次 op 调用；该阶段的 calls 不是处理器故障数量。getter 引发嵌套 op 时，op 耗时同样为包含耗时。

诊断报告 v2 增加 `read_to_v8`，围住读取封套的真实 serde 序列化调用；v3 增加 `write_from_v8`，围住真实载荷反序列化，与 TryCatch 创建/销毁和错误文本格式化分开。两者分别包含在 op_read/op_write 内，与 host_read/host_write 分开；write_from_v8 包含 getter 执行及其嵌套 op，不能称为纯数据转换耗时。转换失败的尝试也计时，故障数仍由 op errors 记录。

CLI 开启诊断时声明 `profile_schema_version: 3`；报告门要求完整链每槽六读六写对应各六次转换，缺失或计数不符即失败。没有版本字段的历史报告按 v1 校验原阶段集合，v2 要求读转换、v3 要求读写转换，支持旧二进制前后对照；未知版本拒绝。默认关闭时版本与 profile 均为 null。

开启诊断会增加计时和 op 包装开销，诊断结果与无插桩基线分开汇总。报告门校验各阶段的准确调用次数、错误数与预热排除；无插桩采集拒绝诊断报告。

汇总前校验墙钟分位数与进程 CPU 为有限非负数，分位数有序；报告提供最大值时不得小于 P99。计数与阶段纳秒须为整数，布尔值不能充当数值。上下文封套及门禁的复审修复见 [复审记录](../research/notes/mod-context-envelope-2026-10-03.md#mod-上下文优化复审与修复2026-10-03)。

当前字段缓存的前后对照与归因见 [热路径记录](../research/notes/mod-attribution-2026-10-03.md)。

上下文直接装配与 state 移交的后续对照见 [复制优化记录](../research/notes/mod-context-copy-2026-10-03.md)。

固定读取封套与直接转换计时见 [类型化封套记录](../research/notes/mod-context-envelope-2026-10-03.md)。

写入转换直接计时、getter 资源边界与后续采集见 [写入转换记录](../research/notes/mod-write-conversion-2026-10-03.md)。

拥有值写入封套、Symbol 转换 panic 清偿及全部成对样本见 [写入封套记录](../research/notes/mod-write-envelope-2026-10-04.md)。本地完整链中位成本降低主要落在 host_write，write_from_v8 未见明确下降；尾延迟与无 Mod/空包波动仍未收口，不能据此宣称整体零性能回归。

前后对照探针按模式核验包与预算身份：native、empty、chain 各自跨二进制版本和重复轮次保持一致。不同模式之间允许使用不同包；相同对象终态不能替代该身份校验。发现变化即拒绝成功汇总，保留此前的 partial 记录。同一二进制、模式和诊断开关的重复报告还须保持诊断版本与阶段集合一致；两端二进制允许版本不同，不能将同一端的新增阶段按首份报告静默省略。

### 可选逐 tick 诊断

`mod-perf-probe --tick-diagnostics` 和重复采集脚本的同名选项保留原顺序的逐 tick 墙钟、进程 CPU core-ms 与主动/非主动上下文切换增量。报告 `tick_diagnostics` v1 的 tick 编号从预热后第一个世界 tick 开始；默认字段为 null，不额外调用逐 tick `getrusage`。它与 `--profile` 独立，可组合使用。

```sh
python3 scripts/mod-perf-baseline.py --tick-diagnostics --scenarios 400 --warmup 100 --ticks 500 --repetitions 3 --output /tmp/mod-tick-diagnostics.json
```

每次在 `EmbedEngine.tick(1)` 前后各调用一次 `getrusage(RUSAGE_SELF)`，不包含后续 RSS 采样。墙钟仍只围住 engine 调用；CPU/切换计数的两个采样点稍宽，包含两点之间的计时和样本追加。CPU 是所有线程之和，可超过墙钟；切换也不能指认某个线程、调度等待、GC 或具体业务原因。字段变化只提供排查线索，不能将墙钟减 CPU 当作准确等待时间。

额外系统调用的墙钟合计单列 `sampling_wall_ms`，不从汇总扣除；整进程窗口仍包含诊断采样及记录开销。原始记录在采集完成后序列化。重复采集声明 `instrumented: true`，校验记录数量、顺序、有限非负耗时、整数切换计数，以及由原始墙钟重算的全部分位数/最大值/总和。普通基线和成对采集拒绝逐 tick 诊断报告；汇总也拒绝在同组混入不同诊断口径。没有该字段的历史普通报告继续接受。同二进制对照、逐 tick 原始记录及未解决的波动见[诊断记录](../research/notes/mod-tick-diagnostics-2026-10-04.md)。

### 可配置成对采集

[mod-perf-compare.py](../../scripts/mod-perf-compare.py) 将局部前后对照移入维护脚本，默认预热 500 tick、采集 3000 tick、独立运行五轮，单房 400 link。先保存旧端 release，再传入 `--before`；新端默认使用当前 release：

```sh
python3 scripts/mod-perf-compare.py --before /tmp/kkscreeps-before --output /tmp/mod-paired.json
```

每种模式的两端相邻运行，逐轮交换先后，模式也轮换。支持房间数、link 数、窗口、重复次数和子进程外层期限；`--profile` 额外采完整链诊断，旧端可用 `--profile-before` 指定独立诊断构建。普通窗口始终不插桩，诊断结果单独汇总，两端诊断版本可不同。

`--order-pattern abba` 每模式每轮运行四个独立进程：奇数轮 before/after/after/before，偶数轮 after/before/before/after。每轮包含两对，原始报告记录 `pair` 和顺序口径；默认 `adjacent` 每轮仍一对。除了逐对指标，汇总另列轮数、每轮两端窗口指标的算术均值、均值差/倍率及其中位数。四窗口保留两端在位置上的平衡，可以抵消等间隔的加性线性漂移；不同运行时长、非线性漂移、外部负载仍会影响结果。

```sh
python3 scripts/mod-perf-compare.py --before /tmp/kkscreeps-before --modes native empty --order-pattern abba --output /tmp/mod-balanced.json
```

两对属于同一轮，不能当成两轮独立重复；各窗口分位数的均值也不是合并 tick 后的分位数。零基线保留该对差值并使配对倍率为 null；每轮均值倍率按该轮包含全部窗口的均值计算，均值为零时为 null。同组缺失第二对、重复版本、混用顺序或无效编号会拒绝成功汇总。没有 `pair`/顺序字段的历史报告按 adjacent 第 1 对处理，原逐对口径继续保留。

四窗口工具验收、native/empty 长窗口和同二进制 A/A 见[平衡采集记录](../research/notes/mod-native-balanced-2026-10-04.md)。这次前后版本的 native CPU 各轮有升有降；历史差异仍未归因，不能用后续 A/A 波动消除旧问题。

继续核验实际负载、链计数、全部 link 状态、包/预算和二进制指纹；诊断版本/阶段口径不得在同一端漂移。每次启动前删除临时报表，不能把上一子进程的报表当作新结果。负载错误、超时或身份变化不生成成功报告，保留此前有效窗口的 partial。启动子进程前独占 partial 路径，逐次更新采用同目录临时文件原子替换；成功报告以独占硬链接发布，后来出现的 output 也不能覆盖。首次失败保留零窗口 partial，已有 output/partial 路径拒绝覆盖。

保留每轮 P50/P95/P99 与 CPU，另列每对 after−before 和 after/before 的全部值及中位数；成对倍率的中位数与两个独立中位数之比是不同口径。零基线时倍率为 null，保留该轮差值；不删除零值后计算倍率。增加窗口不能控制桌面温度、频率和外部负载，工具不设置性能硬门禁，纯结算不能作为架构整体达标结论。工具验收与五轮局部对照见 [成对采集记录](../research/notes/mod-paired-sampling-2026-10-03.md)。

## 本地共同负载对照

[compare.py](../../scripts/benchmark/compare.py) 当前默认单组成熟房场景：50 玩家×6 个 RCL8 满功能建筑房，共 300 房；预热 500 tick、采集 3000 tick、五次独立运行、交替两端先后顺序。角色、正常寿命/补员、持续经济/生产、路线控制、库存窗口及工作量校验见[工具说明](../../scripts/benchmark/README.md)。kkscreeps 使用 KKPV，xxscreeps 使用生产 runner、processor、isolated-vm 及场景所需正式包。两端预算统一 limit=100、bucket=10000、tickLimit=500；并发数共同配置。

这是一组明示参数的合成成熟负载，不能把满建筑等同于真实 bot 的行为分布。两端共同 wall 包含玩家执行、结算、提交及错误检查，排除启动、预热、边界查询和 RSS 采样；只比较共同 wall 及当前进程全部线程 CPU/RSS，内部阶段不强行映射。短窗口汇总明确 `formal_window=false`，不作为正式性能达标证据。用户 2026-10-10 修订本批目标为比 xxscreeps 更快且内存更低，2GiB 不作成功硬门槛。默认必须保持同一初始夹具、完整玩家/房间执行与实际活动；后续玩法差异量化披露，允许尚未完成玩法带来的有限差异，不能把 `equivalent=false` 写成逐位兼容通过。严格逐位对照保留为显式排错选项。

xxscreeps 由自定义协调器推进生产执行器，使用进程内共享存储，未启动 HTTP 后台、外部数据库、定期写盘或客户端推送。当前单组只验证成熟房引擎闭环，完整服务成本在相同场景加保存与联网后复验。完整三方 A–D 矩阵仍是长期验收要求，不作为用户指定本批单组的前置。

历史 1/4/8/32 玩家轻量夹具保留 `--profile legacy`；旧版结果见[本地对照记录](../research/notes/p2-local-comparison-2026-10-02.md)，不能用于推断当前成熟房优势。

## 比较对象与正确性前置

比较本项目、pin 版本的官方私服和 xxscreeps。固定源码提交、Node/Rust 工具链、
机器型号、系统、核心数、内存限制、tick 配置、地图、地形、玩家代码及随机种子。
同一个场景在各实现上先核对玩家可见的关键状态与成功执行的动作；行为不等价的
功能不进入“等工作量加速比”，单独列出覆盖差异。官方与 xxscreeps 的版本、
配置和启动命令与原始结果一起归档，不能用调研期的局部微基准代替正式基线。

至少保留以下场景，具体 fixture 在首次跑通时锁定为可复现文件：

| 场景 | 固定负载 | 主要检验 |
|---|---|---|
| A：空闲世界 | 100 房，少量玩家，无本地指令，持续运行 | 休眠房、固定开销、后台写盘 |
| B：单房热点 | 1 活跃房，约 400 对象，其中 50 个 50 部件 creep | 空间索引、结算与单房尾延迟 |
| C：多房多玩家 | 300 房中 150 房活跃，约 50 玩家×人均 6 房（容量目标口径），含共享房 | 派发/结算重叠、世界投影和总资源开销 |
| D：持久化与恢复 | C 的负载加固定频率检查点及故障注入 | 写盘峰值、停顿、恢复时间和丢失窗口 |

先对三方已实现玩法建立共同子集；跨房、市场、power、复杂 bot 具备共同可比能力后扩充
场景，不能把缺失功能造成的低负载算成性能收益。对 A/C 同时报告实际结算的活跃房
数量，确保“休眠”没有改变应有的游戏推进。

先建立 A（空闲世界）与 C（多房多玩家）的共同基线，B（单房热点）和 D（持久化与恢复）在对应能力具备后补齐。完整矩阵要求四类场景、三种实现、每项至少五次独立运行；执行成本按实际采集窗口记录，不预设完成时长。

## 统一指标

| 指标 | 口径 |
|---|---|
| tick 墙钟时间 | 从固定输入到版本提交；报告 p50/p95/p99、最大值及吞吐 tick/s |
| 服务器 CPU | 全部相关进程与中间件的 CPU core-ms/tick；与玩家账单 `Game.cpu` 分列 |
| 内存 | 稳态和峰值常驻内存；多进程列各进程及汇总，共享页按平台可得的 PSS/等价口径说明 |
| 复制与分配 | 每 tick JSON 序列化字节、世界/快照复制字节、分配量、GC 时间 |
| 阶段耗时 | 玩家运行、等待房间依赖、房间结算、全局阶段、提交、检查点、推送 |
| 正确性 | 关键状态差异、失败/跳过 tick、实际执行玩家数与房间数 |

正式观测点应带 `tick_id`、`room_id` 和阶段名；现有汇总报告尚未提供完整逐房追踪，采样开销需单独测量。CPU 限额属于游戏
经济语义，不能为提高“服务器 CPU”结果而偷偷减少玩家预算。磁盘与网络后台工作
计入总 CPU/内存，并报告是否积压。

## 跑法与判定

所有实现使用 release/生产构建。每个场景先预热至少 500 tick，再采集至少
3,000 tick，独立运行至少 5 次；报告每次结果和中位数，不只挑最好的一次。
机器负载、温度/频率变化、数据目录状态与测量工具版本需记录。基准环境不能保证
稳定时，扩大样本并报告波动，暂不把噪声结果设成 CI 硬门禁。

架构验收需覆盖真实玩家 API、指令和结算闭环，量化每玩家世界物化、每房克隆/索引及检查点成本。当前完整世界 JSON 仍是每玩家每 tick 的必经数据，房间结算仍等待请求玩家结果收齐；这些尚未兑现的优化须明确记录，不能用局部查询数字宣称整体达标。

正式对照目标是在行为等价的 C 类参考负载上，服务器总 CPU 与稳态内存分别不高于官方私服的 20%；10% 为进阶目标。它们是验收目标，不是现有结果。B、D 同时公布尾延迟、峰值内存和恢复结果。未达到时报告差额和限制，不改变工作负载来保留倍率。

在专用、稳定的基准机建立历史基线后，可对关键场景设置回归报警；先用连续多次
结果判断显著退化，再决定 CI 阈值。功能 CI 的 screeps-ok 棘轮继续独立运行，
性能门禁不得以跳过兼容用例换取绿色结果。
