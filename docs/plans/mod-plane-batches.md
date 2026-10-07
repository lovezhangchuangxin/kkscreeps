# 贡献平面方向

贡献平面的架构选择见 [ADR-014](../decisions/ADR-014-contribution-plane.md) 和 [ADR-015](../decisions/ADR-015-mod-chain-execution.md)。旧 A–G 批次的实施与测量过程已[归档](archive/mod-plane-batches.md)，不继续复制当时的缺口、门禁数字或交接指令。当前执行只维护[任务台账](mod-next-batches.md)，长期验收以[完整玩法计划](mod-complete-gameplay.md)为准。

| 长期方向 | 当前范围入口 |
|---|---|
| 开放领域定义及 JS 玩法 | 完整玩法 N1/N2；各领域缺口按 G01–G18 分片 |
| 性能与真实规则 | [性能验收](../architecture/performance-acceptance.md)及 G30 滚动对照；默认参考、官方预设和第三方增量分别验收 |
| 保存恢复与耐久 | G22–G27；新增写入域当片保存恢复，数据库/outbox 仍需耐久确认与崩溃验证 |
| 网络与客户端贡献 | G28/G29；先实现 M3 服务和客户端基础，再开放 provider/renderer 等贡献 |
| 完整能力收口 | G19–G21/G30；覆盖世界生命周期、默认内容和完整能力矩阵，不能以少数样例替代 |

候选、VM 生命周期与编排各有所有者，统一发布点保持。Mod 扩展与 vanilla 兼容推进可穿插；任务编号用于追溯，不规定机械串行顺序。测试、构建与文档要求统一见 [AGENTS.md](../../AGENTS.md)。
