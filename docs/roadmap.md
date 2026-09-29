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

- **2A 沙箱运行时 + 玩家 API 核心**（进行中，方案 [m2-playable.md](plans/m2-playable.md)，三轨并行——A 沙箱/API、B 引擎结算、C pathfinder/存储；A1 查询面契约快照 277→374、A2+B1/B1.1 intent 通道与动作方法族 374→592、A2.1 外部复审修复（安全网收口）592→648、A3a prelude 30_game/ 零行为拆分（棘轮零翻转）与自审清偿（toJSON null-proto 结构位/工地不授视野/spawn 入 structures/预置 parse 全链）648→649，全程零转红）：deno_core 沙箱正式接入（一实例一线程、预算桶、内存双闸门、快照、遮蔽内部函数——全部按 [sandbox.md](architecture/sandbox.md)）；玩家 API 先做核心（Game/Room/Creep/基础指令/Memory）。
  - 关卡：screeps-ok 通过率 ≥40%（D4 现实校准，"过半"落在 2B API 补全批出口；2026-09-29 地板已达 1356/2914=46.53%（A5-core 片 1 Memory/模块形态批 +13、片 2 RoomPosition packedPos 形态族 +5、片 3a Room look 家族真化 +28），数字以 [ratchet.json](../compat/reports/ratchet.json) 为唯一真相源；待 A5 剩余片+A6 收口）。
- **B0 正确性清偿批**（2026-09-29 架构复审裁决，[ADR-013](decisions/ADR-013-architecture-review-coordinator.md)、方案 [b0-runtime-coordinator.md](plans/b0-runtime-coordinator.md)；A6 关段后立即执行）：运行协调层与部分提交修复（RunResult 统一类型、世界与玩家持久状态同一提交点，embed 编排移出 kk-engine）、LIFO 每线程纠偏（空闲回收支持任意实例）、墙钟入 TickInput（重放确定性）、`__kk` 宿主面收进 Rust 私有句柄。
  - 关卡：复审复现场景全部转为回归测试且全绿（部分提交/LIFO 乱序销毁/重放一致性）；棘轮零转红（或转红逐字豁免并归因）。
- **M2B 前置批**（B0 之后、2B 扩面之前，[ADR-013](decisions/ADR-013-architecture-review-coordinator.md) 决策 2）：seam 三件套（世界注入来源接口化 + 地形按世界版本缓存 + 不可变房间版本句柄）、投影/编解码样板宏化（新增对象类型 7 处同步点收敛）、双房双玩家最小实弹验证 tick 契约合并规则（临时 ID/依赖集合/乱序完成不变性）。
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

- **2026-09-29（架构复审）**：① 全项目架构复审（内部六路深查 + 外部独立结论交叉对比，全部可验证主张逐条复核，[笔记](research/notes/architecture-review-2026-09.md)），总判定"核心决策链正确、无系统性偏离"，四项裁决与排期记入 [ADR-013](decisions/ADR-013-architecture-review-coordinator.md)；② M2 插入 **B0 正确性清偿批**（A6 关段后、M2B 前置批前）；③ 新增 **M2B 前置批**定义（seam 三件套/样板宏化/双房双玩家实弹验证）；④ 2A 关卡口径统一为 ≥40%，消除总表"过半"与 m2-playable D4 校准的不一致（无修订记录的软着陆教训）；⑤ sandbox.md LIFO 表述纠偏（每线程约束，非全局）、CPU 计时现状标注（墙钟，M3 前切线程时钟）、ADR-004 intent 二进制延期追记。
- **2026-09-27（M0 完成）**：全部验收关卡达成，报告归档于 compat/reports/；适配器桥接与哑世界等四项决策记入 ADR-010。
- **2026-09-27（全面 review）**：① M0 重定义——原验收"机器人原样运行 / screeps-ok 首批用例通过"存在依赖倒置（都需要引擎存在，M0 时还没有），改为"环境兼容性 + 适配器能测并出报告"；② M2 内部拆出 2A/2B 两道关卡（最长杆需要中间检查点）；③ M2 通过率从模糊的"达标"定量为 ≥95% + 缺口登记（锚点：xxscreeps 91%）；④ CI 基建从隐含变为 M0 显式条目；⑤ M3 补上"账号与多用户"，端点明确"核心先行、市场随 M4"；⑥ M5 的 soak 定量（72 小时）、安全加固标注为公网部署前置条件；⑦ 路线图从 README 拆出为独立文档；⑧ screeps-ok 深度评估后（ADR-006），M0 增加『独立复现 parity 数字』前置项。
