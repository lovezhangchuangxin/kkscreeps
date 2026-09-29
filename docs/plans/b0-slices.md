# B0 拆片执行规格（事实差核对 + 切片计划）

> 状态：执行中。依据：[b0-runtime-coordinator.md](b0-runtime-coordinator.md)（主方案）、
> [ADR-013](../decisions/ADR-013-architecture-review-coordinator.md)。
> 本文记录 2026-09-29 开工时对 HEAD（1f1059a）的事实差核对结论与切片拆分；
> 主方案内容不重复，冲突处以本文对 HEAD 的实测核对为准。

## 1. 事实差核对（方案定稿于 A5 前 → HEAD=1f1059a）

| WP | 方案位点 | HEAD 实况 | 差异 |
|---|---|---|---|
| WP1 | embed.rs `run_players` 循环 `?` | embed.rs:446 `?`（任一玩家 Err 冒泡整轮） | 无漂移 |
| WP1 | sandbox.rs `end_tick` 即时 `budget.settle` | sandbox.rs:392（含 `pending_intents` 即时暂存 JS 内） | 无漂移；**Memory 持久位在 prelude game.js:98 `rawMemoryData` 闭包串（跨 tick 自持），官方是每 tick 从 DB 注入**——候选化的权威侧应落 PlayerState |
| WP1 | kk-engine → kk-runner 依赖 | Cargo.toml 确认；embed.rs:15 `use kk_runner` | 无漂移 |
| WP2 | proto.rs:230 `evict_idle` 栈顶限制 | proto.rs:230-244（`while last()` 只逐栈顶连续空闲段） | 无漂移；sandbox.md:69 文档已在复审文档批纠偏，代码未动 |
| WP3 | creep.rs:1482 `SystemTime::now` | 漂移至 **creep.rs:1511**（A6 前移）；:1498-1509 仍并存「不落盘/投影合成」与「落盘墙钟」两条矛盾注释 | 行号漂移 |
| WP4 | 90_host.js:43 `globalThis.__kk` | 确认；玩家面依赖仅三处：90_host.js（定义）、20_cjs.js（无直接引用，resetModules 经 Rust）、sandbox.rs（Rust 调用方 `globalThis.__kk.*` 共 7 通道）；测试面 sandbox_tests.rs:1172-1263（冻结断言+白名单）；compat 侧 `__kkEmbedClient` 是适配器自有全局，无关 | 依赖清单已闭 |
| WP4 | PerfRecorder 无界 | embed.rs:54-58（A6 新增，注释已登记归 B0） | 方案定稿时不存在，本批新增落点 |
| WP4 | `.call` 家族 | 实测清单：00_shared.js:81、_registry.js:50、game.js×7、room.js×4、room-position.js×4、creep.js×3（`__substr/__charAt/__arrayIncludes/__mapGet/__mapSet/__setAdd/__setHas/__arrayPush` 八内建）；structures.js:886 是注释非调用 | 全量约 20 处，标杆在 structures.js isActive（Reflect.apply 加载期捕获） |

其余核对：世界侧两阶段提交已成立（`Engine::tick_with_intents` 克隆房间/players/next_id，全房成功才写回+推进 time，settle/mod.rs:592-668）——**部分提交的泄漏面全部在玩家侧持久状态（沙箱内 rawMemoryData + Rust 侧 Bucket）**，与 ADR-013 判断一致。bot_check 用 `PlayerThreads::tick`（Cmd::Tick 路径），perf_probe 用 EmbedEngine——WP1 迁移时两者消费面保持不变。

## 2. 切片（每片独立走完整批次循环）

