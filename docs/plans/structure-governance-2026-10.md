# 结构治理 S 系列（2026-10-07 登记）

对全仓模块结构的一次性深度治理：修正「1000 行门槛成为拆分唯一触发器」「批次号编码进文件名」「测试根 `#[path]` 仪式」等结构性问题。本文件是批次拆分与阶段验收的执行入口，进度状态以[台账](mod-next-batches.md)为准；历史决策 [b1-structure-governance.md](b1-structure-governance.md) 中「集成测试根须显式 `#[path]`」的结论自 S01 起被 `tests/<name>/main.rs` 形态取代，历史文档不追改。

## 病因诊断（2026-10-07 三路盘点结论）

1. **拆分由行数驱动而非职责驱动**：门禁脚本实测 39 个文件超过 700 行贴线堆叠，多次在批次内被迫应急拆分；`misc_and_notify.rs`（19 测试、至少 7 个域）与 `structures_and_attack.rs` 根（13 测试、5 个域）是垃圾抽屉。
2. **批次号编码进永久文件名**：settle_tests 的 `m2b_*`/`m2b2_*`/`c3_*`、embed_tests 的 `m2b_*`/`c3_*`/`c4_*`/`c_mod_*`；同域测试散落多文件（safemode×5 处、movement×6 处、resources×5 处）。
3. **测试根 `#[path]` 仪式**：7 个测试根共 138 个 `#[path]` 属性（含嵌套族文件合计 157 处计数）。cargo 原生支持 `tests/<name>/main.rs` 形态，mod 声明自动解析同目录。
4. **模块声明风格混用**：全仓配对式（`xxx.rs + xxx/`）50+ 处，`mod.rs` 仅 4 处，`settle/creep/mod.rs` 是 settle 子模块中唯一的 mod.rs 形态（目录根 `settle/mod.rs` 除外）；测试挂接一半用冗余 `#[path]`。
5. **贴线大文件未预防性拆分**：`working.rs` 910 行（单 impl 块 790 行）、`world.rs` 995（内联测试 482 行）、prelude `room.js` 996（`makeRoom` 闭包工厂约 700 行）等。

## 治理总原则（所有批次共守）

- **纯移动优先**；仅三处已批准的小改：prelude 闭包提升（S05）、`convert.rs` 臂提取（S04）、`sim.rs` execute 段提取（S04）。
- **每批独立绿**：受影响 crate `cargo test` + 无参数 `pnpm run check` 完整门禁；测试注册数量不变（settle 301、mod_chain 269、sim 84、freeze 48、register 66、embed 1 个串行套件入口、sandbox 2）；`m0_sandbox_suite`/`embed_engine_suite` 串行单入口保持不动（进程级 LIFO 计数器约束，历史 flake 证据见 compat/reports/a5-lite-wiring.md）。
- **每批一个提交**（S02 可拆 engine/runtime 两笔），提交由用户拍板，不自动 push。
- 排除项：`.worktrees/`（renderer 旧副本，将来合并会有冲突，接受）、生成物 `kk-core/src/constants.rs`、lodash、spikes。
- 行数只是复审信号不是拆分理由；批次号不进文件名（git log 已承载历史）；测试根统一 `main.rs` 形态。

## 批次拆分

### S01 测试根 main.rs 化 + 冗余属性清理 + 门禁预警档（机械层，零语义变化）

1. 七个测试根转 `tests/<name>/main.rs`：settle_tests（删根 24 个 `#[path]`）、sandbox_tests（8）、embed_tests（21）、mod_chain_tests（41）、sim_tests（30，根内 4 个 `#[test]` 随迁）、freeze（`freeze_cases/` 同步改名 `freeze/`，7）、register_tests（7）。
2. 嵌套族文件的 `#[path]` 一并删（settle 树 13、embed 树 4、sandbox 树 2）：族文件成为普通模块文件后 `mod xxx;` 自动解析。
3. 源内冗余 `#[path]` 挂接清理共 11 处（settle 域七处 + 同型四处：kk-mod-api typegen×2、kk-mod-host realm、kk-runtime embed/checkpoint）。
4. `scripts/check-source-size.mjs` 增加 >700 行警告档（不挡门）。
5. 修活文档链；同步 source-organization.md 测试组织段与 AGENTS.md 门槛句（mod_chain_tests/README.md 复核后无需改动）。
6. 验收：`pnpm run check` 全绿；`#[path]` 在 `crates/*/tests` 归零；七个目标测试计数与基线一致（301/2/1/269/84/48/66）。

### S02 测试按域归并（settle_tests + embed_tests 批次号清理与抽屉拆散）

