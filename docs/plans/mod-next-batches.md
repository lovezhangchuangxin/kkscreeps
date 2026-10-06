# Mod 后续开发任务与接手记录

本文件是**持续更新的执行台账**，用于 AI 接手、上下文恢复和每批进度交接。它细化[完整玩法计划](mod-complete-gameplay.md)，不替代 N1–N6 的范围、ADR 或现行架构。G01–G30 只是本台账的稳定任务编号；尚未完成的条目是目标，不代表接口已经存在。

长期目标：开发者通过独立 JS Mod 轻松创造各种完整玩法，覆盖调研版本 xxscreeps 的扩展领域，并提供更完善的组合、恢复和开发工具。项目自身逐步以公共 SDK 提供可选官方内容；第二种同类玩法不应再要求修改 Rust 或重编宿主。

## 当前接手位置

- 更新日期：2026-10-07。
- 当前任务：**深度质量优化六批已全部完成验收并随本批提交**（2026-10-07 收口；提交 ID 见 git log，文档与实现同次提交不回填自引用）。第 1–5 批实现加第 6 批最终验收落地：cooldown 热点实验五轮无稳定收益已撤回（最终 release 与 correctness 产物字节一致 `f0c78d0b…`）；正式性能矩阵 150 窗口完成——玩家主循环与 Mod 链持平、寻路四场景一致 4–7.5% 收益、RSS 持平、两版行为逐窗等价；长期 RSS 512KB 后平台通过。最终源码完整 `pnpm run check`、workspace doctest、release 棘轮 1918/89/907（转红 0、地板/豁免未动）、三 bot 2000/2000/10、旧 release JSON/KKPV 检查点逐值恢复与续跑均绑定最终产物重验通过。附带修复 quality.py 寻路基准行解析 bug（libtest 同行前缀）并补工具回归。实施期间 HEAD 保持 `229bea4`，收口提交见 git log；冻结产物与证据见[详细接手说明](deep-quality-handoff-2026-10-07.md)收口记录与[实施与验收报告](../research/notes/deep-quality-2026-10-06.md)（证据目录 deep-quality-evidence-2026-10-06/，矩阵原始窗口已合并为 matrix/rounds.json）。
- 实现参考基线：`0efd645`，整房移动请求视野与准入链预演（含此前移动策略、作者工具和 trace）；这是功能基线，后续文档提交不改变它的含义。接手时仍须核对实际 HEAD 和工作区。
- 最近已完成：G01 移动竞争（`9c8687d`）与 G02.a/G02.b 移动准入/障碍准入槽（`b7e65d5`、`e032991`）、G29.a 玩家类型生成（`73b0f1b`），此前为 N2c 受击策略第二片（`8e28ac4`）。
- 上批任务：**G22.a 状态清单与检查点 schema 门禁已交付**（2026-10-06，随本批提交，登记基线 `eeaf5f8`）：① `docs/architecture/state-inventory.md` 逐域登记编排层 8 域/引擎层 9 域/不持久化域 7 项（归属模块、字段路径、版本策略、恢复消费者、迁移状态——除调用方自管 metadata 外全部“拒绝不迁移，G23 负责”），三处索引与 checkpoints.md 分工交叉引用同步。② `checkpoint_schema_keys_are_pinned` 机器门禁：三场景键树并集（官方包全量双通道+p1 跨 shard 资源 ∪ 故障自动停用场景+mounts 扩展）与 99 键钉死清单双向 diff，夹具取值非空的字段增删即红；不透明叶子除 metadata 外含 mods.options、manifest、files_sha256、扩展字段载荷与 `engine.mod_states`（namespace state 为 Mod 自管 JSON，与扩展载荷同理，宿主门禁不耦合官方 Mod 内部 state 形状）。③ 版本与身份门现状如实登记（engine_version+room_schema_version 双硬门、plan_hash 恢复时重算等值核对、ModIdentity manifest+files_sha256+options、sim 附加 CLI/场景身份），RNG 边界注明属 G22.b、回放属 G25。夹具修正：MarketOrder.created_timestamp 改当前墙钟（订单生命周期按墙钟判过期，置 0 首轮即被清退，订单表原先进不了键树；该值随运行变化，不得据此引入字节级比较）。复审：三路后台只读（测试正确性/文档事实核对/台账范围），无 P0；两条 P1 已清偿——故障三域（fault_streak/last_fault/last_auto_disable，持久化链上无 deny_unknown_fields 的少数结构，R08 原形态）与空映射字段存在性（resources/extensions）改由非空取值场景钉死。验证：`cargo test -p kk-runtime --lib checkpoint` 14 通过；变异验证两轮（删 plan_hash 键红、订单 12 键新增红）；`pnpm run check:fast -- kk-runtime` 与完整 `pnpm run check` 见提交。证据：[状态域清单](../architecture/state-inventory.md)、[交付笔记](../research/notes/g22a-state-inventory-2026-10-06.md)。
- 上批任务：**G03.b 拉拽登记策略已交付**（2026-10-06，随本批提交，登记基线 `8f1677e`）：前置复审完成——①每 puller 每 tick 单目标由 `UserIntents.objects` 的 BTreeMap 结构保证；②`add_pulling` 覆写不清对侧残标（多拉者同目标原生可达、读侧无害，同拉者多目标不可达）；③开放重登记下首个失败配对的复核清理会连带清除当前配对（结果依赖胜者 ID 升序=对象创建序，任意）。交付：① 引擎 `add_pulling` 双向覆盖一致性（换目标清旧 `pulled_by`、换拉者清旧 `pull_of`，两表双射——复核与遍历顺序结构无关，顺带消除多拉者残标污染槽输入 pulling 视野）；② 新房间级槽 `builtin:room.movePullRegistration`（内建槽 18→19、revision v12、PlanHash 变更；SLOT_KEYS/描述符/bindings/install NATIVE/BUILTIN_SLOT_KEYS/freeze catalog 断言 19 全同步）：`arbitrate_room` 最前（预检之前）整房一次进 JS，输入=配对升序（pullerId/targetId/双方位置与 owner），`ctx.pull.isDenied/deny(pullerId)` 单向否决（等同从未登记：双向移除，预检/竞争/准入视野、准入预填、疲劳转移同源反映）；无配对不进 JS；decline/故障回退=全配对保留+before 层 state 丢弃；放行/新增不开放（新增拉拽无属主意图、疲劳转移会向未行动玩家计费——宿主约束，理由入文档）。③ 七点接线清单全落（含 execution_plan decline_allowed、chain.rs 错误枚举、sdk.js 两处 ctx.pull、realm op）。④ 验收：引擎 6 + 运行时 9（含疲劳转移可观测、pull×precheck×admission 组合、decline 双包）+ anchor（全量禁拖）/leash（反绑架跨主人）两独立包 + 原生 + 组合四场景 × 双通道 + 第 1 tick 后检查点冷恢复（sim 2 测试 97.8s）；变异验证证明顺序无关断言具备回归力（还原旧 add_pulling 即红）。复审：三路后台只读，1 P0（freeze 槽计数漏改，G01 同类坑）+2 P1+6 P2 全部清偿。证据：[架构](../architecture/mod-move-pull-registration.md)、[交付笔记](../research/notes/g03b-pull-registration-2026-10-06.md)、夹具 [sim-mod-move-pull-registration](../../compat/fixtures/sim-mod-move-pull-registration/README.md)。G03 剩余：障碍槽预演/放行方向（表示有效性宿主约束）与 G04 代价执行另拆。
- 上批任务：**G04.a 移动代价槽已交付**（2026-10-06，随本批提交，登记基线 `13c32c7`）：前置核对——执行链代价=裸 `move_weight`（非 MOVE/CARRY 件+负载，无下限，纯 MOVE 件自重 0）×地形费率（平原 2/沼泽 10/路 1 覆盖沼泽），疲劳经 `add_fatigue` 转记 `head_puller` 链首；恢复 -2×MOVE 件；道路磨损/工地踩踏/跨房归零为宿主固定顺序。交付：① 新房间级槽 `builtin:room.moveCost`（内建槽 19→20、revision v13、PlanHash 变更；SLOT_KEYS/描述符/bindings/install NATIVE/BUILTIN_SLOT_KEYS=20/freeze catalog 断言 20 全同步）：`arbitrate_room` 末段（取消链后、执行循环前）整房进 JS，输入=终局幸存者升序（id/owner/origin/target/weight/terrainSwamp/hasRoad/bodyLength/pulledBy 链首/nativeFatigue 与实收同源），`ctx.cost.getFatigue`（缺省=原生）/`setFatigue(id, 0..=u32::MAX)` 数值替换（升/降双向），缺项/decline/故障回退原生，无幸存者不进 JS；sdk.js 侧另有非负安全整数校验。② 宿主固定执行效果不开放：疲劳转记链、道路磨损、踩踏、跨房归零、恢复节奏；疲劳恢复策略与重量作为竞争评分要素另拆登记。③ 同批修复 `add_fatigue` 两处 u32 回写钳制（resting 与链首转记分支）——JS 可设 u32::MAX 覆盖后，同 tick 拉拽链多次转记可超 2^32，`as u32` 回绕会让惩罚经恢复路径蒸发；回归测试 + 变异验证钉死。④ 验收：引擎 4（输入要素/覆盖与缺项回退/域与值域/跨 2^32 不回绕）+ 运行时 7（沼泽原生停走 vs 策略续走、bog 平原隔 tick、decline、replace×after 组合 getFatigue 链、能力门、故障停用回退）+ swampwalker（无路沼泽压平原当量）/bog（全量翻倍封顶）两独立包四场景（含沼泽地形数组夹具，--ticks 4 断言 3 次落位）× 双通道 + 第 2 tick 后检查点冷恢复（sim 2 测试）。复审：三路后台只读（首派因账号配额上限失败、恢复后原样重派完成；配额窗口内先做本人自审——head 分支钳制即自审发现，复审路再补 resting 分支），1 P1（resting 分支回绕）+10 P2（weight rustdoc 跨槽口径、sdk 类型校验、注释 off-by-one、台账漂移等）全部清偿。证据：[架构](../architecture/mod-move-cost.md)、[交付笔记](../research/notes/g04a-move-cost-2026-10-06.md)、夹具 [sim-mod-move-cost](../../compat/fixtures/sim-mod-move-cost/README.md)。G04 剩余：疲劳恢复/重量评分要素开放另拆。
- 上批任务：**G07.a 玩家预检契约·战斗射程首片已交付**（2026-10-06，随本批提交，登记基线 `1422203`）：单目标战斗射程从双端硬编码改为规则驱动双端同源——一条 `builtin:creep.*.range` 规则经同一冻结计划同时决定 Rust 结算距离与玩家 API 预检常量。交付：① 四条内建规则 `attack.range`(1)/`rangedAttack.range`(3)/`heal.range`(1)/`rangedHeal.range`(3)，值域 0..=50（revision v13→v14，槽不动 20，规则 25→29，catalog 断言同步）；`CombatRanges::from_plan` 装配期快照（LinkRules 同形），引擎两条 install 路径 + `Engine::new` 默认（`builtin_combat_ranges`）+ `Plane.combat` 全链持有，结算三处判定点（attack/rangedAttack/heal±ranged 共函数）改读规则值，距离比较扩 i64。② 玩家面：constants.json +4 键（423→427，**本地新增**——上游 dump 无此名，重 dump 丢键由 catalog 逐键比对与 `c::*_RANGE` 编译引用双门拦住；gen-constants 重生成 kk-core），prelude 四门改读 C 表常量（attack/heal 由 isNearTo 改 `target.pos.inRangeTo(this.pos, C.*)`，官方接收者形态保留；默认行为逐位等价），setup.rs canary 与真实玩家两通道注入生效值（LINK_CAPACITY 先例）。③ rangedMassAttack 面积语义（3 格 1/0.4/0.1 档）显式排除并测试钉死。④ 验收：引擎 5（默认≡硬编码、扩程命中、零程拒、heal/rangedHeal 独立、对角切比雪夫度量、mass 排除）+ runner 常量默认/覆盖平铺与四门 + 运行时 4×双通道（端到端门+结算+事件、治疗预打伤归因、越界冻结拒、mass 排除）+ reach（attack 1→3）/medic（heal 1→3、rangedHeal 3→5，登记段子集表述、实际含 rangedHeal）两独立包 + 原生 + 组合四场景 × JSON/KKPV 双通道 + 第 2 tick 检查点冷恢复（sim 2 测试）。⑤ 文档 [player-precheck-rules.md](../architecture/player-precheck-rules.md)（通用契约首篇）+ 三处索引 + [交付笔记](../research/notes/g07a-combat-ranges-2026-10-06.md)。登记项「typegen 常量清单同步」核实为无需同步（typegen 只生成类型/意图声明，不枚举常量）。复审：三路后台只读（引擎/玩家面/夹具文档），无 P0/P1，9 P2 全部清偿（常量来源表述更正、对角度量测试、门形态统一、显式 ID 替代 find 顺序依赖、注释残留、台账闭环）；验证含评审机 sim 实跑 2/2。G07 后续子片（EXECUTION_ORDER、动作互斥、参数策略、可扩展意图盘点）未开始。
- 玩法队列：G24.a 已交付，本轮进行上述质量优化，未开始新的 G 玩法批次。下一批候选：G22.b 确定性玩法时间/RNG（解锁 G21 随机事件、G25.b 回放、G23 RNG 联动，当前首选）、G07.b 预检契约族后续子片、G08 剩余（RCL 配额/逐资源容量/专属 FIND/生产事件）、G25.a/G26.a、G09 起。sdk.js 981/1000 贴线，任何动 SDK 的批次先拆分该文件。
- 上批任务：**G24.a 无开放轮次的包/API 切换已交付**（2026-10-06，随本批提交，登记提交 `6a22684`，基线 `5fbadbb`）：世界在已提交边界（no_open_round + 同线程 V8 栈序门）原地替换 Mod 包集合与执行层，任一步失败旧分片原样继续运行。交付：① `EmbedEngine::switch_mod_packages`（embed/switch.rs，替代分片模式与 restore_checkpoint 同骨架）：活状态捕获 → 交集包 manifest/files_sha256 逐项等值（同包内容变更拒并指路 G23）→ 候选全链装配（注册→canary→install_hosted_plan→spawn）→ 孤立 namespace state 分诊（链式提交对每个执行过的绑定落 null 占位键——无信息量随包丢弃；非 null 实数据拒绝）→ 停用投影按新计划**装配选择集**求交重放并核对闭包（按发现集求交会把未安装 ID 喂给闭包，实测踩中）→ 故障三域按新装配集过滤 → retire + trace 缓冲/console 转移 → `*self = replacement` 原子切换；返回前后 plan_hash/执行策略/增删摘要。② 引擎侧 `Engine::restore_checkpoint_replanned` 换计划收养：跳过载荷 plan_hash 等值，保留 mod_states 命名空间/Mod 对象类型/扩展字段 schema 三重内容校验**对新计划**执行——天然充当去包残留硬门（残留会把世界钉死）；与原入口共享抽出的 adopt_committed_state。③ 玩家工作者重建：预算与模块表（玩家代码）保留，计划派生常量封闭集（LINK_CAPACITY/仓库十键/射程四键/RESOURCES_ALL）由 create 路径确定性覆盖或移除——旧计划常量不残留，玩家可见常量按新计划生效（sim 实证 LINK_CAPACITY 700→800）；V8 堆与冷恢复同口径。④ sim 场景字段 `mods_switch`（after_tick 后整体替换 configs/selection、拒绝不使运行失败）+ 报告 v6 顶层 `mods_switches` + 检查点元数据 `applied_switches`：续跑按世界时间**与实际生效的切换**选恢复集合（被拒切换不改变身份），续跑不重放已生效切换；载荷只读一次。⑤ 补 `fail_next_switch_for_test`（候选全验证后、retire 前）。⑥ 同批修复 kk-world 存量隐患：from_checkpoint 的 u64::MAX 陈旧哨兵令恢复后的世界插入对象即溢出（基线测试从未覆盖恢复后插入），insert 改减法比较。验收：runtime 7 项（增包身份换封+冷恢复新集合过旧选择集拒/减包规则回落/残留与内容双拒保旧/console 历史不被失败路径搬走/停用投影闭包/停用跨切换存活/闭包增长拒绝）+ 全量 46 项；sim 3 项双通道（增/减/冷恢复含边界前保存分支与 rejected 闭环——rejected 检查点按初始集合续跑）；完整 `pnpm run check` 绿。三路复审 P1×3（last_console 转移时序违反替代分片自述、架构目录索引漏登、台账表行漏同步）+ P2×15 全部清偿，复审记录入笔记。G24 剩余：watch（文件监视自动触发）未开始；内容级迁移/RNG 联动属 G23。证据：[包切换](../architecture/mod-switching.md)、[G24.a 交付笔记](../research/notes/g24a-package-switch-2026-10-06.md)、夹具 [sim-mod-switch](../../compat/fixtures/sim-mod-switch/README.md)。
- 上批任务：**G23.a 检查点格式版本与显式迁移链已交付**（2026-10-06，随本批提交，登记提交 `14c8f1d`，基线 `fe73d95`）：为 JSON 载荷域引入显式状态版本与解码前迁移框架（G23 段「格式号变化必须配套读取/迁移、不自动修补不一致载荷」的首片）。**范围拍板（用户，2026-10-06）**：仅本地开发、无存量用户——版本字段必填（缺失即拒、不做缺键猜测），迁移框架作为前瞻机制，v1 只以显式 `checkpoint_format_version: 1` 载荷存在供链演练。交付：① `Checkpoint` 新增必填 `checkpoint_format_version`（当前 2，保存恒写 2；缺失/非整数/超 u32/0/未来版本五路独立拒绝），版本职权四层文档化（信封 version=线格式 / 载荷格式版本 / engine_version+room_schema_version=二进制双硬门，engine_version 因 workspace 单一版本实为全量构建产物门）。② `embed/checkpoint/migrate.rs`：`migrate_to_current` 在 `deny_unknown_fields` 解码**之前**对原始 Value 路由（核心不变量），升级链仿房间快照 `UPGRADE_CHAIN`（第 n 项 0 起算把 n+1 升到 n+2，`skip(raw-1)` 依次执行，版本号链后统一改写；任一步失败整体拒绝、旧分片完整），返回迁移前原始版本供语义门决策。③ 首个注册迁移 v1→v2：`last_auto_disable` 旧形态归一（mod_id 从故障绑定命名空间派生，不可派生或与现有值不一致即畸形拒绝），解码端宽容反序列化保留为纵深。④ G22.c 跟进项 #1 清偿：v2 起 Mod 检查点必带 `execution_strategy`（硬门按迁移前版本判定，显式 v1 载荷宽容；原生检查点含该键仍拒）。⑤ 跟进项 #2 清偿：mod inspect 报告 v5 顶层输出 `execution_strategy`（停用预览后重算，passed/setup_failed 两路径同源常量），CLI 测试钉死版本与停用敏感。⑥ 钉死键 100→101（`checkpoint_format_version`）并显式断言总数。验收：checkpoint 15 项（v1 迁移夹具往返、版本路由与迁移失败保旧片、迁移层直测不经解码端独立验证、兼容分支四钉、篡改七+一例）、runtime 全量 300+、mod inspect 3 项含 v5 断言、完整 `pnpm run check` 绿。复审：三路后台只读，P0×1——`take` 应为 `skip`（链方向倒置：v1 载荷实未迁移被直接盖章、全绿被解码端宽容掩盖）——已修复并补迁移层直测；P1×4（u32 截断绕过未来版本门、mod inspect 零测试覆盖、checkpoints/state-inventory 导语自相矛盾、笔记验收计数失实）与 P2 若干全部清偿，复审记录入笔记。证据：[checkpoints](../architecture/checkpoints.md)、[状态域清单](../architecture/state-inventory.md)、[G23.a 交付笔记](../research/notes/g23a-checkpoint-migration-2026-10-06.md)。G23 剩余：对象字段/namespace state/包内容的内容级迁移、与 G22.b 的 RNG 迁移联动，另拆子片。
- 上批任务：**G22.c 活动执行策略的持久身份已交付**（2026-10-06，随本批提交，登记提交 `3a8d563`，基线 `5c6aec7`）：把「冻结计划 + 停用掩码 + 组合派生」固化为单一可比对凭据。交付：① kk-mod-api `domain_digest` 域分隔摘要入口（与 PlanHash 同一键序规范化，域前缀防跨身份原像混用）；② `CompositionReport::execution_strategy_digest`（输入钉死 plan_hash + 停用闭包 + 每槽键/阶段/模式/活动链；诊断字段刻意不入）；③ `ModVm::recover` 组合同步从注释假设升级为摘要等值机器核验（失败分片停顿可重试）；④ 检查点新键 `execution_strategy`（钉死清单 99→100），恢复在重装配+停用重放后重算等值，缺键/null 降级为 legacy 语义（其余身份门仍逐项核验，G23.a 决定是否升格硬门），原生检查点含该键即拒；⑤ sim 报告顶层 `execution_strategy`，report_version 4→5（SIM_REPORT_VERSION 常量统一成功/setup_failed 两路径）。验收：mod-host 摘要确定性/停用敏感/诊断不敏感（同安装集合不同选择输入等值）、runtime 兼容分支钉死（删键恢复成功/原生注入拒绝）+ 篡改第七例 + 15 项 checkpoint 测试、sim 冷恢复等值与 64 位形状断言；完整 `pnpm run check` 绿。三路复审 P1×2 + P2 全部清偿（含 research 索引失修修复、local-simulation/checkpoints/mod-composition 文档同步）；mod inspect 输出该身份登记为 G23.a 跟进。契约见 [mod-composition](../architecture/mod-composition.md) 与 [checkpoints](../architecture/checkpoints.md)，证据见 [G22.c 交付记录](../research/notes/g22c-execution-strategy-2026-10-06.md)。
- 上批任务：**G08.a 建筑自定义字段与生命周期 tick 已交付**（2026-10-06，随本批提交，登记提交 `f2a6515`，基线 `75b92d9`）：建筑从纯配方数据升级为「配方 + 自定义字段 + 自主生命周期」，全部经冻结计划进入身份。交付：① 声明面——`BuildingDescriptor` 增 `fields`/`lifecycle_slots`，SDK `buildings.define` 新成员 `fields`（与 objects.define 同形）与 `tick(ctx)`（槽/绑定从建筑键派生，ObjectTick 阶段）；② 存储——字段存 Generic.data，存储键 `{ns}:{name}` 含冒号与原生键结构不相交；完工落 Value 默认（冻结期已按 schema 规范化 backing）、Missing 不落键；③ 写面 `update_building_fields`——原生键整比对只读、proposed 新增无冒号键点名拒、自定义键按字段表整集校验（短名载荷 ↔ 存储键转换）、首触进链日志、decline 升格故障；④ 分派——room_loop `mod-building` 类先产后 tick（tick 读本轮生产后候选）；⑤ 玩家面六层直供——清单四元组带字段存储键表，结构装配器实例级 getter（与内建同回查机制、双通道同源），`mod types` 出 `{构造器}Fields` 接口；⑥ 停用语义定案——tick 随停用掩码停、字段成惰性数据、生产按 plane 规则存续（与资源/仓储/射程同口径）。冻结权威层补五门（复审清偿）：完整 schema（空枚举会把房间钉死永久故障）、重复字段键、槽声明/派生/阶段、字段绑定入 bindings 集、非 public 可见性拒；install 门保留为纵深防御。验收：引擎单测 3 + 集成 3（含 freeze/install 双层门）、运行时 5×双通道（tick 读写/原生键漂移整轮回滚/decline 故障/停用回退/完工默认→玩家读成链）、冻结 4（第二遍门反例 + 身份与默认值规范化）、typegen、sim `sim-mod-building-fields` 四场景（bl:gauge + bk:beacon 两独立包 + 组合 + 无 Mod 对照）×双通道×检查点冷恢复（字段终值随房间快照跨进程存续）；完整 `pnpm run check` 绿。三路后台复审 P1×3 + P2 全部清偿（含 SurfaceBuilding 死代码删除、working.rs/structures_spawn.js 越线按域拆分——后者拆出 tombstones.js 新 prelude 段 18→19）；sdk.js 981/1000 贴线，下批动 SDK 前先拆。契约见 [mod-buildings](../architecture/mod-buildings.md)，证据见 [G08.a 交付记录](../research/notes/g08a-building-fields-2026-10-06.md)。
- 前批参考：**G03.a 整房请求视野与获准集合链预演已交付**（2026-10-06，`0efd645`）：准入槽（`builtin:creep.moveAdmission`）输入扩展 `ctx.args.requests`（整房原始请求矩阵含竞争落选者，逐条 won 标记；原始矩阵在 `rank_winners` 零拷贝暂存）与 `movers[].pulledBy/pulling`（有效拉拽配对 ID）；新增 `ctx.admission.evaluate()` 只读链预演——按候选当前终值在克隆状态上走与正式仲裁**完全相同**的 `cancel_blocked`，返回幸存胜者 ID，实现"暂存候选 + 宿主链验证"闭环；拒绝仍是整房回退原生预填。不动槽集合（17 槽/revision v10/PlanHash 预期不变，验收实证）。验收计划：引擎等价性测试（预演幸存集 == 实际链幸存集：交换/环/链/拉拽/落选/占位拓扑）+ wave/calm 两独立包（wave 用 evaluate 发现级联损失全房停下、calm 用落选者视野扣住竞争胜者）+ 原生对照 + 组合 + 双通道与冷恢复。前置已核对：G01/G02.a/G02.b 五阶段（contest→rank→prefill/admission→obstacle→cancel 链）与执行阶段同源消费齐备；`validate_pull_pair` 胜者升序遍历的顺序语义留待 pull 登记开放子片重审。详见下文 G03.a 段落。G06.a 运行 trace 前批已交付（`ce84345`，回填 `a8a751e`）。
  - G29.a 玩家类型生成已交付（2026-10-06，`73b0f1b`）：`kkscreeps mod types` 冻结计划确定性生成玩家面 TypeScript 声明 + `--check` 字节级新鲜度门禁 + 输出/报告双守卫（包外、先于建目录）；TS 标识符安全（引号键/枚举转义/参数类型名消歧）、可执行性门与引擎装配共用 `intent_gate`。契约见 [mod-types](../architecture/mod-types.md)、证据见[交付记录](../research/notes/g29a-mod-types-2026-10-06.md)。
  - G02 剩余不变：意图静态预检 `has_static_obstacle` 开放（后续子片或 G07 预检族）、摘要附带安全模式主人（可解释性增强）；G29.b/.c 依赖 G28.a。
