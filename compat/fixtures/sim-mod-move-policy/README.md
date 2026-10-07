# JS 移动竞争优先级策略场景

三个不同身体的 creep 在第 0 tick 同时挤同一目的格 `(10,11)`；`mid`（`work,move`，功率比 1.0）从上方、`light`（`move,move`，功率比 2.0）从右侧、`heavy`（`work,work,move`，功率比 0.5、重量 2）从下方。之后各 tick 只观察胜者位置并写入 Memory。两包均使用公共 SDK，宿主没有按包 ID 分派的规则。

| 场景 | 竞争行为 | 胜者 |
|---|---|---|
| `scenario-plain.json` | 原生默认排序：MOVE 功率/重量比最高者胜 | `light` |
| `scenario-seniority.json` | seniority 包替换排序：每格 ID 最小（最早分配）的竞争者 +1，其余 -1 | `mid` |
| `scenario-heaviest.json` | heaviest 包 after 调整：每格重量最大者设 +2 | `heavy` |
| `scenario-combined.json` | seniority 的 replace 先设 ±1，heaviest 的 after 再把最重者抬到 +2 | `heavy` |

三个策略产生三种不同胜者，组合语义独立可观察。失败者保持原格不移动（仲裁不递补落选者）。有竞争的 tick 恰好调用一次槽并使对应 namespace 的 `applied` 计数为 1；无请求的 tick 不进入 JS。正常拒绝、非法数值与恢复边界见[JS 移动竞争策略](../../../docs/architecture/movement.md#竞争排序)。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-policy/scenario-combined.json --ticks 3 --report /tmp/move.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-policy/scenario-seniority.json --ticks 3 --kkpv --report /tmp/move-seniority.json
```

`kk-cli` 的 `sim_tests/move_policy.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份；在第 1 tick 后保存、退出进程、加载再续跑，确认胜者位置与 namespace 计数恰一次延续。
