# 双房双玩家实弹验证批（dual-room live-fire）

> 状态：**已收口（2026-09-30）**——方案经用户确认（含 O1+O3 纳入、
> O2 缓行的拍板），五片全部落地（ec771e2→36d25b7），切片执行与评审
> 清偿记录见 §7。
> 外部复审：两轮独立复审清偿完毕（0b83f1d，含第一轮 note 八条处置与
> 勘误标注）；第三轮经用户拍板免除（08302c1）——记录见 §7 末三节。
> 定位：2026-09-29 架构复审定义三件之一（M2B 前置批时脱钩另出方案，见
> m2b-prerequisite 修订记录），**M2B 扩面（~150→340 方法）之前的最后一道实弹
> 关卡**。核心交付=测试与夹具；引擎行为面预期零改动（除测试旋钮与经拍板的
> 可选小片）。

## 0. 背景与目标

在真实全链路（EmbedEngine：per-viewer 投影 → 真实 V8 沙箱玩家代码 → intent
清洗 → 多房结算 → 统一提交）上验证：

1. **跨房 tick 合并规则**（[tick 契约](../architecture/tick-contract.md)验收
   场景 2 的三形态 + 顺序不变性）；
2. **真实玩法验收**（场景 1 的多房形态：成功动作断言、共享资源与竞争）；
3. **结算失败恢复的双房行为**（场景 3 的多房形态——B0-6 刚修完的三件事在
   双房双玩家下的表现）；
4. 搭车：**perf-probe 夹具空转修复**（B0-6 登记项，先行动作——本批 perf
   工作基线以修复后夹具为准）。

## 1. 现状勘测（关键代码事实，2026-09-30 于 0edbe5c）

- **协调器**（kk-runtime/src/embed.rs `run_players`:398）：执行序 =
  `players()` 返回序（注册序，实现期核实保真）；结果收集进
  `BTreeMap<handle, RunResult>`（handle 排序）；意图合并
  `batch_from_results` 按 handle 序；`settle_world` →
  `engine.tick_with_intents_with_input(batch, input)`。
- **投影可见性**（kk-world world_json.rs `world_json_for_viewer`:31）：查看者
  **有对象的房即可见**（flag/工地不授视野，官方 data.js 视野归集对齐）；
  不可见房仅地形。→ 一玩家在两房各摆对象即双房可见；两玩家在同房各有对象
  即共享房互见。`Game.rooms` 断言与跨房 room 级 intent（createFlag 等）自然
  可达。
- **结算**（kk-engine settle/mod.rs `tick_with_intents_with_input`:610）：
  **两阶段提交**——逐房（房名序，BTreeMap keys）在**克隆副本**上结算，任一
  房 Err 整体放弃（世界/gcl/id 分配器/time 全保旧）；全部成功统一写回并
  time+1。同 tick 跨房隔离是结构性的（每房克隆自已提交态）。
- **意图批**（kk-engine intent.rs:364-373）：`IntentBatch{users:
  BTreeMap<玩家, UserIntents>}`；对象级意图按 ObjId（全局唯一）自然分房
  路由，房级意图（createFlag 等）带 roomName 显式路由。
- **失败注入面**：`EmbedEngine::fail_next_settle_for_test`（整轮，embed.rs:686）
  已有；`Engine::fail_room`（房级，settle/mod.rs:541 私有字段）仅 crate 内
  单测可达——embed 集成测试做房级注入需新增测试旋钮（片 3）。
- **既有覆盖盘点（本批不重复做）**：
  - `embed_run_players_shared_snapshot_impl`：两玩家一房、同一快照、合计 +1；
  - b05/b06 embed 链：失败回退/被拒重试守卫/TickInput 复用——**单房形态**；
  - 引擎级 `fail_room` 两阶段回退单测（settle/mod.rs:717/743）——假沙箱/
    纯结算层；
  - ADR-013 决策 5 重放一致性（引擎级双房：同初始世界+同 intents+同
    TickInput 逐字节一致）。
  - **空缺（本批交付）**：双房双玩家**真沙箱全链**零覆盖；共享资源竞争零
    覆盖；玩家执行顺序不变性零覆盖；perf 夹具空转。
- **ratchet-allow 现存 1 条豁免**：inter-room creep transition（用例现红 =
    跨房移动过渡能力缺口，结算侧无出口过渡逻辑——movement.rs 全文无
    transition）。**本批不触及（见 §5），豁免保留**。

## 2. 切片规格

### 片 1：perf-probe 夹具真实化 + 负载自检（先行——建立本批 perf 基线）

