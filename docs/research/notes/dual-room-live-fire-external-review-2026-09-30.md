# 双房双玩家实弹验证批 外部复审（独立复核）

- **复审对象**：`main@35a5502`（`ec771e2`→`36d25b7` 六笔 + 收口 `35a5502`），改动面 18 文件 / +2160 −64，已用 `git log --oneline -8`、`git diff 0edbe5c..35a5502 --stat` 核对，与方案自述一致。
- **复审方式**：逐条对照代码核实；数值声明自行复跑；关键归因自行做消融/还原实验；关键测试做变异实验（mutation）验其杀伤力。凡推演未实测者显式标注。
- **复审日期**：2026-09-30。复审期间工作树全程保持干净（所有临时补丁均已还原，`git status` 为空）。
- **勘误（同日第二轮外部复审推翻一项）**：本报告 §0 判「未发现 P0」、
  §2.5.6 以 `(bucket+limit).max(−2×limit)` 复算停跑轨迹判 O3 数值成立——
  该式与官方 make.js:77「先按原桶严格比较」在 (−3×limit, −2×limit) 开窗
  （limit=20：−59..−41）内分歧，属 P0 缺陷（第二轮复审发现，当日修复，
  见 docs/plans/dual-room-live-fire.md §7「外部复审清偿（第二轮）」）。
  本报告其余结论（O1 等价/串行链/快照形态/两阶段回退变异）未受影响。

---

## 0. 总判定（先给结论）

**本批五片声明基本成立。** 未发现 P0（错误性缺陷）；未发现影响正确性或 perf 基线的 P1。所有我独立复跑的数值型声明（perf 负载计数/四相、棘轮 1405 零转红、bot 三基线、workspace 全绿、O1 的 55×）全部对上；两项关键归因（事件通道 +21%、O1 insert 55×）经我自行做的消融/还原实验成立；两条最紧要的断言（O1 续档守卫、房级两阶段回退）经变异实验确认有真杀伤力。

发现的问题集中在三类：**注释失实 1 处**、**测试判别力/覆盖面的局部缺口**（出生落位与 directions、max 续档守卫恒真）、**测试旋钮「一次性/零开销」纪律未完全闭合**（生产 tick 克隆、spawn 失败路径不清表、分片重建不覆盖全部旋钮）。均为 P2 级，不改行为、不动基线。

**倾向性观察两条**（不构成 finding）：
1. 「夹具/旋钮自述」类注释仍偶有与实现不同步——片 1 刚以「评审 1 P1-1 注释失实」清偿过一次，本批新写入的注释又出现同类失实（见 P2-1）。说明这条纪律需要一次固定的「注释与实测对账」动作，而不是靠逐次评审抓。
2. 「多重防线」的表述有时把「常量自洽微测」计入了行为防线（片 1 的距离预算微测只锁常量；max 续档守卫恒真）。建议把「这条防线能否分辨性质破坏」作为防线登记时的必填项。

---

## 1. Findings

### P0

**无。** 逐检查面（O1 语义等价、generation 不变量覆盖、测试旋钮可达性、O3 停跑数值、串行链不变量、快照字节形态）均未发现错误性缺陷。

### P1

**无。** 未发现需要在本批内修复的正确性/基线问题。

### P2-1 perf 夹具注释失实：「第三只兜底落 (4,2) 程外」

- **证据**：`crates/kk-cli/src/perf_probe.rs:254-259`
  > 「第三只孵出时两格已占，兜底全 8 向扫描落 (4,2) 族（程外、无动作 intent，仅投影/结算宽度——如实口径，负载由 3 放置 + 2 出生工作 creep 承载）」
  方案 `docs/plans/dual-room-live-fire.md` §7 片 1 同述「第三只兜底落 (4,2) 程外如实注记」。
