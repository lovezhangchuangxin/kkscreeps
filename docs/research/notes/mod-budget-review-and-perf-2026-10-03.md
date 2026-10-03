# Mod 预算复审清偿与密集负载（2026-10-03）

本片承接 [Mod VM 资源预算](mod-vm-budget-2026-10-03.md)，基线仍为 `471ac1d`；预算与本次清偿未提交。先修复纯 JS 超时后的异常转换漏洞，再建立密集对象的纯结算对照，未改变默认限额。

## 复审发现与修复

P1 复现：注册 CPU 2 s、墙钟 30 ms，模块先安装死循环的 `Error.prepareStackTrace` 或 `Error.prototype.name` getter，再执行顶层死循环。普通死循环约 30 ms 返回，带错误钩子的两个子进程均超过 3 s，只能外部终止。

根因是看门狗首个墙钟中断后停止监控，但 deno_core 0.412.0 的 `exception_to_err` 会暂时取消终止，并读取错误属性/格式化栈。预算看门狗现在在到期时留存墙钟故障，每 1 ms 持续投递至 `finish/disarm`，覆盖异常转换中的 JS；代次检查、投递与解除武装仍共用锁。普通玩家看门狗保持原单次投递行为。

故障矩阵增加 CPU/墙钟 × prepareStackTrace/name getter 四例，总计 15 例；两种墙钟钩子各独立执行 50 次均有界返回。外层父测试仍设 10 秒期限，失败不会挂住测试进程。

## 测量方法

新增 `mod-perf-probe` 与 [重复运行脚本](../../../scripts/mod-perf-baseline.py)，使用[固定包](../../../compat/fixtures/mod-perf/chain/perf/server.js)。三种模式为无 Mod 的 native、空包的 empty、before + 两层 wrap + native 中心 + after 的 chain。每个链位累计自己的 namespace 计数，额外状态写回属于该负载成本。

每个 link 初始冷却为总 tick 数，保证整个窗口每 tick 都发生 native 减冷却写入。各模式在初始、预热后和采集后验证所有 link 的完整快照、tick/generation 和链位计数；状态摘要必须一致。该合成初始值不代表真实 link 的冷却上限。

正式采集选单房 100/400 个 link，500 tick 预热、3000 tick 采集，每模式五次独立进程，轮换模式顺序；release 二进制与包/预算身份固定。采集只包含纯结算，没有玩家执行或世界投影。CPU 为当前进程全部线程、RSS 包含闲置玩家工作者，不能解释为 Mod 单独资源账单。测量时不并发运行本片的构建、测试或 bot/棘轮；其他桌面应用与机器频率未控制，不设硬门禁。

## 结果

30 次独立运行全部成功；每次三个边界的 link 状态摘要与预期一致，四个链位计数正确。表内为五次运行各项指标的中位数；RSS 为采样点 p50 的中位数。全部原始报告、每次 p99 和二进制摘要见[正式采集数据](mod-perf-validation-2026-10-03.json)。

| link 数 | 模式 | wall P50 ms | wall P99 ms | CPU core-ms/tick | RSS MiB |
|---|---|---:|---:|---:|---:|
| 100 | native | 0.514 | 0.565 | 0.512 | 28.7 |
| 100 | empty | 0.490 | 0.558 | 0.499 | 32.0 |
| 100 | chain | 9.203 | 10.306 | 9.490 | 60.2 |
| 400 | native | 1.999 | 2.125 | 2.000 | 31.4 |
| 400 | empty | 1.993 | 2.140 | 1.991 | 35.3 |
| 400 | chain | 36.688 | 38.624 | 37.712 | 63.7 |

400 link 的 chain 每次 p99 为 38.159 / 40.909 / 38.257 / 38.624 / 39.646 ms，最大采样 RSS 65.5 MiB。100/400 对象的正式运行均未触发默认限额；进程 RSS 不是 V8 堆/堆外账单，这组数值不用于证明内存硬隔离。

空 Mod 的结算耗时接近 native，当前测量不支持改掉无扩展槽的 native 直调路径。完整链成本明显更高，下一片应先量化上下文构造与 JSON 桥的成本，再尝试减少重复物化/转换；必须保留 next 前同步、内层返回后刷新及故障留存语义。尚未完成操作级归因，本片不把全部差值归咎于序列化，也不预设性能收益。

四房 1600 link 的短探针（5 tick 预热、20 tick 请求采集）在默认 100 ms 累计 CPU 预算下失败，报 `W2N1` 的 link.tick CPU 预算耗尽。它只证明该密度的完整链超过当前额度，不给出成功耗时分位数，不能与其他模式形成性能加速比。本片保留默认预算，由部署方根据实际玩法与资源配置调整。

## 验证

`pnpm run check`：555 个 Rust 测试通过、8 个 ignored，19 个 JavaScript 测试通过，格式与 workspace/all-targets Clippy 通过。Python 报告门禁三测通过。棘轮 1854 passed / 153 failed / 907 skipped，零转红/转绿；tooangel、overmind、wasm 完成 2000/2000/10 tick，零终止/惩罚跳过/missing API；overmind 的 1 个 console-error tick 与 tooangel 的 lookFor 日志错误仍是既有形态。

采集期间不改 release 产物；构建摘要、15 例故障矩阵、两个钩子各 50 次复跑、短探针和 bot 原始记录见[机器验收文件](mod-budget-review-validation-2026-10-03.json)。不可中断原生调用、进程级 OOM、未统计外部分配等资源边界保持原约束。