- **现状**（B0-6 实测复现）：creep(24..26,26) / source(10,10)(40,40) /
  controller(1,1，create_shard 硬编码 embed.rs:304) 全超动作距离
  （harvest≤1、upgrade≤3）且脚本无移动 → harvest/upgrade 全部
  ERR_NOT_IN_RANGE（API 预检拒绝、零 intent）、能量与进度零变化；夹具自述
  「harvest/upgrade 走已就绪执行器」不成立。
- **修法（对象摆进距离内；零引擎改动）**：controller 保持 (1,1) 不动，
  source/creep/spawn 迁至其邻域。约束：每 creep 到某 source Chebyshev ≤1
  且到 controller ≤3——脚本按 store 分支（有能→upgrade、空→harvest），
  两条臂都在程内才有稳态 intent 流。spawn 迁近使新孵 creep 出生即工作。
  坐标实现期定并写入夹具注释。**备选（脚本加 moveTo）不取**：引入寻路
  负载与移动结算噪声，破坏基线可比性（旧夹具本就无移动维度）。
- **负载自检（防再空转，B0-6 指控的结构性防复发）**：采集循环后经公共
  读取面（find_in_room/get_object）断言真负载发生——controller progress
  增量 >0 且至少一 source 能量低于初始容量；不满足则非零退出并显式报错。
  report JSON 增补负载计数（progress_delta / source_energy_delta）供 A/B
  归因留痕。
- **同日 A/B 重取**：旧夹具 HEAD vs 新夹具各 ≥2 跑（**非回归门禁**——负载
  变了数字必然动，记录用）；新工作基线（2 玩家为主，加 4 玩家缩放点）记入
  §7。预期：settle 相上升（真 intent 结算）、run 相或微升（intent 序列化）、
  project 相不变（对象数与形状不变仅坐标）——project 若动必须归因。

### 片 2：跨房合并三形态实弹（验收场景 2）

新测试文件 `crates/kk-runtime/tests/embed_tests/dual_room_live_fire.rs`
（子模块 0 `#[test]`、`*_impl` 经 glob 进根链追加调用——串行链不变量）。
每场景独立 EmbedEngine、teardown 必达（失败路径也不泄漏线程）。

- **场景 A「一玩家影响两房」**：p1 于 W0N1（own，rcl4：source+
  controller 邻域 creep→upgrade 臂）与 W1N1（野房：source 邻域 creep→
  harvest 臂）。脚本返回 `Object.keys(Game.rooms)` 断言双房可见；结算后
  W0N1 progress 增、W1N1 source 减/creep store 增、time +1；p1 的
  RawMemory 恰提交一次。同一 RunResult 携带两房对象级意图即跨房合并
  路径（~~跨房 room 级 intent（createFlag）~~——**实现期勘误**：
  createFlag/createConstructionSite 在 prelude 均为 countStub（room.js:477/
  489），room 级 intent 无玩家可达 API 面，该子项撤销、登记归 API 扩面）。
  野房夹具一律 `controller: false` 且 source 摆 4000/4000——野房容量逐
  tick 归一（有 controller 的野房→NEUTRAL 1500 且钳 energy、无
  controller→KEEPER 4000；economy.rs source_tick），摆同值免归一差。
- **场景 B「两玩家影响一房」**：p1/p2 各有私房 + 双方 creep 共处共享房
  SHARED，**采同一 source**（资源竞争）。断言：source 扣减 = 两玩家采集和
  （意图按玩家名序处理，期望值按该序手推）；两 creep store 各自增加；两
  玩家脚本均见 SHARED。
- **场景 C「无关房先完成」**：B 布局 + p3 独居房名序更后的远房。断言：p3
  房终态与「单独跑 p3 的同参世界」逐值一致（同 tick 跨房隔离的结构性保证
  钉成 embed 级回归）；p1/p2 终态与无 p3 时一致。
- **「打乱玩家完成顺序结果不变」**：同世界两引擎、同 codes 各跑同 tick 数 →
  逐值断言终态一致（各房 source 能量/creep store/controller progress/
  time/两玩家 memory）。执行序经 **test-only 执行序旋钮**打乱——**实现期
  勘误**：`players()` 返回 `handles.keys()`，handles 为 `BTreeMap`（lib.rs:52）
  →注册序天然被排序吸收，注册排列不构成自由度；fallback 旋钮（预登记项）
  转正：`EmbedEngine::set_execution_order_for_test(order)` 只改
  execute_players 循环序（投影+执行完成序），结果收集/意图合并/结算序不动。
- **多 tick 稳态**（约 50 tick，场景 A/B 世界）：progress 累计=期望斜率、
  能量守恒恒等式（窗口内 Δsource + Δspawn 存量 = 采集量 = 升级消耗 +
  store 增量；期望从 kk-core 常量与实测 store 推导，不硬编码未核实的
  regen 语义——夹具 source 起始满容量且窗口内不采空则无 regen 项）、无
  意外对象增生。
