# 移动玩家门规则包（G02.d2）

两个独立的规则覆盖包：不注册任何处理器，只经 `api.rules.override` 把[移动玩家门](../../../docs/architecture/player-precheck-rules.md)的两条数值规则写入冻结计划——玩家面 `move()`/`moveTo()` 预检常量与结算仲裁的原生准入预填同源生效（同一 `MoveGate` 快照），单一 override 双端可见。

| 包 | 规则 | 默认 → 覆盖 | 语义 |
|---|---|---|---|
| marathon | `builtin:creep.move.fatigueLimit` | 0 → 4294967295 | 疲劳移动者不再停歇（[move,work,work] 逐 tick 连走） |
| legless | `builtin:creep.move.minActiveMoves` | 1 → 0 | 无 MOVE 部件的 creep 可自发移动（疲劳照常累积且永不恢复） |

两包可组合：两门独立叠加。被拉豁免不经规则面（`move(target)` 结构通道），拉拽玩法不受影响。真实场景见 [sim-mod-move-playergate](../sim-mod-move-playergate/README.md)。
