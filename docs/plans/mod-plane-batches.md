# 贡献平面批次计划（A–G）：事务边界 + 开放领域 + JS Mod

> 状态：**架构已定型（[ADR-014](../decisions/ADR-014-contribution-plane.md)，2026-10-01 用户拍板四项）**。本文登记 A 批切片与两条并行轨的执行方案；B–G 为方向性登记，切片到批前另出。
> 体裁例外注：本文 §3 的 B–G 方向登记**长期有效**，不随 A 批收口归档（plans 目录其余文档收口即归档）；A 批收口时将把 B–G 登记迁往 roadmap/ADR 附表。
> 依据：[06 架构复审](../research/06-architecture-and-js-mod-review.md) §2/§6 批次表、[mod-system.md](../designs/mod-system.md) §6/§7/§11/§13、ADR-014 决策 4（四处调整）。
> 批次方向决策归用户（已拍板）；批内切片节奏自主（同 2B 批先例）。
> 定位：在继续 vanilla 扩面与冻结存储格式**之前**建立事务与扩展边界；不受边界影响的 vanilla 扩面可继续，但受 ADR-014 决策 3 门禁约束（新增封闭分派须同时提供具名 slot 或登记豁免）。

## 0. 总原则

2026-10-02 基于提交 `6248546` 的实现核查见 [A 阶段核查与后续实施顺序](a-stage-current-audit.md)。以下测试统计及完成记录保留当时口径；实施时使用现行棘轮地板与源码核查结果。

- **统一提交点纪律不可破**：每片的行为、测量、恢复结果先可评审，再扩大范围（mod-system §13 同款）。
- **无 Mod 路径零回归**：screeps-ok 棘轮地板 1852/2914=63.55% 不降、零未豁免转红；bot 三夹具**同既有三基线口径**——tooangel/overmind 各 2000 tick、wasm 10 tick（不放大 wasm 口径），0 硬中断、0 console error。
- **串行参考保留**：事务化/并行化各片以「与串行参考逐状态一致」为判据（执行序无关测试旋钮已有，kk-runtime embed 测试）。
- **A 批触碰 tick 主链路**：以 settle_tests 210 个集成测试（kk-engine 全部测试约 230）+ kk-runtime 集成测试 + 棘轮 + bot 三夹具为网；每片小步提交。
- **故障注入基建随切片建立**：现有钩子只覆盖 settle 段（`Engine::set_fail_room_for_test`、embed 的 `fail_next_settle`/`fail_next_settle_room`）；A1 随状态机新增 `fail_stage_for_test` 族钩子（prepare/settle/validate/publish/市场段/发布点），后续切片复用。

## 1. A 批切片：事务与故障边界

### A1 引擎 tick 拆段 + 故障状态机

- **内容**：`Engine::tick_with_intents_with_input`（settle/mod.rs:723）显式拆为 prepare / settle / validate / publish 四段；引入运行状态机 `Ready → Running → Prepared → Committed`，失败进入 `NeedsRecovery` 并禁止新的世界变异；收编现有 `PendingRound` 语义（失败轮缓存玩家结果 + 原 TickInput 重试，不重跑玩家代码）；线程回执/命令接线 tick/generation/epoch 字段（拒收**单测**在本片落地，联调验收在 P1——当前 proto 每命令独立 ack 通道 + 同步 recv 的模型下结构上不存在迟到报告，解耦后才出现）。
- **判据**：任一段失败后世界保持上一 tick（`fail_stage_for_test` 族注入）；状态机单测覆盖全部转移；epoch 拒收单测；棘轮零回归。
- **依赖**：无（首片）。

