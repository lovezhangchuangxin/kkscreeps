# M2B 扩面 P4（结构读取面 + 衰减结算）提交前复审（独立复核）

- **复审对象**：`main@772f857`（HEAD=P3）工作区未提交改动——15 文件 / +422 −11，另 2 个新测试文件（`git status` 与自述一致）。
- **复审方式**：三路独立视角 subagent（官方语义逐条对照 / 测试判别力 / 跨层键名一致性）+ 主审自行复跑官方源码对照、写守卫测试复现缺陷、变异实验验证测试杀伤力、复跑 workspace 与棘轮。数值声明全部自行复跑。
- **官方基准**：`sources/screeps/engine/src/`（`processor.js`、`game/structures.js`、`processor/intents/{ramparts,roads,containers,links}/tick.js`、`_create-energy.js`、`creeps/build.js`）与 `screeps-ok/adapters/vanilla/index.ts`（`placeStructure` / `getStructureDefaults` / `getDefaultDecayTime`——适配器即官方 DB 形态的权威口径）。
- **复审日期**：2026-09-30。变异实验每轮后均还原，`git status` 全程可控。

---

## 0. 总判定（先给结论）

**P4 的结算数值与次序语义扎实（官方符合度高），但初版有两处必须修的缺陷，已在本轮修复并加守卫。**

- **P1 修复 2 项**：①**建造落成入口漏建衰减钟**——phase 8 把缺钟（`nextDecayTime=0`）判为「已到点」，建成 rampart（hits=`RAMPART_HITS`=1）在**落成同一 tick** 被 300 打光移除、container 首 tick 误掉 5000（P4 引入的功能性回归）；②**rampart 的 hitsMax 同步被误置入衰减钟门内**——官方在门外每 tick 执行，初版要等下一个衰减窗（最长 100 tick）才更新。
- **P2 修复 2 项**：③link 的 cooldown 形态（初版误折 `cooldownTime`，官方 link 用每 tick 计数器 `o.cooldown`）；④wall 撤除官方不存在的 `ticksToDecay`。
- **清偿复审（对上述修复自身的独立复核）再修 3 项 + 补 1 项**（§5）：⑤容量平面/per-resource 未成对（`getCapacity()` 由官方 null 变 5000）；⑥装配兜底遮蔽 `energyCapacity` 取数（lab/powerSpawn/nuker/link/tower 恒 0）；⑦link 冷却无结算面（永不递减）；补 build 落成臂的 `storeCapacityResource` 与 nuker `cooldownTime`。
- **数值声明全部复跑对上**：workspace **310 绿 / 0 failed**；棘轮 **1623 / 2914**（P4 收口 1593 → 本轮终值 1623），相对 HEAD~1 **转红 0 条**、转绿 45 条；clippy 0；fmt 干净；bot 三基线（本轮补做，见 §4）判定全过；perf 四相持平。**棘轮读数勘误**：上稿记 1618 且称「清偿修复不改变计数」——第三轮复核实测为 1623，那 5 条转绿实由清偿修复贡献（上轮跑棘轮时 release 未随批重建，读到陈旧二进制），见 §4 与 §5-6。
- **修复后棘轮 +5 转绿**——即前 4 项修复矫正了 5 个此前未通过的官方用例，是「修复方向正确」的机器证据，而非主观判断。清偿复审的 3+1 项**未改变棘轮计数**（官方套件不覆盖 lab/link/nuker 的容量与冷却读数）——其判别力改由**四条变异实验**逐条证明（§5）。
- **测试缺口 3 处已补**（此前无守卫）：hitsMax 门内外同步、road 重挂钟、container 落地的多资源与量。清偿复审另补 5 条守卫（§5），并对 container 落地断言做了逐键校量强化。
- 另有 3 项低危**登记待后续**，不影响本批。

---

## 1. Findings

### P1-1 建造落成入口漏建衰减钟（已修复）