- **实测（复现路径）**：用 `EmbedEngine` 复刻同一布局（`spawn(3,3)`、`SOURCES=(2,2)(1,3)`、`CREEPS=(2,1)(1,2)(3,1)`、`controller` 由 `create_shard` 置于 (1,1)），跑同一 `player_code()` 形态 1200 tick：
  - 带 `directions:[TOP,LEFT]`：出生序列 = **(3,2) → (2,3)**，第三只**永不出生**——它作为 `spawning=true` 的雏形停在 **spawn 自身格 (3,3)**；
  - 不带 `directions`（缺省序 1..=8）：出生序列 = (3,2) → (4,2) → (4,3)（第三只确实出生，且确实在程外）。
- **机理**：`spawn.rs:549-553` 缺省方向序 = `1..=8`；带 directions 时第一轮只在 TOP/LEFT 找（两格已被前两只占）→ 进入 `spawn.rs:607-640` 二次扫描，其余方向中 (4,2) 为空 → `return false`（「有出口但我们没选它，不 spawnstomp」）→ `spawn.rs:486-492` 把 `spawn_time` 顺延一 tick 重试；下一 tick 复现同一判定 → **永久推迟**。因此 (4,2) 这条兜底路径在本夹具下不可达；而滞留在 (3,3) 的雏形到首源 Chebyshev=1、到 controller ≤3，也并非「程外」。
- **影响**：注释的**结论**（第三只不承担负载 → 工作 creep 3→5）仍成立，基线数字不受影响；但这是片 1「注释如实」整改后新写入的失实注释，且掩盖了一个真实形态——本夹具的 spawn 孵化位被永久占用（`spawning` 永不落 None），种群因此稳定在 5 只工作 creep + 1 只永久雏形。
- **修法**：改为「第三只因 TOP/LEFT 被占、二次扫描在 (4,2) 找到空位而逐 tick 顺延，永不完成出生——雏形滞留 spawn 格 (3,3)（spawning=true）；实际负载 = 3 放置 + 2 出生共 5 只工作 creep」。

### P2-2 出生落位断言对 `directions` 无判别力（测试非空洞性）

- **证据**：`crates/kk-runtime/tests/embed_tests/dual_room_live_fire.rs:753-760` 断言 `born1` 落 `(3,2)`；`crates/kk-cli/src/perf_probe.rs:245-287` 的 `fixture_distance_budget` 只断言 `BORN_TILES` 常量在程内。
- **判定步骤（实测）**：`spawn.rs:549-553` 缺省序 = `1..=8`，TOP=1 即首格；本夹具下 (3,2) 为空 → 带/不带 directions 首个出生位都是 (3,2)。我用同布局探针实测两形态首个出生位完全相同（均 (3,2)）。
- **影响**：片 1 声明「出生经 `spawnCreep` directions 控制」的**夹具依赖**没有任何运行时观测面——`directions` 被静默丢弃时，`dual_lifecycle` 与 perf 负载自检**都仍会绿**（后者因 3 放置 + 出生#1 已足以产生 progress>0，见 P2-1 实测：不带 directions 时出生#2 落 (4,2) 无能量、无动作，但自检仍通过）。方案 §7 片 1 登记的「部分退化盲区——微测+注释双保险」中，「微测」实际是**常量检查**而非行为检查，不构成该防线的第二保险。
- **缓解现状**：`directions` 的**消费**在引擎级另有覆盖（`crates/kk-engine/tests/settle_tests/tombstone_ruin_engine.rs:882-905`：方向 1 被占 → 雏形不出世），机制本身不是零覆盖；缺口只在本夹具依赖它这一层，故判 P2 而非 P1。
- **修法**：把断言改成可判别的形态，三选一：(a) 生命周期测试先用一个占位 creep 压住 TOP 格、`directions:[LEFT]`，断言落 (2,3)；(b) 断言**第二只**出生位（带 directions=(2,3)、不带=(4,2)，天然可判别）；(c) 在 perf 侧加一条运行时断言 `第二只出生位 == BORN_TILES[1]`。

### P2-3 `max_id_cache` 续档守卫恒真（skip 分支不可达）

