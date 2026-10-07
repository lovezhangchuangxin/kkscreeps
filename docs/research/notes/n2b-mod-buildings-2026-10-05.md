# N2b Mod 建筑交付记录（2026-10-05）

基线 f84b130（N4a）。本批三个提交：声明域（`api.buildings.define` 冻结目录）、引擎消费面（工地/建造/完工/伤害/维修/拆解/寻路/生产）、玩家面与官方内容包。验收环境与命令如下；数字取自实际运行输出。

## 提交与范围

| 提交 | 范围 |
|---|---|
| N2b-1 | kk-mod-api `buildings` 域（描述符/目录/PlanHash/组合报告 v4）、SDK 与 op 转换、冻结语义测试 |
| N2b-2 | Plane/Engine/Movement 装配目录；工地创建、多资源累计支付、完工 Generic、hitsMax/维修/拆除/销毁门、寻路语义、生产周期、场景放置默认值；`mod_building_family` 11 项 |
| N2b-3 | ModPlayerApi buildings 清单与 41 段绑定、预检白名单/配额跳过/互斥/代价矩阵/结构注册/类装配；官方包 smelter+panel；sim-mod-buildings 三场景；CLI 测试 4 项；事务边界测试 2 项 |

## 机器验证

- `cargo test --workspace`：**844 通过 / 0 失败**（基线 809 + 本批新增 35：冻结 10、mod-host 注册声明 1、mod-api 单元 2、结算 13、事务 3、CLI 4、玩家面绑定 1、评审清偿追加 1）。
- 三路评审（Rust 数据面 / 玩家面 JS / 官方包与验收证据）全部清偿：dismantle 玩家预检并目录类型、bind 命名空间归属门、拆解回收分母改成本总量（总值=伤害/200 与内建一致）、生产容量可行性冻结门、boost 支付分歧文档化+锁定测试、超限工地回拨测试、玩家路径 dismantle/异常不发布候选/空载预检拒绝测试、三处弱断言强化。
- 三场景人工核对（`target/debug/kkscreeps sim run`）：
  - `scenario.json`（320 tick）：`grid_foundry=255`、`foundry_hits=5000`（受击 6 次后维修回满）、`foundry_ingots=40`、`storage_ingots=100`（产物真实搬回仓库）、`foundry_owner=p1`。
  - `scenario-combined.json`（320 tick）：另得 `grid_panel=1`、`panel_energy=200`（免费生产至容量停止）、`panel_hits=300`；smelter 各值不回归；组合报告 `buildings=[panel:collector, smelter:foundry]`、selection `[panel, smelter]`。
  - `scenario-plain.json`（8 tick）：`site_rejected / withdraw_rejected / no_sites / no_foundry` 全真，世界仅存内建对象。
- 双通道全等：`sim_tests/buildings.rs` 内 JSON/KKPV 两份报告 `final.world`、`plan_hash` 逐项相等（主场景与组合场景）。
- 检查点：`cold_resume_matches_uninterrupted_building_world` 以真实子进程 170 tick 保存退出、恢复续跑 150 tick，最终世界（含建筑/工地/库存）与一次跑完 320 tick 的参考全等（仅剥离实测 CPU 桶）。
- 事务边界：建造槽 after 拒绝后工地进度/多资源支付/事件/结构插入整体回滚；`fail_next_writeback(RawMemory)` 后缓存重试，生产恰好一次、`Memory.n=1`（玩家代码不重复执行）。

## 关键实现决定

1. **载体选 Generic 结构**：结构类型本是字符串，工地/受击/废墟/投影/FIND 对字符串天然开放——阻挡点集中在成本白名单、hitsMax 表、维修/拆除门、寻路表、transfer 白名单与 RCL 配额，逐一按目录放开。`RoomObject::Mod` 保持对象域语义不动。
2. **多资源支付锚定累计比例** `floor(amount·p/total)`：单资源成本严格退化为内建 1:1（无 boost 时）；boost 语义与内建有意的分歧——支付恒锚定进度比例（内建能耗按未强化效果），复刻内建口径需逐工地付款账本（新快照字段），不值得。
3. **生产是宿主原生周期**（`time % interval`，无相位状态）：检查点恢复零额外状态；玩法差异（配方/周期/寻路语义）全部由声明表达，第二个包零 Rust 改动。
4. 玩家类必须 `api.buildings.bind`（finalize 强制），推荐继承 `OwnedStructure` 取得 `owner`/`my`。

## 已知边界

