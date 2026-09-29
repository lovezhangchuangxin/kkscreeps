# B1 结构治理批：>1000 行文件纯搬移拆分方案

> 状态：方案定稿（2026-09-29，基准 HEAD=fb018fe）。依据：用户 2026-09-29 拍板
> （B0 收口后新增批：>1000 行文件合理拆分，纯搬移零行为）。
> 纪律沿用 [b0-slices.md](b0-slices.md) §3 统一验收口径；本文是 B1 的拆分映射
> 与零行为证明口径的唯一规格。

## 0. 目标与总原则

- **纯搬移零行为**：只做文件级拆分与机械可见性调整，不改任何逻辑、签名语义、
  执行顺序、加载时序。每片的 diff 必须能在「函数体逐字节」层面证明等价。
- **最小切面**：每个目标文件拆到全部片段 <1000 行即止，切点选内聚缝，不为拆
  而拆；<1000 行的内聚文件不拆。
- **每片独立走完整批次循环**（见 §5）。

## 1. 范围定夺（2026-09-29 对 HEAD=fb018fe 实测）

| 文件 | 实测行数 | 决策 |
|---|---|---|
| crates/kk-engine/src/settle/creep.rs | 2119 | **拆**（B1-1，按 intent 家族） |
| crates/kk-world/src/place.rs | 1092 | **拆**（B1-2，建造面单切一刀） |
| crates/kk-runner/src/prelude/30_game/creep.js | 1043 | **拆**（B1-3，有序段×2） |
| crates/kk-runner/src/prelude/30_game/structures.js | 968 | **拆**（B1-3；用户点名文件，实测略低于 1000 仍按拍板执行，有序段×2） |
| crates/kk-engine/tests/settle_tests.rs | 6906 | **拆**（B1-4，mod 子模块连续切块不重排；实测 7 块即满足 <1000/块，无需 8 块） |
| crates/kk-runner/tests/sandbox_tests.rs | 3328 | **拆**（B1-5，mod 子模块，保顶层唯一 #[test] 串行链） |
| crates/kk-runtime/tests/embed_tests.rs | 1963 | **拆**（B1-5，同上） |
| crates/kk-engine/src/settle/mod.rs | 779 | **不拆**：<1000 且内聚（Working/Engine/settle_room 是结算核心同体） |
| crates/kk-schema/src/room.rs | 1116 | **范围外登记**：>1000 但不在用户拍板清单；schema 声明面（struct/enum 长表），拆分收益存疑——B1 收口时随其他遗留一并回用户决策 |
| crates/kk-world/src/world_json.rs / snapshot.rs | 1617 / 1106 | 不动（归 M2B 前置批宏化，用户既定决策） |
| crates/kk-runner/assets/lodash-3.10.1.js | 12351 | 不拆（资产，用户既定决策） |
| compat/fixtures/**（overmind-dist/tooangel 等） | — | 不动（外部 bot 生态夹具，非我方源码） |

其余全仓 rs/js 无 >1000 行文件（kk-engine/src/intent.rs 916、structures.js 之外
的 prelude 段全部 <900）。

## 2. 机械调整白名单（每片 diff 允许出现的全部改动类型）

任何不在白名单内的改动 = 违规，评审按此判：

**Rust 源码/测试拆分**：
1. 新增 `mod x;` 声明与新文件（文件头注释段）；
2. `use` 行按需增删（子模块各自取最小导入集；mod.rs 保留原 use 供自身与
   re-export）；
3. 可见性前缀的最小调整：家族子模块内被 mod.rs 调度器调用的 `fn intent_*`
   加 `pub(super)`；原文件里 `pub(super)` 项随层级加深须改 `pub(in crate::settle)`
   等等值扩围（**语义等价**：保持原可见域不变）；
4. 对外 API 路径不变的 re-export：`pub use` 于 creep/mod.rs；
5. 注释中的文件路径/段号随搬移更新（不改注释语义）。

**JS 段拆分**：
1. 新段文件的 IIFE 包装（`(() => { 'use strict'; const gw = globalThis.__kkp.gw;`
   开头 + `})();` 结尾）与段头注释；
2. 跨段共享项的定义点挂 `gw.X = X`、消费点改经 `gw.X`（既有 A3a 形态）；
3. PRELUDE_SEGMENTS 注册表在保持既有条目**相对顺序**的前提下于正确位插条目
   （数组长度字面量同步改）；sandbox.rs:36-39 与 _registry.js:4-5 的段序注释
   同步更新；
4. 闭包 `let` per-tick 状态与其全部用户留在同一段（structures.js 的
   `createdCreepNames`/`deprecatedShown` 与 `gw.perTickReset` 钩子注册随
   spawn 族段走，**零接线变更**）。

**测试拆分**：
1. 子模块文件 `use super::*;` 一行（Fixture/helpers 留 root，私有项对子模块
   天然可见）；
2. 被链/外部引用的子模块项加 `pub(super)`（sandbox_tests/embed_tests）；
   settle_tests 子模块内测试保持 `#[test]` 原样（纯 Rust 并行不受影响）；
3. root 的 `use m1::*;` 等聚合，使串行链函数体逐字节不动。

## 3. 拆分映射表

### 3.1 B1-1：settle/creep.rs（2119）→ settle/creep/ 家族模块

| 新文件 | 收编函数（原行号） | 约行数 |
|---|---|---|
| creep/mod.rs | 头部与 use、mod 声明、`pub use` re-export、`process_creep_intents` 调度器（18-59）、共享底仓 helpers：`set_action_log`（1980）、`store_slot/_add/_sub`（1998-2026）、`creep_slot`（2027）、`slots_add/_sub`（2033-2086）、`store_capacity`（2087）、`body_parts_kinds`（2115） | ~250 |
| creep/movement.rs | `direction_from_delta`（60）、`intent_move`（74）、`creep_tick`（1611）、`movement_execute`（1702） | ~340 |
| creep/harvest.rs | `intent_harvest`（156）、`intent_harvest_source`（178）、`intent_harvest_mineral`（244）、`extractor_active`（334）、`harvest_power`（1985） | ~210 |
| creep/build.rs | `intent_build`（348）、`complete_construction`（465） | ~250 |
| creep/controller.rs | `intent_upgrade_controller`（597）、`controller_target`（1262）、`intent_claim_controller`（1279）、`intent_reserve_controller`（1337）、`intent_attack_controller`（1397）、`intent_sign_controller`（1486）、`intent_generate_safe_mode`（1521）、`controller_levels`（2097）、`controller_downgrade`（2106） | ~470 |
| creep/store.rs | `intent_transfer`（711）、`intent_withdraw`（802）、`intent_drop`（880）、`intent_drop_amount`（900，pub(super)→pub(in crate::settle)）、`intent_drop_amount_res`（909，同上）、`intent_pickup`（926） | ~330 |
| creep/combat.rs | `intent_attack`（978）、`blocked_by_safe_mode`（1030）、`intent_repair`（1037）、`intent_dismantle`（1113）、`intent_heal`（1179） | ~290 |
| creep/lifecycle.rs | `intent_say`（1555）、`intent_suicide`（1579）、`intent_pull`（1591）、`die_creep`（1793）、`die_creep_with_rate`（1797）、`TombStore`（1909-1979） | ~330 |

依据（2026-09-29 实测的跨家族依赖，决定归属）：
- `slots_add` 被 harvest（311）与 store 族共用 → 共享底仓（mod.rs，私有项对
  子模块天然可见，零可见性改动）；`store_capacity` 同（188/246 harvest，744/811/933 store）；
- `set_action_log` 被 harvest/build/controller/combat 多族用 → 共享底仓；
- `controller_levels/_downgrade` 仅 upgrade 用（663/671/683）→ controller.rs；
- `controller_target` 仅 controller 族用 → controller.rs；`blocked_by_safe_mode`
  仅 combat 族用（1018/1143/1226）→ combat.rs；`extractor_active`/`harvest_power`
  仅 harvest 用 → harvest.rs；`TombStore` 仅 die 族用 → lifecycle.rs；
- `intent_drop_amount/_res` 被 harvest（228/315）与 damage.rs:250（`super::creep::
  intent_drop_amount_res` 限定路径）用 → store.rs + 可见性 `pub(in crate::settle)`
  （原 pub(super) 语义等价扩围）+ mod.rs `pub(in crate::settle) use` 保 _res 外部路径；
  **登记（评审 F3）**：`intent_drop_amount` 无树外消费者，未挂 re-export——路径
  `crate::settle::creep::intent_drop_amount` 随拆分消失（项可见域仍等值保持），
  零行为零编译影响；
- 对外 pub 面（`process_creep_intents`/`creep_tick`/`die_creep`/
  `die_creep_with_rate`/`store_capacity` 等被 settle 树外或树内引用的项）经
  mod.rs `pub use` 保持 `crate::settle::creep::X` 路径不变。

### 3.2 B1-2：kk-world place.rs（1092）→ place.rs + place_structure.rs

| 新文件 | 收编（原行号） | 约行数 |
|---|---|---|
| place.rs | `PlaceCtx`、`normalize` 调度器、共享 helpers（`generic_fields`/`generic_from_spec`/`read_pos`/`spec_str`/`store_energy`/`store_with_extra`/`store_extra_of`/`saturating_u32`）、`normalize_creep`/`normalize_spawn`/`normalize_flag`/`normalize_resource`/`normalize_tombstone`/`normalize_ruin` | ~800 |
| place_structure.rs | `normalize_structure`（241-476）、`generic_structure_hits`（477）、`normalize_site`（502）——建造面（结构/工地归一） | ~290 |

单切一刀理由：normalize_structure 是 236 行的 structureType 大匹配，与周边
零耦合（只用共享 helpers 与 normalize_spawn 分发位）；一刀即把两文件都带到
<1000，是最小切面。`normalize_structure`/`normalize_site` 加 `pub(super)`（或
`pub(in crate::world)` 等值域——实现时按实际引用面取最小），place.rs 调度器
调用点不变。

### 3.3 B1-3：prelude creep.js（1043）与 structures.js（968）各拆 2 段

**creep.js（段 4）→ creep.js（4a）+ creep_resources.js（4b，新段插注册表
creep 条目之后）**：
- 4a 收编（原 1-523 行）：段头、OFFSETS_BY_DIRECTION、isObjectLike、
  fetchXYArguments、deserializePath（含 gw/Room 静态挂载）、freezeConst+C、
  countActiveParts、blockedBySafeMode、carriedEnergy、字段表+memory getter、
  动作方法族（move/moveByPath/moveTo/harvest/build/repair/dismantle/
  upgradeController/attack/ranged*/heal/rangedHeal，原 261-523 段全部；
  say/suicide/pull 三定义在原 524-1042 段故随 4b）；
