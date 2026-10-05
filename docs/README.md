# 项目文档

文档分别说明当前实现、目标设计、技术决策和历史证据。阅读时先确认内容属于哪一类；采用某个方案或完成某项实验，不代表对应功能已经实现。

## 按问题阅读

| 问题 | 入口 |
|---|---|
| 项目现在能运行什么？ | [项目 README](../README.md) |
| 模块如何协作？ | [总体架构](architecture/overall.md) |
| 一轮玩家执行和结算如何完成，失败怎样重试？ | [游戏循环](architecture/game-loop.md)、[tick 契约](architecture/tick-contract.md) |
| 世界数据有哪些表示，快照怎样管理？ | [总体架构的数据流](architecture/overall.md#数据流与状态边界)、[快照契约](architecture/snapshot-contract.md) |
| 玩家行为应满足什么要求，如何验证？ | [兼容性设计](designs/compatibility.md)、[验收工具](../compat/README.md) |
| 为什么采用 Rust、V8 和当前模块边界？ | [决策索引](decisions/README.md) |
| 当前 Mod 处理链怎样执行？ | [Mod 处理链执行](architecture/mod-execution.md) |
| 独立 JS Mod 距完整玩法还有哪些缺口，接下来怎样开发？ | [现状复查](research/09-mod-capability-current-audit.md)、[完整玩法计划](plans/mod-complete-gameplay.md) |
| 官方玩法怎样拆成可选 Mod，如何选择内容和处理存量数据？ | [官方内容包决策](decisions/ADR-016-official-content-packs.md)、[内容包设计](designs/content-packs.md) |
| Mod、前端和开发工具计划如何工作？ | [功能设计](designs/README.md)、[前端架构](architecture/frontend.md) |
| 如何写清楚注释和术语？ | [注释规范](contributing/comment-style.md)、[术语词典](glossary.md) |

## 文档分工

| 目录或文件 | 用途 | 维护方式 |
|---|---|---|
| [architecture/](architecture/README.md) | 模块机制、接口边界和目标架构 | 实现变化时同步维护，明确尚未实现的设计 |
| [designs/](designs/README.md) | 玩家行为、功能要求和验收条件 | 保留来源与兼容性例外，区分要求和实装 |
| [decisions/](decisions/README.md) | 技术决策及取舍理由 | 保留决策背景；方案改变时注明替代关系 |
| [roadmap.md](roadmap.md) | 开发范围和里程碑验收 | 根据进展更新，不作为接口使用说明 |
| [plans/](plans/README.md) | 开发拆分、实施过程和阶段验收 | 完成后保留历史记录，现行机制回写架构文档 |
| [research/](research/README.md) | 调研、实验与原始证据 | 保留实验环境、版本和结果，不追改历史结论 |
| [contributing/comment-style.md](contributing/comment-style.md) | 注释内容和表达规则 | 保持稳定原则，示例须说明适用范围 |

## 使用与维护

当前接口行为应结合源码和测试核实。架构文档中的目标设计须明确写出尚未具备的能力；单项实验数字不能替代完整系统的性能结论。

正文优先说明对象、条件、结果和限制。开发批次、评审轮次和修复过程保留在计划、研究记录或提交说明中；现行文档通过链接引用其依据，不要求读者先理解开发历史。

命令示例说明执行目录、依赖准备和输出位置；测试统计优先链接机器报告，避免多处复制容易过期的数字。修改文档后运行 `node scripts/check-doc-links.mjs` 检查本地内联链接；该工具不是完整的 Markdown 校验器，重复标题等情况仍需人工核对。
