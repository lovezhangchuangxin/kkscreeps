# ADR-014 贡献平面架构定型：mod 扩展性与性能的统一基座

- **日期**：2026-10-01
- **状态**：已接受（用户拍板：四项开放决策全部按建议定案）
- **前置**：[ADR-004](ADR-004-core-architecture.md)（房间快照/共享直读/增量通道）、[ADR-007](ADR-007-mod-system.md)（JS-only Mod）、[ADR-008](ADR-008-runtime-model.md)（同步主循环+数据并行）
- **依据**：[05 Mod 扩展能力调研](../research/05-mod-extensibility-review.md)、[06 架构复审](../research/06-architecture-and-js-mod-review.md)、[mod-system.md](../designs/mod-system.md)，以及 2026-10-01 架构再思考会话（对 05/06 全部关键断言的源码独立复核 + xxscreeps 机制核实，两路独立调查均确认）

## 背景

ADR-007 修订（2026-10-01）确认 Mod 作者只写 JS 后，仍有四个架构级开放问题：vanilla 玩法是否走与 Mod 同一贡献契约；玩家投影协议按何种节奏演进；首版玩法 Mod VM 的隔离等级；性能对照基线的排期。这四点共同决定贡献平面能否同时兑现项目总原则：性能优于官方与 xxscreeps、内存不超官方、玩家零差异、mod 扩展性不弱于两者。

独立复核确认 05/06 全部关键断言属实：`Action`（33 变体）/`RoomObject`（16 变体）封闭 enum 与多处（≥5 处）封闭 match 分派（intent 清洗、creep/结构动作分派、阶段逐对象分派、市场分派、schema 宏展开）使新增玩法必改 Rust；投影为每玩家每 tick 全量 JSON 重建（8 玩家档 wall 约 24%，玩家执行约 45%，结算仅约 5%——投影与玩家执行为两大成本）；`settle_market` 直接改 `self.market` 非候选；生产路径逐玩家串行等回执。xxscreeps 的 mod 能力本质是「一切皆注册表 + 加载期冻结 + 玩法即 mod」（classic 本身只是 mod 依赖包），代价是五套 TS/Node 特有基建、intent JSON 中转与 per-viewer 每 tick 全量 blob 拷贝。

## 决策

### 1. 六项架构原则

1. **贡献平面是支点：启动冻结，运行期全静态。** 所有 Mod 声明（对象类型/字段/intent/规则覆盖/handler/玩家 API/客户端贡献）启动时合并、校验、拓扑排序，冻结为 `ContributionPlan`；运行期一切查找是冻结后的数组索引或完美哈希，不存在运行期动态注册。动态性集中在启动一刻，运行期静态——同时拿下扩展性与性能两个目标（xxscreeps「加载期封闭世界」的 Rust 化加强版）。
2. **vanilla 走同一贡献契约。** Rust native 实现是贡献契约的*默认实现*，不是契约之外的特权路径：每个默认行为挂在可被包裹/替换的具名 slot 上，数值规则走 `EffectiveRules` 数据表。契约覆盖面 = Mod 能力面——xxscreeps 的 mod 完整性正来自 classic 自身即 mod；若 vanilla 走特权路径，契约将永远落后于实际行为，Mod 能力永远有暗缺。由决策 3 的评审门禁强制。
3. **冷热双轨 slot：无 Mod 零开销。** 冻结计划中每个槽位为 Native（直接调用，无 Mod 注册时）或 Chained（before/wrap/replace/after 链）二选一，分派形态冻结期决定；无 Mod 世界的额外成本上限为一次 enum match + 间接调用，「无 Mod 零开销/新增注册检查成本」是独立验收条目。
4. **三平面分离：权威结算 / 共享投影 / 贡献定义。** 权威平面 = typed 内建 enum + 扩展包络 + 全域 TickTxn 候选；投影平面 = 世界公共部分每 tick（乃至仅脏房间）编码一次、全部玩家共享只读字节，per-viewer 只叠私有 overlay，玩家 VM 内 lazy wrapper 按需读字段——这是 ADR-004 决策 1「共享内存直读+增量通道」的具体收敛形态，同时是性能（超越 per-viewer 全量重建）、内存（共享字节、未读零堆占用）与 Mod 可见性裁切（public/owner/server-only）的同一交汇层；贡献平面 = 纯数据契约（禁依赖 V8），可单测、可 fuzz、可生成 TS 类型。**投影协议一步设计到位、分步实施**（见决策 2）。
5. **全域 TickTxn，一次发布。** rooms、players（Memory/segments/CPU 账本）、market、全局 ID、Mod 状态、outbox 全部纳入同一候选；全部可失败阶段完成后一次 `publish`。Mod 状态（`ctx.state`）存 Rust 侧事务内，JS 堆只留可重建缓存（同时是内存优势：对照官方在 JS 堆内堆世界数据）。
6. **vanilla 零差异 + Mod 能力显式发布。** 无 Mod 世界严格 [ADR-003](ADR-003-player-compatibility.md) 红线（screeps-ok 棘轮为护栏）；启用 Mod 的世界在握手中显式发布能力/内容指纹。两条验收线分开，不互相绑架。

### 2. 四项拍板（2026-10-01，用户）