- **串行参考口径（本批）**：现单线程协调器的既定序（玩家名序执行 + 意图
  handle 序合并 + 房名序结算）**即**串行参考；本批钉「结果不依赖可打乱的
  自由度（执行序/注册序/建房序）」+「语义正确（守恒核算）」，作为将来
  并行化批（D_t/RoomPatch 落地时）的对齐基线。

### 片 3：真实玩法验收 + 失败恢复双房化（验收场景 1/3 的多房形态）

- **成功动作断言（双房上下文）**：采集量（source −2×work/creep/tick、
  store 对应增加）；资源变化（spawn 能量 −200 孵化）；controller 进度
  （upgrade +1×work/tick）；**对象生命周期**——孵化链（spawnCreep →
  spawning → 新 creep 出现于世界，FIND_CREEPS 计数增长）+ 建造链
  （工地经夹具摆放（`place_object kind="constructionSite"`——实现期
  勘误：createConstructionSite API 为 stub，见 §2 场景 A 勘误）→ build
  推进 → complete_construction 转正为结构；Action::Build 已在清洗白名单
  intent.rs:88）。creep 死亡/tombstone 生命周期不覆盖（无短 ticksToLive
  触发面，战斗归后批，登记）。
- **结算失败恢复（双房双玩家全链）**：新增 test-only 旋钮
  `EmbedEngine::fail_next_settle_room_for_test(room)`（委托 Engine::fail_room
  经方法开放；一次性语义与 fail_next_settle 对齐）。流程：正常跑数 tick →
  注入次轮房级失败 → `run_players` 返回 Err：**两房终态均不变**（含未失败
  的那房——两阶段提交跨房回退的 embed 全链形态；引擎级已有单测，此处为
  真沙箱全链）→ 两玩家 memory/桶不变 → 重试 `run_players`（同 codes，
  计数代码 `globalThis.n` 防重跑）→ 两房一并提交、执行恰一次。
- **B0-6 三件事的双房行为**：①被拒重试不销毁缓存（双玩家键集形态）②
  TickInput 复用（失败轮→重试，双房下世界两处一并推进不双跑）——各一条
  增量回归；③停跑回充候选化的 embed 层驱动需 SandboxConfig 旋钮 =
  **可选片 O3**，未纳入则维持既有三层测试（budget 单测/sandbox 链/proto
  链）覆盖并沿用 B0-6 登记口径。

## 3. 验证矩阵（每片完整批次循环）

写规格 → 实现 → workspace 全绿 / release 重建后棘轮 1405 零未豁免转红 /
bot 三基线（tooangel console 零错须人工确认）/ perf 同口径 A/B 消融（片 1
起以新夹具基线） / 视批次（本批交付即测试：新测试全绿且断言口径可归因）/
≥3 路 subagent 并行评审 → 逐条清偿 + 清偿复审 → 沿革回写（roadmap 批次
条目 + 本方案 §7）→ pnpm run check → 单一归因提交（subject ≤350 字符且
冒号前 ≤48 字）并 push。

## 4. 可选小片（**待拍板**）

- **O1 ObjId→Room 索引**：现状 `next_obj_id()` 每次 insert 全世界扫 max
  （world.rs；`tick_with_intents` 每 tick 亦扫）、`get/remove` 逐房查。
  方案（**落地口径——实现期订正，见 §7**）：generation 代际懒重建目录
  + insert 增量续档（非逐可变面维护——复用既有代际不变量兜底；快照
  字节形态不变、reset 归零）；max 走分体零分配扫描缓存（结算每 tick
  路径与原同价）；读侧 O(log N)。现状 2-4 房收益小（小规模首房命中
  常数反转），价值=insert 序列 O(N²)→O(N log N)（storage-bench T3 实测
  55×）+ O() 形状先行于 M2B/2B 增长。量级：小片。
- **O2 领域语义模块归属**：controller_progress_total_opt/store 家族 helper
  从投影模块（snapshot.rs/world_json.rs）移 kk-world 领域模块，纯搬移零
  行为（B1 纪律）。**建议缓行**：随 M2B 扩面首片搭车（新增 ~190 getter 时
  一次性归属到位，免两次搬）。
- **O3 embed 层停跑回归锁**：EmbedEngine 的 SandboxConfig 测试旋钮
  （create_shard 硬编码 default → 可注入），驱动负桶停跑走 embed 全链，
  断言 commit_players 结局盲语义（Terminated 玩家回充候选不丢）——清偿
  B0-6 评 2 P2-1 登记的残余风险。量级：小片。

推荐：**O1、O3 纳入，O2 缓行**——按拍板执行。

## 5. 不做清单

