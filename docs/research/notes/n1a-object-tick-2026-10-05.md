# N1a 自有对象 tick 与 Mod 场景（2026-10-05）

本片基于 `e0bed47`，实现[完整玩法计划](../../plans/mod-complete-gameplay.md)的第一片。现行 SDK 见[自有对象 tick](../../architecture/mod-object-tick.md)，场景输入与报告见[本地模拟](../../architecture/local-simulation.md)。

## 交付范围

- `api.objects.define(typeKey, { tick(ctx) { ... } })` 注册自有 ObjectTick 槽和 process，贯通冻结、装配和真实房间 tick 派发；纯 Rust 扩展安装与 JS 整槽宿主均可使用。
- 动作后候选字段、命名空间 state、同步处理链与位置校验沿原通路写入；生命周期不允许 decline，没有资源/移动/战斗/建造域能力。
- sim v1 可显式装载 Mod 根目录与配置、初始化自有对象、断言 namespace state；报告记录包/配置/玩家 API/代码摘要、真实 PlanHash、停用集合及结构化 Mod 故障。
- 官方 `clockwork` 原型只使用公共 SDK，带独立玩家 API；样例在普通作者目录也可安装。源码规模门禁增加 mods/，场景夹具纳入 Git 白名单。

尚不包含 preTick、通用对象读写/创建/删除、资源生产、内容预设、默认内容迁移、持久恢复或确定性回放。原型只是自主生命周期验收，不能代表完整生产玩法或全量 xxscreeps 能力已经覆盖。

## 验证

定向测试已通过：runtime 6 个生命周期用例、engine 2 个装配/替换用例、CLI 4 个包加载/身份/双通道/故障报告用例。多参数循环覆盖两条投影通道、双房稳定派发、零玩家意图更新、动作后字段、四种处理链位置、replace 回退/所属包停用、异常/超时/thenable/非法字段/拒绝/领域越权、晚期写回失败和缓存玩家重试。

新构建 release CLI、按脚本准备固定上游 pin `34a630efa42908c117c19c25d2350fa2387bd633` 并显式 build 适配器后，在仓库根目录运行：

```bash
KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check
KKSCREEPS_BIN="$PWD/target/release/kkscreeps" node compat/scripts/ratchet.mjs
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-tick/scenario.json --ticks 2000 --report docs/research/notes/n1a-object-tick-json-2026-10-05.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-tick/scenario.json --ticks 2000 --kkpv --report docs/research/notes/n1a-object-tick-kkpv-2026-10-05.json
```

完整 check 通过格式、workspace Clippy、730 个 Rust 测试（11 个忽略）、JavaScript workspace 测试（适配器 14 个通过）、文档链接和源码规模检查。源码门禁涵盖 334 个主项目源文件/测试，均不超过 1000 行。

双通道各完成 2000/2000 tick，两个玩家各执行成功 2000 次，无错误或未求值轮次，3 条断言全过。最终对象 value 为 4000 和 4007，namespace visits 为 4000，玩家最后读值为 3998 和 4005。最终世界、Mod state、玩家 Memory 和 PlanHash 跨通道一致，报告二进制摘要与测试使用的 release 文件一致。原始报告：[JSON](n1a-object-tick-json-2026-10-05.json)、[KKPV](n1a-object-tick-kkpv-2026-10-05.json)。

完整兼容棘轮为 1918 passed / 89 failed / 907 skipped，总数 2914，地板仍为 1918。相对脚本规定的 HEAD~1 与本片基础 HEAD 报告均无状态变化、无转红；不新增跳过、豁免或调整地板。两条既有豁免未命中，脚本仍给出陈旧警告。全量结果见[套件报告](../../../compat/reports/kkscreeps.json)，汇总与身份见[机器记录](n1a-object-tick-2026-10-05.json)。

推进循环时间只作为场景诊断值；本轮与其他检查并行、未预热，也没有同功能原生/JS 成组测量，不作为跨实现性能或通道优劣结论。

## 提交前复审

核对派发时候选借用释放、仅开放自身 tick、阶段与中心装配门、被 replace 占位的原 process 安装要求、停用掩码、领域能力拒绝、对象/state/玩家结果原子性、包字节与报告身份一致性，以及文档中当前能力和目标的区分。定向复查修正了纯 Rust 生命周期替换后的原绑定余留校验，并将官方 JS 目录纳入原 1000 行门禁。未开放阶段仍显式拒绝，不以有描述符代替真实派发。
