# 功能设计

本目录定义玩家可见行为、工具使用方式和验收要求。默认玩法以[兼容性要求](compatibility.md)为约束；服主显式启用的玩法 Mod 可以改变规则，须按 [ADR-007](../decisions/ADR-007-mod-system.md)说明能力与版本。

设计文档提出要求，不代表功能已实现。当前可用入口与模块边界见[项目 README](../../README.md)和[总体架构](../architecture/overall.md)。

## 文档索引

| 文档 | 内容与适用范围 |
|---|---|
| [compatibility.md](compatibility.md) | 默认玩法的兼容性要求、例外和差异记录；具体覆盖率以验收报告为准 |
| [dev-kit.md](dev-kit.md) | 本地编写、模拟、测试、调试和部署的目标流程；完整开发套件尚未实现 |
| [pathfinder-terrain.md](pathfinder-terrain.md) | D2 寻路地形直通：PathFinder.search/findPath/moveTo 接原生搜索的接线架构、玩家形状与已知分歧；roomCallback 归回调文档 |
| [pathfinder-callback.md](pathfinder-callback.md) | 寻路回调与异常行为的对照，以及回调通道的接线设计；地形直通已由 pathfinder-terrain.md 落地，roomCallback/costCallback 尚未接线 |
| [mod-system.md](mod-system.md) | JS Mod 的贡献、规则替换、事务、投影和能力验收设计；未提供完整 Mod 执行通路 |
| [domain-projection.md](domain-projection.md) | B 阶段的冻结定义、有效规则、意图入口与授权投影；以 link 为首个接入家族，包含实施与验收要求 |

## 设计与实现的核对

功能设计先说明用户行为和正确性要求，再说明实现方案。涉及上游行为时保留仓库、版本和文件或符号来源；有限场景的实验结果须注明其覆盖范围。

实现状态应说明具体入口和限制，避免用开发批次代替解释。与默认玩法兼容要求不一致的行为须登记原因、影响和验证方法。当前差异见[兼容性文档](compatibility.md)，测试与回归门禁见[验收工具](../../compat/README.md)。

开发任务与顺序见[路线图](../roadmap.md)和[实施计划](../plans/README.md)。回放、运营、多分片等未完成设计不应被描述成已有的存储或网络能力。
