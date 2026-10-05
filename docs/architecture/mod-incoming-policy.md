# JS Creep 受击策略

2026-10-06 已开放 `builtin:creep.receiveDamage` 对象 tick 槽。独立 JS 包可替换默认 TOUGH 公式，或组合护甲、护盾和免疫。攻击动作仍登记减伤前伤害；此槽选择治疗合并前的最终扣血量，不产生玩家意图。

## 输入与结算

房间动作结束后，每个 creep 完成自己的移动、寿命和疲劳处理；仍存在且有正值待伤害的目标调用一次受击链。仅有治疗、原始伤害为零或寿命到期已删除的对象不调用。结构不经过此槽。输入按当时尚未分配受伤的身体计算：

| 字段 | 含义 |
|---|---|
| `ctx.target` | 当前受击 creep；`fields` 是只读候选副本 |
| `ctx.args.rawDamage` | 当轮累计减伤前伤害 |
| `ctx.args.defaultDamage` | 按当前身体算出的[默认 TOUGH 扣血量](creep-incoming-damage.md) |
| `ctx.args.pendingHeal` | 当轮待治疗；不提前修复本轮 TOUGH |
| `ctx.args.sources` | 指向该对象的正值攻击事件摘要，按事件顺序最多 256 项：`sourceId`、减伤前 `amount`、`attackType` |
| `ctx.args.sourceCount` / `sourcesTruncated` | 全部正值来源数量，以及是否截断摘要 |
| `ctx.args.unattributedDamage` | 原始伤害减去**全部**匹配事件的伤害量；截断不使已记录事件成为未归因伤害 |

来源从当前房间事件流提取，不新增持久化伤害账本。原生近战、单体/群体远程、塔、原生反击、JS 单体/批量与显式反击沿共用队列进入此槽；此前正常拒绝的攻击已撤销队列和事件。摘要中的来源可能已在自己的对象 tick 中受伤或死亡。`ctx.combat.get(id)` 和 `list(after, limit)` 只读查询**当前**同房候选，不能当作攻击发生时的历史快照。

入链前数值候选初始化为 `defaultDamage`。`ctx.incoming.getDamage()` 读取当前选择，`setDamage(amount)` 可多次调整，因此 before、wrap、replace、after 可以组合。默认 native 中心保留已准备的值，不覆盖 before 的选择；replace 使用 `rawDamage` 就能完全替换 TOUGH。成功退出后不再次应用 TOUGH，直接扣选中伤害、加待治疗，再沿已有身体分配、死亡/墓碑和 CARRY 超容掉落路径结算。攻击事件 `damage` 保持原始量；选中伤害也不等于治疗后的净 hits 减少量。

无 JS 贡献的路径直接计算原生 TOUGH，不进入 realm、不转换来源摘要。Hosted 输入中的 raw/default/heal 和 `setDamage` 数值必须为 0..=2^53−1 的安全整数；超界是故障，不允许 Number 静默舍入。SDK、原始 op 与引擎候选均校验选中值。无 JS 的原生队列仍使用 i64，最终伤害/治疗算术加宽到 i128 避免中间溢出。

## 独立包组合

护甲包替换公式（这里按活跃 TOUGH 层数计算，不叠加强化比例）：

```js
export default function register(api) {
  api.handlers.replace('builtin:creep.receiveDamage', { id: 'armor' }, (ctx) => {
    const layers = ctx.target.fields.body.filter(p => p.type === 'tough' && p.hits > 0).length;
    ctx.incoming.setDamage(Math.ceil(ctx.args.rawDamage / (1 + layers * 2)));
  });
}
```

另一个护盾包使用 after 吸收当前选择，并把剩余值存入可恢复的 namespace state：

```js
export default function register(api) {
  api.handlers.after('builtin:creep.receiveDamage', { id: 'shield' }, (ctx) => {
    const pools = ctx.state?.pools ?? {};
    const left = pools[ctx.target.id] ?? 18;
    const absorbed = Math.min(left, ctx.incoming.getDamage());
    ctx.incoming.setDamage(ctx.incoming.getDamage() - absorbed);
    ctx.state = { pools: { ...pools, [ctx.target.id]: left - absorbed } };
  });
}
```

仅安装护盾时默认 TOUGH 先计算；同时安装护甲时吸收替换公式的结果。after 使用冻结后的逆序，多个护盾包须按[handler 依赖与组合诊断](mod-composition.md)明确顺序。来源超过 256 项时不得把截断摘要误当作全部攻击来计算逐来源策略。

## 拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条受击链：丢弃本次数值与所有暂存 namespace state，回退到入链前的默认 TOUGH 数值。此前成功的攻击/治疗/事件和目标移动仍保留；拒绝不取消伤害。免疫应成功调用 `setDamage(0)`。
- 数值与 state 候选不直接改写世界。受击槽只开放 incoming 数值和 combat 查询；combat request/cancel/伤害/治疗/反击表、资源转账、移动请求、对象写入均不开放，target.fields 不提供 hits/body 写能力。
- 能力只在所属处理器同步期有效，Promise/thenable、重复 next、非法数值、越界能力及本次调用中的过期能力故障不能被 catch + decline 掩盖。错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。
- 写回失败从已提交身体与 namespace state 重新结算，复用玩家缓存，不重复执行玩家代码或消耗护盾。停用出错的 replace 后回退 native，健康钩子继续组合。检查点只保存已提交身体、Mod state 与计划/停用身份，冷启动重新生成受击数值；没有持久化待受击候选。

内建槽现为 14 个，新增冻结描述和 native 绑定会改变 PlanHash；旧计划身份的检查点不会被当作同一玩法静默加载。此槽的非默认贡献经整槽宿主接线，目前没有对应的纯 Rust 自定义受击处理器入口。

## 验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-incoming-policy/README.md)提供原生、独立护甲、独立护盾和两包组合；JSON/KKPV 完整 final 一致，受击后保存退出与冷启动续跑等价。专项覆盖五位置拒绝、非法原始 op、能力过期、来源截断、不同伤害来源、停用回退和晚期失败缓存重试；结果见[交付记录](../research/notes/n2c-incoming-policy-2026-10-06.md)。

当前不开放结构受击、自有对象伤害 trait、按实际净伤害自动反击或 power 免疫语义。移动竞争排序已另行开放（见[JS 移动竞争](mod-move-contest.md)），移动准入/完整仲裁、玩家预检策略替换与可配置批量预算仍待实现。来源提取在 Hosted 路径扫描房间事件流，密集受击场景性能尚未单独采集。
