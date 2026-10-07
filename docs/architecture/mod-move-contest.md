# JS 移动竞争优先级策略

2026-10-06 已开放 `builtin:room.moveContest` 房间级槽。独立 JS 包可改变同格竞争者的排序：输入提供全部竞争格与默认评分要素，JS 通过优先级数值候选调整排序，宿主按 优先级 → 默认四键 → 登记序 稳定排序选胜。这只是排序切片：准入门槛已另行开放为独立槽（见[JS 移动准入](mod-move-admission.md)），获准集合与执行效果仍由原生路径决定（G03/G04 另行开放）。

## 输入与结算

全部 move/pull 意图登记完成后、仲裁开始前，房间内**存在多竞争者目的格**且有本槽贡献时，整房一次进入 JS；无贡献或无竞争格的房间不进入 realm，不构造输入（默认路径零开销，改造前后微基准无可测量回归，见[交付记录](../research/notes/g01-move-contest-2026-10-06.md)）。`ctx.args.contests` 按目的格给出，竞争者按登记序排列、**不截断**：

| 字段 | 含义 |
|---|---|
| `contests[].x/y` | 被竞争的目的格 |
| `contenders[].id/owner` | 竞争者对象 ID 与归属 |
| `contenders[].originX/originY` | 登记时的原格 |
| `contenders[].originRequestCount` | 原格被请求总数（默认评分第一键的原始值） |
| `contenders[].reciprocal` | 对穿：原格请求中存在站在本竞争格上的对象（此时默认第一键取 100） |
| `contenders[].pulled/pulling` | 被拉/拉者（默认评分第二、三键） |
| `contenders[].movePower/weight/powerRatio` | 含 boost 的 MOVE 功率、重量（≥1）与两者之比（默认评分第四键） |

`ctx.target.fields` 恒为 null（房间级槽没有对象目标；`target.id` 是诊断占位符，不可用于对象能力）；本槽不开放移动请求、资源、战斗或对象能力。非 creep 登记者按防御值填充（owner 空串、功率 0、重量 1；其默认评分为全 0，与原生一致——登记路径当前只有 creep）。`ctx.contest` 是槽专属能力（不可枚举、只在处理器同步期有效）：

| 方法 | 语义 |
|---|---|
| `getPriority(id)` | 读取当前选择；未设置为 0 |
| `setPriority(id, value)` | 写入或覆盖一个竞争者的优先级；id 须在本次输入中，value 须为 ±(2^53−1) 内的 JS 安全整数 |

排序键为 `priority` 降序，再回落默认四键（原格请求数/对穿 100、被拉、拉者、功率/重量比），完全平手保持登记序。未设置的竞争者处于 0（默认层）：正值抬升、负值压低、同值不改变原生结果。选胜之后仍按原逻辑取消不可移动或被阻塞者，不自动递补落选者；准入（疲劳/MOVE/被拉门槛）已按 G02.a 开放为独立槽（见[JS 移动准入](mod-move-admission.md)），链取消与执行阶段同源消费准入终值，不再二次复核 `can_move`。

## 独立包组合

seniority 包替换排序（每格 ID 最小者优先，与默认“功率比最高者胜”相反）：

```js
export default function register(api) {
  api.handlers.replace('builtin:room.moveContest', { id: 'seniority' }, (ctx) => {
    for (const cell of ctx.args.contests) {
      let oldest = null;
      for (const info of cell.contenders) {
        if (!oldest || info.id < oldest) oldest = info.id;
      }
      for (const info of cell.contenders) {
        ctx.contest.setPriority(info.id, info.id === oldest ? 1 : -1);
      }
    }
  });
}
```

另一个 heaviest 包用 after 抬升每格重量最大者，可叠加在任意 before/replace 之上：

```js
export default function register(api) {
  api.handlers.after('builtin:room.moveContest', { id: 'heaviest' }, (ctx) => {
    for (const cell of ctx.args.contests) {
      let heaviest = null, best = -1;
      for (const info of cell.contenders) {
        if (info.weight > best) { best = info.weight; heaviest = info.id; }
      }
      ctx.contest.setPriority(heaviest, 2);
    }
  });
}
```

before/中心/replace/after 与其他槽同构：native 中心为无操作（保留 before 设置的优先级，类比受击槽的默认 TOUGH 中心）；after 在整槽成功后执行。多包平手关系按[handler 依赖与组合诊断](mod-composition.md)冻结。真实场景中三种排序选出三个不同胜者、组合由 after 层压回最重者，见[场景夹具](../../compat/fixtures/sim-mod-move-policy/README.md)。

## 拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条竞争链：丢弃全部优先级候选与暂存 namespace state，回退默认四键排序；此前已登记的请求与拉拽关系保持参与仲裁。已登记的其他 creep 请求不受影响。
- 优先级候选不直接改写世界。非法数值（非整数、超出 ±(2^53−1)）、输入外 ID、越权能力（移动请求/资源/战斗/受击）的故障不能被 catch + decline 掩盖；错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。
- 晚期写回失败经玩家缓存重试整轮：竞争结果与 namespace state 恰一次提交，不重复执行玩家代码。停用出错的 replace 后回退 native，健康钩子继续组合。
- 仲裁结果是 tick 内瞬态，不进入检查点；检查点只保存已提交世界与 Mod state。该批内建槽增至 15（现 18，后续批次见[JS 移动准入](mod-move-admission.md)与[JS 移动预检](mod-move-precheck.md)），新增冻结描述和 native 绑定改变 PlanHash；旧计划身份的检查点不会被当作同一玩法静默加载。

## 验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-move-policy/README.md)提供原生、seniority、heaviest 和两包组合；JSON/KKPV 完整 final 一致，竞争后保存退出与冷启动续跑等价。专项覆盖输入完整性与要素、非法数值/外部 ID、能力门、负优先级层、停用回退、晚期失败缓存重试与拒绝回退，结果见[交付记录](../research/notes/g01-move-contest-2026-10-06.md)。

障碍准入的仲裁半边已开放单向追加（见[JS 移动障碍](mod-move-obstacle.md)），疲劳/MOVE/被拉门槛、整房请求视野与链预演见[JS 移动准入](mod-move-admission.md)；静态放行已由[JS 移动预检](mod-move-precheck.md)贯通，移动代价与 pull 登记策略分别见[移动代价](mod-move-cost.md)和[拉拽登记](mod-move-pull-registration.md)；完整执行效果仍待 G04 后续片。意图静态预检已开为[JS 移动预检](mod-move-precheck.md)（否决在竞争输入构造之前生效）。竞争槽输入只含竞争格；房间其余请求经准入槽的 `ctx.args.requests` 整房视野可见。JS 策略的结算开销由 Mod VM 预算统一管控，密集竞争场景的端到端对照随 G30.d 滚动补测。
