# B0-6 外部评审清偿批（GPT 架构锐评复核）

> 状态：已收口（2026-09-30）。触发：外部 GPT 架构锐评经本仓逐条实测复核
> ——三组复现指控全部独立复现成立（探针取证后删除、未入库，已按序列复原为
> 回归测试）；本批清偿其中的正确性债务，架构级建议维持既有决策与排期
> （处置表见 §3，执行记录见 §4）。

## 0. 背景与复核结论

外部评审（2026-09-30，交叉检查架构文档/ADR/Rust 核心/JS prelude/兼容适配器/CI/
性能报告并自称独立复现）给出 7 组批评。本仓复核判定：**三组复现指控全部属实**
（本仓探针独立复现，数值与外部探针逐位一致）；其余多为已登记过渡态
（m2b 方案 §3.2/§7、ADR-013 决策 2/5 有出处），另有若干未登记债务本批登记
不修（§3 处置表）。

### 复现指控（本批回归测试即按探针序列复原）

1. **重试缓存销毁**（kk-runtime/embed.rs `run_players`）：先 `pending.take()`
   再校验键集，被拒的重试请求销毁缓存——原请求重试**重跑玩家代码**（本仓
   探针：`globalThis.n=2` 而世界仅 +1 tick）。违 tick 契约「不得为重试再次
   运行玩家代码」+ 验收场景 3「玩家代码执行次数仍为一次」；现有 b05 回归只
   测同参数 happy path，被拒路径零覆盖（B0-5 加键集守卫防 panic 时留下的洞）。
2. **负桶停跑回血绕过统一提交点**（kk-runner/budget.rs + sandbox.rs）：
   `begin_tick` SkipPenalty 分支即时落桶（探针：-40 → begin → -20、
   `pending_bill=None`、`discard_bill` 后仍 -20）——世界结算失败/丢弃轮玩家
   桶仍前进，违 tick 契约「CPU 桶宿主持久值必须暂存到提交点」。停跑语义
   本身（官方 make.js:72-83，budget 单测锁定）不变，错的是落桶时机。
3. **TickInput 重采样**（embed.rs `settle_world`）：重试重新取
   `TickInput::now()`，pending 只存 RunResult 不存输入——违 ADR-013 决策 5
   自述「重放用记录值」（受影响面：signController datetime 落盘值）。

另有一项复现指控（**不在本批修**，仅登记）：perf-probe 夹具空转——creep
(24..26,26)/source(10,10)(40,40)/controller(1,1) 全超动作距离且脚本无移动 →
harvest/upgrade 全部 ERR_NOT_IN_RANGE（API 预检拒绝、不产生 intent）、能量/
进度零变化；夹具注释自述「harvest/upgrade 走已就绪执行器」不成立。修复搭车
双房双玩家实弹验证批（另出方案）。

## 1. 修片规格

### 片 1：重试守卫前移 + 输入随轮缓存（kk-runtime/embed.rs）

- 键集校验移到 `pending.take()` **之前**：被拒请求 bail 时缓存原样保留。
- `pending` 从 `Option<BTreeMap<String, RunResult>>` 扩为
  `Option<PendingRound>`（`{ results, input: TickInput }`）：首跑在结算前取
  `TickInput::now()` 一次，失败轮连同 results 一起缓存，重试结算复用记录值。
- `settle_world` 改收 `input: TickInput`；`tick`/`settle_external` 维持
  `now()`（各自是独立新轮，非重试路径）。

### 片 2：停跑回血候选化（kk-runner/budget.rs + sandbox.rs + proto.rs）

- `Bucket::begin_tick` 负桶分支不再即时落桶；新增纯函数 `skip_recharge()`
  （官方公式 `(bucket+limit).max(−2×limit) − bucket`，make.js:72-83 钳地板
  语义不变）与 `apply_recharge()`（提交点并入，重算增量防陈旧候选）。
- `Sandbox` 新增 `pending_recharge: Option<f64>`，与 `pending_bill` 同生命
  周期：begin_tick SkipPenalty 分支暂存候选；`settle_bill` 同点并入；
  `discard_bill` 一并丢弃；begin_tick 开头防御清残留（同 B0-5 不变量）。
- `run_one_tick`（Cmd::Tick 通道——「无提交点概念、立即提交」）：SkipPenalty
  分支补即时 `settle_bill()`，保持该通道口径（否则 bot-check 通道停跑轮回血
  丢失）；`run_eval_tick`（runPlayer 通道）不动——候选留协调器处置。
- `EvalReport.bucket_after` 口径统一为「提交前」：原停跑轮是唯一即时落桶的
  例外，候选化后与 Run 轮（计费候选未落桶）一致；无消费方依赖旧值（适配器
  不读 bucket，compat/src 零引用）。