| 片 | 内容 | 主落点 |
|---|---|---|
| B0-1 | **WP2 LIFO 每线程纠偏**：注册栈降级为存活计数+每线程断言；`evict_idle` 全表回收任意超时实例；`shutdown_all` 保持逆序；乱序销毁压力测试（中间实例被逐出+存活实例正常执行+干净退出+每线程零违规） | kk-runner sandbox.rs/proto.rs、proto_tests 串行链 |
| B0-2 | **WP3 TickInput**：`TickInput { now_ms }` 贯通 `tick_with_intents`/`Working`；signController 读注入值；首跑取墙钟、重放注入记录值；重放一致性回归（含 sign）；清理 creep.rs:1498-1509 矛盾双注释 | kk-engine settle、embed 入口 |
| B0-3 | **WP4 宿主面收口**：`__kk` 七通道改 Rust 私有 `v8::Global<v8::Function>` 句柄+删除全局；set_raw_memory 改私有通道（销灰区 :172）；玩家全局面白名单更新（`__kk` 不存在）；`.call` 家族全部迁 Reflect.apply 加载期捕获（销灰区 :176）；endTick 信封 error 串字节闸（销灰区 :171）；PerfRecorder cap/开关（销灰区 :180） | kk-runner sandbox.rs/prelude、embed.rs |
| B0-4 | **WP1a 协调层骨架**：新 crate `kk-runtime`（依赖 kk-engine+kk-runner）；`RunResult` 统一类型（身份/结局枚举区分玩家结局与基础设施故障/intents/RawMemory 候选/CPU 计费候选/console）；`Sandbox` trait 演进为 `run(&RunContext) -> RunResult`；假沙箱同接口迁移；真沙箱适配器；`EvalReport`/`PlayerRun` 归并为投影层（对外 JSON-RPC 形状不变） | 新 crate、kk-engine sandbox.rs |
| B0-5 | **WP1b 统一提交点 + WP5 回写**：`end_tick` 候选化（不即时 settle、RawMemory 候选上提）；begin_tick 从 PlayerState 注入 RawMemory（官方每 tick 从 DB 恢复同构）；提交点一次切换世界+玩家持久状态；玩家级失败不冒泡整轮；部分提交三连回归测试；embed.rs 编排移交 kk-runtime、kk-engine 摆脱 kk-runner 依赖；文档回写（tick-contract 状态注/sandbox.md/ADR-013 落实状态/灰区表销账：整 tick 原子回退 :170 等） | kk-runtime、embed.rs、kk-engine Cargo、docs |

顺序依主方案 §6（先局部修复后结构重构，协调层吸收前序最终形态）。B0 不掺文件搬移（>1000 行拆分归 B1；kk-runtime 新 crate 是目标结构本身，不算搬移）。

## 3. 各片验收统一口径

每片：`cargo test --workspace` 全绿 → 显式 cd 主仓 `cargo build --release` 后 `node compat/scripts/ratchet.mjs` 零未豁免转红 → bot 三基线（tooangel 2000t console 零错人工确认 + overmind 2000t + wasm 10t）→ ≥3 路 subagent 并行评审（官方语义对照/对抗安全/回归兼容）→ 逐条清偿+复审 → 灰区表/沿革回写 → `pnpm run check` → 单一归因提交并 push。

B0-5 收官关卡（ADR-013 复现场景全转回归且绿）：①部分提交三连（注入失败轮先执行玩家 Memory/CPU/世界版本不变；结算失败重试不重跑玩家代码；玩家结局不中止整轮）②LIFO 乱序销毁 ③重放一致性。性能：perf-probe 同口径对比 compat/reports/perf-a6.md（2 玩家 p50 0.813ms/tick 基线），重构不得显著劣化（各相 p50 劣化 >20% 需归因）。

## 4. 片 4（WP1a）接口设计定稿（随 B0-4 实现收敛修订）