- **证据**：`crates/kk-world/src/world.rs:214-224`
  ```rust
  let id = self.next_available_obj_id()?;   // → next_obj_id() → max_obj_id()：在此处刷 m.0 = 当前代际
  ...
  self.generation += 1;
  {
      let mut m = self.max_id_cache.borrow_mut();
      if m.0 + 1 == self.generation {        // ← 恒成立
  ```
  `max_obj_id()`（`world.rs:340-356`）在 `m.0 != self.generation` 时刷成 `(self.generation, 真 max)`；由于它就在同一函数上方被调用，`m.0` 必等于自增前的代际，自增后 `m.0 + 1 == self.generation` 必然成立——max 侧守卫永不跳过。
- **变异实验（实测）**：
  - 把目录侧守卫（`world.rs:216`）删除 → `obj_dir_guard_skips_extension_when_stale` **转红**（失败点 `world.get(ObjId(900)).is_some()`），证明目录守卫确有杀伤力、测试非空转；
  - 把 max 侧守卫改为 `if false`（关闭 max 续档）→ `obj_dir_eager_maintenance_on_consecutive_inserts` **转红**，证明「续档」这件事本身被测试钉住；
  - 两处守卫同时改 `if true` 时，转红的仍只有目录侧断言——**max 侧「陈旧则跳过」这条语义在任何测试里都不可观测**（因为它不可达）。
- **影响**：无正确性影响（该处续档结果恒等于真值 max）。但注释暗示的「陈旧则跳过」在 max 侧不存在；一旦将来调整 `next_available_obj_id` 的调用位置（例如把 id 分配挪到 `insert_with_id` 之后），守卫会从恒真变成可跳过且无人察觉，语义脆弱。
- **修法**：二者取一——(a) 删掉 max 侧守卫、改无条件续档，并在注释写明「调用点保证 `max_obj_id` 已刷新本代际」；(b) 保留守卫但在注释里点明「当前恒真（防御未来调用序变化），目录侧守卫才是真失效纪律」。现状注释把两侧守卫写成对称的两件事，容易误导。

### P2-4 生产路径上的 `last_execution_order` 每 tick 克隆

- **证据**：`crates/kk-runtime/src/embed.rs:560-572`——`None`（生产）臂执行
  ```rust
  self.last_execution_order = Some(registered.clone());
  ```
  即每 tick 一次 `Vec<String>` 克隆 + 一次写入（另有上方 `handles = registered.clone()`，本批未新增）。
- **影响**：非零开销（每 tick 数次小分配）。量级在 perf 噪声以下（故同日 A/B 观察不到，方案 §7 片 2 的 A/B 数字仍可信），但方案「旋钮 Option 判别**零开销**／生产 None 路径与 HEAD 逐行为等价」这半句在「零开销」上不成立——HEAD 没有这个字段与这次克隆。
- **修法**：取证字段只在旋钮开启时记录（None 臂不写），测试侧的「默认序取证」改为读 `players()` 或加一个显式开关；或退一步：把这一行开销写进登记的「可忽略开销」清单，别写成「零开销」。

### P2-5 生产路径「非零成本」的其余旋钮（口径登记，供 M2B 参考）

- **证据/判定**：`Engine::fail_room` 已去 `#[cfg(test)]`（`crates/kk-engine/src/settle/mod.rs:539-541`），`tick_with_intents_with_input` 每房每 tick 一次 `Option<&str>` 比较（`settle/mod.rs:661`）；`EmbedEngine::settle_world` 每轮一次 `take()` + 两次 `is_some()`（`embed.rs:501-511`）；`Cmd::Bucket` 为枚举变体（`proto.rs:70-71`）；`sandbox_configs_for_test` 为 BTreeMap，仅在 `create_shard` 消费。**结论：其余旋钮结构上或近零成本可达，判定成立**；仅 P2-4 是真正的非零残留。此条列出是为了把「已判成立」的部分留痕（方案登记的数字「24B+每房每 tick Option 比较」与实现相符）。

