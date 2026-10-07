# 战斗领域：伤害、治疗与受击策略

战斗能力的目标与数值策略由 JS 决定，生命值、延迟队列、死亡和原子发布由引擎负责。玩家预检与动作互斥继续适用，受击阶段共用原生伤害/治疗应用。

## 代码与维护入口

能力契约在 `kk-mod-api/src/combat.rs` 与 `incoming.rs`，规则在 `kk-engine/src/settle/creep/combat/` 和 `settle/damage.rs`；对象首触与延迟队列恢复统一由引擎 `ObjectJournal` 提供，runtime 只保存调用期句柄。

## 单体伤害

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

### 单体伤害：读面与表示边界

五个[战斗动作槽](creep-combat-slot.md)均开放 `ctx.combat.get(id)`，返回同房候选的脱离副本：`id/typeKey/owner/x/y/hits/hitsMax/body/spawning`。creep 的 body 是 `{type,hits,boost}` 数组，结构 body 为 null；部件类型用玩家字符串，未强化的 boost 为 null；spawning 对 creep 为布尔值，对结构为 null。已声明的 Mod 建筑按建筑目录给出耐久上限。缺失、跨房、无耐久类型或 Mod 自有字段对象返回 null；畸形 ID 是能力故障。修改副本不会写回候选，也不会消耗动作。

执行者的 `ctx.target.fields` 新增只读 `hitsMax` 和同形 `body`。creep 伤害仍延迟到对象 tick 应用，链内查询 hits/body 不包含尚未落地的待伤害；结构伤害立即生效。能力仅在所属处理器同步期有效，不能保存到另一个处理器或下一次槽调用后使用。

策略伤害须为 **0..=1e9 的整数**，SDK、op 和引擎分别校验；这是表示上限，不是 ATTACK 功率上限。零伤害合法并记录攻击事件。执行者须为已出生 creep；目标须为同房存活 creep 或宿主已有耐久表示的结构，不能整值替换对象或给无耐久类型补造 hits。目标不存在、无耐久或仍在孵化时是能力故障。

### 单体伤害：默认规则与策略规则

| 入口 | 结算行为 |
|---|---|
| `ctx.combat.request(id)` | 原生主攻击/治疗：距离、部件/强化功率、安全模式和 rampart 改向仍生效；未声明反击表时保持原生反击 |
| `ctx.combat.applyDamage(id, amount)` | 使用 JS 指定的承伤对象和数值；不重新计算部件/强化功率，不检查距离/阵营/安全模式，不做 rampart 改向，不追加原生近战反击 |

