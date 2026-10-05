# JS 群体伤害策略（N2c 第三片）

`builtin:creep.rangedMassAttack` 的 JS 处理器可用 `ctx.combat.list` 自行选择目标，调用 `applyDamageBatch` 一次应用多个伤害量。没有 JS 贡献时仍 native 直调，不增加包 ID 专用规则。

```js
export default function register(api) {
  api.handlers.replace('builtin:creep.rangedMassAttack', { id: 'formula' }, (ctx) => {
    const requests = [];
    let after = '';
    do {
      const page = ctx.combat.list(after, 100);
      for (const target of page.objects) {
        const distance = Math.max(Math.abs(target.x - ctx.target.fields.x), Math.abs(target.y - ctx.target.fields.y));
        if (target.owner && target.owner !== ctx.target.fields.owner && !target.spawning && target.hits > 0 && distance <= 6) {
          requests.push({ id: target.id, amount: Math.ceil(40 / (distance + 1)) });
        }
      }
      after = page.next;
    } while (after !== null);
    ctx.combat.applyDamageBatch(requests);
  });
}
```

## 查询与批量表示

- `list(after = '', limit = 100)` 在五个战斗槽开放，返回 `{objects,next}`。对象形状与[战斗 get](mod-damage-policy.md#读面与表示边界)相同，按对象 ID 升序，页为脱离副本。包含执行者、孵化和零耐久对象；缺少耐久表示的对象、自有 Mod 字段对象和跨房对象不列出，资格由 JS 筛选。
- 空游标从头查询；非空游标须为合法 ID，继续查询严格大于该 ID 的对象，游标对象已经删除仍可继续。`next` 是本页最后一个 ID，有后页时返回它，否则为 null。每页限 **1..256 整数**；每次读取当前候选，不保证修改期间的跨页快照一致性。
- `applyDamageBatch([{id,amount},...])` 仅在 rangedMassAttack 槽开放，最多 **256 项**。ID 须为原始字符串；amount 为 **0..=1e9 整数**，与[单体伤害](mod-damage-policy.md)同域。条目只读取 id/amount，其他字段不参与应用。
- 执行者须为已出生 creep，目标须为同房存活、已出生 creep 或有耐久结构。同一实际 ID 不可重复，即使使用 `kk3` 与规范长形式两个不同字符串指向同一对象也拒绝。
- 整批检查全部 ID、数值、资格、唯一性及待伤害累计表示域后，才按输入顺序应用；非法最后一项也不能先扣前面的结构生命或消耗 ID。空批量成功，记录群体动作日志并消耗本动作，不生成攻击事件。

SDK 先读取有界数组的原始值、复制成 id/amount 条目；op 在转换前检查数组与限额，保留原始浮点校验，不把小数截成整数。getter/Proxy 转换期间不借用 OpState 或候选，转换完成后重查 token、绑定、故障与 consumed；内层取消/批量写或故障不能绕过恰一次门。Rust 公共宿主和引擎也复核限额，不能仅依赖 JS 预检。

## 默认规则、结算与恢复

JS 决定目标、阵营筛选、距离与伤害公式；应用端不追加原生射程、安全模式、部件/强化功率、距离衰减或 rampart 跳过/改向，也不生成默认反击。原玩家 `Creep.rangedMassAttack` 的预检、意图身份清洗与动作互斥仍生效，玩家预检策略替换尚未开放。

批量与 request/applyDamage/applyHeal/cancel/native 共用单向 consumed；before 应用后 native 中心跳过，再次写能力是故障。每个目标按输入顺序记录 `EVENT_ATTACK`，attackType 为群体远程种类；零伤害也记攻击事件。结构立即扣耐久/击毁，creep 仍累计延迟伤害，在对象 tick 与治疗合并后分配身体并判死亡，沿既有库存/死亡/墓碑/废墟路径。

拒绝边界复用[战斗链局部恢复](creep-combat-slot.md#局部恢复边界)：原始范围三内快照由引擎保存，JS 指定的全部额外目标由运行时首触日志保存对象与既有伤害/治疗条目。链内增删日志恢复被击毁结构、废墟、spawn 连带删除的孵化 creep、空间索引及 ID。after/wrap 正常拒绝只撤销本动作，保留此前伤害/治疗、转账和后续移动；成功后才移交 namespace state。

当前调用中的非法条目、越界、重复消耗、转换和处理器过期引用均为故障，catch + decline 不能发布本轮；跨调用旧 token 仍在候选访问前拒绝，捕获后可继续新调用。晚期写回失败沿缓存重试，批量及 Mod state 恰一次。没有新增检查点状态域。

## 验收与剩余范围

[独立 JS 场景](../../compat/fixtures/sim-mod-mass-policy/README.md)包括六格固定伤害、距离衰减公式及无 Mod 对照，两通道完整 final（排除 CPU 桶）和 PlanHash 一致；批量完成后退出进程再恢复，与连续运行的全部目标/state/组合相等。引擎直接验证“最后一项非法也不写前项”、队列溢出、分页与空/上界；运行时验证多目标拒绝、结构及孵化对象恢复、转换重入、故障、生命周期和缓存重试。

当前批量限额固定为 256，超过限额是能力故障，不能在同一动作中重复分批写入。反击策略、TOUGH 默认算法替换、移动仲裁、自有对象伤害 trait、玩家预检替换以及可配置批量预算仍待开放；本切片不代表完整 N2c 或所有战斗规则完成。

本切片的构建、专项和兼容性结果见[交付记录](../research/notes/n2c-mass-policy-2026-10-06.md)。
