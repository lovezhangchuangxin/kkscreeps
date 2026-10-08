# 自有对象耐久

`api.objects.define` 的 `hitsMax` 声明让自有对象类型可被攻击：对象创建（结算期 `ctx.objects.create` 与场景/管理放置）即满血写入 `hits`/`hitsMax` 信封键（与建筑完工同形），耐久只能经伤害动作变动，扣至 0 即移除并发 `objectDestroyed` 事件（`type` 为类型键）。耐久随类型冻结进计划身份；界值与建筑 `hits_max` 同口径（1..=u32::MAX）。可与 `capacity` 同时声明——两族信封键不相交，伤害动作不动库存、资源动作不动耐久。

```js
export default function register(api) {
  api.objects.define('mymod:barrel', {
    version: 1, playerClass: 'Barrel', positionFields: ['x', 'y'],
    hitsMax: 300,
    fields: { x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 } },
    tick(ctx) { ctx.state = { hits: ctx.target.fields.hits }; },
  });
}
```

## 消费链

近战/远程/塔伤害全部收敛于单一类型门 `object_hits > 0`（意图层与伤害应用层双处），无变体白名单；耐久对象走**结构语义**（不经过 creep 的 TOUGH/治疗合并）；结构受击槽未托管时立即扣减，托管时按下节延迟排空：

| 链 | 位置 | 语义 |
|---|---|---|
| 近战/远程目标门 | `settle/creep/combat.rs` | `object_hits>0` 放行 Mod；`object_owner` 返 None 使 safeMode 主人保护不适用 |
| 伤害应用 | `settle/damage.rs` | Mod 分支共用扣减与声明反击效果（`set_structure_hits` 写信封）；托管受击经槽修改伤害后扣减 |
| 死亡 | `settle/damage.rs` | 扣至 0 → `remove_obj`（索引/扩展字段/队列一次清理）+ 事件 `{"type": 类型键}`；**不产废墟**（废墟绑定建筑成本模型） |
| 塔攻击 | `settle/structures.rs` | 同一 `object_hits` 门，自动接纳 |
| 战斗读面/策略目标 | `damage_policy.rs` | `structure_hits_max` 的 Mod 臂读 `hitsMax` 信封；`validate_damage_target` 因此接纳；CombatView 类型标识为类型键 |
| 玩家预检 | prelude `creep.js`/`structure-api.js` | attack/rangedAttack/塔 attack 共用 `gw.modDurabilityTargetOk` 目标族判别：玩家类绑定 + `hits` 信封在场（与结算 `object_hits>0` 门同源），无信封类型 -7 |
| 投影 | `mod_record_json` | `hits/hitsMax` 直供玩家记录（与结构 hits 同公开口径）；玩家类经 `api.view.field(this, 'hits')` 读 |
| 对象 tick | `ctx.target.fields` | payload 含信封键，Mod 执行期可读耐久 |

`hits`/`hitsMax` 是投影保留键：字段声明占用在冻结层拒绝。字段更新（`ctx.objects.update`）拒绝写信封键；字段写回（`ctx.target.fields`）传入的信封值一律丢弃并回填世界当前信封。逐 tick 世界校验三臂：声明耐久却缺失/不完整信封、信封非法（`hitsMax` 必须等于冻结声明、存活对象的 `hits` 在 1..=`hitsMax` 内——写一次上限语义）、未声明耐久却携带信封，都拒绝结算。

## 声明式反击（G09.d）

`hitsMax` 之外可再声明 `retaliationRatio`（(0,1] 浮点，JS 面作者形态；冻结为毫精度 1..=1000，`retaliation_milli` 无运行时字段，不占信封/保留键）。语义对齐官方 powerBank 的 damage×0.5：**任意攻击类型**触发（dismantle 经双重排除到不了 Mod 目标，见已知角落）、不耗自身耐久、毫精度整除**向下取整**（不足 1 不触发）；房主安全模式抑制在攻击时刻确定并随挂起条目保留，与上游 `_damage.js` 的 powerBank 分支同款（controller 主人==攻击者主人且 safemode 激活时清零）；与显式反击策略的关系同原生反击——策略存在即整体替换（空表可禁用反击）；要求耐久声明（无耐久类型无处触发，冻结层拒绝）。

