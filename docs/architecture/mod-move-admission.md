# JS 移动准入策略

2026-10-06 已开放 `builtin:creep.moveAdmission` 房间级槽。独立 JS 包可替换移动准入门槛（原生为疲劳 0 且有存活 MOVE，或有效被拉）：仲裁选胜后对全部获胜执行者整房一次进入 JS，候选初值即原生 `can_move` 结果，JS 可读可覆盖；准入终值**同源**约束链取消与执行阶段——`movement_execute` 不再按当前身体二次 native 复核，缺项一律未获准（fail-closed）。这只是准入切片：目的格障碍（地形/结构/rampart/安全模式）与静态预检仍由原生判定（G02.b 另行开放），获准集合与让位链属 G03。

## 输入与结算

竞争优先级槽（若有贡献）先运行并选出各目的格胜者；随后仲裁在**链取消之前**暂停，把全部胜者的准入要素整房一次交给 JS；JS 终值确定后才取消未获准者及其阻塞链。房间内没有移动胜者时不进入 realm（无贡献路径零开销）。准入槽的触发面比竞争槽宽：竞争槽只在存在多竞争者目的格时进入 JS，准入槽对装有贡献的房间**只要存在移动胜者**即整房进 realm 并为每个胜者分配输入字符串；密集移动服务器的结算开销由 Mod VM 预算统一管控。`ctx.args.movers` 按对象 ID 升序、不截断：

| 字段 | 含义 |
|---|---|
| `movers[].id/owner` | 执行者对象 ID 与归属 |
| `movers[].originX/originY` | 登记时的原格 |
| `movers[].targetX/targetY` | 获准的目的格 |
| `movers[].oldFatigue` | 意图时疲劳（原生门槛的疲劳输入） |
| `movers[].activeMoves` | 存活 MOVE 部件数（原生门槛的部件输入） |
| `movers[].movePower` | 含 boost 的 MOVE 功率 |
| `movers[].weight` | 移动重量（≥1） |
| `movers[].pulled` | 有效被拉标记：配对复核的集合与旧链取消时点等价（拉者仍指向被拉者且被拉者获准移向拉者当前格，否则解除残标；拉者已移除时保留标记），遍历顺序为胜者对象 ID 升序。同一拉者多目标残标的顺序差异在现有意图流不可达（每拉者每 tick 至多一个 pull 目标）；G03/G04 开放 pull 登记策略时须重审该顺序语义 |

`ctx.target.fields` 恒为 null（房间级槽没有对象目标）；本槽不开放移动请求、资源、战斗或对象能力。非 creep 胜者按防御值填充（owner 空串、部件 0、功率 0、重量 1），原生准入为否——与原生一致。`ctx.admission` 是槽专属能力（不可枚举、只在处理器同步期有效）：

| 方法 | 语义 |
|---|---|
| `getAdmitted(id)` | 读取当前终值；初值即原生结果，输入外对象为 false |
| `setAdmitted(id, admitted)` | 覆盖终值；id 须在本次输入中，admitted 须为布尔 |

准入与排序解耦：竞争槽（`builtin:room.moveContest`）决定谁胜出，准入槽决定胜出者能否执行。放宽准入（如疲劳者放行）不等于绕过障碍——目的格被未获准让位者占据时仍按阻塞链取消。玩家面方向形态 `move()` 在 API 层预检疲劳与 MOVE 部件（同官方），疲劳/无件移动者经朝向相邻 creep 的 `move(target)` 形态或移动槽能力到达仲裁；准入覆盖对这两条通道同样生效。

## 独立包组合

tireless 包 replace 全部放行（忽略疲劳/MOVE 门槛）：

```js
export default function register(api) {
  api.handlers.replace('builtin:creep.moveAdmission', { id: 'tireless' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      ctx.admission.setAdmitted(mover.id, true);
    }
  });
}
```

strict 包 after 收紧配重不足者，可叠加在任意 before/replace 之上——after 层总能把整面放行收回到自身门槛：

```js
export default function register(api) {
  api.handlers.after('builtin:creep.moveAdmission', { id: 'strict' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.movePower < mover.weight) {
        ctx.admission.setAdmitted(mover.id, false);
      }
    }
  });
}
```

native 中心为无操作（保留候选中的原生准入终值，类比竞争槽）；多包平手关系按[handler 依赖与组合诊断](mod-composition.md)冻结。真实场景中原生拒绝无件移动者、tireless 放行、strict 拒绝超载、组合由 after 层压回，见[场景夹具](../../compat/fixtures/sim-mod-move-admission/README.md)。

## 拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条准入链：丢弃全部覆盖与暂存 namespace state，保留原生预填终值（等同没进过 JS）；此前竞争槽的排序结果不受影响。
- 准入候选不直接改写世界。输入外 ID、非布尔终值、越权能力（竞争优先级/移动请求/资源/受击）的故障不能被 catch + decline 掩盖；错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。执行阶段只读准入终值表，缺项（未参与仲裁或已取消）一律未获准。
- 晚期写回失败经玩家缓存重试整轮：准入结果与 namespace state 恰一次提交，不重复执行玩家代码。停用出错的 replace 后回退 native 门槛，健康钩子继续组合。
- 准入终值是 tick 内瞬态，不进入检查点；检查点只保存已提交世界与 Mod state。内建槽现为 16 个，新增冻结描述和 native 绑定改变 PlanHash；旧计划身份的检查点不会被当作同一玩法静默加载。

## 验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-move-admission/README.md)提供原生、tireless、strict 和两包组合；JSON/KKPV 完整 final 一致，准入后保存退出与冷启动续跑等价。专项覆盖输入完整性与要素、原生配对时序（含配对破裂）、放行/收紧两端、非法写入/外部 ID、能力门、停用回退、晚期失败缓存重试与拒绝回退，结果见[交付记录](../research/notes/g02-move-admission-2026-10-06.md)。

当前不开放障碍准入（地形/结构/rampart/安全模式与 `has_static_obstacle`/`check_obstacle_at_xy`，G02.b）、孵化中/spawning 门槛、获准集合与让位链/交换/循环（G03）、移动代价与执行效果（G04）、pull 配对登记策略（保持 native）。准入输入只含胜者，落选者不在其中；整房请求视野属 G03。