- **类型落点**：`RunOutcome`（**四形态** Executed/Terminated/InjectFailed/InfraFailure——玩家软异常不单列：经 expr 携带（与 PlayerRun.executed 的 `kind!="terminated"` 口径一致）；InfraFailure 是唯一非玩家结局，协调器见此转整轮停顿）、`RunResult`（player/outcome/intents: IntentBatch（已按注册玩家清洗）/expr/cpu_billed_ms 候选/raw_memory 候选/console）、`RunSpec`（player+code）全部定义在 kk-engine（Sandbox trait 旁，零 deno_core 依赖）；kk-runtime `pub use` re-export。
- **Sandbox trait 演进**：`collect_intents(&WorldView, &str) -> IntentBatch` → `run(&WorldView, &RunSpec) -> RunResult`。投影由实现方自行从 WorldView 生成（假沙箱零投影开销；真沙箱内部 world_json_for_viewer）。ScriptedSandbox 同接口迁移，Engine::tick 适配（行为不变 + **结局门**：仅 Executed 的 intents 进结算批）。真路径执行器在 kk-runtime 的 `PlayerExecutor`/`ThreadExecutor` 面（Sandbox 真实现/桥接形态随 B0-5 落定）。
- **kk-runtime 骨架**（B0-4 已交付）：crate 依赖 kk-engine+kk-runner；`PlayerExecutor` 执行器抽象 + `ThreadExecutor`（包装 PlayerThreads 通道，EvalReport→RunResult；失败分类 B0-4 保守：run_player Err 一律 InfraFailure——**InjectFailed 拆分推迟至 B0-5**（proto.rs 通道把 begin_tick 注入失败从 Err 改为玩家级报告时接线））。
- **契约承载边界（前瞻登记，B0-4 评审 P2）**：IntentBatch 现按玩家分组（objects 键 ObjId + room 列表）——tick 契约的「按房间分组指令」可经 world 反查推导（ObjId 不内嵌房名），「全局指令」（市场/power/跨房）尚无承载类别；跨房移动（2B）/市场落地时 IntentBatch 须扩类别或契约改写。
- **统一提交点（片 5）**：end_tick 不再即时 `budget.settle`（产出 cpu_billed 候选）；Memory 写回产物上提为 raw_memory 候选；begin_tick 从 PlayerState.raw_memory 注入（官方每 tick 从 DB 恢复同构）；协调器提交点一次切换世界+玩家持久状态。**B0-5 接线注意项（评审登记）**：①InjectFailed → PlayerRun 投影必须 executed=false（代码未执行——adapter 的 mapRunResult 在 executed=true 时读 kind，inject_failed 越出 syntax|runtime|serialization 三分类会抛不识别的 RunPlayerError；executed=false 走普通 Error 路径反而兼容）；②InfraFailure 的 expr 是 Null——不得误投影为 executed=true（mapRunResult 读 expr.ok 直接 TypeError）；③InfraFailure 错误串可携带玩家污染文本（beginTick 异常经全局污染）——协调器日志/RPC 按不可信内容对待；④PlayerThreads::set_raw_memory 通道（B0-4 开）无测试覆盖，B0-5 首批补 ThreadExecutor 往返测试。

## 5. 片 5（WP1b 统一提交点）实现规格（B0-4 后定稿）

**候选化改造（kk-runner）**：
- `Sandbox` 增 `pending_bill: Option<f64>`（end_tick Ok 路径写入 `used+0.2×billed`；after_termination 写整份 tickLimit——**settle/settle_terminated 全部不再即时落桶**）与 `pending_raw_memory`（endTick 信封新键 `rawMemory`=writeBackMemory 成功后的 rawMemoryData 快照；game.js memoryWorld 导出 rawMemoryData getter 给 90_host）。
- 新通道 `settle_bill(id)`（应用暂存→桶）/`discard_bill(id)`（丢弃）——协调器提交点二选一。begin_tick 开头若发现未处置暂存→丢弃（不变量：begin 前必无暂存）。
- `EvalReport` 加 `cpu_billed_ms: Option<f64>`、`raw_memory: Option<String>`；proto.rs 的 run_eval_tick 对 begin_tick Err 改产 `kind:"inject_failed"` 报告（玩家级——mpsc 失联仍是 Err=infra）。
- **每 tick 注入**：ThreadExecutor.execute 前 `set_raw_memory(player, PlayerState.raw_memory)`（官方 make.js 每 tick 从存档恢复同构；世界回退→PlayerState 不更新→下轮注入旧值，泄漏消灭）。