- **跨房 room 级 intent 不覆盖**：createFlag/createConstructionSite 玩家
  API 面为 countStub（room.js:477/489——双房批实测取证；m2-playable 4.3
  的「已交付」表述已勘正：交付的是 intent 通道+结算执行器，API 面归
  M2B 扩面）——「一玩家影响两房」由对象级意图（两房各摆 creep）承载。
- **跨房移动过渡不实现**（creep 出口格→邻房出现）：移动族后续批
  （movement.rs 无 transition，豁免条目归因明确）；本批场景**不依赖跨房
  移动**（夹具直接两房各摆对象），ratchet-allow 现 1 条豁免**保留**（用例
  红 = 能力缺口如实呈现，转绿销账归移动族批）。
- 并行调度 / D_t 依赖集合推导 / RoomPatch / 空闲房活跃条件参与：tick 契约
  目标态，本批只钉现串行实现的确定性基线（现状：全部房每 tick 无条件参与
  结算）。
- typed FIND / seam 收窄 / InfraFailure 持久故障态 / proto recv 超时：既有
  决策与登记排期维持（后两者 M3 前）。
- 战斗/死亡生命周期（tombstone/ruin）、市场、power：后批。
- 不动 packages/、不碰 .worktrees/renderer/（并行前端线隔离）；不改
  snapshot 契约字节形态（schema v1 冻结）。

## 6. 风险与工具链钉子

- `players()` 已核实返回 BTreeMap 键序（注册序被排序吸收）——执行序自由度经测试旋钮（§2 已更新为转正口径）。
- 竞争期望值按「玩家名序意图处理」手推；不对照官方 MMO 真并发（那是
  并行化批的验收面）。
- 新 V8 测试并入 embed_tests 串行链（根唯一 #[test]、子模块全 0）；多
  EmbedEngine 各占线程，串行链内天然安全，teardown 必达（含断言失败路径
  ——防线程泄漏污染后续链测试）。
- perf 基线同日重取（测量学纪律：绝对值不跨会话）；单跑离群以「未动相
  劣化」甄别（settle 相金丝雀）。
- 通用防错坑按项目记忆清单：python 插测试锚点须含 `#[test]` 属性行 /
  ratchet 跑完 `git checkout -- compat/reports/kkscreeps.json` / cargo fmt
  reorder_modules 登记 / zsh `===X===` glob 坑 / Bash cwd 显式 cd 回主仓 /
  ratchet 前显式 release 重建。

## 7. 执行记录

### 片 1（perf 夹具真实化 + 负载自检）——已收口

- **落地**：对象迁入作用距离（controller(1,1) 硬编码不动；source
  (2,2)/(1,3)、spawn(3,3)、放置 creep (2,1)/(1,2)/(3,1)；脚本
  spawnCreep 加 `directions:[TOP, LEFT]` 控制出生落位 (3,2)/(2,3)——
  第三只兜底落 (4,2) 程外如实注记）；布局常量单源 + 距离预算微测
  `fixture_distance_budget`（占位互斥/首源=FIND_SOURCES[0] 取序耦合一并
  锁死）；负载自检双臂口径——progress 增量>0 硬失败（B0-6 空转形态的
  主判）+ 采集证据双边界采样（窗口 ≥100 且两边界全满容量才判死：单边界
  落 source 300-tick 再生回填点会假红，双边界同时命中缩小 ~300 倍）；
  `--ticks 0` 显式拒绝；报告 workload 字段（progress_delta/双边界能量）。
  kk-cli 新增 kk-core workspace 依赖（FIND 常量；既有传递依赖显式化，
  评审 3 判「更轻替代不成立」）。
- **评审三路（正确性/负载方法学/工程面）**：P1×3、P2×6。清偿：评 1 P1-1
  出生位注释失实（born_creep 取 spawn 首个空闲邻格而非 spawn 位——原注释
  「出生即工作」不成立）→ directions 控制落位+注释如实+微测锁预算；评 2
  F2a/评 3 P1-1 同源自检假红（模式 A 短窗口首采未启、模式 B regen 回填点
  重合——评 3 三组实测复现）→ 双臂重设计（上述），三组参数复验全转绿；
  评 2 F1 project +4% 非噪声（**事件通道激活**：旧夹具零事件、新夹具每房
  每 tick ~5-6 条 harvest/upgrade 事件注入 per-viewer 投影）→ 本段归因；
  评 2 F3 字段名（双边界能量入报告）；评 2 F4 source 放置序耦合注释钉死；
  评 3 P2-1 提交态产物脱节 → compat/reports/perf-probe.json 重生成；评 3
  P2-2 perf-a6.md/performance-acceptance.md 口径注记补齐。**评审间矛盾实
  测复核**：评 2 称 spawn 有 +1/tick 雁流（spawn.rs:494-517）、评 3 称只
  孵 1 只——实测评 2 对（雁流在，种群 t≈309 达 6）。登记不修：自检单房
  抽样（各房同构）、部分退化盲区（工作 creep 部分超距自检仍绿——微测+
  注释双保险）、TTL 死亡分段 ≥~1500 tick（M2B 长窗注记）、refill 刀刃
  双边界理论残余。
