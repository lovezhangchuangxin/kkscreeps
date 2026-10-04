# native/empty 长窗口与 ABBA/BAAB 平衡采集（2026-10-04）

【实测】这次长窗口中，看门狗优化前后 native CPU 的逐轮均值倍率中位差为 −0.502%，五轮有升有降；empty 为 +0.093%。同二进制 native A/A 的对应中位差为 +0.505%。本次较长窗口的中位数没有复现上一轮短窗口 native CPU +1.255% 的升幅，但没有定位历史差异的原因，也没有证明 native 零回归。保留[上一轮全部窗口与结论](mod-watchdog-notify-2026-10-04.md)。

## 本轮实现与源码核对

修改范围是 Python 对照工具、工具测试与文档；没有修改游戏规则、预算逻辑或 Rust 实现，也没有重建二进制。[工具](../../../scripts/mod-perf-compare.py)新增可选 `--order-pattern abba`，默认 `adjacent` 仍每轮一对。

每模式每轮在独立进程中采集四窗口：奇数轮 before/after/after/before（ABBA），偶数轮 after/before/before/after（BAAB）。模式逐轮轮换，诊断窗口单独安排。报告保留 `repetition`、`pair`、`order_pattern` 与全部窗口；每轮两对，五轮共十对。原逐对差值和倍率保留，另算每轮两端窗口指标的算术均值、均值差、倍率以及中位数。两对属于同一轮，不能当成两轮独立重复；窗口 P99 的均值也不是合并 tick 后的 P99。

【推断】四个位置两端对称，可以抵消等间隔的加性线性漂移；实际进程时长不同、非线性漂移、温度/频率及外部负载仍会影响结果。工具没有给出置信区间或性能硬门禁。单组缺失第二对、重复版本、混用顺序或无效编号会拒绝汇总；零基线保留差值并将无法定义的倍率记为 null。历史报告没有新增字段时按 adjacent 第 1 对解释。

【源码】`EmbedEngine::create_shard` 调用不带 Mod 包的 `create_shard_inner`，后者将 `mod_realm` 设为 None；`tick` 通过 `settle_world` 结算，`settle_world_with_writebacks` 仅在存在 Mod VM 时启停整轮预算。见[实现](../../../crates/kk-runtime/src/embed.rs)。因此 native 纯结算不进入 Mod 预算启停，不能把其升幅直接归因于通知合并。该路径核对不能排除构建/链接布局、分配、后台线程或环境影响，也不构成已确定的根因。

## 环境与复跑

基准来自 `e1f866ef72ccd4dbd95de0222524890701e39871` 上的未提交工作区；不能用该提交号单独复现二进制。macOS 15.7.3 / arm64 / 10 个逻辑 CPU；运行环境 rustc 1.98.1。未控制桌面外部负载、温度与频率。采集期间没有并行构建、测试或兼容性棘轮。

- before：`83d5fa2cb0c97901397e4f5949e795bb973eb27145c93529a908220882adce6d`，优化前备份 `/tmp/kkscreeps-watchdog-before`。
- after：`89b73d73afde8564d112022f7d356e03ca10880588b4b21860dd8bda5e4039d7`，当前 release 二进制；A/A 两端都用这一文件。
- 每窗单房 400 link、预热 500 tick、采集 3000 tick、五轮，无阶段/逐 tick 插桩。40 个前后窗口和 20 个 A/A 窗口合计 60 个，采集 tick 合计 180000；原始报告的全部窗口均保留，没有剔除异常窗口。
- 每窗口验证 release、实际负载、计数、三个边界全部 link 状态摘要；同模式包和预算身份跨版本/轮次不变，二进制指纹采集前后不变。不同模式允许包不同。

```sh
python3 scripts/mod-perf-compare.py --before /tmp/kkscreeps-watchdog-before --modes native empty --order-pattern abba --warmup 500 --ticks 3000 --repetitions 5 --output /tmp/mod-native-balanced-new.json
python3 scripts/mod-perf-compare.py --before target/release/kkscreeps --after target/release/kkscreeps --modes native --order-pattern abba --warmup 500 --ticks 3000 --repetitions 5 --output /tmp/mod-native-balanced-aa-new.json
```

实际 A/A 在前后对照完成后顺序采集，并未穿插到前后对照中。因此它只展示后续同二进制窗口的差异，不能当作前后对照的同期噪声标定，更不能扣除后用来消除历史回归。

## 全部结果

【实测】下表百分比为 `(after/before−1)×100%`。逐轮口径先取该轮每端两窗口指标的均值，再计算倍率；逐对口径按相邻对算倍率。两种中位数不同，不能替换。原始数据见[前后版本 40 窗口](mod-native-balanced-perf-2026-10-04.json)与[同二进制 20 窗口](mod-native-balanced-aa-perf-2026-10-04.json)。

