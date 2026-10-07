> 历史归档：正文保留当时的方案、状态和验证；不得作为当前接手指令。当前工作见[任务台账](../mod-next-batches.md)，现行契约见[架构索引](../../architecture/README.md)。

# 深度质量优化接手说明（2026-10-07）

用户因当前 AI 额度接近耗尽要求交接。本文件记录实际状态，不代表六批全部验收完成。

## 收口记录（2026-10-07，后任 AI）

六批已于 2026-10-07 全部完成验收，下文任务 1–6 全部执行完毕；最终事实以[实施与验收报告](../../research/notes/deep-quality-2026-10-06.md)为准。要点：cooldown 实验读完十窗口数据后按无稳定收益撤回（撤回后最终 release 与 correctness 产物字节一致，`f0c78d0b…`）；正式性能矩阵 150 窗口完成（玩家主循环与 Mod 链持平、寻路四场景 4–7.5% 收益、RSS 持平、两版行为逐窗等价）；长期 RSS 验收通过（512KB 后平台）；矩阵原始窗口文件校验后合并为 `matrix/rounds.json`（保留 summary/identity/rounds 三份）；最终源码完整 `pnpm run check` 与 workspace doctest 通过；最终 release 棘轮 1918/89/907 转红 0、三 bot 2000/2000/10 与旧检查点双通道恢复均绑定 final 产物重验通过。附带修复 quality.py 寻路基准行解析 bug（libtest 同行前缀，`startswith` 永不匹配）并补工具回归。产物冻结于 `/tmp/kkscreeps-quality-20261006/final/`，final 源码归档 `final-source.tar.gz`。全部修改经用户确认后随本批提交（文档与实现同次提交，真实 ID 见 git log）。

## 工作区与约束

- 仓库：`/Users/keqing/Desktop/projects/games/kkscreeps`。
- 开始及当前 HEAD：`229bea4b702590b87d730f3a106b184538c62e5e`。开始时干净，当前大量实现/测试/文档修改均来自本轮，全部未提交；保留工作区，不 reset、覆盖整文件或自动提交/推送/发布。
- 读取 AGENTS.md、`/Users/keqing/.codex/RTK.md`、mod-next-batches.md、mod-complete-gameplay.md。shell 命令须以 rtk 开头；可用 `rtk proxy sh -c '…'`。主项目源码/测试 ≤1000 行，说明性注释中文。
- 执行用户已批准的六批计划：正确性优先，性能仅保留重复测量有稳定收益的改动；允许收紧 Rust API。玩家 JS、公用 Mod SDK、检查点字段/版本、PlanHash 语义不变。远期玩法/网络/持久化/RNG/回放不在本轮。
- 没有创建其他聊天、子代理、PR 或提交。请继续本地实现与验证，不另起项目重做。

## 已落地实现

1. `scripts/check.mjs` 显式构建当前 debug CLI，固定 CARGO_TARGET_DIR 和绝对 KKSCREEPS_BIN，再运行 JS；CI 调用同一完整门禁，单独 doctest/release ratchet。门禁旧 release/子步骤失败/启动失败及指定不存在二进制回归；性能工具核验全部玩家、动作计数和全部房间前后状态。
2. TickState 分配/Working 插入返回 Result，首次致命错误留在共享候选，捕获/decline/游标恢复不消除；房间结束与全局发布前检查。校验 ID 非零、跨房唯一、键/记录一致、合法坐标、ID 小于游标。原生所有创建路径处理失败。MAX-1 多房及被捕获错误双通道/重试/冷恢复已测试。
3. Engine 权威状态外部私有，getter + 隔离 `edit_world/players/market/mod_states`；开放/恢复期间拒绝编辑，账户/市场编辑推进 generation 拒收旧回执。World 无公开 room_mut/get_mut/commit_room；内容编辑与地形编辑分开，完整房间候选批量提交，所有失败检查在写入前。World 的 MAX generation 是缓存哨兵，不能成为有效版本。时钟/版本耗尽原子拒绝。Mod 世界编辑复用恢复校验。
4. `ChainCapability` 枚举携带领域状态；ChainHost 拆为五类领域 trait；能力分类在 `kk-mod-api::builtin_capabilities`。SDK 公共 sdk.js 门面，内部 sdk/ 按注册/规则/链/刷新/五领域拆分，固定模块白名单仅内部静态导入。op 与 SDK 静态调用双向接线测试、内部导入拒绝、原有链语义回归通过。
5. Arc 不可变 TerrainSet，每次原生搜索显式借用查询器；无全局 LOADED/地形锁/重复加载。原始 pf.cc/pf.h 未改，构建期唯一锚点生成适配，RAII 在返回/异常/取消清掉地形/矩阵/回调指针；保留 map_pos 供下次算法清理。超过池深度使用独立实例。运行时缓存世界 Arc 身份 + terrain_epoch，JSON/KKPV 同一所有权；原始 JSON 核对内容，坏 JSON/KKPV 明确报错。并发、六层嵌套、Weak 释放、原始路径校验和均通过。

