# 本地引擎性能对照

`compare.py` 对照 kkscreeps 的 release `EmbedEngine` 与本地 xxscreeps 的生产 runner、processor 和 isolated-vm。每次启动独立进程、创建新世界；xxscreeps 使用临时配置和进程内共享存储，不连接既有服务器或数据目录。

首次接手先运行同一 50×6 场景的短窗，确认本机依赖和工作量闭环：

```bash
cargo build --release -p kk-cli
python3 scripts/benchmark/compare.py \
  --xxs-root /path/to/xxscreeps \
  --players 50 --runs 1 --warmup 5 --ticks 50 --concurrency 4 \
  --output /tmp/kkscreeps-mature-smoke
```

短窗成功后，正式采集使用默认的 500+3000 tick、五轮；建议保持同一并发配置：

```bash
python3 scripts/benchmark/compare.py \
  --xxs-root /path/to/xxscreeps --concurrency 4 \
  --output /tmp/kkscreeps-mature-formal
```

当前基线每 tick 约 6 秒，完整窗口每端数小时，五轮耗时更长；热点优化前优先短窗归因。`--output` 每次使用新目录。两端顺序执行，正式采集期间保持机器空闲，不同时构建、跑测试或其他基准。

查看 `summary.json`：`success` 表示夹具/执行/活动校验通过；`formal_window` 表示是否满足正式采样长度和轮次；`equivalent` 表示所比较的公开状态和动作是否一致；`observed_goal` 只记录 P50 与峰值 RSS 两个观测是否领先。查看 `workload_comparisons` 的动作差异，再解释速度倍率。严格排错可追加 `--strict-equivalence`。退出失败时看对应引擎 `.log`、原始 `.json` 和已有的 `failure.json`；不是所有启动/运行失败都会写 `failure.json`。

xxscreeps 需要已编译的 `packages/xxscreeps/dist`、安装好的 `node_modules` 和可用的 isolated-vm 原生模块。本次适配器在 Node v24.11.0 上验证，复跑请使用该版本；较旧 Node 的语法支持未验证。工具不会安装依赖或重建外部项目；发现对应 TypeScript 比 dist 新时会拒绝开始，需先在外部项目执行其正常构建。输出目录必须不存在，避免覆盖已有采样。

默认采用 `--profile mature`：只采集 50 玩家×6 个 RCL8 满功能建筑成熟房，共 300 房，各做五次独立运行；每次预热 500 tick、采集 3000 tick，玩家并发上限为 2。两端预算明确统一为 limit=100、bucket=10000、tickLimit=500，玩家基础堆上限均为 64MiB（xxscreeps 内部另计共享地形 allowance），并在首个玩家 tick（time=1）校验玩家实际看到的值。kkscreeps 成熟房走 KKPV；xxscreeps 使用生产 isolated 沙箱与 classic/factory/observer/powerspawn/nuker 包。重复间交替两个引擎的先后顺序。`--players`、`--runs`、`--warmup`、`--ticks`、`--concurrency` 可调整，但短窗口或少于五次只能用作调试，不能沿用正式结果的结论。

## 成熟房工作量与正确性

当前单组夹具在 [mature.py](mature.py) 与 [mature-player.js](mature-player.js)。每名玩家六房，全部 RCL8：按仓库固定 `CONTROLLER_STRUCTURES` 放满 13 类功能建筑（3 spawn、60 extension、6 link、6 tower、10 lab、5 container，以及 storage/terminal/extractor/observer/powerSpawn/nuker/factory 各 1）。道路、防御环和墙保留可通行出入口，报告初始对象给出精确数量与位置，不把道路/墙/rampart 各 2500 的配额机械填满。

每房另外有 298 road、100 rampart 和 22 constructedWall；全服显式播种 160200 对象，控制器由分片创建。每房初始 14 个角色：双源矿工、双运输、三个填充者、升级、维修、矿物采集，以及 terminal/factory/powerSpawn/link 供能。身体为 12/20/26 部件、无 boost，MOVE 足以满载走平地。creep 使用正常 1500 tick 寿命，spawn 根据缺失角色补员；容量与原生衰减保持正常值。每玩家初始化 8KiB Memory 载荷，保留角色与路线缓存，并清理死亡角色记忆。Memory 大小和角色数为明示的合成成熟负载参数，尚未由真实 bot 分布标定。

