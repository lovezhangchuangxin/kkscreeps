# D4 建造族槽交付记录（2026-10-05）

基线 `338dfac`（流程第二片 check:fast）。本片开放三个建造族动作槽（`builtin:creep.build/repair/dismantle`）的 JS 处理链、正常拒绝与 `ctx.build` 受控能力。协议与边界见[建造动作槽](../../architecture/creep-build-slot.md)。

## 切点与恢复边界（源码核查结论）

三槽切点与战斗族相同（意图处理阶段）。本片新增/复用的恢复面：

1. **完工变换**（build）：`remove_obj(工地)+insert_new(结构)` 与战斗槽的 `chain_mutations` 链内增删日志完全同构——拒绝时逆序撤销（结构删除、工地连同伴随候选状态回插）、ID 游标回退（只回不进）。被拒的完工在下一 tick 重建时结构以同一 ID 落成（`bump_next_obj_id` 取 max，被拒候选未发布，复用成立）。
2. **dismantle 掉落面**：超容掉落只写执行者格上的对象（容器 Generic store 先接收、同资源名掉落堆合并、新堆 insert_new），首触快照取同格全部掉落堆与容器的超集；新堆插入走增删日志，同 tick 的 `ceil(amount/ENERGY_DECAY)` 衰减与恢复语义正交。
3. **快照类型泛化**：`CombatSnapshot` → `ChainSnapshot`，战斗/建造族共享 `snapshot_touched`/`restore_chain_snapshot`/`finish_chain_snapshot`；repair 链内无增删（空日志）仍走同一边界。
4. **`ctx.build.request` 后的视图刷新**：建造族 native 落在执行者投影字段（store.energy/hits）上，SDK 在 request 前先暂存当前上下文、执行后 `refreshContext`，避免处理器返回时的字段写回把旧视图当修改——与 `ctx.resources` 转账同坑；战斗动作不落在执行者投影面（反击/自愈走延迟队列、自攻被拒），无需刷新。

## 评审要点（复核时重点看）

- `ctx.tick` 在首个结算轮为 0（世界 time 在发布后才前进），不能用作用例内的轮次标记——完工重试用模块内一次性闩门控。
- dismantle 改向后拒绝是运行时首触日志唯一的活路径（改向目标不在引擎预计算集内），专测覆盖原目标/改向目标/掉落面/事件四层复原。
- `replay_redirect_journal` 的注释同时钉住两个前提：consumed 恰一次执行 + 链内无其他候选写通道；给建造族开放新写能力时须重审。
- `snapshot_touched` 有 `debug_assert` 固化"未武装时才进快照"前提（嵌套链当前不存在）。

## 验收矩阵与结果

| 面 | 结果 |
|---|---|
| 全 workspace | 715 通过 / 0 失败 / 11 忽略（基线 699 + 16：链测试 8 + 世界测试 8，含钉子扩展） |
| mod_chain 套件 | 108 通过（90 基线 + 18 新；含评审清偿 4 测） |
| 覆盖点 | 四类 handler 顺序/权威参数/双通道；replace 改向/取消/选择性重发；五位置拒绝（完工瞬间；保留此前 transfer、后续 upgradeController、他人建造）；不可掩盖（外层故障/只读写）；能力故障（参数/双耗/取消后改向/native 后调用/跨面 resources+combat+movement）；过期能力/跨槽；完工被拒（工地复原/结构撤销/ID 游标回退重试同 ID/能量返还）；dismantle 摧毁+掉落恢复（容器先收 41/溢出成堆 31−衰减 1/废墟转存/rampart 重定向）；repair hitsMax 钳制（缺口 50/能耗 ceil）与有符号进度超限（完工+返还能量 35）；dismantle 改向后拒绝（首触日志重放活路径）；无 Mod 与 observe 终态一致（双通道）；写回失败缓存重试；双玩家双房串行=并发 |
| 完整棘轮（release `59c51477`） | 2914 总数 1918/89/907，地板 1918，转红 0、转绿 0、新豁免 0（两条既有未命中豁免警告与基线一致） |
| bot 三夹具 | tooangel/overmind 2000 tick、wasm 10 tick 全完成，0 中断、0 惩罚；overmind 仅既有首 tick `DirectiveHarvest.create` 错误 |
| 真实建造场景（新夹具 sim-build） | 单玩家 120 tick × JSON/KKPV 双通道：11/11 断言通过、终态一致——修路 30 次（300 进度完工）、补完 extension（2900→3000）、repair 预伤墙 900→1300、dismantle 塔 400→击毁（4 tick），计数器逐项钉死 |

## 评审与清偿

三路评审（恢复边界/协议装配 SDK/测试覆盖语义）：P0×0、P1×3、P2 若干，全部清偿——repair hitsMax 钳制专测、dismantle 改向后拒绝专测（日志重放活路径）、架构文档五处同步（mod-execution 拒绝清单与边界段、creep-combat-slot 改名引用与过时句、creep-resource-slots/store-rules 10→13、roadmap、source-organization）；P2 清偿含 `restore` panic 文案、journal 注释归因补全、`ctx.movement` 跨面故障用例、完工 `incomplete:false` 与 dismantle 事件断言、错误文案族中立化、`snapshot_touched` debug_assert。

## 残留与未开放

- `attackController`（控制器降级计时）、容量族、link tick 拒绝仍为原生直调/未开放。
- dismantle 击毁带孵化任务的 spawn 时连带移除孵化 creep 的恢复无专测（放置面不支持孵化任务，与战斗槽同一登记）。
- repair/dismantle 的四类 handler 顺序/args 防篡改仅 build 代表槽深测（三槽共用同一执行器与边界代码）；before/wrap-before 位置拒绝同。
- 有符号进度超限的**拒绝路径**（超限工地完工被拒→进度回 305）已由超限用例的拒绝组覆盖；boost 分离等既有语义由 settle_tests 既有测试在新派发路径上继续钉住。
- 零 Mod 开销判读随 D7（三槽 Native 直调路径未测开销，默认无 JS 进入）。

机器可读记录见 [d4-build-slot-2026-10-05.json](d4-build-slot-2026-10-05.json)。