重要：builtin.rs 与 builtin/store.rs 字节进入 PlanHash 的代码摘要。曾无意在 builtin.rs 加模块声明导致摘要漂移，已经移到 lib.rs 的独立 builtin_capabilities 导出，builtin.rs 当前与 HEAD 完全相同。不要通过放松身份校验解决漂移。

## 验证状态（不得混同最终验收）

- 两次无参数完整 `pnpm run check` 成功，日志 `/tmp/kks-quality-final-check.log`、`/tmp/kks-quality-accepted-check.log`。第二次源码规模 592 文件、compat Node 15/15 且 skipped=0、Mod 链 254/254、World 68/68。两次运行期间仍有少量边界修正/模块迁移，最终选择性能改动后须再跑一次严格对应最终源码的完整门禁。
- `cargo test --workspace --doc` 通过，`/tmp/kks-quality-doctest.log`；最后模块导出位置调整后可再跑确保最终一致。
- release ratchet 已通过：1918 passed /89 failed /907 skipped /2914 total，转红 0、豁免 0、地板 1918。`/tmp/kks-quality-ratchet.log`。当时产物在最后 PlanHash 导出迁移前，必须用最终 release 再验一次，不改 floor/allow。
- 三 bot 已通过执行验收：TooAngel 2000/2000、Overmind 2000/2000、WASM 10/10，终止/惩罚跳过均 0，console-error 0/1/0。Overmind 的 colony 初始化错误为既有。固定 upstream 34a630efa42908c117c19c25d2350fa2387bd633；TooAngel 87645a0b933360224737cca3b18ae9d112f46fef；Overmind v0.5.2.1 重新下载校验与缓存 SHA 相同。原始报告已复制入 evidence/bots。
- 旧 release 在独立 JS 组合场景 100 tick 后保存两通道检查点，当前 release runtime 原样读取，恢复再保存逐值完全相等，真实两玩家续跑到 101。PlanHash/执行策略身份与旧版相等。证据 `evidence/old-restore.json`，旧夹具 `evidence/old-checkpoints/`。工具 `scripts/benchmark/checkpoint_restore.py` + .rs 用 cargo JSON 产物身份链接当前 release rlib；直接 runtime 恢复，不放松 sim 自身二进制 SHA 门。
- 定向测试日志还有 `/tmp/kks-quality-path-tests.log`、`/tmp/kks-quality-runner-all.log`、`/tmp/kks-quality-caught-id.log`、`/tmp/kks-quality-boundaries.log`，不可据此省略最终完整门禁。

## 正在后台运行的性能实验

截至交接核对，`cooldown-experiment.partial.json` 已完成 9/10 窗口，尚无完成报告。进程会自行完成，不要同时运行构建或其他验证。若接手时间较晚先核对 final/partial 与进程，勿重复启动或覆盖原始证据。

命令：

```bash
rtk proxy sh -c 'python3 scripts/mod-perf-compare.py --before /tmp/kkscreeps-quality-20261006/correctness/kkscreeps --after /tmp/kkscreeps-quality-20261006/after/kkscreeps --output docs/research/notes/deep-quality-evidence-2026-10-06/cooldown-experiment.json --links-per-room 400 --warmup 500 --ticks 3000 --repetitions 5 --modes chain > /tmp/kks-quality-cooldown-experiment.log 2>&1'
```

会话 session 63888（若新聊天不能使用该 ID，直接读日志/文件及进程）。源代码里的唯一性能专用实验是 `ModChainContext::write_fields` 内 link cooldown 分支：借用比较只读字段，避免两棵 JSON clone；写后缓存 fields 避免再解析。五次交替结果目前未见稳定加速，CPU 略增，**倾向撤回，但必须读完最终五组数据再决定**。不要把前 9 窗口当完整结果。

撤回时仅替换 `mod_chain.rs` 中从 `// 当前内建写面仅接 cooldown；` 到下一个 `impl ChainHost` 前的 link 分支，参考 `/tmp/kkscreeps-quality-20261006/cooldown-before.rs`。**不能覆盖整个备份文件**（模块导出路径等后来已变）。当前优化分支备份 `cooldown-optimized-block.txt`。保留实验失败原始报告。

