# G07.a 交付笔记：玩家预检规则·战斗射程首片（2026-10-06）

登记基线 `1422203`。目标：单目标战斗射程从双端硬编码改为**规则驱动双端同源**——一条 `builtin:creep.*.range` 规则同时决定 Rust 结算距离与玩家 API 预检常量，两端装配期取同一冻结计划快照。

## 交付内容

1. **常量表与 kk-core 同源**：`constants.json` 新增 `ATTACK_RANGE`(1)/`RANGED_ATTACK_RANGE`(3)/`HEAL_RANGE`(1)/`RANGED_HEAL_RANGE`(3)（423→427 键）。复审更正来源表述：**四键为本仓库本地新增**——上游 `common/lib/constants.js` 不含这四个名字（grep 0 命中），官方文档只以行文描述射程，未提供同名全局常量；重新 dump 上游会丢掉四键，该回归由三道门拦住——kk-core 的 dump 键数门（427=423+4，`constants_match_official_dump`）、freeze catalog 测试逐键比对（缺键即红）与 `builtin.rs` 的 `c::ATTACK_RANGE` 等编译期引用（kk-core 缺常量即编译失败）。键数门在完整 `pnpm run check` 才触发（本批 check:fast 未含 kk-core 而漏过，已在清偿段记录）。`scripts/gen-constants.mjs` 重生成 `kk-core/src/constants.rs`。
2. **规则与快照**：`builtin.rs` 声明四条规则（值域 0..=50，consumers 为四个战斗 native 绑定）+ `CombatRanges::from_plan`（LinkRules 同形）；revision v13→v14（槽集合不变 20，规则 25→29）。引擎：`Engine.combat` + `combat_ranges()` 访问器 + install 两条装配路径 + `Plane.combat`（engine_tick 生产构造 1 处 + 11 个测试文件共 15 处 `Plane` 字面量补字段）；`kk-world::place::builtin_combat_ranges()` 为无计划默认。
3. **结算消费**：`combat.rs` 三处改读 `w.plane.combat`——attack（原 `>1`）、rangedAttack（原 `>3`）、heal/rangedHeal（原 `3/1`）；距离比较升 i64（规则 i64 域）。rangedMassAttack 面积扫描（`<=3` 与 1/0.4/0.1 档）保持原生，不随单目标规则变化。
4. **玩家面**：prelude `creep.js` C 表捕获四常量，attack/heal 由 `isNearTo` 改 `inRangeTo(·, C.*)`（默认行为逐位等价），rangedAttack/rangedHeal 的字面量 3 改 `C.*`；`setup.rs` canary（预检沙箱）与真实玩家两条通道按 `CombatRanges::from_plan`/`engine.combat_ranges()` 注入四键（LINK_CAPACITY 先例；未知键 fail-fast 通道复用）。

## 验收（全部实际执行）

- **引擎**（`settle_tests/combat_ranges.rs`，4 测试）：attack 覆盖 1→3 距离 2 命中/默认拒；rangedAttack 零程邻格拒（默认臂对照）；heal.range 与 rangedHeal.range 独立覆盖（heal=3 时距离 4 远程仍按默认 3 拒，rangedHeal=5 后 +8 落地）；mass 在 rangedAttack.range=1 下距离 2 仍按 0.4 档命中。
- **runner**（`sandbox_tests` `combat_range_constants_impl`）：默认 `[1,3,1,3]`；四键覆盖平铺后 attack 距离 2/rangedAttack 距离 4/heal 距离 2/rangedHeal 同轮全放行（默认臂 -9 对照）。`cargo test -p kk-runner` 17 全过。
- **运行时**（`mod_chain_tests/combat_ranges.rs`，4 测试 × 双通道）：attack 覆盖端到端（常量平铺 3、门 0、结算 70、EVENT_ATTACK damage 30）；治疗族端到端（先近战预打伤 30，heal 距离 2 放行 +12 落地、rangedHeal 距离 4 放行 +4 落地，EVENT_HEAL 断言）；mass 排除；越界覆盖（51/-1）冻结即拒。`cargo test -p kk-runtime` 298 全过。
- **sim**（`sim_tests/combat_ranges.rs`，2 测试）：reach（attack 1→3）/medic（heal 1→3、rangedHeal 3→5）两独立包 + 原生 + 组合四场景，门返回码/常量注入串/3 轮结算 foe 血量/终态存活全部按表断言；四场景 × JSON/KKPV 双通道 final 等价、4 个 plan_hash 互异；第 2 tick 保存检查点（退出码 2）→ 加载续跑 final 与直跑等价。98.15s。
- **mod-api**：freeze 44 过（catalog 断言规则 29、CombatRanges 默认快照、四规则与常量 dump 逐键锁定）。
- 完整 `pnpm run check` 见提交记录。

## 复审记录

三路后台只读评审（引擎/规则正确性、玩家面管道、夹具/sim/文档），无 P0/P1，共 9 条 P2，全部清偿：

- R1-P2-1 常量来源表述不实（见交付内容第 1 条更正）；R1-P2-2 新用例全为轴向偏移、自身不区分切比雪夫/曼哈顿——补 `range_rules_use_chebyshev_metric_on_diagonal_offsets`（heal.range=2 下 (10,10)→(11,12) 对角放行，曼哈顿口径即红）；R1-P2-3 夹具处数更正（15 处）。
- R2-P2-1 attack/heal 门接收者形态与仓库其余门不一致（`this.pos.inRangeTo` vs 官方 `target.pos.isNearTo(this.pos)` 形）——改回 `target.pos.inRangeTo(this.pos, C.*)`（数学等价，形态统一）；R2-P2-4 sandbox 覆盖用例的 `foes[0]/foes[1]` 隐式依赖 find 数组序——改 `Game.getObjectById('t1'/'t2')` 显式引用。R2-P2-2（isNearTo/inRangeTo 的 roomName 缺省差异，真实 RoomPosition 下不可达）与 R2-P2-3（rangedHeal 传 `target.pos` 而非对象，官方同形差异、非本批引入）记录不改。
- R3-P2-1 sim 测试注释「模块恰计一次」为 move_cost 复制残留（规则包无状态不可观测）——注释改写并说明退出码 2 的实际依据；完整 check 首轮暴露 kk-core dump 键数门 423→427 未同步（三路评审与本批 check:fast 均未覆盖 kk-core，已更新该门并记入教训：改常量 dump 的批次 check:fast 必须包含 kk-core）；R3-P2-2 台账登记项「typegen 常量清单同步」经核为**确认无需同步**——`kk-mod-api/src/typegen.rs` 与 `mod types` 均不枚举常量（只生成类型/意图声明），登记段照此闭环；登记段「medic（heal 射程 1→3）」为子集表述，实际包还覆盖 rangedHeal.range 3→5，交付段如实记录。

评审人另核通过：gen-constants 重生成物与工作区逐字节一致；三处消费与原硬编码默认逐位等价；注入双通道同源（同一冻结计划 Arc）；场景断言数值推演与实跑一致（评审机 sim 2/2，129.51s）。

## 已知边界

- 值域上界 50 为宿主表示约束（房宽内安全放宽空间）；0 为实效禁用该档。
- 身份/归属/安全模式/参数表示检查保持原生；塔与 power creep 射程族未开放。
- rangedMassAttack 面积语义不在首片；开放需另行设计（距离档本身是语义而非单一射程）。
- 预检契约族后续子片：EXECUTION_ORDER、动作互斥、参数清洗策略、可扩展意图盘点（台账 G07 条目）。typegen 不枚举常量，无需同步（复审核实）。