> **✅ A1 已收口（2026-10-01，本提交）**：五段落地（prepare/settle/**market**/validate/publish——市场为独立段位）+ 状态机（外部变异门键控 (time,generation) + abandon_recovery 两条出路）+ 市场快照桥（A2 前过渡——每 tick 无条件 MarketState 克隆，当前订单规模可忽略，A2 真候选化时撤销）+ validate 三 tripwire（房间覆盖含错序/id 单调/玩家键不减）+ RunEpoch 接线（Cmd::RunPlayer/EvalReport 回带 + ThreadExecutor 错轮拒收单测，变异击杀×1 验证桥恢复逻辑）。**评审清偿增补（三路 2×P1）**：embed 串行链死代码修复；`no_open_round` 门（世界/市场变异操作在 embed pending **或**引擎恢复态任一存在时拒绝——settle_external/tick 失败不缓存 pending 的两门分叉场景，封死 place_market_order 在恢复态外改订单簿的漏洞）；settle_external/tick 统一走 settle_world（embed 钩子对全部结算入口一致消费）；恢复逃生路/段钩子顺延/重试恰一次消费闭环测试。
> **范围注记（后批承接）**：①Cmd::Tick 通道（bot_check 消费）未接 epoch——消费方不消费 epoch，随 P1 解耦派发/回收一并接线，RunEpoch 定义位置（现驻 kk-runner 传输层）届时上移评估；②状态机「全部转移」按**外部可观察转移**口径覆盖（调用内瞬态由类型私有性保证；Running/Prepared 跨调用拒绝分支无私有 setter、panic 注入前不可单测——显式登记）；③kk-world commit_room 的「不 bump、advance_ticks 单点」纪律自 A1 起额外承重（恢复门判据依赖），已在 world.rs 文档锚定。

### A2 市场纳入候选

- **内容**：`settle_market` 从直接改 `self.market` 改为操作 A1 的候选事务；订单簿/credits 写入随 publish 生效。现状有利条件：`settle_market` 对房间的访问已是「收集已结算房数组」形态（settle/market.rs 以已结算房间切片为入参），跨房读写无互相嵌套。**注入方式**：市场候选的故障注入打在 settle 之后、publish 之前的段边界；`settle_market` 函数本身保持不可失败（官方 global 阶段无 throw 路径，为注入而添加真实 Err 路径是对官方镜像语义的反向破坏）。
- **判据**：段边界注入后世界/玩家/市场一起回滚；现有市场全链测试（2B 批 P0–P2）全绿；「publish 之前不得存在不可回退写入」不变式测试。
- **依赖**：A1。与 A3 顺序可换（见 A3），实施时按耦合度定。

> **✅ A2 已收口（2026-10-01，本提交）**：`settle_market` 改写候选 `MarketState`（staged_body 的 Market 段每 tick 一次 clone 作候选本体、`publish_tick` 一次换入）——publish 前权威市场结构上不可变，A1 快照桥及其 StageCursor 快照 API 撤销，失败轮无需市场恢复（世界/玩家/市场同保持上一 tick 由候选纪律结构性保证）。判据达成：①五段注入矩阵全绿（订单簿/流水/money 保持 + NeedsRecovery 段位 + 重试恰一次消费）；②2B P0–P2 市场全链零改动全绿；③注入矩阵 + 变异击杀三连（直写+换入/直写+不换入死于探针臂、完全回退无桥死于注入臂）+ 评审清偿补强：`MarketState` derive PartialEq，含 intershard createOrder 的失败轮夹具锁**整态相等**（覆盖私有计数器——直改 committed 只动 seq 的形态由变异击杀验证拦截）与**重试订单 id 稳定**（失败轮序号不消耗，"mkt1"→重试同 id、次单 "mkt2" 续档）。**残留登记**：①恢复门只键控 world (time,generation)，`Engine.market` pub 直改在恢复态不可检测（A1 评审曾预期 A2 候选化缩小此空洞——实际未缩小：候选从被直改的 committed clone 起、直改静默进重试历史；embed 管理面已由 no_open_round 封死生产路径，直改面待 A4 随统一发布点收口）；②候选克隆成本与 A1 快照同形（每 tick 一次、现无订单负载可忽略）——订单负载出现时在 P2 对照 harness 观测。

### A3 OwnedTickTxn（Working 改造）