**证据**
- 初版 `crates/kk-engine/src/settle/creep/build.rs`（`other` 臂）落成 data 只有 `type/structureType/owner/hits/hitsMax/notifyWhenAttacked`——**无 `nextDecayTime`、无 `storeCapacity`**。
- 官方 `processor/intents/creeps/build.js:161-168`（rampart）`nextDecayTime: gameTime + C.RAMPART_DECAY_TIME`；`:255-263`（container）`storeCapacity: C.CONTAINER_CAPACITY, …, nextDecayTime: gameTime + C.CONTAINER_DECAY_TIME`。
- 判据 `crates/kk-engine/src/settle/economy.rs::generic_decay_tick`：`if next != 0 && w.time < next.saturating_sub(1) { return }`——`next==0` 视为**已到点**。
- 落成对象在 phase 8 的 `ids` 快照之内（`settle/mod.rs` 的快照在 creep intent 阶段之后取）——所以是**本 tick**、不是「下一 tick」。

**实测复现（修复前）**：新增守卫 `built_rampart_gets_decay_clock` 转红于「rampart 落成」——首次 `settle` 后房内已无该 rampart。

**影响**：玩家用 creep 建成 rampart，下一帧它就不存在；container 建成即掉 5000 hits 且 `ticksToDecay`/`storeCapacity` 读 undefined。P4 之前无衰减结算，缺钟无害——本条是 P4 引入。

**修复**：新增 `kk_world::place::{built_decay_clock, built_store_capacity}`（与放置面同源），`build.rs` 落成臂复用。

### P1-2 rampart 的 hitsMax 同步被置入衰减钟门内（已修复）

**证据**
- 初版 `economy.rs::generic_decay_tick` 先 `return`（未到点），之后才做 hitsMax 同步。
- 官方 `processor/intents/ramparts/tick.js:19-24` 的同步**在 :26 的门之前**，即每 tick 无条件执行。

**变异实验**：新增守卫 `rampart_hitsmax_syncs_before_decay_window`（远钟 + rcl3）；把同步移回门内 → **唯一转红的就是它**，其余 6 个衰减族用例（含到点场景的 `rampart_decay_and_hitsmax_sync`）全绿。即该分歧点此前**无任何测试覆盖**。

**影响**：有主房 rcl 变化后 `rampart.hitsMax` 要等下一个衰减窗才归一，机器人按 hitsMax 规划 repair 会被误导（repair 上限本身由 `damage.rs::structure_hits_max` 现算，不受影响）。

**修复**：同步移回门外，并**改回官方精确查表**（`RAMPART_HITS_MAX.iter().find(|(k,_)| *k == level.to_string())`，与 `damage.rs` 同式）——初版「向下取最近键」在 level>8 时与官方分歧。

### P2-1 link 的 cooldown 形态（已修复）

- 官方两形态（vanilla `index.ts:1145-1158` 明文注释）：**link/extractor 用每 tick 递减计数器 `o.cooldown`**（`links/tick.js:11-22` 递减、getter 直读），lab/terminal/nuker/factory 才用 `cooldownTime` 折算。
- 初版对所有 Generic 结构把 `spec.cooldown` 折成 `cooldownTime`，prelude `StructureLink.cooldown` 也挂 `cooldownFold`。
- 修复：place 面按类型落对键；prelude link getter 改回 `(o) => o.cooldown || 0`；注释勘正（初版「官方 :481-487 域」套 lab 式属失实）。玩家可见值两形态等价，但错形态会在接线 transferEnergy（P5）时静默失配。

### P2-2 wall 挂了官方不存在的 ticksToDecay（已修复）

- 官方 `game/structures.js:839-841`：`StructureWall` 只挂 `ticksToLive`，**无 ticksToDecay**；`StructureRoad`（:672-674）才有，且带 `decayTime` 兜底分支。
- 初版对 road/wall 共用循环同挂。修复：road 单挂并补 `decayTime` 兜底、wall 撤除。

### P2-3 注释失实（已修）

- `economy.rs` 注释「官方 tick 头段——与衰减同 tick 执行」（官方是每 tick）。
- `compatibility.md` 原登记把 P1-2 的行为写成「放置面 hitsMax 缺省取上限、首个衰减 tick 起随 rcl 归一」——**把缺陷写成了设计口径**，已改回真实语义并补两条新登记。

