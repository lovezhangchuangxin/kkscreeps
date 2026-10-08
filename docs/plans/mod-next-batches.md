# Mod 后续开发任务与接手记录

本文件是**持续更新的执行台账**，用于 AI 接手、上下文恢复和每批进度交接。它细化[完整玩法计划](mod-complete-gameplay.md)，不替代 N1–N6 的范围、ADR 或现行架构。G01–G30 只是本台账的稳定任务编号；尚未完成的条目是目标，不代表接口已经存在。

长期目标：开发者通过独立 JS Mod 轻松创造各种完整玩法，覆盖调研版本 xxscreeps 的扩展领域，并提供更完善的组合、恢复和开发工具。项目自身逐步以公共 SDK 提供可选官方内容；第二种同类玩法不应再要求修改 Rust 或重编宿主。

## 当前接手位置

- 当前任务：**G11.a 建筑 RCL 配额首片已交付（随本次提交）**。132 条 `builtin:structure.quota.{type}.rcl{N}` 逐型逐级规则（键集/默认=官方表）+ `StructureQuotaSnapshot`（装配期解析、Rc 入逐房 Plane，五消费者统一 `allowed_for`）+ 折叠注入玩家 `CONTROLLER_STRUCTURES`（canary/worker 双点，switch/冷恢复同路径）；link.rcl0-8 迁入族修复「link 规则不达玩家面」既有缺口；builtin revision v18（旧检查点身份门拒绝）。契约见[配额规则](../architecture/structure-quota.md)，夹具 [mod-quota](../../compat/fixtures/mod-quota/README.md)/[sim-mod-quota](../../compat/fixtures/sim-mod-quota/README.md)，Rust 侧 `sim_tests/quota.rs`。上一任务 M2D ID 构造器批已交付，含 M2C 回归修复与 probe 诊断豁免，细节见实施记录。
- 已完成基线：S01 `e0b457b`、S02 `5a25bc2`。旧 S03–S06 的纯搬迁限制由本轮职责治理方案替代，详见[维护结构方案](archive/structure-governance-2026-10.md)。
- 已完成实现：移动领域跨层归拢；Working 统一构造与瞬态字段收口、engine 首触日志；世界容器/投影与 runtime 执行器分离、Generic 共享读取；串行场景名称/耗时/子集反馈；文档契约合并、旧计划归档与工具门禁。保留玩法语义和恢复身份，不推进 G02.d3 等新玩法。
- 本轮实现：kk-mod-api 配额族+快照（builtin.rs）、engine Plane/Engine 接线与五消费者收口、runtime 双点常量注入（setup.rs/switch.rs 注释）、contrib_link/link_decline 键迁移、冻结与结算测试、sim 三轮探针场景与两独立夹具包、架构文档 structure-quota.md 及三处索引。无 SDK 协议变化；新增写入域无（规则值经既有冻结/恢复链路持久）。
- 当前验证：无参数 `pnpm run check` 完整通过；release 重建后棘轮 **1983 持平**（0 转红 0 转绿），ratchet 地板 1983；三组 bot 2000/2000/10 指标与基线一致且验收判定全部通过（Overmind 保留既有首轮异常 1）。三路复审 0 P0/P1；P2（Plane 逐房深拷贝→Rc）当批清偿，P3 清偿或登记（折叠键序 preserve_order 依赖注释、缺族拒绝补测、全表折叠相等断言、link 玩家面间接覆盖、RCL0 键不声明边界、quota 恢复调用并发参数对齐）。**间歇登记**：sim_tests 全套并行下三例冷恢复子进程偶发退出码 1（非本批引入，隔离全绿；再现须存子进程报告归因）。剩余 24 失败集中于 keeper lair/getGameTime/setup 零散域。工作区含本次未提交改动。
- 历史交接、阶段验证与审查过程见[历史记录](archive/mod-history-2026-10.md)；玩法缺口继续以本文件任务表为准。

## G11.a 建筑配额策略（2026-10-08 登记；同日选型拍板并实施交付）

调查结论（登记时点，改造前口径）：配额的引擎判定收口在 `settle/structures.rs::allowed_for`（link 走 `w.plane.links.rcl` 有效规则——`builtin:link.rcl0..8` 九条标量规则先例，其余类型查 `controller_allowed` 静态表）；引擎消费者五处（工地创建结算 room.rs、link 转移 isActive 门 structures.rs、spawn/extension 放置 mark_off、terminal 容量 economy、extractor 采集门 harvest）；玩家面两个消费者读冻结常量（`structures.js` isActive 与 `room/creation.js` checkControllerAvailability，后者在意图记录前返回 ERR_RCL_NOT_ENOUGH）。

**选型（路线①+③ 合体）**：计划级逐型逐级标量规则 `builtin:structure.quota.{type}.rcl{N}`（132 条，type 为官方 CONTROLLER_STRUCTURES 全 16 型、N 按官方键集——spawn/extension/road/container 0-8 其余 1-8，默认=官方表值）+ 折叠注入 `CONTROLLER_STRUCTURES` 常量表（引擎 Plane 快照与玩家常量同计划派生，G02.d2/G07.a 双端同源同款；kk-constants 先于 prelude 段执行、表值可注入——RESOURCES_ALL 先例）。**房槽路线（②）的硬事实**：玩家预检在意图记录前拒绝超配额（错误码被兼容用例钉死），结算期槽收不到超配额意图 → 无法升配额，而升配额（TD 多塔/迷宫限墙）是主用例；单向降配额半值。**顺带修复既有缺口**：`builtin:link.rcl*` 至今只进引擎不进玩家面（link 的 isActive/createConstructionSite 预检读静态表）——link.rcl0-8 迁移并入本族（键集改官方形 1-8），LinkRules 去 rcl、allowed_for 统一走配额快照。约束：Mod 建筑不占内建配额表（既有口径不变）；builtin revision v17→v18，PlanHash 变化旧检查点按身份门拒绝（G02.d3 同款）。验收按每批共同验收（两独立 JS 包+原生对照+组合+双通道+冷恢复）+ 领域项：rcl8 超官方塔数无 mod 拒/配额包放行、降墙配额生效、玩家预检错误码与结算同源、link 迁移回归。

**实施交付（同日）**：契约见[建筑 RCL 配额规则](../architecture/structure-quota.md)。`StructureQuotaSnapshot` 装配期一次解析、Rc 共享入逐房 Plane；`fold_controller_structures` 折叠表经 setup.rs canary 预检与玩家 worker 双点注入（switch/冷恢复同一路径）；五消费者统一 `allowed_for`（快照 miss 回退官方静态表，仅默认引擎/非官方类型可达）。夹具 [mod-quota](../../compat/fixtures/mod-quota/README.md)（quota-raise 塔 20 / quota-lower 墙 3 两独立包）与 [sim-mod-quota](../../compat/fixtures/sim-mod-quota/README.md)（三轮探针：首 tick 结算收口、次 tick 预检读「已提交数 vs 注入表」、末 tick 回读世界工地数，三面互证）；Rust 侧 `sim_tests/quota.rs` 四场景×双通道+冷恢复+组合身份互异。验证：完整门禁通过；棘轮 1983 持平（0 转红 0 转绿）；三 bot 2000/2000/10 指标+验收判定全通过；三路复审 0 P0/P1，P2（Plane 深拷贝→Rc）当批清偿，P3 清偿或登记（折叠键序依赖 workspace preserve_order 已注释；link 迁移玩家面覆盖为间接）。已知边界：RCL 0 级别键不声明（官方表无此键）；首片不开放逐房差异/动态配额/Mod 建筑配额。

## G09.a 自有对象库存（2026-10-08 交付）

调查结论（登记时点现状）：`RoomObject::Mod`（`api.objects.define` 类型实例）今天完全不能作为资源端点——六条链全部按变体白名单拒绝：① 结算接收门 `settle/creep/store.rs` intent_transfer/intent_withdraw 只认 Creep/Spawn/Extension/Storage/Generic（withdraw 另有 Tombstone/Ruin），`_ => return` 静默拒 Mod；② Mod 侧资源槽 `settle/resources.rs::inspect/transfer/change` 同名单拒（「对象没有已接线的可用资源仓库」）；③ 玩家预检 `creep-resources.js` transfer/withdraw 要求 structureRegister（`raw.type === 'structure'`）或 creep + `instanceof Structure` 族，Mod 对象投影 `type:"mod"` 两门都不过；④ 对象字段 schema（definition.rs ValueKind：Bool/Number/String/ObjectRef/Enum/Array/Map/Record）无库存形态字段，ObjectTypeDescriptor 无容量槽；⑤ 投影 `mod_record_json` 只合成 type/modType/id/room/x/y/pos 信封键，且 `store` 不在保留键清单（player_surface.rs）——声明的同名字段是惰性数据；⑥ 既有先例是 Mod 建筑（G08.a）：`store_capacity` → 完工写 `store/storeCapacity` → 落 `RoomObject::Generic`，天然过①②③全部 Generic 臂——但建筑是静态放置+工地生命周期，不能替代可动态增删/移动/自定义玩家类的对象。

**选型（路线①：对象级库存声明 + Generic 库存语义复用）**：ObjectTypeDescriptor 新增可选 `store` 声明（首片=单一总容量 u32，形态对齐建筑 `store_capacity`）；引擎把库存作为 ModObj.data 信封键 `store/storeCapacity` 维护（复用 `resources/generic.rs` 的 generic_capacity/change_generic_store，六链按「带合法库存字段的 Generic」同款语义接纳 Mod 对象）；`store/storeCapacity` 入 player_surface 保留键（冻结期拒绝声明同名字段，无存量用户）；投影 mod_record_json 合成两信封键；预检 creep-resources.js 增 `type:"mod"` 目标族（读同一信封键容量）；mod 侧 inspect/change 增 Mod 臂。容量来源=类型级冻结声明（与建筑同口径），不进 StoreRules（那是内建结构规则族）；首片不做逐资源容量/实验室式槽绑定/损耗（mod 可用 ctx.resources 槽组合）。验收=共同验收（两独立包：搬运型/仓储型 + 原生对照 + 组合 + 双通道 + 冷恢复）+ 领域项：creep transfer/withdraw 对 Mod 对象进出双向、预检错误码与结算同源、mod 槽 read/transfer 触达自有对象、保留键冻结拒绝、投影 store 键形状、事件与保存恢复随既有链路。G09.b 耐久/死亡、G09.c 结构受击另拆（本片不碰 hits/伤害）。

**实施交付（同日）**：契约见[自有对象库存](../architecture/object-store.md)。`store_capacity` 为 Option<u64> 且冻结界 ≤2^53-1（登记为 u32，实施放宽以对齐 JS 安全整数；`skip_serializing_if` 使未用 store 的计划 PlanHash 稳定）；三条创建路径（结算 create、embed 放置、既有建筑线）同形注入 `{"energy":0}` 信封。六链接纳：creep 双向结算四臂、mod 资源槽 inspect/change（新增 `change_mod_store` 纯助手，不带实验室槽语义）、`mod_record_json` 投影合成信封键、`creep-resources.js` modEntry 目标族（容量门读同一信封）、对象 tick 读、发布前校验三臂守卫（声明缺信封/信封非法/未声明携带）。保留键 +store/storeCapacity（共 8 项）并移到**冻结层**拒绝（validate::object，builtin:link 豁免——其字段名即官方投影形状；storeCapacityResource 经信封拆分隔离，不占保留键）；字段写回丢弃处理器提供的信封、回填世界当前值（同时防篡改与防陈旧快照回滚）；typegen 为 store 声明类型合成 `store/storeCapacity` 成员。夹具 [mod-objstore](../../compat/fixtures/mod-objstore/README.md)（objstore-hauler/objstore-vault 两独立包）与 [sim-mod-objstore](../../compat/fixtures/sim-mod-objstore/README.md)（hauler/vault/combined 三场景四轮探针，`--ticks 4`）；Rust 侧 `sim_tests/obj_store.rs` 三场景×双通道+冷恢复+plan_hash 互异。验证：完整门禁通过；棘轮 1983 持平（0 转红 0 转绿）；三 bot 2000/2000/10 验收判定全通过；三路复审 1 P1（信封写回边界）+4 P2 当批清偿，P3 已知角落登记于契约文档（structureType:terminal 预检分歧、object_owner 对 Mod 返 None 的 rampart 语义、同 tick 预提交读与单意图覆盖）。首片不做逐资源容量/槽绑定/损耗。

## G09.b 自有对象耐久与死亡（2026-10-08 交付）