amount 为减伤前伤害；creep 在对象 tick 合并累计伤害后执行[默认 TOUGH 减伤](creep-incoming-damage.md)，可经[JS 受击链](#受击)替换或组合，再合并治疗。结构立即扣耐久。生命值变化、身体耐久分配、死亡、库存掉落、结构击毁/废墟及事件沿共用应用器处理；事件 damage 保持减伤前量，攻击种类由所属近战/远程槽决定。JS 可通过 before 或 replace 接管攻击公式；能力与 request/cancel/native 中心共用单向 consumed 标记，先到者消耗动作。before 应用伤害后，后续 native 中心跳过，不能叠加一份默认伤害；再次调用写能力是故障。

这个入口不重定义原玩家 `Creep.attack/rangedAttack` 的预检：调用这些玩家方法仍需满足它们的部件、距离和安全模式要求，才能产生原生意图。策略可改为同房其他承伤对象；测试中改向远处且被 rampart 覆盖的目标，证明应用端没有默认距离/护墙否决。任意新意图与玩家 wrapper 的创作面仍须后续开放，不能把此首片称为所有战斗规则完整可替换。

### 单体伤害：拒绝、故障与恢复

Hosted 链复用[战斗局部恢复](creep-combat-slot.md#局部恢复边界)：原始受影响集由引擎快照；策略指定的链外目标由运行时首触日志保存对象和既有延迟队列。结构摧毁使用已有增删日志，撤销废墟、对象移除、索引和 ID 消耗；事件截回链前长度，namespace state 只在成功后移交。after/wrap 的正常拒绝恢复本动作，保留之前成功动作和队列。

当前调用内的非法数值、能力越界、重复写入和已结束处理器的能力访问均为故障，catch 后返回 decline 也不能发布本轮。跨槽调用的旧 token 在候选访问之前即拒绝；沿既有对象能力契约，这个过期引用错误可被调用方捕获，不能写入新调用的候选，也不会自动把新调用毒化。晚期玩家写回失败保留玩家缓存，realm 按原包重建，重试结算伤害及 state 恰一次。公式必须依赖已提交/候选输入或 ctx.state；VM 全局变量不是检查点持久域。

### 单体伤害：验收与剩余范围

[独立 JS 夹具](../../compat/fixtures/sim-mod-damage-policy/README.md)包含固定伤害包、生命值/护甲比例伤害包及原生对照；两包分别选择，直接同时启用会按既有多 replace 冲突规则拒绝。真实 CLI 的 JSON/KKPV 最终状态和进程退出后的检查点续跑相等（排除实际 CPU 桶）。运行时另验证部件读面、远处目标、拒绝保留此前伤害、击毁/库存/空间索引/ID 恢复，以及故障和缓存重试。

自定义治疗已在[第二片](#治疗)开放；群体目标查询与批处理已在[第三片](#群体伤害)开放；[反击表](#反击)已在第四片开放；原生 TOUGH 减伤已交付，[JS 受击公式替换与护盾组合](#受击)已交付；移动仲裁、自有对象伤害 trait 或新玩家意图 wrapper 仍待开放。示例包按活跃 TOUGH 数量计算的护甲为 JS 自定公式；若身体同时强化 TOUGH，将叠加默认减伤，详见[作者边界](creep-incoming-damage.md#js-作者与恢复边界)。后续按这些消费者继续拆分 N2c，不以单体伤害首片代替完整目标。

## 治疗

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

### 治疗：表示与动作边界

- 参数为原始 ID 字符串和 **0..=1e9 整数**；SDK/op/引擎各自校验。该上限是表示约束，不是 HEAL 部件功率。零治疗合法，仍记治疗事件。
- 仅 heal/rangedHeal 槽开放；执行者与目标都须为本房有效、已出生 creep，目标须存活，允许自愈。结构修理、自有对象生命值、复活和身体部件新增不经此能力开放。
- 读面复用[战斗视图](#单体伤害读面与表示边界)：`get` 返回脱离副本，body/spawning 可用于作者正常拒绝；无目标返回 null。链内 hits/body 不包含尚未落地的伤害或治疗。
- JS 已决定数值和目标，应用端不再检查距离、阵营、安全模式或重算部件/强化功率。`request` 仍执行完整 native 预检和 12/4 基础功率；玩家原 `Creep.heal/rangedHeal` 的部件、距离等预检与意图清洗仍生效，本批未开放它们的策略替换。
- `applyHeal` 与 request/applyDamage/cancel/native 共用单向 consumed；before 成功应用后 native 中心跳过，再次写入是故障。不同动作互斥和执行次序仍遵守原 `EXECUTION_ORDER`；同一 creep 同时提交 heal 与 attack 时，heal 会压制 attack，不能据此构造双份效果。

### 治疗：延迟应用与恢复

native 和 JS 共用治疗登记器：累计 `heal_to_apply`、设置执行者/目标动作日志并产生 `EVENT_HEAL`，事件 amount 是请求量，healType 由所属近/远程槽决定。累计量溢出在写候选前拒绝。治疗不会立即改 hits，也不会在登记时按缺失生命截断：满血目标本轮仍可能受伤。

移动后，`damage::apply_pending_hits` 统一计算 `oldHits - damage + heal`，钳到身体耐久上限后再判断死亡。净变化时重分配既有部件耐久；净减少时继续处理 CARRY 缩容掉落。治疗能抵消同轮致死伤害；不足以抵消时仍走既有死亡/墓碑路径，治疗本身不创造新身体或复活已移除对象。

局部恢复复用[战斗链边界](creep-combat-slot.md#局部恢复边界)。引擎保存原始受影响集，策略另选目标由运行时首触日志记录对象与既有伤害/治疗条目；after/wrap 正常拒绝恢复本动作的队列、日志、事件与 namespace state，保留此前成功动作、治疗和伤害。没有新增持久状态域。

能力仅在所属处理器同步期内有效。当前调用中的非法数值、目标越界、重复消耗或已结束处理器引用是故障，catch + decline 不能发布本轮。跨调用旧 token 在候选访问前拒绝，捕获后可继续新调用，契约与既有对象/伤害能力一致。晚期玩家写回失败沿缓存重试，realm 重新装载原包，治疗与 namespace state 恰一次提交。

### 治疗：验收与剩余范围

[独立 JS 场景](../../compat/fixtures/sim-mod-heal-policy/README.md)提供固定治疗和缺失生命比例治疗包，以及无 Mod 原生对照。六种场景各验证 JSON/KKPV；分别在预伤害后和治疗完成后退出进程再恢复，完整 final（排除 CPU 桶）、state、组合与 PlanHash 与连续运行相等。

运行时覆盖近/远程公式、延迟读面、零/上界、自愈、身体恢复、同轮致死伤害抵消、远处目标、拒绝保留此前伤害/治疗及转账/移动、非法数值和目标、孵化目标、能力生命周期与缓存重试。

群体目标批处理已在[第三片](#群体伤害)开放；[反击表](#反击)已在第四片开放；[原生 TOUGH 减伤](creep-incoming-damage.md)已交付，治疗在默认减伤后合并、不会提前修复护甲。[JS 受击公式替换与护盾组合](#受击)也已交付，治疗在受击链选中伤害之后合并。N2c 仍待移动仲裁、自有对象伤害 trait 和玩家预检策略替换。自定义治疗与[单体伤害](#单体伤害)是已交付切片，不能代表完整战斗规则或完整 N2c 已实现。

## 群体伤害

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

### 群体伤害：查询与批量表示

- `list(after = '', limit = 100)` 在五个战斗槽开放，返回 `{objects,next}`。对象形状与[战斗 get](#单体伤害读面与表示边界)相同，按对象 ID 升序，页为脱离副本。包含执行者、孵化和零耐久对象；缺少耐久表示的对象、自有 Mod 字段对象和跨房对象不列出，资格由 JS 筛选。
- 空游标从头查询；非空游标须为合法 ID，继续查询严格大于该 ID 的对象，游标对象已经删除仍可继续。`next` 是本页最后一个 ID，有后页时返回它，否则为 null。每页限 **1..256 整数**；每次读取当前候选，不保证修改期间的跨页快照一致性。
- `applyDamageBatch([{id,amount},...])` 仅在 rangedMassAttack 槽开放，最多 **256 项**。ID 须为原始字符串；amount 为 **0..=1e9 整数**，与[单体伤害](#单体伤害)同域。条目只读取 id/amount，其他字段不参与应用。
- 执行者须为已出生 creep，目标须为同房存活、已出生 creep 或有耐久结构。同一实际 ID 不可重复，即使使用 `kk3` 与规范长形式两个不同字符串指向同一对象也拒绝。
- 整批检查全部 ID、数值、资格、唯一性及待伤害累计表示域后，才按输入顺序应用；非法最后一项也不能先扣前面的结构生命或消耗 ID。空批量成功，记录群体动作日志并消耗本动作，不生成攻击事件。

SDK 先读取有界数组的原始值、复制成 id/amount 条目；op 在转换前检查数组与限额，保留原始浮点校验，不把小数截成整数。getter/Proxy 转换期间不借用 OpState 或候选，转换完成后重查 token、绑定、故障与 consumed；内层取消/批量写或故障不能绕过恰一次门。Rust 公共宿主和引擎也复核限额，不能仅依赖 JS 预检。

### 群体伤害：默认规则、结算与恢复

JS 决定目标、阵营筛选、距离与攻击公式；应用端不追加原生射程、安全模式、部件/强化功率、距离衰减或 rampart 跳过/改向，也不生成默认反击。批量 amount 为减伤前伤害，creep 的最终扣血统一在对象 tick 经过[默认 TOUGH](creep-incoming-damage.md)及可选的[JS 受击链](#受击)。原玩家 `Creep.rangedMassAttack` 的预检、意图身份清洗与动作互斥仍生效，玩家预检策略替换尚未开放。

批量与 request/applyDamage/applyHeal/cancel/native 共用单向 consumed；before 应用后 native 中心跳过，再次写能力是故障。每个目标按输入顺序记录 `EVENT_ATTACK`，attackType 为群体远程种类；零伤害也记攻击事件。结构立即扣耐久/击毁，creep 仍累计延迟伤害，在对象 tick 与治疗合并后分配身体并判死亡，沿既有库存/死亡/墓碑/废墟路径。

拒绝边界复用[战斗链局部恢复](creep-combat-slot.md#局部恢复边界)：原始范围三内快照由引擎保存，JS 指定的全部额外目标由运行时首触日志保存对象与既有伤害/治疗条目。链内增删日志恢复被击毁结构、废墟、spawn 连带删除的孵化 creep、空间索引及 ID。after/wrap 正常拒绝只撤销本动作，保留此前伤害/治疗、转账和后续移动；成功后才移交 namespace state。

当前调用中的非法条目、越界、重复消耗、转换和处理器过期引用均为故障，catch + decline 不能发布本轮；跨调用旧 token 仍在候选访问前拒绝，捕获后可继续新调用。晚期写回失败沿缓存重试，批量及 Mod state 恰一次。没有新增检查点状态域。

### 群体伤害：验收与剩余范围

[独立 JS 场景](../../compat/fixtures/sim-mod-mass-policy/README.md)包括六格固定伤害、距离衰减公式及无 Mod 对照，两通道完整 final（排除 CPU 桶）和 PlanHash 一致；批量完成后退出进程再恢复，与连续运行的全部目标/state/组合相等。引擎直接验证“最后一项非法也不写前项”、队列溢出、分页与空/上界；运行时验证多目标拒绝、结构及孵化对象恢复、转换重入、故障、生命周期和缓存重试。

当前批量限额固定为 256，超过限额是能力故障，不能在同一动作中重复分批写入。[反击表](#反击)已在第四片开放；原生 TOUGH 减伤已交付，[JS 受击公式替换与护盾组合](#受击)已交付；移动仲裁、自有对象伤害 trait、玩家预检替换以及可配置批量预算仍待开放；本切片不代表完整 N2c 或所有战斗规则完成。

本切片的构建、专项和兼容性结果见[交付记录](../research/notes/n2c-mass-policy-2026-10-06.md)。

## 反击

攻击链处理器可以在执行前调用 `ctx.combat.setRetaliation([{id,amount},...])`，声明本动作的反击表，再保留 native 中心或使用 request/JS 伤害能力。表按实际受击对象匹配；反击由目标指向原执行者，直接登记一次伤害，不递归执行另一条攻击链。

```js
export default function register(api) {
  api.handlers.before('builtin:creep.attack', { id: 'counter' }, (ctx) => {
    // 仅为当前原请求目标配置固定反击；改向或 rampart 命中另按实际 ID 匹配。
    ctx.combat.setRetaliation([{ id: ctx.args.id, amount: 7 }]);
  });
}
```

### 反击：声明与组合

- 仅 attack/rangedAttack/rangedMassAttack 三槽开放。必须在动作 consumed 前声明一次；声明本身不消耗攻击，不登记伤害或事件。其他槽、重复声明、cancel/native/request/JS 伤害之后声明均为故障，catch + decline 不能发布本轮。
- 存在表就替换本动作全部默认反击，未列出的实际受击目标反击量为 0；空表可禁用反击。未声明时，native 维持现有近战反击，JS applyDamage/applyDamageBatch 维持无隐式反击。
- 表最多 256 项；id 为原始字符串，amount 为 0..=1e9 整数。目标须为本房存活、已出生 creep 或有耐久结构，按实际 ID 排除重复。SDK/op/引擎完整验证后才保存表，转换期间允许 getter 重入但不持候选/OpState 借用，完成后重查能力与故障。
- 表是当前调用内的不可变值，不进入世界或检查点，不跨对象、槽或 tick。多个 Mod 可以分别提供反击表与主攻击公式；只有一个处理器负责声明表，其他处理器保留该表并执行/继续链。多个声明者会报故障，不采用静默覆盖。
- native/request 的主攻击仍按原生距离、功率、阵营相关安全模式和 rampart 选目标；自定义伤害主攻击仍沿[单体](#单体伤害)/[群体](#群体伤害)边界。仅实际进入伤害应用的目标触发反击，原生未命中不触发。零主伤害仍触发配置的反击，与原生零功率伤害入口的事件口径一致。

JS 决定显式反击量，不追加原生“仅近战/ATTACK 功率/攻击者 rampart/安全模式抑制”的反击规则。作者可禁用反击、让结构产生荆棘，或让远程/群体伤害目标反击。攻击被 rampart 改向时，配置被保护 creep 的表项不匹配；要让 rampart 反击须配置 rampart 的实际 ID。结构被本次攻击击毁，仍可按击毁前的命中记录反击。

### 反击：结算与恢复

执行前以执行者此前待伤害量加整表反击量（含未命中的预留）检查 i64 累计表示域；自定义自伤另外计入主伤害。批量在任何结构扣血/增删前完成该检查。表项上限之和在 i64 表示域内，不允许累计溢出后截断。

共用伤害应用器登记执行者的延迟伤害、ACTION_ATTACKED 和 `EVENT_ATTACK`/`EVENT_ATTACK_TYPE_HIT_BACK`，事件来源为实际受击对象、目标为执行者；正反击量才产生反击事件，0 反击不产生。表项 amount 和事件 damage 均为减伤前量，执行者在对象 tick 按[累计伤害计算 TOUGH](creep-incoming-damage.md)。事件维持结构摧毁 → 反击 → 主攻击顺序，群体按主批量输入/原生遍历顺序逐目标记录。creep 身体恢复、死亡、墓碑及库存仍在既有对象 tick 与治疗合并后处理。

本动作的执行者已经在[战斗局部快照](creep-combat-slot.md#局部恢复边界)中；目标仍由原生目标集或 JS 首触日志保存。声明表不修改未命中对象，所以无额外世界写面。正常拒绝撤销本动作主伤害、反击、事件与结构变换，保留此前伤害/治疗、转账和后续移动；晚期写回失败按缓存玩家结果重试，死亡、Memory 与 namespace state 恰一次提交。没有新增持久化状态域。

### 反击：验收与边界

[独立 JS 场景](../../compat/fixtures/sim-mod-retaliation-policy/README.md)使用固定反击与按未强化 ATTACK 基础功率 20% 计算的荆棘包，两者通过 request 保留原生主攻击；无 Mod 为对照。双通道完整 final（排除 CPU 桶）和 PlanHash 一致，反击后保存并退出进程，恢复与连续运行等价。专项还覆盖 native 中心、改向、JS 单体/群体伤害、结构击毁、反击致死、局部恢复、重入与能力过期。

当前是攻击前预计算表，没有携带实际主伤害量的逐命中 JS 回调。作者若要按自定义攻击的减伤前伤害比例反弹，应在自己的主伤害公式中同时计算两者；独立反击 Mod 无法自动得知其他处理器随后算出的未知伤害量，也不能按对象 tick 后的实际扣血自动反弹。塔、dismantle 等其他伤害源和自有 Mod 对象尚无此反击入口；原生 TOUGH 减伤已交付，[JS 受击公式替换与护盾组合](#受击)已交付；移动仲裁、玩家预检替换与可配置批量预算仍未开放。

构建、专项及兼容性结果见[交付记录](../research/notes/n2c-retaliation-policy-2026-10-06.md)。

## 受击

2026-10-06 已开放 `builtin:creep.receiveDamage` 对象 tick 槽。独立 JS 包可替换默认 TOUGH 公式，或组合护甲、护盾和免疫。攻击动作仍登记减伤前伤害；此槽选择治疗合并前的最终扣血量，不产生玩家意图。

### 受击：输入与结算

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

### 受击：独立包组合

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

### 受击：拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条受击链：丢弃本次数值与所有暂存 namespace state，回退到入链前的默认 TOUGH 数值。此前成功的攻击/治疗/事件和目标移动仍保留；拒绝不取消伤害。免疫应成功调用 `setDamage(0)`。
- 数值与 state 候选不直接改写世界。受击槽只开放 incoming 数值和 combat 查询；combat request/cancel/伤害/治疗/反击表、资源转账、移动请求、对象写入均不开放，target.fields 不提供 hits/body 写能力。
- 能力只在所属处理器同步期有效，Promise/thenable、重复 next、非法数值、越界能力及本次调用中的过期能力故障不能被 catch + decline 掩盖。错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。
- 写回失败从已提交身体与 namespace state 重新结算，复用玩家缓存，不重复执行玩家代码或消耗护盾。停用出错的 replace 后回退 native，健康钩子继续组合。检查点只保存已提交身体、Mod state 与计划/停用身份，冷启动重新生成受击数值；没有持久化待受击候选。

内建槽现为 14 个，新增冻结描述和 native 绑定会改变 PlanHash；旧计划身份的检查点不会被当作同一玩法静默加载。此槽的非默认贡献经整槽宿主接线，目前没有对应的纯 Rust 自定义受击处理器入口。

### 受击：验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-incoming-policy/README.md)提供原生、独立护甲、独立护盾和两包组合；JSON/KKPV 完整 final 一致，受击后保存退出与冷启动续跑等价。专项覆盖五位置拒绝、非法原始 op、能力过期、来源截断、不同伤害来源、停用回退和晚期失败缓存重试；结果见[交付记录](../research/notes/n2c-incoming-policy-2026-10-06.md)。

当前不开放结构受击、自有对象伤害 trait、按实际净伤害自动反击或 power 免疫语义。移动竞争排序已另行开放（见[JS 移动竞争](movement.md#竞争排序)），移动准入已开放（见[JS 移动准入](movement.md#准入与预演)），完整仲裁、玩家预检策略替换与可配置批量预算仍待实现。来源提取在 Hosted 路径扫描房间事件流，密集受击场景性能尚未单独采集。
