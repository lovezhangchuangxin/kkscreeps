# A6 性能观测点首次实测记录（小规模负载）

> **口径注记（2026-09-30，双房批片 1 夹具修复）**：本表数字为**空转夹具**
> 产物——B0-6 外部评审复现并经本仓探针证实：当时的夹具布局（creep
> (24..26,26)/source(10,10)(40,40)/controller(1,1)）全部动作超距，harvest/
> upgrade 全被 API 预检拒绝、零 intent——上表「负载形态」的「孵化至 6 只
> 后维持 harvest/upgrade 循环」自述不成立（spawn 雁流下种群可达 6，但
> 全部动作空转）。数字仅作 A6 时点的**架构观测点形态**留档（project 相
> 仍有效——投影构建与动作无关；run 相为无 intent 的脚本执行；settle 相
> 为零 intent 结算）。夹具已修复（对象摆进作用距离+出生 directions 控制
> +负载自检），当日新基线与 A/B 归因见
> [双房批方案 §7](../../docs/plans/dual-room-live-fire.md)。

- **日期**：2026-09-29
- **工具**：`kkscreeps perf-probe`（A6 交付；驱动 EmbedEngine 完整闭环——per-viewer
  world_json 投影 → deno_core 真沙箱执行玩家代码 → intent 清洗 → settle 结算）
- **负载形态**：每玩家一间 rcl4 自有房（controller + 2 source + 1 spawn + 3 creep，开局
  玩家代码孵化至 6 只 creep 上限后维持 harvest/upgrade 循环）；玩家代码走真实 API 面
  （find/FIND_SOURCES/spawnCreep/harvest/upgradeController）
- **环境**：macOS 15（darwin 24.6.0，arm64，Apple Silicon 桌面机）；release 构建；
  预热 100 tick 后采集 1000 tick；机器为日常开发机（非专用基准机，绝对值仅供参考）
- **口径**：[performance-acceptance](../../docs/architecture/performance-acceptance.md)
  「阶段耗时」（M2A 架构验收重点：玩家 API → intent → 结算闭环及快照读取路径）；
  采样为 `std::time::Instant`（单次读表 ~20ns，每 tick 每玩家 6 次采样，开销可忽略；
  max 尖峰含 GC/OS 调度，正式基线以多次独立运行中位数为准）

## 实测数据（p50 / p95 / max，单位 ms/tick）

| 配置 | 世界投影构建 | 玩家运行 | 结算 | tick 墙钟 |
|---|---|---|---|---|
| 2 玩家（2 房） | 0.164 / 0.195 / 0.395 | 0.520 / 0.720 / 1.757 | 0.008 / 0.015 / 0.053 | 0.813 / 1.054 / 2.075 |
| 4 玩家（4 房） | 0.517 / 0.736 / 2.757 | 1.665 / 2.592 / 42.334 | 0.023 / 0.053 / 0.221 | 2.451 / 3.562 / 70.414 |

JSON 原始报告：`compat/reports/perf-probe.json`（2 玩家）。

## 初步观察（不作为优化结论，仅为 M2B 基准的输入）

1. **投影构建已是显著占比**（2 玩家 ~20%、4 玩家 ~21% 的 p50 墙钟）：印证
   performance-acceptance 的预判——"每玩家每 tick 完整世界 JSON 仍是必经数据面"是
   M2A 已登记的未兑现架构目标，seam 先行（ADR-013 决策 2）后由 M2B 直读兑现。
2. **玩家运行占大头且超线性增长**（2→4 玩家 3.2×）：包含 creep 数量增长（孵化到
   上限）与 V8 实例数增长的复合效应；max 尖峰（42ms/70ms）为偶发（GC/调度），
   p95 形态健康。
3. **结算占比极小**（<1%）：当前负载下结算不是瓶颈；B0 运行协调层重构时的回归
   对比基线即本表（perf-probe 同口径可重复）。
4. 本记录是**小规模架构验收实测**，不替代正式场景基线（场景 A 空闲世界 / C 多房
   多玩家 × 3 实现 × ≥5 次独立运行）——后者随 M2B 基准批次用同一观测点采集。

## 复现命令

```sh
cargo build --release
./target/release/kkscreeps perf-probe --ticks 1000 --warmup 100 --players 2 --report compat/reports/perf-probe.json
```
