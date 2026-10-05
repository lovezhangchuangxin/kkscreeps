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

2026-10-02 已接入 RawMemory、segments 及宿主 CPU 桶候选，与房间和市场共用发布点。RunJob 合并注入与执行，执行阶段故障保留已确认结果并阻止自动重派。现有玩家状态的候选子步故障矩阵、显式工作者重建及真实异步回执拒收验证已补齐。并发调度和命令期限仍属于 P1；Mod namespace 尚未接入，未来新增状态须沿用同一候选发布边界。实施与验证见 [A4 玩家状态统一发布](../research/notes/a4-memory-writeback-2026-10-02.md)。

- **内容**：CPU 桶、RawMemory/segments、market（及 Mod namespace 占位）全部经统一 publish 点。**桶账语义一次性写死（不留「保留现状」选项，06 §2.1 已有方向）**：桶权威移入 PlayerState/事务——RunJob 携带协调器算好的预算，沙箱只报告用量，在发布点换入桶候选，删除 `settle_bill`/`discard_bill` 往返与「线程失联漏计桶账」现状（embed.rs 当前 `let _ =` 忽略回执失败随之消灭）。迟到回执拒收（worker epoch）在本片联调（若 P1 未竟则本片兜底）。**注入范围**：发布点各子步（桶/RawMemory/segments/market）的就地失败注入；进程级崩溃恢复（kill -9 后只接受完整版本）显式留给 E 批（kk-storage 就绪前无持久化可恢复）；扩展 B0-6 三项回归（重试缓存销毁/负桶停跑回血/TickInput 重采样）覆盖面。
- **判据**：发布点任一子步注入后，已提交状态只可能是完整版本、账本无迟到污染；同轮预算语义（官方 make.js 口径）不变；B0-6 回归全绿。
- **依赖**：A2 + A3 + 并行轨 P1 的 RunJob 预算字段（可先以占位接线）。

## 2. 并行轨（ADR-014 决策 4 第 2 条与决策 2 第 4 项拍板）

### P1 RunJob 并发执行 + 线程 CPU 计费

已实现有界玩家派发与回收、绝对命令期限及 macOS/Linux 线程 CPU 时钟。RunEpoch 上移到 kk-core，诊断 Tick 通道回带版本；保留墙钟看门狗和 A4 候选发布边界。实现、回归与重复测量见 [P1 玩家执行](../research/notes/p1-player-execution-2026-10-02.md)。

- **内容**：合并注入+执行为 `RunJob {view_handle, player_state, budget, code_revision, tick, generation}`（06 §2.3）；一次派发、有界并发回收（上限实测定，不等于玩家数）；线程命令携带期限；**CPU 计费墙钟 → 线程 CPU 时钟**（macOS/Linux 分别验证），墙钟保留为看门狗活性保护；RunJob 携带协调器算好的预算（A4 桶账语义的前半）。epoch 起步可先用已有的 `world.generation()`/`time()`。A1 范围承接：Cmd::Tick 通道 epoch 接线 + RunEpoch 定义位置上移评估。
- **判据**：同世界多玩家输出与串行参考逐位一致（执行序无关测试旋钮）；同轮预算语义不变、bot 三夹具不红；**迟到报告拒收联调验收**（A1 只测单测）；P50/P95/P99 与进程 CPU/RSS 改善有测量记录（≥5 独立运行，采样器见 P2）；看门狗 arm/disarm/terminate 代际原子性保持。
- **依赖**：A1 的状态机 epoch 字段（可先行以现有 generation/time 起步）；不依赖 A3（P1 改 execute_players/proto/budget，与 A3 的引擎侧无类型耦合，仅同文件 embed.rs 需协调）。

### P2 对照 benchmark harness

已实现本仓与本地 xxscreeps 的共享夹具对照，使用生产隔离沙箱，检查每玩家执行和动作计数、全部房间关键状态及实际地形/预算。1/4/8/32 玩家各五次独立运行的结果与边界见 [P2 本地对照记录](../research/notes/p2-local-comparison-2026-10-02.md)。官方私服接入后置；当前结果不代表完整三方端到端矩阵完成。