- **内容**：`Working<'w>` 的 `players`/`next_id` 共享借用改为拥有数据的候选事务（mod-system §6.2 / 06 §2.1 路线）；结算入口签名迁移；为 mod 宿主 `Rc<RefCell<OwnedTxn>>` 短借用/reentrant 调用铺路。禁 unsafe 指针/lifetime transmute 规避（mod-system §6.2 同款禁令）。**波及面实测（评审核实）**：`w.players` 仅 3 处（creep/controller.rs）、`w.next_id` 仅 `Working::insert_new`、`settle_room` 调用点仅 2 处（引擎 mod.rs + settle_tests 唯一测试助手）；子模块 `&mut Working` 参数在字段访问语法不变的前提下不需逐个改——真实成本在 OwnedTxn 容器设计、Engine 字段所有权与提交路径，而非调用点数量级。因此 A3 也可排在 A2 之前（先容器后市场槽位，冲突面更小）。
- **判据**：引擎结算测试全绿；代际 `generation` 恰 +1 的 debug 断言保持；无生命周期逃逸（clippy + 评审）。
- **依赖**：A1（先有事务容器再迁移所有权）。

2026-10-02 实施记录：[A3 候选事务改造与验证](../research/notes/a3-owned-txn-2026-10-02.md)。已建立 OwnedTickTxn、同线程共享玩家/ID 候选和 PreparedTick，将通知与玩家房间集合移到发布前准备；完整 CPU/Memory 提交及 JS 回调仍属于后续范围。

### A4 同一发布点验证 + 故障注入收口

- **内容**：CPU 桶、RawMemory/segments、market（及 Mod namespace 占位）全部经统一 publish 点。**桶账语义一次性写死（不留「保留现状」选项，06 §2.1 已有方向）**：桶权威移入 PlayerState/事务——RunJob 携带协调器算好的预算，沙箱只报告用量，发布后扣桶，删除 `settle_bill`/`discard_bill` 往返与「线程失联漏计桶账」现状（embed.rs 当前 `let _ =` 忽略回执失败随之消灭）。迟到回执拒收（worker epoch）在本片联调（若 P1 未竟则本片兜底）。**注入范围**：发布点各子步（桶/RawMemory/segments/market）的就地失败注入；进程级崩溃恢复（kill -9 后只接受完整版本）显式留给 E 批（kk-storage 就绪前无持久化可恢复）；扩展 B0-6 三项回归（重试缓存销毁/负桶停跑回血/TickInput 重采样）覆盖面。
- **判据**：发布点任一子步注入后，已提交状态只可能是完整版本、账本无迟到污染；同轮预算语义（官方 make.js 口径）不变；B0-6 回归全绿。
- **依赖**：A2 + A3 + 并行轨 P1 的 RunJob 预算字段（可先以占位接线）。

## 2. 并行轨（ADR-014 决策 4 第 2 条与决策 2 第 4 项拍板）

### P1 RunJob 并发执行 + 线程 CPU 计费

- **内容**：合并注入+执行为 `RunJob {view_handle, player_state, budget, code_revision, tick, generation}`（06 §2.3）；一次派发、有界并发回收（上限实测定，不等于玩家数）；线程命令携带期限；**CPU 计费墙钟 → 线程 CPU 时钟**（macOS/Linux 分别验证），墙钟保留为看门狗活性保护；RunJob 携带协调器算好的预算（A4 桶账语义的前半）。epoch 起步可先用已有的 `world.generation()`/`time()`。A1 范围承接：Cmd::Tick 通道 epoch 接线 + RunEpoch 定义位置上移评估。
- **判据**：同世界多玩家输出与串行参考逐位一致（执行序无关测试旋钮）；同轮预算语义不变、bot 三夹具不红；**迟到报告拒收联调验收**（A1 只测单测）；P50/P95/P99 与进程 CPU/RSS 改善有测量记录（≥5 独立运行，采样器见 P2）；看门狗 arm/disarm/terminate 代际原子性保持。
- **依赖**：A1 的状态机 epoch 字段（可先行以现有 generation/time 起步）；不依赖 A3（P1 改 execute_players/proto/budget，与 A3 的引擎侧无类型耦合，仅同文件 embed.rs 需协调）。

