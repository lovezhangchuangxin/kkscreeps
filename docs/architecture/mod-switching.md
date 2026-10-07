# 无开放轮次的包切换

世界在已提交边界上原地替换 Mod 执行层（包集合、冻结计划、玩家面与工作者配置），世界与账户状态不动。这是 G24 的首片（G24.a）：解决「一个世界创建时绑定一组包，此后没有任何换执行层而不换世界的通道」——此前改包集合只能重建世界，或走冷恢复（而冷恢复的身份门按设计要求新旧包集逐字节一致）。

- 入口：`EmbedEngine::switch_mod_packages(&packages, &options)`（`crates/kk-runtime/src/embed/switch.rs`）。
- 边界门：与检查点保存同一套判据——无待重试轮、无执行故障、无待恢复 Mod VM、引擎非恢复态（`no_open_round`），外加同线程 V8 栈序门（同 `restore_checkpoint`）。
- sim 暴露：场景字段 `mods_switch`（`after_tick` + 整体替换的 `configs`/`selection`，复用 `mods.root`）；报告 v6 顶层 `mods_switches` 记录每次切换。

## 范围与拒绝语义

| 变化 | 语义 |
|---|---|
| 增包 | 允许：新计划含新包，新包效果自切换后出现 |
| 减包（无残留） | 允许：贡献随新计划消失；规则覆盖回落内建默认 |
| 宿主选项（selection/configs/预算/停用阈值） | 允许调整：属宿主权威，不是包内容；产生新 plan_hash |
| 减包（有残留） | 拒绝：已移除类型的 Mod 对象、已移除包的扩展字段会把世界钉死（每轮结算与冷恢复都红）；孤立 namespace state 的 **null 占位**显式丢弃（链式提交对每个执行过的绑定都落键，null 无信息量），**非 null 实数据**拒绝——静默删除等于丢数据 |
| 同包内容变更 | 拒绝：交集包 manifest/文件摘要逐项等值是硬门；报错指路 G23 内容级迁移（「开发更新 ≠ 存量世界升级」） |
| 已移除建筑 | 惰性 Generic 对象：不 tick、不生产（沿用既有静默跳过语义），仍是障碍物 |
| 停用集合 | 投影到新计划**装配选择集**后重放并核对闭包（发现集含未选择包，不参与投影） |
| 故障诊断三域 | 指向已移除包的记录丢弃（诊断是瞬态信号，不是数据） |

## 实现结构（替代分片模式）

与 `restore_checkpoint` 同骨架：全部可失败步骤发生在候选分片，`*self = replacement` 是唯一原位变更点，任一步失败旧分片原样继续运行。与冷恢复的区别在身份门方向——冷恢复要求新旧同身份（ModIdentity/plan_hash/执行策略逐项等值），切换恰好要换身份，取而代之的是交集内容等值、残留分诊与停用投影。

1. 从**活状态**（而非 JSON 往返）捕获：引擎提交快照、工作者配置+模块表、停用集、故障三域、last_console、并发与通道、trace 意愿与缓冲。
2. 候选分片全链装配：`create_shard_with_mods`（空房间）走注册 → canary 预检 → `install_hosted_plan` → 玩家 spawn。
3. 引擎侧**换计划收养**：`Engine::restore_checkpoint_replanned` 跳过载荷 plan_hash 等值（计划已由宿主先换装），保留全部内容校验——mod_states 命名空间、Mod 对象类型、扩展字段 schema 对**新计划**执行，天然充当去包残留硬门。
4. 停用投影重放 → 故障过滤 → `retire_previous`（realm 栈序退役）→ trace 意愿与缓冲转移 → 原子切换。

## 玩家常量与工作者

工作者重建保留预算与模块表（玩家代码不动）；常量与 Mod 玩家面按新计划重注入。计划派生常量键是**封闭集**（LINK_CAPACITY、仓库十键、战斗射程四键、RESOURCES_ALL），装配路径确定性覆盖或移除，旧计划值不会残留——玩家可见常量与结算同源切到新计划。V8 堆与模块级状态不保留，与冷恢复同口径（见[基础检查点](checkpoints.md)）。

## sim 场景与冷恢复

- 切换恰在**提交完第 `after_tick` 轮后**应用；拒绝（含切换装载本身的失败）不使运行失败——旧计划继续结算，`mods_switches` 记 `rejected` 与错误。
- 续跑按检查点世界时间**与实际生效的切换**选择恢复用包集合：`applied_switches`（保存进 sim 检查点元数据）为空说明切换未生效，即使 `world.time >= after_tick` 也装载初始集合；实际身份由 ModIdentity 逐项核验，装载错集合会被响亮拒绝。续跑循环不会重放已生效的切换，也不会重试时间已过边界的被拒切换。
- 切换后保存的检查点以新包集合身份冷恢复通过——这是切换正确性的关键回归（夹具 [sim-mod-switch](../../compat/fixtures/sim-mod-switch)：增包/减包/冷恢复三场景，双通道）。

## 边界与后续

- 原生分片（无 Mod）不支持切换；重建分片用 `create_shard_with_mods`。
- 候选分片按当前世界种子构造（realm 装配期注入），切换不换随机协议根（[协议](gameplay-determinism.md)）。
- sim metadata 信封（executable_sha256 门）与切换正交；包内容级迁移、Mod 对象/字段的转归、RNG 迁移联动属 G23 后续子片。
- watch（文件监视自动触发切换）未实现，属 G24 后续。
