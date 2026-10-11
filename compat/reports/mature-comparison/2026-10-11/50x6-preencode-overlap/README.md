# 50×6 房间级编码-结算重叠（L1a）交付证据

日期：2026-10-10（会话跨日，按台账批次记 2026-10-11 目录归档）
基线：HEAD `7ddb0c89`（row-dirty 已交付，`/tmp/kkscreeps-dirty-rel`，release 72MB）
产物：本提交工作树（release `/tmp/kkscreeps-l1a-final`）
夹具：50 玩家 × 6 成熟房（`/tmp/l1b-fixture.json`），warmup=5、ticks=100、concurrency=4

## 结论

共享 full 块编码从 project 相移入 settle 相的编码线程池（2 worker，
产物写共享 `Mutex<HashMap>`，下一拍共享构建逐房懒消费）。冻结二进制
ABBA（2 丢弃窗 + O,N,N,O）：

- wall P50 均值 0.869×（−13.1%），两相邻对方向一致（0.869 / 1.149）
- project 0.381×（218→82ms，共享块编码几乎全免）
- settle 1.291×（+40ms：快照克隆 ~7ms + 内存带宽争用）
- CPU 1.174×（+~300 core-ms），RSS 峰 1.014×

进程内交替 A/B（`compare-probe --interleave 25`，200 tick 同进程按块
切换开关，机器状态对两模式同权，判定主证据）：

| 轮次 | wall P50 on/off | Δ | CPU mean on/off |
|---|---|---|---|
| 快速档 | 304.6 / 404.9 | −100ms（−24.8%） | 1312 / 1215（+8.0%） |
| 慢速档 | 424.5 / 514.2 | −89.7ms（−17.5%） | 1700 / 1474（+15.3%） |

run 相回归在交替设计下消失（+2ms），证明此前窗口级 ABBA 中 run 相
"+40~85ms" 全部为机器状态双峰噪声（本轮会话曾受用户播放视频污染，
安静机器复测后以交替 A/B 为准）。

## 结构验证

计数器插桩（交付前移除）：hit=300/300、miss=0、evict=0、leftover=0
——预编码全量命中、零双算、零深比较剔除、零残留。偶发全量剔除轮
（约每 25 tick 一轮市场等晚期阶段变异）由消费点 `*entry.source ==
*state` 深比较兜底，回退内联编码，p95 无尖峰。

## 公平性

CPU +8~17%（worker 疑似调度到能效核，同样编码量占核更久）。本机
4P+6E 下实测平均并行度 ~3.9/10，未饱和，wall 收益兑现；按 2026-10-11
仲裁规则属"CPU 小超 + wall 大赢"档：全机 ABBA 已做；taskset 2 核亲和
窗在 darwin 不可用，分析结论——2 核 CPU-bound 部署上 +8~17% CPU 会
反噬为同幅度 wall 回退（吞吐受 CPU 总量约束）。部署基准（2G 轻量服
务器）走容量口径，不与本 wall 基准混同；若未来 2 核容量测试受限，优
先尝试 worker QoS 提升（P 核驻留）再考虑 worker 数降为 1。

## 证伪记录（v1，同批）

同步 flush 取回版：settle 2.05×、CPU 1.27×、wall 持平——单编码线程
全量 ~140-155ms 放不进 ~110ms settle 窗，flush 阻塞等待 42-56ms；
插桩归因（克隆 6.7ms/编码 140-155ms/深比较 5.5ms/flush 42-56ms）。
教训：重叠型优化的收益上限 = min(被隐藏相时长, 窗口时长)，且同步取
回点会把超额部分变成主线程阻塞。

## 文件

- `interleave-ab-run1.json` / `interleave-ab-run2.json`：交替 A/B 两轮
- `abba-summary.json`：冻结二进制 ABBA 汇总（O,N,N,O + 2 丢弃窗）

复现：`target/release/kkscreeps compare-probe --fixture <fixture> --warmup 5
--ticks 200 --concurrency 4 --interleave 25 --report out.json`，读
`interleave_ab` 节；ABBA 协议见 `50x6-row-dirty/README.md`。