### P2-6 `create_shard` 的 `?` 早退路径不清 `sandbox_configs_for_test`

- **证据**：`crates/kk-runtime/src/embed.rs:294-311`——`players.spawn(&handle, config)?` 在循环体内，`self.sandbox_configs_for_test.clear()`（`:311`）在循环之后。
- **复现/判定**：构造任一使 `spawn` 失败的情形（如线程创建失败）→ 早退后残留的配置项会被**下一次** `create_shard` 静默消费，违背「一次性消费」纪律（评审 1 P2-1 的清偿意图正是「防跨分片静默复用」，只差这一步）。
- **影响**：测试面专用旋钮，生产无影响；当前无用例踩到。
- **修法**：循环前先整表取走：`let configs = std::mem::take(&mut self.sandbox_configs_for_test);`，失败路径也不会残留。

### P2-7 分片重建/teardown 不覆盖全部测试旋钮

- **证据**：`create_shard` 只重置 `execution_order_for_test`（`embed.rs:292`）；`teardown`（`embed.rs:810-817`）不触碰任何旋钮。对比 O3 的 `sandbox_configs_for_test` 是显式重置的。未重置者：`fail_next_settle`、`fail_next_settle_room`、`last_execution_order`。
- **影响**：同进程内「设了旋钮 → 未消费 → 重建分片」时旋钮跨分片留存（例如 `fail_next_settle_room("W2N1")` 会让下一个分片的第一次结算按旧房名注入失败，若新房集不含 W2N1 则静默失效、含则意外失败）。测试面专用，当前无用例踩到。
- **修法**：`create_shard` 补齐清零，或在旋钮文档里明确「随分片不重置，调用方自管」。

### P2-8 O1 使 `pub rooms` 直写的后果升级（挑战登记口径）

- **证据**：`crates/kk-world/src/world.rs:142`（`pub rooms`）、`world.rs:306-331`（`get/get_mut/remove` 现依赖代际键目录）。
- **机理**：旧实现 `get/remove` 逐房扫描 → 直写 `rooms`（不 bump 代际）最多让**投影缓存**陈旧；新实现下同样的直写会让 `get` 返回 `None`、`remove` 静默失败（且失败时不 bump，形成静默不一致）。
- **现状核查**：全仓唯一的 `rooms` 生产直写点是 `crates/kk-engine/src/settle/mod.rs:677`，其所在函数在 `:700` 必调 `advance_ticks(1)`，**当前无不一致**；其余出现处均为读侧、`#[cfg(test)]` 或 bench。
- **判定**：此项在「已登记勿重复报告」清单内（`pub rooms` 收敛归后续批）。此处不是重复报告，而是按评审要求挑战登记准确性：风险**性质已升级**（从「缓存陈旧」到「对象定位错误/静默丢失」），登记时点早于 O1 落地。
- **修法（建议提前到 M2B 扩面之前）**：`rooms` 收 `pub(crate)`；或保留 `pub` 但在字段上加一句硬纪律注释「直写后必须 bump 代际，否则 O1 目录供陈旧定位」；或提供 `rooms_mut()`（内 bump）作为唯一可变出口。

### P3（观察，非缺陷）

- **ratchet-allow 现存 1 条豁免在本批是空转的**：该用例（inter-room creep transition）在 `HEAD~1` 基线即为 `failed`、本批仍 `failed`，不构成翻转，故豁免条目永不命中——棘轮自身也打出了「豁免条目未命中任何转红（可能已转绿，应清理）」的陈旧警告。方案 §5/§7 对它的表述（「用例现红=能力缺口如实呈现，豁免保留」）**没失真**，但「豁免」这个动作当前无门禁作用；能力补齐使该用例转绿时，会以「转绿 1 条」呈现，同样不需要豁免。清理与否不影响门禁，如实登记即可。

---

## 2. 验证清单（我独立验证成立的部分）

### 2.1 硬数据复跑