- **内容**：本仓 / 本地 xxscreeps（`/Users/keqing/Desktop/projects/sources/screeps/xxscreeps`）/ 官方私服三方同负载对照：同源玩家脚本（简单 harvest→upgrade bot）、**同地形数据导入**（跨引擎无共同 seed——本仓 ShardSpec 收 terrain 字节、xxs 走 `scripts/import.ts` 导入通道、官方走 import mod）、1/4/8/32 玩家档、≥5 独立运行；采集 wall/进程 CPU/RSS/P50/P95/P99 与 tick 成分。**首个交付物是进程 CPU/RSS 采样器**（现有 perf-probe 只记各相 wall，无进程级采样；P1 判据复用）。先做本仓 + xxs 本地（`engine/service/main.ts` 的 avg tick 观测点可采）；官方私服（Node+Mongo）后置，A 批内登记状态。
- **判据**：产出 `docs/research/notes/` 对照报告（机器/版本/commit/负载自检齐全，同 perf-probe 口径）；「性能超官方与 xxs」的后续声称以此为据。
- **依赖**：无（可与 A1 同日启动）。

## 3. B–G 方向登记（长期有效；切片到批前另出）

B 阶段的接口、投影布局、link 接入范围与切片见[开放领域定义与玩家投影](../designs/domain-projection.md)。该文档细化以下方向，不表示对应能力已实现。

B.1 已建立 `kk-mod-api` 的描述、校验、冻结与 link 默认目录（64f2d91）；**B.2 已收口（2026-10-02）**：link 有效规则接入放置/建造/结算/配额/伤害/玩家常量，槽执行器（Native/Chained+wrap-next）与 `Engine::with_plan` 装配落地，棘轮 1854 零转红——实现与限制见 [B.2 接线记录](../research/notes/b2-link-wiring-2026-10-02.md)。**B.3 已收口（2026-10-02）**：对象级扩展字段全链——挂载声明/目录/写入门/发布前校验/投影裁切（见 [B.3 记录](../research/notes/b3-extension-fields-2026-10-02.md)）；有界动作的玩家通道与条件权限归 C/B.4。**B.4 已收口（2026-10-02）**：KKPV v1 编解码原型（`kk-schema::kkpv` 布局+惰性视图+公开/私有合并、`kk-world` 同源桥、`kk-runtime` 不可变块句柄库）——逐字段/缺失/null 对照、畸形拒绝矩阵、慢工作者保留均有测试锁定，生产路径零变化（见 [B.4 记录](../research/notes/b4-kkpv-prototype-2026-10-02.md)）。**B.5 第一片已收口（2026-10-03）**：KKPV 通道接线与等价——束框架/沙箱侧可信解码器（惰性记录+捕获内建）/RunJob 与低层通道的 bundle 字段/EmbedEngine 双通道开关/bot-check `--kkpv`；三道等价测试（沙箱深对照、Ref 全环、e2e 含自杀→墓碑）+ bot 双通道门禁锁定"两通道玩家可见面一致"，嵌套键序经 preserve_order 特性统一发现并修复（编码保序）（见 [B.5 记录](../research/notes/b5-kkpv-wiring-2026-10-03.md)）。**B.5 第二片（测量矩阵首组）已收口（2026-10-03）**：perf-probe `--kkpv` 双通道 A/B（4/32 玩家**双样点值，≥5 样与臂序交替的正式判读后置**：私有拷贝路径 32p wall +45%/CPU +50%/RSS +14%，字节 4p +68%/32p +77%——信封与束随全服注册房间数线性增长且地形双载；settle 金丝雀把机器态漂移约束在 ~3%；按设计 §6 该路径不声称共享直读验收，数字为公开块/共享直读批的收敛基线）+链路径引擎级基准（无操作链开销与 settle 运行噪声 ~5% 不可分辨，点估计 +0.8%）+消费者缺口六项登记（同 note 测量矩阵节）。**B.6 接线片已收口（2026-10-03）**：公开块受众划分与共享直读——地形进公开束（`kkpv_public_terrain_blocks`，受众=公开组摘要，每代际编码一次）、私有块与信封撤地形、沙箱零拷贝视图直读（`getRawBuffer` 拷出不外泄——评审 P0 双外泄向量清偿：捕获内建+真 set 品牌 校验+HOSTILE 敌意探针）、`SharedKkpvBundle` 跨玩家共享 backing store（每玩家每 tick 拷贝 2→0）、`KkpvBlockStore::publish_shared` 接进派发点（全量校验+代际缓存+保留报告）；等价面扩地形全量走查、跨线程共享与多玩家并发（串行一致）均锁定，棘轮 1854 零翻转、bot 三夹具双通道通过（见 [B.6 记录](../research/notes/b6-kkpv-audience-2026-10-03.md)）。**B.6 测量片已收口（2026-10-03）**：双样首组（口径同 B.5）——32p 全维度收敛：wall +45%→+19~24%、CPU +50%→+26%、**RSS +14%→−16%（反超 JSON）**、每玩家注入字节 +77%→**−90%**（11.6KB vs 112.7KB；公开束 85.4KB 全服一份）；4p 墙钟未收敛（小规模每玩家投影/解码固定开销主导，字节仍 −65% 反转）；残余 ~20% 归因每玩家双趟投影+列式编码与 JS 解码 vs 原生 JSON.parse，收敛手段归 C/D；固定内存保留报告已接（稳态仅当前代际）——判读与缺口更新见同 note"测量矩阵"节。