### P2 登记（未修，不影响本批）

- **`storeCapacityResource` 无源**（**本轮已修，见 §5-1/§5-2**）：初稿登记为「prelude 的 lab/powerSpawn/nuker `energyCapacity` 读 `o.storeCapacityResource.*`，而放置/建造面不写该键 → 恒 0（官方 lab 为 2000）」。清偿复审确认这是**两条独立缺陷的叠加**——数据源缺失 + 装配兜底遮蔽取数——并已双双修复（平面/per-resource 成对落；兜底改读官方口径）。**残留勘正（实测）**：清偿稿曾把 `spawn`/`extension` 也列为「`storeCapacityResource` 无源 → `energyCapacity` 读 0」，**实测不成立**。两者走 typed 变体装配，容量键由**投影**携带（`world_json.rs:450` spawn `storeCapacityResource.energy`=300、`:473` extension 按 rcl、`:519` storage `storeCapacity`）——而喂给 prelude 的正是投影（`embed.rs:597` 走 `world_json_for_viewer`，非 snapshot）。探针实测：spawn `energyCapacity`=300、extension=50（rcl 3）、storage `storeCapacity`=1000000、两者无参 `getCapacity()`=null（官方 store.js:28-33），**全部对上**。探针已留作守卫 `m2b_structure_defaults_probe_impl`。
  - **承重点的精确化（独立复核指出）**：让读数正确的是「投影携带该键」，**不是**「装配路径不挂 ELSE 兜底」。复核把 `structures_spawn.js:490-491` 的 `cls` 映射里 extension 项删掉（迫使它走实例级 ELSE 兜底）后探针**仍全绿**——因 §5-2 修复后兜底读的也是 `storeCapacityResource.energy`（`:197`），与类原型表（`:171`）同键同式。「不挂兜底」只在 §5-2 修复**之前**（兜底原读原始键 `o.energyCapacity`）才承重。
- **真实残留**（按官方 `getStructureDefaults` 16 类逐键对账，独立复核逐类核过）：`actionLog`（tower/link/lab）、`isPublic`（rampart）—— 归 **P5**（动作族 / `setPublic`）。**核对无缺口**：`observer`/`extractor`（typed 与 Generic 两路 hits/cooldown 均齐）、`name`/`spawning`（spawn）、`hitsMax`（16 类逐类有值）。
  - **原列的 `mineralType` 属误列**：官方 lab 的 `mineralType` 也是**从 store 派生**的 getter（`structures.js:305` `labMineralTypeGetter = o => _(o.store).keys().filter(k => k != RESOURCE_ENERGY && o.store[k]).first()`），`engine/src` 内无任何直读 DB 键 `obj.mineralType` 者；我方同式派生 → 两侧同值。该 DB 键在官方也是死数据。
  - **须分层登记**：`isPublic` 在**玩家 getter 面已等价**（`!!undefined` ≡ 官方 `!!o.isPublic`），差异只在 **snapshot 契约面**（vanilla `RampartSnapshot.isPublic:false`）。清单应按「玩家面 / 快照契约面」两列分写。
  - **建议不单独立「结构 defaults 完整化」批**，并入 P5 的字段表同步点。