| 项 | 声明 | 我实测 | 判定 |
|---|---|---|---|
| workspace 测试 | 全绿 | 全绿；kk-world 48（=47+1 ✓）/ kk-engine 142 / kk-cli 3 / kk-storage-bench 8 | ✅ |
| fmt / clippy / 文档链接 | 零告警 | `cargo fmt --all --check` 过；`clippy --workspace --all-targets` 零告警；doc-link 148 md / 529 链接通过 | ✅ |
| 棘轮（release 重建后） | 1405 零未豁免转红 | `passed 1405 / failed 522 / skipped 987（floor 1405）`；相对 HEAD~1 **转红 0、转绿 0** | ✅ |
| bot tooangel | console error tick=0 | 2000/2000，硬中断 0、跳帧 0、console error tick **0**、缺失 API 0 → 通过 | ✅ |
| bot wasm | 零错 | 2000/2000，错 tick 0、缺失 API 0、桩调用 0 → 通过 | ✅ |
| bot overmind | 既知形态 | 验收判定「通过」（console 报错为既知形态） | ✅ |
| perf 负载计数 | progress 3328 / source 2836→2366 | **逐值一致**（与提交态 `compat/reports/perf-probe.json` 也逐值一致） | ✅ |
| perf 四相 p50 | 0.124/0.209/0.032/0.467 | 我跑 0.126/0.218/0.034/0.505（提交态产物 0.125/0.209/0.032/0.467）——同日同机噪声级 | ✅ |
| O1 insert 提速 | T3 约 55× | 见 2.3 | ✅ |

### 2.2 归因消融实验（我自己做的）

**「事件通道激活致 project +21%」——成立。** 临时屏蔽 `crates/kk-world/src/world_json.rs:72-79` 的事件注入后重测 perf-probe：负载计数保持 `3328 / 2836→2366`（证明消融干净、行为未变），project p50 由 **0.126 → 0.105 ms**（≈ 方案所记旧空转基线 0.102-0.103），run 由 **0.218 → 0.192 ms**。⇒ project 相增量确实来自每房每 tick 的事件载荷，而非对象形状（两版夹具对象数与类型均同）；run 相增量则是「事件载荷 + intent 序列化」两项之和。**注意**：run +44% 这一句只归因到「intent 序列化」是不完整的——消融显示事件载荷单独就贡献了 run 相约 12%。

（实验已还原，工作树干净。）

### 2.3 O1 性能声明复现（还原法）

把 `world.rs:241-244` 的 `next_obj_id` 临时还原为 O1 前的全房扫描写法：

- `cargo test -p kk-storage-bench --lib deterministic_bytes`（T3 双构建）= **16.41 s**
- 恢复 O1 后同一套件（8 测试，含同一 T3 双构建）= **0.28 s**

⇒ 约 **55–58×**，与「≈55×」相符；也直接印证了 insert 序列 O(N²)→O(N log N) 的量级判断。（实验已还原。）

### 2.4 测试杀伤力（变异实验）

| 变异 | 结果 | 结论 |
|---|---|---|
| 删 `world.rs:216` 目录续档守卫 | `obj_dir_guard_skips_extension_when_stale` 转红（`world.get(ObjId(900))` 为 None） | 该测试**有真杀伤力**，非空转；方案所称「删守卫全套件仍绿」的缺口确已封堵 |
| 关 `world.rs:221` max 续档（改 `if false`） | `obj_dir_eager_maintenance_on_consecutive_inserts` 转红 | max 续档被测试钉住 |
| `settle/mod.rs:676-678` 改为逐房即时写回（破坏两阶段提交） | `dual_room_live_fire.rs:853` 以「先结算房 W1N1 的良好结果随整体回退」转红；引擎级 `engine_tick_rolls_back_all_rooms_on_failure` 同步转红 | 片 3 该断言**确实区分**「先房提交后回滚」与「根本没结算」，不是同构重复 |

（三组变异均已还原，`cargo test --workspace` 复跑无失败。）

### 2.5 逐面核实（读码级，均成立）

