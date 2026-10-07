# JS 移动请求预检策略

`builtin:room.movePrecheck` 在全部意图与移动动作槽登记完成后、竞争输入构造之前整房一次进入 JS。2026-10-07 的 G02.d1 增加 `ctx.precheck.allowStatic(id)`：独立 JS 包可以放行墙地形、结构、私有敌方 rampart 和己方障碍工地；`block(id)` 仍是不可撤销的请求级否决。被否决请求从登记矩阵移除，等同从未登记，不参与竞争、不让位、不占格。

顺序为：拉拽登记策略 → 本槽 → [竞争排序](mod-move-contest.md) → [准入与链预演](mod-move-admission.md) → [障碍追加](mod-move-obstacle.md) → 原生取消链 → [移动代价](mod-move-cost.md) → 对象 tick 位移。静态放行随同一请求传给准入预演和实际取消链；动态占格、安全模式、获准让位与级联取消仍按既有规则处理。

## 登记与原生回退

两个入口 `intent_move`（玩家意图）和 `slot_request_direction`（移动槽能力）共用 `movement/static_obstacles.rs` 的结构判定：

- 无预检贡献时，结构/私有 rampart 在登记时直接拒绝，保持原生路径；墙和己方工地仍在仲裁时拒绝。
- 有本槽贡献时，原生静态拒绝的请求也暂存进入输入，同时保存**登记时的拒绝快照**。成功后先过滤未放行的登记拒绝与显式 block，再构造竞争输入；放行绑定请求者，不会借同格落选者的许可放行胜者。
- 空处理器或正常拒绝仍兑现登记快照：即使结构在后续动作中被删除，也不能把原本未登记的请求误放行。相反，登记后新增的障碍按仲裁快照复核；显式 allowStatic 可覆盖该静态障碍。
- 坐标、方向、ID、孵化与候选提交约束继续由宿主校验；静态玩法障碍属于可替换决策。

请求级否决与障碍槽胜者级否决不同：预检移除强者请求后弱者可独占目的格；选胜后挡住强者不会重新选弱者。障碍槽的追加 block 也能挡住静态放行者。

## 公共 SDK

有本槽贡献且存在请求时进入 JS；无请求不调用。`ctx.target.fields` 恒为 null。`ctx.args.requests` 按对象 ID 升序、不截断，每对象至多一条：

| 字段 | 含义 |
|---|---|
| `moverId/owner` | 请求者 ID 与归属 |
| `originX/originY`、`targetX/targetY` | 原格与目的格 |
| `movePower/weight` | 含 boost 的 MOVE 功率与移动重量 |
| `pulled/pulling` | 预检时点的原始拉拽标记，配对一致性复核在准入预填发生 |
| `terrainWall/terrainSwamp/hasRoad` | 目的格地形与道路摘要 |
| `objects[]` | 目的格对象：`id/kind/owner/isPublic/structureType`；尚无让位预期 |
| `nativeRegistrationBlocked` | 登记时结构/私有 rampart 的原生拒绝快照 |
| `nativeStaticBlocked` | 预检时静态初判，含墙和己方障碍工地；不含 creep 动态占格 |

`ctx.precheck` 只能在所属处理器同步执行期使用：

| 方法 | 语义 |
|---|---|
| `isBlocked(id)` | 显式 block，或上述两静态初判任一为真且未 allowStatic；不预测动态让位 |
| `block(id)` | 对本次输入中的请求追加不可撤销否决 |
| `allowStatic(id)` | 放行本次输入中的请求的静态障碍；不能清除任何显式 block 或动态占格 |

因此 `allowStatic` 后 `isBlocked` 为 false 并不保证最终移动：竞争落选、疲劳/MOVE 门槛、动态占格及后续障碍策略仍可能拒绝。放行只在本房本 tick 生效，下一 tick 重新决策。

## 独立 JS 组合

[ghost 与 seal 两个独立包](../../compat/fixtures/mod-move-static/README.md)分别实现穿越和区域管制：

```js
export default function register(api) {
  api.handlers.before('builtin:room.movePrecheck', { id: 'ghost' }, (ctx) => {
    for (const r of ctx.args.requests) ctx.precheck.allowStatic(r.moverId);
  });
}
```

```js
export default function register(api) {
  api.handlers.after('builtin:room.movePrecheck', { id: 'seal' }, (ctx) => {
    for (const r of ctx.args.requests) {
      if (r.targetX >= 30) ctx.precheck.block(r.moverId);
    }
  });
}
```

组合时 ghost 放行墙与私有 rampart，seal 保留区域否决；即使另一个处理器随后再 allowStatic，显式 block 仍然生效。native 中心不写 JS 决策，多包顺序经[组合诊断](mod-composition.md)冻结。既有 [fenced/courtesy 请求否决场景](../../compat/fixtures/sim-mod-move-precheck/README.md)继续适用。

## 拒绝、故障与恢复

- `api.decline(code)` 丢弃整条链全部 JS 放行、显式否决和暂存 namespace state，再按登记快照恢复原生过滤；其他移动槽仍正常运行。这是整房回退边界。
- 域外 ID、非字符串、跨槽或异步过期调用走能力故障，不能被 catch + decline 掩盖；整轮不发布，按[Mod 执行恢复](mod-execution.md)重建 realm。
- 晚期写回失败重用玩家结果缓存，重新决策后世界与 state 恰一次提交；停用包重装后按有效链执行，全部贡献摘除则恢复 native 登记。
- 新集合是 tick 内瞬态，无新持久写入域；检查点只保存已提交世界与 Mod state。内建槽仍为 20，catalog revision 从 v14 升至 **v15**，覆盖输入/能力语义变更；旧计划身份的检查点按已有身份门拒绝，没有自动迁移。

## 验收与后续

[四个真实玩家场景](../../compat/fixtures/sim-mod-move-static/README.md)核对原生、ghost、seal、组合，JSON/KKPV final、冷启动续跑、namespace 恰一次与回放一致。引擎和运行时另覆盖墙地形/结构/私有 rampart/己方工地、不可逆否决、动态取消链、准入预演、移动槽入口、拉拽、源房障碍放行后跨房、decline/能力门/停用/失败重试。证据见 [G02.d1 交付记录](../research/notes/g02d1-static-movement-2026-10-07.md)；G02.c 的[历史证据](../research/notes/g02c-move-precheck-2026-10-06.md)保留当时的单向接口。

玩家 `move(direction)` 没有静态地形/结构预检，本片在正常身体和疲劳条件下可直接发出请求。`moveTo`/PathFinder 仍使用原生寻路障碍，**不会自动规划穿墙路径**；疲劳/MOVE 的方向移动与 moveTo 玩家预检尚未策略化，登记 G02.d2。跨房到达格仍沿用既有原生过渡语义（不复检目的房占格/地形），本片不改写它。G02/G03 父项保持部分完成；孵化门槛、完整仲裁替换和障碍槽第二次预演属于后续切片。
