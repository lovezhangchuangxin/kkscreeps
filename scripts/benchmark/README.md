# 本地引擎性能对照

`compare.py` 对照 kkscreeps 的 release `EmbedEngine` 与本地 xxscreeps 的生产 runner、processor 和 isolated-vm。每次启动独立进程、创建新世界；xxscreeps 使用临时配置和进程内共享存储，不连接既有服务器或数据目录。

```bash
cargo build --release -p kk-cli
python3 scripts/benchmark/compare.py \
  --xxs-root /path/to/xxscreeps \
  --output /tmp/kkscreeps-comparison
```

xxscreeps 需要已编译的 `packages/xxscreeps/dist`、安装好的 `node_modules` 和可用的 isolated-vm 原生模块。本次适配器在 Node v24.11.0 上验证，复跑请使用该版本；较旧 Node 的语法支持未验证。工具不会安装依赖或重建外部项目；发现对应 TypeScript 比 dist 新时会拒绝开始，需先在外部项目执行其正常构建。输出目录必须不存在，避免覆盖已有采样。

默认 1／4／8／32 玩家，各做五次独立运行；每次预热 500 tick、采集 3000 tick，玩家并发上限为 2。两端预算明确统一为 limit=100、bucket=10000、tickLimit=500，并在首 tick 校验玩家实际看到的值。重复间交替两个引擎的先后顺序。`--players`、`--runs`、`--warmup`、`--ticks`、`--concurrency` 可调整，但短窗口或少于五次只能用作调试，不能沿用正式结果的结论。

## 工作量检查

两端读取同一份夹具和 [player.js](player.js)。每玩家一间独立房，RCL 4、两处 source、一座 spawn、三个 WORK/CARRY/MOVE creep。出生方向固定为 TOP、LEFT；静止工作者采集同一 source，持有能量时升级。长窗口包含再生、孵化及 creep 死亡。

首 tick 从玩家 API 逐格核查实际地形和控制器归属。报告保存初始、预热结束、采集结束的全部房间关键状态：控制器等级和进度、source 坐标和能量/容量、creep 名称/坐标/部件/生命/能量/孵化状态、spawn 坐标/生命/能量/孵化状态。空 store 的 energy 统一成 0；对象 ID 不参与比较，两端生成方式不同。

脚本检查每个动作返回 OK，并为每名玩家输出累计执行 tick、孵化、采集、升级计数。工具拒绝缺少玩家、跳过 tick、没有升级或缺少完整负载的报告；拒绝两端关键状态或动作计数不同的结果。独立重复之间也检查世界状态一致。这些检查覆盖夹具使用的机制，不代表完整 API 兼容证明。

## 计时与适用范围

- tick wall 包含玩家运行、世界结算、提交及执行错误检查，排除启动、预热、前后状态查询和每 tick 后的 RSS 采样。kkscreeps 另外保留原有 `EmbedEngine` 阶段计时。
- CPU 使用当前进程全部线程的采集窗口累计用量，单位 core-ms/tick。xxscreeps 的 runner、processor 和隔离沙箱均在该 Node 进程内。两端 CPU 窗口包含 RSS 采样及循环开销，排除前后状态查询。
- RSS 在采集前和每 tick 后采样，报告中位数与采样点最大值，可能遗漏 tick 内瞬时峰值。统计窗口相同，进程包括各实现已保留的运行时、模块和沙箱资源。
- 每次 wall 分位数采用排序后 `round((N−1)×q)` 的样本，再对五次运行的同名指标取中位数；不把全部 tick 混成一个样本集。

xxscreeps 由适配器编排生产服务，包含共享存储和消息队列成本；kkscreeps 直接调用嵌入接口。xxscreeps 可在玩家执行期间处理已具备依赖的房间，阶段边界与本项目不同，因此只比较共同的 tick wall、进程 CPU/RSS，不强行对齐 project/run/settle。

此对照不包括 HTTP 后台、客户端推送、定期磁盘保存、外部数据库或完整服务器部署。它也未覆盖共享房竞争、跨房、多部件热点和复杂 bot。官方私服尚未接入；结果不能用于判定[三方端到端验收目标](../../docs/architecture/performance-acceptance.md)达标。

## 产物与验证

输出包括共享夹具、每次原始报告和日志、二进制/Node/isolated-vm/xxscreeps 源码及 dist 的哈希、提交与工作区状态、机器信息、每档汇总。记录的是实际执行的 dist，不以 Git 提交号代替未提交代码或编译产物。xxscreeps 的正常配置加载可能生成 `dist/config.schema.json`；该派生文件不计入代码树哈希，结束后另存工作区状态。

每次对子结束检查两端源码、夹具脚本、适配器、Node、二进制、锁文件、xxscreeps 编译产物及 isolated-vm 模块没有变化。超时会终止采集进程组；服务退出、沙箱错误和行为差异使工具失败，失败日志仍保留，且不会生成成功汇总。较大的玩家档应给够 `--timeout`，默认每进程 600 秒。

```bash
python3 -m unittest discover -s scripts/benchmark -p '*test*.py'
node --check scripts/benchmark/xxs-probe.mjs
```

## 历史研究采集工具

`research/` 保存架构、Mod 宿主/归因、玩家工作量与 KKPV 规模实验的采集脚本。日期标识它们对应的实验协议；历史证据和复现参数仍在 [研究索引](../../docs/research/README.md)，默认报告位置保持在 `docs/research/notes/`。这些脚本可复跑对应实验，不作为当前性能达标结论。

工具回归分别位于 `scripts/tests/` 和本目录。完整 `pnpm run check` 同时执行两组 Python 测试，以及门禁和文档链接检查器的 Node 回归测试。
