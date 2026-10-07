# 移动玩家门场景（G02.d2）

runner（[move,work,work]，官方门下动一停一）与 crawler（[work,carry] 无 MOVE，官方门下完全静止）每 tick 向南移动；位置在玩家期记录，反映上一 tick 结算后的状态（3 tick 运行即前两次结算的位移）。规则包是纯规则覆盖（无处理器），不产生 namespace state——场景验证由位置断言与 plan_hash 身份（四场景互异）承担。

| 场景 | 选择 | runner | crawler | 说明 |
|---|---|---|---|---|
| plain | 无 | [20,21] | [20,25] | 官方门：动一停一（首 tick 走一步后疲劳 2 停歇）、无件静止 |
| marathon | marathon | [20,22] | [20,25] | 疲劳门放开：runner 连走；crawler 仍受 MOVE 门 |
| legless | legless | [20,21] | [20,26] | MOVE 门放开：crawler 首步可行走（疲劳累积后仍被疲劳门拦）；runner 仍动一停一 |
| combined | 两包 | [20,22] | [20,27] | 双门独立叠加，两 creep 均连走 |

JSON/KKPV 双通道一致；每场景 `--save-checkpoint` 退出码 2 + 新进程 `--load-checkpoint` 续跑与直跑 final 逐位等价（比较剥离实测 CPU bucket），`--verify-replay` 全部 passed。包说明见 [mod-move-playergate](../mod-move-playergate/README.md)。
