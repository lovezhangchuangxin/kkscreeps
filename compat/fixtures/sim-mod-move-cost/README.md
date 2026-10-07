# G04.a 移动代价策略场景

`scout`（`move,work`，重量 1）从平原 (19,25) 向右进入沼泽带（x≥20）；`walker`（同型）在纯平原 (5,25) 向右。每 tick 都提交移动，`--ticks 4`、`time>=3` 起记录位置（玩家代码先于结算，该时点已结算 3 次，断言反映 3 次落位）。

| 场景 | 代价策略 | scout（沼泽） | walker（平原） |
|---|---|---|---|
| `scenario-plain.json` | 原生（沼泽 10×重量） | 一步即停 `(20,25)` | 每 tick 前进 `(8,25)` |
| `scenario-swampwalker.json` | swampwalker after：无路沼泽压到 `weight×2` | 每 tick 前进 `(22,25)` | 不动策略 `(8,25)` |
| `scenario-bog.json` | bog replace：全量翻倍 | 一步即停 `(20,25)` | 隔 tick 前进 `(7,25)` |
| `scenario-combined.json` | bog replace + swampwalker after | after 层压回 `(22,25)` | 仍为翻倍值 `(7,25)` |

- **数值替换语义**：`ctx.cost.setFatigue(id, 0..=u32::MAX)` 替换该移动者的疲劳代价；`getFatigue` 缺省读**原生代价**（组合中后层可读先层改写）；缺项/拒绝/故障回退原生（重量×费率：平原 2/沼泽 10/路 1）。
- 输入为终局幸存者（取消链后）按对象 ID 升序：`id/owner/originX/originY/targetX/targetY/weight/terrainSwamp/hasRoad/bodyLength/pulledBy(链首拉者)/nativeFatigue`。停走者（疲劳未清，玩家面 move() 预检拦截）不产生请求、不进输入。
- 宿主固定执行效果不开放：疲劳转记链首拉者、道路磨损、工地踩踏、跨房归零、每 tick 恢复（-2×MOVE 件）。代价与恢复的相对大小决定行军节奏：≤恢复量即持续行军、>恢复量隔 tick 行军。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-cost/scenario-combined.json --ticks 4 --report /tmp/cost.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-cost/scenario-swampwalker.json --ticks 4 --kkpv --report /tmp/cost-swamp.json
```

`kk-cli` 的 `sim_tests/move_cost.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份（四身份互异；内建槽 19→20、revision v13 由 freeze 槽计数断言钉死）；第 2 tick 后保存检查点、退出进程、加载再续跑，确认位置与 namespace 计数恰一次延续。策略包在 [../mod-move-cost/](../mod-move-cost)。
