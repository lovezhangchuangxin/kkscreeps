# 实施方案

本目录保存里程碑和专项工作的范围、技术拆分、验收条件及执行记录。方案中的代码位置、统计和状态属于相应阶段；完成记录不能替代当前机制说明，早期“待确认”或“执行中”字样也不能直接作为今天的进展判断。

现行职责与机制见[架构文档](../architecture/README.md)，玩家行为与工具要求见[功能设计](../designs/README.md)，采用的技术选择见[决策记录](../decisions/README.md)，里程碑验收见[路线图](../roadmap.md)。

## 方案索引

| 工作 | 文档 | 内容与阅读范围 |
|---|---|---|
| M1 引擎骨架 | [m1-engine-skeleton.md](m1-engine-skeleton.md) | 类型化世界、单房间结算、嵌入接口和 CI 基线的原始方案；定型结论见 ADR-011 |
| M2 单机可玩 | [m2-playable.md](m2-playable.md) | 沙箱与玩家 API、2A/2B 验收、存储和流水线的总体拆分 |
| B0 运行协调 | [b0-runtime-coordinator.md](b0-runtime-coordinator.md) | 正确性问题、候选状态和统一提交要求 |
| B0 实施记录 | [b0-slices.md](b0-slices.md) | 接口规格、切片与评审修复；须结合后续失败恢复修改阅读 |
| B1 结构整理 | [b1-structure-governance.md](b1-structure-governance.md) | 文件拆分及保持行为的核对方法，文件规模属于当时的测量 |
| M2B 前置工作 | [m2b-prerequisite.md](m2b-prerequisite.md) | 世界来源接口、版本、地形缓存和编码宏；包含对象投影缓存收益为负的实验 |
| B0 外部复审修复 | [b0-6-external-review-clearance.md](b0-6-external-review-clearance.md) | 失败轮缓存、结算时间与停跑回充的修复依据 |
| 双房双玩家验证 | [dual-room-live-fire.md](dual-room-live-fire.md) | 跨房竞争、失败恢复、生命周期和性能夹具的阶段验证 |
| M2B 玩家 API 扩展 | [m2b-expansion.md](m2b-expansion.md) | 当时的 API 盘点、实施与差异登记；盘点状态不自动等于完整兼容 |
| terminal 与 market | [2b-terminal-market.md](2b-terminal-market.md) | 订单、跨房资源转移、credits 与相关测试的专项实现记录 |
| 贡献平面与 JS Mod | [mod-plane-batches.md](mod-plane-batches.md) | ADR-014 的事务、并发、投影与扩展实施方向；确认方案不代表已经交付 |

## 使用原则

实施前核对源码和最新验收报告，再使用旧方案中的路径、数量和假设。已完成的方案保留执行记录；新发现的差异应写明证据和后续处置，避免把历史正文改成无法追溯的当前状态表。

经确认的架构结论写入 ADR，实现机制同步到架构文档。测试统计引用对应机器报告，局部实验注明环境与负载；方案要求的验证范围和实际完成的验证分别记录。
