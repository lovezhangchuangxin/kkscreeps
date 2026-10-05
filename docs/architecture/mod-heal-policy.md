# JS 治疗策略（N2c 第二片）

`builtin:creep.heal/rangedHeal` 的 JS 处理器可用 `ctx.combat.applyHeal(targetId, amount)` 指定同房治疗目标和数值。沿既有具名动作槽组合，未贡献处理器时继续 native 直调；不按包 ID 分派规则。

```js
export default function register(api) {
  for (const action of ['heal', 'rangedHeal']) {
    api.handlers.replace(`builtin:creep.${action}`, { id: action }, (ctx) => {
      const target = ctx.combat.get(ctx.args.id);
      if (!target?.body || target.hits <= 0 || target.spawning) return api.decline('unavailable');
      ctx.combat.applyHeal(target.id, Math.ceil((target.hitsMax - target.hits) / 2));
    });
  }
}
```

## 表示与动作边界

- 参数为原始 ID 字符串和 **0..=1e9 整数**；SDK/op/引擎各自校验。该上限是表示约束，不是 HEAL 部件功率。零治疗合法，仍记治疗事件。
- 仅 heal/rangedHeal 槽开放；执行者与目标都须为本房有效、已出生 creep，目标须存活，允许自愈。结构修理、自有对象生命值、复活和身体部件新增不经此能力开放。
- 读面复用[战斗视图](mod-damage-policy.md#读面与表示边界)：`get` 返回脱离副本，body/spawning 可用于作者正常拒绝；无目标返回 null。链内 hits/body 不包含尚未落地的伤害或治疗。
- JS 已决定数值和目标，应用端不再检查距离、阵营、安全模式或重算部件/强化功率。`request` 仍执行完整 native 预检和 12/4 基础功率；玩家原 `Creep.heal/rangedHeal` 的部件、距离等预检与意图清洗仍生效，本批未开放它们的策略替换。
- `applyHeal` 与 request/applyDamage/cancel/native 共用单向 consumed；before 成功应用后 native 中心跳过，再次写入是故障。不同动作互斥和执行次序仍遵守原 `EXECUTION_ORDER`；同一 creep 同时提交 heal 与 attack 时，heal 会压制 attack，不能据此构造双份效果。

## 延迟应用与恢复

native 和 JS 共用治疗登记器：累计 `heal_to_apply`、设置执行者/目标动作日志并产生 `EVENT_HEAL`，事件 amount 是请求量，healType 由所属近/远程槽决定。累计量溢出在写候选前拒绝。治疗不会立即改 hits，也不会在登记时按缺失生命截断：满血目标本轮仍可能受伤。

移动后，`damage::apply_pending_hits` 统一计算 `oldHits - damage + heal`，钳到身体耐久上限后再判断死亡。净变化时重分配既有部件耐久；净减少时继续处理 CARRY 缩容掉落。治疗能抵消同轮致死伤害；不足以抵消时仍走既有死亡/墓碑路径，治疗本身不创造新身体或复活已移除对象。

局部恢复复用[战斗链边界](creep-combat-slot.md#局部恢复边界)。引擎保存原始受影响集，策略另选目标由运行时首触日志记录对象与既有伤害/治疗条目；after/wrap 正常拒绝恢复本动作的队列、日志、事件与 namespace state，保留此前成功动作、治疗和伤害。没有新增持久状态域。

能力仅在所属处理器同步期内有效。当前调用中的非法数值、目标越界、重复消耗或已结束处理器引用是故障，catch + decline 不能发布本轮。跨调用旧 token 在候选访问前拒绝，捕获后可继续新调用，契约与既有对象/伤害能力一致。晚期玩家写回失败沿缓存重试，realm 重新装载原包，治疗与 namespace state 恰一次提交。

## 验收与剩余范围

[独立 JS 场景](../../compat/fixtures/sim-mod-heal-policy/README.md)提供固定治疗和缺失生命比例治疗包，以及无 Mod 原生对照。六种场景各验证 JSON/KKPV；分别在预伤害后和治疗完成后退出进程再恢复，完整 final（排除 CPU 桶）、state、组合与 PlanHash 与连续运行相等。

运行时覆盖近/远程公式、延迟读面、零/上界、自愈、身体恢复、同轮致死伤害抵消、远处目标、拒绝保留此前伤害/治疗及转账/移动、非法数值和目标、孵化目标、能力生命周期与缓存重试。

N2c 仍待群体目标批处理、反击策略、TOUGH 默认算法替换、移动仲裁、自有对象伤害 trait 和玩家预检策略替换。自定义治疗与[单体伤害](mod-damage-policy.md)是已交付切片，不能代表完整战斗规则或完整 N2c 已实现。