### 片 3：回归测试

- embed 链 ×2（embed_strictness_and_partial_commit.rs）：
  `b06_rejected_retry_preserves_pending_impl`（被拒请求后缓存幸存；原请求
  重试返回缓存 expr、`globalThis.n` 终值 1=执行恰一次）、
  `b06_retry_reuses_recorded_tick_input_impl`（signController intent + 失败轮
  → sleep 150ms → 重试 → sign.datetime 早于重试起点=记录值非重采样）。
- sandbox 链 ×1（watchdog_and_memory.rs）`b06_skip_recharge_candidate_impl`：
  −40 停跑桶不动/候选 20 → discard 后仍 −40（原实现此处已 −20）→ settle
  并入 −20 → 复跑至 0 → 恢复 Run；深负 −580 候选 540（−2×limit 地板）。
- budget.rs 既有负桶单测改候选化口径（begin 不落桶、apply_recharge 并入，
  官方数值断言不变）。
- sandbox_tests 根 `tick()` helper 的 Skip 臂补 settle_bill（helper 语义=
  模拟无回退协调器的即时提交，与 Run 臂对齐）。

## 2. 验证矩阵

workspace 全绿 / release+棘轮 1405 零未豁免转红 / bot 三基线（tooangel
console 零错人工确认 + overmind-dist + wasm-bot）/ perf 同口径 A/B 消融
（HEAD vs 本批；热路径仅每轮一次键集校验与 Option 检查，四相预期噪声级）/
≥3 路 subagent 评审 + 逐条清偿 / pnpm run check。

## 3. 处置表（外部评审其余建议）

| 建议 | 处置 |
|---|---|
| 显式 TickTransaction 状态机 | 方向即 tick 契约目标态；本批只修实现洞、不加框架 |
| CPU 桶权威值迁宿主 PlayerState | 候选化已闭合提交点泄漏；权威值迁移随 M2B 存储批评估 |
| InfraFailure 持久故障态 + 按 P_t 重建 | 登记 tick-contract「仍为目标」（现状：单轮 Err、无故障态，下次调用在原沙箱开新轮） |
| proto.rs 命令回执 recv() 无超时 | 登记 sandbox.md（线程活而卡死 → 协调器永久挂起；M3 多用户承载前清偿） |
| ObjId→Room 索引 / next_obj_id 全量扫描 | 登记（M2B 扩面前候选小片） |
| 领域语义模块归属（controller_progress_total_opt 等移出投影模块） | 登记（M2B 扩面前候选小片） |
| typed FIND（先 typed 匹配再投影） | 不采纳（m2b-prerequisite §2 显式不做：JS 镜像面手写 + 层间对表） |
| 真假执行器统一协调器 | 假路径 seam 化已显式延后（m2b 方案 §2）；随并行化批 |
| perf 夹具修复 | 搭车双房实弹验证批（本批仅登记） |
| bot-check 静态世界 | 文档明示的验收边界，不修 |
| 计费提交失败吞错 | B0-5 已登记低危取舍，维持 |
| WorldSource 返 Value / pub rooms / room_mut 禁写地形 / RoomVersionHandle 仅元信息 / Working 并行障碍 | 已登记过渡态（ADR-013 决策 2、m2b 方案 §3.2/§7 P2-P3），维持 |

## 4. 执行记录（收口，2026-09-30）

**切片落地（单提交）**：

- 片 1（kk-runtime/src/embed.rs）：`PendingRound{results, input}` + 键集校验先于
  take + `settle_world` 收 input 参数；`tick`/`settle_external` 维持 now()（独立
  新轮）。守卫注释补子集语义（重试以缓存轮为原子单位——评 1 P2-1）。
- 片 2（kk-runner budget.rs / sandbox.rs / proto.rs）：`skip_recharge`/
  `apply_recharge` 纯函数化（官方公式与 −2×limit 地板不变）；`pending_recharge`
  候选与 pending_bill 同生命周期（含互斥 debug_assert——评 2 P2-3）；Cmd::Tick
  通道 Skip 臂即时 settle_bill（bucket_after 与原即时落桶口径逐位一致）；
  Cmd::RunPlayer 通道候选留协调器（bucket_after 统一为提交前口径，消费方零）。