- 最近维护工作：2026-10-06 作者工具与恢复边界修复复审已完成，完整门禁和逐用例兼容对照通过；同日用户拍板两项门禁调整——完整 check 移除 doctest 阶段（rustdoc 全量重编占热缓存测试约 3/4；约定：改动或新增 doctest 时单独 `cargo test -p <crate> --doc`，CI 仍全量跑含 doctest），随后 check/check:fast 合并为单入口 `pnpm run check`（无参数=完整门禁，`-- <crates>` 限定子集），`check:fast` 与 `scripts/check-fast.mjs` 删除。本批没有新增已实现的 G 功能。
- 未解决的产品决定：本轮路线复审没有需要用户补充的决定；具体公开接口、权限与耐久协议尚未冻结，须在对应子片设计和验收。
- 验证参考：使用上述验收记录及其机器数据，不复制其测试数量作为永久门槛。最新兼容性报告和门禁配置位于 `compat/reports/`。

目前已交付自主对象动作/tick/同房增删改、组合诊断、有效规则、内容选择、命名资源、建筑配方/生产、建筑自定义字段与生命周期 tick、伤害/治疗/反击与 JS 受击策略、基础完整检查点与执行策略身份。以下边界仍在：移动动作可改向/拒绝但仲裁是 Rust；建筑字段可见性只支持 public 且无玩家面写路径；反击表不能自动按实际扣血反弹；检查点不含迁移/热更新/回放；正式网络服务、客户端和可替换存储后端未实现。接手时从[架构索引](../architecture/README.md)核实这些边界，不能只相信本段日期记录。