| 对照 / 模式 | 指标 | 五轮均值倍率中位差 | 十对倍率中位差 |
|---|---|---:|---:|
| 前后版本 native/empty / native | P50 | -0.414% | -0.947% |
| 前后版本 native/empty / native | P95 | -0.557% | -1.357% |
| 前后版本 native/empty / native | P99 | -12.426% | -4.026% |
| 前后版本 native/empty / native | CPU core-ms/tick | -0.502% | -1.054% |
| 前后版本 native/empty / empty | P50 | +0.640% | +0.613% |
| 前后版本 native/empty / empty | P95 | -0.739% | -1.667% |
| 前后版本 native/empty / empty | P99 | -7.605% | -5.243% |
| 前后版本 native/empty / empty | CPU core-ms/tick | +0.093% | +0.143% |
| 同二进制 native A/A / native | P50 | +0.671% | +0.299% |
| 同二进制 native A/A / native | P95 | +0.669% | +0.669% |
| 同二进制 native A/A / native | P99 | -0.160% | -0.322% |
| 同二进制 native A/A / native | CPU core-ms/tick | +0.505% | +0.475% |

逐轮均值倍率的全部五个差异如下，顺序与采集轮次一致：

| 对照 / 模式 | 指标 | 第 1–5 轮差异 |
|---|---|---|
| 前后版本 native/empty / native | P50 | -0.414%, -2.277%, +2.300%, -2.224%, +2.032% |
| 前后版本 native/empty / native | P95 | -2.755%, -6.602%, +1.281%, -0.557%, -0.321% |
| 前后版本 native/empty / native | P99 | -13.686%, -28.503%, +2.668%, +1.341%, -12.426% |
| 前后版本 native/empty / native | CPU core-ms/tick | -0.502%, -2.652%, +2.148%, -1.531%, +1.146% |
| 前后版本 native/empty / empty | P50 | +1.366%, +0.640%, -2.547%, +0.953%, +0.200% |
| 前后版本 native/empty / empty | P95 | -3.385%, +2.111%, -3.351%, +0.001%, -0.739% |
| 前后版本 native/empty / empty | P99 | -7.865%, +29.297%, -25.939%, -5.006%, -7.605% |
| 前后版本 native/empty / empty | CPU core-ms/tick | +0.093%, +0.748%, -2.609%, +0.449%, +0.091% |
| 同二进制 native A/A / native | P50 | +0.671%, +1.271%, +1.115%, -0.281%, +0.285% |
| 同二进制 native A/A / native | P95 | +0.511%, +2.722%, +2.248%, -0.992%, +0.669% |
| 同二进制 native A/A / native | P99 | -0.160%, -6.487%, +7.884%, -1.689%, +0.395% |
| 同二进制 native A/A / native | CPU core-ms/tick | +0.505%, +0.799%, +1.016%, -0.563%, +0.258% |

native 前后 CPU 和 P50 五轮均三降两升，未形成一致升高方向。empty CPU 四升一降、幅度不一，其中位差较小；P99 的轮次差异依然很大。同二进制 native CPU 四升一降，说明这个后续采集时段即便二进制相同也会产生差异。这里描述样本方向，没有据此做统计显著性判断。

## 验证与结论边界

[工具测试](../../../scripts/tests/test_mod_perf_compare.py)与 baseline 测试合计 33 项通过，覆盖默认次序、ABBA/BAAB 和独立诊断、合成线性漂移抵消、相关对与轮次分离、缺失窗口、零基线、非法编号、身份/状态/期限失败和证据路径保护。另将本轮 60 个报告逐一重验，并从原始窗口重算汇总。命令、源文件/二进制/报告摘要及文档检查见[验证清单](mod-native-balanced-validation-2026-10-04.json)。

本轮未重跑 Rust 全量检查或 screeps-ok。Rust 源码与二进制摘要已核对仍与[看门狗批次验证](mod-watchdog-notify-validation-2026-10-04.json)一致；该批次已通过 593 项 Rust、19 项 JavaScript、release 资源测试及兼容性棘轮（1854 passed / 153 failed / 907 skipped，red=0）。这些是同一实现的先前验收，不能写成本轮重新执行。

本次只完成测量工具和局部长窗口证据。500/3000/五轮满足窗口设置，但桌面环境、纯结算合成世界及该探针未覆盖的玩家投影/执行、存储和网络都限制结论。完整链此前每对 CPU 降低的证据支持保留看门狗优化；native 的历史差异仍待稳定环境或进一步可控实验归因。下一步应回到真实玩家与 Mod 的执行、结算、失败重试闭环验收，避免继续仅凭合成 link 数字评估整体架构。
