# screeps-ok 初期质量评估

本页保存提交 `6c01eac` 中 ADR-006 的详细评估与证据段，仅调整本地链接。项目规模、版本、风险和补测安排属于当时的记录，不能直接作为当前覆盖或完成状态。原文的纯文本路径和章节编号按原记录理解。现行决策见 [ADR-006](../../decisions/ADR-006-screeps-ok-ci.md)，当前工具规则见[验收说明](../../../compat/README.md)。

## 2026-09-27 深度评估（回应用户"是否 demo 级项目"的质疑）

对仓库做了全面的质量评审，结论：**(b) 有真实价值但需加固**，采用并加固显著优于自建同等 harness（自建估 4~6 人月，最大成本不是写测试，而是逐条确认"vanilla 真实行为是什么"——它的每条 gap 都引了官方引擎源码行号，这个基准自建无从跳过）。

**不是 demo 级的证据**：160 个测试文件/4.1 万行/2897 个展开用例（静态 1388 个调用点 + 170 个矩阵循环，数字经独立复核可对账——status.md 逐条列名，恰好 2897/2642 条）；断言精度高（精确 `toBe` 2338 处 vs 弱断言仅 147 处且被成文规则约束）；覆盖官方语义深水区（intent 覆盖仍返回 OK、cancelOrder 退费、fatigue 精确数学含 boost 矩阵、tower 射程衰减、市场手续费按公开公式重算、memhack 未文档化行为、对象精确键集比对）；反作弊机制成体系（禁 skip 静态扫描、参考引擎必须零 skip、期望值禁止从被测引擎读回、parity 缺口有回归陷阱）；vanilla 基准跑的是**真实官方引擎**（官方 npm 包锁定版本 + 子进程，不是 mock），xxscreeps 侧跑真实 isolated-vm 沙箱；数字由 CI 自动生成（最近一次 2026-09-27）。6 个月 453 commits、单作者（Matt Wise）、AI 辅助但治理文档严格且确被执行。

**风险与缺口（据此加固）**：单维护者、2 star、alpha 未发 npm（bus factor=1，契约可能 breaking）；vanilla 适配器用字符串补丁改 `@screeps/driver` 内部源码（driver 升级即断）；**CPU 计费行为（bucket/tickLimit/超时强杀）、`Game.cpu.halt()`、WASM 模块加载均无行为测试**（MMO 面 pixel/unlock 属有意排除）；power 效果与市场生命周期只有 vanilla 单侧验证；少量形状级测试注水（多 shard 身份、堆统计）。

**加固措施（并入路线图 M0）**：
1. 先独立复现：pin 上游 commit，本地跑 `npm run parity` 对账 2897/2642；
2. 我们的适配器 `runPlayer` 必须走真实玩家沙箱（禁止测试捷径），capability 如实声明（估 800~1500 行 TS，参照 xxscreeps 适配器量级）；
3. fork/PR 补缺口测试：CPU bucket/tickLimit/超时强杀、`cpu.halt()`、WASM 模块——CPU 方向我们已有 bucket_sim 实验基础；
4. 护栏：锁定 `@screeps/*` 依赖版本；上游 driver/engine 变动后重跑全套；我们的 fork 里维护 gap 清单。

## 证据

- 本地克隆：`~/Desktop/projects/sources/screeps/screeps-ok/`（官方原版基准 2897 用例通过、xxscreeps 2642 用例通过——它自己也用这个套件衡量兼容度；453 commits / 2026-03-31 起 / 单作者 Matt Wise / MIT）
- [兼容性红线](../../designs/compatibility.md) 第四节"验收关卡"
