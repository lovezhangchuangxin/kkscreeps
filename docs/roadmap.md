# 路线图

> 里程碑与验收关卡。**只有范围和验收，没有时间估算**：每个关卡都尽量是机器可判定的（screeps-ok 通过率、bot 能跑、基准数字），做完即过关。
> 排序是风险驱动的：最大的不确定性（沙箱对真实代码的兼容性、引擎库接口）最先消灭；M2 建立端到端性能基线以验证关键架构路径，正式对照报告放在 M5。调研期的局部实测是设计锚点，不替代完整系统基准。
> 本文档取代早期写在 README 里的路线图；2026-09-27 的一次全面 review 修订了 M0/M2/M3 的定义（见文末修订记录）。

## 总表

| 里程碑 | 内容 | 验收关卡 |
|---|---|---|
| **M0** 兼容验收与工程基建 | 详见下节 | CI 可用；适配器出报告；两个真实 bot 桩环境下加载运行无崩溃、缺失 API 清单清零或登记 |
| **M1** 引擎骨架 ✅ | 详见下节 | screeps-ok 基线确立并在 CI 棘轮运行 |
| **M2** 单机可玩（两道关卡） | 详见下节 | 2A：通过率 ≥40%（D4 校准，2026-09-28）；B0：正确性清偿（见下节）；2B：≥95% 且缺口登记、真实 bot 全功能运行 |
| **M2.5** 开发套件 | `kkscreeps sim / test / watch / deploy` | 千 tick 集成测试秒级完成（基准实测） |
| **M3** 网络、客户端与多用户 | 详见下节 | 自建客户端登录游玩（核心端点冒烟清单全过）；官方客户端可连接 |
| **M4** 完整玩法与扩展 | 详见下节 | 全功能对照官方可玩；负载基准 |
| **M5** 生产化 | 详见下节 | 基准报告发布；72 小时 soak 无泄漏 |

## M0 兼容验收与工程基建 ✅（2026-09-27 完成）

> 交付物：monorepo 与 CI 基建；screeps-ok 上游基准独立复现零偏差（报告 [compat/reports/m0-screeps-ok-parity.md](../compat/reports/m0-screeps-ok-parity.md)）；
> 适配器跑通全量套件出基线 229/1659/1026，八轮评审修复后重验为 237/1651/1026（M1 棘轮起点，[基线报告](../compat/reports/m0-screeps-ok-baseline.md)）；
> tooangel + overmind 桩环境 2000 tick 零崩溃、缺失 API 清单清零（[bot 验收报告](../compat/reports/m0-bot-missing-api.md)）；
> 一实例一线程原型测试全绿。架构决策见 [ADR-010](decisions/ADR-010-screeps-ok-adapter-bridge.md)。

**内容**：
- monorepo 骨架（Rust workspace + pnpm workspace，按 [overall.md](architecture/overall.md) 与 [frontend.md](architecture/frontend.md) 的布局）；
- CI：格式化/静态检查/单元测试/文档链接检查（GitHub Actions）；
- screeps-ok 独立复现：pin 上游 commit、本地跑通 `npm run parity` 并对账 2897/2642 两个数字（需 Node 24 + C++ 工具链）——在依赖它之前先验证它的基准是真实的（质量评估与加固清单见 [ADR-006](decisions/ADR-006-screeps-ok-ci.md) 深度评估一节：结论是有真实价值但需加固的单人 alpha 项目，非 demo 级）；
- screeps-ok 适配器：**能执行套件并输出通过率报告**。注意 M0 不要求用例通过——用例测的是游戏行为，需要引擎存在，从 M1 起才会开始通过；M0 交付的是"能测"这个基建，以及适配器接口（它同时就是引擎作为库的嵌入式 API 定义，与 `kkscreeps sim` 共用，见 [ADR-008](decisions/ADR-008-runtime-model.md) 第 8 条；`runPlayer` 必须走真实沙箱，估 800~1500 行 TS）；
- 真实开源 bot 的**环境兼容性验收**：在 deno_core + CommonJS 加载器 + lodash 预置环境的**桩环境**（stub Game）下加载社区主流 bot（如 tooangel/overmind 类），跑上千个 tick 的 loop 循环，产出"桩环境缺失 API"清单并清零或登记；
- "一个玩家实例绑一个线程"的承载原型（`JsRuntime` 非 Send，此模型是 M2 沙箱正式接入的前提）。

