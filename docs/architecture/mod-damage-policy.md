# JS 伤害策略（N2c 首片）

当前 `builtin:creep.attack` 和 `builtin:creep.rangedAttack` 的 JS 处理器可用 `ctx.combat.applyDamage(targetId, amount)` 自行计算、应用伤害。沿既有具名动作槽组合，不增加按包 ID 分派的规则。没有 JS 贡献的槽继续 native 直调。

```js
export default function register(api) {
  api.handlers.replace('builtin:creep.attack', { id: 'formula' }, (ctx) => {
    const target = ctx.combat.get(ctx.args.id);
    if (!target || target.hits <= 0 || target.spawning) return api.decline('unavailable');
    const armor = target.body?.filter((part) => part.type === 'tough' && part.hits > 0).length || 0;
    const damage = Math.ceil(ctx.target.fields.hits * 0.3 / (1 + armor));
    ctx.combat.applyDamage(target.id, damage);
  });
}
```

## 读面与表示边界

五个[战斗动作槽](creep-combat-slot.md)均开放 `ctx.combat.get(id)`，返回同房候选的脱离副本：`id/typeKey/owner/x/y/hits/hitsMax/body/spawning`。creep 的 body 是 `{type,hits,boost}` 数组，结构 body 为 null；部件类型用玩家字符串，未强化的 boost 为 null；spawning 对 creep 为布尔值，对结构为 null。已声明的 Mod 建筑按建筑目录给出耐久上限。缺失、跨房、无耐久类型或 Mod 自有字段对象返回 null；畸形 ID 是能力故障。修改副本不会写回候选，也不会消耗动作。

执行者的 `ctx.target.fields` 新增只读 `hitsMax` 和同形 `body`。creep 伤害仍延迟到对象 tick 应用，链内查询 hits/body 不包含尚未落地的待伤害；结构伤害立即生效。能力仅在所属处理器同步期有效，不能保存到另一个处理器或下一次槽调用后使用。

策略伤害须为 **0..=1e9 的整数**，SDK、op 和引擎分别校验；这是表示上限，不是 ATTACK 功率上限。零伤害合法并记录攻击事件。执行者须为已出生 creep；目标须为同房存活 creep 或宿主已有耐久表示的结构，不能整值替换对象或给无耐久类型补造 hits。目标不存在、无耐久或仍在孵化时是能力故障。

## 默认规则与策略规则

| 入口 | 结算行为 |
|---|---|
| `ctx.combat.request(id)` | 完整原生攻击/治疗：距离、部件/强化功率、安全模式、rampart 改向及反击仍生效 |
| `ctx.combat.applyDamage(id, amount)` | 使用 JS 指定的承伤对象和数值；不重新计算部件/强化功率，不检查距离/阵营/安全模式，不做 rampart 改向，不追加原生近战反击 |

生命值变化、身体耐久分配、死亡、库存掉落、结构击毁/废墟及事件沿共用应用器处理；事件攻击种类由所属近战/远程槽决定。JS 可通过 before 或 replace 接管伤害；能力与 request/cancel/native 中心共用单向 consumed 标记，先到者消耗动作。before 应用伤害后，后续 native 中心跳过，不能叠加一份默认伤害；再次调用写能力是故障。

这个入口不重定义原玩家 `Creep.attack/rangedAttack` 的预检：调用这些玩家方法仍需满足它们的部件、距离和安全模式要求，才能产生原生意图。策略可改为同房其他承伤对象；测试中改向远处且被 rampart 覆盖的目标，证明应用端没有默认距离/护墙否决。任意新意图与玩家 wrapper 的创作面仍须后续开放，不能把此首片称为所有战斗规则完整可替换。

## 拒绝、故障与恢复

Hosted 链复用[战斗局部恢复](creep-combat-slot.md#局部恢复边界)：原始受影响集由引擎快照；策略指定的链外目标由运行时首触日志保存对象和既有延迟队列。结构摧毁使用已有增删日志，撤销废墟、对象移除、索引和 ID 消耗；事件截回链前长度，namespace state 只在成功后移交。after/wrap 的正常拒绝恢复本动作，保留之前成功动作和队列。

当前调用内的非法数值、能力越界、重复写入和已结束处理器的能力访问均为故障，catch 后返回 decline 也不能发布本轮。跨槽调用的旧 token 在候选访问之前即拒绝；沿既有对象能力契约，这个过期引用错误可被调用方捕获，不能写入新调用的候选，也不会自动把新调用毒化。晚期玩家写回失败保留玩家缓存，realm 按原包重建，重试结算伤害及 state 恰一次。公式必须依赖已提交/候选输入或 ctx.state；VM 全局变量不是检查点持久域。

## 验收与剩余范围

[独立 JS 夹具](../../compat/fixtures/sim-mod-damage-policy/README.md)包含固定伤害包、生命值/护甲比例伤害包及原生对照；两包分别选择，直接同时启用会按既有多 replace 冲突规则拒绝。真实 CLI 的 JSON/KKPV 最终状态和进程退出后的检查点续跑相等（排除实际 CPU 桶）。运行时另验证部件读面、远处目标、拒绝保留此前伤害、击毁/库存/空间索引/ID 恢复，以及故障和缓存重试。

此首片不开放自定义治疗、群体目标批处理、反击策略、TOUGH 默认算法替换、移动仲裁、自有对象伤害 trait 或新玩家意图 wrapper。宿主不隐含 TOUGH 减伤；示例包的护甲公式只由 JS 计算。后续按这些消费者继续拆分 N2c，不以单体伤害首片代替完整目标。
