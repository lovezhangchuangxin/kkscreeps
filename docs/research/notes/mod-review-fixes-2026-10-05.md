# Mod 规则与资源复审修复（2026-10-05）

基于 d0d593b，修复 N1d/N1e/N2a 复审中复现的三个问题；历史批次笔记与原始报告保留，当前契约见[规则读取](../../architecture/mod-rules.md)和[资源目录](../../architecture/mod-resource-catalog.md)。验证命令、二进制身份及门禁计数见[机器汇总](mod-review-fixes-2026-10-05.json)。

## 修复与回归

1. SDK 的 installRules 原来边验证边写入共享 Map，失败后仍留有部分条目。包导入 seal/installRules，捕获含合法虚构规则及坏条目的安装异常后，宿主注册能成功，执行期能读到冻结计划外的值。现在先构造并完整验证独立规则表，全部成功后替换共享引用；同时检查稳定键形状。回归覆盖后续畸形条目、重复键、getter 抛错、非法稳定键四种失败，真实内建规则仍可读，虚构键始终报未知规则。
2. pickup 原来只检查对象、距离与容量，没有资源目录准入。现在在改写两端前拒绝目录外资源。回归覆盖装有其他资源时的未声明键、未知普通资源名，以及无 Mod 目录时的命名空间资源；creep 库存保持零，掉落堆只受正常对象阶段衰减，不能被拾取扣减或移除。既有声明资源 drop/pickup 正向闭环继续通过。
3. 规则文档示例 tick 修改 value，但 schema 只声明 x/y，实际运行报未声明字段。补充 u32/default=0 声明，并说明 player.js 绑定要求。CLI 回归直接读取文档的 JS 示例，配置最小玩家绑定，在 JSON/KKPV 两通路各运行 9 tick，核对 tick 0/4/8 更新后 value=3。

新增回归位于[宿主规则读取](../../../crates/kk-mod-host/tests/register_tests/rules_read.rs)、[资源结算](../../../crates/kk-engine/tests/settle_tests/mod_resource_family.rs)及[CLI 规则场景](../../../crates/kk-cli/tests/sim_tests/rules.rs)。定向结果为宿主 5 项、资源结算 6 项、CLI 规则场景 2 项通过。

## 提交前验证

重新构建 release CLI，并以 KKSCREEPS_BIN 显式指定该二进制执行 pnpm run check。按 compat/README.md 准备固定 screeps-ok、重建适配器并运行 ratchet；[兼容性报告](../../../compat/reports/kkscreeps.json)须与本次基线逐项核对，不能只依据总通过数。完整结果归档在机器汇总。

完整检查通过：Rust 793 项通过/0 失败/11 项忽略，适配器 14 项通过；格式、Clippy、272 个 Markdown/1571 个本地链接与 380 个源码/测试文件规模门禁通过。兼容性共 2914 项，1918 通过/89 失败/907 跳过，满足 floor 1918；与批次开始 d0d593b 的所有用例身份及状态相同，相对 ratchet 的 HEAD~1 转红/转绿均为 0，没有新增豁免。

同一 release CLI 上重跑原规则残留复现，虚构规则读取现在走未知键故障，整轮提交数为零、对象 value 保持零。文档示例 JSON/KKPV 各 9 tick 后 value=3；正常资源闭环场景两通路各 100 tick 全断言通过、玩家错误数为零，最终世界及组合报告完全一致。

本轮不扩展规则修改、资源生产、市场/跨房或内容迁移；仍沿原有冻结计划、正常衰减和候选提交边界执行。