- **新增修复：spawn 放置面 `notifyWhenAttacked` 缺省应为 `true`**（独立复核发现，**非 P4 引入**，属 A4-iv 通知开关面遗留）。官方 `getStructureDefaults('spawn')` 缺省 `true`（vanilla index.ts:1919）、引擎 `build.js:116` 对新建结构同；我方 `place.rs:245` 原缺省 `None` → 玩家对**放置的** spawn 调 `notifyWhenAttacked(true)` 会**多记一条 intent**（官方 `true != true` 不记，`structures.js:1220` 差分门），且 `_damage.js:78 if(target.notifyWhenAttacked)` 的受击通知判据对放置 spawn 判假（该通知发送面本身尚未接线，已登记）。修复：`.unwrap_or(true)`（显式传值仍覆盖）。既有测试用**显式带 `notifyWhenAttacked:true` 的夹具**，恰好绕过了这条缝。
- **投影形状残留**（复核顺带指出）：spawn 投影多出非官方平面键 `energyCapacity`（`world_json.rs:449`）与 `off`（`:453`）、extension 多出 `off`（`:474`）。当前无消费者（getter 只读 `storeCapacityResource`），不可观测；归 P5 字段表同步。
- **effects 键原样透传**：`generic_fields` 保留 `effects`；若伪数据塞 `PWR_SHIELD`，官方会跳过衰减、我方照衰减。M2 无 power 体系，判不可达，建议放置面剥除或显式登记。
- **过期绝对钟的折算形态**：snapshot 用 `saturating_sub`（读 0）、prelude `decayFold` 用裸减（可读负）。正常路径钟到点即被消费重挂，不可达；两面服务不同消费者，属已知形态差。
- **遗留相对键**：P4 前的持久化若存相对 `ticksToDecay`，新链路不识别。M2 无跨版本持久化通道，风险仅限真实遗留字节。

---

## 2. 已核对一致（独立验证成立）

1. **road 地形系数**：官方两个独立 `if`（沼泽 ×5、墙 ×150）可叠加；P4 同构。同格 swamp/wall **对象**的兼容分支在 typed 世界无对应形态（不可构造）。
2. **`object.hits = object.hits || 0`**、container 死亡落地次序（先减 hits → 同格承接门 `hits>0` 拒收 → 成堆）、`amount > 0` 过滤。
3. **容器衰减窗判据** `roomController && level > 0 ? OWNED : NON_OWNED`；与放置面 `ctx.controller` 判据等价。
4. **门判据** `!nextDecayTime || gameTime >= nextDecayTime-1` 与 `next != 0 && time < next-1`（含 next==0 / next==1 / next==time 边界）。
5. **phase 8 遍历快照语义**：官方 lodash `keys()` 起始快照 ≡ 我方 `ids` 快照（中途新增的掉落堆不入本轮）。遍历顺序差异（id 升序 vs JS 键序）是既有 D4-1 登记项。
6. **Resource.energy 别名**：仅 energy 堆补 `energy=amount`，非 energy 无键——与官方 `resources.js:36` 同形。
7. **getter 逐条对照**（container/rampart/link/terminal/lab/powerSpawn/nuker/factory/portal/keeperLair/powerBank/invaderCore）：与官方 `structures.js` 定义一致；keeperLair/powerBank 的 `my`/`owner` 常量与官方同。
8. **跨层键名闭合**：place 写 `nextDecayTime`/`cooldownTime` → world_json 原样透传（非白名单，不被过滤）→ prelude 读同名；typed Road 投影的 `nextDecayTime` 与 prelude 读的键一致；`remove` 保证不留双形态。

---

## 3. 测试缺口与补充

| 缺口（复审前无守卫） | 变异验证 | 已补测试 |
|---|---|---|
| hitsMax 同步在门内/门外的分歧 | 移回门内 → 仅新守卫转红 | `rampart_hitsmax_syncs_before_decay_window` |
| 建造落成入口建钟 | 修复前直接转红 | `built_rampart_gets_decay_clock`、`built_container_gets_capacity_and_decay_clock` |
| road 衰减后重挂钟 | — | `road_decay_rearms_clock` |
| container 落地只校「存在堆」，不校量与多资源 | — | `container_decay_drops_store_and_dies` 强化为逐键校 energy=50 / U=30 |
| **清偿复审补（§5）** | | |
| 平面容量缺 per-resource 配对（`getCapacity()` 判据反转） | 摘 lab 臂 → `Null` vs `2000`；e2e `5000` vs `null` | `built_lab_gets_paired_capacity` + e2e `store.getCapacity()` 断言 |
| 装配兜底遮蔽 `energyCapacity` 取数 | 取数回退原始键 → `0` vs `2000` | e2e `lab.energyCapacity` 断言 |
| link 冷却无递减结算（旧读数 7 恒真，无判别力） | 摘 link 分派 → `7` vs `6` / `1` vs `0`；e2e `7` vs `5` | `link_cooldown_decrements_each_tick`、`link_cooldown_stops_at_zero` + e2e `tick(2)` 后读数 |
| nuker 落成的 `cooldownTime` / build 臂 per-resource 容量 | — | `built_nuker_gets_cooldown_time` |

