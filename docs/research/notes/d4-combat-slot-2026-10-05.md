# D4 战斗槽交付记录（2026-10-05）

基线 `f280389`（D4 移动片收口）。本片开放五个战斗动作槽（`builtin:creep.attack/rangedAttack/rangedMassAttack/heal/rangedHeal`）的 JS 处理链、正常拒绝与 `ctx.combat` 受控能力。协议与边界见[战斗动作槽](../../architecture/creep-combat-slot.md)。

## 切点与恢复边界（源码核查结论）

战斗意图在 `EXECUTION_ORDER` 派发期经贡献槽执行；对 creep 的伤害/治疗**延迟**到对象 tick（`damage_to_apply`/`heal_to_apply` 队列，先伤后治再判死），对结构立即写 hits、击毁即移除并建废墟（spawn 连带移除孵化中同名 creep）。由此本片新增三类恢复面，均为资源/移动槽没有的：

1. **延迟队列条目**：受影响对象的队列值按链前快照恢复（保留此前动作的既有值）。
2. **rampart 重定向受击对象**：攻击实际落点可能是目标格上的第三个对象，进入引擎预计算集；能力改向触达的链外对象由运行时首触日志恢复。
3. **链内对象增删**：`Working::chain_mutations` 仅在战斗 Hosted 链期间武装，`insert_new`/`remove_obj` 逐条记录，拒绝时逆序撤销（废墟删除、被移除对象连同扩展字段/疲劳/队列条目回插），ID 游标回退（只回不进）。Native 直调路径保持 `None` 零开销；Hosted 战斗链无条件武装（空日志）。

`ctx.combat.request(targetId)` 以新目标执行完整 native 语义（全部原生预检保持），执行遵循**恰一次消耗**：request/cancel/native 中心先到者消耗，后续调用（含 native 后的 after 处理器）为故障——与移动槽"末次登记生效"不同，战斗执行有真实世界效果、不可叠加。massAttack 无目标改向（request 故障、cancel 可用）。

## 评审要点（复核时重点看）

- `chain_mutations` 在链 Err（故障）路径不解除武装：候选随整轮丢弃，字段不跨 tick 存活——若引入"链失败但房间候选保留"的路径须重新审计。
- `restore_next_id` 只回退不前进，且回退前必须已删除本链全部插入对象（`restore_combat` 的逆序回放保证）。
- 运行时首触日志在引擎边界**之前**恢复，引擎恢复最后落笔——两侧快照值一致性依赖"单链对同一对象至多一次伤害执行"（consumed 约束）。
- spawn 摧毁连带移除孵化 creep 的恢复由 `Removed(bundle)` 结构覆盖，但**无专测钉住**（放置面暂不支持孵化任务，登记为残留）。

## 验收矩阵与结果

| 面 | 结果 |
|---|---|
| 定向测试（mod-api/mod-host/engine/runtime） | 499 通过 / 0 失败 / 5 忽略；新增 15 测（链语义 7 + 世界 8，含评审补的 rangedAttack 专测与槽键等价钉子） |
| 全 workspace | 699 通过 / 0 失败 / 11 忽略（基线 683 + 16，含槽键钉子） |
| mod_chain 套件 | 90 通过（75 基线 + 15 新） |
| 覆盖点 | 四类 handler 顺序/权威参数/双通道；replace 改向/取消/选择性重发；五位置拒绝（保留此前 transfer、后续 move、他人攻击）；不可掩盖（外层故障/只读写）；能力故障（参数/双耗/取消后改向/native 后调用/跨槽）；过期能力；写回失败缓存重试；击杀拒绝（存活/无墓碑/无事件）；反击双面恢复；结构摧毁（废墟回滚/资源转存对照/rampart）；rampart 重定向；heal/rangedHeal 延迟治疗+改向；mass 距离倍率 1/0.1+cancel+request 故障；双玩家双房串行=并发 |
| 完整棘轮（release `e665e1fc`） | 2914 总数 1918/89/907，地板 1918，转红 0、转绿 0、新豁免 0 |
| bot 三夹具 | tooangel/overmind 2000 tick、wasm 10 tick 全完成，0 中断、0 惩罚；overmind 仅既有首 tick `DirectiveHarvest.create` 错误 |
| 真实战斗场景（新夹具 sim-combat） | 双玩家 PvP 300 tick × JSON/KKPV 双通道：6/6 断言通过、终态一致——蓝方双守卫与 spawn 全部摧毁（延迟伤害致死+结构击毁废墟路径）、红军存活且部件级耐久正确分布（前列先损） |

## 残留与未开放

- `attackController`（控制器降级计时）、repair/dismantle（建造族）仍为原生直调。
- 孵化中 creep 随 spawn 摧毁被移除的恢复无专测（放置面不支持孵化任务）。
- 战斗 wrapper 槽（mod-system 寻路 wrapper 同族设计）未接；伤害数值/TOUGH 减伤/power 免疫维持引擎现状未模拟。
- 零 Mod 开销判读随 D7（战斗 Native 直调路径未测开销，默认无 JS 进入）。

机器可读记录见 [d4-combat-slot-2026-10-05.json](d4-combat-slot-2026-10-05.json)。