反击落点按攻击者类型分流（共用的 `mod_damage_effects`；原生在意图阶段，托管在排空阶段，均早于对象 tick）：creep 攻击者进 `damage_to_apply` 延迟队列（同轮 tick 消费、按部件扣血，置 ATTACKED 动作标记——攻击部件耗尽后输出归零，单挑决斗会结构性停滞，攻击者死亡须经由其他伤害来源）；塔等结构攻击者的队列无人消费（`apply_pending_hits_selected` 非 creep 早退、塔无 tick 分支），**立即扣减**且不经本函数重入（禁递归：反击不再触发被反击方的反击），击毁走结构销毁。事件复用 HIT_BACK（objectId=受击对象、`targetId`=攻击者），事件序保持摧毁→反击→主攻击。

```js
api.objects.define('mymod:spike', {
  version: 1, playerClass: 'Spike', positionFields: ['x', 'y'],
  hitsMax: 300, retaliationRatio: 0.5,
  fields: { x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 } },
});
```

首片不做：按实际净伤害反击（依赖 G09.c 受击槽的修改后伤害值）、逐 attackType 差异比率、反击上限/触发次数声明、反击事件进 mod_states 通知面。

## 结构受击槽（G09.c）

`builtin:mod.receiveDamage` 让 JS 包在**结构扣减前**修改落入耐久对象的伤害量。声明面与 creep 受击槽同形（`api.handlers.replace/wrap/before/after('builtin:mod.receiveDamage', {id}, fn)`），无需新 JS API；数值面复用 `ctx.incoming`（getDamage/setDamage，JS 安全整数界）。输入是当次立即值：`ctx.args.damage`（原始伤害）与 `ctx.args.source`（当次攻击者摘要 `{sourceId, amount, attackType}`，无 creep 槽的累计队列语义；来源摘要保留攻击时刻 ID，即使攻击者随后消失仍可读；`ctx.combat.get(sourceId)` 此时返回 null）；**目标形状不进输入**，经只读 `ctx.target.fields` 的 payload（含耐久/库存信封）承载，提交字段改写会导致槽故障并撤销整轮候选；正常拒绝直接丢弃尚未提交的本地字段副本。输出即 `setDamage` 的修改值，可多次调整组合；正常拒绝回退默认伤害。

**托管延迟排空**是本槽与 creep 槽的关键架构差：伤害入口（`apply_damage_inner`）只持有 `&mut Working`，而 JS 链需要 `RoomCtx` 共享句柄。因此 Native 派发（默认引擎，槽未被覆写）在伤害入口**零开销内联**（行为与 G09.b 完全一致，既有测试零扰动）；Hosted 派发（任一包覆写本槽即全局生效）把伤害挂 `mod_damage_pending` 队列，房间循环在**意图阶段后、对象 tick 前**统一排空——此点持有共享句柄，逐项跑槽得修改后伤害再落地。扣减与声明式反击效果提取为 `mod_damage_effects` 共用（内联路径与排空路径同一段代码）。

**事件序偏差**（同 tick 内、确定性，与 creep 延迟路径同形）：Native 路径保持「摧毁→声明反击→主攻击」；Hosted 路径的主攻击事件与攻击者动作标记仍在意图时刻发出（挂起不提前返回），但扣减/摧毁/类型键反击事件移到排空时刻——即主攻击事件先于摧毁事件。显式反击策略仍在意图时刻经共用尾部落地。排空时目标已被更早挂起项击毁的条目整项丢弃（槽不跑——宿主读取面以目标缺失为故障，集火同 tick 击穿时跑槽会拖垮候选；跳过同时避免观察者 state 与实际落地背离）。攻击者已消失不撤销已经执行的主伤害，声明反击只对仍在场的攻击者落地，也不产生指向失效攻击者的反击事件。动作链正常拒绝会恢复挂起队列长度，既有成功条目与后续动作保持。到达槽的调用来源摘要恒非空；输入的 `source` 可空形态是对未来调用点的防御性契约。槽故障使本房间候选整体失败（Err 丢弃候选，安全回滚）。同一计划内任何包覆写本槽即对所有耐久对象生效（见 combined 场景：减伤中心对两独立包的对象同时生效）。

```js
api.handlers.replace('builtin:mod.receiveDamage', { id: 'halve' }, ctx => {
  ctx.incoming.setDamage(Math.ceil(ctx.args.damage / 2)); // 30 → 15
});
```