**验收关卡**：CI 可用；适配器出报告；两个真实 bot 在桩环境下加载运行无崩溃、缺失 API 清单清零或登记。

## M1 引擎骨架 ✅（2026-09-28 完成）

**内容**：kk-core / kk-schema / 单房间结算（暂无沙箱，用固定指令脚本驱动，Sandbox trait 用假实现，见 [ADR-008](decisions/ADR-008-runtime-model.md) 第 5 条）；世界用预生成地形 + 手工摆房（官方地形生成算法的移植放 M4）；建立 screeps-ok 通过率基线与**棘轮机制**（通过率只增不减，回归即 CI 失败）。

**验收关卡**：基线确立并在 CI 棘轮运行。

> **收官（2026-09-28）**：八步全部交付（开发方案 [docs/plans/m1-engine-skeleton.md](plans/m1-engine-skeleton.md)，
> 每步经三方并行评审；架构结论升格 [ADR-011](decisions/ADR-011-engine-skeleton.md)）——kk-core/kk-schema/kk-world
> typed world/kk-engine 单房间结算（movement 仲裁/资源经济/孵化链）/**embed 通道切真实结算**（screeps-ok
> 适配器经 settle_room 推进世界，DumbWorld 退役）；**棘轮进 CI（第四 job）**，地板 277/2914
> （[compat/reports/ratchet.json](../compat/reports/ratchet.json) 唯一真相源；沿革 M0 237 → 步 5 真结算 265
> → 步 6 仪表批次 268 → 深评 follow-up 274 → 外部复审清偿 277）。
> bot 验收：tooangel 房间管线完整执行（M1 期实跑 300 tick）、overmind 推进到 RoomIntel 全量执行
> （2000 tick 零中断）、缺失 API 清零，bot-check 退出码机器可判定（0/1/2）。欠账处置终表见
> [M0 评审台账](../compat/reports/m0-code-review.md) 追记（17 项：15 修 + 2 重登记 M2-2A）。
>
> **M1 深度评审轮（2026-09-28）**：收官后 5 路 subagent 并行深评，修 4 high + 13 medium + 3 low
> （movement 工地障碍/出生占位/extension 容量/生成脚本落盘/safeMode 冻结可穿/跨房两阶段提交/
> directions 整单拒/energyAvailable 投影/controller 种子族/progressTotal 存值等）+ 6 项重登记，
> 详见台账追记与 [兼容灰区表](designs/compatibility.md)。

## M2 单机可玩（内部两道关卡）

这是**全项目最长的杆**：玩家 API 层要对照移植约 340 个方法（官方 game/ 层约 7000 行的对应物）。内部设两道关卡，在长杆中间留检查点。
正式接线遵循 [tick 调度与提交契约](architecture/tick-contract.md) 和
[快照与共享视图契约](architecture/snapshot-contract.md)；性能数据按
[端到端验收方案](architecture/performance-acceptance.md) 建立基线。三份契约均为 M2 目标，
其字节布局与负载数字须经过原型和行为测试后冻结。

- **2A 沙箱运行时 + 玩家 API 核心**（A1–A6 全部收口 2026-09-29，方案 [m2-playable.md](plans/m2-playable.md)，架构定型升格 [ADR-012](decisions/ADR-012-m2a-sandbox-runtime.md)；A5-core 三片 +48、A6 结构读取面 +47（isActive 官方四门真化/generic 结构 hits 缺省表/build 负 remaining 算术））：deno_core 沙箱正式接入（一实例一线程、预算桶、内存双闸门、快照、遮蔽内部函数——全部按 [sandbox.md](architecture/sandbox.md)）；玩家 API 核心全部落地。
  - 关卡：screeps-ok 通过率 ≥40%（D4 现实校准，"过半"落在 2B API 补全批出口；2026-09-29 地板 **1405/2914=48.25%，2A 出口关卡达成**（A5+A6 合计 +95；A6 另交付 tooangel 300t→2000t 基线零错升级、性能观测点 perf-probe + [首次实测记录](../compat/reports/perf-a6.md)、ADR-012），数字以 [ratchet.json](../compat/reports/ratchet.json) 为唯一真相源。
- **B0 正确性清偿批**（2026-09-29 架构复审裁决，[ADR-013](decisions/ADR-013-architecture-review-coordinator.md)、方案 [b0-runtime-coordinator.md](plans/b0-runtime-coordinator.md)；A6 关段后立即执行）：运行协调层与部分提交修复（RunResult 统一类型、世界与玩家持久状态同一提交点，embed 编排移出 kk-engine）、LIFO 每线程纠偏（空闲回收支持任意实例）、墙钟入 TickInput（重放确定性）、`__kk` 宿主面收进 Rust 私有句柄。
  - 关卡：复审复现场景全部转为回归测试且全绿（部分提交/LIFO 乱序销毁/重放一致性）；棘轮零转红（或转红逐字豁免并归因）。
- **B1 结构治理批**（2026-09-29 用户拍板新增，B0 与 M2B 前置批之间；方案与切片执行记录 [b1-structure-governance.md](plans/b1-structure-governance.md)，五片全部收口）：>1000 行文件纯搬移零行为拆分——settle/creep.rs（2119）按 intent 家族拆 creep/ 八模块；place.rs（1092）建造面单切 place/place_structure.rs；prelude creep.js/structures.js 各拆 2 有序段（PRELUDE_SEGMENTS 13→15，段间共享 gw 桥、加载序与加载期捕获纪律不变）；settle_tests.rs（6906/142 测试）与 sandbox_tests.rs/embed_tests.rs（串行链文件）mod 子模块化（顶层唯一 #[test] 不变量保持）。每片过完整批次循环（workspace 全绿/release+棘轮 1405 零转红/三基线/≥3 路 subagent 独立对账评审/清偿复审/pnpm check），函数体（JS 到调用点）逐字节一致由三路独立对账证明。kk-schema/room.rs（1116）已裁决不拆（2026-09-30 用户决策：声明面字典，长≠复杂）；world_json/snapshot 归 M2B 前置批宏化。
- **M2B 前置批**（B0 之后、2B 扩面之前，[ADR-013](decisions/ADR-013-architecture-review-coordinator.md) 决策 2；方案与切片执行记录 [m2b-prerequisite.md](plans/m2b-prerequisite.md)，五片全部收口 2026-09-30，c253d0c→74340cd）：①seam 地基——`WorldSource` trait（kk-runtime，embed 五读取面改经 seam，玩家循环代际绊线）+ World 版本面（generation/terrain_epoch 全可变面 bump、`RoomVersionHandle`——「同一 tick 内所有 getter 绑定同一版本」纪律的 typed 层起步，snapshot 契约加现状标注）+ Terrain 内表示 Arc 化；②地形按版本缓存——注入协议改紧凑载荷 `{rev, payload(b64)}`（Rust 纪元缓存零重编码 + prelude 跨 tick 零重解码，双形态接收），**perf 实测 project −42%/run −40%/wall −30%**（对 ef43f4a 同机器态基线消融，三相咬合；Level 2 稳态省略载荷判定不做并登记）；③投影/编解码宏化——`kk_room_object!` 一次变体声明生成 RoomObject 枚举全管道（含编译期 tag 互异断言；机械同步点 9 块收敛为声明一次，tags 表保持手写冻结面）+ 投影 helper 单源化（store 家族/progress_total/owner 三态 + 结算侧 controller_levels 收编）+ world_json/snapshot 测试拆出（B1 留置两文件 <1000 达成）；④按代际对象投影共享缓存**实测回退**（project +106%——Value 树克隆的读侧局部性成本≫从连续 typed 载荷直构，Value 级共享结构性不回本，兑现共享须换表示（列式/载荷旁路等），「列式由测量决定」的直接数据点）。每入库片完整批次循环（P5 实测回退无代码入库，判据闭环见方案 §7）（workspace/release+棘轮 1405 零转红/三基线/≥3 路 subagent 评审+清偿/pnpm check/perf 同口径 A/B 消融归因；P1 期发现并登记 perf-a6 基线整机漂移，本批工作基线改当日 HEAD 实测）。双房双玩家实弹验证不在本批（另出方案：跨房 tick 合并规则）。
- **B0-6 外部评审清偿批**（2026-09-30，外部 GPT 架构锐评逐条实测复核触发——三组复现指控全部独立复现成立；方案 [b0-6-external-review-clearance.md](plans/b0-6-external-review-clearance.md)）：①重试守卫前移——被拒的重试请求不再销毁 pending 缓存（原实现销毁后原请求重试会**重跑玩家代码**、同一 tick 双执行，违 tick 契约与验收场景 3）；②结算输入随失败轮缓存——重试复用记录的 TickInput（ADR-013 决策 5「重放用记录值」在协调器侧落实，signController datetime 不随重试漂移）；③负桶停跑回充候选化——统一提交点并入/失败轮丢弃（原实现即时落桶绕过提交点；官方停跑数值口径不变——**勘误（双房批外部复审 P0）**：「先加再钳」式在 (−3×limit,−2×limit) 开窗与官方 make.js:77 分歧，已改按原桶比较分支，见双房批方案 §7）。外部建议处置：typed FIND 不采纳（m2b-prerequisite §2 显式不做）；proto.rs 命令回执无超时（M3 多用户承载前清偿）、InfraFailure 持久故障态（tick 契约仍为目标）、ObjId→Room 索引、领域语义模块归属（后两者 M2B 扩面前候选小片）登记待批；perf-probe 夹具空转（动作全超程被 API 预检拒绝、无 intent 产生——夹具自述目的不成立）搭车双房实弹验证批修。
- **双房双玩家实弹验证批**（2026-09-30 定义为独立批、同日收口，方案与切片执行记录 [dual-room-live-fire.md](plans/dual-room-live-fire.md)；M2B 扩面前最后一道实弹关卡）：①跨房合并三形态实弹（tick 契约验收场景 2——一玩家两房/两玩家一房竞争含稀缺名序杀伤线/无关房先完成双向；执行序测试旋钮+取证面钉「打乱玩家完成顺序结果不变」；双世界 50 tick 能量守恒精确恒等式——实现期取证满仓 harvest 超容掉落+小堆衰减为官方语义非缺陷）②真实玩法验收+失败恢复双房化（验收场景 1/3 多房形态：孵化/建造生命周期；房级失败注入旋钮——先结算房良好结果随整体回退的 embed 全链取证；B0-6 三件事双房变体含 discard 臂 memory 时点保旧）③perf-probe 夹具空转修复（B0-6 登记项清偿：对象摆入作用距离+directions 出生落位+负载自检双臂口径——空转形态非零退出防复发；同日 A/B 重取工作基线 2p 0.124/0.209/0.032/0.467，旧空转基线作废并注记）④O1 World 对象目录（generation 代际懒重建+insert 增量续档——insert 序列 O(N²)→O(N log N)，storage-bench T3 实测 55×；实现期自抓 settle +6% 回归经 max/目录分体复原）⑤O3 embed 层停跑回归锁（B0-6 评 2 P2-1 残余风险清偿：SandboxConfig 测试旋钮+Cmd::Bucket 桶读取面+结局盲提交断言——双玩家混合轮/失败/重试/丢弃/新轮五臂）。每片完整批次循环（workspace/release+棘轮 1405 零转红/三基线/perf 同口径/≥3 路 subagent 评审+逐条清偿+杀伤力抽查）。ratchet-allow 现存 1 条豁免保留（跨房移动过渡归移动族后续批，本批场景不依赖跨房移动）。
- **2B 完整玩家 API + 存储 + 完整世界**：API 补全；存储落盘（redb vs libmdbx 基准后定，[ADR-008](decisions/ADR-008-runtime-model.md) 第 7 条）；命令行启动一个完整世界，含机器人对手。
  - 关卡：screeps-ok ≥95% 且每个未过用例有登记的差异说明（参照：xxscreeps 当前为 91%）；真实 bot 全功能运行。

## M2.5 开发套件

`kkscreeps sim / test / watch / deploy`，设计见 [dev-kit.md](designs/dev-kit.md)，兼作自研回归工具（与 screeps-ok 互补：screeps-ok 管官方语义一致性，开发套件管真实场景行为）。
**关卡**：上千个 tick 的集成测试秒级完成（基准实测，不是推断）。

## M3 网络、客户端与多用户

**内容**：
- HTTP + WebSocket 接口，兼容官方协议。**核心端点先行**：认证、代码上传、房间订阅、控制台；市场类端点随 M4 的玩法实现填充（避免空壳端点）；
- **账号系统与多用户承载**（MMO 的本质属性：注册/登录/多玩家同世界）；
- 自建 Web 客户端：Vue3 应用 + 自研渲染器 + @kkscreeps 包群 + 常量/DTO 生成管线（见 [frontend.md](architecture/frontend.md)）；渲染器先达"房间观战可用"，世界地图/回放/市场视图可滑至 M4；
- 客户端构建产物嵌入服务器二进制（rust-embed）。

**验收关卡**：自建客户端登录游玩（核心端点冒烟清单全过）；官方客户端可连接（协议兼容关卡，见[兼容性红线](designs/compatibility.md)）。

## M4 完整玩法与扩展

市场、power、跨世界（shard）与跨世界通信、导入官方世界数据、官方地形生成算法移植、mod 系统（[ADR-007](decisions/ADR-007-mod-system.md)）、回放与历史系统 UI（[设计待补](designs/README.md)）、多机部署后端（Redis 等，见 [storage.md](architecture/storage.md)）。
**关卡**：全功能对照官方可玩；负载基准（含与官方/xxscreeps 的对照数据采集）。

## M5 生产化

- 性能基准正式报告（与官方/xxscreeps 对照；按[统一场景与指标](architecture/performance-acceptance.md)验证资源目标）；
- 长跑稳定性：soak 测试，目标 **72 小时**内存平稳无泄漏（对照官方"每小时重启缓解泄漏"的现状）；
- 安全加固——**公网部署的前置条件**（自用阶段可单进程）：进程按组拆分、seccomp、WebAssembly 缓冲区分配上限、内部函数遮蔽复查（详见 [sandbox.md](architecture/sandbox.md) 安全边界）。

**关卡**：基准报告发布；72 小时 soak 无泄漏。

## 贯穿性纪律

- 每个里程碑的合入都要过 CI 全套：格式化、静态检查、单元测试、screeps-ok 棘轮、生成物新鲜度校验（常量/DTO，M3 起）；
- 任何与官方行为的偏差必须登记到[兼容性红线](designs/compatibility.md)灰区表，不允许静默发生；
- 文档随代码走：每个里程碑结束时相关设计文档更新为实现现状。

## 修订记录

- **2026-09-30（M2B 扩面批开批 + P0 收口）**：方案 [m2b-expansion.md](plans/m2b-expansion.md) 经用户确认开批（目标口径勘正为 ~150→~230：官方全量 340 中 PowerCreep/market/power 等属 M4/2B 能力门，余量逐项归因见方案 §6；market 整体推 2B 由 notify 小面先行验证 global intent 通道）。P0 结构片落地：①O2 领域语义模块归属（kk-world `domain/`——controller_progress_total_opt/store 双核/owner 三态五函数从两投影模块纯搬移，token 级对账全等，双房批缓行项销账）；②`pub rooms`/`pub time` 收敛（pub(crate) + rooms()/time()/commit_room 不 bump 代际——「一次成功 tick ⇒ 恰 +1」不变量由 advance_ticks 单点承载/`#[doc(hidden)] set_time_for_test`；跨 crate 写面编译器穷尽收口，双房批 P2-8 与 O1 登记项销账）；③方案入库（评审清偿含 commit_room bump 口径勘正）。验证：workspace 283 绿/release+棘轮 1405 零未豁免转红/三基线/perf 四相持平（0.122/0.205/0.032/0.456）；三路评审全 PASS 零代码缺陷。
- **2026-09-30（双房批外部复审清偿）**：外部 AI 对 HEAD 35a5502 两轮独立复审（第一轮八条 P2+P3 观察处置表见 [note](research/notes/dual-room-live-fire-external-review-2026-09-30.md) §4；第二轮 P0×1/P1×2/P2×3——第一轮「无 P0」判定被第二轮推翻，note 头部已加勘误注记）逐条实测清偿（0b83f1d）：**P0** 停跑回充公式「先加再钳」与官方「先按原桶严格比较」在 (−3×limit, −2×limit) 开窗（limit=20：−59..−41）分歧——B0-6 潜伏缺陷（既有测试值恰在合流带未暴露），改按原桶比较分支+边界六值/窗口轨迹/embed 级边界场景三层锁，B0-6 方案同步补勘误指针；**P1** 顺序不变性补稀缺臂（source=2 名序杀伤线）+生命周期重构双房两联装（占位压格+directions 出生判别）；**P2** 守恒正锚/观测门控/O1 注释勘正。外部第三轮复审经用户拍板免除（08302c1，先例 B0-6 条件清偿）。领域语义模块归属仍登记（随 M2B 扩面首片）。
- **2026-09-30（双房批收口）**：三形态+顺序不变性+守恒、玩法验收与房级失败恢复双房化、perf 夹具修复（搭车项）、O1 对象目录、O3 停跑回归锁五片全部落地（ec771e2→36d25b7 六提交）；tick 契约验收场景 1/2/3 的多房形态全部有 embed 全链回归锁定（串行参考=现协调器既定序，作将来并行化批对齐基线）；perf 工作基线换新（真负载口径）。B0-6 登记的三项中：perf 夹具与 embed 层停跑锁已清偿销账（处置表为时点记录不回改），ObjId 索引清偿（O1），领域语义模块归属仍登记（随 M2B 扩面首片）；跨房 room 级 intent 玩家 API 面为 stub 的现状经实测取证（m2-playable 4.3 超前宣称已勘正，真实现归 M2B API 扩面）。
- **2026-09-30（B0-6 收口）**：外部 GPT 架构锐评逐条实测复核（7 组批评：三组复现指控——重试缓存销毁/停跑回血绕提交点/perf 夹具空转——全部独立复现成立；其余多为已登记过渡态，处置表见方案 §3）→ 正确性清偿批落地：重试守卫前移（被拒请求不销毁缓存——双执行 bug）、TickInput 随失败轮缓存（重试复用记录值）、停跑回充候选化（统一提交点并入/失败轮丢弃）；三条探针序列复原为回归测试。登记待批：proto.rs 命令回执无超时（M3 前）、InfraFailure 持久故障态（tick 契约仍为目标）、ObjId→Room 索引与领域语义模块归属（M2B 扩面前候选小片）、perf 夹具修复（搭车双房实弹验证批）；typed FIND 不采纳（显式决策维持）。
- **2026-09-30（M2B 前置批收口）**：seam 地基（WorldSource/世界版本面/Arc 化）+ 投影/编解码宏化 + 地形按版本缓存全部落地（方案 [m2b-prerequisite.md](plans/m2b-prerequisite.md)）；地形紧凑载荷实测 project −42%/run −40%/wall −30%；对象投影共享缓存实测负结果回退登记（列式测量数据点）；新增对象类型同步点机械层收敛为声明一次（`kk_room_object!`）+ 真重复层单源（§8 全表见方案）；perf-a6 基线整机漂移登记（绝对值口径改当日 A/B 消融）。**批构成修订**：双房双玩家实弹验证（2026-09-29 定义的三件之一）脱钩为独立方案批（M2B 扩面前另出——跨房 tick 合并规则），本批只落 seam/宏化/缓存。收口终审订正：P4 测试文件清偿补入后曾超 1000 行（原记行数未回更），已再拆两子模块。
- **2026-09-29（架构复审）**：① 全项目架构复审（内部六路深查 + 外部独立结论交叉对比，全部可验证主张逐条复核，[笔记](research/notes/architecture-review-2026-09.md)），总判定"核心决策链正确、无系统性偏离"，四项裁决与排期记入 [ADR-013](decisions/ADR-013-architecture-review-coordinator.md)；② M2 插入 **B0 正确性清偿批**（A6 关段后、M2B 前置批前）；③ 新增 **M2B 前置批**定义（seam 三件套/样板宏化/双房双玩家实弹验证）；④ 2A 关卡口径统一为 ≥40%，消除总表"过半"与 m2-playable D4 校准的不一致（无修订记录的软着陆教训）；⑤ sandbox.md LIFO 表述纠偏（每线程约束，非全局）、CPU 计时现状标注（墙钟，M3 前切线程时钟）、ADR-004 intent 二进制延期追记。
- **2026-09-29（B1 收口）**：B1 结构治理批五片全部落地（8dc0da3→3049efc，切片执行与评审清偿记录 [b1-structure-governance.md](plans/b1-structure-governance.md) §7）——纯搬移零行为：Rust 侧函数体逐字节（可见性仅 pub(super)/pub(in crate::settle) 等值调整+re-export 保路径）；JS 段拆调用点逐字节（gw 桥 const 绑定同一对象）；142 测试计数/原文序与两 V8 串行链不变量保持；实测验证=workspace 268 全绿×5、棘轮 1405 零转红×5、bot 三基线×5、fmt/clippy/pnpm check 零告警。新纪律沉淀：集成测试根（crate root）mod 解析平级故 #[path]；cargo fmt reorder_modules 会归一 mod 声明序；测试切块用 gap-attachment 保段落横幅注释。
- **2026-09-29（B0 收口）**：B0 正确性清偿批五片全部落地（切片执行与清偿记录 [b0-slices.md](plans/b0-slices.md)）——①LIFO 纠偏为每线程（evict_idle 快照式全表回收+乱序销毁压力测试）②墙钟入 TickInput（signController datetime 注入值+Fixture/引擎级双房重放一致性回归）③`__kk` 宿主面收口（Rust 私有 v8::Global 句柄、set_raw_memory 私有通道、.call 家族 21 行全迁 Reflect.apply、信封 error 16K 闸、near-heap 全路径恢复、PerfRecorder cap；strict 帧与 near-heap 恢复两把回归锁）④新 crate **kk-runtime**（PlayerExecutor/ThreadExecutor、Sandbox trait 统一执行接口、RunResult 四形态结局分类）⑤**统一提交点**（CPU 计费与 RawMemory 候选化、每 tick 从 PlayerState 注入、失败缓存重试不重跑玩家、注入失败玩家级恢复；部分提交三连回归全绿=ADR-013 复现场景①②③全部转回归）；embed 编排迁 kk-runtime，**kk-engine 编译期摆脱 kk-runner/deno_core**（验收关卡 3）；perf-probe 同口径 run 相 p50 -52.8%（句柄通道替代脚本拼接）、四相无显著劣化；灰区表 B0 相关注记七条销账十余条新登记。
- **2026-09-27（M0 完成）**：全部验收关卡达成，报告归档于 compat/reports/；适配器桥接与哑世界等四项决策记入 ADR-010。
- **2026-09-27（全面 review）**：① M0 重定义——原验收"机器人原样运行 / screeps-ok 首批用例通过"存在依赖倒置（都需要引擎存在，M0 时还没有），改为"环境兼容性 + 适配器能测并出报告"；② M2 内部拆出 2A/2B 两道关卡（最长杆需要中间检查点）；③ M2 通过率从模糊的"达标"定量为 ≥95% + 缺口登记（锚点：xxscreeps 91%）；④ CI 基建从隐含变为 M0 显式条目；⑤ M3 补上"账号与多用户"，端点明确"核心先行、市场随 M4"；⑥ M5 的 soak 定量（72 小时）、安全加固标注为公网部署前置条件；⑦ 路线图从 README 拆出为独立文档；⑧ screeps-ok 深度评估后（ADR-006），M0 增加『独立复现 parity 数字』前置项。