调查结论（登记时点现状）：Mod 对象今天完全不能被攻击——近战/远程/塔伤害全部收敛于单一类型门 `damage::object_hits(&target) > 0`（combat.rs 意图层与 apply_damage_inner 双处），**没有变体白名单**，Mod 落 `object_hits` 的 `_ => 0` 臂被静默忽略（无事件无错误）。伤害应用二分：creep 分支走延迟队列（受击槽+TOUGH+治疗合并+墓碑）；结构分支立即扣耐久（`set_structure_hits`）→ 0 即 `destroy_structure`（remove_obj+Ruin 转存）+ `EVENT_OBJECT_DESTROYED {type: structureType}`。周边事实：受击槽 `builtin:creep.receiveDamage` 输入形状 creep 专属（无对象分型，G09.c 界）；玩家预检 attack 门要求 `isKnownStructure`+`instanceof Structure` 族（Mod 投影 `type:"mod"` 得 -7，G09.a 已在资源门开 modEntry 先例）；`hits/hitsMax` 不在保留键清单（键位空闲）；G08.a 建筑耐久=Generic data JSON 键（`hits_max` 冻结声明、完工写 `hits/hitsMax/notifyWhenAttacked`），`object_hits`/`structure_hits_max`/`set_structure_hits` 的 Generic 臂即为它开口；`remove_obj` 一次清索引/扩展字段/疲劳/伤害治疗队列/链日志，投影与 KKPV 每轮派生无增量链，扩展字段不可能键控在 Mod 对象上——死亡移除的伴随清理全部白吃；rangedMassAttack 走 owner/powerBank 判别（`object_owner` 对 Mod 恒 None）；策略伤害目标校验 `validate_damage_target` = creep 或 `structure_hits_max().is_some()`（Mod 得 None 被拒）；战斗读面 `slot_combat_get` 同源排除。

**选型（信封键路线，与 G09.a 库存同构）**：ObjectTypeDescriptor 新增可选 `hits_max`（>0 且 ≤u32::MAX，对齐建筑冻结口径；可与 store_capacity 同时声明）；引擎把耐久作为 ModObj.data 第二族信封键 `hits/hitsMax` 维护（创建路径注入满血 `hits=hitsMax=声明值`，三条创建路径同形）；`hits/hitsMax` 入保留键（冻结层拒绝同名字段，builtin 豁免）与字段更新拒绝；`object_hits`/`structure_hits_max`/`set_structure_hits` 增 Mod 臂读写信封（结构分支立即扣耐久语义，不经受击槽/TOUGH/反击——creep 语义属 G09.c/d）；死亡=扣至 0 → remove_obj + `EVENT_OBJECT_DESTROYED {type: 类型键}`（无废墟无掉落——废墟绑定建筑成本模型，Mod 对象不经工地，另拆；store 信封内容随对象消失）；投影 mod_record_json 合成 hits/hitsMax；预检 creep.js attack/rangedAttack 增 modEntry 目标族（G09.a 资源门同款）；策略伤害目标与战斗读面随 structure_hits_max 的 Mod 臂自动接纳；逐 tick 校验三臂守卫（声明缺信封/信封非法/未声明携带）；typegen 合成两成员；type_name_of/object_owner 的 Mod 臂按逐调用点影响面核验后最小接线（事件标识优先，movement 等消费点不得被顺带改变行为）。首片不做：受击槽结构形状（G09.c）、反击（G09.d）、repair/heal 覆盖与自愈 API（维修绑定建筑成本模型，后续与 owner/工地联动另拆）、rangedMassAttack 覆盖与 safeMode/rampart 主人保护（owner_field 语义未接线，object_owner 维持 None）、废墟/掉落产物、mod_states 死亡通知。rampart 位置改向若为纯位置判定则自动覆盖，实现期核实。验收=共同验收（两独立包：可击毁型+耐久×库存双信封组合型 + 原生对照（无耐久类型预检 -7 与结算静默不动）+ 组合 + 双通道 + 冷恢复）+ 领域项：attack/rangedAttack 逐 tick 扣血、击毁移除+事件 type=类型键、塔攻击命中、字段写回不覆盖信封、保留键冻结拒绝、投影 hits 形状、策略伤害目标接纳、逐 tick 校验拒绝缺/伪信封。

**实施交付（同日）**：契约见[自有对象耐久](../architecture/object-durability.md)。`hits_max`（Option<u64>，冻结界 1..=u32::MAX 与建筑同口径）随 ObjectTypeDescriptor 冻结；两条创建路径（结算 create、embed 放置）同形满血注入 `hits/hitsMax` 信封；`object_hits`/`structure_hits_max`/`set_structure_hits` 增 Mod 臂读写信封（近战/远程/塔/策略伤害全走同一门）；死亡=扣至 0 → remove_obj + `objectDestroyed {type: 类型键}`，不产废墟不掉落；保留键 10 项（+hits/hitsMax）冻结层拒绝；字段更新拒绝信封键、字段写回丢弃输入信封回填世界值（与库存同款）；逐 tick 校验三臂守卫（缺信封/信封非法——**hitsMax 须等于冻结声明、hits 须在 1..=hitsMax 内**——0 只能直改造出，发布前拒绝；未声明携带）。**与登记的偏差**：塔玩家面 attack 也开了 modEntry 目标族（登记只写 creep.js；三处共用 `gw.modDurabilityTargetOk` 助手，塔结算侧测试覆盖、玩家路径与 attack/rangedAttack 共享判别）；`type_name_of` 对 Mod 保持空串（战斗读面 CombatView 单独取类型键，movement 等消费点零影响——登记的"最小接线"落地为不接线）。夹具 [mod-objhits](../../compat/fixtures/mod-objhits/README.md)（objhits-siege barrel 300/objhits-vault cache 200×100 双信封两独立包 + 无耐久 decoy 对照）与 [sim-mod-objhits](../../compat/fixtures/sim-mod-objhits/README.md)（siege 近战 30/击 300→210、vault 远程 10/击 200→170+转入 50 互不扰动、combined 共存，`--ticks 4`）；Rust 侧 `sim_tests/obj_hits.rs` 三场景×双通道+冷恢复+plan_hash 互异、`settle_tests/mod_hits.rs` 近战/远程/塔/无耐久静默/击毁事件+无废墟、`damage_policy.rs` 战斗读面与策略目标接纳、validation/freeze/typegen 单测。首片不做：受击槽结构形状（G09.c）、反击（G09.d）、repair/自愈、massAttack 与 owner 联动、废墟/掉落、死亡通知面。

## G09.c 结构受击槽 与 G09.d 声明式反击（2026-10-08 调查、选型与交付）

调查结论（登记时点现状，两片共用同一份调查）：结构/Mod 的立即扣减路径（apply_damage_inner 结构与 Mod 分支）今天**没有任何 Mod 介入点**——受击槽 `builtin:creep.receiveDamage` 的 `default_pending_damage` 以 creep 为门（非 creep 整槽不调用），IncomingInput 形状是纯数值+来源 ID 摘要（无目标对象形状）；反击两条路（原生=仅 creep 目标+近战+无 rampart 按 ATTACK 部件；显式 RetaliationPolicy=攻击者侧 JS 预声明表）都不覆盖结构/Mod 目标；反击伤害统一落 `damage_to_apply[attacker]` 延迟队列——该队列只在 creep tick 消费，**塔/结构攻击者的反击会静默丢失**（apply_pending_hits_selected 非 creep 早退、塔无 tick 分支）；受击槽在目标自己的对象 tick 运行，ID 更小的攻击者已 tick 完成，此刻向其排反击队列随候选丢弃（父任务警告的机制基础）。官方对照（上游 _damage.js：仓库注释/研究记录取证）：反击仅近战+带 ATTACK 部件的 creep 目标，结构不反击；唯一结构例外 powerBank=任意 attackType 伤害×0.5 反击（特殊硬编码）；结构受击=立即扣减无 TOUGH。

**选型（G09.d 先行：声明式反击首片，官方 powerBank 同构）**：ObjectTypeDescriptor 新增可选反击声明（首片=固定比率 `retaliation_ratio`，值域 0..=1（milli 精度 i64 存储，对齐价格毫单位先例），可与 hits_max 组合（无耐久类型拒绝声明——无处触发））；落点在 apply_damage_inner 的 Mod 受击分支（意图阶段，安全——不踩对象 tick 丢反击坑）：受击耐久对象按声明比率计算反击量（任意 attackType，对齐 powerBank），**按攻击者类型分落点**——creep 攻击者沿用 `damage_to_apply` 延迟队列（现有 HIT_BACK 事件与「摧毁→反击→主攻击」事件序），塔/结构攻击者立即扣减（直接结算，禁递归：反击施加不再触发被反击方的反击）；界值预检沿 RetaliationPolicy::preflight 先例（i64 表示域）。反击量不占用耐久（与 powerBank 同构，无自伤）。首片不做：按实际净伤害反击（依赖 G09.c 的修改后伤害值）、逐 attackType 差异、反击上限声明、mod_states 通知面。

**选型（G09.c 结构受击槽，G09.d 后实施）**：新槽（键名区分 creep 语义，如 `builtin:mod.receiveDamage`），介入点在 apply_damage_inner 的 Mod 分支扣减前（意图阶段）；输入形状=伤害值+来源摘要（复用事件归因）+目标形状（类型键/hits/库存信封摘要，沿 ctx.target mod_fields 先例）；输出=修改伤害量（钳 JS 安全整数）；沿 receiveDamage 槽全链接线（builtin.rs 槽清单+revision、incoming 输入结构、settle/incoming prepare、contrib 三件、install NATIVE 清单、kk-runtime capability/ops、kk-mod-host chain/sdk、夹具）。新增槽键改 PlanHash。首片不做：治疗合并/pendingHeal（结构无延迟队列）、放行型减免、结构（非 Mod）目标覆盖。

验收（两片各自按共同验收形态）：G09.d=两独立包（纯反击型/反击×库存组合型）+无耐久对照+组合+双通道+冷恢复+领域项（creep 攻击者延迟扣血+HIT_BACK 事件、塔攻击者立即扣减、事件序、比率界值、无递归、反击不耗自身耐久）；G09.c=两独立包（减伤型/记录型）+无耐久对照+组合+双通道+冷恢复+领域项（修改后伤害参与判死、界值钳制、来源归因、默认值回退、creep 受击槽零扰动）。

**G09.d 实施交付（同日）**：契约见[自有对象耐久](../architecture/object-durability.md)的声明式反击节。`retaliation_milli`（Option<i64>，冻结层毫精度 1..=1000 + 要求耐久声明；JS 面作者形态 `retaliationRatio` (0,1] 浮点、round 换毫精度、四舍五入后 <1 拒绝）无运行时字段——不占信封/保留键、typegen 无变化。落点在 apply_damage_inner 的 Mod 受击分支（意图阶段）：creep 攻击者走 `damage_to_apply` 延迟队列（同轮 tick 按部件扣血+ATTACKED 标记），塔/结构攻击者立即扣减（禁递归：不重入伤害应用器）；事件复用 HIT_BACK，序=摧毁→反击→主攻击。**与登记的偏差（复审驱动）**：① 值域收紧 (0,1]（登记 0..=1——0 等价不声明，冻结层拒绝）；② 「界值预检沿 preflight 先例」落地为 saturating 乘（damage 界 1e9×1000 无溢出，与原生路径同不加检）；③ 登记时「powerBank 语义无 safeMode 抑制」判断**错误**——上游 `_damage.js:39-41` 实际抑制 powerBank 反击，已改为同款抑制（controller 主人==攻击者主人且激活清零）并补测试；④ 显式反击策略与声明反击的关系明确为**策略存在即整体替换**（combat.md 既有契约「空表可禁用反击」，复审 P1 修复双触发）；⑤ 无耐久对照落地为冻结层拒绝+结算无声明对照（无耐久类型无法声明反击，sim 级对照不可构造）；⑥ 无递归为构造性保证（立即扣减不重入；Mod 不发攻击意图，无可测递归场景）。**行为发现**：反伤沿队列按部件扣血且输出口径是 count_active——攻击部件耗尽后输出归零，creep 攻击者单挑反击型对象会**结构性停滞**（存活但打不动），死亡须经其他伤害来源；已钉停滞测试并入文档。夹具 [mod-objret](../../compat/fixtures/mod-objret/README.md)（thorn spike 0.5 / reactor core 1.0×容量 100 两独立包）与 [sim-mod-objret](../../compat/fixtures/sim-mod-objret/README.md)（三场景四轮探针）；Rust 侧 `settle_tests/mod_hits.rs` 反击七测（近战/远程/塔/塔毁/safeMode 抑制/停滞/多攻击者/极小比率）、`damage_policy.rs` 策略替换测试、freeze/host 注册测试、`sim_tests/obj_ret.rs` 双通道+冷恢复。首片不做：按净伤害反击（依赖 G09.c）、逐 attackType 差异、触发次数/上限声明、反击通知面。

