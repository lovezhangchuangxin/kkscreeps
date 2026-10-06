# JS 移动障碍策略

2026-10-06 已开放 `builtin:room.moveObstacle` 房间级槽。独立 JS 包可对获胜执行者**追加**目的格障碍阻挡：输入提供全部胜者的目的格对象/地形摘要与 native 初判，JS 通过单向 `block` 能力追加；取消链按 追加阻挡 ∥ 链内 native 判定 取消。这只是障碍切片的仲裁半边：意图登记期的静态预检（`has_static_obstacle`，结构/私有 rampart）仍为原生，后续子片或 G07 预检族另行开放；地形代价与执行效果属 G04。

## 为什么只允许追加

放行 native 判定的阻挡（让移动者进入未让位对象占据的格）会破坏坐标表示有效性——两个对象同格。因此能力面是单向的：`ctx.obstacle.block(id)` 只能追加，没有放行。追加取消沿让位链传播：交换/环移中一方被追加阻挡后，依赖其让位的后续请求由引擎链内递归取消（native 判定此时看到"不再让位的占据者"自然阻挡），JS 不逐格重算。链内 native 判定相对预填时点只会收紧（让位被取消）不会放宽。

## 输入与结算

准入槽之后、阻塞链取消之前，房间内存在移动胜者且有本槽贡献时整房一次进入 JS；无贡献或无胜者的房间不进入 realm。`ctx.args.movers` 按对象 ID 升序、不截断，含全部胜者（准入终值已定，被拒者仍在列——对其追加阻挡无额外效果）：

| 字段 | 含义 |
|---|---|
| `movers[].id/owner` | 胜者对象 ID 与归属 |
| `movers[].targetX/targetY` | 获准的目的格 |
| `movers[].terrainWall` | 目的格地形为墙（无路时 native 阻挡） |
| `movers[].hasRoad` | 目的格有路 |
| `movers[].objects[]` | 目的格对象摘要：`id/kind/owner/isPublic/structureType/yielding` |
| `movers[].objects[].kind` | `creep` / structureType / `constructionSite` / 类型名；Mod 对象为空串 |
| `movers[].objects[].structureType` | 工地在建结构类型；其余对象为 null |
| `movers[].objects[].yielding` | creep 让位预期成立：胜者且准入终值为真。准入被拒的胜者必被链内取消，不标让位 |
| `movers[].nativeBlocked` | 预填时点（全部胜者未取消）的 native 初判；链内让位取消只会收紧 |

`ctx.target.fields` 恒为 null（房间级槽没有对象目标）；本槽不开放移动请求、准入、竞争优先级、资源或对象能力。`ctx.obstacle` 是槽专属能力（不可枚举、只在处理器同步期有效）：

| 方法 | 语义 |
|---|---|
| `isBlocked(id)` | 读取当前判定：native 初判或已追加的阻挡 |
| `block(id)` | 追加阻挡；id 须在本次输入中，单向、不可撤销 |

取消链对每个胜者取 `追加阻挡(id) || check_obstacle_at_xy(链内)`；被取消者从胜者表移除并递归检查其原格上的其他请求（让位链传播）。

## 独立包组合

stay-north 包 replace 拦截南下（目的格 y≥20）：

```js
export default function register(api) {
  api.handlers.replace('builtin:room.moveObstacle', { id: 'stayNorth' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.targetY >= 20) ctx.obstacle.block(mover.id);
    }
  });
}
```

no-swap 包 after 禁止依赖让位链（目的格上有 yielding creep 即拦截），可与任意 replace/before 叠加——追加集在同一候选上取并集：

```js
export default function register(api) {
  api.handlers.after('builtin:room.moveObstacle', { id: 'noSwap' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.objects.some((o) => o.kind === 'creep' && o.yielding)) {
        ctx.obstacle.block(mover.id);
      }
    }
  });
}
```

native 中心为无操作（无追加阻挡）；多包平手关系按[handler 依赖与组合诊断](mod-composition.md)冻结。真实场景中原生交换成立、stay-north 拦南下、no-swap 拦交换、组合全拦，见[场景夹具](../../compat/fixtures/sim-mod-move-obstacle/README.md)。

## 拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条障碍链：丢弃全部追加与暂存 namespace state，取消链只按 native 判定；此前竞争/准入槽的结果不受影响。
- 障碍候选不直接改写世界。输入外 ID、非字符串参数、越权能力（竞争优先级/准入/移动请求/资源）的故障不能被 catch + decline 掩盖；错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。
- 晚期写回失败经玩家缓存重试整轮：追加结果与 namespace state 恰一次提交。停用出错的 replace 后回退 native 障碍判定，健康钩子继续组合。
- 追加集是 tick 内瞬态，不进入检查点；检查点只保存已提交世界与 Mod state。该批内建槽增至 17（现 18，见[JS 移动预检](mod-move-precheck.md)），新增冻结描述和 native 绑定改变 PlanHash；旧计划身份的检查点不会被当作同一玩法静默加载。

## 验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-move-obstacle/README.md)提供原生、stay-north、no-swap 和两包组合；JSON/KKPV 完整 final 一致，障碍判定后保存退出与冷启动续跑等价。专项覆盖输入完整性与对象摘要（含 yielding 与 native 初判）、墙地形无路判定、追加的让位链传播、两独立策略与并集组合、非法写入/外部 ID、能力门、停用回退、晚期失败缓存重试与拒绝回退，结果见[交付记录](../research/notes/g02b-move-obstacle-2026-10-06.md)。

当前不开放放行方向（表示有效性宿主约束）；意图登记期静态预检已开为请求级预检槽（见[JS 移动预检](mod-move-precheck.md)，其原生判定保持 native）、摘要中的安全模式主人（creep 阻挡的安全模式分支现只能依赖 `nativeBlocked` 初判，后续切片可附带以增强可解释性）、障碍槽自身的链预演（准入槽的 `evaluate()` 已开放，障碍追加后无第二预演点）、G04 地形代价与执行效果。整房请求视野经准入槽 `ctx.args.requests` 提供。
