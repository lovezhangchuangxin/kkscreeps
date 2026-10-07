# JS 移动请求预检策略

2026-10-06 已开放 `builtin:room.movePrecheck` 房间级槽。独立 JS 包可对移动**请求登记**做单向收紧：全部意图与移动动作槽登记完成后、竞争输入构造之前整房一次进入 JS，JS 通过 `ctx.precheck.block` 否决请求；被否决的请求从登记矩阵移除，**等同从未登记**——不参与竞争评分、不让位、不占格。当前已经接入的决策点为：请求预检（本槽）→ 竞争排序（[G01](mod-move-contest.md)）→ 准入门槛与获准集合（[G02.a/G03.a](mod-move-admission.md)）→ 仲裁障碍追加（[G02.b](mod-move-obstacle.md)）→ 取消链（native）。

## 与原生预检和障碍槽的关系

- 原生意图期静态预检 `has_static_obstacle`（结构障碍/私有敌方 rampart）保持 native 并在登记时点生效：**被原生挡下的请求不入矩阵、不进本槽输入**。两个登记入口 `intent_move`（玩家意图）与 `slot_request_direction`（移动槽能力）共享同一矩阵，本槽在矩阵上统一否决，天然同源。
- **放行方向尚未实现**：意图期 `has_static_obstacle` 已拒绝的请求不会进入本槽，仲裁期还会对结构/私有 rampart/墙地形复检。目前两处 native 否决尚未统一到可替换策略，这属于 G02 剩余玩法消费者，不是必须永久保留的宿主不变式。坐标/ID 有效性及最终占格一致性仍由宿主保证；G02.d 将静态阻挡决策、玩家预检和仲裁复检一并接线后再验收放行。
- **请求级 ≠ 胜者级**：本槽否决的是请求（竞争之前）；障碍槽追加的是胜者阻挡（选胜之后）。否决强竞争者的请求会让同格弱者独占目的格胜出并移动；障碍槽挡下胜者则无人可动。两级语义互补，可同房组合。
- 被否决请求的下游与"从未登记"逐步一致：spawn 放置的 `is_tile_busy` 读不到它，准入槽的整房请求视野（`ctx.args.requests`）与竞争评分也不含它。`old_fatigue` 中的登记时残项仅被胜者读取，无害。

## 输入与结算

房间内存在移动请求且有本槽贡献时整房一次进入 JS；无贡献或无请求的房间不进入 realm。`ctx.args.requests` 按对象 ID 升序、不截断（单执行者单请求不变量下每对象至多一条）：

| 字段 | 含义 |
|---|---|
| `requests[].moverId/owner` | 请求者对象 ID 与归属（非 creep 按防御值：owner 空串） |
| `requests[].originX/originY` | 登记时的原格 |
| `requests[].targetX/targetY` | 请求目的格 |
| `requests[].movePower/weight` | 含 boost 的 MOVE 功率与移动重量（防御值 0/1） |
| `requests[].pulled/pulling` | 登记时点的原始拉拽标记（配对一致性复核发生在准入预填时点） |
| `requests[].terrainWall/terrainSwamp` | 目的格地形（墙/沼泽） |
| `requests[].hasRoad` | 目的格有路 |
| `requests[].objects[]` | 目的格对象占位摘要：`id/kind/owner/isPublic/structureType`（与障碍槽输入同形；不携带让位预期——预检时点尚未选胜） |

`ctx.target.fields` 恒为 null（房间级槽没有对象目标）；本槽不开放移动请求改向、竞争优先级、准入、障碍或资源能力。`ctx.precheck` 是槽专属能力（不可枚举、只在处理器同步期有效）：

| 方法 | 语义 |
|---|---|
| `isBlocked(id)` | 读取当前判定：原生初值恒为否（列表请求均已通过原生静态预检），之外只有已追加的否决 |
| `block(id)` | 追加否决；id 须在本次输入中，单向、不可撤销 |

否决集在槽成功退出后由宿主在竞争输入构造之前统一应用（`Movement::apply_precheck_blocks` → 逐请求 `remove_requests`）；取消链、准入、竞争与执行阶段对被否决请求一无所知。

## 独立包组合

fenced 包 replace 区域管制（西区禁入）：

```js
export default function register(api) {
  api.handlers.replace('builtin:room.movePrecheck', { id: 'fenced' }, (ctx) => {
    for (const r of ctx.args.requests) {
      if (r.targetX < 10) ctx.precheck.block(r.moverId);
    }
  });
}
```

courtesy 包 after 功率让行（配重过剩的快车让行，弱者独占竞争格），可与任意 replace/before 叠加——否决集在同一候选上取并集：

```js
export default function register(api) {
  api.handlers.after('builtin:room.movePrecheck', { id: 'courtesy' }, (ctx) => {
    for (const r of ctx.args.requests) {
      if (r.movePower > r.weight && !ctx.precheck.isBlocked(r.moverId)) {
        ctx.precheck.block(r.moverId);
      }
    }
  });
}
```

native 中心为无操作（无否决）；多包平手关系按[handler 依赖与组合诊断](mod-composition.md)冻结。真实场景中原生竞争胜出、fenced 拦西区、courtesy 让行改写竞争结果、组合并集，见[场景夹具](../../compat/fixtures/sim-mod-move-precheck/README.md)。

## 拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条预检链：丢弃全部否决与暂存 namespace state，登记矩阵保持原状（等同没进过 JS）；后续竞争/准入/障碍槽不受影响。这是**整房级**回退边界。
- 预检候选不直接改写世界。输入外 ID、非字符串参数、越权能力（竞争优先级/准入/障碍/移动请求/资源）的故障不能被 catch + decline 掩盖；错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。
- 晚期写回失败经玩家缓存重试整轮：否决结果与 namespace state 恰一次提交。停用出错的 replace 后回退 native 预检，健康钩子继续组合。
- 否决集是 tick 内瞬态，不进入检查点；检查点只保存已提交世界与 Mod state。本批新增第 18 个内建槽、revision v11（冻结描述、原生绑定与 catalog 槽计数断言同批同步）：计划身份变更经可执行文件与包身份间接反映在检查点一致性校验中，旧检查点不会被当作同一玩法静默加载。

## 验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-move-precheck/README.md)提供原生、fenced、courtesy 和两包组合；JSON/KKPV 完整 final 一致，预检后保存退出与冷启动续跑等价。专项覆盖：引擎侧 输入完整性（字段/排序/防御值/地形与占位摘要）、**否决等同从未登记**的下游等价（竞争胜负易主、准入视野不含、is_tile_busy 不占）与 拉者被否决后被拉者随链取消的基线对照；运行时侧 字段形态 JS 通道校验 + 原生对照、decline 回退、replace/after 组合 isBlocked、准入与障碍槽内调用走能力门且不可被 decline 掩盖。结果见[交付记录](../research/notes/g02c-move-precheck-2026-10-06.md)。

当前不开放：预检放行方向（待 G02.d 贯通静态阻挡消费者）、玩家面 `move()` 返回值的预检策略（属 G07 玩家预检族——本槽改的是结算侧登记，玩家 API 返回路径另行盘点）、障碍槽放行（待 G02.d；当前 native 仍否决）、pull 登记拒绝策略已开放（见[拉拽登记](mod-move-pull-registration.md)）。