- settle_tests 24 顶层文件 → 约 16 个域文件（301 测试不变）：`misc_and_notify.rs` 拆散至 resources/controller_safemode/engine_world（新，吸收 structures_and_attack/ownership.rs）/controller/creep_actions/movement/notify/minerals_and_boost；`structures_and_attack.rs` 根拆散至 creation/movement/structure_lifecycle/spawn；批次号改名（m2b_creation_family→creation、m2b_structure_actions_family→tower_link_actions、m2b_structure_decay_family→structure_lifecycle、m2b2_terminal_market_family→terminal_market、m2b_contrib_link_family→contrib_link、m2b_extension_family+c3_ext_intent_family→`ext/`）；building_lifecycle+mod_building_family→`mod_buildings/`；movement 域归拢；combat 域归拢为 `combat/`。controller_safemode 吸收后若近 1000 行则自分子目录。
- embed_tests 21 文件 → 约 14 个域文件（95 场景归位、串行单入口不动）：m2b_*/m2b2_*×5 按域改名；m2b_creep_tails/m2b_map_notify_cpu/m2b_closing 抽屉拆散；c_mod_settle→mod_settle、c3_counter_chain→ext_intent_counter、c4_player_api→mod_player_api；embed_channels_and_families 转发壳压平为 `channels/`。
- 顺带：sandbox 域 `prelude_and_room_forms.rs`（847 行）拆成 prelude_forms.rs + room_forms.rs（S05 回归网）。
- 文档同步：compat/fixtures 反引号引用、docs 内引用改名文件的段落。验收：三 crate 测试计数不变 + `pnpm run check`。

### S03 kk-engine 源码重组

1. `working.rs`（910 行）拆分：字段全部保持扁平（crate 外字面量构造必须仍编译）；`working.rs` 留 Plane/Working/RoomCtx + 核心 impl；`working/chain_snapshot.rs`（约 300 行，字段不变量文档随迁）、`working/drops.rs`、`working/move_slots.rs`（L787-901 桥接族）、`working/extensions.rs`、`working/mod_objects.rs`。子模块名避开 settle 同名模块；私有方法 `pub(super)` 放宽须在提交说明明示。
2. `settle/mod.rs` 的 structure_rcl_prologue + controller_allowed 下沉 controller.rs。
3. `settle/creep/mod.rs` → `creep.rs + creep/` 配对式。
4. kk-mod-host：`realm_lifecycle.rs` → `realm/lifecycle.rs`（删 `#[path]`，零语义）。
- 验收：`cargo test -p kk-engine -p kk-mod-host` + `pnpm run check`；`kk_engine::settle::Working` 公开 API 不变。

### S04 各 crate 大文件预防性拆分（每 crate 一步绿）

- kk-world：world.rs（482 行内联测试外移 + 目录/版本段→world/index.rs）；world_json.rs（object_json→object.rs、可见性→visibility.rs、find 域→find.rs）；place.rs（403 行测试外移）；kkpv/tests.rs 按族分。
- kk-schema：room.rs 428 行测试外移（结构体+宏体系不硬拆）。
- kk-runner：pathfinder.rs（测试外移 + terrain.rs/host.rs）；proto.rs（工作者执行段→proto/worker.rs）；sandbox.rs（end_tick 段→sandbox/end_tick.rs，装配序注释随迁）。
- kk-cli：sim.rs（setup/replay/assertions 分件，报告版本注释随报告装配走）。
- kk-mod-host：convert.rs 值域/schema 解析纯移出 + build_contribution 八臂提取（已批准小改）。
- 验收：逐 crate `cargo test` 后 `pnpm run check`；source-organization.md 导航表逐 crate 同步。

### S05 prelude JS 分件（含批准的闭包提升小改）

1. `room.js`（996 行）→ `30_game/room/`：terrain.js、visual.js、find.js（24 臂整体搬、保持官方 findCacheFn 顺序注释）、look.js、intents.js；`makeRoom` 局部变量提升为外层共享 const、方法段提具名函数；`PRELUDE_SEGMENTS`（host.rs 数组，顺序唯一权威）按原序插条目。
2. `structures_spawn.js`（938 行）按类族分件；共享状态留先加载段或挂 gw（沿用 `_registry.js` 既有模式）。
3. `game.js`（779 行）抽 `game_map.js`（findRoute A* 整体搬）；memoryWorld 桥保持加载序。
4. 验收：逐段拼接产物 diff 语义等价；sandbox 56 场景 + sim 双通道 + `pnpm run check` + release ratchet 零转红兜底。

### S06 治理规则收口与台账关闭

1. source-organization.md 全面改写（治理规则段 + 导航表对齐最终结构 + prelude 加载清单显式段落）。
2. AGENTS.md 门槛句同步 700 预警档；docs/README.md 索引同步。
3. 台账 S 系列收口 + 研究笔记（治理前后 700+/1000+ 行分布对比证据）。
4. 最终验收：无参数 `pnpm run check` + release ratchet + 警告清单复核。

## 风险登记

- `.worktrees/renderer` 含测试旧副本，S01/S02 的移动将来合并 renderer 分支时会冲突——接受，不为其调整方案。
- 归并时 1000 行门禁可能拦截（controller_safemode 吸收后）→ 就地分子目录，不压缩代码。
- 文档链接断链由 check-doc-links 门禁兜住，批内修复。
- prelude 拆分是全方案唯一的运行时行为敏感区 → 逐段 diff + ratchet 双保险。
- `Working` 字段重组为子 struct 明确禁止（破坏 crate 外构造与 227 处直取）。

## 已拍板决策（2026-10-07 用户确认）

1. 串行套件保持单入口形态（m0_sandbox_suite/embed_engine_suite 不动），仅按域重排场景文件。
2. prelude 全拆三个文件（room.js/structures_spawn.js/game.js）。
3. convert.rs 巨型 match 臂提取包含在本次治理内。