全链：槽注册与 revision（`kk-mod-api/src/builtin.rs`，槽目录 21 项）、输入结构（`kk-mod-api/src/incoming.rs`）、候选（`settle/incoming.rs` `ModIncomingDamage`）、槽执行（`settle/contrib/incoming.rs` `ModIncomingSlot`）、挂起与效果（`settle/damage.rs`）、排空点（`settle/room_loop.rs`）、能力路由（`kk-runtime` capability/call/CombatHost 双变体；combat 读面与正常拒绝能力随 SlotCapability::Incoming 放行）。测试：结算域 `settle/damage/tests.rs`（挂起/排空/拒绝/内联四径）、宿主链 `mod_chain_tests/mod_guard.rs`（减伤/拒绝/免疫/组合/击毁双通道）、sim `sim_tests/obj_guard.rs`。夹具 [mod-objguard](../../compat/fixtures/mod-objguard/README.md)（warden 减伤型 / logbook 记录型两独立包）与场景 [sim-mod-objguard](../../compat/fixtures/sim-mod-objguard/README.md)（三场景四轮探针、组合链尾读值、双通道与冷恢复）。

## 边界

首片不做：受击槽按净伤害联动声明式反击（上节 G09.d 的动态反击候选，槽修改值已可经 G09.c 读到但反击比率仍按原始伤害计算）、repair/heal 覆盖与 Mod 自愈 API（维修绑定建筑成本模型，后续与 owner/工地联动另拆）、rangedMassAttack 覆盖（目标收集走 owner/powerBank 判别，`object_owner` 对 Mod 恒 None）、safeMode/rampart 主人保护（同因 owner 语义未接线）、废墟/掉落产物（store 信封内容随对象消失）、mod_states 死亡通知（namespace state 按命名空间键控，引擎无逐对象清理；事件是唯一通知形态）。攻击者仍是 creep/塔（Mod 对象不发攻击意图）。耐久随 `ModObj.data` 以不透明 JSON 持久化——检查点、KKPV 通道与冷恢复无需版本变化；旧计划（无 hits 声明）不受影响（`hits_max` 序列化省略 None，计划身份不变）。

测试排空助手与房间结算共用 `damage::drain_mod_damage`，逐条执行槽并落地，避免生产路径与测试重复实现。

首片口径：位置型 rampart 改向在意图层按目标格判定，耐久对象与其同格时与结构同款处理；`object_owner` 对 Mod 返 None 意味着 massAttack 永不命中耐久对象（登记为后续与 owner_field 打通的候选）；dismantle 对耐久对象双重排除（玩家面 `instanceof Structure` 门 + 结算面 `cost_bearing` 为假）；战斗策略槽可把耐久对象列为伤害目标（`validate_damage_target` 接纳），但策略治疗仍 creep-only；Mod 玩家类若自选继承 `Structure` 且类型未声明耐久，attack 预检的 `instanceof Structure` 分支会放行而结算静默不动（与无耐久结构同形，作者自选继承的角落）。塔玩家面与 creep attack 共用同一目标族判别（结算侧测试覆盖；玩家路径与 attack/rangedAttack 共享同一助手）。

夹具 [mod-objhits](../../compat/fixtures/mod-objhits/README.md)（siege barrel 300 / vault cache 200×100 双信封两独立包 + 无耐久 decoy 对照）与场景 [sim-mod-objhits](../../compat/fixtures/sim-mod-objhits/README.md)（四轮探针：预检返回码、逐击扣减、-7 对照、双信封互不扰动、组合共存、双通道与冷恢复）；反击夹具 [mod-objret](../../compat/fixtures/mod-objret/README.md)（thorn spike 0.5 反击 / reactor core 1.0 全额+容量 100 两独立包）与场景 [sim-mod-objret](../../compat/fixtures/sim-mod-objret/README.md)（攻击者同轮扣血、资源动作互不扰动）；受击槽夹具见上节。Rust 侧断言见 `crates/kk-cli/tests/sim_tests/obj_hits.rs`、`obj_ret.rs`、`obj_guard.rs`、结算域 `settle_tests/mod_hits.rs` 与校验/冻结/类型生成/宿主注册单元测试。