- 4b 收编（原 524-1043 行）：资源族四方法（transfer/withdraw/drop/pickup 及
  其段内捕获与 capacity helpers 524-852）、controller 族（claim/reserve/
  attackController/signController/generateSafeMode 853-1038）、CREEP_METHODS
  残余桩循环（1033）与 creep 装配（1039）；
- **跨段共享**（定义点 gw.X 桥、消费段头 const 绑定同一对象——调用点保持
  裸名逐字节不动）：C 表整对象桥（`gw.__creepC`，零重复捕获、对象同一性
  精确保持）、`countActiveParts`、`blockedBySafeMode`；CREEP_METHODS 表、
  mathPow/__Set 捕获、calcNeededGcl/gclPoints/myClaimedRoomCount 整体随
  controller 族落 4b；
- 4b 头部重新捕获 `const gw = globalThis.__kkp.gw;`；段名
  `kk-prelude-30-game-creep-resources`。

**structures.js（段 5）→ structures.js（5a）+ structures_spawn.js（5b，插
structures 条目之后）**：
- 5a 收编（原 1-469 行）：预置捕获、freezeConst+C、sumValues、
  capacityForResource、Store 类、makeStore（既有 gw.makeStore 导出不动）、
  structureTypeOf、checkStructureAgainstController、now、原型字段表、
  isActive（A6 真化段）；deprecated/assertTargetObject/isKnownCreep/
  calcCreepCost/calcEnergyAvailable 与 per-tick 状态（createdCreepNames/
  deprecatedShown+perTickReset 钩子）实测用户全在 5b——整体随 5b（闭包自洽
  零桥）；
