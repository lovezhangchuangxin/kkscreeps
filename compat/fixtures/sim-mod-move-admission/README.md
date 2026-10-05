# JS 移动准入策略场景

第 0 tick 四个移动者并行发出请求：`puller`（`move`）让位到 `(10,9)`；`f`（`work`，无 MOVE 部件）经朝向相邻 creep 的 `move(puller)` 形态请求移入拉者原格（方向形态 `move()` 在玩家面预检疲劳/MOVE，同官方，无件移动者只有这条通道能到达仲裁）；`heavy`（`work,work,move`，功率 1 重量 2）与 `light`（`move`）各移一步。之后各 tick 只记录位置。两包均使用公共 SDK，宿主没有按包 ID 分派的规则。

| 场景 | 准入行为 | f | heavy |
|---|---|---|---|
| `scenario-plain.json` | 原生门槛：疲劳 0 且有存活 MOVE（或有效被拉） | 不动 | 移动 |
| `scenario-tireless.json` | tireless 包 replace：全部放行 | 移入 `(10,10)` | 移动 |
| `scenario-strict.json` | strict 包 after：`movePower<weight` 拒绝 | 不动 | 不动 |
| `scenario-combined.json` | tireless replace 全放行后 strict after 再收紧 | 不动 | 不动 |

准入初值即原生 `can_move` 结果，`ctx.admission.getAdmitted` 可读、`setAdmitted` 可覆盖；终值同源约束链取消与执行阶段。组合场景演示 after 层总能把 replace 的整面放行收回到自身门槛之上。有移动请求的 tick 恰好调用一次槽并使对应 namespace 的 `applied` 计数为 1；无请求的 tick 不进入 JS。正常拒绝、非法写入与恢复边界见[JS 移动准入策略](../../../docs/architecture/mod-move-admission.md)。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-admission/scenario-combined.json --ticks 3 --report /tmp/admission.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-admission/scenario-tireless.json --ticks 3 --kkpv --report /tmp/admission-tireless.json
```

`kk-cli` 的 `sim_tests/move_admission.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份；在第 1 tick 后保存检查点、退出进程、加载再续跑，确认位置与 namespace 计数恰一次延续。