| 开放问题 | 决定 |
|---|---|
| vanilla 是否走同一贡献契约 | **是**（硬门禁，见决策 3；代价是 B 批起新玩法须多写 slot 声明与规则表条目） |
| 投影协议演进节奏 | **一步设计到位、分步实施**：共享二进制 + per-viewer overlay + lazy wrapper + 扩展字段目录是同一个协议，不是四个优化；先做 JSON 局部优化再另起二进制协议（导致玩家协议改两遍）禁止 |
| 首版玩法 Mod VM 隔离等级 | **共同可信宿主**：全部玩法 Mod 共享一个玩法 VM + 独立后台 I/O 执行域；Mod 间不构成安全边界（管理员安装=可信，与官方一致），玩家隔离不降级；需要逐 Mod 资源归因时再拆 VM/进程 |
| 性能对照基线排期 | **对照 benchmark harness 与 A 批并行**：本仓/本地 xxscreeps/官方私服三方同负载对照为第一等交付物与验收门；产出对照表前不得声称「性能超官方与 xxs」 |

### 3. 评审门禁（即刻生效）

新增封闭 enum 分派、硬编码类型/行为白名单或规则常量内联，必须同时提供具名 slot / `EffectiveRules` 数据通道，或在提交说明中显式登记豁免与清偿批次。此规则守护决策 1.2「契约覆盖面 = Mod 能力面」，与 screeps-ok 棘轮同为行为护栏。

### 4. 批次 A–G 接受（06 §6），四处调整

1. **市场纳入候选提前至 A 批早期**：`settle_market` 对房间访问已是「收集已结算房数组」形态，改造阻力小；完成前不得在其后插入任何可失败处理器。
2. **RunJob 合并注入/执行 + 有界并发 + 线程 CPU 计费与 A 批并行**（与 Mod 无耦合的纯性能轨；worker epoch 为 A 批状态机前置；计费口径切换不得晚于 M3 多用户承载前——[ADR-013](ADR-013-architecture-review-coordinator.md) 决策 6 既有锚点，实际随 A 批并行轨落地）。
3. **B 批（开放领域定义）与投影协议一步设计合并**，实施仍分步。
4. **不受边界影响的 vanilla 扩面可继续**，但受决策 3 门禁约束。

切片与判据：[mod-plane-batches](../plans/mod-plane-batches.md)。

### 5. crate 拓扑红线（目标态；初期可先立边界后拆包）

- kk-engine 不依赖 deno_core（现状已成立，维持）；
- 新增 kk-mod-api：纯数据描述与冻结计划契约，禁依赖 V8/deno_core；
- 新增 kk-mod-host：JS 装载、ops、回调与后台调度；engine 与 deno_core 的隔离通过它保持；
- 依赖方向图见 [06 §5](../research/06-architecture-and-js-mod-review.md)。

## 理由

官方用零扩展性换简单（mod 绑死 Node 进程内部）；xxscreeps 用全动态注册换扩展性（运行期仍是 JS 对象查表 + per-viewer 全量拷贝）。本架构把动态性集中在启动冻结一刻、运行期完全静态，并以「vanilla 同契约」保证契约覆盖面，以「三平面 + 投影一步设计」把性能、内存、Mod 可见性三个目标收敛到同一层解决。性能上结算已仅占 wall 约 5%，剩余大头（投影/玩家执行）恰由投影共享化与 RunJob 并发直接命中；内存上共享字节 + Mod 状态在 Rust 侧 + V8 startup snapshot 三项均指向「不超官方」。

## 后果

**正面**：mod 能力有结构性保证（非事后补丁）；性能/内存目标与扩展性不再互相牺牲；无 Mod 世界零新增热路径成本。

**成本与风险**：投影协议一次性设计成本前置；B 批起每个玩法多写 slot 声明；共同可信宿主意味着 Mod 间无隔离（恶意 Mod 可影响其他 Mod——与官方一致，接受）；对照 benchmark 需搭官方私服环境（Node+Mongo，重，可后置但须登记状态）；A 批触碰 tick 主链路，须以引擎结算测试与棘轮、bot 三夹具为网。

**与既有 ADR 的关系**：不推翻 ADR-004/007/008 任何决策；ADR-004 决策 1 的共享直读由决策 1.4 收敛为具体协议；ADR-007 的「实现方向」由本 ADR 升格为定型架构。投影一步设计**演进**（非推翻）三条既有表述：ADR-011 决策 1「投影 JSON 保持 M0 契约形状」与 ADR-012 决策 4 的 JSON 时间线被目标态协议族取代（实施仍分步，过渡期 JSON 通道继续作为行为护栏）；ADR-013 决策 2「列式/零拷贝由端到端测量决定」收敛为「协议族一步设计、具体编码形态仍由测量决定」，snapshot 契约的冻结前验收条款全部仍然有效。ADR-003 零差异红线的适用范围经 [ADR-007](ADR-007-mod-system.md) 与本 ADR 决策 1.6 收窄为「无 Mod 的 vanilla 世界」（已在该 ADR 追记）。

## 证据

- [05 Mod 扩展能力调研](../research/05-mod-extensibility-review.md)：官方/xxscreeps 能力证据、七个阻塞边界、Generic 链路探针。
- [06 架构复审](../research/06-architecture-and-js-mod-review.md)：性能探针（8 玩家 wall p50 2.265ms 及成分）、js_mod_host 再入实验、A–G 批次表。
- [mod-system.md](../designs/mod-system.md)：完整设计，含 **[ADR-014]** 定型增补标记。
- 2026-10-01 架构再思考会话：两路独立源码核实（本仓全链锚点复核；xxscreeps 机制核实——11 贡献面、三处冻结时机、tick 管线、blob 双缓冲、runner 每玩家每 tick 全量读可见房间 blob），确认 05/06 无与代码相悖的实质断言；复核摘要与锚点清单存档于[再思考笔记](../research/notes/architecture-rethink-2026-10-01.md)。