- 5b 收编（原 470-968 行）：Spawning 类、StructureSpawn 字段表、spawn 方法族、
  notifyWhenAttacked、原型方法、controller 真方法、装配、Structure.destroy、
  Tombstone/Ruin 接线；`createdCreepNames`/`deprecatedShown` 闭包与
  `gw.perTickReset` 钩子注册**随 5b**（用户全在 5b：spawnCreep 586/626、
  deprecated 调用面实现时核实，若 5a 也有调用则 deprecated 经 gw 桥）；
- 跨段共享：C 表整对象桥（`gw.__structuresC`）、now（`gw.now`）、Store
  （既有 `gw.Store`）、makeStore（既有 `gw.makeStore` 调用形零改动）；
  capacityForResource/checkStructureAgainstController 实测 5b 零使用不桥。

**注册表序**（新序，相对序不变）：… → creep(4a) → **creep_resources(4b)** →
structures(5a) → **structures_spawn(5b)** → sites-flags → resources →
path-finder → game。段总数 13→15；sandbox.rs:36-39「九段」注释与
_registry.js:4-5 段序注释同步。

### 3.4 B1-4：settle_tests.rs（6906）→ mod 子模块（连续切分不重排；实测 7 块）

- root 保留：use 块、`terrain_with`/`body`/`move_intent` 等 helpers（22-305）、
  `Fixture` struct + `impl`、`spawn_with_job`/`chick_of`/`generic_tower`/
  `extension_at`/`generic_structure`/`attack_intent*`/`creep_parts`/
  `heal_intent*`/`ctl_of`/`controller_action`/`controller_intent` 等共享
  helper fn（各域测试都用）、mod 声明与 `use m*::*`（若 root 无聚合需求则免）；
