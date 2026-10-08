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

近战/远程/塔伤害全部收敛于单一类型门 `object_hits > 0`（意图层与伤害应用层双处），无变体白名单；耐久对象走**结构语义**（立即扣减，不经受击槽/TOUGH/延迟队列——那是 creep 语义，属 G09.c/d）：

| 链 | 位置 | 语义 |
|---|---|---|
| 近战/远程目标门 | `settle/creep/combat.rs` | `object_hits>0` 放行 Mod；`object_owner` 返 None 使 safeMode 主人保护不适用 |
| 伤害应用 | `settle/damage.rs` | 结构分支立即扣减（`set_structure_hits` 写信封）；反击只对 creep 目标计算 |
| 死亡 | `settle/damage.rs` | 扣至 0 → `remove_obj`（索引/扩展字段/队列一次清理）+ 事件 `{"type": 类型键}`；**不产废墟**（废墟绑定建筑成本模型） |
| 塔攻击 | `settle/structures.rs` | 同一 `object_hits` 门，自动接纳 |
| 战斗读面/策略目标 | `damage_policy.rs` | `structure_hits_max` 的 Mod 臂读 `hitsMax` 信封；`validate_damage_target` 因此接纳；CombatView 类型标识为类型键 |
| 玩家预检 | prelude `creep.js`/`structure-api.js` | attack/rangedAttack/塔 attack 共用 `gw.modDurabilityTargetOk` 目标族判别：玩家类绑定 + `hits` 信封在场（与结算 `object_hits>0` 门同源），无信封类型 -7 |
| 投影 | `mod_record_json` | `hits/hitsMax` 直供玩家记录（与结构 hits 同公开口径）；玩家类经 `api.view.field(this, 'hits')` 读 |
| 对象 tick | `ctx.target.fields` | payload 含信封键，Mod 执行期可读耐久 |

`hits`/`hitsMax` 是投影保留键：字段声明占用在冻结层拒绝。字段更新（`ctx.objects.update`）拒绝写信封键；字段写回（`ctx.target.fields`）传入的信封值一律丢弃并回填世界当前信封。逐 tick 世界校验三臂：声明耐久却缺失/不完整信封、信封非法（`hitsMax` 必须等于冻结声明、`hits` 在 0..=`hitsMax` 内——写一次上限语义）、未声明耐久却携带信封，都拒绝结算。

## 声明式反击（G09.d）

`hitsMax` 之外可再声明 `retaliationRatio`（(0,1] 浮点，JS 面作者形态；冻结为毫精度 1..=1000，`retaliation_milli` 无运行时字段，不占信封/保留键）。语义对齐官方 powerBank 的 damage×0.5：**任意攻击类型**触发（dismantle 经双重排除到不了 Mod 目标，见已知角落）、不耗自身耐久、毫精度整除**向下取整**（不足 1 不触发）；房主安全模式抑制与上游 `_damage.js` 的 powerBank 分支同款（controller 主人==攻击者主人且 safemode 激活时清零）；与显式反击策略的关系同原生反击——策略存在即整体替换（空表可禁用反击）；要求耐久声明（无耐久类型无处触发，冻结层拒绝）。

反击落点按攻击者类型分流（`apply_damage_inner` 的 Mod 受击分支，意图阶段——早于对象 tick，不踩「来源已 tick 丢反击」坑）：creep 攻击者进 `damage_to_apply` 延迟队列（同轮 tick 消费、按部件扣血，置 ATTACKED 动作标记——攻击部件耗尽后输出归零，单挑决斗会结构性停滞，攻击者死亡须经由其他伤害来源）；塔等结构攻击者的队列无人消费（`apply_pending_hits_selected` 非 creep 早退、塔无 tick 分支），**立即扣减**且不经本函数重入（禁递归：反击不再触发被反击方的反击），击毁走结构销毁。事件复用 HIT_BACK（objectId=受击对象、`targetId`=攻击者），事件序保持摧毁→反击→主攻击。

```js
api.objects.define('mymod:spike', {
  version: 1, playerClass: 'Spike', positionFields: ['x', 'y'],
  hitsMax: 300, retaliationRatio: 0.5,
  fields: { x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 } },
});
```

首片不做：按实际净伤害反击（依赖 G09.c 受击槽的修改后伤害值）、逐 attackType 差异比率、反击上限/触发次数声明、反击事件进 mod_states 通知面。

## 边界

首片不做：受击槽的结构形状（`builtin:creep.receiveDamage` 输入是 creep 专属数值形态，G09.c）、按净伤害/逐类型的动态反击（见上节）、repair/heal 覆盖与 Mod 自愈 API（维修绑定建筑成本模型，后续与 owner/工地联动另拆）、rangedMassAttack 覆盖（目标收集走 owner/powerBank 判别，`object_owner` 对 Mod 恒 None）、safeMode/rampart 主人保护（同因 owner 语义未接线）、废墟/掉落产物（store 信封内容随对象消失）、mod_states 死亡通知（namespace state 按命名空间键控，引擎无逐对象清理；事件是唯一通知形态）。攻击者仍是 creep/塔（Mod 对象不发攻击意图）。耐久随 `ModObj.data` 以不透明 JSON 持久化——检查点、KKPV 通道与冷恢复无需版本变化；旧计划（无 hits 声明）不受影响（`hits_max` 序列化省略 None，计划身份不变）。

已知角落（首片口径）：位置型 rampart 改向在意图层按目标格判定，耐久对象与其同格时与结构同款处理；`object_owner` 对 Mod 返 None 意味着 massAttack 永不命中耐久对象（登记为后续与 owner_field 打通的候选）；dismantle 对耐久对象双重排除（玩家面 `instanceof Structure` 门 + 结算面 `cost_bearing` 为假）；战斗策略槽可把耐久对象列为伤害目标（`validate_damage_target` 接纳），但策略治疗仍 creep-only；Mod 玩家类若自选继承 `Structure` 且类型未声明耐久，attack 预检的 `instanceof Structure` 分支会放行而结算静默不动（与无耐久结构同形，作者自选继承的角落）。塔玩家面与 creep attack 共用同一目标族判别（结算侧测试覆盖；玩家路径与 attack/rangedAttack 共享同一助手）。

夹具 [mod-objhits](../../compat/fixtures/mod-objhits/README.md)（siege barrel 300 / vault cache 200×100 双信封两独立包 + 无耐久 decoy 对照）与场景 [sim-mod-objhits](../../compat/fixtures/sim-mod-objhits/README.md)（四轮探针：预检返回码、逐击扣减、-7 对照、双信封互不扰动、组合共存、双通道与冷恢复）；反击夹具 [mod-objret](../../compat/fixtures/mod-objret/README.md)（thorn spike 0.5 反击 / reactor core 1.0 全额+容量 100 两独立包）与场景 [sim-mod-objret](../../compat/fixtures/sim-mod-objret/README.md)（攻击者同轮扣血、资源动作互不扰动）；Rust 侧断言见 `crates/kk-cli/tests/sim_tests/obj_hits.rs`、`obj_ret.rs`、结算域 `settle_tests/mod_hits.rs` 与校验/冻结/类型生成/宿主注册单元测试。