**C 批切片（2026-10-03 定）**：C.1 装载与可信注册（包发现/manifest/依赖图/指纹 + 注册 realm + SDK 声明收集 → 冻结计划）→ C.2 处理器执行与 EmbedEngine 接线（结算期宿主桥、Mod 状态候选域、零 Mod 开销基线）→ C.3 counter 对象与动作全链（世界存储/意图收集清洗/管理创建）→ C.4 玩家 API 安装（player.js 进玩家 VM、FIND/LOOK、投影扩展）→ C.5 既有结构 wrap/replace 验收 + 性能矩阵（含外审登记三件：≥5 样交错正式判读、C 批后 4p/8p kkpv 双通道复测、SharedKkpvBundle 地形视图不上玩家可达链的钉住测试）。切片判据随片落到本表与对应 note。

> **✅ C.1 已收口（2026-10-03）**：`kk-mod-host` crate——mod.json 严格子集/依赖拓扑/整包内存快照指纹（符号链接拒绝、装载与摘要同字节）、可信注册 realm（deno_core ESM，仅包内相对导入 + `@kkscreeps/mod` 虚拟模块，包根包含与跨包导入在 load 侧强制）、注册期 SDK（objects/intents/handlers/rules/mounts；函数留 VM、Rust 只记键；seal 双门）、声明→描述符权威转换（键归属 Rust 定论、声明绑定须有函数）。产出 `Registered { plan(含 builtin:link 合并), contributions, executables }`。22 测 + workspace 491 全绿、clippy/fmt/doc-links 通过、棘轮 1854 零翻转（新 crate 不入服务端二进制，默认路径零变化；bot/perf 未跑的条件与理由见 [C.1 记录](../research/notes/c1-mod-loader-2026-10-03.md)）。残留九项登记同 note（处理器执行/玩家 API/SDK 子集面/阶段映射等归 C.2+）。

> **✅ C.2 已收口（2026-10-03）**：处理器执行与 EmbedEngine 接线——执行器 before/after/replace 可失败化（JS 异常必须能炸候选）、`mod_states` 候选域（SharedState/PreparedTick/publish 全链，失败回滚）、sdk `dispatch` + `ModRealm::invoke`（同步纪律/thenable 拒）、`Engine::install_plan` 就地装配 + `create_shard_with_mods`（桥闭包装 link 两槽 Mod 处理器；**wrap 装配即拒**——续行接线归 C.5；自有槽绑定显式拒——C.3）。验证：workspace 492 绿、棘轮 1854 零翻转、bot 三基线过、**零 Mod 开销同机 A/B 双样全相不可分辨**（见 [C.2 记录](../research/notes/c2-mod-execution-2026-10-03.md)）；残留七项登记同 note。

> **✅ C.4 已收口（2026-10-03）**：玩家 API 安装——Mod 对象进玩家投影（`mod_record_json` 对象级可见性：RoomVisible 走房间视野门/OwnerCollection 归属者集合语义（与己方旗同形）+ 字段级 Public/Owner/ServerOnly 裁切；JSON 与 KKPV 两通道同口径、桶键=类型键；无目录的 vanilla 形态不进）；`PlayerSurface::from_plan` 冻结计划确定性导出玩家面目录（FIND 自 1000 按键序分配、保留键/类名/条件可见性装配期拒绝）；player.js 经 `kkmodplayer://` 内存装载器进每个玩家 VM（静态导入图、拒跨包/动态导入/查询串；安装运行时句柄 Rust 持有、模块执行前删 `__kkp`——Mod 代码与玩家同权限面；安装带墙钟期限与近堆兜底；canary 预检先于引擎装配）；安装 API（builtins/objects.bind/globals.define/view.field/intents.emit 命名空间子集/queries.install）+ FIND/LOOK/look 桶预初始化进 prelude；KKPV 块头换 `Engine::plan_hash()` 真实指纹（B.5 残留收口）；Float 域写入口 backing 规范化（防 KKPV 列型混派）。三路评审 4×P1+2×P2+9×P3 清偿并复审闭合。验证：workspace 523 绿、棘轮 1854 零翻转、bot 三基线同形、零 Mod A/B project/settle/wall/RSS 不可分辨（run 相 +3.5% 点估计、区间重叠按瞬态归因并标注复核——见 [C.4 记录](../research/notes/c4-player-api-2026-10-03.md)）；残留七项登记同 note。