## 执行与更新规则

1. 接手或恢复上下文后，阅读本文件、完整玩法计划、相关 architecture 和 AGENTS.md，再核对代码、HEAD、未提交修改及最新证据。保留已有工作，不依据旧状态重复实现。
2. 开始一片时更新当前接手位置、表中状态及实施记录：写清范围、前置条件、计划验收和正在修改的模块。按下文执行队列穿插 G05/G06/G29.a 和恢复前置片，不按父编号机械串行；依赖变化须记录理由。
3. 状态使用“未开始 / 进行中 / 部分完成 / 待复审 / 已完成 / 等待决定”。“已完成”必须有相应交付、适用测试和复审证据；开放玩法能力需独立 JS 示例，工具/协议/审计按下文验收类别提供证据；只完成首个切片时标“部分完成”，列出未接线消费者。
4. 每片同步相关 architecture、SDK 使用说明、样例及 research 交付证据；达到里程碑时同步 roadmap 和完整玩法计划。修改文档入口时更新索引。历史 research 记录保留当时结论，本台账当前状态和追加记录持续维护。
5. 每次结束开发会话、转交其他 AI 或达到阶段停点前，更新本文件的当前任务、已完成/剩余事项、实际验证、工作区情况、已知问题及下一步。不要只在聊天里交代。不要把未执行的检查写成通过。
6. 每批先 code review 并修复问题，再按仓库门禁提交；没有通过检查不能标为完成。已提交时登记提交 ID；若实现记录与实现同次提交，可先写“随本批提交”，下一次接手从 Git 补录真实 ID，无须为获得本提交自身 ID 制造循环提交。
7. 拆片时用 `Gxx.a/Gxx.b` 子编号并追加记录，保留父任务范围和未完成项；调整顺序不删除任务，取消或替代须保留原因及替代位置。不要因聊天压缩遗失长期目标或默认兼容性门禁。

