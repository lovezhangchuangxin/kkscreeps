# Mod 逐 tick 诊断与桌面测量波动

## 范围与实现

基于 `e1f866e` 的停用/自动处置提交，继续调查历史无 Mod、空包和尾延迟差异。此批增加测量能力，没有修改 Mod 调用、预算、中断、指令或候选提交规则。历史差异仍未归因，不以新增 A/A 数据将其解释为噪声或关闭问题。

CLI 与 `scripts/mod-perf-baseline.py` 新增 `--tick-diagnostics`；默认关闭。开启时保存原顺序逐 tick 的世界编号、engine 墙钟、进程全部线程 CPU core-ms、主动/非主动上下文切换增量。每 tick 两次 `getrusage`，采样系统调用墙钟另列；序列化发生在采集窗口之后。可与阶段 `--profile` 组合，但这些诊断数据增加测量开销。

报告 v1 校验数量、顺序、数值和从原始 wall 重算的汇总；普通基线/成对工具拒绝逐 tick 诊断，避免被当成无插桩结果。同组汇总拒绝诊断口径改变；历史普通报告没有该字段仍可接受。详细边界见[性能架构](../../architecture/performance-acceptance.md)。

## 原始窗口与身份

全部 51 个窗口均保留，没有删除慢窗口、用长窗口替换短窗口或设置性能硬门禁。使用单房 400 link，默认 VM 预算。采集与构建、测试互不重叠；桌面应用负载、温度及频率未控制。报告元数据中的 git 提交/状态来自运行时 checkout，并非编译来源证明；二进制 SHA-256 与[验证记录](mod-tick-diagnostics-validation-2026-10-04.json)中的源码摘要分列。

- 改动前 release：`17d82a7667cde48114692340c4675188bb5fa1e1c8e4920c350bb9480ffb6763`，保存在 `/tmp/kkscreeps-auto-disable-before-tick-diagnostics`。
- 改动后 release：`83d5fa2cb0c97901397e4f5949e795bb973eb27145c93529a908220882adce6d`。
- [同二进制 A/A](mod-tick-aa-perf-2026-10-04.json)：native/empty 各五对，预热 500、采集 3000，20 个普通窗口。before/after 只是相邻两个进程的标签，两端哈希相同。
- [工具改动前后短窗口](mod-tick-tool-perf-2026-10-04.json)：native/empty 各三对，预热 100、采集 500，12 个普通窗口。
- [empty 长窗口复核](mod-tick-tool-empty-long-perf-2026-10-04.json)：五对，预热 500、采集 3000，10 个普通窗口。因短窗口 empty 三对均升高而新增，原短窗口保留。
- [逐 tick 诊断](mod-tick-diagnostics-perf-2026-10-04.json)：native/empty/chain 各三次，预热 100、采集 500，9 个诊断窗口、4500 条原始记录；没有开启阶段 profile。
- [派生排查数据](mod-tick-diagnostics-analysis-2026-10-04.json)：各诊断窗口的 CPU/切换总量、采样成本和最慢十个 tick；按 wall 降序取前十，CPU 与切换按窗口求和。

复现配置：

```sh
python3 scripts/mod-perf-compare.py --before target/release/kkscreeps --after target/release/kkscreeps --modes native empty --warmup 500 --ticks 3000 --repetitions 5 --output /tmp/mod-tick-aa.json
python3 scripts/mod-perf-compare.py --before /tmp/kkscreeps-auto-disable-before-tick-diagnostics --modes native empty --warmup 100 --ticks 500 --repetitions 3 --output /tmp/mod-tick-tool.json
python3 scripts/mod-perf-compare.py --before /tmp/kkscreeps-auto-disable-before-tick-diagnostics --modes empty --warmup 500 --ticks 3000 --repetitions 5 --output /tmp/mod-tick-tool-empty-long.json
python3 scripts/mod-perf-baseline.py --tick-diagnostics --scenarios 400 --warmup 100 --ticks 500 --repetitions 3 --output /tmp/mod-tick-diagnostics.json
```

## 结果与限制

下表是所有成对 after/before 倍率的中位数减一；短、长窗口分别列出，不合并。

| 采集 | 模式 | P50 | P99 | 进程 CPU/tick |
|---|---|---:|---:|---:|
| 同二进制 A/A，五对 | native | +0.622% | −2.551% | +0.590% |
| 同二进制 A/A，五对 | empty | −0.798% | −6.117% | −1.131% |
| 工具改动前后短窗口，三对 | native | −0.253% | +2.266% | −0.357% |
| 工具改动前后短窗口，三对 | empty | +2.427% | +5.059% | +2.140% |
| 工具改动前后长窗口，五对 | empty | −1.687% | +3.417% | −1.901% |

A/A native 的第一对 CPU 差 +4.661%、P99 差 +27.438%；其余四对 CPU 为 +0.191%、−0.273%、+1.611%、+0.590%。同版本也会出现明显配对差异，但这只说明当前环境的重复性不足，不能证明历史的 +2.75% CPU、空包和链尾延迟变化没有代码原因。

工具改动的 empty 短窗口 CPU 三对为 +0.687%、+2.140%、+6.472%，第三对 P99 +78.253%。长窗口 CPU 五对为 +1.269%、+1.444%、−3.966%、−1.901%、−3.377%，P99 为 +4.862%、+3.417%、−9.585%、−3.881%、+3.961%。没有观察到跨这两组持续同方向的 CPU 升幅，P99 仍未归因；不能据长窗口中位数转负宣布零回归。

逐 tick 诊断中，native/empty 每个窗口 CPU 总和约等于 wall 总和；chain 为 wall 的约 1.035 倍，因为进程 CPU 包含其他线程。chain 第三次 tick 325 的 wall 为 49.7875 ms，CPU 为 32.914 core-ms，非主动切换增量 940；第一、二次最慢 tick 的 CPU 接近或超过 wall。墙钟尖峰并非均具有同一 CPU 表现，不能用总 CPU 汇总说明全部峰值。采样窗口略宽于 wall，wall−CPU 也不是精确的等待时长。

两次额外 `getrusage` 的墙钟合计每 tick：native/empty 各窗口约 0.658..0.766 微秒，chain 约 1.423..2.004 微秒。这只量化系统调用采样，未包括全部样本记录、分支和缓存影响，不能作为诊断总开销的 A/B 估计。

chain 每窗口约 21.1 万次非主动切换，native/empty 为 0..154；计数涵盖全部线程，不能直接指认其来源。源码中 `ModRealm::with_budget` 每对象槽启动/结束预算，`ExecutionBudget::start/finish` 武装/解除看门狗并通知条件变量，是下一步值得单独验证的成本候选。需先独立归因，再考虑减少启停，保留跨对象累计预算、V8 安全点持续中断和失败候选回滚边界。

## 验证

Debug 与 release CLI 测试均 9 通过；三种模式与两个诊断开关的组合核对对象终态、精确链调用数、预热排除和墙钟汇总。Python 工具测试 30 通过，覆盖缺失/错序/不合法原始记录、汇总不符与普通/诊断混用。全仓 `pnpm run check` 通过：590 Rust 通过、10 个约定忽略，19 JavaScript 通过、0 跳过；格式、Clippy 与文档链接通过。

JavaScript/适配器检查显式选择重建后的 release CLI。此批只改测量工具，没有重跑完整 screeps-ok；最近完整兼容性门禁仍为[自动停用验收](mod-auto-disable-validation-2026-10-04.json)的 1854 通过、153 失败、907 跳过、0 红。该历史结果不当作本批重新执行的结果。
