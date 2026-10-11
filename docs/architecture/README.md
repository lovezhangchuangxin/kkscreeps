# 技术架构

本目录说明模块职责、执行流程和架构约束，同时保留尚未实现的目标设计。先从[总体架构](overall.md)了解当前调用链；设计已被采用、实验可行或接口已预留，都不代表对应能力已完整接入。

## 文档索引

| 文档 | 内容 |
|---|---|
| [overall.md](overall.md) | 当前运行入口、模块职责、实际依赖和数据表示；区分预留模块与目标服务 |
| [performance-target.md](performance-target.md) | 2026-10-11 推荐的未实现重构目标：紧凑房间存储、受控变化集、增量视图、依赖调度与完整 Mod 能力 |
| [source-organization.md](source-organization.md) | 当前源码模块导航、测试分组与文件规模门禁 |
| [game-loop.md](game-loop.md) | 当前玩家执行与结算流程，以及并行流水线的目标 |
| [background-services.md](background-services.md) | G27.a 独立后台线程/V8、受控异步生命周期、有限只读探针和作者 CLI；耐久/网络仍待后续 |
| [mod-execution.md](mod-execution.md) | JS/native 组合链、候选访问与当前支持范围 |
| [mod-composition.md](mod-composition.md) | JS handler 依赖、冻结/活动链、规则来源、停用闭包及 mod inspect |
| [mod-rules.md](mod-rules.md) | 玩法 JS 读取冻结有效规则值：可用范围、错误行为、数值语义与恢复/停用一致性 |
| [mod-content-selection.md](mod-content-selection.md) | 新世界启动期的显式包选择：依赖展开、排除冲突、实际内容身份与恢复一致性 |
| [mod-objects.md](mod-objects.md) | 同房候选对象查询、创建/更新/删除、权限与局部拒绝恢复 |
| [mod-object-tick.md](mod-object-tick.md) | 自有对象同步生命周期、处理链、遍历资格与整轮失败恢复 |
| [store-rules.md](store-rules.md) | 仓库容量的冻结规则、JS 数值覆盖与实际消费者 |
| [structure-quota.md](structure-quota.md) | 建筑 RCL 配额规则族：132 条逐型逐级覆盖、玩家表折叠注入与预检/结算同源 |
| [object-store.md](object-store.md) | 自有对象库存：类型级 capacity 声明、store/storeCapacity 信封、六链接纳与字段更新拒绝 |
| [object-durability.md](object-durability.md) | 自有对象耐久与反击：类型级 hitsMax/retaliationRatio 声明、hits/hitsMax 信封、伤害/死亡/预检同源、逐 tick 守卫、声明式反击与 builtin:mod.receiveDamage 结构受击槽 |
| [creep-resource-slots.md](creep-resource-slots.md) | creep 转入/提取动作链、只读候选面与局部拒绝恢复 |
| [creep-move-slot.md](creep-move-slot.md) | creep 移动动作槽、受控请求能力与按执行者粒度的拒绝恢复 |
| [creep-combat-slot.md](creep-combat-slot.md) | creep 战斗动作槽、目标改向/取消能力与延迟队列/结构摧毁的拒绝恢复 |
| [player-precheck-rules.md](player-precheck-rules.md) | 规则驱动双端同源的玩家预检契约：战斗射程四条与扩展意图压制，含可扩展意图盘点 |
| [combat.md](combat.md) | 伤害、治疗、群体伤害、反击与受击的统一契约 |
| [creep-incoming-damage.md](creep-incoming-damage.md) | 原生强化 TOUGH、累计受伤/治疗、原始事件与恢复边界 |
| [movement.md](movement.md) | 移动登记、预检、竞争、准入、障碍与代价的统一契约 |
| [creep-build-slot.md](creep-build-slot.md) | creep 建造族动作槽（build/repair/dismantle）、目标改向/取消能力与完工变换/掉落面的拒绝恢复 |
| [mod-resources.md](mod-resources.md) | Mod 同房库存读取、精确转账与损耗、多对象拒绝日志 |
| [mod-resource-catalog.md](mod-resource-catalog.md) | 命名空间资源声明、冻结目录、玩家常量同步与市场/跨房显式拒绝 |
| [mod-types.md](mod-types.md) | 冻结计划确定性生成玩家面 TypeScript 声明、新鲜度门禁与包外输出守卫 |
| [mod-trace.md](mod-trace.md) | 可选有界的 Mod 运行 trace：槽结果与回退事件、与活动链一致、非回放日志 |
| [tick-contract.md](tick-contract.md) | 已提交状态、候选结果、失败重试与目标任务合并规则 |
| [sandbox.md](sandbox.md) | 玩家沙箱、执行预算、内存限制与模块加载的机制和目标 |
| [local-simulation.md](local-simulation.md) | 已实现的 sim run：场景、模块、断言、报告与失败退出边界；mod init 作者包模板与 mod test 汇总测试 |
| [checkpoints.md](checkpoints.md) | 完整已提交状态清单、单文件原子发布、身份核验、sim 冷恢复与周期保存/恢复选择 |
| [state-inventory.md](state-inventory.md) | 检查点逐域字段清单、版本策略、不持久化域与 schema 键树门禁 |
| [mod-switching.md](mod-switching.md) | 无开放轮次的包集合切换：残留分诊、停用投影、工作者重注入与 sim mods_switch |
| [gameplay-determinism.md](gameplay-determinism.md) | 确定性玩法时间与随机协议：无状态随机流、种子生命周期、Date 映射与非确定边界 |
| [input-log.md](input-log.md) | 持久输入与玩家结果日志：逐轮提交后追加的可校验回放输入源与结算回放核对（G25.a 写入面；G25.b 回放） |
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

- [权威编辑、候选发布与 Rust API 迁移](state-editing.md)