## 任务台账

“依赖/前置”表示实施前需核验的能力，不是已冻结的 API 或调度顺序。领域预检随本领域首次开放一起接通，不能以 G07 未完成为由留下隐藏 native 否决。

| 编号 | 任务 | 依赖/前置 | 状态 | 交付证据 |
|---|---|---|---|---|
| G01 | 移动竞争优先级策略 | 现有 move 请求与仲裁 | 已完成 | [mod-move-contest](../architecture/mod-move-contest.md)、[交付记录](../research/notes/g01-move-contest-2026-10-06.md)（G02/G03/G04 均已分片开放） |
| G02 | 移动准入策略 | 已稳定的移动请求输入；同步必要玩家预检，G01 不作硬依赖 | 已完成 | G02.a 准入槽（b7e65d5）、G02.b 障碍槽（e032991）、G02.c 预检槽（`0f390b5`）：[mod-move-admission](../architecture/mod-move-admission.md)、[mod-move-obstacle](../architecture/mod-move-obstacle.md)、[mod-move-precheck](../architecture/mod-move-precheck.md)（放行方向按宿主约束/语义惰性关闭；孵化门槛归 G13；安全模式主人摘要为可解释性增强另登记） |
| G03 | 完整房间移动仲裁 | G01、G02 | 部分完成 | G03.a 整房请求视野 + evaluate 链预演（`0efd645`）：[mod-move-admission](../architecture/mod-move-admission.md)、[记录](../research/notes/g03a-move-arbitration-2026-10-06.md)；pull 登记已开放（`movePullRegistration` 槽 + add_pulling 双射修复，见当前接手位置）；障碍槽预演/放行方向未开放 |
| G04 | 移动代价与执行效果 | G02；代价可先行，完整仲裁交互在 G03 后验收 | 部分完成 | G04.a 代价槽已交付（`builtin:room.moveCost` + add_fatigue u32 钳制，见当前接手位置）；疲劳恢复/重量评分要素未开放 |
| G05 | Mod 作者基础 CLI | 现有 sim、inspect、检查点 | 已完成 | G05.a `mod init` `ee296cb`；G05.b `mod test` `dbc82a4`（[模板与测试说明](../architecture/local-simulation.md#作者包模板)）；后续增强另登记 |
| G06 | 运行 trace 与故障复现 | 现有活动链与故障诊断 | 部分完成 | G06.a 槽调用/回退事件 trace + `--mod-trace` CLI `ce84345`：[mod-trace](../architecture/mod-trace.md)、[记录](../research/notes/g06a-mod-trace-2026-10-06.md)；过滤参数/停用阈值暴露/一键复现未做 |
| G07 | 玩家预检策略扩展 | 各领域策略契约；可分族穿插 | 部分完成 | G07.a 战斗射程首片（见当前接手位置）：[player-precheck-rules](../architecture/player-precheck-rules.md)、[交付记录](../research/notes/g07a-combat-ranges-2026-10-06.md)；EXECUTION_ORDER/动作互斥/参数策略/可扩展意图盘点未开始 |
| G08 | Mod 建筑字段与生命周期 | 现有建筑目录、schema、tick | 部分完成 | G08.a 字段+tick 首片已交付（见上批任务）；RCL 配额/逐资源容量/专属 FIND/生产事件未开始 |
| G09 | 自有对象库存与耐久能力 | 现有对象/资源/伤害能力 | 未开始 | — |
| G10 | 可编程生产与建筑动作 | G08；现有资源与配方 | 未开始 | — |
| G11 | 建筑配额与资源容量策略 | 现有建筑/资源目录 | 未开始 | — |
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
| G22 | 执行身份、玩法时间与随机协议 | .a 状态清单先行；.b 在随机事件前；.c 在更新/回放前 | 部分完成 | G22.a 已交付：状态域清单+检查点 schema 键树门禁；G22.c 已交付：执行策略身份摘要+恢复自检+检查点键（见上批任务）；.b 未开始 |
| G23 | 包/schema/state 迁移 | G22.a/c；RNG 迁移另依赖 G22.b | 部分完成 | G23.a 已交付（`97f4ed9`）：载荷格式版本与解码前显式迁移链（v1→v2 收编 last_auto_disable 旧形态、v2 起执行策略硬门）；内容级迁移与 RNG 联动未开始 |
| G24 | tick 边界更新与 watch | G23；玩家工作者/API 原子切换 | 部分完成 | G24.a 已交付（`1ee4fc9`）：无开放轮次的包/API 切换（替代分片换装、残留硬门、sim mods_switch 与冷恢复闭环）；watch 未开始 |
| G25 | 持久输入日志与结算回放 | G22；.a 日志协议与保存，再 .b 重放校验 | 未开始 | — |
| G26 | 自动检查点与 JS storage provider | .a 基于现有检查点；.b 依赖 G27.a；回放恢复接 G25 | 未开始 | — |
| G27 | 提交后输出与后台服务 | .a 最小后台宿主；.b 依赖 G25.a/G26.a，按需接 G20 | 未开始 | — |
| G28 | Backend 贡献入口 | .a 实现 M3 网络基础；.b 依赖 G27 与 .a | 未开始 | — |
| G29 | 玩家类型与客户端贡献 | .a 类型生成前置；.b 客户端基础依赖 G28.a；.c 依赖 .b | 部分完成 | G29.a `mod types` 声明生成与 `--check` 门禁 `73b0f1b`：[mod-types](../architecture/mod-types.md)、[记录](../research/notes/g29a-mod-types-2026-10-06.md)；.b/.c 未开始 |
| G30 | 官方预设、覆盖审计与性能验收 | .a/.d 先建立再滚动；.b/.c 随领域交付，存量变更依赖 G23 | 未开始 | — |

## 实际执行队列与前置门

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

## 近期：移动与作者工具

### G01 移动竞争优先级策略

开放房间级竞争的只读输入及可组合优先级候选，让 JS 改变竞争者排序。原生默认排序与平手登记顺序保持兼容。至少两种独立策略及原生对照，覆盖抢格、多处理器组合、正常拒绝默认回退、故障与缓存重试。这只是排序切片，不代表完整仲裁已开放。当前 `Movement::check_impl` 使用完整原始矩阵评分，先给每个目的格选一个胜者，再取消不可移动/阻塞者，**不自动递补落选者**；G01 默认路径不能改为先过滤资格再选胜者。实际执行阶段还会复核 `can_move`，G02 必须同时处理这处消费者。G01 先通过有界同步调用生成数值/排序候选，再稳定排序，不在 Rust 排序比较器中回入 realm 执行有副作用回调；明确平手、非法值、来源完整性和 before/native/replace 的关系。不得静默截断房间请求。入口先核查[默认仲裁](../../crates/kk-engine/src/settle/movement.rs)、[移动执行复核](../../crates/kk-engine/src/settle/creep/movement.rs)、`room_loop.rs` 和[移动动作槽](../architecture/creep-move-slot.md)。

### G02 移动准入策略

盘点疲劳、MOVE、地形/结构障碍等默认限制，开放具名可替换入口；玩家预检、请求登记、仲裁和实际执行同源消费，不得 JS 放行后又被隐藏 native 检查否决。未开放的检查逐项登记。数值、坐标和索引一致性属于宿主约束，准入规则属于玩法。

**G02.b 仲裁障碍单向追加阻挡槽（2026-10-06 已交付）**：新增房间级槽 `builtin:room.moveObstacle`（内建槽 16→17、revision v10）：准入槽之后、取消链之前整房一次进 JS；输入含全部胜者（准入被拒者仍在列）的目的格对象摘要（creep 让位=胜者∧准入终值、rampart 公开性、工地 structureType、墙/路）与预填时点 native 初判；`ctx.obstacle.block(id)` 单向追加 + `isBlocked` 读初判，decline 丢弃追加。取消链 obstacle = 追加集 ∥ 链内 `check_obstacle_at_xy`，让位链增量传播（挡 b → b 取消 → a 失让位随链取消）由引擎完成；放行方向不开放（表示有效性宿主约束）。stay-north（replace 区域管制）与 no-swap（after 禁止依赖让位）两独立包 + 原生 + 组合并集验收，双通道冷恢复等价；默认路径零新增可测开销。意图静态预检 `has_static_obstacle`（intent_move/slot_request_direction）与摘要安全模式主人登记为剩余。

**G02.c 意图静态预检槽（2026-10-06 已交付，`0f390b5`）**：新增房间级槽 `builtin:room.movePrecheck`（内建槽 17→18、revision v11、PlanHash 变更；catalog 断言同步）：全部意图与移动动作槽登记完成后、竞争输入构造之前整房一次进 JS；输入为整房请求摘要（moverId/owner/origin/target/movePower/weight/pulled/pulling + 目的格 terrainWall/terrainSwamp/hasRoad/objects[]，按对象 ID 升序）；`ctx.precheck.isBlocked/block` 单向收紧，被否决请求经 `remove_requests` 在竞争前移除，**等同从未登记**（不参与竞争评分、不让位、不占格；old_fatigue 残项仅胜者可读、pull 配对保留与基线逐步一致——均有引擎基线对照测试）。放行方向不开放（语义惰性：仲裁期 native 障碍一律复检）；两登记入口（玩家意图/移动槽能力）共享矩阵统一否决，同源有运行时可观测证据。请求级 vs 胜者级语义（否决强竞争者请求 → 弱者独占目的格）与障碍槽互补、可同房组合（有测试）。验收：引擎 4（输入完整性/请求级语义+准入视野不含/拉者否决基线/候选域）+ 运行时 8（字段形态+原生对照、decline 回退含 before 层暂存 state 丢弃、组合 isBlocked、移动槽通道同源、预检×障碍两级组合、能力门、故障停用回退+晚期重试恰一次、无请求不进 JS）+ fenced/courtesy 两独立包四场景 × JSON/KKPV + 检查点冷恢复 + 四 plan_hash 身份。movement.rs 超限按职责拆出 `movement/inputs.rs`（槽输入投影子模块）。三路复审无高危，14 项发现全部清偿（staged_states 缺失、gitignore 白名单、槽计数史实、恒真断言等），详见[交付记录](../research/notes/g02c-move-precheck-2026-10-06.md)。G02 父任务至此收口：放行方向按表示有效性/语义惰性关闭、孵化门槛归 G13、障碍摘要安全模式主人为可解释性增强另登记。**下一步按队列：G22.a 状态清单**；G03 剩余（pull 登记策略、障碍槽预演）与 G04 按依赖另拆。

**G02.a 疲劳/MOVE/有效被拉门槛准入槽（2026-10-06 已交付）**：新增房间级槽 `builtin:creep.moveAdmission`，仲裁拆为 排名 → 原生预填（配对复核 + can_move 终值表）→ JS 覆盖（`ctx.admission.getAdmitted/setAdmitted`，初值即原生结果）→ 取消链四阶段；执行阶段 `movement_execute` 改读准入终值（同源、fail-closed）。配对复核遍历顺序改为胜者对象 ID 升序（集合等价；同拉者多目标残标在现有意图流不可达，G03/G04 开放 pull 登记时须重审）。tireless（replace 全放行）与 strict（after 收紧配重不足）两独立包 + 原生对照 + 组合（after 收回 replace 放行）验收，双通道冷恢复等价；默认路径输入构造零付费，微基准 1.84–1.95µs/房对基线 1.61–1.75（密集 32 移动者场景准入终值表维护 +0.2–0.3µs）。内建槽 16、revision v9、PlanHash 变更；旧检查点按身份拒绝。剩余：G02.b 障碍准入（`has_static_obstacle` 意图预检、`check_obstacle_at_xy` 仲裁障碍与让位链耦合）、孵化中拒绝、越界/坐标（宿主约束）。

### G03 完整房间移动仲裁

允许 JS 选择获准移动集合，覆盖让位链、交换、循环、拉拽和阻塞传播。宿主验证表示与索引，不重新运行默认算法覆盖结果。明确整条房间链的数值/集合候选、暂存 state、正常拒绝与 native 回退，不复用单执行者拒绝边界冒充房间恢复。

**G03.a 整房请求视野与获准集合链预演（2026-10-06 已交付，`0efd645`）**：准入槽输入扩展 + 只读链预演，不动槽集合（17 槽、revision v10、PlanHash 不变——基线对照构建实测一致 `72d231e2…`）。交付：① `ctx.args.requests` 整房原始请求视野——全部登记（含竞争落选者）按 目的格→格内登记序 排列，逐条 `moverId/x/y/index/originX/originY/won`；原始矩阵在 `rank_winners` 零拷贝暂存（Working 每房每 tick 重建，天然瞬态）。② `movers[].pulledBy`（有效配对拉者 ID，与 `pulled` 同点复核）与 `pulling`（拉者侧登记态，目标落选本轮不生效）。③ `ctx.admission.evaluate()` 暂存候选链预演——按候选**当前**终值在瘦克隆（只复制链消费字段）上运行与正式仲裁完全相同的 `cancel_blocked`，返回幸存胜者 ID 升序数组；只读、即时计算非快照；障碍槽追加集此时必为空（槽序固定 + 预演能力随准入槽令牌过期双重保障），预演结果是最终结果的上界。拒绝仍整房回退原生预填（非单执行者边界）；`Working::active_safe_mode_user` 抽取后障碍输入/预演/正式链三处同源。验收：引擎等价性 5 函数 11 拓扑案（交换/环/扣环员级联/链式/占位/覆盖/拉拽双案/安全模式分支/快照不消费）+ 运行时 3 测试（覆盖→预演端到端闭环含 requests 字段形态、evaluate 在障碍/竞争/动作槽走能力门不可被 decline 掩盖）+ wave（replace，预演发现级联损失全房停）/calm（after，落选者视野扣同格胜者）两独立包 + 原生对照 + 组合并集，JSON/KKPV 双通道、四 plan_hash 身份互异、准入后检查点冷恢复等价（CLI 2 测试）。三路复审无高危（正确性/一致性路零中级），14 项发现全部清偿（含夹具 gitignore 强制入库、克隆瘦身、安全模式等价护栏），详见[交付记录](../research/notes/g03a-move-arbitration-2026-10-06.md)。剩余另拆：pull 登记策略开放（须重审 `validate_pull_pair` 胜者 ID 升序遍历的同拉者多目标顺序语义）、障碍槽预演/放行方向（表示有效性宿主约束）、G04 代价执行。

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
- 源码/测试不超过 1000 行，按职责拆分；更新现行架构、SDK 用法、夹具、机器证据和本台账后复审。
- 每片首次设计记录输入范围/大小、单位与整数/浮点域、完整预检、权限及 state/世界写面；变更持久域同时更新清单与版本策略。native 中心、before/replace 与拒绝的关系逐槽定义，生命周期不自动支持动作式局部拒绝。
- 修改 Rust 后重新构建测试 CLI、显式指定 `KKSCREEPS_BIN`；提交前运行完整 `pnpm run check`。兼容相关改动按 [compat/README.md](../../compat/README.md) 新构建 release、核验固定依赖、构建适配器并运行 ratchet，另外比较本批实际基线的逐用例状态，不能只比较总通过数。

## 实施记录与交接模板

此处按批次追加实际记录；未开始的任务不填虚构产物。重大拆片和顺序变化在对应记录注明。

### 台账建立（2026-10-06）

- 范围：将已确认的 30 项后续任务写入本文件，在 AGENTS.md 增加主动更新要求，并连接文档/计划索引、完整玩法计划和路线图。没有实施 G01–G30 的功能。
- 复审：核对 30 个表项与详细任务编号一一对应；修正计划索引中过时的“下一片为自有对象 tick”；明确持续台账与历史证据的维护区别、部分完成状态和提交 ID 补录方式。
- 验证：文档链接检查、`git diff --check` 与完整 `pnpm run check` 通过；检查显式使用现有功能基线对应的 debug CLI。Rust 940 通过、12 个既有忽略，JS 19 通过，Clippy/格式/源码规模门禁通过。本次只有文档修改，没有重跑 release 兼容性棘轮。
- 提交：`784ffef`。
- 剩余事项：G01–G30 尚未开始，下一步 G01；本次没有未解决的复审问题或待用户决定事项。

### 路线复审（2026-10-06）

- 基线：`784ffef`；只修订规划、执行前置和贡献指南，没有实施 G 功能。
- 检查依据：实际移动仲裁及 tick 中的 `can_move` 复核、当前检查点身份/清单、storage/backend/client 占位源码、ADR-016；抽查本地 xxscreeps `5ff0d1a6` 的 loader 与 classic/portal/intershardResource manifest，不宣称已复审其全部消费者。
- 已修正：后台/provider/outbox 的模糊依赖与崩溃投递边界；新写入域的当批保存/版本要求；作者 CLI/类型、覆盖审计和性能参考过晚；遗漏的 M3 基础、preTick/清洗器/现代玩法责任；builtin 内容提取与回退区别；移动默认选胜与末端复核；超大父任务拆片及按能力类别验收。
- 结构验证：30 个父编号与详细条目对应；16 个关键子片硬依赖无未知引用和循环；xxscreeps loader 的 11 个领域都有任务责任映射。这里只验证声明的局部图，远期首次拆片仍须复审实际接口/消费者与依赖。
- 仓库验证：文档链接、暂存差异检查与完整 `pnpm run check` 通过；Rust 940 通过、12 个既有忽略（56 组），JS 19 通过，Clippy/格式/源码规模门禁通过。显式使用功能基线对应的 debug CLI；只有文档修改，没有重跑 release 兼容性棘轮。
- 提交：`c84eac5`。
- 下一步：G01 前登记移动覆盖清单与默认参考，随后穿插 G05/G06/G29.a；G 功能状态仍为未开始。剩余工作是按片设计具体接口和全量消费者验收，不是待用户决定事项。

### 开发命令可移植性整理（2026-10-06）

- 基线：`c84eac5`；只整理文档与实验复跑入口，没有实施 G 功能。
- 范围：历史记录、机器证据中的命令统一为直接调用 cargo/python3，脚本说明与示例注释同步；保留当时的日期、实验结果和结论。玩家工作量采集脚本直接执行目标二进制，其测试直接调用当前 Python 解释器，不依赖额外命令包装器。
- 复审：仓库受版本控制的文件已无可选包装工具引用；4 个 Python 文件语法通过，历史 JSON 仅命令展示变化，结果字段完整保留。
- 验证：`python3 -m unittest discover -s scripts/tests -p 'test_mod_player_capture.py' -v` 的 5 项回归通过；文档链接、`git diff --check` 与完整 `pnpm run check` 通过。显式使用功能基线对应的 debug CLI；Rust 940 通过、12 个既有忽略，JS 19 通过，Clippy/格式/源码规模门禁通过。没有重新采集历史性能数据或重跑 release 兼容性棘轮。
- 提交：随本批提交，后续按 Git 补录 ID。
- 下一步：仍按 G01 的覆盖清单与默认参考前置启动；G 功能状态未改变。

```text
批次 / 日期：
状态 / 基线提交：
本次实现范围与关键契约：
已完成事项（代码/架构/样例链接）：
剩余消费者与限制：
验证（实际命令、构建身份、结果、机器证据链接）：
复审发现与处理：
提交 ID / 未提交文件及用途：
已知问题 / 需要用户决定的具体事项：
下一步与依赖：
```

### G01 移动竞争优先级策略（2026-10-06）

- 基线：`8e28ac4` 功能基线之上实施；启动前已采集 G30.d 默认仲裁微基准（release 8 竞争格 ×4 竞争者 ≈1.6 µs/房），改造后同负载 ≈1.6–2.4 µs（噪音内，无默认路径回归）。
- 本次实现范围与关键契约：新增房间级槽 `builtin:room.moveContest`（内建槽 14→15，builtin revision v8）。整房一次提供全部竞争格（≥2 竞争者目的格）与按登记序的竞争者摘要（含默认四键评分要素，不截断）；`ctx.contest.getPriority/setPriority` 优先级数值候选（±2^53−1 JS 安全整数、id 白名单限本次输入），排序键 优先级 → 默认四键 → 登记序，未设置为 0 保证原生默认与平手登记序兼容；正常拒绝丢弃优先级与暂存 state 回退默认排序；native 中心无操作、停用回退、晚期写回失败缓存重试与受击槽同构。仲裁结果为 tick 瞬态，不新增持久写入域；新增冻结描述改变 PlanHash（旧检查点明确拒绝）。
- 已完成事项：mod-api 输入与槽描述；引擎 `Movement::check_impl` 优先级接线、`contest_input`/候选 `MoveContest`/`arbitrate_room` 编排、装配与 install 清单；mod-host ChainHost/op/sdk.js `ctx.contest`；runtime `SlotCall::Contest` 与能力实现。独立包 `compat/fixtures/mod-move-policy/{seniority,heaviest}`，场景 `sim-mod-move-policy` 四件套（plain/seniority/heaviest/combined，三种策略三种胜者）。契约文档[JS 移动竞争](../architecture/mod-move-contest.md)、G30.a 移动领域覆盖清单与 G30.d 基准见[交付记录](../research/notes/g01-move-contest-2026-10-06.md)。
- 剩余消费者与限制：G02 移动准入（含执行阶段 `can_move` 复核同源）、G03 获准集合/让位链/整房请求视野、G04 代价与执行效果、pull 策略未开放；xxscreeps 随机平手语义未提供；JS 策略端到端开销未单独量化（VM 预算管控，G30.d 滚动补测）。
- 验证：`cargo test -p kk-engine`（新增 contest 单测 6 项含对穿/拉着走 + 竞争排序集成测试）；`kk-runtime mod_chain_tests::move_contest` 7 用例（输入完整性、重排、拒绝回退、非法值/越权不可掩盖、能力门、负优先级、停用回退、缓存重试恰一次）；`kk-cli sim_tests::move_policy`（四场景 × JSON/KKPV 全等、PlanHash 四身份、竞争后保存/冷启动续跑等价、applied 恰一次）；改造前后微基准对照。完整 `pnpm run check` 通过（Rust 955 通过/12 既有忽略、JS/格式/Clippy/链接/规模门禁全绿）；兼容性棘轮 release 重跑 1918/2914 通过、零转红（两条历史豁免未命中警告与本批无关）。
- 复审发现与处理：三路只读 subagent 评审（仲裁正确性/恢复路径、能力面/数值边界、模块规约/文档一致性）无中/阻断级语义问题；已清偿：冻结目录测试槽计数 14→15（阻断 workspace 测试）、基准补 `#[ignore]` 并统一手动命令、clippy unnecessary_cast、对穿/拉着走两个正向契约用例与 e2e 字段断言补齐、`MoveContest::new` 不可解析 id 改构造期故障、竞争槽 `write_fields` 提前拒绝、decline 文案补受击/竞争、`ctx.target` 文档措辞与非 creep 防御值说明、记录移至时间序末尾。明细见[交付记录](../research/notes/g01-move-contest-2026-10-06.md)。
- 提交 ID：`9c8687d`（含全部代码、夹具、文档与本台账）。
- 下一步与依赖：G01 已完成；随后按执行队列推进 G05.a 作者 CLI init 或 G02 准入策略首片（G02 须同源处理仲裁与执行两处准入消费者）。

### G05.a Mod 作者 CLI `mod init`（2026-10-06）

- 基线：`9490a9c` 之上实施；G05.b `mod test` 未开始，父任务保持部分完成。
- 本次实现范围与关键契约：`kkscreeps mod init --root <目录> --id <包ID>` 生成最小公共 SDK 包模板（落在 `<root>/<id>/`）：manifest、声明每 tick 自增计数对象的 server 处理器（`ctx.target.fields`/`ctx.state`）、绑定玩家类并安装 FIND/LOOK 查询的玩家面、玩家脚本、引用本包并带断言的场景、README（组合检查/运行/保存恢复命令与复制派生提示）。ID 借探针键复用 Key 命名空间校验；目标已存在拒绝、不覆盖；玩家类与常量从 ID 派生，同根多包不冲突。
- 验收：`kk-cli tests/mod_init_tests.rs` 3 用例经真实二进制驱动——模板文件齐全、sim 3 轮断言直接通过（value=3、玩家读上轮值 count=2、state.ticks=3）、1+2 检查点续跑等价（plan_hash 与对象终值一致）、同根双包（`alpha-pkg`/`beta_pkg`）注册无冲突且两场景独立通过、非法 ID（大写/前导数字/下划线/冒号/空格/路径分隔符/`..`）与既有目录全部拒绝且不落文件；`mod inspect` 对生成包通过。模板命令在 `--root` 目录下按 README 字面可执行。
- 复审发现与处理：两路 subagent（正确性/可运行性、规约/文档）中级发现 2 项已清偿——README 命令的工作目录指引改为"在 --root 目录执行 + 真实二进制路径"；玩家全局名改为从 ID 派生（原固定 `ExampleCounter` 在同根双包时注册冲突，实测复现）。低级清偿：控制台提示区分 report/checkpoint 路径、错误文案补首字符规则、路径分隔符守卫前置、死代码删除、测试补前导数字/冒号/双包/非空断言并改名、文档指向 mod-object-tick、索引行补模板节。
- 提交 ID：`ee296cb`。
- 下一步与依赖：G05.b `mod test` 最小闭环（模板自带断言场景 + CLI 一键运行）；或按队列 G02 移动准入首片 / G29.a 类型生成。

### G05.b Mod 作者 CLI `mod test`（2026-10-06）

- 基线：`ee296cb` 之上实施；G05 就此收口为已完成（后续增强另行登记）。
- 本次实现范围与关键契约：`kkscreeps mod test --root <目录> --report <汇总.json>`（`--ticks` 默认 3、`--kkpv` 透传）对根下自带 `scenario.json` 的包按同一 `sim run` 路径运行断言并汇总：场景按包真实目录（`package.root`）定位，目录改名不改变测试对象；无场景包记 skipped；退出码取各场景最大值（0/1/2 沿用 sim）；装载期失败也落盘 `setup_failed` 汇总；`--report` 强制 `.json`、`--ticks` 至少 1。汇总内联各场景机器结果（plan_hash、断言状态、channel）。
- 验收：`kk-cli tests/mod_test_tests.rs` 4 用例真实二进制驱动——双模板通过+无场景包 skipped+篡改断言传播退出码 2、改名目录包仍被测试（不静默 skip）、损坏场景 JSON 记 setup_failed 传播退出码 1、根缺失/空根退出 1。完整 `pnpm run check` 全绿。
- 复审发现与处理：两路 subagent 评审高级发现 1 项已清偿——包键（manifest id）被误作目录名定位场景，改名包静默跳过成假阴性（实测复现），改为按 `package.root` 定位并补改名目录回归测试。其余清偿：setup 失败落盘汇总、`.json` 后缀校验、`--ticks` range 解析器、退出码聚合语义与选项补文档、ModCommand 变体字母序、skipped 打印形态、索引行同步。
- 提交 ID：`dbc82a4`。
- 下一步与依赖：G02 移动准入首片或 G29.a 类型生成；G06.a 链诊断可穿插。

## 可直接使用的接手提示词

```text
请接手本项目 Mod 开发。先读 AGENTS.md、docs/plans/mod-next-batches.md 和 docs/plans/mod-complete-gameplay.md，核对实际 HEAD、工作区和相关架构/验收证据；以任务台账的当前接手位置选择工作，不重复已完成能力。
请实际开发，按台账执行队列及前置门完成可独立验收的小批次；提供代码、按能力类别要求的独立 JS 示例/故障证据、适用测试和文档，code review 修复问题后按仓库要求提交，再推进下一项；不要合成巨型提交或把首片称作全领域完成。
开始任务、范围变化、完成批次和结束会话前，主动更新 mod-next-batches.md 的状态、接手位置和实施记录；同步现行架构、SDK 用法、样例与机器证据，达到里程碑同步路线图。聊天中的进度不能替代仓库记录。
长期目标是独立 JS Mod 创造完整玩法、覆盖 xxscreeps 扩展领域，提供更完善的组合、恢复和工具；默认兼容性、原子候选提交与恢复边界必须保留。不要自动 push 或发布。
```

### 作者工具与恢复边界修复复审（2026-10-06）

- 修复基线：`c52cdd3`；复审范围为 `b3aa9cb` 后新增的移动策略、作者 CLI 与运行 trace。先清偿上一轮 7 项确认问题，再处理深入复审确认的旧自动停用记录解码、sim 失败报告覆盖输入及恢复 trace 时间问题；不新增 G 功能。
- 实现：只读共用输出守卫、原子声明替换、模块级类型名分配、递归数组声明、无损模板 ID 转义、原子模板目录与私有场景报告目录、trace 开启意愿及恢复完成后启用、旧来源字段恢复。对象/建筑同键的假设经冻结门核实为不合法，无需新增生成策略。
- 验证：真实 tsc、作者工具、checkpoint 及场景定向回归通过；fresh release 兼容门禁通过，2914 用例状态与本批实际基线逐项一致（1918 通过、89 既有失败、907 跳过）。最终完整 pnpm run check 通过：Rust 1037 通过、13 个既有忽略；JavaScript 19 通过；305 篇 Markdown、1819 个本地链接和 507 个源码/测试文件的规模门禁通过；[机器证据](../research/notes/mod-tools-review-fixes-2026-10-06.json)。
- 代码、当前架构与模板指引已同步；代码提交 `100a859`；本次回填交付身份与最终验证，工作区无其他开发中的改动。完整问题表与复审范围见[修复记录](../research/notes/mod-tools-review-fixes-2026-10-06.md)。原开发队列不变：G02 剩余静态预检、G22.a 状态清单；G03 剩余拉拽登记与障碍预演分片。