### P2 对照 benchmark harness

- **内容**：本仓 / 本地 xxscreeps（`/Users/keqing/Desktop/projects/sources/screeps/xxscreeps`）/ 官方私服三方同负载对照：同源玩家脚本（简单 harvest→upgrade bot）、**同地形数据导入**（跨引擎无共同 seed——本仓 ShardSpec 收 terrain 字节、xxs 走 `scripts/import.ts` 导入通道、官方走 import mod）、1/4/8/32 玩家档、≥5 独立运行；采集 wall/进程 CPU/RSS/P50/P95/P99 与 tick 成分。**首个交付物是进程 CPU/RSS 采样器**（现有 perf-probe 只记各相 wall，无进程级采样；P1 判据复用）。先做本仓 + xxs 本地（`engine/service/main.ts` 的 avg tick 观测点可采）；官方私服（Node+Mongo）后置，A 批内登记状态。
- **判据**：产出 `docs/research/notes/` 对照报告（机器/版本/commit/负载自检齐全，同 perf-probe 口径）；「性能超官方与 xxs」的后续声称以此为据。
- **依赖**：无（可与 A1 同日启动）。

## 3. B–G 方向登记（长期有效；切片到批前另出）

| 批 | 内容（06 §6 + ADR-014 调整） | 到批前须补 |
|---|---|---|
| B 开放领域定义 | EffectiveRules（平坦形态验收）、ObjectType/Extension、IntentDescriptor、handler graph、默认 slots；**与投影协议一步设计合并**（共享二进制 + per-viewer overlay + lazy wrapper + 扩展字段目录一次定协议，实施分步） | 投影协议设计文档；首个「内建结构家族迁移成内建贡献」选型（rampart/link 候选） |
| C JS 最小完整玩法 | Mod loader、可信 VM、玩家 API 安装、自定义 FIND/LOOK、counter 对象与动作；一个既有结构的 wrap/replace | 依赖 B 契约冻结 |
| D 性能与真实规则 | 线程 CPU 计费收口（若 P1 未竟）、公开移动/战斗/建造/容量等必要 slots；同房/跨房串行参考一致；**06 §2.6 两条玩家执行接口收敛**（`Engine::tick` 假沙箱路径当前对非 Executed 结局照常推进 tick——收敛为 engine 只留纯结算/prepared txn 接口 + 限定测试 facade） | 依赖 B/C |
| E 首次持久化前 | extension 编码、manifest、迁移/缺失 Mod、输入日志与 outbox；**进程级崩溃注入只恢复完整版本**（A4 留下的验收面在此闭合） | 存储格式冻结前最后闸门 |
| F 网络/客户端随 M3 | JS routes/auth/subscription/providers、资产/renderer/panel/types、握手 | 与 F2 渲染器 worktree 的合流风险登记：worktree 领先基线仅触碰 kk-cli（export_fixtures）与 packages/{renderer,shared}、不改 kk-runtime/kk-runner——真风险是其 feed/export-fixtures 快照契约写于 pre-2B 市场/终端之前，**合流前须对 pre-2B 基线做契约重验与重放**，并考虑 A 批提交时序重构后的兼容 |
| G 完整能力收口 | 世界生成、周期任务、存储 provider、NPC/经济/跨房规则；[能力验收矩阵](../designs/mod-system.md#12-能力验收矩阵)全覆盖 + 真实大规模基准 | 全部前序批收口 |

## 4. 每片通用验证矩阵

1. `cargo test` 全绿（受影响 crate）+ 棘轮零未豁免转红（地板 1852）。
2. bot 三夹具同既有口径（tooangel/overmind 2000 tick + wasm 10 tick）：0 硬中断、0 console error（触碰 runner/runtime 的片必跑）。
3. 新增单测锚定该片判据（故障注入/不变式至少各一）。
4. 性能相关片附测量（perf-probe 扩展或 P2 harness，含进程 CPU/RSS 采样）。
5. 提交说明引用本计划切片号与判据达成情况。
