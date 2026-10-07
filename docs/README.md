# 项目文档

文档分别说明当前实现、目标设计、技术决策和历史证据。阅读时先确认内容属于哪一类；采用某个方案或完成某项实验，不代表对应功能已经实现。

## 按问题阅读

| 问题 | 入口 |
|---|---|
| 项目现在能运行什么？ | [项目 README](../README.md) |
| 模块如何协作？ | [总体架构](architecture/overall.md) |
| 一轮玩家执行和结算如何完成，失败怎样重试？ | [游戏循环](architecture/game-loop.md)、[tick 契约](architecture/tick-contract.md) |
| 源码按什么职责组织，何时拆分模块？ | [源码组织](architecture/source-organization.md) |
| 权威状态怎样安全编辑，Rust API 怎样迁移？ | [状态编辑](architecture/state-editing.md) |
| 世界数据有哪些表示，快照怎样管理？ | [总体架构的数据流](architecture/overall.md#数据流与状态边界)、[快照契约](architecture/snapshot-contract.md) |
| 如何保存完整世界并在重启后继续？ | [基础检查点](architecture/checkpoints.md)（含周期保存与恢复选择）、[状态域清单](architecture/state-inventory.md)、[本地模拟](architecture/local-simulation.md) |
| 运行中的世界如何更换 Mod 包集合？ | [无开放轮次的包切换](architecture/mod-switching.md) |
| Mod 规则代码的随机与时间如何确定且可恢复？ | [确定性玩法时间与随机协议](architecture/gameplay-determinism.md) |
| 如何记录每轮提交的输入以便回放？ | [持久输入与玩家结果日志](architecture/input-log.md) |
| 如何从检查点+日志重放并核对完整状态？ | [输入日志（回放节）](architecture/input-log.md) |
| 玩家行为应满足什么要求，如何验证？ | [兼容性设计](designs/compatibility.md)、[验收工具](../compat/README.md) |
| 为什么采用 Rust、V8 和当前模块边界？ | [决策索引](decisions/README.md) |
| 当前 Mod 处理链和自主对象 tick 怎样执行？ | [Mod 处理链执行](architecture/mod-execution.md)、[自有对象 tick](architecture/mod-object-tick.md)、[对象操作](architecture/mod-objects.md)、[组合诊断](architecture/mod-composition.md)、[有效规则读取](architecture/mod-rules.md)、[新世界内容选择](architecture/mod-content-selection.md)、[资源目录](architecture/mod-resource-catalog.md)、[可建造建筑](architecture/mod-buildings.md)、[战斗领域](architecture/combat.md)、[移动领域](architecture/movement.md)、[玩家预检规则](architecture/player-precheck-rules.md)、[建筑 RCL 配额规则](architecture/structure-quota.md)、[自有对象库存](architecture/object-store.md) |
| 作者如何获得玩家面 TypeScript 声明？ | [玩家类型生成](architecture/mod-types.md) |
| 作者如何看到 Mod 拒绝/故障与回退过程？ | [运行 trace](architecture/mod-trace.md) |
| 独立 JS Mod 距完整玩法还有哪些缺口，接下来怎样开发？ | [当前接手台账](plans/mod-next-batches.md)、[完整玩法计划](plans/mod-complete-gameplay.md)；历史差距分析见[专项复查](research/09-mod-capability-current-audit.md) |
| AI 接手从哪里继续，怎样维护进度？ | [后续任务与接手台账](plans/mod-next-batches.md)、[仓库贡献指南](../AGENTS.md#ai-接手与进度维护) |
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
| [plans/](plans/README.md) | 开发拆分、实施过程和阶段验收 | 当前任务只维护台账；已完成计划移入 archive，现行契约回写架构文档 |
| [research/](research/README.md) | 调研、实验与原始证据 | 保留实验环境、版本和结果，不追改历史结论 |
| [contributing/comment-style.md](contributing/comment-style.md) | 注释内容和表达规则 | 保持稳定原则，示例须说明适用范围 |

## 使用与维护

当前接口行为应结合源码和测试核实。架构文档中的目标设计须明确写出尚未具备的能力；单项实验数字不能替代完整系统的性能结论。

每个领域的现行契约维护唯一入口；普通修复或内部重构不默认新增文档。正文优先说明对象、条件、结果和限制。开发批次、评审轮次和修复过程保留在计划、研究记录或提交说明中；现行文档通过链接引用其依据，不要求读者先理解开发历史。

命令示例说明执行目录、依赖准备和输出位置；测试统计优先链接机器报告，避免多处复制容易过期的数字。修改文档后运行 `node scripts/check-doc-links.mjs` 检查本地内联链接；检查范围由 Git 跟踪及未忽略的新 Markdown 决定，不扫描个人工作副本；该工具不是完整的 Markdown 校验器，重复标题等情况仍需人工核对。