> **✅ C.3 已收口（2026-10-03）**：counter 对象与动作全链——`RoomObject::Mod` 世界存储（标签 16、字段 payload 按类型描述符校验、位置字段同步信封，二进制快照往返锁定）；扩展意图玩家面原语 `__kkEmitExtensionIntent`（**按沙箱配置门控装配——vanilla 全局面零变化**，计费与对象动作同轨）→ `UserIntents.ext` 收集（冒号键形+身份绑定+同键覆盖）→ 结算侧权威校验（目录/schema/接收者/归属 owner_field，未过即丢弃不炸世界）→ 自有槽执行链（process 中心+before/after，第三执行器族 ExtSimple；wrap 拒）；对象域外的形态（房间/玩家/世界域、队列合并、非严格转换、显式入口）装配门显式拒。管理命令 `place_mod_object`（默认值落位/未知字段拒/整体校验）+ 发布前 `validate_mod_objects`（直改与位置失同步封堵）。桥 ctx 增 target 对象读写（JSON 整上下文，单目标；多对象与 fast op 后置）。C.2 残留对账：自有槽接线✅、before-Err/replace-Err 引擎级专测✅、ctx 通道并入✅（单目标）。验证：workspace 502 绿、棘轮 1854 零翻转、bot 三基线同形过、零 Mod 开销 A/B 双样全相 ±2% 内不可分辨（见 [C.3 记录](../research/notes/c3-mod-objects-2026-10-03.md)）；残留九项登记同 note。

> **✅ 外审清偿片（2026-10-03）**：MiniMax-M3.1-Flash 对 B2..C3（64f2d91..3140f63）的外部复审报告逐条对 HEAD 实测复核后清偿——报告分级发现 9 项（P1×1/P2×4/P3×4）：8 项证实、1 项已由 C.4 部分清偿（snapshot 管理面）；另按槽判定扫描定性非活漏洞 1 项（零行为分歧，SlotOwner 枚举归后续结构化重构）。本片清偿：`object_json` 对 Mod 返回 `None`（"空对象+补造 pos"残影记录从类型上排除，`find_in_room`/`find_snapshots_in_room` 结构性过滤）+ ext 裁切 owner 双通道统一 `domain::object_owner`（消灭 JSON 记录键/枚举派生值潜伏分叉）+ `validate_mod_objects`/`validate_extensions` 改按候选内容短路（计划级布尔量 fail-open 消除——快照恢复等新来源不再可能无校验发布）+ `move_obj` 对 Mod 显式拒（原始索引移动不维护 payload↔信封位置不变量）+ `freeze` 拒 builtin 命名空间（`freeze_with_builtin` 双入口，保留名保证收回 kk-mod-api 自身）+ `create_shard` 失败语义注释改真 + 注释规范违例五处 + 管理读面（object_snapshot/CLI）权限契约句与 ServerOnly 钉住测试。登记：处理器异常"全分片失败半径+玩家可触发"运营危害与既定缓解（分型随 D 批 SDK 面、毒化为 E 批前置——见批表与设计 §7）；`graph.rs` dependency_order 的 Version newtype 归装载器后续。验证：workspace 525 绿（+2 钉住/回归测试）、棘轮 1854 零翻转、bot 三基线过（tooangel 的 lookFor 报错经 HEAD worktree 对照确认为既有形态）、零 Mod A/B 交错双样全相不可分辨（settle 点估计 −1.2% 反向）；判定总表与清偿明细见 [外审清偿记录](../research/notes/external-review-clearance-2026-10-03.md)。