- 测试按**原文顺序连续切块**到 8 个子模块（切点选测试间自然边界，每块
  <1000 行；不重排、不合并、不改一个测试的字节），子模块名按块内主域命名
  （如 movement_store / combat / controller / mineral_misc 等，实现时定），
  每文件头 `use super::*;`；
- **142 个 #[test] 原样保留**（纯 Rust 测试无 V8，并行安全——串行链不变量
  不适用于本文件）；计数与顺序（原文序）为验收断言。

### 3.5 B1-5：sandbox_tests.rs（3328）+ embed_tests.rs（1963）→ mod 子模块保串行链

- **不变量**：两文件顶层各自**唯一一个 #[test]**（`m0_sandbox_suite` /
  `embed_engine_suite`）——V8 单测试串行纪律（ADR-013 线程局部销毁序 +
  进程级终局计数器不可并行污染），破坏会出现假违规 flake（历史实测 5 跑 2 挂）；
- root 保留：helpers（sandbox：fixtures_dir/world/load_sample_bot/tick/
  is_allowed_global 五件——真跨块共享仅 world/tick，其余为一致性留根；
  embed 原文件除套件外无 helper）、唯一 #[test] 链函数、mod 声明 +
  `use m1::*; …`；
- 其余 impl fn 按原顺序连续切块到子模块（sandbox ×4 ≈800/块、embed ×3 ≈620/
  块），每块 `use super::*;` 头 + 被 root 链调用的 fn 加 `pub(super)`；
  **链函数体逐字节不动**（链内调用序原样——原文中链调用序本≠声明序，两者都保持不变）；
