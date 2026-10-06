# G03.b 拉拽登记策略场景

第 0 tick 两组拉拽并行登记：p1 的 `hauler`（`move`）拉同主人 `cart`（`work`，无 MOVE 部件）让位上移；p2 的 `thief`（`move`）拉跨主人 `victim`（p1，`work`）让位右上。被拉者都经 `move(拉者)` 形态请求拉者当前格（无 MOVE 部件者的唯一通路）。之后各 tick 只记录位置。

| 场景 | 登记策略 | cart（同主人） | victim（跨主人） |
|---|---|---|---|
| `scenario-plain.json` | 原生登记（无 Mod） | 跟走 `(10,10)` | 跟走 `(11,10)` |
| `scenario-anchor.json` | anchor replace：全量否决 | 原地 `(10,11)` | 原地 `(12,10)` |
| `scenario-leash.json` | leash after：跨主人否决 | 跟走 `(10,10)` | 原地 `(12,10)` |
| `scenario-combined.json` | anchor replace + leash after 并集 | 原地 `(10,11)` | 原地 `(12,10)` |

- **配对级语义**：否决的是**拉拽登记**（预检输入构造之前双向移除，等同从未登记）——被拉者回到自身准入门槛（无 MOVE 部件即不能移动），拉者自身移动不受影响；对照预检槽（G02.c）否决的是移动**请求**。
- 输入为整房配对摘要（`pullerId/targetId/pullerX/pullerY/targetX/targetY/pullerOwner/targetOwner`），按拉者对象 ID 升序、不截断；列表中的配对均已通过原生登记校验（相邻、非孵化、循环链拒绝）。
- 能力 `ctx.pull.isDenied/deny` 单向收紧；正常拒绝整房丢弃否决（等同没进过 JS）。任一侧对象已移除的配对不进输入也不可否决。放行/新增方向不开放（新增拉拽无属主意图，疲劳转移会向未行动玩家计费——宿主约束）。
- 引擎侧同批把 `add_pulling` 重复登记改为后到覆盖先到并保持两表双射：换目标清旧被拉标记、换拉者清旧拉动标记，`validate_pull_pair` 的复核与胜者遍历顺序结构无关。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-pull-registration/scenario-combined.json --ticks 3 --report /tmp/pull.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-pull-registration/scenario-leash.json --ticks 3 --kkpv --report /tmp/pull-leash.json
```

`kk-cli` 的 `sim_tests/move_pull_registration.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份（四身份互异；内建槽 18→19、revision v12 由 freeze 槽计数断言钉死）；在第 1 tick 后保存检查点、退出进程、加载再续跑，确认位置与 namespace 计数恰一次延续。策略包在 [../mod-move-pull-registration/](../mod-move-pull-registration/)。