**G09.c 实施交付（同日，G09.d 后）**：契约见[自有对象耐久](../architecture/object-durability.md)的结构受击槽节。新槽 `builtin:mod.receiveDamage`（槽目录 21 项、revision v19、install NATIVE 清单、能力映射 SlotCapability::Incoming——正常拒绝与战斗读面随之放行）。**托管延迟排空**（与登记「介入点在 Mod 分支扣减前」的落地偏差，动机：伤害入口只有 `&mut Working`，JS 链需 RoomCtx 共享句柄）：Native 派发（默认引擎）在伤害入口内联零开销（行为与 G09.b 逐位一致，既有测试零扰动）；Hosted 派发把伤害挂 `Working.mod_damage_pending`，room_loop 意图循环后、移动仲裁前统一排空——逐项跑槽得修改后伤害再落地；扣减与声明反击效果提取为 `mod_damage_effects` 供内联/排空共用。**与登记的偏差**：① 「目标形状进输入」落地为经 `ctx.target`（Mod payload 含耐久/库存信封）承载，ModIncomingInput 只含 damage+source 摘要（当次攻击者，无累计队列语义）；② 事件序偏差（同 tick 内确定性）：Hosted 主攻击事件/动作标记/显式反击仍在意图时刻，扣减/摧毁/类型键反击移到排空时刻——主攻击事件先于摧毁事件（Native 保持摧毁→反击→主攻击）；③ 排空点选在移动仲裁前而非对象 tick 前（同批内更早、语义等价，排空项的 creep 反击队列仍被同轮 tick 消费）；④ 目标被更早挂起项击毁的条目整项丢弃不跑槽（自查发现：宿主读取面以目标缺失为故障，集火同 tick 击穿会把整个 tick 候选拖垮），补集火测试钉住。输入/输出/JS 声明面与登记一致（setDamage 多次可调、JS 安全整数界、正常拒绝回退默认、api.handlers.replace 通用）。kk-runtime：ChainCapability::ModIncoming 变体 + CombatHost incoming 双变体路由 + combat 读面门三变体放行；JS SDK/宿主 ops 零改动。夹具 [mod-objguard](../../compat/fixtures/mod-objguard/README.md)（warden 减伤型 30→15 + 无耐久 decoy -7 对照 / logbook 记录型 After 两独立包）与 [sim-mod-objguard](../../compat/fixtures/sim-mod-objguard/README.md)（三场景四轮探针、combined 减伤中心对两包对象同时生效且观察者读链尾值）；Rust 侧 `settle/damage/tests.rs` 挂起/排空/拒绝/内联四径、`mod_chain_tests/mod_guard.rs` 减伤/输入形状/拒绝/免疫/组合/放大击毁×双通道、`sim_tests/obj_guard.rs` 三场景×双通道+mod_states+plan_hash 互异+冷恢复。首片不做：治疗合并/pendingHeal（结构无延迟队列）、放行型减免、结构（非 Mod）目标覆盖、按修改后伤害联动声明反击（G09.d 候选）。

## 执行与更新规则

以实际 HEAD、源码和适用验收为准。当前状态只在本台账维护；历史记录不得当作未完成指令。首片完成仍保留父项缺口；依赖和范围变化写清原因。普通修复与重构更新受影响契约及简短状态，不强制新建 research 文档；验证要求见 [AGENTS.md](../../AGENTS.md)。

## 任务台账

“依赖/前置”表示实施前需核验的能力，不是已冻结的 API 或调度顺序。领域预检随本领域首次开放一起接通，不能以 G07 未完成为由留下隐藏 native 否决。