- **同日 A/B（2 玩家 p50，旧空转 → 新真负载）**：project 0.102-0.103 →
  0.124-0.125（+21%：事件通道激活+工作 creep 3→5）、run 0.143-0.148 →
  0.208-0.211（+44%：intent 序列化）、settle 0.009 → 0.032（3.6×：真
  intent 结算）、wall 0.398-0.409 → 0.463-0.476（+16%）。4 玩家：
  0.222/0.352/0.019/0.903 → 0.266/0.469/0.064/1.035。**本批工作基线
  （M2B 扩面 perf 对照）=2p 0.124/0.209/0.032/0.467**（提交态产物
  perf-probe.json 同数字；三跑稳定、settle 金丝雀形态健康）。负载计数：
  progress 3328/1000t、source 2836→2366。
- **验证**：workspace 全绿（kk-cli +1 微测）/ clippy 零告警 / release+
  棘轮 1405 零未豁免转红 / bot 三基线（tooangel console error tick=0、
  overmind-dist 2000 错 tick=既知形态、wasm-bot 零错；skipped_penalty
  全 0）/ 假红三参数复验转绿。

### 片 2（跨房合并三形态实弹）——已收口

- **落地**：新测试文件 embed_tests/dual_room_live_fire.rs（子模块 0
  #[test]、根链追加五连调用）+ 执行序测试旋钮
  （`execution_order_for_test`/`set_execution_order_for_test`——只改
  execute_players 循环序，结果收集/意图合并/结算序不动；生产 None 路径
  与 HEAD 逐行为等价，评审 2 逐点证明）。五场景：A 一玩家两房（双臂
  数值断言+双房可见性+memory 提交恰一次+time）；B 两玩家一房（竞争
  source 双份扣减+**稀缺变体** source=2——玩家名序杀伤线：p1 足额
  p2 颗粒无收）；C 无关房双向（E9N9 字典序先结算+W9N9 后结算，各与
  单独跑逐值一致+SHARED 不受远房影响）；顺序不变性（旋钮打乱执行序
  +建房序打乱——终态逐值一致：source/creep/time/own 房 progress/两玩家
  memory 五面）；多 tick 守恒（A 世界 Δsource+Δstore+Δprogress=0 精确
  恒等+B 竞争世界 Δsource=Δstore_p1+Δstore_p2，50 tick=满仓停采分界
  两倍窗）。
- **实现期实测取证**：满仓继续 harvest 的超容部分按官方语义掉落
  （harvest.js:43-60）且小堆每 tick 衰减 ceil(amount/1000)——守恒式
  曾差 −50/−25 两轮定位为掉落与衰减项，非引擎缺陷（引擎级
  combat_heal.rs 已锁该路径）；脚本改满仓即停保持恒等式无缺项。
- **评审三路（场景语义/协调器影响面/测试方法论）**：P0×0、P1×4、
  P2×11。P1 清偿：①顺序不变性断言面补 memory+own 房 progress（方案
  明列子项）②场景 C 结算序注释失实（E9N9 实为先结算——恰合「无关房
  先完成」，注释与方案反向）+补 W9N9 后置取向双向覆盖 ③旋钮生效取证
  面（`last_execution_order_for_test` 记录实际迭代序，测试断言注入序
  生效——空转通过面消除）④竞争稀缺路径变体（source=2 名序杀伤线）。
  P2 清偿：场景 A/bworld 补 time 断言；HARVEST_CODE 补 memory+满仓门；
  FIND 常量改 kk_core 引用；守恒扩 B 世界；排列校验 debug_assert 改
  无条件 assert（Some 臂内零生产开销，release 误置从静默变早失败）；
  §5 补跨房 room 级 intent 条目+§6 players() 已核实+m2-playable 4.3
  「room 级 API 已交付」超前宣称勘正（实况：intent 通道+执行器已交付、
  API 面 countStub——git 史证实自 A3a 起即 stub）。登记不修：魔数期望
  值保留字面+注释依据（引用常量反损可读性）；场景 C 事件流面（跨房
  事件路由）不比——getEventLog 面归后续批；50 tick 窗口选型依据已入
  注释。
- **验证**：workspace 全绿 / clippy 零告警 / release+棘轮 1405 零未豁免
  转红 / 三基线（tooangel console error tick=0、overmind/wasm 既知
  形态、skipped_penalty 全 0）/ perf 同口径 A/B 噪声级（0.124/0.204/
  0.032/0.458 vs 基线 0.124/0.209/0.032/0.467——旋钮 Option 判别零
  开销）/ 守恒恒等式杀伤力抽查（期望改 1 即红、复原即绿）。

