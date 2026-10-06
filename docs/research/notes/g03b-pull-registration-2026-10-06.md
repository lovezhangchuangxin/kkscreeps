# G03.b 拉拽登记策略交付笔记（2026-10-06）

基线 `8f1677e`（登记提交随基线）。目标：开放拉拽登记策略槽，并完成台账要求的前置复审——`validate_pull_pair` 胜者升序遍历在同拉者多目标场景的顺序语义。

## 前置复审结论

1. **单目标不变量的来源**：`UserIntents.objects: BTreeMap<ObjId, BTreeMap<Action, IntentData>>`（crates/kk-engine/src/intent.rs:394）结构上保证每 creep 每 tick 至多一条 pull 意图（后到覆盖先到）。
2. **残标缺口**：`add_pulling` 覆写不清理对侧旧标记——同拉者换目标留下 `pulled_by[旧目标]` 残标、多拉者同目标留下 `pull_of[旧拉者]` 残标。前者在原生意图流不可达（单目标不变量），后者可达但读侧无害（`pulled_by` 是权威表），仅槽输入 `pulling` 标记出现已失效残标。
3. **顺序敏感性**：若开放重登记（同拉者多目标），首个失败配对的复核清理会连带清除当前配对——结果依赖胜者 ID 升序遍历（= 对象创建序，任意）。开放前必须修复。
4. **修复**：`add_pulling` 双向覆盖一致性（换目标清旧 `pulled_by`、换拉者清旧 `pull_of`），两表保持双射；每个失败配对的清理只触及自身两侧，`validate_pull_pair` 与遍历顺序结构无关。`pulling` 残标消失（无玩家面暴露，仅 Mod 槽视图更真）。

## 交付内容

1. **引擎**：`movement.rs` `add_pulling` 双射修复（含 `prefill_admissions` 注释更新）；`movement/inputs.rs` `pull_registration_input` 投影（拉者升序）与 `apply_pull_denies`；`settle/pull_registration.rs` 候选（`deny/is_denied/into_denies/denies_snapshot`，镜像预检候选）；`contest.rs` `arbitrate_room` 首位接线（预检之前）。
2. **槽登记**：`builtin:room.movePullRegistration`（内建槽 18→19、revision v12、PlanHash 变更；`SLOT_KEYS`/描述符/bindings 清单/install NATIVE/`BUILTIN_SLOT_KEYS` 计数同步）。
3. **链路**：`SlotCall::PullRegistration` 变体、mod_chain 能力门（`pull_registration_slot`）+ ctx 桥接（`pull_denied/pull_deny`）、`load_fields`/`write_fields` 守卫、`staged_states` 条件、Declined 白名单（execution_plan `decline_allowed` + chain.rs 错误枚举文案）、sdk.js `ctx.pull`（isDenied/deny，同步期开门）、realm op 注册（`op_mod_chain_pull_deny/denied`）。
4. **能力面**：单向否决（等同从未登记）；放行/新增方向不开放——新增拉拽无属主意图，疲劳转移会向未行动玩家计费（宿主表示约束，理由入文档与台账）。
5. **验收**：引擎 6 测试（双射与幂等、遍历顺序无关双 id 布局、离房拉者边界、否决逐阶段与从未登记等价、候选域校验、输入完整性）；运行时 9 测试（字段形态+原生对照含疲劳转移、否决=无 pull 意图基线、decline 回退与 before 层 state 丢弃、与预检组合视野、与准入组合预填+放行、能力门、故障停用回退+rebuild 序列）；anchor（全量禁拖）/leash（反绑架跨主人否决）两独立包 + 原生 + 组合四场景夹具（sim-mod-move-pull-registration）；kk-cli sim 2 测试（四场景 × JSON/KKPV 双通道 final/plan_hash/组合身份互异 + 第 1 tick 后检查点冷恢复续跑）。
6. **文档**：architecture/mod-move-pull-registration.md + 两处索引 + 夹具 README + 本笔记。

## 夹具层踩坑

- `Game.creeps` 只含本方 creep（官方语义）：跨主人拉拽场景中 p1 直接 `Game.creeps.thief` 为 undefined、脚本中途抛错——敌方对象须经 `Game.rooms.W1N1.find(FIND_CREEPS)` 获取（首版夹具即踩中，原生场景断言失败后修正）。
- 无 MOVE 部件被拉者的唯一通路是 `move(相邻 creep)` 形态（G02.a 既知）；拉者移动方向与被拉者请求格的关系：被拉者请求的是**拉者当前格**。

## 证据

- `cargo test -p kk-engine` → 84 + 285 + 2 doc 全过（含 pull_registration 6 项）。
- `cargo test -p kk-runtime --test mod_chain_tests move_pull` → 9 passed。
- `cargo test -p kk-cli --test sim_tests move_pull` → 2 passed（四场景 × JSON/KKPV 双通道 final/plan_hash/组合身份互异 + 第 1 tick 后检查点冷恢复续跑，97.8s）。
- `cargo test -p kk-mod-api --test freeze` → 44 passed（含 catalog 槽计数断言 19）。
- 变异验证：临时移除 `add_pulling` 双侧清理（还原旧行为）→ `pair_validation_is_traversal_order_independent` 红（低 id 布局下当前目标失去被拉豁免）；恢复后绿——顺序无关断言具备回归力。
- `target/debug/kkscreeps sim run --scenario …/scenario-leash.json --ticks 3` 退出码 0。
- 复审清偿：三路后台只读复审（引擎正确性/接线完整性/验收与文档），1 P0（freeze 槽计数 18→19 漏改，G01 同类坑）+2 P1（顺序无关测试无区分力→重构为空身体+疲劳形态并做旧行为变异验证；台账承诺的 pull×admission 运行时组合缺失→补预填视野+放行测试）+6 P2（decline 丢弃 before 层 state 对仗、疲劳转移可观测断言、目标侧移除边界措辞、笔记计数 7→6 实为 6、证据回填、重复解除配对逻辑备忘）全部清偿。