---

## 4. 复跑记录

- `cargo test --workspace`：**310 passed / 0 failed**（逐行 `^test result:` 求和实测；settle 169 / world 49 / runtime embed suite 等）。
  - 口径勘误：本报告初稿记 settle 165、plan 记 workspace 309——清偿复审期以逐行求和复测，加本轮 4 条新守卫前为 306。原记数值未复现，按实测更正。
- `cargo clippy --workspace --all-targets -- -D warnings`：**0 warning**。
- `cargo fmt --all --check`：干净。
- `node compat/scripts/ratchet.mjs`：`passed 1623 / failed 304 / skipped 987（floor 1618→1623）`，**转红 0 条**（豁免 0、待归因 0），转绿 45 条。
  - **勘误（第三轮复核）**：上稿记 `1618`、并称清偿复审的 3+1 项修复「不影响该计数」——**均误**。实测 1623，差额 5 条正来自清偿修复。**根因**：本仓棘轮不自建二进制（`ratchet.mjs` → `run-suite.sh` → `compat/src/rpc.ts:38-42` 取 `target/release/kkscreeps` 优先），而 `run-suite.sh` 头注把 `cargo build --release -p kk-cli` 列为**前置**——上轮跑棘轮前未重建，读到的是修复**前**的 release。
  - **消融实证**（本轮跑前已重建，并以二进制 md5 核对构建确随代码更新）：含全部修复 = 1623；回退 spawn `notifyWhenAttacked` 修复 = 1623（**该修复对棘轮 ±0**，两版二进制 md5 不同）；再回退清偿修复的装配兜底 `energyCapacity` 与 link 分派 = 1622（这 2 项占 **+1**）；其余 **+4** 归容量成对落与 build 臂。
  - 地板已随本批上调 **1618 → 1623**（README 55.53% → 55.70%）。
- **bot 三基线**（`compat/reports/bot-{tooangel,overmind,wasm}.json`，本轮按基线口径重跑并刷新入库）：

| 基线 | 参数 | 完成 | 中断 | 跳过罚 | console 错误 | 缺失 API | 判定 |
|---|---|---|---|---|---|---|---|
| tooangel | `fixtures/tooangel/src` 2000t | 2000 | 0 | 0 | 0 | {} | 通过 |
| overmind | `fixtures/overmind-dist` 2000t | 2000 | 0 | 0 | 2000（**类型集与旧基线全等**——唯一条数 10=10，首条仍为已知 `RawMemory.setPublicSegments`，归 P6） | {} | 通过 |
| wasm | `fixtures/wasm-bot` 10t | 10 | 0 | 0 | 0 | {} | 通过 |

- **基线陈旧（新发现）**：入库的三条基线最后一次提交分别为 **A6（tooangel/wasm）**、**P2（overmind）**——P1/P3/P4 各批的「三基线通过」用的是未入库的本地跑（默认 report 路径未跟踪）。本轮已刷新。
- **消融归因**：刷新后 tooangel 的 `stub_calls_top` 相对陈旧基线整体位移（`PathFinder.search` 10000 登顶、room.find 行号集改变）。回退本轮两条 bot 可见改动（`ELSE_STRUCTURE_FIELDS.energyCapacity` 取数、link 冷却分派）重跑 → `stub_calls_top` 与错误集**全等**（仅首行 `Load: 5/6` 加载计数噪声）→ 位移属 A6→P4 累计变化，**与本轮修复无关**。
- **perf 观测点**（`perf-probe --ticks 1000 --warmup 100 --players 2`）：p50 project 0.1254→0.1244 / run 0.2087→0.2099 / settle 0.0322→0.0309 / wall 0.4665→0.4610 ms，**持平**；负载自检三项（progress_delta 3328、source_energy_min 2836→2366）与基线**完全相同**（夹具行为无漂移）。
- `node scripts/check-doc-links.mjs`：160 个 Markdown / 555 个本地链接，通过。