### 片 3（玩法验收 + 失败恢复双房化）——已收口

- **落地**：Engine fail_room 去 #[cfg(test)] + `set_fail_room_for_test`
  （pub #[doc(hidden)]，持续语义；embed 包装一次性）；EmbedEngine
  `fail_next_settle_room_for_test`（take→set→settle→无条件 clear，顺延
  语义文档化）。三测试：①生命周期（孵化链：spawnCreep 扣费 200 →
  needTime 9 → 出生落位 directions [TOP] 首空格 (3,2)+spawning=false；
  建造链：夹具工地（API 为 stub）progress 2990+BUILD_POWER 5×2 →
  extension 转正；spawn 能量 111..=112=300−200+涓流 11~12）②房级失败
  恢复（W2N1 名序在后注入 → **先结算房 W1N1 良好结果随整体回退**（两
  阶段跨房提交的 embed 全链取证）→ 世界/时间保旧 → 重试零执行两房
  一并提交 → globalThis/Memory 双通道 [2,2] → **discard 臂**：失败轮
  候选丢弃后 Memory 恒 2、globalThis=3——memory 失败时点保旧的双房
  断言）③B0-6 双房变体（被拒重试：pending {p1,p2} 请求含注册但缺席
  的 p3 → bail 缓存幸存——较 b06 单房 ghost 未注册形态的实质增量；
  TickInput 复用：signController datetime 三段判窗与单房版同构）。
- **评审三路（失败恢复语义/引擎改动面/验收映射）**：P0×0、P1×2、
  P2×12。P1 清偿：①「两玩家 memory 全保旧」注释失实（探针不可区分
  提交时点）→ 补 discard 臂真断言+注释如实；②方案「无短 ticksToLive
  触发面」登记理由失实（place 支持 ticksToLive 播种）→ 勘正为「范围
  归后批」。P2 清偿：born1 补 spawning=false 断言（雏形计数弱断言消
  误导）；spawn 能量区间收紧 111..=112；旋钮顺延语义 doc；「③」消歧。
  登记不修：console 半边（capture_console_logs 失败轮结构性不更新——
  可选补强）；bucket 断言面（归 O3 SandboxConfig 旋钮合并）；datetime
  NTP 假红窗口（承接 b0-6 §4 登记口径）；名序第一房注入臂（独有杀伤
  力≈0，后房臂已双层覆盖）；结算 panic 窗口的 engine 侧清零不达（理论
  残余）；fail_room 常驻 release 的 24B+每房每 tick Option 比较（噪声
  级，有意权衡避免 feature 分叉生产二进制）。
- **验证**：workspace 全绿 / clippy 零告警 / release+棘轮 1405 零未豁免
  转红 / 三基线（tooangel console error tick=0、overmind/wasm 既知形态、
  skipped_penalty 全 0）/ perf 同口径 A/B 噪声级（0.124/0.210/0.032/
  0.467 vs 基线 0.124/0.209/0.032/0.467）/ 房级回退断言杀伤力抽查（期望
  改 p0+1 即红、复原即绿——真实值 p0=回退成立）。

### O1（World 对象目录——代际懒重建）——已收口

- **落地形态（较方案原描述的实现期订正）**：非逐可变面维护，改为
  **generation 代际懒重建**——①`ObjDir{id→房名}`（get/get_mut/remove
  房名定位；每代际首触达重建一次、之后 O(log N)；insert 增量续档；
  值用 map key+or_insert 首房胜——对齐 F-1 key 权威与原扫描同构）；
  ②`max_id_cache`（next_obj_id 的分体零分配键扫描缓存——结算每 tick
  的 next_available 路径与原扫描同价）。快照字节形态不变、reset 归零、
  Clone 深拷贝独立。
- **实测**：insert 序列 O(N²)→O(N log N)——评审 3 独立微基准（防 LLVM
  死存储消除后）500/1000/2000/4000 对象 3.0/4.7/10.6/22.9× 提速；
  storage-bench T3 夹具（~18282 insert）16.95s→0.31s（**dev 测试套件
  计时 ≈55×；外部复审独立复测 dev ≈60.7×/release 中位 ≈19.0×——提速
  倍数随构建口径不同，方向与量级一致**）；perf-probe 四相持平
  （0.124/0.209/0.032/0.465）。
- **实现期自抓回归（评审启动后、评审 3 F2 交叉证实）**：首版单一目录
  让 next_available 每 tick 全量重建（每对象 String clone）——settle 相
  +6% 两跑复现；max/目录分体后复原 0.032（评审 3 微基准：分体前形状
  @1000 对象 90×/tick 劣化于增长方向——分体即其建议修法，本批已落）。
