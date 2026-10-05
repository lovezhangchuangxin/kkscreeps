# JS 反击策略（N2c 第四片）

攻击链处理器可以在执行前调用 `ctx.combat.setRetaliation([{id,amount},...])`，声明本动作的反击表，再保留 native 中心或使用 request/JS 伤害能力。表按实际受击对象匹配；反击由目标指向原执行者，直接登记一次伤害，不递归执行另一条攻击链。

```js
export default function register(api) {
  api.handlers.before('builtin:creep.attack', { id: 'counter' }, (ctx) => {
    // 仅为当前原请求目标配置固定反击；改向或 rampart 命中另按实际 ID 匹配。
    ctx.combat.setRetaliation([{ id: ctx.args.id, amount: 7 }]);
  });
}
```

## 声明与组合

- 仅 attack/rangedAttack/rangedMassAttack 三槽开放。必须在动作 consumed 前声明一次；声明本身不消耗攻击，不登记伤害或事件。其他槽、重复声明、cancel/native/request/JS 伤害之后声明均为故障，catch + decline 不能发布本轮。
- 存在表就替换本动作全部默认反击，未列出的实际受击目标反击量为 0；空表可禁用反击。未声明时，native 维持现有近战反击，JS applyDamage/applyDamageBatch 维持无隐式反击。
- 表最多 256 项；id 为原始字符串，amount 为 0..=1e9 整数。目标须为本房存活、已出生 creep 或有耐久结构，按实际 ID 排除重复。SDK/op/引擎完整验证后才保存表，转换期间允许 getter 重入但不持候选/OpState 借用，完成后重查能力与故障。
- 表是当前调用内的不可变值，不进入世界或检查点，不跨对象、槽或 tick。多个 Mod 可以分别提供反击表与主攻击公式；只有一个处理器负责声明表，其他处理器保留该表并执行/继续链。多个声明者会报故障，不采用静默覆盖。
- native/request 的主攻击仍按原生距离、功率、阵营相关安全模式和 rampart 选目标；自定义伤害主攻击仍沿[单体](mod-damage-policy.md)/[群体](mod-mass-policy.md)边界。仅实际进入伤害应用的目标触发反击，原生未命中不触发。零主伤害仍触发配置的反击，与原生零功率伤害入口的事件口径一致。

JS 决定显式反击量，不追加原生“仅近战/ATTACK 功率/攻击者 rampart/安全模式抑制”的反击规则。作者可禁用反击、让结构产生荆棘，或让远程/群体伤害目标反击。攻击被 rampart 改向时，配置被保护 creep 的表项不匹配；要让 rampart 反击须配置 rampart 的实际 ID。结构被本次攻击击毁，仍可按击毁前的命中记录反击。

## 结算与恢复

执行前以执行者此前待伤害量加整表反击量（含未命中的预留）检查 i64 累计表示域；自定义自伤另外计入主伤害。批量在任何结构扣血/增删前完成该检查。表项上限之和在 i64 表示域内，不允许累计溢出后截断。

共用伤害应用器登记执行者的延迟伤害、ACTION_ATTACKED 和 `EVENT_ATTACK`/`EVENT_ATTACK_TYPE_HIT_BACK`，事件来源为实际受击对象、目标为执行者；正反击量才产生反击事件，0 反击不产生。表项 amount 和事件 damage 均为减伤前量，执行者在对象 tick 按[累计伤害计算 TOUGH](creep-incoming-damage.md)。事件维持结构摧毁 → 反击 → 主攻击顺序，群体按主批量输入/原生遍历顺序逐目标记录。creep 身体恢复、死亡、墓碑及库存仍在既有对象 tick 与治疗合并后处理。

本动作的执行者已经在[战斗局部快照](creep-combat-slot.md#局部恢复边界)中；目标仍由原生目标集或 JS 首触日志保存。声明表不修改未命中对象，所以无额外世界写面。正常拒绝撤销本动作主伤害、反击、事件与结构变换，保留此前伤害/治疗、转账和后续移动；晚期写回失败按缓存玩家结果重试，死亡、Memory 与 namespace state 恰一次提交。没有新增持久化状态域。

## 验收与边界

[独立 JS 场景](../../compat/fixtures/sim-mod-retaliation-policy/README.md)使用固定反击与按未强化 ATTACK 基础功率 20% 计算的荆棘包，两者通过 request 保留原生主攻击；无 Mod 为对照。双通道完整 final（排除 CPU 桶）和 PlanHash 一致，反击后保存并退出进程，恢复与连续运行等价。专项还覆盖 native 中心、改向、JS 单体/群体伤害、结构击毁、反击致死、局部恢复、重入与能力过期。

当前是攻击前预计算表，没有携带实际主伤害量的逐命中 JS 回调。作者若要按自定义攻击的减伤前伤害比例反弹，应在自己的主伤害公式中同时计算两者；独立反击 Mod 无法自动得知其他处理器随后算出的未知伤害量，也不能按对象 tick 后的实际扣血自动反弹。塔、dismantle 等其他伤害源和自有 Mod 对象尚无此反击入口；原生 TOUGH 减伤已交付，[JS 受击公式替换与护盾组合](mod-incoming-policy.md)已交付；移动仲裁、玩家预检替换与可配置批量预算仍未开放。

构建、专项及兼容性结果见[交付记录](../research/notes/n2c-retaliation-policy-2026-10-06.md)。
