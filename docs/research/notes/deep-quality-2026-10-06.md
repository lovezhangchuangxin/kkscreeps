# 深度质量优化实施与验收（2026-10-06）

开始 HEAD：`229bea4b702590b87d730f3a106b184538c62e5e`，工作区干净。六批按已批准计划推进并全部完成验收，随本批提交（真实提交 ID 见 git log）。保持公共 JS、SDK 入口、检查点字段/格式版本及 PlanHash 语义；没有新增玩法能力或降低兼容性通过数下限。收口于 2026-10-07，接手与冻结细节见[接手说明](../../plans/archive/deep-quality-handoff-2026-10-07.md)。

## 已实施

1. 门禁显式构建当前 debug CLI，通过绝对路径传给 JS；CI 使用同一完整门禁，另跑 doctest 与 release 棘轮。显式二进制不存在时报错，旧 release 不再影响完整门禁。子步骤失败停止后续步骤。
2. ID 分配/创建返回 Result，首次致命错误保存在共享候选中，回退与捕获不能消除。全局 ID/记录键/坐标/游标在发布前验证。
3. Engine 字段私有化、World 受控编辑和整轮房间提交；编辑在开放/恢复期间拒绝，账户与市场编辑推进 generation。Mod 类型及扩展校验与冷恢复共用。
4. Mod 能力枚举携带领域状态，宿主按对象/资源/战斗/建造/移动拆分，能力分类来自 kk-mod-api 固定元数据。SDK 拆分为注册、规则、链、刷新与领域模块，内部模块仅 SDK 静态导入。
5. 每次原生搜索借用不可变 TerrainSet；删除全局地形与加载路径。构建期按唯一锚点生成适配版本，固定上游原文未变；池在正常、异常、取消时解除地形/矩阵/回调指针。运行时缓存同时核对实例身份与地形版本。
6. 第 6 批最终验收（2026-10-07 收口）：cooldown 热点实验五轮无稳定收益撤回（仅替换 `mod_chain.rs` link 分支，模块导出路径修正保留）；正式性能矩阵与长期 RSS 见下。附带发现并修复 quality.py 寻路基准行解析 bug——libtest 在同一行先输出 `test <名称> ... ` 前缀，`startswith` 匹配永不成立，改为子串定位并补工具回归（此前两轮工具回归均未覆盖真实 libtest 输出格式，因正式矩阵从未运行过）。

## 最终验证（全部绑定最终产物）

- 最终源码完整 `pnpm run check` 通过（`/tmp/kks-quality-final2-check.log`）：格式/Clippy/Rust/JS、323 篇 Markdown 1922 个本地链接、592 个源码文件规模门全绿，门禁自身失败路径回归通过。
- `cargo test --workspace --doc` 通过（`/tmp/kks-quality-final2-doctest.log`）。
- 最终 release 棘轮：1918 通过 / 89 既有失败 / 907 跳过（地板 1918），逐用例转红 0、豁免 0、待归因 0；地板与豁免未动。
- 三 bot（final 二进制）：TooAngel 2000/2000、Overmind 2000/2000、WASM 10/10，缺失 API 0，惩罚跳过 0，console-error 0/1/0（Overmind 为既有 colony 初始化异常）。
- 旧 release 检查点双通道恢复（[old-restore.json](deep-quality-evidence-2026-10-06/old-restore.json)）：JSON/KKPV 逐值相等、plan_hash 相等、两玩家续跑到 101，库 SHA 绑定当前 release rlib。

## 性能矩阵与长期 RSS

正式矩阵 150 窗口：1/4/8/32 玩家 × JSON/KKPV、100/400 link × native/empty/chain、寻路四场景；每版五次独立交替运行、500 tick 预热 / 3000 tick 采样；每窗校验负载有效性与两版行为逐值等价。

- 玩家主循环：各规模/通道五轮中位数 ratio 0.96–1.03 双向分布，噪声内持平，无回归。
- Mod 链：native ≈1.00、empty ≈1.00–1.04（p95/p99 均 ≈1.0，p50 波动判定为噪声）、chain ≈1.00；RSS 全持平。重构为结构性收益，运行成本不变。
- 寻路四场景一致收益 4–7.5%：terrain 0.928 / callback_matrix 0.925 / callback_none 0.935 / room_grid 0.959——TerrainSet 借用与运行时地形缓存的回报。
- 长期 RSS（[rss-long.json](deep-quality-evidence-2026-10-06/rss-long.json)）：1000 次预热后 5×10000 双世界替换 + 100000 次搜索，RSS 增长 512KB 即平台（末三检查点持平，断言 <16MiB），无旧地形缓冲累积。
- cooldown 实验（[cooldown-experiment.json](deep-quality-evidence-2026-10-06/cooldown-experiment.json)，400 link chain 五轮交替）：借用比较+写后缓存分支 p50 +0.28% / p95 +0.63% / CPU +0.52%，无稳定收益，已撤回；失败原始数据保留。
- 证据文件：[matrix/](deep-quality-evidence-2026-10-06/matrix) 下 summary.json（结论中位数表）、identity.json（产物/夹具 SHA 绑定）、rounds.json（150 窗口逐轮指标合并）。逐窗口原始报告与日志（300 个文件）在采集期逐窗校验后合并为 rounds.json 并删除，可用 identity.json 登记身份重跑再生。

## 冻结产物与身份

根目录 `/tmp/kkscreeps-quality-20261006/`：before/kkscreeps `cdb87ac5…`、before/pathfinder_perf `5e3bc9ac…`、baseline-source.tar.gz `b51c710d…`（全轮统一旧基线，不重建不覆盖）。correctness/kkscreeps `f0c78d0b…`；撤回 cooldown 实验后最终 release 与之字节一致：final/kkscreeps `f0c78d0b…`、final/pathfinder_perf `1c8f3641…`、final/isolation `574343c5…`。诊断窗口 [diagnostic-before.json](deep-quality-evidence-2026-10-06/diagnostic-before.json)（400 link chain，仅定位用，不能引用为正式收益）：JS call 与 V8 转换为主要成本，计时存在包含关系不能相加。

## 复审修正

- 能力分类声明最初放入参与内建代码摘要的 builtin.rs，导致无意 PlanHash 漂移；移到 lib.rs 的独立 builtin_capabilities 导出后 builtin.rs 与 HEAD 完全相同。旧/new inspect 的 PlanHash 与执行策略身份完全相等。
- quality.py 寻路解析 bug 由正式矩阵首轮运行暴露（StopIteration），最小复现定位为 libtest 同行前缀问题后修复并以工具回归钉死。