- 搬移的 impl fn 上方 doc 注释（含「不得独立 #[test]」纪律注释）随体搬移。

## 4. 零行为证明口径（评审与自查共用）

1. **函数体逐字节对照**：提供对账脚本（python，逐文件独立读写）：
   - Rust：从 `git show HEAD^:path` 提取每个 `fn X` 块（签名行到匹配闭括号），
     与新文件集内同名块 diff——预期差异仅白名单第 3 条的可见性前缀；
   - JS：提取每个 function 定义/原型挂载块，同上——预期差异仅 gw 桥行；
   - 测试：fn 块 diff + `#[test]` 计数断言（142 / 1 / 1）+ 链调用序断言；
2. **注册表顺序**：diff PRELUDE_SEGMENTS——旧 13 条相对序保持、新 2 条插入位
   正确（creep 后 / structures 后）；
3. **行为面**：`cargo test --workspace` 全绿（workspace 覆盖 settle/sandbox/
   embed 全部语义）→ 显式 cd 主仓 `cargo build --release` 后棘轮零未豁免转红
   （compat 套件是官方语义对照的行为面）→ bot 三基线（tooangel 2000t console
   零错人工确认 / overmind-dist 2000t / wasm-bot 10t）；
4. **串行链不变量**（B1-5 专项）：grep 断言两文件 #[test] 计数=1、链内调用与
   搬移前逐字节一致；
5. perf-probe 不在本批关卡（纯搬移不触 tick 热路径；B1-3 的段加载差异仅沙箱
   创建期一次性 +2 次 execute_script，bot 基线墙钟可观察，无回归面）。

## 5. 切片与验收统一口径

| 片 | 内容 | 主落点 |
|---|---|---|
| B1-1 | settle/creep.rs → creep/ 八模块（§3.1 映射） | kk-engine/src/settle/creep/ |
| B1-2 | place.rs → place.rs + place_structure.rs（§3.2） | kk-world/src/ |
| B1-3 | creep.js/structures.js 各拆 2 段+注册表插条目（§3.3） | kk-runner prelude + sandbox.rs |
| B1-4 | settle_tests.rs mod 化×8（§3.4） | kk-engine/tests/ |
| B1-5 | sandbox_tests/embed_tests mod 化保串行链（§3.5） | kk-runner/tests/、kk-runtime/tests/ |

每片：实现 → 对账脚本 PASS → `cargo test --workspace` 全绿 → 显式 cd 主仓
`cargo build --release` → `node compat/scripts/ratchet.mjs` 零未豁免转红 →
bot 三基线 → ≥3 路 subagent 并行评审（**重点视角=零行为差证明**：搬移前后
产物逐字节对照 / 段注册表顺序与加载期捕获纪律 / mod 拆分后测试归属与串行
链）→ 逐条清偿+复审（复审复核评审共识本身）→ 沿革追加 → `pnpm run check`
→ 单一归因提交（subject ≤350 字符）并 push。

## 6. 风险与已知坑（本批专项）

- **可见性陷阱**：`pub(super)` 随项下沉一层后可见域收窄（原=settle 树，
  新=creep 树）——凡被 settle 树内兄弟模块引用的项（如 intent_drop_amount_res
  被 economy.rs 用）须 `pub(in crate::settle)` 等值扩围或经 mod.rs re-export；
  编译器会兜底，但 re-export 漏项会在别的 crate 爆——`pub use` 清单以 §3.1
  对外 pub 面为准；
- **JS 段拆的加载期捕获**：新段内的内建捕获（freezeConst 时机、mathPow/__Set、
  Reflect.apply 族）保持**加载期**形态；段加载序=注册表序，插条目位即插加载
  位，4b/5b 捕获发生在 4a/5a 之后、sites-flags 之前——与原单文件内语句序
  等价（同 tick 同步执行，无可观测差）；
