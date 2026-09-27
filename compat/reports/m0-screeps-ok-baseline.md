# M0 报告：screeps-ok 适配器首份通过率基线

> 2026-09-27。M0 验收关卡之一：**适配器能执行套件并输出通过率报告**（不要求用例通过——
> 用例测的是游戏行为，引擎从 M1 起才会开始通过）。本基线是 M1 棘轮机制的起点。

## 结论：M0 关卡达成

全量 **2914** 用例执行完毕、宿主与适配器全程稳定、JSON 报告产出：

| 指标 | 数值 | 含义 |
|---|---|---|
| **passed** | **229** | 哑世界 + 真实沙箱已经能真实通过的用例 |
| failed | 1659 | 等待引擎（M1+）的游戏行为用例——正是后续里程碑的工作清单 |
| skipped | 1026 | capability 门如实声明（M0 全 false）+ 上游 skip 规则 |

对照锚点：vanilla 2897 通过（其中 10 expected-fail）、xxscreeps 2642 通过（77 ef）——
我们的 229 是**零游戏规则前提下**的起步线，M1 棘轮从 229 起只增不减。

## 通过用例的构成（有信息量的部分）

| 分区 | 通过 | 说明 |
|---|---|---|
| 00-adapter-contract | 63/157 | 适配器契约本身（哑世界设计的目标收益：createShard/placeXxx 精确存取/runPlayer 三分类错误模型/tick 语义） |
| 00-framework | 59/61 | 套件框架层 |
| 22-roomposition | 17/31 | 桩实现的纯计算方法（getRangeTo/getDirectionTo 等）直接过了官方语义对照 |
| 27-undocumented | 17/107 | 未文档化行为区 |
| 25-memory | 8/22 | Memory 懒解析/写回语义 |
| 06-controller | 11/263 | 控制器初态投影 |
| 30-cpu-runtime | 3/4 | getUsed 口径 |
| 其余 16 个分区 | 51 | 零星通过（多为设置/读取类用例） |

## 报告与复现

- JSON 报告：`compat/reports/kkscreeps.json`（vitest json reporter 产物）
- 复现：`compat/scripts/run-suite.sh`（前置见 compat/README.md，全量约 3 分钟）
- 环境：release 二进制、Node 24.11.0、上游 pin `34a630e`

## 过程记录

- 首跑（类全局补全前）：217 passed / 1671 failed / 1026 skipped；
- 终跑（bot 验收补全类全局/类层级/原型链接/具体 API 桩后）：229 / 1659 / 1026（+12）。
  桩环境的补全同时提升 screeps-ok 通过数与 bot 兼容性——两个验收面互相印证。

## M1 起的棘轮机制（本基线的用途）

- 每次合入跑 `compat/scripts/run-suite.sh`，passed 数不得低于上次登记值；
- failed 用例随引擎实装逐区转绿（01-movement/03-harvesting 等按 M1 引擎骨架的推进顺序）；
- capability 从 false 逐项翻真时，对应 skip 池转入 passed/failed 统计，需在翻真 commit
  里登记新的基线值与理由。