> **✅ C.4.5 前置片已收口（2026-10-03）**：JS 侧 wrap `next()` 工厂接线的前置重构，三片递进——(a) 候选去生命周期化：`Working` 不再持借用（`settle_room` 按值收发房间，无逐 tick 克隆），`Plane` 派生物与 `Engine` 的 contrib/extensions 改 `Rc`/`Arc` 共享；(b) 处理器边界迁 `RoomCtx`（`Rc<RefCell<Working>>`）：六类处理器签名统一为 `&RoomCtx`，让「进 JS 或调 `next()` 前必须释放借用」由类型强制而非约定；`RoomCtx` 不进 OpState（重入 op 取的是挂起内层链 `NextAction`，候选句柄由该闭包捕获作入参）；(c) 槽载荷 `Rc` 化 + 重入 op：三族链载荷经 `Rc` 使内层链可导出为 `'static` 闭包，`kk-mod-host` 增 `op_mod_next`（`reentrant`）、`ModRealm::invoke_wrap`、sdk 按 kind 只给 wrap 传 `next`；扩展槽补齐 wrap 链（此前仅 link 两槽）。`next()` 载荷用新增的 `action_chain_below(index)` 跳过 `before` 与 `wraps[..=index]`，三个「整槽」导出仅供整槽重放（用错会让外层 wrap 双跑）。**装配门仍关闭**：宿主桥 next 工厂通道未接线，非空 wrap 仍显式拒收（引擎侧四链位已可执行，拒绝点是桥不是执行器）。评审清偿：`try_borrow_mut` 取代 `borrow_mut` 消除嵌套重入的 SIGABRT 面（降级为响亮结算错误，嵌套支持待下片）；三个静态导出补 after 等价测试（变异实测变红）；`reentrant` 明确非能力隔离。验证：workspace 536 绿、棘轮 1854 零翻转、bot 三基线同形、clippy/fmt/doc-links 通过。

> **⚠ C.4.5 的 reentrant 方向已被实验推翻（2026-10-03）**：本片把 `next()` 载荷做成 Rust 闭包（`NextAction`），内层链再进 JS 需重借 `ModRealm`，与外层 `RefMut` 结构性冲突。后续最小实验（[链驱动模型记录](../research/notes/mod-chain-driver-2026-10-03.md)）证明**该机制在解决一个不存在的问题**：链由 JS 驱动时，wrap 续行是 JS 闭包、native 中心是普通 op，宿主→JS 回入只有入口一次，两层 wrap 嵌套 + native 中心全程 0 个 `reentrant` 标记即可跑通。据此 `op_mod_next`/`NextSlot`/`invoke_wrap`/`action_chain_below`/三个 `'static` 导出的存在前提不成立；本片的 `Working` 去生命周期化与 `RoomCtx` 处理器边界仍必要并保留。是否据此重做待定，见该记录 §6。

> **C.4.5 执行接线（2026-10-03）**：采用 [ADR-015](../decisions/ADR-015-mod-chain-execution.md) 的双路径：无扩展 native 直调，JS 贡献整槽组合，native 经普通 op 改候选。删除 Rust 闭包续行与静态链导出，开放 link/自有动作的 JS wrap；next 前同步外层修改、返回后刷新上下文；故障留存不能被 JS/Rust 捕获消除。after 统一为 wrap 全部成功退栈后一次执行，取消中心仍可成功；当前实现与验证记录见 [整槽链接线记录](../research/notes/js-chain-execution-2026-10-03.md)。链驱动实验只证明机制可行，不能推广为一切重入都无必要。

