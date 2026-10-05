# Creep 战斗动作槽

`builtin:creep.attack`、`builtin:creep.rangedAttack`、`builtin:creep.rangedMassAttack`、`builtin:creep.heal`、`builtin:creep.rangedHeal` 已开放 JS before/wrap/replace/after 和正常拒绝。执行仍由原玩家 API 收集意图、原清洗器校验；动作互斥（`EXECUTION_ORDER` 与压制表）、creep 归属校验保持生效。无扩展时五槽默认 Native 直调，不进 JS、无快照。

## 槽切点与结算时序

战斗槽切在**意图处理阶段**：全部玩家意图按 `EXECUTION_ORDER` 派发时，五个战斗动作经贡献槽执行；对 creep 的伤害与治疗不立即落地，而是累加进候选的 `damage_to_apply` / `heal_to_apply` 延迟队列，待移动仲裁后在该 creep 的对象 tick 施加并判死亡；对结构的伤害立即写 hits，击毁即移除对象并创建废墟（spawn 摧毁连带移除孵化中的同名 creep）。因此：

- 战斗槽链内观察到的是**登记后的延迟量**，不是最终 hits；`ctx.target.fields.hits` 在链内不因本次攻击变化。
- 正常拒绝移除本动作的队列条目后，对象 tick 不会施加这份伤害——致死攻击被拒绝时目标存活、无墓碑、无死亡事件。

## ctx.combat 受控能力

战斗槽的处理器获得不可枚举、不可替换的 `ctx.combat`（仅处理器同步执行期内有效，与 [ctx.movement](creep-move-slot.md) 同型）：

| 方法 | 语义 |
|---|---|
| `request(targetId)` | 以新目标执行**本动作**的完整 native 语义（距离/孵化/耐久/安全模式/rampart 重定向全部原生预检仍生效；不满足时静默无效果）。`rangedMassAttack` 无目标改向能力，调用为故障。 |
| `cancel()` | 取消本动作：native 中心不再执行。 |
| `get(id)` | 同房候选的脱离副本；支持耐久与部件/强化读面，详见[JS 伤害策略](mod-damage-policy.md)。 |
| `applyDamage(id, amount)` | 仅 attack/rangedAttack 槽，使用 JS 计算的单体伤害并消耗动作，详见[JS 伤害策略](mod-damage-policy.md)。 |

`request` 的 power 仍由身体部件与 boost 决定；`applyDamage` 另开放受限的 JS 伤害值，治疗和部件损耗没有直接写面。执行遵循**恰一次**约束：`request`/`applyDamage`/`cancel`/native 中心先到者消耗本动作，get 不消耗动作；之后的再次写能力调用（含 native 执行后的 after 处理器内调用）都是故障，不能被 catch + decline 掩盖。这与移动槽的"单执行者单请求、末次登记生效"不同：战斗执行有真实世界效果，不可重复叠加，消耗标记单向。

`replace` 返回 `undefined` 仍表示静默取消默认中心；选择性 replace 对非目标执行者必须 `ctx.combat.request(ctx.args.id)` 重发，否则该执行者的攻击被静默取消（与移动槽同坑）。

## 读面

`ctx.target.fields` 为执行者候选只读视图（`name/owner/x/y/spawning/hits/hitsMax/body/fatigue/store/storeCapacity`）；`ctx.args.id` 为请求目标（massAttack 槽为 null），修改本地副本不影响 native 接收者。事件（`EVENT_ATTACK`/`EVENT_HEAL`/`EVENT_OBJECT_DESTROYED`）与动作日志由结算侧权威产生。

## 局部恢复边界

战斗链的拒绝恢复分两层，均在整链开始前快照：

- **引擎边界**（`Working::snapshot_combat` / `restore_chain_snapshot`；快照与恢复实现与建造族槽共享，见[建造动作槽](creep-build-slot.md)）：
  1. 受影响对象首触克隆——执行者、请求目标及其格上的 rampart（攻击重定向面）；`rangedMassAttack` 为切比雪夫 3 格内全部对象（命中集的超集）。
  2. 这些对象的延迟伤害/治疗队列条目（含此前动作的既有值，恢复到快照值而非清零）。
  3. 事件长度截除。
  4. **链内对象增删日志**（`chain_mutations`，Hosted 战斗链无条件武装、Native 直调路径保持 `None` 零开销）：`insert_new`/`remove_obj` 逐条记录，拒绝时逆序撤销——结构击毁产生的废墟插入被删除、被移除结构（含伴随的扩展字段、疲劳与队列条目）按移除时快照回插，ID 游标随后回退。
- **运行时首触日志**（`ctx.combat.request/applyDamage` 触达、且不在引擎预计算集内的对象）：改向目标及其格上 rampart 的原始记录与队列条目，拒绝时先于引擎边界恢复。

此前动作的写入、其他执行者的攻击与队列条目、移动登记不受影响；后续动作继续结算。原生伤害尚无统一数值上限，TOUGH 减伤与 power 免疫尚未模拟。策略伤害的表示上限见[JS 伤害策略](mod-damage-policy.md)。

## 未开放范围

`attackController`（涉及控制器降级计时）仍为原生直调（repair/dismantle 已随[建造动作槽](creep-build-slot.md)开放）；战斗意图的 wrapper 槽（mod-system 的寻路 wrapper 同族设计）未接。spawn 摧毁连带移除孵化 creep 的恢复由增删日志结构覆盖，尚无专测钉住。

验收见[交付记录](../research/notes/d4-combat-slot-2026-10-05.md)。
