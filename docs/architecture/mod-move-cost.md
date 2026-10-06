# 移动代价策略

`builtin:room.moveCost` 是取消链结算后的房间级策略槽：终局幸存者（被取消链移除的请求者不出现）按对象 ID 升序整房一次进入 JS，Mod 可对每个幸存者**替换疲劳代价**——缺项/拒绝/故障回退原生代价（重量×地形费率）。它开放移动域的执行期数值规则，与登记期（拉拽登记、预检）、仲裁期（竞争、准入、障碍）构成完整策略链。

## 执行时点与输入

`arbitrate_room` 依序执行：拉拽登记 → 预检 → 竞争 → 准入 → 障碍 → 取消链 → **代价槽**；执行循环随后消费覆盖值。输入 `ctx.args.movers` 每条携带：

- `id/owner/originX/originY/targetX/targetY`：身份与起讫格；
- `weight`：与实收同源的裸 `move_weight`（非 MOVE/CARRY 件数+负载，无下限——纯 MOVE 件自重 0；预检/准入槽的展示值另有 max(1) 下限，跨槽口径不同）；
- `terrainSwamp/hasRoad`：目的格地形要素（原生费率 平原 2/沼泽 10/路 1 覆盖沼泽）；
- `bodyLength`：全身件数（道路磨损按此计算，仅供策略参考）；
- `pulledBy`：链首拉者 ID（疲劳原生转记对象），无配对为 null；拉者对象已移除时链首退化为自身 ID；
- `nativeFatigue`：原生代价（weight×费率），未覆盖时的执行值——策略可直接在其上做相对调整。

无幸存者的房间不进入 JS，原生整链不付费。

## 能力面与边界

- `ctx.cost.getFatigue(id)`：当前候选值，缺省为**原生代价**（不是 0）——组合场景中后层可读到先层的改写。
- `ctx.cost.setFatigue(id, value)`：数值替换，升/降双向（可替换规则：滑行面降、泥沼面升）；值域 0..=u32::MAX 整数，id 须为本次输入中的幸存者，域外报错。
- 正常拒绝整房丢弃全部覆盖；整槽故障按既有契约重试/停用，停用后回退原生。
- **宿主固定的执行效果**（不在替换面内）：疲劳转记给链首拉者（`add_fatigue`/`head_puller`）、道路磨损（ROAD_WEAROUT×全身件数）、敌方工地踩踏、跨房入边缘疲劳归零、每 tick 恢复（-2×MOVE 件效能）。这些效果的开放须另行设计（疲劳恢复策略与重量作为竞争评分要素已另拆登记）。
- 任意传送不属于方向请求能力，不在此槽混入。

## 组合

- 多包 replace/after 组合逐层改写：后层经 `getFatigue` 读到先层终值并可再覆盖，终值由最后一层写入决定。
- 与准入槽：准入门槛读**上一 tick 结算后的疲劳**，本 tick 代价改变下一 tick 的准入形态——策略使代价 ≤ 恢复量即可实现持续行军（swampwalker 演示），代价 > 恢复量则隔 tick 行军（bog 演示）。

## 验收

引擎测试（输入完整性含原生要素与链首拉者、覆盖替换与缺项回退、域与值域校验）与运行时测试（沼泽原生停走 vs 策略续走的原生对照、decline 回退、replace×after 组合、能力门、故障停用回退）见 `settle/move_cost/tests.rs` 与 `mod_chain_tests/move_cost.rs`；swampwalker（沼泽压平原当量）/bog（全量翻倍）两独立包 + 原生对照 + 组合的四场景 × JSON/KKPV 双通道与检查点冷恢复见 [sim-mod-move-cost](../../compat/fixtures/sim-mod-move-cost/README.md)。证据见[交付记录](../research/notes/g04a-move-cost-2026-10-06.md)。