| 编号 | 任务 | 依赖/前置 | 状态 | 交付证据 |
|---|---|---|---|---|
| G01 | 移动竞争优先级策略 | 现有 move 请求与仲裁 | 已完成 | [mod-move-contest](../architecture/movement.md#竞争排序)、[交付记录](../research/notes/g01-move-contest-2026-10-06.md)（G02/G03/G04 均已分片开放） |
| G02 | 移动准入策略 | 已稳定的移动请求输入；同步必要玩家预检，G01 不作硬依赖 | 部分完成 | G02.a 准入槽（b7e65d5）、G02.b 障碍槽（e032991）、G02.c 预检槽（`0f390b5`）：[mod-move-admission](../architecture/movement.md#准入与预演)、[mod-move-obstacle](../architecture/movement.md#障碍追加)、[mod-move-precheck](../architecture/movement.md#请求预检)（G02.d1 静态放行、G02.d2 玩家门双端冻结、G02.d3 首片墙地形静态通行声明均已交付，见[静态通行声明](../architecture/movement.md#静态通行声明g02d3)；孵化门槛归 G13；安全模式主人摘要为可解释性增强另登记） |
| G03 | 完整房间移动仲裁 | G01、G02 | 部分完成 | G03.a 整房请求视野 + evaluate 链预演（`0efd645`）：[mod-move-admission](../architecture/movement.md#准入与预演)、[记录](../research/notes/g03a-move-arbitration-2026-10-06.md)；pull 登记已开放（`movePullRegistration` 槽 + add_pulling 双射修复，见[移动契约](../architecture/movement.md)）；静态放行由 G02.d1 实现，障碍槽第二预演与完整仲裁替换仍未开放 |
| G04 | 移动代价与执行效果 | G02；代价可先行，完整仲裁交互在 G03 后验收 | 部分完成 | G04.a 代价槽已交付（`builtin:room.moveCost` + add_fatigue u32 钳制，见[移动契约](../architecture/movement.md)）；疲劳恢复/重量评分要素未开放 |
| G05 | Mod 作者基础 CLI | 现有 sim、inspect、检查点 | 已完成 | G05.a `mod init` `ee296cb`；G05.b `mod test` `dbc82a4`（[模板与测试说明](../architecture/local-simulation.md#作者包模板)）；后续增强另登记 |
| G06 | 运行 trace 与故障复现 | 现有活动链与故障诊断 | 部分完成 | G06.a 槽调用/回退事件 trace + `--mod-trace` CLI `ce84345`：[mod-trace](../architecture/mod-trace.md)、[记录](../research/notes/g06a-mod-trace-2026-10-06.md)；过滤参数/停用阈值暴露/一键复现未做 |
| G07 | 玩家预检策略扩展 | 各领域策略契约；可分族穿插 | 部分完成 | G07.a 战斗射程首片（[player-precheck-rules](../architecture/player-precheck-rules.md)、[记录](../research/notes/g07a-combat-ranges-2026-10-06.md)）；G07.b ext 意图压制接线+可扩展意图盘点篇（2026-10-07，见[历史交付记录](archive/mod-history-2026-10.md)）——动作互斥的 ext 声明面已开放、vanilla 压制表与 EXECUTION_ORDER 未动；EXECUTION_ORDER 策略化（G07.c 候选，设计前置）与参数策略未开始 |
| G08 | Mod 建筑字段与生命周期 | 现有建筑目录、schema、tick | 部分完成 | G08.a 字段+tick 首片已交付（见[历史交付记录](archive/mod-history-2026-10.md)）；RCL 配额/逐资源容量/专属 FIND/生产事件未开始 |
| G09 | 自有对象库存与耐久能力 | 现有对象/资源/伤害能力 | 部分完成 | 四子片已交付：G09.a 库存（store 声明+信封六链）、G09.b 耐久（hitsMax+信封、伤害/死亡/预检同源）、G09.d 声明式反击（retaliationRatio、creep 延迟/塔立即、safeMode 抑制、策略替换）、G09.c 结构受击槽（builtin:mod.receiveDamage、托管延迟排空、修改后伤害落地）（[自有对象库存](../architecture/object-store.md)、[自有对象耐久](../architecture/object-durability.md)）；实际伤害联动反击等按需另拆子片 |
| G10 | 可编程生产与建筑动作 | G08；现有资源与配方 | 未开始 | — |
| G11 | 建筑配额与资源容量策略 | 现有建筑/资源目录 | 部分完成 | G11.a 配额族首片已交付（132 条逐型逐级规则+玩家表折叠注入+link.rcl 迁移，见[配额规则](../architecture/structure-quota.md)与[夹具](../../compat/fixtures/mod-quota/README.md)）；资源容量策略与逐房/动态配额未开始 |
| G12 | 采集与资源再生策略 | 相关原生消费者与当批领域预检，G07 可随后通用化 | 未开始 | — |
| G13 | 孵化、寿命与死亡策略 | 相关生命周期与当批领域预检，G07 可随后通用化 | 未开始 | — |
| G14 | 塔的动作策略 | 现有伤害/治疗/受击策略 | 未开始 | — |
| G15 | 控制器与玩家成长策略 | 对象与账户候选边界 | 未开始 | — |
| G16 | 实验室与工厂策略 | G10/G11 相关能力与默认玩法核查 | 未开始 | — |
| G17 | 跨房 Mod 资源传输 | 现有 terminal 全局结算与资源目录 | 未开始 | — |
| G18 | 市场扩展 | G17；订单/账户候选 | 未开始 | — |
| G19 | 房间生命周期 | 房间阶段与受控候选访问 | 未开始 | — |
| G20 | 玩家与世界生命周期 | G19；全局串行屏障 | 未开始 | — |
| G21 | 地图初始化与自主世界事件 | G19/G20 对应域、G22.b 时间/RNG、地形缓存 | 未开始 | — |
| G22 | 执行身份、玩法时间与随机协议 | .a 状态清单先行；.b 在随机事件前；.c 在更新/回放前 | 已完成 | G22.a/.c/.b 全部交付（.b：确定性时间/随机协议+种子面，2026-10-07，见[历史交付记录](archive/mod-history-2026-10.md)） |
| G23 | 包/schema/state 迁移 | G22.a/c；RNG 迁移另依赖 G22.b | 部分完成 | G23.a 已交付（`97f4ed9`）：载荷格式版本与解码前显式迁移链（v1→v2 收编 last_auto_disable 旧形态、v2 起执行策略硬门）；内容级迁移与 RNG 联动未开始 |
| G24 | tick 边界更新与 watch | G23；玩家工作者/API 原子切换 | 部分完成 | G24.a 已交付（`1ee4fc9`）：无开放轮次的包/API 切换（替代分片换装、残留硬门、sim mods_switch 与冷恢复闭环）；watch 未开始 |
| G25 | 持久输入日志与结算回放 | G22；.a 日志协议与保存，再 .b 重放校验 | 已完成 | G25.a/.b 全部交付（.a：写入与校验读取；.b：结算回放与完整状态核对，2026-10-07，见[历史交付记录](archive/mod-history-2026-10.md)） |
| G26 | 自动检查点与 JS storage provider | .a 基于现有检查点；.b 依赖 G27.a；回放恢复接 G25 | 部分完成 | G26.a 已交付（2026-10-07：sim 周期保存、keep 淘汰与目录「最新可用」恢复选择，见[历史交付记录](archive/mod-history-2026-10.md)）；.b 待 G27.a |
| G27 | 提交后输出与后台服务 | .a 最小后台宿主；.b 依赖 G25.a/G26.a，按需接 G20 | 未开始 | — |
| G28 | Backend 贡献入口 | .a 实现 M3 网络基础；.b 依赖 G27 与 .a | 未开始 | — |
| G29 | 玩家类型与客户端贡献 | .a 类型生成前置；.b 客户端基础依赖 G28.a；.c 依赖 .b | 部分完成 | G29.a `mod types` 声明生成与 `--check` 门禁 `73b0f1b`：[mod-types](../architecture/mod-types.md)、[记录](../research/notes/g29a-mod-types-2026-10-06.md)；.b/.c 未开始 |
| G30 | 官方预设、覆盖审计与性能验收 | .a/.d 先建立再滚动；.b/.c 随领域交付，存量变更依赖 G23 | 未开始 | — |

## 实际执行队列与前置门

上一轮维护范围已提交；realm 注册/执行/预算及声明转换优化已提交为 `0c28c92`。Room API 状态所有权和历史文档入口治理已提交为 `8f822fd`；Prelude 命名与集中装配优化已提交 `de15765`；验收成本与首轮 research 文档价值优化已提交 `646e395`；Game 装配、地图缓存、指令状态与世界注入边界已提交 `a0d375e`；结构 API/孵化生命周期、字段/装配所有权及共用目标/诊断依赖优化已完成验收，三轮已提交 `b99cda3`，Creep 移动已提交 `3abb616`；engine/Mod 三组与 CLI 职责分组及整体复审已提交 `eedfbfb`；计划安装候选与统一发布已提交 `4443c3c`；runtime 单次调用与局部恢复治理已提交 `dd1e4f7`；字段校验边界治理已提交 `f32f972`。专项维护暂告一段落，剩余治理项不作为恢复开发的前置。G02.d1/d2/d3 首片均已交付；G02.d3 后续（结构/rampart 通行声明、逐房粒度）按需另拆。G11.a 与 G09 全四子片（a 库存/b 耐久/c 结构受击槽/d 声明式反击）均已交付（见上方实施记录）；下一批候选：G02.d3 后续（结构/rampart 通行声明）、剩余 24 例零散兼容失败（量小，见 M2D 记录）或 G11 后续。M2B 兼容批按路线图并行。

父编号用于领域归档，**不是一口气做完 G01 再顺序做到 G30 的调度表**。每个父任务首次实施前拆成可独立验收的子片，记录显式依赖；前置小片可先完成，父任务保留“部分完成”。默认兼容性 M2B 按现有路线图继续推进，不等全部 Mod 领域结束。

| 阶段 | 建议执行 | 阶段出口 |
|---|---|---|
| 启动 | G30.a 当前领域能力清单、G30.d 同功能默认参考；G01 | 排序扩展没有改变默认选胜/阻塞传播，输入有界且拒绝可恢复 |
| 作者与移动 | G05.a init、G05.b test、G29.a 类型生成；G02/G03/G04 分片；G06.a 链诊断 | 作者能用公共工具运行两个策略包、定位失败和保存恢复；每批均有必要预检 |
| 对象与规则 | G22.a 全状态/身份清单；G08/G09/G10/G11；G12–G18 逐领域 | 每新增写入域同时保存、冷恢复和失败重试；G30.b/c 随已开放能力提取官方内容 |
| 房间与世界 | G19/G20；G22.b 时间/RNG 在 G21 随机事件前接通；G21 | 权限/阶段、跨房原子候选和新增持久输入清晰 |
| 更新与耐久 | G22.c 执行身份；G23.a 状态版本及显式迁移；G24/G25 分片；G26.a 周期检查点可提前 | 旧数据明确拒绝或显式迁移；切换失败保旧版，回放不重跑玩家 |
| 异步与存储 | G27.a 最小异步后台宿主；G26.b provider；G27.b 持久输入/outbox | 同步 tick 与异步 I/O 分域，世界与待输出原子持久，确认耐久后投递 |
| 网络与客户端 | G28.a M3 最小真实服务、G28.b provider；G29.b 最小客户端、G29.c 贡献 | HTTP/订阅与渲染有真实消费者；认证/授权由宿主检查 |
| 收口 | G30.a–e 的滚动覆盖、官方预设、完整证据与基准 | 未完成的领域继续新增任务，不能以 30 个父编号结束代替完整目标 |

前置门：

- **新增写入域当批恢复**：开新字段、对象能力、账户、跨房、RNG、外部输入或 outbox 时，当批扩展[检查点清单](../architecture/checkpoints.md)及验证，不等 G23/G26。改变持久布局/语义时登记版本；迁移尚未实现可明确拒绝旧版本、验收新世界，不能静默默认填值宣称兼容。
- **预检在首片接线**：G02/G12/G13 等先实现自己所需的冻结数据与玩家/引擎检查；G07 负责通用化，不得让 G07 等所有领域、所有领域又等 G07。玩法纯函数/声明用于预检，不能从玩家 VM 同步回调可信服务器 realm。
- **后台与持久化拆开**：G27.a 不依赖 outbox 或 JS storage，先提供受控异步执行与生命周期；G26.b 依赖 G27.a；G27.b 依赖 G25 的持久输入协议与 G26.a 的原子发布基础，可使用 native 后端，无须等 JS provider。因此不形成 provider ↔ outbox 循环。
- **协议设计与实际交付分开**：G27.b 首片即明确耐久确认点、输入消费/输出记录与世界的原子保存；仅“提交后发送”不保证崩溃恢复。先用模拟接收端验收幂等/去重，再接 G28 的真实服务。
- **M3 基础不能当免费前置**：G28.a 包含原生最小网络宿主、认证/授权、代码上传、房间订阅、控制台等[路线图 M3](../roadmap.md#m3-网络客户端与多用户)基础；G29.b 包含最小真实客户端。两者各自拆子片，基础未实现时 provider/渲染贡献不标完成。

关键子片硬依赖如下；“现有”表示本次核查可用的基础，实施时仍须确认源码。其余领域首次拆片时追加自己的依赖与验收，不能从这个局部图推断所有远期设计已经冻结。

| 子片 | 硬依赖 | 首片交付 |
|---|---|---|
| G22.a | 现有 | 全状态/版本与身份清单 |
| G22.b | G22.a | 可恢复玩法时间/RNG |
| G22.c | G22.a | 活动执行策略的持久身份 |
| G23.a | G22.c | 状态版本与显式替代候选迁移 |
| G24.a | G23.a | 无开放轮次的包/API 切换 |
| G25.a | G22.c | 持久输入与玩家结果日志协议/保存 |
| G25.b | G25.a、G22.b | 结算回放与完整状态核对 |
| G26.a | 现有 | 单写者周期完整检查点、RPO 与恢复选择 |
| G27.a | 现有 | 独立异步后台生命周期，仅探针 I/O/已提交读面，不改玩法 |
| G26.b | G26.a、G27.a | native 与 JS provider 等价原子发布/读取 |
| G27.b | G27.a、G25.a、G26.a | 持久输入消费与 outbox，确认耐久后投递 |
| G28.a | G27.b | M3 最小真实服务，外部写入走受控输入 |
| G28.b | G28.a | JS 路由/订阅/业务 provider |
| G29.a | 现有 | 冻结描述的玩家类型生成 |
| G29.b | G28.a | 最小真实客户端 |
| G29.c | G29.b | 客户端资产/渲染/面板贡献 |

G28.a 的网络协议及只读原型可提前设计；开放会改变玩法的请求前须完成输入/提交边界。G23.a 不必等待玩法 RNG，实现其迁移时再依赖 G22.b；G25.b 则不能省略时间/RNG 输入。类型与工具可先交付，不把整个 G29 或 G05 作为移动策略的硬依赖。

## 维护结构优化

S01 已提交 `e0b457b`，S02 已提交 `5a25bc2`。旧 S03–S06 的按行数拆文件和禁止调整候选字段约束由[职责治理方案](archive/structure-governance-2026-10.md)替代；串行场景默认顺序和 V8 生命周期约束继续保持。本轮维护已完成验收并提交；专项优化暂告一段落，建议恢复功能开发，具体选型见文件开头。

## 近期：移动与作者工具

### G01 移动竞争优先级策略

开放房间级竞争的只读输入及可组合优先级候选，让 JS 改变竞争者排序。原生默认排序与平手登记顺序保持兼容。至少两种独立策略及原生对照，覆盖抢格、多处理器组合、正常拒绝默认回退、故障与缓存重试。这只是排序切片，不代表完整仲裁已开放。当前 `Movement::check_impl` 使用完整原始矩阵评分，先给每个目的格选一个胜者，再取消不可移动/阻塞者，**不自动递补落选者**；G01 默认路径不能改为先过滤资格再选胜者。实际执行阶段还会复核 `can_move`，G02 必须同时处理这处消费者。G01 先通过有界同步调用生成数值/排序候选，再稳定排序，不在 Rust 排序比较器中回入 realm 执行有副作用回调；明确平手、非法值、来源完整性和 before/native/replace 的关系。不得静默截断房间请求。入口先核查[默认仲裁](../../crates/kk-engine/src/settle/movement/mod.rs)、[移动执行复核](../../crates/kk-engine/src/settle/creep/movement.rs)、`room_loop.rs` 和[移动动作槽](../architecture/creep-move-slot.md)。

### G02 移动准入策略

盘点疲劳、MOVE、地形/结构障碍等默认限制，开放具名可替换入口；玩家预检、请求登记、仲裁和实际执行同源消费，不得 JS 放行后又被隐藏 native 检查否决。未开放的检查逐项登记。数值、坐标和索引一致性属于宿主约束，准入规则属于玩法。

**G02.b 仲裁障碍单向追加阻挡槽（2026-10-06 已交付）**：新增房间级槽 `builtin:room.moveObstacle`（内建槽 16→17、revision v10）：准入槽之后、取消链之前整房一次进 JS；输入含全部胜者（准入被拒者仍在列）的目的格对象摘要（creep 让位=胜者∧准入终值、rampart 公开性、工地 structureType、墙/路）与预填时点 native 初判；`ctx.obstacle.block(id)` 单向追加 + `isBlocked` 读初判，decline 丢弃追加。取消链 obstacle = 追加集 ∥ 链内 `check_obstacle_at_xy`，让位链增量传播（挡 b → b 取消 → a 失让位随链取消）由引擎完成；静态放行未开放（2026-10-07 登记 G02.d；动态占格保持宿主约束）。stay-north（replace 区域管制）与 no-swap（after 禁止依赖让位）两独立包 + 原生 + 组合并集验收，双通道冷恢复等价；默认路径零新增可测开销。意图静态预检 `has_static_obstacle`（intent_move/slot_request_direction）与摘要安全模式主人登记为剩余。

**G02.c 意图静态预检槽（2026-10-06 已交付，`0f390b5`）**：新增房间级槽 `builtin:room.movePrecheck`（内建槽 17→18、revision v11、PlanHash 变更；catalog 断言同步）：全部意图与移动动作槽登记完成后、竞争输入构造之前整房一次进 JS；输入为整房请求摘要（moverId/owner/origin/target/movePower/weight/pulled/pulling + 目的格 terrainWall/terrainSwamp/hasRoad/objects[]，按对象 ID 升序）；`ctx.precheck.isBlocked/block` 单向收紧，被否决请求经 `remove_requests` 在竞争前移除，**等同从未登记**（不参与竞争评分、不让位、不占格；old_fatigue 残项仅胜者可读、pull 配对保留与基线逐步一致——均有引擎基线对照测试）。放行方向当前未接线（仲裁期 native 障碍仍复检；2026-10-07 登记 G02.d 贯通）；两登记入口（玩家意图/移动槽能力）共享矩阵统一否决，同源有运行时可观测证据。请求级 vs 胜者级语义（否决强竞争者请求 → 弱者独占目的格）与障碍槽互补、可同房组合（有测试）。验收：引擎 4（输入完整性/请求级语义+准入视野不含/拉者否决基线/候选域）+ 运行时 8（字段形态+原生对照、decline 回退含 before 层暂存 state 丢弃、组合 isBlocked、移动槽通道同源、预检×障碍两级组合、能力门、故障停用回退+晚期重试恰一次、无请求不进 JS）+ fenced/courtesy 两独立包四场景 × JSON/KKPV + 检查点冷恢复 + 四 plan_hash 身份。movement.rs 超限按职责拆出 `movement/inputs.rs`（槽输入投影子模块）。三路复审无高危，14 项发现全部清偿（staged_states 缺失、gitignore 白名单、槽计数史实、恒真断言等），详见[交付记录](../research/notes/g02c-move-precheck-2026-10-06.md)。当时记录 G02 父任务收口；2026-10-07 深度复审已纠正该范围判断，父项恢复部分完成，静态阻挡放行及必要玩家预检登记 G02.d。孵化门槛归 G13、障碍摘要安全模式主人为可解释性增强另登记。**下一步按队列：G22.a 状态清单**；G03 剩余（pull 登记策略、障碍槽预演）与 G04 按依赖另拆。

**G02.a 疲劳/MOVE/有效被拉门槛准入槽（2026-10-06 已交付）**：新增房间级槽 `builtin:creep.moveAdmission`，仲裁拆为 排名 → 原生预填（配对复核 + can_move 终值表）→ JS 覆盖（`ctx.admission.getAdmitted/setAdmitted`，初值即原生结果）→ 取消链四阶段；执行阶段 `movement_execute` 改读准入终值（同源、fail-closed）。配对复核遍历顺序改为胜者对象 ID 升序（集合等价；同拉者多目标残标在现有意图流不可达，G03/G04 开放 pull 登记时须重审）。tireless（replace 全放行）与 strict（after 收紧配重不足）两独立包 + 原生对照 + 组合（after 收回 replace 放行）验收，双通道冷恢复等价；默认路径输入构造零付费，微基准 1.84–1.95µs/房对基线 1.61–1.75（密集 32 移动者场景准入终值表维护 +0.2–0.3µs）。内建槽 16、revision v9、PlanHash 变更；旧检查点按身份拒绝。剩余：G02.b 障碍准入（`has_static_obstacle` 意图预检、`check_obstacle_at_xy` 仲裁障碍与让位链耦合）、孵化中拒绝、越界/坐标（宿主约束）。

### G03 完整房间移动仲裁

允许 JS 选择获准移动集合，覆盖让位链、交换、循环、拉拽和阻塞传播。宿主验证表示与索引，不重新运行默认算法覆盖结果。明确整条房间链的数值/集合候选、暂存 state、正常拒绝与 native 回退，不复用单执行者拒绝边界冒充房间恢复。

**G03.a 整房请求视野与获准集合链预演（2026-10-06 已交付，`0efd645`）**：准入槽输入扩展 + 只读链预演，不动槽集合（17 槽、revision v10、PlanHash 不变——基线对照构建实测一致 `72d231e2…`）。交付：① `ctx.args.requests` 整房原始请求视野——全部登记（含竞争落选者）按 目的格→格内登记序 排列，逐条 `moverId/x/y/index/originX/originY/won`；原始矩阵在 `rank_winners` 零拷贝暂存（Working 每房每 tick 重建，天然瞬态）。② `movers[].pulledBy`（有效配对拉者 ID，与 `pulled` 同点复核）与 `pulling`（拉者侧登记态，目标落选本轮不生效）。③ `ctx.admission.evaluate()` 暂存候选链预演——按候选**当前**终值在瘦克隆（只复制链消费字段）上运行与正式仲裁完全相同的 `cancel_blocked`，返回幸存胜者 ID 升序数组；只读、即时计算非快照；障碍槽追加集此时必为空（槽序固定 + 预演能力随准入槽令牌过期双重保障），预演结果是最终结果的上界。拒绝仍整房回退原生预填（非单执行者边界）；`Working::active_safe_mode_user` 抽取后障碍输入/预演/正式链三处同源。验收：引擎等价性 5 函数 11 拓扑案（交换/环/扣环员级联/链式/占位/覆盖/拉拽双案/安全模式分支/快照不消费）+ 运行时 3 测试（覆盖→预演端到端闭环含 requests 字段形态、evaluate 在障碍/竞争/动作槽走能力门不可被 decline 掩盖）+ wave（replace，预演发现级联损失全房停）/calm（after，落选者视野扣同格胜者）两独立包 + 原生对照 + 组合并集，JSON/KKPV 双通道、四 plan_hash 身份互异、准入后检查点冷恢复等价（CLI 2 测试）。三路复审无高危（正确性/一致性路零中级），14 项发现全部清偿（含夹具 gitignore 强制入库、克隆瘦身、安全模式等价护栏），详见[交付记录](../research/notes/g03a-move-arbitration-2026-10-06.md)。剩余另拆：pull 登记策略开放（须重审 `validate_pull_pair` 胜者 ID 升序遍历的同拉者多目标顺序语义）、障碍槽预演/静态放行方向（G02.d；动态占格保持宿主约束）、G04 代价执行。

### G04 移动代价与执行效果

开放重量、地形代价、疲劳生成/恢复等策略，明确道路磨损、工地踩踏、跨房过渡的顺序。真实玩家验证不同规则；任意传送若需新能力另行设计，不混入方向请求。覆盖战斗受伤后身体效能及失败重试。

### G05 Mod 作者基础 CLI

实现 `mod init`、`mod test` 最小闭环：模板包含 manifest、server/player、场景、断言和保存恢复示例，作者只用公开 CLI 即可验收。不得覆盖现有目录；独立包示例无需内部 Rust 测试。名称及参数在实施时核查既有 CLI，避免占位命令冲突。

### G06 运行 trace 与故障复现

提供可选、有界 trace，包含 tick/房间/目标/binding/阶段、拒绝、故障和回退，与 inspect 活动链一致。输出可复现材料，说明它是否足以回放，不把诊断快照当确定性日志。限制记录量、开启成本和玩家私有数据范围。

## 对象能力与内建规则

### G07 玩家预检策略扩展

建立可扩展的玩家预检契约，逐族调整射程、目标准入、部件条件等玩法；身份、归属、参数表示检查保留。canary 与真实玩家 VM 同源，结算端有对应权威检查。领域首片先接通必要消费者，本任务收口通用机制。玩家只得到该预检所需的公开规则/纯函数，不获得服务器字段或世界写句柄；清洗器的 `EXECUTION_ORDER`、动作互斥、参数策略和可扩展意图也要逐项盘点，不能只改 API 返回值。身份可按明确的服务端命令授权扩展，不通过玩家伪造 owner 绕过。

### G08 Mod 建筑字段与生命周期

接通 schema 字段、玩家读取和自主 tick，兼容现有工地/完工/库存/配方。验收初始化、受损维修、摧毁、检查点、异常和缓存重试。与[当前建筑边界](../architecture/mod-buildings.md)核对，不另建不参与建造的伪建筑。先拆 G08.a 字段/初始化及投影，G08.b tick/阶段资格；新增字段随当批检查点保存，preTick 另在 G19.a 收口对象与房间的相对顺序。

### G09 自有对象库存与耐久能力

通过通用能力声明让自有对象参与资源搬运、伤害、治疗/维修及死亡。明确类型适用范围，贯通预检、查询、事件、快照、索引。第二种对象不新增专用 Rust 变体；结构受击与实际伤害反击须明确另拆子片，不能假定现有 creep 受击槽已覆盖。至少拆 G09.a 库存、G09.b 耐久/死亡、G09.c 结构受击；需要实际伤害反击时另立 G09.d，明确减伤前/后、治疗前/后数值、来源归因、截断摘要、非递归与同轮追加结算顺序。不要直接在当前受击槽向已经 tick 完成的来源排队而丢失反击。新的部件/效果/能力表示随 G30.a 的差距清单单列，不假定 `BodyKind` 永久封闭。

### G10 可编程生产与建筑动作

在声明式配方之外提供自定义生产决策、具名动作、多配方、状态条件、冷却和进度。全部资源支付/产出完整预检、原子应用，覆盖满仓、拒绝、异常与恰一次恢复。

### G11 建筑配额与资源容量策略

补齐 RCL 配额、isActive、逐资源容量。玩家常量、工地预检、建造、搬运、生产、超容处理共同消费目录，明确内容选择和检查点身份变化。

### G12 采集与资源再生策略

开放采集产量/消耗及再生入口；验证耗尽、周期恢复、多人竞争、玩家面和失败恢复。资源对象的投影/生命周期与现有声明资源区别写清。

### G13 孵化、寿命与死亡策略

至少拆孵化成本/耗时、出生/出口、寿命、死亡产物四个子片，分别开放。保持出口、名字/ID、孵化对象、库存事务一致，用真实 spawnCreep 验收，不伪造 spawning。

### G14 塔的动作策略

开放攻击/治疗/维修公式及消耗，复用统一伤害/受击。覆盖距离、改向、结构摧毁、事件和拒绝；不能把 tower 已可造成原生伤害当作策略已开放。

### G15 控制器与玩家成长策略

开放升级、占领/预定、控制器周期与成长规则；明确房间归属、GCL/账户候选，不能只改对象字段遗漏玩家状态。跨域写入不足时先完善边界或分片登记。

### G16 实验室与工厂策略

核查实际默认实现后拆片，开放配方、库存约束、冷却与玩家方法。至少拆 G16.a 实验室/化学和 G16.b 工厂；配方、玩家预检、库存消费、冷却、事件与恢复按领域完整接线。配置表存在不等于消费者接线，尚未实现的默认能力另行登记，不放宽兼容跳过来掩盖缺口。

### G17 跨房 Mod 资源传输

在解除 terminal 对声明资源拒绝之前接齐目录、费用、目标房、两端库存、事件与检查点。跨房成功/失败原子，缓存重试和冷恢复不重复支付；当前 terminal 已存在不等于接受 Mod 资源。

### G18 市场扩展

接通 Mod 资源订单和交易策略，覆盖余额、订单余量、费用、记录及游标，明确玩家预检、全局结算与账户访问域。与 G17 共享资源身份，非法输入不得局部入账。

## 世界规则、恢复与更新

### G19 房间生命周期

提供房间 pre/post 及受控候选，明确相对对象动作、仲裁、对象 tick 的顺序。拆 G19.a 对象 preTick 与房间阶段的顺序/资格、G19.b 受控房间操作，验证无玩家意图自主玩法、双房失败不发布；不只相信作者 parallelSafe 声明。生命周期不默认继承动作 decline；每个阶段明确成功/故障或经完整局部边界支持的拒绝。

### G20 玩家与世界生命周期

拆玩家阶段、世界阶段、服务端/跨房命令、账户操作子片，提供具名入口与可核验访问域/串行屏障。玩家全局、动态集合和运行连接器随 G30.a 领域清单落实到对应子片，连接器不绕过玩家沙箱隔离。输入、对象与账户一起提交，异常不会只发布部分房间。

### G21 地图初始化与自主世界事件

开放地图/地形初始化、NPC、周期事件；初始化与运行期更新分开。地形、投影、查询、寻路缓存及检查点同源更新，随机输入按 G22 协议完善。

### G22 执行身份、玩法时间与随机协议

区分 PlanHash、内容身份和停用后的实际执行身份，建立可记录/恢复的玩法时间与 RNG。拆 G22.a 全状态与身份清单（对象能力前置）、G22.b 确定性玩法时间/RNG（随机事件前置）、G22.c 可持久化执行策略身份（更新/回放前置）。任意 Date/Math.random/全局缓存不能被称为可回放状态；保持现有身份校验，不直接放宽旧检查点加载。

### G23 包/schema/state 迁移

在替代候选中显式迁移对象字段、namespace state、规则和包内容，预检失败保留旧世界/计划。缺包默认拒绝可写启动，保留只读导出。格式号变化必须配套读取/迁移，不自动修补不一致载荷。

### G24 tick 边界更新与 watch

依赖迁移实现包更新/watch；存在开放轮次、玩家缓存或待恢复 VM 时不直接切换。成功后玩家 API、计划、工作者与链一致，失败保留旧版。明确开发更新与存量世界升级的区别。

### G25 持久输入日志与结算回放

记录玩家已执行结果、外部输入及执行身份，从检查点重放结算，不重跑玩家代码代替日志。核对完整状态/事件；损坏、缺段、身份不匹配明确拒绝，实测 CPU 等输入的保存/比较口径写清。

### G26 自动检查点与 JS storage provider

拆 G26.a 基于现有完整检查点的周期保存/版本恢复选择，和 G26.b 基于 G27.a 后台宿主的可替换 provider。先明确 RPO、单写者发布、崩溃完整性；.a 无需等待 JS provider 或数据库，采用本地文件即可验收。异步 JS 存储属于后台域，不把同步玩法 realm 改成任意异步结算；用两种实现证明替换，故障注入覆盖发布各阶段。仅检查点不得承诺零丢失。

### G27 提交后输出与后台服务

拆 G27.a 异步后台 VM/模块生命周期及受控消息契约，先用有限模拟 I/O 验证可用于 storage；G27.b 再接持久输入/outbox。输入有身份/顺序，响应只能进入后续 tick；世界、输入消费游标与待输出记录须按所选耐久策略原子发布并确认后投递。定义去重或幂等、失败重试，覆盖发布前、确认后发送前和发送后确认前崩溃；不对任意外部系统承诺恰一次。后台不能直接写正在结算的权威世界。

## 服务、客户端与最终验收

### G28 Backend 贡献入口

拆 G28.a 真实最小网络宿主（纳入 M3 认证、上传、房间订阅与控制台消费者），G28.b JS 路由/订阅/业务 provider。明确访问/管理边界及协议版本，玩家上传脚本不能被作为管理员可信 Mod 装载；provider 返回数据仍按宿主认证/授权检查。至少两种实现可替换；接口占位不算服务交付。

### G29 玩家类型与客户端贡献

拆 G29.a 冻结类型/资源/字段/意图的 TypeScript 声明（近期穿插）、G29.b 最小真实客户端（M3）、G29.c 资源资产/渲染/面板贡献。玩家 JS 方法无法从源码可靠推断的签名须有显式描述或作者声明，不用全 any 冒充类型支持。未选包不残留声明，生成物新鲜度进门禁；客户端贡献在 G28.a 有真实 API 后验收，JSON 投影不算图形交付。

### G30 官方预设、覆盖审计与性能验收

拆成贯穿全程的子片：

- G30.a 覆盖与缺口清单：开始每个领域前核对本地 xxscreeps 实际版本和可执行消费者，登记当前/部分/未实现、本项目子片、独立包及证据。新增缺口要新增子片或 G31+，不能用父领域名字代替全量覆盖。
- G30.b 官方内容与新世界预设：随能力开放交付真实公共 SDK 内容、依赖组合与默认/精简/自定义预设。新世界选择不必等 G23；存量卸载/升级另依赖 G23。
- G30.c 内建内容所有权迁移：第一次提取原生玩法时解决 builtin 固定装配、键/数字常量所有权、选择门和默认兼容。选中包可用通用 native 加速，未选不得由 native fallback 恢复已卸载内容；故障停用保留目录的回退另按既有契约。不要用两个目录互相覆盖绕过冻结冲突。
- G30.d 性能与工作量证据：重大重构前保存同功能默认参考，每片测量相同输入规模、原生/JS/组合的时间、资源和 VM 调用。不能先删掉参考实现再到收口补基准，或为了零开销禁止独立 JS 策略；优化如 native 加速应公开且语义等价。
- G30.e 最终收口：汇总全部领域、官方预设、真实作者工作流、恢复/升级与同场景对照；未完成项继续留在台账，不因 G01–G30 的父任务有首片而宣布全面超越。

2026-10-06 路线复审抽查本地 xxscreeps `5ff0d1a682170f6b5f9c8bac3e703767625988e0`：`config/loader.ts` 提供 11 个领域；`mods/classic/index.ts` 还聚合 powercreep、observer、nuker、effects、portal、intershardResource 与 messages/visual 等内容。以下是责任映射和待细化范围，**不是本次已经完成的逐能力覆盖审计**。

| 领域 | 责任任务 | 不能遗漏的待核查消费者 |
|---|---|---|
| config | G05、G22、G23、G30.b/c | 配置 schema/默认值、版本与能力清单、预设依赖与实际展开身份 |
| constants | G07、G09、G11、G29.a、G30.c | 资源/类型/效果/数值、玩家与服务端同源、内建数字常量所有权 |
| schema | G08、G09、G22.a、G23 | 新字段/枚举/能力表示、内建挂载、编码、引用与迁移 |
| processor | G01–G04、G08–G21 | 对象 preTick/tick、动作与互斥/执行序、房间/世界阶段、默认规则替换 |
| game | G07、G20、G29.a | 玩家类/方法/全局、动态集合、查询/预检、环境初始化和声明 |
| driver | G20、G22、G25、G27 | 玩家输入/输出/持久数据连接器、隔离、已执行结果与重试 |
| main | G20、G21、G26、G27 | 服务器启动/关闭、周期调度、NPC/世界初始化和管理 |
| terrain | G21 | 地图/地形生成、更新、缓存 epoch、寻路与恢复 |
| storage | G25、G26、G27.b | 完整快照、日志、业务存储与底层 JS provider、原子耐久发布 |
| backend | G27、G28、G29.b/c | 认证/授权、路由/订阅、messages/notifications、版本与客户端资产 |
| test | G05、G06、G25.b、G30 | 作者场景/断言、组合/故障复现、回放、独立包与成本基线 |

全量玩法清单还需展开 power/effects、observer、nuker、portal/跨 shard、intershardResource、flag/visual 等在 30 个父标题中不明显的内容：G09 负责对象/能力通路，G20 负责世界/跨域规则，G21 负责初始化，G27–G29 负责服务/客户端；G30.a 按真实源码明确每个子片或新增父任务，不用这些责任范围宣称已有实现。现代玩法或跨 shard 基础缺失时同时登记默认玩法前置，不能依赖不存在的域开始对外开放写入。

## 每批共同验收

- 先明确公开契约、候选/拒绝/故障边界和实际消费者。可替换玩法/能力至少两种独立 JS 实现及原生对照；CLI/类型工具用多包输入与失败/保护案例，迁移/日志/存储用多版本与崩溃故障矩阵，审计用源代码/消费者证据。不能为通用工具强造两个玩法实现，也不能用一个硬编码包冒充通用能力。作者创作不依赖 Rust 特判或重编。
- 距离、价格、公式、准入和竞争优先级属于可替换玩法；ID、合法表示、字段/索引/编码一致性和原子发布属于宿主约束。现有表示不够时设计扩展，不能静默否决 JS。
- engine 不依赖 V8，mod-api 保持纯数据；玩法 JS 同步，进入 VM 前释放候选借用，复用预算、首故障、缓存重试和停用机制。
- 按相关性覆盖组合、已定义局部边界的拒绝、异常/预算、停用回退、晚期写回失败、缓存重试和真实进程冷恢复；JSON/KKPV 完整业务状态一致，非确定 CPU 等字段说明口径。
- 源码/测试不超过 1000 行，按职责拆分；按影响更新现行契约与适用夹具，记录实际验证后复审。
- 每片首次设计记录输入范围/大小、单位与整数/浮点域、完整预检、权限及 state/世界写面；变更持久域同时更新清单与版本策略。native 中心、before/replace 与拒绝的关系逐槽定义，生命周期不自动支持动作式局部拒绝。
- 修改 Rust 后重新构建测试 CLI、显式指定 `KKSCREEPS_BIN`；提交前运行完整 `pnpm run check`。兼容相关改动按 [compat/README.md](../../compat/README.md) 新构建 release、核验固定依赖、构建适配器并运行 ratchet，另外比较本批实际基线的逐用例状态，不能只比较总通过数。

## 实施记录与交接模板

当前会话只记录范围、剩余工作、实际验证和提交状态；历史过程见[归档](archive/mod-history-2026-10.md)。贡献与交接要求以 [AGENTS.md](../../AGENTS.md) 为准，不复制提示词或逐项复审流水账。

2026-10-08 G09.b 自有对象耐久首片交付：基线 `39334ae` 后的 G09.b 登记提交（同日三连批：G09.a `978a042` → 登记 → 本批）。信封键路线落地：`hits_max` 冻结声明（1..=u32::MAX 与建筑同口径）+ ModObj.data 第二族信封键 `hits/hitsMax`（与 store 族键集不相交、可并存；持久化随不透明 data 串，无 revision bump）；伤害链全线同门（近战/远程/塔/策略伤害经 `object_hits`/`structure_hits_max` Mod 臂）；死亡=remove_obj+事件 `type=类型键`（无废墟无掉落）；保留键 10 项冻结层拒绝；写回丢弃输入信封回填世界值；逐 tick 校验三臂（hitsMax==声明、hits∈1..=hitsMax——复审收紧，0 只能直改造出）。与登记偏差：塔玩家面也开 modEntry 族（三处共用 `gw.modDurabilityTargetOk`）；type_name_of 对 Mod 保持空串（战斗读面单独取类型键）。验收：两独立包（siege 近战/vault 双信封+远程）+无耐久 decoy -7 对照+组合+双通道+冷恢复全绿，三场景 plan_hash 互异；领域项补齐 rangedAttack（RANGED_ATTACK_POWER=10）、塔击毁、战斗读面/策略目标接纳、击毁无废墟、计划内无耐久类型静默（settle_room 不走 validate_stage，直插构造测的就是结算纵深门——Lane C 核实）。验证：fmt/crate 686+sim 91 全绿（间歇问题未再现）；棘轮 1983 持平（0 转红 0 转绿）；三 bot 2000/2000/10 验收判定通过；无参 check 通过。三路复审 0 P0，P1×6/P2×8 当批清偿（信封文案、okTarget 重建事故、塔门、ranged/tower/策略读面补测、marker 真类型、界值两端接受等），P3 登记或清偿（自选继承 Structure 角落、dismantle 双重排除、冷恢复不跨死亡——hits 为不透明串+remove_obj 通用清理，低风险登记）。**本批坑：prelude JS 嵌在二进制内——改 creep.js 后必须重建再跑 sim（提取共享助手时丢 okTarget 定义，旧二进制下 vault 过而 siege run_failed 的分裂形态即此因）；python 正则替换多步脚本一步断言失败全弃是安全的，但断言锚点要随早前修改更新。** 下一批候选：G09.c 结构受击或 G09.d 反击（依赖面待调查）、G02.d3 后续或剩余 24 例零散兼容失败。

2026-10-08 G09.a 自有对象库存首片交付：基线 `39334ae`（G09.a 登记，前序 G11.a 为 `bd1a87c`）。路线①落地：`store_capacity` 冻结声明（Option<u64> ≤2^53-1，登记为 u32、实施放宽以对齐 JS 安全整数，无存量用户）+ ModObj.data 信封键 `store/storeCapacity`（库存作为不透明 data 串的内嵌键，不新增持久化形态，KKPV/检查点随既有链路，无 revision bump）；六链接纳（creep 双向结算、mod 资源槽 inspect/change、投影合成、玩家预检 modEntry 目标族、对象 tick 读、发布前校验三臂守卫）；保留键 8 项（+store/storeCapacity）在冻结层拒绝（builtin:link 豁免；storeCapacityResource 经信封拆分隔离）；字段写回丢弃处理器信封、回填世界值。验收：两独立包（搬运型/仓储型）+原生对照（无容量 marker 预检 -7）+组合+双通道+冷恢复全绿，三场景 plan_hash 互异；领域项（双向 transfer/withdraw、错误码同源、mod 槽触达、保留键冻结拒绝、投影形状）随 sim 断言。验证：完整门禁通过；棘轮 1983 持平（0 转红 0 转绿）；三 bot 2000/2000/10 验收判定通过；三路复审 1 P1+4 P2 当批清偿（信封写回、保留键层次、容量界、typegen 成员、校验守卫补测、hash 对照），P3 已知角落登记于契约文档。已知边界：首片单一总容量，无逐资源容量/槽绑定/损耗。下一批候选：G09.b 耐久/死亡、G02.d3 后续或剩余 24 例零散兼容失败。

2026-10-08 G11.a 建筑配额首片交付：基线 `c8fcb8d`（M2D + 台账登记）。路线①+③ 合体落地：132 条 `builtin:structure.quota.{type}.rcl{N}`（键集=官方表形，默认=官方值）+ `StructureQuotaSnapshot` 装配期一次解析、Rc 共享入逐房 Plane + `fold_controller_structures` 折叠注入玩家 `CONTROLLER_STRUCTURES`（canary/玩家 worker 双点，switch/冷恢复同路径）；`allowed_for` 统一五消费者（工地创建/link isActive/spawn 放置/terminal/extractor，快照 miss 回退官方静态表）；顺带修复 link.rcl 规则不达玩家面的既有缺口（rcl0-8 迁入族、键集改官方形，LinkRules 去 rcl）；revision v18。三轮探针场景三面互证预检/结算同源：native 锚 6/2500，raise 塔 20、lower 墙 3，组合共存，四场景 plan_hash 互异，双通道一致，冷恢复身份一致。验证：完整门禁通过；棘轮 1983 持平（0 转红 0 转绿）；三 bot 2000/2000/10 指标+验收判定全通过；三路复审 0 P0/P1，P2（Plane 逐房深拷贝）当批改 Rc 清偿，P3 清偿或登记（折叠键序依赖 workspace preserve_order 已注释并加全表相等断言、配额族缺键拒绝补测、link 玩家面覆盖为间接、RCL0 键不声明为既知边界）。**间歇问题登记**：sim_tests 全套并行运行时 move_policy/heal_policy/move_arbitration 冷恢复子进程偶发退出码 1（复审方 5 次全套撞 2 次，隔离复跑全绿、非本批改动引入；本批新增测试或放大并行负载），再现时须保存失败子进程报告归因。下一批候选：G09.a 自有对象库存、G02.d3 后续（结构/rampart 通行声明）或剩余 24 例零散兼容失败。

2026-10-08 M2D ID 构造器批交付：基线 `5133968`（M2C 对象形状批）。ID 构造器/对象身份族 18 例清偿：registry.js 游戏类从计数桩改为官方构造器（四款签名：guardedId/strictId/powerCreep/flag），位置初始化同构官方 RoomObject 构造器；embed 场景 96 钉 instanceof/字段回查/子类/空实例/未知 id 文案/strictId/Flag 六参/Structure 键序。**含 M2C 回归修复**：room 挂载三键「结构存在才有键」使 probe 缺失诊断把建造前读计为缺 API、bot 验收判红（M2C 批验证只对了 tick 指标）——诊断豁免补 Room.controller/storage/terminal + classOfLabel 剥 `[...]`，三 bot 验收判定复验通过。教训：bot 验证必须核对验收判定行，不能只比四项指标。验证：完整门禁通过；棘轮 1965→1983（18 转绿零转红）；三 bot 指标+判定全通过；三路复审 0 P0、P1/P2 当批清偿。有意分歧登记：手动构造不重指 room 三键（保持 getObjectById 同引用）、Room 保持桩、effects 原始格式双轨待 power 域统一、类全局 Proxy 使 `X.prototype.constructor === X` 不成立（沿袭）。剩余 24 失败集中于 keeper lair/getGameTime/setup 零散域，量小不构成下批主体——下一批建议回主线（G11.a 建筑 RCL 配额或 G09.a 自有对象库存）或 keeper lair NPC 域。

2026-10-08 M2C 对象形状批交付：基线 `2effa30`（M2B 安全模式批）。对象形状族 18 例 + ROOM-SURVIVAL-001 清偿，全部为 prelude 挂载面对齐、无引擎语义改动：结构类字段表按官方类层次挂原型（fields.js known 类型表覆盖全部官方 structureType，ELSE 兜底桶仅剩 Mod 建筑与真未知类型；road/wall/rampart/extractor 撤泄漏的 energy/store 族，terminal/container 保留官方 store/storeCapacity，link/tower 补三件族，wall 补 ticksToLive）；摘除实例标记 isStructure（全仓无读者）与 controller activationDate 桩；room 的 controller/storage/terminal 改官方「结构存在才有键」自有属性挂载（官方 Structure 构造器 structures.js:41-56 同款）、撤 mode、恒挂 survivalInfo；Game.powerCreeps 恒对象、Game.cpu 撤 shardLimits。验证：完整门禁通过；棘轮 1946→1965（本批 19 转绿零转红）；三 bot 基线一致（Overmind terminal.energy 读值随官方语义转 undefined，指标不变）；三路复审 0 P0/P1/P2，P3 已清偿或登记（cpu setShardLimits 方法面待 multi-shard 补数据面、powerBank 防御分支、room memory 挂载位差异）。下一批候选：ID 构造器/对象身份族（剩余 42 失败的主体 18 例——`new Creep(id)` 官方构造器语义）、keeper lair NPC 域、G11.a/G09.a。

2026-10-08 M2B 安全模式批交付：基线 `8c69481`（G02.d3 首片）。归因：21 例安全模式族失败非守卫缺失——`blockedBySafeMode`/withdraw 门/activateSafeMode 预检本已在位，根因是 (1) 引擎激活门把缺失冷却/降级钟数值化为 0 拒绝（官方 undefined 比较恒 false；世界时间 0 首激活被 `0 >= 0` 拒），(2) embed 种子相对钟 +1（vanilla 的 +1 系其 createShard 热身 tick 所致），(3) prelude `safeMode` 未生效读 0 导致敌方守卫恒不放行。修复：controller.rs 三门 `if let Some`、setup.rs 去 +1、structures.js getter undefined 化；同批清偿同族 undefined 保留（ticksToRegeneration 投影 null + getter 还原）。已知角落（复审 P3）：官方 claim 落 null 数值化会拒同批激活、本实现 claim 落 None 放行——原始 intent 序才可达，官方玩家面挡在 API 层，已在 controller.rs 注释登记；prelude `ticksToDowngrade` 缺省仍 0（与引擎 None 放行两半建模差异，对象形状族后续批）；economy/creep 侧 sibling `unwrap_or(0)` 恒等价待按约定对齐。验证：完整门禁通过；棘轮 1918→1946（+28 零转红零豁免增长），allow 清理 D3 两项惰性条目；三 bot 2000/2000/10 与基线一致；三路复审 0 P0/P1、2 P2 注释清偿。下一批候选：对象形状/ID 构造族（剩余 61 失败的主体）、G11.a、G09.a 或 G02.d3 后续。

2026-10-08 G02.d3 首片交付：基线 `f32f972`。实现与验收细节见上文 G02.d3 节与[交付记录](../research/notes/g02d3-terrain-passability-2026-10-08.md)。验证：无参数 `pnpm run check` 完整通过（Rust 1235 通过/0 失败，sim 85，文档链接与源码规模绿，817 文件无超 1000；pathfinder.rs 879 行越过 700 复审线为既有+本批增量，登记后续按职责复审）；release 重建后棘轮 1918 通过/89 既有失败/907 跳过，零转红零转绿；三组 bot 完成 2000/2000/10 轮，核心指标与入库基线一致（Overmind 保留既有首轮异常 1）。三路复审无 P0/P1 代码问题，P1 文档层已清偿，P3 登记见上。下一批候选：G02.d3 后续（结构/rampart 通行声明）、G11.a 建筑 RCL 配额、G09.a 自有对象库存或 M2B 兼容批（安全模式 17 项失败优先复现）；恢复开发时按依赖与价值重选。

上一轮验证：完整 `pnpm run check` 通过（Rust 1221 通过/12 忽略、Python 工具 58+12、Node 门禁回归、JS workspace、文档链接与源码规模）；`cargo test --workspace --doc` 通过（6 通过/2 忽略），最终 Clippy/格式通过。重建 release 后兼容棘轮 1918 通过/89 既有失败/907 跳过，零转红；三组 bot 指标及错误形态与入库基线一致（TooAngel/Overmind 2000 轮、WASM 10 轮；Overmind 有既有首轮初始化异常）。本记录随维护重构提交入库；开发基线为 `5a25bc2`，提交号以 Git 历史为准，未推送。

2026-10-07 维护结构优化完成：移动领域归拢、Working 构造/瞬态及首触恢复所有权、世界/投影/执行器组织、Generic 读取、串行诊断及文档/工具治理已落地。架构页 46→37，当前 plans 页 17→4（含索引），历史结论保留归档。本轮不扩展玩法或放宽恢复身份；builtin.rs 与 builtin/store.rs 字节同 HEAD，串行 56+94 场景调用和顺序保持。Working 外部字面构造迁至 new，调用方已迁移，见[Rust 迁移说明](../architecture/state-editing.md#rust-调用方迁移)。

完整门禁、doctest、最终 Clippy/格式、release ratchet 和三组 bot 已执行，结果见当前接手位置；定向 `embed_snapshot_contract_impl` 通过，未匹配的 `__no_such_scenario__` 正确失败（退出 101），完整门禁清除过滤条件。兼容机器报告更新；bot 对照报告保存在 `/tmp/kks-maintenance-{tooangel,overmind,wasm}.json`，不替换历史基线。复审未发现阻塞问题；现有长文件与共享 VM 故障半径仍需按实际职责持续改善，不以本轮宣布全仓架构完成。下一玩法任务仍须审定 G02.d3 设计或按依赖选择其他领域；本轮未自动继续新玩法。用户已授权提交本轮改动；提交记录以 Git 历史为准，未推送。

## G02.d3 声明式寻路规则（2026-10-07 登记；2026-10-08 首片交付）

G02.d2 盘点结论登记：数据面=计划级可走性规则（仿建筑 `BuildingPathing::{Obstacle,Plain,Road}` 先例，冻结计划承载墙/结构可走性声明）；消费面三点同源=kk-pathfinder 地形包/room.js `getPathfindingGrid2` 基线矩阵/engine `static_obstacles.rs` 原生默认；开放问题=玩家可见地形形状（`room.getTerrain().get`）是否随规则改写、跨房矩阵投影边界。验收至少包括：独立 JS 包声明墙可走、原生对照、双端寻路一致、结算与寻路同源、双通道冷恢复。tick 级 allowStatic 决策与服务器 JS 回调永久不进玩家寻路（时序与粒度不可行，宿主约束）。

2026-10-08 开发选型建议（据此实施）：首片限定计划级墙地形通行声明，统一 native 寻路地形/Room 基线矩阵/engine 静态默认，保留动态占格、组合否决与玩家 CostMatrix 的既有语义。物理地形查询保持原值，仅派生通行视图；不可见的已注册房可消费公开静态规则，不暴露对象信息，未注册房的全墙保护环不得被规则放开。**当日已按此范围交付**：`builtin:terrain.wallPassable` 规则 + `TerrainPassability` 快照（kk-mod-api）；engine `arbitration_blocked` 墙项与预检输入同源（Plane/Movement 携带旗标，链预演克隆同值）；玩家寻路 `TerrainPack::native_with_walls` 打包后清真实房间的 wall 位（1→平原、3→沼泽），墙环保留；三通道接线（Frozen 缓存键含旗标、`SandboxConfig.terrain_walls_passable` 覆盖 json/kkpv、canary 与工作者配置从计划装配）。执行语义零新分支：疲劳按既有位掩码判沼泽，墙=平原费率、墙|沼泽=沼泽费率。组合语义为 freeze 既有约束：同规则双包一律覆盖冲突（即使值相同），组合验收用跨规则（wallPassable+fatigueLimit）；等值共装若要放开属 freeze 组合语义变更，后续另行设计。验收：引擎 4 测（平原费率放行+原生对照、墙|沼泽沼泽费率、动态占格不放开、结构障碍不放开）、runner 单测 4 项（清位+墙环保留、false=物理表逐字节、kkpv 物化、x≠y 取样）、沙箱串行 59 场景（JSON 通道旗标两态对照，默认分支与 G02.d2 moveTo 边界互证）、sim 四场景（native/ghost/deepwalk/combined）双通道+冷恢复+verify-replay+四 plan_hash 身份、物理查询四场景均读回 TERRAIN_MASK_WALL。三路复审无 P0/P1 代码问题（P1 均为文档/台账层，已清偿；清偿含 native 场景"原地不动"改"沿墙面绕行不越墙列"、combined/deepwalk"同值共装"表述纠正、switch.rs 封闭集注释补 MOVE 两键与旗标、三处批次号注释改行为描述、测试 x≠y 取样）；P3 登记：Worker 快照不含旗标（靠 create 路径覆盖的隐式约定）、kk-world place 无 builtin_terrain_passability 对应物（catalog 测试兜底）、跨房墙环端到端与 allowStatic×声明叠加测试缺口。详见[交付记录](../research/notes/g02d3-terrain-passability-2026-10-08.md)。

备选排序：G11.a 建筑 RCL 配额/isActive（已有建筑、工地和库存通路，需玩家预检/结算同源），G09.a 自有对象库存（价值高，但须贯通通用 Store/目标类型/投影/日志/保存，范围更大）。M2B 单列兼容开发批次（2026-10-08 安全模式/对象形状/ID 构造器三批连续清偿，见实施记录）：当前报告 1983/2914 通过、24 失败、907 跳过，剩余失败为 keeper lair/getGameTime/setup 等零散个例与 NPC 未实现域，量小不构成下批主体；跳过集中于实验室、工厂及 power/nuke 等未启用能力。建议下一批回主线（G11.a/G09.a）或清 keeper lair NPC 域。G27/G28 服务链与内容迁移/watch 暂列后续，不把占位 crate 视为已就绪。

2026-10-07 维护续轮：基线 `679c79d`。realm 按装配、注册、执行、安装、预算、诊断与生命周期组织；Registry 将空闲/注册包/密封阶段和首故障封装，调用方经阶段方法操作；活动链和预算参数改为具名类型。声明转换按对象/意图/处理器/规则/建筑资源领域组织，由单包 ContributionBuilder 持有候选和绑定覆盖校验。VM 字段销毁顺序、SDK/内建身份源码、宿主协议与公开 API 保持。开发门禁支持单目标过滤、空匹配失败及步骤耗时，无参数仍为完整验收；真实定向通过，虚构过滤退出 1。实现及适用架构已同步，无新玩法或任务报告。完整无参数门禁通过（Rust 1221/12 忽略、Python 58+12、Node 7、JS workspace、274 Markdown/1742 本地链接、668 源码文件规模）；host doctest 1 通过/1 依赖宏示例忽略。Rust 全量测试实际耗时 703.71s；本轮改善定向反馈，不声称已降低完整测试成本。复审确认 8 类声明转换分支保留默认值/校验/错误语义，未发现阻塞问题。后续仍需按实际消费者复审玩家 Room API、research 文档价值和全量 CLI 测试成本；整个项目的维护优化并非全部结束。用户已授权提交续轮；提交号以 Git 历史为准，未推送。bot 证据位于 `/tmp/kks-host-{tooangel,overmind,wasm}.json`：TooAngel/Overmind 2000 轮、WASM 10 轮，全部完成，无终止/惩罚跳过/缺失 API；Overmind 保留既有首轮初始化异常。对照仅核验上述指标及归一化异常，日志时序、随机 flag 名与部分调用计数存在差异，不声称机器报告逐字一致。

2026-10-07 Room API 续轮开始：基线 `0c28c92`，查询索引/出口缓存、Room 身份寻路矩阵、地形安全拷出、绘图缓冲与创建预检分别持有状态，Room 入口保留公开方法装配顺序。临时桥工厂在装配期捕获，玩家执行前仍删除 `__kkp`。现有沙箱测试通过；40 个具名函数/方法体复核保持，属性赋值保留匿名方法 name 形态。research 索引去重、已修复审查链接后续处置，08/09 报告注明历史基线，当前进度入口统一指向台账。最终完整门禁与 release 回归通过，实际结果见当前接手位置。13 个公开方法的赋值顺序经 AST 对照保持，内建构造器/真 TypedArray.set/数组 push 的捕获约束保留。bot 证据位于 `/tmp/kks-room-{tooangel,overmind,wasm}.json`：2000/2000/10 轮全部完成，无终止/惩罚跳过/缺失 API，Overmind 仍有既有首轮异常；只比较核心指标和归一化异常，不宣称随机名称、日志与全部调用计数相同。未新增玩法、未新增任务报告、未减少验收断言或轮数；剩余消费者及公开方法均已迁移，复审未发现阻塞问题。用户已授权提交本轮，提交号以 Git 历史为准，未推送。全量 CLI 测试成本与其余 research 文档的逐项价值复审仍是后续事项，不以本轮宣称整个项目完成优化。

2026-10-07 Prelude 命名与装配续轮开始：基线 `8f822fd`，用户已授权执行语义目录与集中装配方案；语义目录、kebab-case 文件名和 V8 脚本名已迁移；内嵌脚本清单、lodash/常量、条件扩展意图及 Mod 安装入口统一归 `sandbox/prelude.rs`，`host.rs` 保留句柄捕获/生命周期调用。已确认 26 个 JS 文件可执行 token 与基线一致、24 个基础段顺序一致，workspace 全目标编译与文档链接通过；完整门禁与新建 release 回归通过，实际结果见当前接手位置。V8 来源名改为职责名称，仅改变诊断定位；宿主句柄捕获、Mod 安装预算、安装入口捕获/解绑/作者模块执行及无 Mod 收尾顺序保持。类型生成校验和现行文档路径已同步，历史证据只修链接、不改当时结论；无未迁移消费者，复审未发现阻塞问题。bot 证据位于 `/tmp/kks-prelude-{tooangel,overmind,wasm}.json`，2000/2000/10 轮全部完成，无终止/惩罚跳过/缺失 API，Overmind 仍有既有首轮初始化异常；对照仅核验核心指标与归一化异常，不声称日志和全部调用计数相同。用户已授权提交；提交号以 Git 历史为准，未推送，不推进新玩法；全量 CLI 测试成本及其余 research 文档价值复审仍待后续优化。

2026-10-07 验收成本与文档价值续轮开始：基线 `0f082f5`，工作区已有其他任务的 renderer/导出夹具/F2 文档修改，不纳入本轮；当前测量 CLI 固定启动成本及场景重复执行，未提交。

2026-10-07 验收成本与文档价值续轮完成：`sim run` 每进程完整可执行文件 SHA-256 是主要固定成本，仅将 dev/test 的 `sha2` 依赖设为 opt-level 3，业务优化级别和 release 不变，仍保留逐进程完整摘要及检查点身份门。同一 117263160 字节文件、相同 64KiB 流式读取、调用方未优化，依赖优化前后各三次：3.824/3.858/3.878s 与 0.228/0.198/0.199s；六次摘要均与 Python hashlib 一致（本地原始结果 `/tmp/kks-hash-comparison.json`）。普通四轮 move-cost 场景优化后约 0.24s，首轮热身另记，不将测量解释为 gameplay tick 收益。

移动代价/竞争策略测试按模块用 OnceLock 共享本次测试进程内的只读 JSON/KKPV 基准报告，正常全目标减少 8 次等价 CLI 启动（每模块 28→24 次进程启动，基准 12 次调用收敛为共享 8 次）；注册的 84 项 sim 测试、业务断言和独立进程保存/恢复均保留，失败收集不缓存为成功。完整门禁结果见当前接手位置（日志 `/tmp/kks-cost-full-check.log`），此前 Rust 步骤 703.71–799.47s 与本轮 204.44s 属跨轮观察，代码/负载不同，不作严格成对倍率声明。未改玩家 API、玩法语义或 release 配置，本轮未重跑完整 release screeps-ok/bot 矩阵，旧轮结果只作历史记录。

逐项复审 research 的 01–09 主报告与九份候选笔记；有独立上游取证、实验/性能方法、边界矩阵的内容保留。N2b 两份后续修复正文并入建筑记录，上下文复审正文并入读取封套，删除三份独立 Markdown，原始 JSON 全部保留，标题仅降一级；正文完整性对照通过，引用/索引与适用架构同步，无新任务报告。该范围之外的存量 research 尚未逐篇复审，项目整体维护优化仍未全部完成。代码复审确认共享报告无跨构建缓存且恢复仍为真实新进程；本轮改动未提交、未推送，下一维护方向为 Game 装配/注入/指令边界，未自动启动新玩法。


2026-10-07 Game 装配续轮开始：验收成本轮已提交 `646e395`，本轮不推进新玩法；按地图缓存、指令状态与 tick 初始化职责建立模块，保持现有公开 API 和宿主句柄，未提交。


2026-10-07 Game 装配续轮完成：Game 入口持有对象装配与当前实例；地图工厂持有每 tick 路由工作数组及惰性 MapVisual；指令模块持有清单/显示计费；world-tick 通过一次性传入的刷新函数协调双通道注入，并提供既有宿主桥。加载期捕获桥，玩家期不查 __kkp；初始 Game/currentGame/指令初始化以及每 tick 索引→指令→Memory/CJS→地形/API→Game 顺序保持。可执行 token 对照确认地图完整工厂体（含属性顺序）、resetIntents/intentsPayload/beginTickImpl 和宿主桥对象保持，buildGame 仅改地图工厂调用；未新增公开写能力或玩法。

完整门禁及最终 release 回归见当前接手位置；Rust 步骤 141.38s，仅记录当前观察，不归因于本轮模块整理。日志 `/tmp/kks-game-{full-check,release,ratchet}.log`；bot 报告 `/tmp/kks-game-{tooangel,overmind,wasm}.json`，2000/2000/10 轮完成，无终止/惩罚跳过/缺失 API，归一化异常与入库基线一致，不宣称所有调用计数或日志逐字相同。兼容机器报告与适用架构/注释同步，无新任务文档。无未迁移消费者，复审未发现阻塞问题；改动未提交、未推送。下一维护方向为 spawn 创建/孵化生命周期的职责及状态所有权，仍须按实际消费者复审；本轮不代表全仓优化完成。


2026-10-07 Game 轮提交前复审：核验新增闭包自由变量与捕获来源、地图缓存每 tick 生命周期、初始刷新/指令重置与双通道注入顺序、宿主桥捕获后解绑，以及其余内嵌脚本顺序。领域函数/装配体可执行 token 和各模块加载期捕获初始化表达式对照通过；未发现需要修复的阻塞问题。完整门禁及最终 release/逐用例/bot 证据沿用本轮已执行结果；用户授权审查通过后提交，提交号以 Git 历史为准。


2026-10-07 结构 API 续轮开始：Game 轮已提交 `a0d375e`；structures-spawn 混合多类结构读面/动作和孵化状态，提取孵化闭包并显式维持装配顺序，剩余入口按实际职责命名，不扩展玩法，未提交。


2026-10-07 结构 API 续轮完成：混合的 structures-spawn 入口改为 structure-api，孵化名称数组、成本/能量计算、Spawning、待出生视图及创建/续命/回收方法归 structures/spawn 闭包。入口传入共用目标检查与弃用诊断，分阶段安装视图/方法/通知/装配器以维持跨结构注册顺序；单个 tick 钩子的注册位置及名称→诊断重置顺序保持，只捕获重置函数，不保留安装接口对象。30 个具名函数/方法的完整 token 保持，展开阶段调用后的 49 个装配表达式内容/顺序保持；安装期捕获内建函数/常量来源、延迟调用与 TDZ、InvaderCore 的 Spawning 复用、同 tick 名称拒绝与 Memory/待出生视图/指令边界经源码复审，无阻塞问题。所有当前消费者与适用注释/架构/设计入口已迁移；无新玩法、无新任务文档。

完整门禁和新建 release 的逐用例/三组 bot 验证见当前接手位置，日志 `/tmp/kks-spawn-{full-check,release,ratchet}.log`；bot 报告 `/tmp/kks-spawn-{tooangel,overmind,wasm}.json`，2000/2000/10 轮完成，无终止/惩罚跳过/缺失 API，归一化异常保持，不宣称全部调用计数和日志逐字相同。兼容机器报告同步。本轮改动未提交、未推送；上一轮 Game 复审通过后已提交 `a0d375e`。后续维护优先复审结构字段目录/通用装配与目标检查的消费者边界，避免继续让结构入口承担无关职责；本轮未把其余结构域或全仓优化标为完成。


2026-10-07 结构字段续轮开始：沿用未提交但已验收的孵化轮工作区，实际 HEAD `a0d375e`；通用字段表被结构入口和装配器共享，时间折算/专用字段混在动作入口，本轮按读面所有权归拢，不提交或改写前轮工作。


2026-10-07 结构字段续轮完成：structures/fields 持有通用字段表、类型选择、Mod 建筑字段回查与专用时间/资源读面；结构入口仅捕获核心字段、结构装配器与专用字段三个安装函数，在原位置调用，私有字段表不进入共享桥。17 个具名函数/方法体、展开阶段后的 40 个装配表达式及字段表/常量初始化表达式保持；实例兜底遮蔽、Missing/0/undefined、冷却计数器/绝对时间区别、Source/Mineral/Controller 排除与 InvaderCore 的 Spawning 复用经源码复审，无阻塞问题。未新增玩法、公开写能力或任务文档。

累计孵化+字段改动的完整门禁和新建 release 回归见当前接手位置；日志 `/tmp/kks-fields-{full-check,release,ratchet}.log`，bot 报告 `/tmp/kks-fields-{tooangel,overmind,wasm}.json`，2000/2000/10 轮完成，核心指标及归一化异常保持，不宣称所有日志/调用计数相同。现行架构与兼容机器报告同步。HEAD 仍为 `a0d375e`，两轮累计改动均未提交、未推送；其余结构动作和共用目标/诊断消费者仍由结构入口持有，后续应按实际变化原因复审，不机械继续拆文件。


2026-10-07 共用目标与诊断续轮开始：HEAD `a0d375e`，孵化/字段两轮保持未提交；Creep/Room 运行期直查结构模块稍后登记的函数，改为独立依赖与加载期捕获。资源 API 的 primitive/string-object 与 plain-object 判定差异保留，不统一拒绝语义，不扩展玩法。

2026-10-07 共用目标与诊断续轮完成：`game/targets.js` 持有当前 rawById 的存在性检查与 lodash 口径序列化拒绝，`runtime/deprecation.js` 独占弃用消息去重数组；在 Room/Creep 前登记，消费者加载期捕获，取消结构入口的反向函数登记。moveTo 在原拒绝分支复用共用检查，资源 API 的 Object/primitive-string 口径保持；修正 harvest 不抛序列化错误的误导注释。原位置单个 tick 钩子保持名称→诊断重置顺序。四个共用函数的完整可执行 token 保持；加载/调用时序、当前索引回查和内建捕获经源码复审，无阻塞问题。

在现有串行沙箱套件增加行为场景，验证有效敌方目标、下一 tick 删除后的旧引用拒绝、boxed String 的 API 差异、删除 __kkp 后的可用性、玩家替换 console 后诊断仍使用原 console，以及 tick 内去重/下一 tick 重发；同步场景计数 56→57。初次测试编译暴露日志字段名错误，随后修正为 text；首次运行发现场景计数未同步，修正后定向与完整门禁均通过。累计验证见当前接手位置；日志 `/tmp/kks-targets-{sandbox,full-check,release,ratchet}.log`，bot 报告 `/tmp/kks-targets-{tooangel,overmind,wasm}.json`。HEAD 仍为 `a0d375e`，孵化/字段/共用依赖三轮累计改动未提交、未推送；现行架构和兼容报告已更新，无新增任务交付文档。后续可复审 Creep 的动作分组与共享预检边界，不按行数机械拆分。

2026-10-07 用户授权提交孵化/字段/共用目标诊断三轮累计改动；最终源码差异和链接复审通过，完整门禁及 release 回归见上条，提交号以 Git 历史为准。随后继续复审 Creep 动作与共享预检职责。

2026-10-07 Creep 移动续轮开始：HEAD `b99cda3`，上轮已提交且工作区干净。移动参数解析/路径解码/Memory `_move` 生命周期与其他动作混合；拟建立移动安装接口，保留 Room 静态与原型方法装配次序，共享部件检查和规则常量仍由入口传入，避免复制计划常量或统一不同动作的预检顺序。

2026-10-07 Creep 移动续轮完成：`creep/movement.js` 的工厂私有持有参数解析、方向表、路径解码及 `_move` 缓存逻辑；加载期捕获 registry/lodash/原 Room 序列化器，入口分别安装路径静态和移动方法，显式传入同一计划常量/部件检查/目标拒绝函数。21 个既有具名函数的完整可执行 token 保持；展开两个安装阶段后，35 条装配语句的内容/顺序保持。复审确认仅加载期查找 __kkp，路径静态在常量捕获/字段定义前安装、移动方法仍在其他动作前安装，不增加 tick 状态、不复制规则常量、不改变预检/可视化/Memory/指令行为。

在现有串行沙箱套件增加行为回归，玩家替换 Room 路径编解码静态方法后仍可移动，下一 tick 根据真实新位置截断并复用 Memory 路径，在 noPathFinding 下不重寻路，非法串仍抛原错误；场景计数 57→58，既有场景相对顺序保持。验证见当前接手位置，日志 `/tmp/kks-creep-movement-{sandbox,full-check,release,ratchet}.log`，三组 bot 报告 `/tmp/kks-creep-movement-{tooangel,overmind,wasm}.json`。现行架构和兼容报告已更新，无新增交付文档。HEAD `b99cda3`，本轮改动未提交、未推送；剩余 Creep 字段/共用常量和检查、采集/建造/战斗仍在入口，资源/控制器仍为独立模块，后续按消费者与变化原因复审，不能把本片标成全部 Creep 职责治理完成。

2026-10-07 用户授权提交 Creep 移动续轮；完整门禁、release 棘轮、bot 与源码复审证据见上条，提交号以 Git 历史为准。后续维护方向扩大到 engine 结算与 Mod API/host 的职责组织，不继续优先拆 Prelude。

2026-10-07 crate 职责治理开始：HEAD `3abb616`，工作区干净。复审 settle 的规则/候选混排、Mod API 的贡献构建环节与运行期目录混排、host 的包发现/选择/模块加载混排。按状态与流程边界分组并收口内部入口；小而单一的领域模块不强制加目录，领域规则不统一成一个中心分派。

2026-10-07 engine/Mod 职责分组完成：settle 的阶段/世界事务/玩家写回/校验/发布归 pipeline，事务内部可见性由 crate 收至 settle，Engine 与检查点仍通过原入口协作；Mod API 的声明/规则合并/处理器排序/冻结/计划/指纹归 contribution，运行期目录从 crate 根读取稳定公共类型，构建子模块保持私有；host 的 snapshot/manifest/dependencies/selection/loader 归 package，realm 只从父入口取得依赖序、选择和模块装载接口。16 份迁移实现除导入/可见性/include 相对路径及 rustfmt 调整外保持，SDK 虚拟 URL 与嵌入字节未变。公开类型/模块路径保持，无新玩法和协议版本改变。小而职责单一的领域/值协议模块继续平铺，未把三个 crate 标为全部治理完成。

复审发现 builtin.rs 的源码字节参与 code_digest：尝试用自然模块声明替代 builtin_capabilities 的 path 属性会改变 PlanHash，已撤回该附带改动，保留 builtin.rs/store.rs 原文。重建最终 release 后，官方与 typed inspect 的全部报告（除重新构建的 executable_sha256）与改动前完全一致，恢复身份没有额外变化。定向全目标编译最初暴露候选测试跨模块可见性问题，恢复原 settle 内访问范围并清除多余导入后通过。中间两轮门禁重叠且共用 debug CLI 的重建，最终版测试出现 5 个检查点加载拒绝；固定产物后恢复子集 13 项、全 sim 84 项及最终串行完整门禁通过，未放宽恢复身份/断言。中间失败结果不计为最终通过。

最终证据见当前接手位置；日志 `/tmp/kks-crate-boundaries-{compile,targeted,doc,full-check-serial,release-final,ratchet-final,resume-recheck}.log`；inspect 的 before/final 和三组 bot 的 final JSON 位于 `/tmp/kks-crate-boundaries-*`。兼容报告、现行模块导航与边界已更新，历史可点击源码链接只改目标、不追改结论；已完成轮次验证摘要归入现有历史记录，当前接手入口不再堆积旧验证。HEAD `3abb616`，本轮改动未提交、未推送。后续重点是 engine install/contrib 的计划与分派依赖、API 运行期目录/校验消费及 host chain 的候选能力边界；按共同状态和变更原因逐组复审，不把所有平级文件机械塞进目录，不恢复优先拆 Prelude。

2026-10-08 G09.c 结构受击槽交付：基线 `4e31928`（G09.d 交付）后实施，登记见上方选型段（登记提交 `96c0d7a`）。核心架构=托管延迟排空（伤害入口 `&mut Working` 与 JS 链的 `RoomCtx` 不可调和：Native 内联零开销/Hosted 挂 `mod_damage_pending` 意图后排空），扣减与声明反击提取 `mod_damage_effects` 两路共用。与登记偏差四处（目标形状经 ctx.target、事件序偏差、排空点在移动仲裁前、集火排空门）均已记入交付段。验证：fmt/release 后棘轮 1983 持平 0 转红、compat fail 0、三 bot 2000/2000/10 全过（overmind 1 例 console-error 为既知基线）、无参 check 通过（终轮）；测试新增引擎 5（挂起/排空/拒绝/内联/反击基数/攻击者消失）+ 宿主链 7（减伤/输入形状/拒绝/免疫/组合/击毁/集火/故障侧/双槽路由，×双通道）+ sim 2（三场景×双通道+mod_states 双通道一致+plan_hash 互异+冷恢复）。三路后台复审：A（引擎语义）P1×1 P2×1 P3×3、B（全链接线）P3×3、C（测试夹具）P1×1 P2×3 P3×5。清偿：反击基数分裂（P1，两位一致：retaliation_base 拆分+钉测）、攻击者消失门（P2，排空门+引擎测试）、故障侧/双槽路由/mod_states 双通道/decoy 贴脸/expect 早退/槽计数兄弟文档/source null 表述全部修复；登记：engine_with_player_files 构建器漂移、测试排空助手双实现、safemode 排空时点角落（已入 object-durability 已知角落）。G09 四子片至此全部交付；下一批候选：G02.d3 后续（结构/rampart 通行声明）、剩余 24 例零散兼容失败或 G11 后续。
