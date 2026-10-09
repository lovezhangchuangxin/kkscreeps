# JS 移动障碍策略场景

第 0 tick 三个移动者并行发出请求：`a`（`move`，(10,10)）与 `b`（`move`，(10,11)）互进对方原格构成交换让位链；`c`（`move`，(30,30)）向南一步。之后各 tick 只记录位置。两包均使用公共 SDK，宿主没有按包 ID 分派的规则。

| 场景 | 障碍行为 | a/b 交换 | c 南下 |
|---|---|---|---|
| `scenario-plain.json` | 原生障碍判定：空平原格放行，交换成立 | 成立 | 移动 |
| `scenario-staynorth.json` | stay-north 包 replace：目的格 y≥20 追加阻挡 | 成立 | 不动 |
| `scenario-noswap.json` | no-swap 包 after：目的格含让位中 creep（yielding）追加阻挡 | 失效 | 移动 |
| `scenario-combined.json` | stay-north replace + no-swap after，追加集并集 | 失效 | 不动 |

障碍槽只允许**追加阻挡**（`ctx.obstacle.block`，单向；`isBlocked` 读预填时点 native 初判）——放行 native 阻挡会让移动者进入未让位对象占据的格、破坏坐标表示有效性。追加取消沿让位链传播：交换一方被阻挡后另一方失去让位依赖、随之取消，这个传播由引擎完成，JS 不逐格重算。让位取消后 native 初判只会收紧不会放宽。有移动胜者的 tick 恰好调用一次槽并使对应 namespace 的 `applied` 计数为 1。正常拒绝、非法写入与恢复边界见[JS 移动障碍](../../../docs/architecture/movement.md#障碍追加)。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-obstacle/scenario-combined.json --ticks 3 --report /tmp/obstacle.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-obstacle/scenario-noswap.json --ticks 3 --kkpv --report /tmp/obstacle-noswap.json
```

`kk-cli` 的 `sim_tests/move_obstacle.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份；在第 1 tick 后保存检查点、退出进程、加载再续跑，确认位置与 namespace 计数恰一次延续。

## 障碍后预演（当前能力）

在 `builtin:room.moveObstacle` 的处理器中调用 `ctx.obstacle.evaluate()`，可读取当前追加阻挡之后的幸存胜者 ID 升序数组。它复用正式取消链，包含交换/循环/跟随/拉拽的级联取消；返回值脱离候选，不消费阻挡集。继续 block 或后续处理器修改后应重新调用；decline 则丢弃整条障碍链的追加与 state。`ctx.obstacle.isBlocked(id)` 只反映 native 初判与直接追加，级联结果请使用 evaluate。

```js
api.handlers.after('builtin:room.moveObstacle', { id: 'audit' }, (ctx) => {
  ctx.state = { survivors: ctx.obstacle.evaluate() };
});
```

回归见[障碍后链预演](../../../crates/kk-runtime/tests/mod_chain_tests/obstacle_preview.rs)，本目录原有场景断言保留。

`ctx.args.safeModeOwner` 给出当前结算时点有效的安全模式主人，未激活、到期或无主为 null。可与 movers 的 owner 和目的格生物摘要一起解释 nativeBlocked；不改变阻挡能力或通行规则。主人移动时可忽略其他归属的未让位 creep/powerCreep，自有生物仍阻挡。参见[双通道归属与到期回归](../../../crates/kk-runtime/tests/mod_chain_tests/safe_mode_summary.rs)。