---

## 5. 清偿复审（对上述修复自身的独立复核）——3 项修复 + 1 项补齐

**方法**：对第 1 节四项修复做独立复核（不看修复自述，直接对官方 `build.js` / `getStructureDefaults` / `structures.js` / `store.js` 逐表比对，并用变异实验确认断言判别力）。

### 5-1（新，修复）容量平面/per-resource 未成对 —— 上轮修复引入

- **判据**：`Store.getCapacity()` 无参是 `object.storeCapacityResource ? null : object.storeCapacity || null`（`game/store.js:31`）。
- **实况**：上轮给 lab/container/terminal/factory/powerSpawn/nuker 补了**平面** `storeCapacity`，但没有成对落 `storeCapacityResource`——lab 的 `getCapacity()` 从官方 **null** 变成 **5000**（`LAB_ENERGY_CAPACITY + LAB_MINERAL_CAPACITY`）。这是上轮修复引入的新分歧。
- **修复**：新增 `kk_world::place::built_store_capacity_resource`（lab `{energy:2000}` / powerSpawn `{energy,power}` / nuker `{energy,G}` / link `{energy}` / tower `{energy}`），放置面与建造面**两入口成对落**。平面表同时补 `link`/`tower` 对齐 vanilla `getStructureDefaults`（`index.ts:1908-1979`——放置面权威表）。
- **官方表差异已核对**：官方 build.js 是 `getStructureDefaults` 的**子集**（对 link/tower 只落 `storeCapacityResource`、对 powerSpawn/nuker 亦然）。该差异**不可观测**：两键并存时 `getCapacity()` 恒 null；link/tower 官方本就没有平面 `storeCapacity` getter。故两入口共用本表单源。
- **变异证据**：`built_store_capacity_resource` 摘掉 lab 臂 → 守卫 `built_lab_gets_paired_capacity` 转红（`Null` vs `2000`）；同变异下 e2e 的 `store.getCapacity()` 断言转红（`5000` vs `null`）。

### 5-2（新，修复）装配兜底遮蔽 `energyCapacity` 的取数

- **判据**：`gw.defineGameObjectProperties(base, …)` 定义的是**实例级 own** 属性——装配兜底 `ELSE_STRUCTURE_FIELDS`（`structures_spawn.js`）会**遮蔽** `StructureLab/PowerSpawn/Nuker/Link/Tower.prototype` 的同名 getter（这些类型的 `cls` 不在映射表内，走 else 分支）。
- **实况**：兜底原读**原始键** `o.energyCapacity`——官方 DB 从无该键（能量容量在 `storeCapacityResource`），故 lab/powerSpawn/nuker/link/tower 的 `energyCapacity` 恒 **0**（官方 lab 2000 / link 800 / tower 1000 / powerSpawn 100 / nuker 300000）。
- **影响面**：方案 §4 P4 行把 PowerSpawn 的 `energyCapacity` 列为交付项——修前该声明**不成立**。属「P4 声明了但实际不可用」的一类，而非单纯的实现细节。
- **修复**：兜底改读官方口径 `(o) => (o.storeCapacityResource && o.storeCapacityResource.energy) || 0`（`structures.js:305` lab / `:483` link / `:605` powerSpawn / `:760` tower / `:1349` nuker 同一式）。**不补** `|| o.storeCapacity` 平面回退——官方对 `energyCapacity` 无此分支，补上会把官方的 0 抬成数值。
- **变异证据**：兜底改回 `o.energyCapacity || 0` → e2e 断言转红（`0` vs `2000`）。

### 5-3（新，修复）link 冷却没有结算面