- 片 3（回归测试）：embed 链 `b06_rejected_retry_preserves_pending_impl`
  （轮 C 请求故意用计数代码——expr==1 与终检 n==1 双杀伤线，评 3 P2-1）+
  `b06_retry_reuses_recorded_tick_input_impl`（预热轮挡首跑装配耗时，评 3
  P2-2；prelude 的 datetime 为官方 ISO 串形态，JS 侧 `new Date().getTime()`
  还原毫秒）；sandbox 链 `b06_skip_recharge_candidate_impl`（discard/settle
  双臂 + 深负地板）；proto 链 `skip_penalty_recharge_channel`（两通道停跑臂
  通道级锁——评 3 P1-1 清偿；**杀伤力已验证**：临时删 Skip 臂 settle_bill，
  测试在 bucket_after 断言精确变红后还原）；budget 单测改候选化口径；
  sandbox_tests 根 tick() helper Skip 臂对齐即时提交口径。

**验证矩阵**：

- workspace 全绿（31 测试目标 0 失败；清偿轮复跑保持）。
- release+棘轮：**1405/2914 地板保持，相对 HEAD~1 零转红零转绿**（1 条豁免
  未命中警告为既有登记）。
- bot 三基线：tooangel 2000t console 零错（ticks_with_console_error=0）、
  overmind-dist 2000t（setPublicSegments/stub-flag 既知基线形态）、wasm-bot
  10t 零缺失；三者 ticks_skipped_penalty=0——停跑路径未被任何负载触及，其
  保证由 budget 单测 + sandbox 链 + proto 链三层测试承担。
- perf 同日 A/B（HEAD vs 本批，2 玩家 × 预热 100 + 采集 1000 ticks）：
  project 0.104→0.103 / run 0.141→0.154 / settle 0.009→0.010 / wall
  0.392→0.418（ms，p50）——四相噪声级（settle 金丝雀持平），无归因必要。
- 三路 subagent 评审（正确性/通道/测试三视角）：P0×0、P1×1、P2×9。P1 已
  清偿（上述 proto 链测试）；P2 处置见下表。
- 清偿轮仅触及测试/注释/debug 断言——release 行为面零变化，棘轮/bot/perf
  结果对最终态保持有效。

**评审 P2 处置表**：

| 项 | 处置 |
|---|---|
| 评 1 P2-1 子集重试语义未文档化 | 已清偿（守卫注释补子集句） |
| 评 1 P2-2 apply_recharge 浮点 1ulp 理论差 | 登记不修（仓内全整数桶配置，单测锁定数值） |
| 评 1 P2-3 settle_terminated 无生产调用方、budget 模块头注释陈旧 | 登记不修（B0-5 起走 after_termination 候选的预存债务，非本批引入） |
| 评 1 P2-4 sandbox.md 看门狗措辞 | 已清偿（改为「V8 JS 执行区段」） |
| 评 2 P2-1 embed 层停跑处置无回归锁 | 部分清偿+登记：proto 链已锁通道级（run_player 通道 kind/候选 None/提交前口径 + settle 恰一次）；embed 层驱动负桶需 SandboxConfig 旋钮（create_shard 硬编码 default）——残余风险=未来给 commit_players 遍历加结局过滤会静默丢回充，登记为 M2B 扩面前候选小片（测试旋钮或结局盲断言） |
| 评 2 P2-2 防御清残留零可观测性 | 登记不修（与 B0-5 pending_bill 同款沉默语义一致；可观测性加固归后批） |
| 评 2 P2-3 settle_bill 双臂互斥未钉死 | 已清偿（debug_assert 互斥不变量） |
| 评 2 P2-4 commit 注释未点名注入失败 bail | 已清偿（注释补 `?` 家族） |
| 评 3 P2-1 终检鉴别力与注释不符 | 已清偿（轮 C 改用计数代码，expr 与终检双杀伤线） |
| 评 3 P2-2 TickInput 测试时序余量 | 已清偿（加预热轮）；NTP 回拨 >50ms 的假红窗口为墙钟域取证固有取舍，登记 |
| 评 3 P2-4 空 codes/子集/连续被拒边界无测试 | 登记不修（既有语义非本批引入；子集语义已补注释） |
| 评 3 P2-5 tick() helper Skip 臂今日不可达 | 保留（一致性改动；负桶 fixture 已由 b06/proto 用例建立，helper 后续触达即生效） |

**清偿复审（同日，单路 subagent 复核清偿增量）**：P1/P2 清偿逐项忠实落实（含杀伤
力与 workspace 全绿的独立复现——复审方在 /tmp 干净副本删 Skip 臂 settle_bill 重
演变红）、登记不修项三项事实依据抽查全部成立（settle_terminated 零生产调用方、
create_shard 硬编码 default 配置、桶配置全整数且 settle 取 ceil）、三路评审共识
无集体误读、清偿增量未破坏串行链/LIFO/销毁纪律。**发现一处清偿自身引入的注释
失实**（子集句「仅返回所请求子集的回执」——实际返回全量缓存玩家回执），已随
复审更正提交。判定：清偿成立，免除外部复审。