- **评审三路（目录正确性/消费方兼容/性能实证+O3 侦察）**：P0×0、
  P1×1、P2×10。P1 清偿（评 2）：续档守卫零覆盖（变异实验：删守卫全套
  仍绿）→ 补 `obj_dir_guard_skips_extension_when_stale` 双形态杀伤线
  （删守卫复验即红、复原即绿）。P2 清偿：重建值改 map key（评 1/评 3
  同报——F-1 纪律对齐）+ or_insert 首房胜；get_mut 先读目录再 bump
  （O(1) 落实，bump 语义不变）；三处注释漂移（分体后措辞）+ O() 精度
  （O(log N)/重建真实成本/小规模常数反转注记——评 3 F3/F4）；generation
  字段注释补目录消费面；fixtures.rs「t3 +16s」旁证按 55× 实测更正
  （评 2 P2-4）。方案 §4 口径订正（评 3 F6）。登记不修：目录 String
  churn（每唯一 id 一次 clone 已缓和；Arc<str> 化归后续优化）；pub rooms 危害
  面扩大（文档化防御，收 pub(crate) 归后续批）；Clone 无生产调用面
  （3 行测试可钉，暂不加）。
- **验证**：workspace 全绿（kk-world 47+1 测）/ clippy 零告警 / release+
  棘轮 1405 零未豁免转红 / 三基线（tooangel console error tick=0、
  overmind/wasm 既知形态、skipped_penalty 全 0）/ perf 四相持平。

### O3（embed 层停跑回归锁）——已收口

- **落地**：驱动面=`set_sandbox_config_for_test`（按玩家 SandboxConfig
  覆盖，create_shard spawn 消费+分片建立后清空——评 1/评 2 P2-1 清偿
  补循环后 clear）；锁面=新测试 stop_run_regression.rs 两场景（①双玩家
  混合轮：p1 正常/p2 深负停跑同轮 commit 结局盲处置——−40 断言对「加
  结局过滤丢回充」可分辨（回归世界=−2000）；桶轨迹 −2000→−40→−20→0
  →第四轮恢复执行；②失败/重试/丢弃/新轮四臂：失败桶不动 → **重试臂**
  （评 3 P2-2 清偿——不丢弃直接重试缓存轮，首跑暂存候选经 commit 并入
  −40；Err 路径若被加误 discard 善后则此断言红）→ 丢弃臂桶不动 → 新轮
  −20）。
- **桶读取面增量（方案未提的实现期必要增量，评 3 P2-1 点名）**：
  Cmd::Bucket 通道（proto.rs，与 SettleBill 同模式）+
  ThreadExecutor::bucket + player_bucket_for_test（#[doc(hidden)] 全
  覆盖）约 25 行——结构必要性：EvalReport.bucket_after 为提交前口径、
  桶权威值驻沙箱线程，无此读取面则「−40 落桶」在 embed 层不可观测。
- **B0-6 销账判定**：b0-6 方案 §4 评 2 P2-1 登记的残余风险（未来给
  commit_players 遍历加结局过滤会静默丢回充）**本片清偿**——登记括号
  「测试旋钮或结局盲断言」二选一落地为双件（驱动面+锁面，评 3 判超
  清偿良性）；B0-6 处置表为时点记录不回改（先例：片 1 perf 夹具项同
  款处理），销账落点=本节+roadmap 批条目。片 3 §7「bucket 断言面归
  O3 合并」登记同步兑现。
- **评审三路（停跑语义/通道工程/验收映射）**：P0×0、P1×0、P2×5、
  P3×3。清偿：循环后 clear（两评审同报）；重试臂补测；bucket()
  doc(hidden) 一致性；模块 doc 补「不含 pending 候选」读取口径。
  登记不修：数值字面（与 budget 单测同口径——cpu_limit 变更 budget
  先红）；recv 无超时（继承 sandbox.md 全命令登记口径，Bucket 臂挂死
  面更窄）；防御清残留 no-op 措辞（随重试臂重构已消）。
- **验证**：workspace 全绿 / clippy 零告警 / release+棘轮 1405 零未豁免
  转红 / 三基线（tooangel console error tick=0、overmind/wasm 既知形态、
  skipped_penalty 全 0）/ perf 四相同口径持平（首轮 wall +7% 为机器
  负载离群，复跑两轮 0.123/0.202/0.031/0.456 噪声级——金丝雀判别）/
  结局盲断言杀伤力抽查（期望改 −2000 即红、真实值 −40=回充确经提交
  点并入）。

### 外部复审清偿（2026-09-30 第二轮，HEAD 35a5502）

外部 AI 对 HEAD 独立复审（自带探针/变异实验/分配器计数）：判定「主要
实现成立，全部声明不通过」——**P0×1、P1×2、P2×3**。逐条实测复核后
全部清偿：