登记在[架构文档](../../architecture/mod-buildings.md#当前边界)：逐级 RCL 配额、逐资源容量、建筑自定义字段/tick、生产事件、建筑专属 FIND 常量未开放；市场/跨房对声明资源维持 N2a 拒绝。D 批遗留（link tick 拒绝、attackController、D7 性能）不变。

## 后续

N2c 行为策略（具名伤害公式/移动仲裁替换入口）；建筑 RCL 配额与生产事件按真实包需要开放。

## N2b review 修复记录

基线 `04bfc14`。本批清偿独立 review 发现的三个问题，现行接口见[建筑契约](../../architecture/mod-buildings.md)。未扩大建筑声明域或新增持久字段。

### 修复范围

- 将 `sim-mod-buildings` 的三个场景、公共 bot 和 README 纳入 Git，增加固定夹具目录的忽略例外，修正 README 的运行轮数为实际验收的 320 tick。此前本机遗留目录掩盖了干净检出时的测试和文档链接失败。
- 生产容量按本次转换的最终库存 `used - consumed + produced` 判断。全部输入足量和最终容量合法才写入；满仓原料可转换，同资源输入/输出按净变化计算，最终溢出时不扣输入。
- 出生的指定出口与其他出口检查共用当前冻结建筑目录。Mod 障碍建筑和任何主人的障碍工地阻挡出生；道路/普通建筑仍可通行。保留“未选方向有空位时延期”和“完全被围时踩死选中格敌人”的原生区别，不复用普通移动只挡自家工地的判断。

### 回归与验证

新增六项引擎回归覆盖满仓输入转换、同资源输入输出、最终库存恰好满仓与溢出拒绝、双方工地和已建障碍、道路/普通出口、未选出口的障碍及出生踩死行为。修复前六项全部失败，修复后与原建筑测试一起通过。

公开 CLI 最小场景在 JSON/KKPV 下复验生产和真实 spawnCreep 出生。另从待提交 Git 树导出干净源码，检查文档链接并运行已纳入版本控制的三个建筑场景，避免再次依赖本机忽略文件。完整检查、release 身份、逐用例兼容性比较与最终统计见[机器记录](n2b-review-fixes-2026-10-05.json)。

沿用现有检查点与恢复契约；本批不增加迁移、回放、生产事件或建筑自定义生命周期。

## N2b 资源向量质量修复（2026-10-05）

基于 `44c73a4` 的代码质量复查确认：公开 Rust 贡献可构造重复资源的成本、生产输入和输出；JS SDK 的对象格式虽不产生重复键，冻结层仍不能依赖装载器保证该不变量。

### 复现和修复

- 最小 Rust 程序使用 `freeze_with_builtin`、`Engine::install_plan` 和一次空意图 tick：两项 `energy:50` 输入、`H:1` 输出、容量 100、实际 energy 50，旧实现提交 `{"H":1,"energy":-50}`。
- 重复成本也被旧冻结层接受；相同资源向量仅改变排列就改变 PlanHash。
- 冻结前对三类资源向量分别排序，冻结校验分别拒绝重复资源名。排序在贡献副本和冻结建筑目录形成之前执行，身份与消费者因此同源。输入与输出之间共享资源仍合法，按既有净变化语义生产。
- 回归覆盖三类非相邻重复项、三类向量重排的身份/目录等价，以及数量变化继续改变身份。两个新测试在修复前失败，修复后通过。
- 建筑冷恢复测试改为比较报告的完整 `final`（包括全部玩家 Memory/raw_memory/GCL/房间及对象读面），只排除实际线程耗时导致变化的 CPU 桶。该报告比较不替代检查点自身对全部数据域的验证。
- 删除一处测试未使用变量，恢复 Clippy 无警告。

### 验证

- `cargo test -p kk-mod-api --test freeze buildings::`：11 通过。
- `cargo test -p kk-cli cold_resume_matches_uninterrupted_building_world`：真实 CLI 冷恢复在 JSON/KKPV 两通道通过。
- 显式选择重建 debug CLI 的 `pnpm run check`：Rust 852 通过、0 失败、12 个已登记忽略；JavaScript 14 通过；Clippy 无警告；源码规模检查通过。
- 按固定 pin 准备 screeps-ok，重建 release CLI 和适配器，显式选择 release 运行 `node compat/scripts/ratchet.mjs`：2914 总用例、1918 通过、89 个既有失败、907 跳过，门禁通过；与 `44c73a4` 的逐用例状态无变化。两项既有未命中豁免警告保留。
- release CLI SHA-256：`f1a968ddff2d2396f4037921ff4f60d9013349eeedd06f53d1ba1f2ed9ff20ee`。机器报告更新到 `compat/reports/kkscreeps.json`。

原 JS 装载器已经排序资源表，所以合法 JS 包的冻结身份保持既有语义。直接 Rust 贡献中此前未排序的向量现在使用规范身份；依赖旧非规范身份的外部检查点需按严格身份核验处理，不绕过恢复拒绝。

现行约束见[可建造建筑](../../architecture/mod-buildings.md)。