> **✅ C.5 已收口（2026-10-04）**：外审登记三件落定——①**KKPV 双通道规模矩阵**（4/8/32 玩家 × JSON/KKPV × 5 轮独立进程双 重交错——通道序逐轮交换+规模序逐轮轮换防位置混杂，采集脚本含门禁+原子归档+20 项单测）：32p 5 样判读 wall +12.6%/CPU +20.7%/每玩家字节 −89.7%（方向与 B.6 双样一致、幅度偏窄，跨会话比值不可比；RSS −4.7% 方向维持、区间无重叠）；4p 墙钟收敛为 +49.7%（劣化是稳态非噪声）；8p 首组 +42.4%/−77.8%；CPU/wall 亏损随规模单调收窄但 4..32p 无回本点，RSS 于 8p→32p 间交叉反超；4p 档结算金丝雀出现 +11.5% 通道间差（区间不重叠，内存压力二阶效应候选，已登记入收敛观测面）；投影/编码/解码成本收敛手段维持归 C/D，默认通道维持 JSON（判读与边界见 [规模矩阵记录](../research/notes/kkpv-scale-matrix-2026-10-04.md)；双臂原始报告随批归档闭合 B.6 缺口 6）。②**SharedKkpvBundle 地形视图钉住测试**（ac3b248，embed 级 e2e）：跨 tick 钉住下 Rust 侧保留报告稳态下界、拷出语义隔离、旧视图存活且值不变、跨玩家完整性四性质锁定，变异 ×2 实测击杀；V8 堆内滞留量化（长窗+压力）维持登记。③**wrap/replace 验收**：既有结构覆盖由 C.4.5 执行接线（两层 JS wrap+link native 中心+JS replace+自有 process、候选读写序/取消/after 时点/失败重试）与双玩家恢复闭环（builtin wrap 累计+faulty 异常回退+跨房视野）共同覆盖。**C.5 收口时的残留登记**：零 Mod 开销 A/B 的 ≥5 样正式判读（C.2-C.4 各批双样后置项——本次矩阵是通道对照非开销基线，随 D 批性能片）；C.3 残留 10（Mod 每 tick 重解析成本观测，条件性）；native/empty 尾延迟维持「未归因、不声称零回归」登记（按平衡采集记录建议不再以合成 link 数字单方面追因，归因随真实负载闭环或可控实验）；矩阵不含 Mod 负载与共享房（mod-player 2p 数据点单列）。**C 批（C.1–C.5）切片全部收口**；下一批 D 按批表行（公开移动/战斗/建造/容量 slots、玩家执行接口收敛、处理器分型 SDK 面定论、同房/跨房串行参考一致），另有登记在案的移动族跨房出口过渡欠账（dual-room 批登记）应随 D 排期。


