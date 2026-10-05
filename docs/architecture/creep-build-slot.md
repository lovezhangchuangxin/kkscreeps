# Creep 建造族动作槽

`builtin:creep.build`、`builtin:creep.repair`、`builtin:creep.dismantle` 已开放 JS before/wrap/replace/after 和正常拒绝。执行仍由原玩家 API 收集意图、原清洗器转换参数；原 `EXECUTION_ORDER`（dismantle 在 attack 前，repair/build 在 move 后）、动作互斥压制表（如 repair 压制 build、build 压制 attack）与 creep 归属校验保持生效。无扩展时三槽默认 Native 直调，不进 JS、无快照。

## 槽切点与三意图的写入面

三槽切在**意图处理阶段**（与战斗槽同点）。三个意图对候选的全部写入：

- **build**：执行者能量按未强化效果扣减、动作日志；`EVENT_BUILD`；工地 progress 写，或完工变换——`remove_obj(工地)+insert_new(结构)`，新结构带初始 hits/容量/衰减/冷却时钟（rampart 的 hitsMax 按建造者是否房主）。宿主可放置进度超限工地，此时 remaining 为负、build 减少进度并返还能量（有符号运算）。
- **repair**：目标 hits 恢复（钳 hitsMax）、执行者能量（`ceil(effect/100)`）、`EVENT_REPAIR`。不创建/移除对象。
- **dismantle**：目标格有 rampart 时先重定向到 rampart；执行者回收 `amount/200` 能量；超容掉落（同格容器先接收，余量合并/新建掉落堆）；`apply_damage` 施伤——结构击毁即 `remove_obj+insert_new(废墟)` 并转存库存，spawn 击毁连带移除孵化中同名 creep。

## ctx.build 受控能力

三槽的处理器获得不可枚举、不可替换的 `ctx.build`（仅处理器同步执行期内有效）：

| 方法 | 语义 |
|---|---|
| `request(targetId)` | 以新目标执行**本动作**的完整 native 语义（距离/能量/安全模式/rampart/同格障碍全部原生预检仍生效；不满足时静默无效果）。 |
| `cancel()` | 取消本动作：native 中心不再执行。 |

进度、伤害与能量数值不经此面修改——没有任意数值写能力，效果仍由 WORK 部件与 boost 决定。执行遵循**恰一次**约束：`request`/`cancel`/native 中心三者先到者消耗本动作，之后的再次调用（含 native 执行后的 after 处理器内调用）都是故障，不能被 catch + decline 掩盖。三个动作都带显式目标，没有战斗槽 rangedMassAttack 那样的无目标例外。

`ctx.target.fields` 为执行者候选只读视图（与资源/战斗槽同投影）；`ctx.args.id` 为请求目标（工地或结构），修改本地副本不影响 native 接收者。与战斗槽的一点差异：建造族 native 会改执行者 `store.energy`/hits 等投影字段，`ctx.build.request` 在执行后由 SDK 刷新处理器的持久成员，避免返回时的字段写回把旧视图当修改（与 `ctx.resources` 转账同型；战斗动作不落在执行者投影面上，无需刷新）。

`replace` 返回 `undefined` 仍表示静默取消默认中心；选择性 replace 对非目标执行者必须 `ctx.build.request(ctx.args.id)` 重发，否则该执行者的建造被静默取消（与战斗/移动槽同坑）。

## 局部恢复边界

三槽共用战斗族的日志化边界（`Working::snapshot_build` / `restore_chain_snapshot`，快照类型 `ChainSnapshot` 与战斗槽共享）：

1. **受影响对象首触克隆**：执行者与请求目标；dismantle 另含目标格 rampart（重定向面，与战斗槽同源计算）与**执行者格上的掉落面**——同格全部掉落堆与容器（超容掉落只写这些对象，取超集不必按资源名精确匹配）。
2. 事件长度截除；链内增删日志（`chain_mutations`，Hosted 建造族链无条件武装、Native 直调路径保持 `None` 零开销）：`insert_new`/`remove_obj` 逐条记录，拒绝时逆序撤销——完工结构/废墟/掉落堆的插入被删除、被移除对象（含伴随候选状态）回插，ID 游标随后回退（只回不进）。被拒的完工在下一 tick 重新建造时结构以同一 ID 落成。
3. **运行时首触日志**（`ctx.build.request` 触达、且不在引擎预计算集内的对象，如 dismantle 改向目标格上的 rampart）：拒绝时先于引擎边界恢复。

repair 链内没有对象增删（空日志），仍走同一边界代码，只有对象克隆与事件截除实际生效。此前动作的写入、其他执行者的同类动作、其他动作的结算不受影响；后续动作继续。掉落堆在落地当 tick 仍按 `ceil(amount/ENERGY_DECAY)` 衰减，恢复语义与该衰减正交。

## 未开放范围

`attackController`（涉及控制器降级计时）仍为原生直调；容量族（transfer 之外的仓库写入槽）与 link tick 拒绝归后续批次。建造意图的 wrapper 槽（mod-system 寻路 wrapper 同族设计）未接。dismantle 击毁带孵化任务的 spawn 时连带移除孵化 creep 的恢复由增删日志结构覆盖，尚无专测钉住（放置面不支持孵化任务，与战斗槽同一登记）。

验收见[交付记录](../research/notes/d4-build-slot-2026-10-05.md)。