玩家执行采集、搬运、升级、道路/防线维修（各塔负责独立防线格，避免重复维修同一目标的语义差异）、link 转移、8 个输出 lab 的 H+O 反应、battery 生产、power 处理、observer 查询和相邻自有房 terminal 运输。初始库存两端相同，不在采集期间宿主补充资源；夹具总窗口限制为 3750 tick，默认 500+3000。nuker 等低频建筑保持真实待机，不强迫每座建筑每拍发出意图。

为了避免两引擎同长路径的择优差异导致不同运输工作量，共同脚本先用相同 BFS 生成可通行路线，再把路线作为 CostMatrix 走廊调用真实 `PathFinder.search` 并核验完整性与长度；实际 `move` 沿共同路线，固定地形/建筑与矩阵缓存在全局，路线保留在 Memory 中，每 20 tick 刷新，当前占格与本拍预留目的格控制竞争。这包含实际移动/寻路成本，但不代表任意 bot 的默认 moveTo 拥堵性能。xxscreeps 对象通过生产工厂创建；管理读面装载时对 road/rampart/container 的初始衰减时钟补一拍，校正工厂原本用于 nextTime 结算的时间约定，不修改后续 processor。

两端比较初始、预热后、采集后全部房间的功能建筑数、坐标/耐久/库存/冷却/孵化状态、creep 名称/身体/坐标/库存/寿命/疲劳、source 与矿物状态。初始建筑/creep 的位置、库存与身体另与共享夹具核对，避免两端共同漏放也被误判等价。每玩家和每房必须完整执行；账本仅计采集窗口，要求采集、升级、搬运、移动、原生寻路、矿物采集、反应、工厂、terminal、power 和 observer 有实际成功调用，窗口覆盖寿命时还要求补员。默认要求初始世界相同、各自通过完整执行与真实活动校验；按用户 2026-10-10 修订口径，后续玩法状态或动作数差异允许保留，汇总给出各动作计数及差异百分比、各阶段有差异的房间数和首个字段。`equivalent=false` 不等于采集失败，不能据此声称逐位兼容；明显不同的动作工作量也不能解释成等工作量加速。`--strict-equivalence` 保留逐字段/逐动作拒收，用于兼容性排错。

本批成功目标是同一成熟房负载下 wall 更低且进程 RSS 更低，2GiB 仅记录作参考，不再作为硬性门槛；汇总的 `observed_goal` 明示 P50 和峰值 RSS 两个观测指标，P95/P99/CPU 同时保留。正式窗口标记只表示采样长度与轮次，不能替代玩法差异和性能结论的审核。

开发排错可缩短窗口、用 `--players 1` 验证同一六房布局；它不是新增性能场景。成功汇总声明 `formal_window`，只有 ≥500 预热、≥3000 采集、≥5 独立轮次才为 true，短窗口不能称为正式性能验收。

旧共同夹具仍可通过 `--profile legacy` 复现，默认 1／4／8／32 玩家，一房 RCL4、两 source、一 spawn、三个 worker，使用 [player.js](player.js)。此历史入口不作为成熟房批次的前置或新增验收组。

## 计时与适用范围

- tick wall 包含玩家运行、世界结算、提交及执行错误检查，排除启动、预热、前后状态查询和每 tick 后的 RSS 采样。kkscreeps 另外保留原有 `EmbedEngine` 阶段计时。
- CPU 使用当前进程全部线程的采集窗口累计用量，单位 core-ms/tick。xxscreeps 的 runner、processor 和隔离沙箱均在该 Node 进程内。两端 CPU 窗口包含 RSS 采样及循环开销，排除前后状态查询。
- RSS 在采集前和每 tick 后采样，报告中位数与采样点最大值，可能遗漏 tick 内瞬时峰值。统计窗口相同，进程包括各实现已保留的运行时、模块和沙箱资源。
- 每次 wall 分位数采用排序后 `round((N−1)×q)` 的样本，再对五次运行的同名指标取中位数；不把全部 tick 混成一个样本集。

