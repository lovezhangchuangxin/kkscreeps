# G03.a 整房请求视野与链预演场景

第 0 tick 六个 creep 中的五个并行发出移动请求（`parked` 静止占位不发请求）：`a`/`b` 互换原格（交换）；`stranger` 请求移入 `parked`（静止 work creep）占据的格——门槛原生获准但取消链必吞（准入输入不含占位信息，**只有链预演可见**）；`strong`（`move,move`，功率 2）与 `weak`（`move`，功率 1）竞争 `(30,29)`（strong 胜出、weak 落选——**落选者只在整房请求视野 `ctx.args.requests` 中可见**，胜者输入 `movers` 不含它）。之后各 tick 只记录位置。

| 场景 | 准入行为 | a（交换） | strong（竞争胜者） |
|---|---|---|---|
| `scenario-plain.json` | 原生：交换成立、stranger 链内取消、strong 移动 | `(10,9)` | `(30,29)` |
| `scenario-wave.json` | wave 包 replace：`evaluate()` 发现级联损失 → 全房停下 | `(10,10)` | `(30,30)` |
| `scenario-calm.json` | calm 包 after：落选者可见 → 扣住同格胜者 | `(10,9)` | `(30,30)` |
| `scenario-combined.json` | wave replace + calm after 并集 | `(10,10)` | `(30,30)` |

- `ctx.admission.evaluate()` 按候选**当前**终值只读预演取消链，返回会存活的胜者 ID；宿主用与正式仲裁**完全相同**的 `cancel_blocked` 代码路径计算，JS 无需自己推导让位级联。wave 据此把"半队出发"改写为"全停"。
- `ctx.args.requests` 是仲裁前的完整原始请求矩阵（含落选者，逐条 `moverId/x/y/index/originX/originY/won`）；`movers[].pulledBy/pulling` 提供有效拉拽配对的对象 ID。
- 预演时点在障碍槽之前：障碍追加只会收紧，预演结果是最终取消结果的上界。正常拒绝整房回退原生预填；两个策略包可叠加（after 层在 replace 之上收紧）。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-arbitration/scenario-combined.json --ticks 3 --report /tmp/arbitration.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-arbitration/scenario-wave.json --ticks 3 --kkpv --report /tmp/arbitration-wave.json
```

`kk-cli` 的 `sim_tests/move_arbitration.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份（四身份互异）；在第 1 tick 后保存检查点、退出进程、加载再续跑，确认位置与 namespace 计数恰一次延续。引擎侧等价性证据（预演幸存集 == 实际取消链幸存集：交换/环/链/拉拽/覆盖/占位拓扑）见 `kk-engine` 准入单测。策略包在 [../mod-move-arbitration/](../mod-move-arbitration/)。