- **perTickReset 钩子**：structures.js 拆段后钩子注册随状态走，注册顺序在
  钩子数组内 +0（5b 在 game.js 注册之前加载）——钩子数组执行序不变；
- **测试大文件**：settle_tests 子模块**保留 #[test]**（142 计数不变），
  sandbox/embed 子模块**绝不允许出现第二个 #[test]**（含 cfg(test) 形态）；
- **Bash cwd 跨调用持久**（本批勘测已实测复发）：跑套件前显式 cd 主仓；
  zsh `===X===` 分隔符会当 glob 报错（已实测复发）；
- **commit subject ≤350 字符**（hook 拒绝）；ratchet 前必须 release 重建
  （旧二进制假零翻转）。

## 7. 切片执行记录

### B1-1（settle/creep.rs → creep/ 八模块）——已收口

- 实测：mod.rs 231 / movement 284 / harvest 219 / build 262 / controller 435 / store 286 / combat 294 / lifecycle 247 行，全部 <1000。
- 对账：58 顶层项（56 fn + TombStore struct/impl + 10 ACTION_* const）零缺失零重复，
  函数体可见性前缀归一后逐字节一致；唯一 rustfmt 重排 = intent_drop_amount_res
  签名换行+尾逗号（前缀变长超 100 列）。三路评审各自独立重做对账均 PASS。
- 评审清偿：P0=棘轮重跑再生 kkscreeps.json（逐用例 status 零差异，已 checkout
  还原，不随片入库）；P3=调度器前置 doc 注释被发射脚本丢一行（已补回）+
  方案文档 intent_drop_amount 树外消费者误写 economy.rs（实为 damage.rs:250，
  已订正）；F3 登记=intent_drop_amount 无 re-export（见 §3.1 注）。
- 评审 P2「bot 报告早于拆分」取证复核不成立：评审所据 compat/reports/bot-*.json
  是旧产物（本片未动），本片三基线产物在 /tmp（mtime 23:19，晚于全部源码修改
  23:15）；其引用的 mod.rs mtime 23:24:46 系评审 2 为强制重编 touch 源码所留
  （内容未变——评审 1/3 的内容对照在该时刻后仍全过）。为形式闭环，清偿后在
  最终树重跑全链：clippy 零告警 / engine 152 测试全绿 / release+棘轮 1405 零
  转红 / 三基线通过（tooangel console error tick=0；overmind 2000 错 tick 为
  setPublicSegments/stub-flag 既知缺口基线形态）。
- 旧路径注释残留（embed_tests.rs:411/998 → 归 B1-5；settle/movement.rs:6、
  settle/mod.rs:13/15 历史叙事注释 → 不动）。

### B1-2（place.rs → place.rs + place/place_structure.rs）——已收口

- 实测：place.rs 814 / place_structure.rs 302 行，全部 <1000。原 1092 = 683 源码 +
  409 内嵌 tests；搬移块 = 原文 239-520 连续段（normalize_structure 十臂分派 +
  generic_structure_hits 十二臂缺省表 + normalize_site）282 行。
- 对账：19 顶层项零缺失零重复；搬移块唯一差异 = 两签名 pub(super) 前缀 +
  normalize_structure 前缀变长触发 rustfmt 签名换行+尾逗号（B1-1 §7 先例同类
  机械后果）；调度器 normalize 与内嵌 tests 逐字节不动（tests 仅伴生补
  `use kk_schema::room::{Reservation, Sign};` 一行——类型原经顶层 use 引入，
  随搬移修剪后测试体需就地导入）。
- 可见性：normalize_structure/normalize_site pub(super)（恰为调度器调用面）；
  generic_structure_hits 保持 pub 经 place.rs `pub use` 保
  `kk_world::place::generic_structure_hits` 路径（唯一树外消费者 =
  kk-engine settle/creep/build.rs:228）。
- 验证：kk-world 40 测试全绿 / workspace 31 套件全绿 / release+棘轮 1405 零
  转红 / 三基线通过（tooangel console error tick=0；overmind 既知缺口形态）。