1. **变更面/提交切分**：六笔提交与方案 §7 各片标签一致；`git diff --stat` 与自述一致。
2. **串行链不变量**：`crates/kk-runtime/tests/` 下唯一 `#[test]` 在根文件（`embed_tests.rs:25`）；三个新子模块 0 个 `#[test]`；链体在既有 21 项**之后追加** 10 项（片 2 五项 + 片 3 三项 + O3 两项），原有调用序逐字节未动；链尾 `lifo_violations()==0` 断言保留。`teardown` 由 `Drop` 兜底（`embed.rs:820-824`），失败路径不泄漏线程。
3. **O1 语义等价**：逐个方法与原实现比对——
   - `get`：目录值取 map **key**（非 `state.name`），与原 `values()` 键序扫描同构；返回的仍是 `state.name`，逐位等价；
   - `remove`：定位与原「键序首个含该 id 的房」相同；未命中时**都不 bump**；
   - `get_mut`：先定位后 bump，bump 语义不变（id 不存在仍 +1，`generation_bumps` 类测试覆盖）；
   - `next_obj_id`：在「无对象」与「id=0」两个边界与原 `map_or(1, saturating_add(1))` 式等价；
   - `insert` 的 `RoomState::insert_with_id` 不 bump，故 `insert` 恰 +1（守卫分析的前置条件成立）。
4. **generation 不变量覆盖全部对象写入面**：唯一直写点 `settle/mod.rs:677` → `:700` `advance_ticks(1)`；`set_terrain`/`register_room`/`room_mut`/`get_mut`/`remove`/`advance_ticks` 均 bump；「一成功 tick 恰 +1、失败 tick +0」有单测（`settle/mod.rs:715-727`）。
5. **快照字节形态未变**：`World` 无 `Serialize`（仅 `Debug, Clone`），新缓存字段不落快照；`deterministic_bytes`/`snapshots_decode_and_shapes` 通过。`World` 无生产克隆调用点，Clone 成本不在热路径。
6. **O3 停跑数值口径**（**勘误：本条公式复算被第二轮推翻，见头部注记**）：`budget.rs:61-82` 的判定与回充公式（`(bucket+limit).max(−2×limit) − bucket`）使 −2000 → −40 → −20 → 0 → 恢复，与 embed 全链五臂实测一致；`Sandbox::bucket()`（`sandbox.rs:317-319`）读的是 `budget.bucket()` **原值**，`pending_recharge` 是独立字段（`sandbox.rs:186`、`:339`）——**不含 pending 候选**，断言语义成立。
7. **「结局盲提交」断言有分辨力**：`commit_players`（`embed.rs:624-643`）按 `results.keys()` 无结局过滤地调 `settle_bill`；若未来改成按结局过滤，Terminated 玩家的 `pending_recharge` 不并入 → 混合轮的 `-40.0` 断言直接转红（回归世界即 `-2000`）。B0-6 销账判定成立。
8. **竞争「玩家名序」有杀伤力**：结算侧玩家序确为 `IntentBatch.users` 的 BTreeMap 名字序（`settle/mod.rs:463-465`）；充裕 source 下任意序同果（无判别力），稀缺变体（source=2）下 p1 足额 / p2 颗粒无收——是真杀伤线，方案自述属实。
9. **守恒恒等式非弱式**：脚本带满仓即停门（把超容掉落/衰减排除在恒等式之外），恒等式为精确 0，另有 `p1_ > p0` 防「零活动恒等」；B 世界另加 `Δsource = Δstore_p1 + Δstore_p2`。A 世界的 harvest 臂在本测试内不被单独断言（由场景 A 覆盖），属可接受的冗余，非缺陷。
10. **夹具容量口径与引擎一致**：`economy.rs:41-56`——有主 controller → 3000、无主 → 1500（并钳 energy）、无 controller → 4000（KEEPER，不钳）。故 own 房摆 3000、野房 `controller:false` 摆 4000 的选型正确，确实免掉逐 tick 归一差。
11. **`Cmd::Bucket` 通道**：与 `SettleBill` 同模式，`recv` 无超时继承既有登记口径（Bucket 臂挂死面更窄，方案已登记）；`#[doc(hidden)]` 全链一致。
12. **排列校验用无条件 assert**（`embed.rs:564`）：只在 `Some` 臂内，生产不进；release 下误置畸形序从「静默漏跑/双跑」变早失败——与自述一致，是相对 HEAD 的实质改进。
13. **文档与登记诚实性**：
    - `m2-playable.md` 4.3 勘正与代码相符：`room.js:477` `createConstructionSite`、`room.js:489` `createFlag` 均为 `countStub`，行号亦对；
    - B0-6 处置表**未回改**（`b0-6-external-review-clearance.md:150` 仍为「部分清偿+登记」），销账落双房批 §7 + roadmap 批条目，与自述一致；
    - `perf-a6.md` 头部作废注记、`performance-acceptance.md` 口径注记、W 室「不做清单」中的 `Action::Build` 白名单行号（`intent.rs:88`）均核实无误。