- **P0（成立，当日修复）**：`skip_recharge` 公式与官方不等价——官方
  make.js:77 **先按原桶严格比较** `bucket < −2×limit ? −2×limit :
  bucket+limit`；B0-6 实现为「先加再钳」`(bucket+limit).max(−2×limit)`，
  在 **(−3×limit, −2×limit) 开窗**（limit=20 时 −59..−41；−60 边界点两式
  合流）分歧（原桶破
  地板者应钳回地板，而非加后悬在地板上）；B0-6/本批既有测试值
  （−2000/−580/−40）恰在两区合流带故未暴露。修复：按原桶比较分支+
  边界单测六值（−41/−50/−59/−60/−40/−2000）+窗口轨迹恢复轮 tickLimit=20
  +embed 级边界场景（−41 全链提交后 −40 非旧式 −21）+本方案/测试两处
  公式注释勘正。杀伤力：换回旧式边界单测即红。**B0-6 的「官方数值口径
  不变」自述在窗口内不成立——继承缺陷，本批清偿。**
- **P1-1（成立）**：顺序不变性测试的充裕 source 可交换负载无法捕获
  「按完成顺序处理意图」（评审变异实验：反序执行+反序处理仍全绿）——
  补**稀缺臂**（source=2 双引擎对照：反序执行下仍名序合并 (2,0)，完成
  序敏感负载下断言可分辨）+memory 改玩家区分标记（交叉错接可辨）。
- **P1-2（成立）**：生命周期验收实为单房，与方案「双房上下文」承诺
  不符——重构为**双房两联装**（p1 两房各 spawn+builder+工地，同轮跨房
  孵化/建造，逐房断言+两房新对象 id 全局唯一）。
- **P2×3（成立）**：①B 竞争守恒臂对整臂空转恒真——补正锚（两玩家
  满仓 50、总采集恰 100，脚本换空转即红）；②`last_execution_order`
  默认路径每轮 clone 玩家名——观测
  门控（仅旋钮启用才记录，记录面零分配——registered 自身 clone 为 HEAD
  既有；公式注释同步勘正在 budget.rs 主注/边界单测注与
  stop_run_regression.rs 模块注三处，§2 未动）；③O1 注释失实——目录
  重建分配单位勘正为「每唯一 id 一次房名分配」（分配探针：单房 1000
  对象 1167 次分配）、get 查找 O(1)→O(log N)（BTreeMap 三跳：
  id→房名→房态→对象）。
- **复审独立验证成立项**（正面结论采信）：workspace/棘轮 1405 零转红/
  三基线/perf 工作量三跑逐位复现（progress 3328、source 2836→2366）、
  夹具空转自检拒旧坐标、O1 五千混合操作对 oracle 等价、O3 结局盲断言
  变异转红+Bucket 读取不含 pending、串行链不变量、文档销账与 stub 勘正。
- **perf 口径注记**：复审轮独立测得真负载 vs 旧夹具 project +18%
  （本批曾记 +21%——绝对值未逐位复现，事件通道归因经「清事件投影」
  消融获支持）；T3 提速倍数附构建口径（见 O1 节订正）。
- **第一轮复审 note 处置（2026-09-30 补登）**：第一轮外部复审报告
  （docs/research/notes/dual-room-live-fire-external-review-2026-09-30.md，
  复审对象同为 35a5502）自带八条 P2 + P3 观察与变异实验，其 §0「无
  P0」判定被上述第二轮推翻（note 头部已加勘误注记，处置表见其 §4）。
  清偿落点：P2-1 夹具注释按实测改写；P2-2 即上条 P1-2 生命周期判别力
  重构的依据（占位压格+directions:[LEFT] 断 (2,3)）；P2-3 max 侧守卫
  恒真注记；P2-4 即上条观测门控；P2-6/P2-7 create_shard 的
  mem::take + 三旋钮清零；P2-8 rooms 硬纪律注释（收 pub 归 M2B）；
  P2-5/P3 留痕登记。
- **复审闭环判定（2026-09-30，用户拍板）**：免除外部复审第三轮。理由：
  本清偿唯一生产面行为改动为 skip_recharge 八行分支修复（三层锁+换回
  旧式即红+边界值覆盖分歧窗口，公式对 make.js:77 逐字转写）；其余为
  测试判别力增强（按指控重构，三路内部清偿复审全 PASS，残留 doc 级
  已修）与注释/文档；两轮外部复审收益递减（第一轮无 P0，第二轮 P0 为
  B0-6 潜伏缺陷被定向注意力暴露，非本批引入）。先例：B0-6 条件清偿
  0edbe5c 同判。残余风险由 M2B 扩面首片全仓门禁兜底。