诊断原始 `evidence/diagnostic-before.json`：400 link chain，100 warmup/300 sample，JS call/V8 转换为主要成本。此诊断允许并行编译，仅定位，不能引用为正式性能收益；计时有包含关系，不能相加。

## 冻结产物及身份

根目录 `/tmp/kkscreeps-quality-20261006/`：

- before/kkscreeps SHA `cdb87ac5569c1e49f647063238faa2e080d0b444e853439f3a7df310860865c5`。
- before/pathfinder_perf SHA `5e3bc9accdabb8b7887599cc403326986175b65368a17b3fa861f0e2084b7099`；是 kk-runner/tests/pathfinder_perf 的 release 可执行，不是 kk-pathfinder bench。
- baseline-source.tar.gz SHA `b51c710d2ad48659d6b5c9f6c572e03238818b940cdd041c9d054d1e9abb5c19`。仅含第一批门禁/负载验证工具，不含后续 engine/world/mod/pathfinder 修复，是全轮统一旧基线；不要重建或覆盖。
- correctness/kkscreeps SHA `f0c78d0b4bcb997629a72f30c0bbb30cf3f603bf0bc2192f3ba86bbf681893e4`：全部正确性/结构修复，无 cooldown 实验。
- after/kkscreeps SHA `d3c3ca9001cf9910ca2b8a4b737b693d340513da312d72c663c1ffa3a4b0c4f9`：当前优化实验。after/pathfinder_perf、after/isolation 已构建；选择最终代码后重建冻结到明确 final 目录，勿把旧 after 当最终源码。
- Cargo release 产物 JSON `/tmp/kks-quality-release-artifacts.jsonl`、path/isolation 各自 artifacts.jsonl；日志可能对应不同候选，最终须建立新的身份记录。

## 必须继续完成的任务

1. 等/核对 cooldown 实验完整结果，决定保留/撤回；没有稳定收益撤回。若优化新热点，先诊断，改动单独记录，仍以五次独立交替正式结果证明收益。
2. 最终源码运行完整 `pnpm run check`、适用 doctest；最终 release CLI、固定依赖/bots、adapter build、完整 ratchet，地板/豁免不动。可重复 bot/旧检查点工具把证据绑定最终产物。
3. **正式全性能矩阵尚未启动，不能省略**。`scripts/benchmark/quality.py` 默认覆盖 1/4/8/32 玩家 × JSON/KKPV，100/400 link × native/empty/chain，现有寻路四场景；5 次独立运行、交替顺序、500 warmup/3000 sample；共 150 个 before/after 进程窗口。寻路每进程沿用独立搜索窗口。采集时暂停构建/测试/其他验证。

```bash
rtk proxy python3 scripts/benchmark/quality.py --before /tmp/kkscreeps-quality-20261006/before --after <最终冻结目录> --output docs/research/notes/deep-quality-evidence-2026-10-06/matrix
```

最终目录须含 kkscreeps 与 pathfinder_perf。报告校验全部玩家/动作/房间状态、两版行为等价、release/窗口/固定夹具 SHA；失败不生成成功 summary。`quality_test.py` 两项工具回归已通过。path 的 CPU/RSS 是含启动的整个子进程，wall 是搜索窗口，报告须区分口径。表格用五轮指标中位数，原始 JSON/log 全保留。

4. 独立运行 `after/isolation --ignored --nocapture --test-threads=1`（或最终重建 isolation）；新增 `terrain_replacement_rss_plateaus` 1000 次预热后 5×10000 双世界替换、100000 次搜索，输出 TERRAIN_RSS_TREND。此长窗口尚未运行；记录长期 RSS 和 Weak 释放证据。
5. 自审 git diff，确保无误改字符串/检查点字段，无源码超过 1000；新独立样例验收已由旧组合场景双通道支持，最终源码再核验绑定产物。冻结最终源码 manifest/归档、CLI/寻路可执行 SHA、环境/夹具 SHA，记录必要校验成本和性能结果，不虚报全面加速。
6. 更新现行 architecture、research 主报告、此接手位置与 mod-next-batches；新文档同步索引。`docs/research/notes/deep-quality-2026-10-06.md` 目前仍为中间状态，必须改为实际最终结果。最终交付可审查工作区 + Rust API 迁移说明 + 六批逐项清偿 + 兼容报告 + 原始性能报告；远期能力继续原 G 队列，勿推进玩法里程碑。

主要文档入口：state-editing.md、mod-execution.md、source-organization.md、snapshot-contract.md、pathfinder-terrain.md、pathfinder-callback.md 已同步实现。不要追改历史 research 结论。
