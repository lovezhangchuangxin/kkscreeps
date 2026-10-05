# 技术架构

本目录说明模块职责、执行流程和架构约束，同时保留尚未实现的目标设计。先从[总体架构](overall.md)了解当前调用链；设计已被采用、实验可行或接口已预留，都不代表对应能力已完整接入。

## 文档索引

| 文档 | 内容 |
|---|---|
| [overall.md](overall.md) | 当前运行入口、模块职责、实际依赖和数据表示；区分预留模块与目标服务 |
| [source-organization.md](source-organization.md) | 当前源码模块导航、测试分组与文件规模门禁 |
| [game-loop.md](game-loop.md) | 当前玩家执行与结算流程，以及并行流水线的目标 |
| [mod-execution.md](mod-execution.md) | JS/native 组合链、候选访问与当前支持范围 |
| [mod-composition.md](mod-composition.md) | JS handler 依赖、冻结/活动链、规则来源、停用闭包及 mod inspect |
| [mod-rules.md](mod-rules.md) | 玩法 JS 读取冻结有效规则值：可用范围、错误行为、数值语义与恢复/停用一致性 |
| [mod-content-selection.md](mod-content-selection.md) | 新世界启动期的显式包选择：依赖展开、排除冲突、实际内容身份与恢复一致性 |
| [mod-objects.md](mod-objects.md) | 同房候选对象查询、创建/更新/删除、权限与局部拒绝恢复 |
| [mod-object-tick.md](mod-object-tick.md) | 自有对象同步生命周期、处理链、遍历资格与整轮失败恢复 |
| [store-rules.md](store-rules.md) | 仓库容量的冻结规则、JS 数值覆盖与实际消费者 |
| [creep-resource-slots.md](creep-resource-slots.md) | creep 转入/提取动作链、只读候选面与局部拒绝恢复 |
| [creep-move-slot.md](creep-move-slot.md) | creep 移动动作槽、受控请求能力与按执行者粒度的拒绝恢复 |
| [creep-combat-slot.md](creep-combat-slot.md) | creep 战斗动作槽、目标改向/取消能力与延迟队列/结构摧毁的拒绝恢复 |
| [creep-build-slot.md](creep-build-slot.md) | creep 建造族动作槽（build/repair/dismantle）、目标改向/取消能力与完工变换/掉落面的拒绝恢复 |
| [mod-resources.md](mod-resources.md) | Mod 同房库存读取、精确转账与损耗、多对象拒绝日志 |
| [mod-resource-catalog.md](mod-resource-catalog.md) | 命名空间资源声明、冻结目录、玩家常量同步与市场/跨房显式拒绝 |
| [tick-contract.md](tick-contract.md) | 已提交状态、候选结果、失败重试与目标任务合并规则 |
| [sandbox.md](sandbox.md) | 玩家沙箱、执行预算、内存限制与模块加载的机制和目标 |
| [local-simulation.md](local-simulation.md) | 已实现的 sim run：场景、模块、断言、报告与失败退出边界 |
| [checkpoints.md](checkpoints.md) | 完整已提交状态清单、单文件原子发布、身份核验与 sim 冷恢复 |
| [storage.md](storage.md) | 二进制快照、检查点、网络传输与持久化的目标设计 |
| [snapshot-contract.md](snapshot-contract.md) | 共享视图的布局、生命周期、校验和恢复要求 |
| [performance-acceptance.md](performance-acceptance.md) | 端到端性能场景、测量方法和验收目标 |
| [frontend.md](frontend.md) | 尚未实现的 Web 客户端、渲染器与协议类型设计 |

## 当前调用链

```text
兼容性适配器 / 命令行工具
  → kk-runtime：投影世界、调用玩家线程、管理失败缓存和提交
      → kk-runner：V8 执行、API 预检查、指令与写回结果
      → kk-engine：房间和市场结算、候选验证与发布
          → kk-world：世界与对象状态、查询及沙箱数据投影
              → kk-schema：字段与二进制编码
```

这张图表示主要运行职责，完整的编译依赖见[总体架构](overall.md#实际依赖)。目前不经过 HTTP 服务或数据库；沙箱数据默认通过 JSON 传递，也可选择 KKPV。可信玩法链由 runtime 连接 kk-mod-host，执行边界见 [Mod 执行](mod-execution.md)。

## 架构原则

1. 默认玩法以玩家行为兼容为约束，通过测试报告和已知差异说明实际覆盖范围。
2. 房间是结算和快照的重要单元；候选工作状态与已提交状态分开管理。
3. 优化数据复制和任务等待前，先明确生命周期、视野权限和失败边界，再用端到端基准衡量收益。
4. 完整服务以单进程为默认目标；沙箱进程隔离和跨机部署需要额外的通信、故障恢复与安全设计，当前尚未实现。

技术选择的原因见[决策记录](../decisions/README.md)，实验依据见[研究记录](../research/README.md)。