xxscreeps 由适配器编排生产服务，包含共享存储和消息队列成本；kkscreeps 直接调用嵌入接口。xxscreeps 可在玩家执行期间处理已具备依赖的房间，阶段边界与本项目不同，因此只比较共同的 tick wall、进程 CPU/RSS，不强行对齐 project/run/settle。

此对照不包括 HTTP 后台、客户端推送、定期磁盘保存、外部数据库或完整服务器部署。成熟房覆盖多房、多部件 creep 与 terminal 跨房运输，尚未覆盖不同玩家共享房竞争、creep 跨房和真实复杂 bot。官方私服尚未接入；结果不能用于判定[三方端到端验收目标](../../docs/architecture/performance-acceptance.md)达标。

## 产物与验证

2026-10-10 已完成同一 50×6 场景的两次开发短窗：30+100（原 lexical terminal 配对，动作数一致、有库存差异）与 5+50（显式邻房配对，三阶段状态和动作数全部一致）。[证据清单](../../compat/reports/mature-comparison/2026-10-10/manifest.json)保留完整参数、原始压缩报告/夹具/日志/哈希；[最终夹具短窗汇总](../../compat/reports/mature-comparison/2026-10-10/50x6-adjacent/summary.json)中 kk/xxs P50 为 6198.34/264.70ms、RSS peak 为 3017.17/4348.03MiB。两次采样都显示内存较低、执行较慢，尚未满足两个条件同时领先；不把短窗数值作为稳态幅度。kk project P50 5552.90ms，下一步优先优化投影重复视野/归属扫描。正式五轮长窗口未执行。

输出包括共享夹具、每次原始报告和日志、二进制/Node/isolated-vm/xxscreeps 源码及 dist 的哈希、提交与工作区状态、机器信息、每档汇总。记录的是实际执行的 dist，不以 Git 提交号代替未提交代码或编译产物。xxscreeps 的正常配置加载可能生成 `dist/config.schema.json`；该派生文件不计入代码树哈希，结束后另存工作区状态。

每次对子结束检查两端源码、夹具脚本、适配器、Node、二进制、锁文件、xxscreeps 编译产物及 isolated-vm 模块没有变化。超时会终止采集进程组；服务退出、沙箱错误、初始夹具差异或严格模式中的行为差异使工具失败，失败日志仍保留，且不会生成成功汇总；初始状态不一致或启用严格模式后发现状态/工作量不一致另写 `failure.json`，给出首个字段差异。较大的玩家档应给够 `--timeout`，默认每进程 43200 秒。

```bash
python3 -m unittest discover -s scripts/benchmark -p '*test*.py'
node --check scripts/benchmark/xxs-probe.mjs
```

## 历史研究采集工具

`research/` 保存架构、Mod 宿主/归因、玩家工作量与 KKPV 规模实验的采集脚本。日期标识它们对应的实验协议；历史证据和复现参数仍在 [研究索引](../../docs/research/README.md)，默认报告位置保持在 `docs/research/notes/`。这些脚本可复跑对应实验，不作为当前性能达标结论。


2026-10-10 优化批追注：视野/observer 共享索引、结算字面量门与单次分类、KKPV 房间块共享装配与类型化直造、JS 定宽列急切解码已落地（见[接手台账](../../docs/plans/mod-next-batches.md)）；同机同窗复测归档于 [50x6-after-opt-abc](../../compat/reports/mature-comparison/2026-10-10/50x6-after-opt-abc/summary.json)，kk/xxs 同窗比率 P50 23.4×→3.68×、CPU 5.4×→1.58×、RSS 峰值低约 20%，动作与状态等价保持。上段"kk project P50 5552.90ms"为优化前归档口径，保留不追改；本批未跑五轮正式长窗。

工具回归分别位于 `scripts/tests/` 和本目录。完整 `pnpm run check` 同时执行两组 Python 测试，以及门禁和文档链接检查器的 Node 回归测试。