- 实得四路评审全 PASS（评审 2 一路通知丢失后重派补位，原路结果随后亦回）零整改；
  LOW 登记：place.rs+place/ 平行文件风格与仓库既有 mod.rs 风格（settle/、
  settle/creep/）并存——§3.2 既定命名，功能无损，后续新增模块须放 place/*.rs
  且不得引入 place/mod.rs（E0761）。P3：embed_tests.rs:608 旧路径注释归 B1-5
  （与 B1-1 登记的 :411/:998 同文件同类）；本段即评审 3 要求的执行记录补记。

### B1-3（prelude creep.js/structures.js 各拆 2 段）——已收口

- 实测：creep.js 486 / creep_resources.js 582 / structures.js 405 /
  structures_spawn.js 593 行，全部 <1000；PRELUDE_SEGMENTS 13→15（新段插
  creep/structures 之后，旧序不变）。
- 对账（三路评审各自独立重做均 PASS）：多重集全行记账（旧文件每行恰出现一
  次，新文件全部行 = 搬移行 + 白名单桥/头/IIFE 包装/空行）+ 搬移区 SHA-256
  哈希逐一比对 + 序保持对齐；perTickReset 钩子全 prelude 恰一次（5b:27）；
  gw 桥五个新键全库恰写一次（4a/5a）恰读一次（4b/5b）。
- 桥清单：gw.__creepC/gw.countActiveParts/gw.blockedBySafeMode（4a→4b）、
  gw.__structuresC/gw.now（5a→5b，另 Store 走既有 gw.Store、makeStore 走既有
  调用形）；消费段头 const 绑定同一对象——调用点保持裸名逐字节不动。
- 评审清偿（全 P3/P4 级，零 P0-P2）：§3.3 四处清单/桥文字订正（本段随附）；
  5a 死捕获 _/__MathCeil/__MathFloor 删除（消费方全迁 5b 的机械后果）；
  game.js:522 与 00_shared.js:5 失效注释路径/段数更新；sandbox.rs 注释措辞
  去冗；4a/5a 段头枚举补全。既有登记：designs/compatibility.md:42/:61 的
  creep.js 行号锚点陈旧（HEAD 前已陈旧，非本片引入）归文档卫生批。
- 验证：workspace 268 全绿 / release+棘轮 1405 零转红 / 三基线通过
  （tooangel console error tick=0；overmind 既知缺口形态）；清偿后重验全链。

### B1-4（settle_tests.rs → crate 根 + 7 子模块）——已收口

- 实测（fmt 后终值）：根 479 + movement_spawn_basics 918 / tombstone_ruin_engine 951 /
  structures_and_attack 940 / combat_heal 958 / controller_safemode 973 /
  misc_and_notify 930 / minerals_and_boost 749 行，全部 <1000。方案写「×8」
  为尺寸推算：7 块即满足每块 <1000，且 heal 族测试带（原 :2921-4088 跨
  1165 行）无论块数必然被切刀劈分——7 块无边界质量损失。
- 结构：crate 根持 use 块+`#[path]` mod 声明×7+16 个共享 helper+Fixture
  struct/impl（helpers 从原交织位归位根尾，§3.4 既定）；子模块头部
  `use super::*;`（根私有项+私有 use 经 glob 对子模块生效，编译零告警
  佐证）。**集成测试根是 crate root，mod 声明默认解析到 tests/ 平级，
  故显式 `#[path = "settle_tests/x.rs"]`**（对照 B1-2 place.rs 是 lib 内
  非根模块走同名子目录解析无需属性）。
- 对账：三路评审独立重做（项级多重集+行级账本+cargo --list 交叉）全过：
  142 个 #[test] fn 体逐字节一致、16 helper+Fixture 逐字节一致、7 块对
  原文测试序是零缝隙零重叠 tiling、同一测试二进制并行/隔离语义零变化
  （无 static/thread_local；测试名仅增模块前缀，全仓无 --exact 调用者）。
- 评审 P3 清偿：①初版发射丢 22 处段落横幅注释共 31 行（attachment 遇
  空行截断所致，含官方对照路径块/_oldHits 证据链/评审溯源标记）——重发
  为 gap-attachment 语义全数补回，行级账本零意外增删；②本段即 §7 补记；
  ③×8→×7 订正（§3.4/§1 两处）。
- 登记（P4）：mod 声明序经 rustfmt reorder_modules 归一为字母序（非原文
  块序）——libtest 执行序本与声明序无关，无行为影响；heal 族三测试落
  controller_safemode 块首（连续切块的必然边界，文件名为近似主域）；
  ghodium_resource_family_settles 落 misc_and_notify 尾。
- 验证：engine 152 全绿（142 集成+10 单元）/ workspace 268 全绿 /
  release+棘轮 1405 零转红 / 三基线通过（tooangel console error tick=0）。

### B1-5（sandbox_tests.rs + embed_tests.rs → 根+子模块保串行链）——已收口

- 实测：sandbox_tests 根 255 + prelude_and_room_forms 837 / watchdog_and_memory
  838 / store_spawn_and_viewer 670 / resource_host_and_nearheap 759（34 impl）；
  embed_tests 根 43 + embed_roundtrip_contract 689 / embed_channels_and_families
  663 / embed_strictness_and_partial_commit 593（18 impl）。全部 <1000。
- 串行链不变量（评审 1 双重口径实证）：两根各恰 1 个顶层 #[test]
  （m0_sandbox_suite/embed_engine_suite），七子全 0（其余 #[test] 字样全在
  注释内）；每 target `-- --list` 恰 1 条；链体（含尾 LIFO 断言注释行）与
  HEAD 逐字节相同；「不得独立 #[test]」纪律注释×3 随体搬移。V8 计数器
  （live_sandbox_count/lifo_violations）在 lib 内未触碰，进程级语义零变化。
- 对账：52 个 impl fn（34+18）剥除 pub(super) 前缀后逐字节相等、零缺失
  零重复；5 helper（fixtures_dir/world/load_sample_bot/tick/is_allowed_global）
  留根逐字节不动——评审 2 事实修正：真跨块共享仅 world/tick，其余三项留根
  属一致性选择（is_allowed_global 为套件级白名单概念位）。
- 评审 P3 清偿：chunk 命名与内容不相称（源序连续切块的必然）——5 文件改名
  名实相符（prelude_and_room_forms/store_spawn_and_viewer/
  resource_host_and_nearheap/embed_channels_and_families/
  embed_strictness_and_partial_commit）；本段即 §7 补记。
- 登记：源序连续切块致链调用序 ≠ 块内源序（链体不动故无影响）；根 glob
  私有绑定使子模块理论上可互调 sibling 块 fn——实测零跨块调用，当前调用面
  恰=根串行链（评审 2 观察）。
- B1-1/2/3 登记的 embed_tests 旧路径注释随块迁内容原样（纯搬移纪律），新锚点：
  embed_roundtrip_contract.rs:384（原 :411，指 settle/creep.rs 的 gcl 强化——现
  真路径 settle/creep/controller.rs）、embed_channels_and_families.rs:286（原
  :998，extension_cap——现 settle/creep/store.rs）、embed_roundtrip_contract.rs:581
  （原 :608，place.rs 预挂钟——现 place/place_structure.rs）；评审 3 F4 判定
  sandbox_tests 内 structures.js 行号锚点为官方上游源锚点（编号空间 >968），
  B1-3 拆分未使其失效，与 compatibility.md:42/:61 同归文档卫生批。mod 声明序
  经 rustfmt reorder_modules 归一字母序（B1-4 同款登记，无行为影响）。
- 验证：两 crate 套件真跑全绿（m0 1.74s/embed 0.89s，LIFO 零违规断言过）/
  workspace 268 全绿 / release+棘轮 1405 零转红 / 三基线通过。