14. **测试旋钮的生产可达性**：`execution_order_for_test` 在生产恒为 `None`（无生产调用方）；`fail_next_settle_room` / `sandbox_configs_for_test` / `player_bucket_for_test` 均为 `#[doc(hidden)]` 且无生产调用方；`Engine::fail_room` 常驻 release 的开销是每房每 tick 一次 `Option<&str>` 比较（P2-5，登记口径属实）。

---

## 3. 处置建议

| 编号 | 建议 |
|---|---|
| P2-1 | 本批内或紧随小修一并改注释（纯文字，无行为影响） |
| P2-2 | 建议本批内补一条可判别的出生位断言（改动局部：加一个占位 creep 或断言第二只出生位） |
| P2-3 | 注释层面收口即可（说明 max 侧守卫当前恒真、目录侧才是失效纪律），随 M2B 扩面首片 |
| P2-4 | 或改实现（None 臂不记录），或把「零开销」表述改为「噪声级可忽略」——建议后者，成本为零 |
| P2-6 / P2-7 | 登记，随 M2B 扩面首片与旋钮清理一并做 |
| P2-8 | 建议**提前**到 M2B 扩面之前（`rooms` 收 `pub(crate)` 或补硬纪律注释）——M2B 将新增约 190 个 getter，可写面扩大前收敛最省 |
| P3 | 如实登记即可，不影响门禁 |

---

## 4. 处置记录（2026-09-30 双房批 §7 补登）

本报告八条 P2 + P3 逐条处置（P0 误证见头部勘误）：

| 编号 | 处置 |
|---|---|
| P2-1 | 已修：夹具注释按实测改写（第三只因 TOP/LEFT 被占逐 tick 顺延永不出生、雏形滞留 spawn 格 (3,3)；负载=3 放置+2 出生） |
| P2-2 | 已修：生命周期测试采用其修法 (a)——占位 builder 压 TOP 格 + `directions:[LEFT]`，断言出生落 (2,3)（带/不带 directions 可分辨） |
| P2-3 | 已修（其建议 b）：max 侧守卫保留 + 恒真注记（入口 next_available_obj_id 已刷代际；真失效纪律在目录侧守卫） |
| P2-4 | 已修（优于其建议 b）：观测门控——None 生产臂不写 `last_execution_order`，记录面零分配（registered 自身 clone 为 HEAD 既有） |
| P2-5 | 留痕即可（判定与登记相符，无需动作） |
| P2-6 | 已修：create_shard 改循环前 `mem::take` 整表取走，失败早退不残留 |
| P2-7 | 已修：create_shard 补齐 `fail_next_settle`/`fail_next_settle_room`/`last_execution_order` 三旋钮清零 |
| P2-8 | 已修（其折中案）：rooms 字段硬纪律注释（直写不 bump → 目录供陈旧定位）；收 `pub(crate)` 归 M2B |
| P3 | 如实登记（豁免空转不改门禁作用；能力补齐转绿时按「转绿 1 条」呈现） |