**协调器（kk-runtime）**：
- EmbedEngine 整体迁入 kk-runtime（含 terrain_codes_from_json/PlayerRun——对外 JSON-RPC 形状不变；embed_tests.rs 随迁 kk-runtime/tests 保串行链——**配套随行非 B1 治理性搬移**）；kk-engine 删 embed.rs+Cargo 删 kk-runner 依赖（验收关卡 3）；kk-cli 改依赖 kk-runtime。
- run_players 改统一提交点：①逐玩家注入+执行→RunResult（InfraFailure→整轮 Err 停顿；InjectFailed→玩家级继续）②tick_with_intents_with_input ③成功→提交（PlayerState.raw_memory=候选 + settle_bill 各玩家 + console/last_console 发布）④失败→整轮 Err，候选**缓存**于协调器（last_pending）——**下一次 run_players 不重跑玩家、只重试结算**（ADR-013 复现①③验收）；成功轮清缓存。
- cfg(test) 故障注入钩子 `fail_next_settle`（部分提交三连的场景②）。
- 部分提交三连回归（kk-runtime/tests）：①注入失败轮玩家级恢复（p2 毒化 Game→InjectFailed，轮次继续，p1 Memory 照常提交）②结算失败轮全量不变（p1 Memory.n/世界 time 不动；重试轮执行次数=1——console 计数器取证）③重试只消费缓存（不重跑玩家代码）。

**WP5 文档回写清单**：tick-contract 状态注（单机协调器已落地条目）、sandbox.md 对应条目、ADR-013 四项标注落实状态、灰区表 :170 整 tick 原子回退销账、b0-runtime-coordinator.md §3 WP1 形态回写（run(&WorldView,&RunSpec)/四形态）、overall.md 过渡边删除、roadmap/沿革、记忆 B0 收口更新。

**验收**：ADR-013 三复现场景全绿 + `cargo tree -p kk-engine` 无 kk-runner/deno_core + 棘轮零转红 + bot 三基线 + perf-probe 同口径对比 perf-a6.md（投影/运行/结算/墙钟 p50 劣化 >20% 需归因）+ `pnpm run check`。

## 6. 片 5 评审清偿与实现偏差回注（B0-5 收口）

- **实现偏差（良性与实现一致）**：①Sandbox 未加 `pending_raw_memory` 字段——候选只经 endTick 信封→RunResult 携带（沙箱内 rawMemoryData 即隐式持有者，每 tick 注入覆盖使 Rust 侧槽位无必要）；②`EvalReport.bucket_after` 语义漂移为**结算前**口径（候选未落桶；bot-check 通道在 run_one_tick 内即时提交后另读——行为不变）。
- **评审 P1 清偿**：pending 状态机闸门——`tick`/`settle_external`/`place_object`/`set_terrain` 在失败轮缓存存在时显式拒绝（no_pending_round）；重试轮键集守卫（请求缓存外玩家 bail，防 runs.remove panic 与「panic 前已提交」事故）。
- **评审 P2 清偿**：inject_failed 计费如实携带（防免费毒化挂机的整份 tickLimit 候选进报告——报告与落桶一致）；endTick 的 rawMemory 候选加 typeof string 门（String 包装对象经污染 String.prototype.toJSON 的 3MB 伪造面关闭）；run_one_tick 的 begin_tick Err 前补 settle_bill（bot 通道丢账）；场景③取证强化（console+Memory.exec 双计数）；discard 臂测试（与重试臂形成两臂对照：丢弃轮的 exec+1 不提交）。
- **性能口径**：vs perf-a6 基线（run -52.8%/wall -29.3%）是 B0-1..5 四片的累计效应（宿主句柄化贡献为主）；**本片自身相对父提交（3f08003）实测 +7.9% wall / 各相 +5~15%**（settle +1.42µs 为 batch_from_results 结算窗口内克隆的真成本、占 wall 0.23%）——均低于 20% 关卡。
- **停顿口径登记**：InfraFailure 的 Err 是建议性停顿信号（embed 层不阻止立即重调）——恢复策略归调用方；停顿轮已执行玩家候选滞留沙箱、下一次 begin_tick 防御丢弃（漏计而非双计）。