- **判据**：官方 `processor.js:378` 按 `type == 'link'` 分派 `links/tick.js:11-22`——`cooldown > 0` 时每 tick `cooldown--`（并钳 0）。
- **实况**：上轮只在**放置面**把 link 的冷却改成了计数器形态（`o.cooldown`），但结算面**没有递减**——冷却永不归零。
- **旧测试为何看不出来**：`m2b_structure_readfaces_family.rs` 只在**放置同 tick** 读一次（恒 7）。副本变异后仍读 7——即该断言对「递不递减」无判别力，读数上把缺陷锁成了预期。
- **修复**：新增 `economy::link_cooldown_tick` + phase 8 分派。我方 Generic 透传桶 `type` 恒 `"structure"`（结构类型在 `structureType` 键），官方按 `type` 取——故分派改用 `settle::generic_structure_type` 取键。
- **变异证据**：摘除 link 分派 → settle 两条新守卫转红（`7` vs `6`；`1` vs `0`）+ e2e 的 `tick(2)` 后读数转红（`7` vs `5`）。e2e 原「放置同 tick 读 7」保留（该点官方同样为 7，正确）。

### 5-4（补齐）build 落成臂缺 per-resource 容量与 nuker 冷却钟

- 官方 `build.js:269-272` 对 nuker 落 `cooldownTime: gameTime + NUKER_COOLDOWN`、`:239/:269` 落 `storeCapacityResource`。已补（`built_store_capacity_resource` / `built_cooldown_time`）。
- **刻意不落并写明理由**（读取面等价，非遗漏）：
  - 各 owned 结构的 `store: {energy: 0}` —— `Store` 构造器吃 `object.store || {}` 且**过滤 0 值键**，缺键 ≡ `{energy:0}`（`getUsedCapacity`/`getFreeCapacity`/`Store` 三函数族同值）。
  - link/lab/factory 的 `cooldown: 0` —— link 的 getter 直读 `o.cooldown || 0`、lab/factory 的 getter 读 `cooldownTime`，写计数器键本就读不到；缺键与写 0 在读取面等价。

### 5-5 复核成立（无需改动）

1. 三处 Generic 构造点（`place.rs:78`、`place_structure.rs:304`、`build.rs:265`）——rampart/container 无第三条构造路径；`create_shard` 只经 place 面产 controller。
2. 快照 encode/decode 无损失真（新增键随 `data` 原样往返）。
3. `world_json` 的 Generic 是**全透传**（只覆盖 id/room/x/y）——新增的 `storeCapacity`/`storeCapacityResource`/`cooldownTime` 键必然到达玩家面，无白名单过滤风险。

### 5-6（第三轮）对「勘正」本身的独立复核

对 §P2 首条的勘正（判 spawn/extension 误报）与新增探针，另起一个独立 subagent 复核。结论：

1. **勘正成立**，但承重点被精确化——见 §P2 首条；复核用变异证伪了「不挂兜底」这条支柱（把 extension 逼进 ELSE 兜底后探针仍绿）。
2. **探针非空转**：5 条断言逐条变异各自打红对应项。其中 `v[3]/v[4]`（无参 `getCapacity()=null`）只在「去掉 `storeCapacityResource` 且补平面 `storeCapacity`」时转红——它守的正是 §5-1 那条判据。
3. **对账清单修正两处**：删 `mineralType`（误列）、补 `notifyWhenAttacked`（spawn 放置面缺省，本轮已修），见 §P2 登记。
4. **两条读法的消费面核实**：`e.get_object()`（`snapshot.rs:20-27`）与 `run_player()`（`world_json`）确是两个消费面，前者**永不喂玩家运行时代码**——snapshot 是 screeps-ok 契约形状（`snapshot.rs:1-9` 头注），spawn/extension 的 `storeCapacity: null` 与 vanilla `snapshots.ts:45-49` 的 `storeCapacityResource ? null : …` 结果一致，故「缺 `energyCapacity`」不构成契约缺口；且全仓无持久化重建调用者（`kk-storage` 仍是占位），当前**不可达**。
5. **未覆盖**：复核只验了这三条主张，§1/§5 的修复项与 §4 的数值声明未复跑（不在其范围内）。
6. **棘轮读数勘误（循复核「每条支柱单独变异」的思路补跑所得）**：上稿 §4 的 `1618` 与「清偿修复不改变计数」是**陈旧 release 二进制**的读数，真实为 **1623**（清偿修复 +5）。完整消融链见 §4。

---

## 6. 总评与建议