| 批 | 内容（06 §6 + ADR-014 调整） | 到批前须补 |
|---|---|---|
| B 开放领域定义 | EffectiveRules（平坦形态验收）、ObjectType/Extension、IntentDescriptor、handler graph、默认 slots；**与投影协议一步设计合并**（共享二进制 + per-viewer overlay + lazy wrapper + 扩展字段目录一次定协议，实施分步） | [定义与投影设计](../designs/domain-projection.md)已给出接口及切片；B.1 定义层已实现，link 运行接线及后续投影尚待完成 |
| C JS 最小完整玩法 | Mod loader、可信 VM、玩家 API 安装、自定义 FIND/LOOK、counter 对象与动作；一个既有结构的 wrap/replace | 依赖 B 契约冻结 |
| D 性能与真实规则 | 线程 CPU 计费收口（若 P1 未竟）、公开移动/战斗/建造/容量等必要 slots；同房/跨房串行参考一致；**06 §2.6 两条玩家执行接口收敛**（`Engine::tick` 假沙箱路径当前对非 Executed 结局照常推进 tick——收敛为 engine 只留纯结算/prepared txn 接口 + 限定测试 facade）；**处理器结果分型**（正常拒绝 Declined 与异常故障分开：拒绝走丢弃+计费+可选事件，异常维持炸候选——SDK 面冻结时定论，为运行期毒化铺路） | 依赖 B/C |
| E 首次持久化前 | extension 编码、manifest、迁移/缺失 Mod、输入日志与 outbox；**进程级崩溃注入只恢复完整版本**（A4 留下的验收面在此闭合） | 存储格式冻结前最后闸门；**运行期毒化须先行落地**（同一 Mod 连续故障达阈值即停用其处理器与 hook、路由意图丢弃计费并响亮记录，世界继续推进——消除"玩家可借缺陷 Mod 触发全分片确定性停摆"的唯一解除手段依赖重建分片的问题；阈值入注册选项） |
| F 网络/客户端随 M3 | JS routes/auth/subscription/providers、资产/renderer/panel/types、握手 | 与 F2 渲染器 worktree 的合流风险登记：worktree 领先基线仅触碰 kk-cli（export_fixtures）与 packages/{renderer,shared}、不改 kk-runtime/kk-runner——真风险是其 feed/export-fixtures 快照契约写于 pre-2B 市场/终端之前，**合流前须对 pre-2B 基线做契约重验与重放**，并考虑 A 批提交时序重构后的兼容 |
| G 完整能力收口 | 世界生成、周期任务、存储 provider、NPC/经济/跨房规则；[能力验收矩阵](../designs/mod-system.md#12-能力验收矩阵)全覆盖 + 真实大规模基准 | 全部前序批收口 |

**D 批切片（2026-10-04 定，现状核查见本日规模矩阵会话）**：

- **D1 跨房移动出口过渡**（dual-room 批登记欠账 + 棘轮 inter-room 红家族）：`movement_execute` 落边界格且方向外指（或站边界格向外 move）= 同 tick 过渡——结算期收集 pending transition，引擎级阶段应用到邻房（镜像位落点、身份/身体/仓库随对象保持、疲劳归 0；验收=screeps-ok ROOM-TRANSITION-001/002/005 转绿、003 保持绿）。现状：intent_move 越界即丢（settle/creep/movement.rs:67-69）、结算无 transition（dual-room-live-fire.md 登记）、Working 单房隔离（settle/mod.rs:166）——需新引入引擎级跨房阶段与邻房名换算（kk-core 坐标面核对）。边界登记：出口格 tick 起自动过渡（官方源码核对后定）、到达格占用/地形墙的处置、到达 creep 是否参与目的房当 tick 结算。
- **D2 寻路接入 a——PathFinder.search 地形直通**：kk-pathfinder（零消费者闲置，pub API 齐）接进玩家面：world 地形打包（注意 kk-core 行序 vs pack_terrain 列序）→ search → 玩家面 path 还原；path-finder.js 直线桩换真实现，findPathTo/findClosestByPath/moveTo/findPath 随之真化（m2b-expansion.md:294 排期落点）。

> **✅ D2 已收口（2026-10-04）**：PathFinder.search 经 `op_kk_pathfinder_search` 直连原生搜索——参数清洗/坐标转换按官方 driver 逐字（JS 数值语义留 JS 侧），地形两通道汇入（JSON：`{rev,payload}` 与夹具数组双形态、Sandbox 按 rev 缓存打包产物；KKPV：公开束懒解码，`SharedKkpvBundle::bytes()` 新增只读视图），进程级 `LOADED` 内容比对 + 注册房 8 邻域全墙环补有限世界边界；findPath=_findPath2 形态（range 1/跨房 maxOps 20000/plainCost 2）、findPathTo/findClosestByPath/moveTo（_move 缓存全流程）真化；roomCallback/costCallback 忽略归 D3（两例桩期偶然绿转红经 ratchet-allow 豁免归因）。**性能插曲**：链接 search 即 RSS +35MB（未执行亦然）——`kk_ffi.cc` 值形态 `thread_local` 池（4.32MB）在 macOS dyld TLS 实例化下由全部线程（含 V8 平台线程）支付物理内存，malloc_history 取证后指针化修复，perf A/B 全相回归不可分辨。棘轮 **1859→1879**（PATHFINDER-007~011/013~015/020/022+LEGACY-PATH-005 等 27 例转绿）；bot 三基线过（PathFinder.search/findClosestByPath 退出桩计数、tooangel 新触达四个既有 find 桩面——归因见记录）。实现、TLS 定位取证与全部验收证据见 [D2 记录](../research/notes/d2-pathfinder-terrain-2026-10-04.md)。
- **D3 寻路接入 b——roomCallback 通道**：按 [pathfinder-callback.md](../designs/pathfinder-callback.md) 契约（官方时序/异常/静默）接玩家 CostMatrix 回调；寻路 wrapper 槽位（mod-system.md:139）同批设计。

> **✅ D3 玩家回调已收口（2026-10-04）**：同步 V8 重入、按回调同步可变矩阵、四层嵌套、原异常对象与 getter 异常、取消主循环及终止后复用；Room 基础矩阵覆盖结构/creep/道路/安全模式，costCallback 克隆隔离，每 tick 选项缓存重建。棘轮 **1879→1887**，D2 两项豁免转绿清理，新增两项 stable vanilla 与套件不同的行为经源码取证登记。Mod wrapper 槽位仅完成设计，尚未接注册/投影准备（[设计 §8](../designs/pathfinder-callback.md#8-mod-寻路-wrapper-槽位设计未实现)）。验证与局部微基准见 [D3 记录](../research/notes/d3-pathfinder-callback-2026-10-04.md)。
- **D4 公开 slots 扩族**：移动/战斗/建造/容量四族具名 slots（默认 Native 直调、零 Mod 零开销同口径）；逐族定槽位切点与局部恢复边界。

> **✅ D4 移动片已收口（2026-10-05）**：`builtin:creep.move` 槽切点定在意图登记阶段——`Movement::add` 收紧为单执行者单请求不变量（纯 native 路径无既有登记，行为不变），JS 侧 `ctx.movement` 受控能力（pending/request/cancel）与 native 共用越界/静态障碍/孵化预检并同步意图时疲劳；拒绝恢复按执行者粒度快照/回插请求矩阵条目（保留格内登记序）+ `old_fatigue`，不触碰其他 creep 请求与 pull 关系；实际位移/道路磨损/工地踩踏/跨房过渡仍走仲裁与执行阶段，边缘格自动过渡与"拒绝本次移动请求"显式区分（验收钉住）。战斗/建造/容量三族与 link tick 拒绝仍开放中。协议与边界见[移动动作槽](../architecture/creep-move-slot.md)，验收见[D4 移动槽记录](../research/notes/d4-move-slot-2026-10-05.md)。

> **✅ D4 战斗片已收口（2026-10-05）**：五槽（attack/rangedAttack/rangedMassAttack/heal/rangedHeal）切点在意图处理阶段——对 creep 伤害/治疗走 `damage_to_apply`/`heal_to_apply` 延迟队列（对象 tick 施加并判死），结构伤害立即落 hits/废墟。`ctx.combat`（request/cancel）以新目标执行完整 native 语义并受恰一次消耗约束（request/cancel/native 先到者消耗，重复调用为故障；massAttack 无改向）。拒绝恢复双层：引擎快照受影响对象集（执行者/目标/格上 rampart；mass 为范围 3 全集）+ 延迟队列条目 + 事件 + 链内增删日志（`chain_mutations`，废墟插入删除、被移除对象连同伴随候选状态回插、ID 游标回退）；运行时首触日志恢复能力改向触达的链外对象。击杀被拒则目标存活无墓碑；反击伤害入攻击者队列并随拒恢复；spawn 摧毁连带的孵化 creep 移除由日志结构覆盖（暂无专测）。attackController/repair/dismantle 留原生（降级计时/建造族）。协议见[战斗动作槽](../architecture/creep-combat-slot.md)，验收见[D4 战斗槽记录](../research/notes/d4-combat-slot-2026-10-05.md)。
- **D5 Engine::tick 收敛**（06 §2.6）：engine 只留纯结算/prepared txn 公开面（OwnedTickTxn/PreparedTick 现 pub(crate)——按需升格）；假沙箱 tick 降级为限定测试 facade；RunSpec/RunResult 归位评估。
- **D6 分型定论**（SDK 面冻结件）：builtin 槽 decline 的局部恢复边界（link transfer 多对象写入的撤销面）、纯 Rust callback 签名定论、拒绝事件面设计（当前不发射事件）。
- **D7 性能与测量收口**：零 Mod 开销 A/B ≥5 样判读（C.5 残留）、同房/跨房串行参考一致验收（依赖 D1）、KKPV 投影/编码/解码成本收敛首步（4p 金丝雀二阶效应同观测）、C.3 残留 10（Mod 重解析成本，条件性）。


## 4. 每片通用验证矩阵

2026-10-03 的预算复审与密集负载片：清偿墙钟超时后错误格式化可再入 JS 的永久挂起；预算看门狗持续到解除武装，玩家模式保持单次投递。新增纯结算基准，核验完整处理链的各链位计数及全部 link 状态；测量、资源边界与后续方向见 [复审与密集负载记录](../research/notes/mod-budget-review-and-perf-2026-10-03.md)。

2026-10-03 的 C 批预算前置片：注册与整轮 Mod 调用累计 CPU/墙钟预算、V8 堆/堆外计数、超限候选失败与原配置重建；提取共用 kk-vm 执行机制，保留默认玩家预算语义。实现、故障矩阵与资源边界见 [Mod VM 预算记录](../research/notes/mod-vm-budget-2026-10-03.md)。密集 Mod 测量、进程级限额与故障停用仍待后续。

2026-10-03 的 C 批恢复前置片：Mod 结算失败即销毁 realm，下次执行前用原包快照与配置重新注册并核验原冻结计划；恢复失败保留玩家缓存并阻止新执行，丢弃轮次不能绕过恢复门。实施、故障矩阵与边界见 [Mod VM 恢复记录](../research/notes/mod-vm-recovery-2026-10-03.md)。注册/结算预算与确定性异常的处置策略仍待后续切片。

1. `cargo test` 全绿（受影响 crate）+ 棘轮零未豁免转红（地板 1854）。
2. bot 三夹具同既有口径（tooangel/overmind 2000 tick + wasm 10 tick）：0 硬中断、0 console error（触碰 runner/runtime 的片必跑）。
3. 新增单测锚定该片判据（故障注入/不变式至少各一）。
4. 性能相关片附测量（perf-probe 扩展或 P2 harness，含进程 CPU/RSS 采样）。
5. 提交说明引用本计划切片号与判据达成情况。