P4 的实现主体（三项衰减结算 + getter 族 + 折叠链路）**与官方语义吻合度高**，绝大多数声明经独立复核成立。真正的问题是三类：

1. **「衰减结算上线」与「结构落成/读取面」的接线没被整体检查**——新增 phase 8 衰减时只给放置入口建了钟，建造入口漏了；hitsMax 的新同步逻辑位置也比官方偏了一层。都不是「算错」，而是「新机制与既有入口的接线不完整」。建议：凡新增「按钟结算」的机制，收尾时过一遍**所有能产生该对象的入口**（放置/建造/持久化恢复），并把「缺钟」当作一等测试场景。
2. **prelude 一次性铺开 11 类 getter，但数据源覆盖不齐**，且**装配兜底的实例级遮蔽**让部分 getter 永远取不到值。本轮已补齐平面 + per-resource 两键并对齐兜底取数。建议把「getter 声明 × 数据源 × 是否被遮蔽」做成对账清单——三者任一缺失，getter 就是死的（5-2 正是「声明了但取不到」的实例）。
3. **修复自身也要过一遍「同一判据的全部消费方」**。5-1 是典型：补平面容量时漏了与它成对的 per-resource 键，反而把一个 null 判据变成数值。建议：凡按官方某个「三元/短路判据」写数据，必须把判据的两个分支都验一遍。

---

## 7. 主线复核补记（2026-10-01，ZCode 对本报告的逐条复核）

- **报告全部承重结论经官方源码逐处抽验成立**：ramparts/tick.js（hitsMax 同步在 :26 门之前、`RAMPART_HITS_MAX[level] || 0` 精确查表、`user != '2'` NPC 例外）、build.js 落成缺省表（rampart :161-168 / lab / powerSpawn / terminal / container / nuker / factory 逐段）、links/tick.js:11-22（`cooldown>0` 递减钳 0）、store.js:28-35（getCapacity 三元）、structures.js（:158/:673 ticksToDecay 折叠族、:839-841 wall 仅 ticksToLive、energyCapacity getters）、vanilla getStructureDefaults 16 类逐键、processor.js phase 8 分派序——与报告所记一致，行号全部 grep 实证。
- **复核新发现 1 项并修复**：**lab 矿物槽的容量绑定**（vanilla placeStructure :1122-1128）。spec.store 带矿物的 lab 须给 storeCapacityResource 补缺失矿物 LAB_MINERAL_CAPACITY——否则未存矿物 `getCapacity` 走回退式错读 3000（官方 0→null，槽绑定语义）。**勘误**：主线初判「填充与回退式恒等、仅 DB 形态保真」——恒等只对已存矿物成立（flat 5000−energy 2000=3000 表定义使然），槽绑定面经消融实测纠正。
- **消融归因**：禁用填充重跑棘轮 1626→1623，差 3 条 = STORE-BIND-002 H/O/G（「stored mineral binds the lab slot」）——填充的可观测面实证。e2e 双锚（玩家面 `getCapacity('O')`=null + 快照 storeCapacityResource U/L 键集）经变异实验各自转红。
- **主线复跑全门禁（修复后终值）**：workspace **310 绿/0 failed**；clippy 0；fmt 净；release 显式重建后棘轮 **1626 / 转红 0 / 转绿 48**（本报告 45 + 填充 3），地板 1623→**1626（55.80%）**；bot 三基线自跑复验三判定全过（overmind 形态集与入库基线全等 10=10；tooangel 有一条 A6 基线没有的 `[log] Executing room failed…lookFor` 形态——bot 自身 `getAdjacentPosition` 出界返 false 的 catch 日志、非 console error，bot 夹具无 lab 填充机械不可达，归 A6→P4 累计位移，与本报告 §4 消融结论一致）；perf 四相 0.125/0.217/0.031/0.480 持平。
- **报告三项建议全部采纳**并已在本批落地或登记：入口全遍历（build 臂已接单源函数）、「getter×数据源×遮蔽」对账（defaults 探针已入库为守卫）、判据双分支验证（成对容量落 + 本补记的消融即此模式）。
