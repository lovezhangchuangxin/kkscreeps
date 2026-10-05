# 路线图

本页定义里程碑的范围与验收条件，不安排日期。先验证玩家运行时和结算机制，再完成单机玩法、开发工具、联网服务与生产部署。局部实验用于选择方案，完整系统的性能结论由端到端基准验证。

当前各组件的能力见[项目说明](../README.md)；具体机制与尚未实现的设计见[架构文档](architecture/README.md)。路线图中的目标不代表已经交付。

## 里程碑

| 里程碑 | 范围 | 验收条件与进展 |
|---|---|---|
| M0 兼容验收与工程基建 | CI、适配器和 bot 环境测试 | 已完成：可执行完整套件并出报告，两个真实 bot 在桩环境下运行，缺失 API 清零或登记 |
| M1 引擎骨架 | 类型化世界、单房间结算、嵌入接口 | 已完成：建立 screeps-ok 基线，CI 检查通过数下限与逐用例回归 |
| M2 单机可玩 | 沙箱、玩家 API、存储与完整世界 | 2A 已达到 ≥40% 门槛；2B 仍须达到 ≥95%、登记剩余差异，并验证真实 bot 全功能运行 |
| M2.5 开发套件 | sim、test、watch、deploy | 完整场景测试可复跑；固定基准中上千 tick 秒级完成 |
| M3 网络、客户端与多用户 | 服务协议、账号、Web 客户端 | 自建客户端登录游玩，核心端点通过冒烟测试，官方客户端可连接 |
| M4 完整玩法与扩展 | 剩余玩法、世界导入、回放与部署扩展 | 全功能对照官方可玩，完成负载基准 |
| M5 生产化 | 性能报告、长跑稳定性和安全隔离 | 发布正式对照报告，完成 72 小时长跑与公网部署安全验收 |

## M0 兼容验收与工程基建

建立 Rust 与 pnpm 工作区、CI 和兼容性工具，先独立复现锁定版本的上游测试结果，再接入本项目的适配器。适配器应真正运行玩家沙箱并输出可核对的报告；这个阶段要求“能够测试”，不要求尚未实现的游戏行为通过。

真实 bot 的环境验收在固定的桩 Game 下检查模块加载、循环运行和缺失 API。它验证运行环境，不能替代后续游戏进展验收。一实例一线程的原型验证用于确认 V8 承载方式。

完成依据：[上游复现报告](../compat/reports/m0-screeps-ok-parity.md)、[适配器基线](../compat/reports/m0-screeps-ok-baseline.md)、[bot 环境报告](../compat/reports/m0-bot-missing-api.md)与 [ADR-010](decisions/ADR-010-screeps-ok-adapter-bridge.md)。报告中的数字属于该阶段。

## M1 引擎骨架

建立 kk-core、kk-schema、kk-world 的类型化数据，以及 kk-engine 的单房间结算。早期以固定指令驱动世界，随后嵌入接口接入真实结算，建立 screeps-ok 基线和回归检查。

当前通过数下限以 [ratchet.json](../compat/reports/ratchet.json) 为准，不在路线图重复维护数字。M1 的实现与验收记录见[引擎骨架方案](plans/m1-engine-skeleton.md)和 [ADR-011](decisions/ADR-011-engine-skeleton.md)。

## M2 单机可玩

M2 分为两道关卡，避免把运行环境可用与完整玩法可玩混为一谈。

- **2A：沙箱与核心玩家 API。** 接入玩家运行时、CPU 预算、Memory、世界投影和核心动作；screeps-ok 通过率达到 40%。该关卡已有验收记录，见[单机可玩方案](plans/m2-playable.md)和 [ADR-012](decisions/ADR-012-m2a-sandbox-runtime.md)。CPU、内存及宿主隔离的具体边界以[沙箱说明](architecture/sandbox.md)为准。
- **2B：完整玩家 API、存储与完整世界。** 补齐玩法与玩家接口，在真实负载下完成存储选型和落盘，提供可启动完整世界及机器人对手的命令。验收要求为 screeps-ok 通过率至少 95%，每个未通过用例都有差异说明，真实 bot 能在推进结算的世界中全功能运行。

运行协调、失败重试、双房双玩家验证和结构整理已有阶段性成果，详见[方案索引](plans/README.md)。这些成果不能替代 2B 的整体验收；固定世界 bot-check 也不能证明孵化、建造和经济链已经正常推进。

玩家 API 扩面见 [M2B 方案](plans/m2b-expansion.md)。terminal 与市场已开展独立实现和验收，见[终端与市场方案](plans/2b-terminal-market.md)，后续应按剩余差异补齐，不能继续把整个市场视为 M4 才开始的能力。

核心仓库搬运与分片 GCL 输入已接入；两种玩家投影分别通过连续 40 tick 的采集、容器搬运和升级守恒场景，记录见[核心经济链与 GCL](research/notes/economy-gcl-2026-10-04.md)。现已提供 [sim run](architecture/local-simulation.md) 和可复跑的采集、搬运、升级、寿命到期后孵化替补场景；完整外部 bot 的玩法验收、特殊存储规则开放及保存恢复仍须继续推进。

并发调度、统一提交和共享视图遵循 [tick 契约](architecture/tick-contract.md)与[快照契约](architecture/snapshot-contract.md)。这些文档分别说明现状和目标；提出契约不等于生产通路已经完整接入。端到端基线按[性能验收方案](architecture/performance-acceptance.md)采集，须先确认夹具实际产生动作与状态变化。

## JS Mod 与架构扩展

官方内容将按可选包和玩法预设交付，其中一部分直接使用公共 JS SDK，见 [ADR-016](decisions/ADR-016-official-content-packs.md)和[内容包设计](designs/content-packs.md)。首个生命周期/生产示例作为官方内容候选，用真实玩法验证公共接口、依赖组合与恢复；已有原生玩法逐族迁移，尚未提供完整内容选择。完整默认预设保持兼容性，精简/自定义组合发布实际能力；故障停用不等同于卸载内容。

独立 JS 包创造完整玩法、覆盖调研版本 xxscreeps 扩展领域，并提供更完善的组合、恢复和开发工具，是已确认的目标，见 [ADR-007](decisions/ADR-007-mod-system.md)。2026-10-05 的[现状复查](research/09-mod-capability-current-audit.md)基于 `1b40930` 核对了 13 个内建槽及其边界：动作改向/取消尚不等于伤害公式、建造配方或移动仲裁可替换。后续按[完整玩法计划](plans/mod-complete-gameplay.md)前移自有对象生命周期、候选对象操作、开放资源/能力和作者工具；下一片接自有对象 tick 与 Mod 场景装载，增删对象另片收口。剩余默认动作槽按完整玩法需求继续开放。

D5 已收敛玩家执行边界：引擎只接受显式指令和写回候选，玩家执行结果类型归到运行时，旧无参数 `Engine::tick` 与沙箱注册表已删除。运行时按注册身份过滤所有意图族，失败重试复用整轮候选，见 [tick 契约](architecture/tick-contract.md#玩家执行与结算接口)。

D4 资源领域后续片已开放 [ctx.resources](architecture/mod-resources.md)：同房候选库存读取、精确转账与损耗、额外对象局部拒绝日志和晚期写回失败缓存重试。

D4 移动片已开放 [creep.move 槽](architecture/creep-move-slot.md)：意图登记阶段的 before/wrap/replace/after、受控移动请求能力（改向/取消登记）与按执行者粒度的正常拒绝，实际位移仍走既有仲裁与执行阶段。

D4 战斗片已开放 [五个战斗槽](architecture/creep-combat-slot.md)（attack/rangedAttack/rangedMassAttack/heal/rangedHeal）：`ctx.combat` 目标改向/取消（恰一次消耗约束）、延迟伤害/治疗队列条目、rampart 重定向受击对象与结构摧毁废墟/对象移除（链内增删日志+ID 游标回退）的局部拒绝恢复；attackController 与容量等其他动作槽仍须逐族开放。

D4 建造族片已开放 [三个建造族槽](architecture/creep-build-slot.md)（build/repair/dismantle）：`ctx.build` 目标改向/取消（恰一次消耗约束；request 后刷新处理器视图）、完工变换（工地移除+结构插入）与 dismantle 击毁废墟/超容掉落面（链内增删日志+ID 游标回退+执行者格掉落面快照）的局部拒绝恢复；attackController 与容量族仍须逐族开放。

D4 首片已开放 Generic 仓库的 10 条容量数值，贯穿放置、建造、玩家常量、实验室槽切换和终端 tick，见[仓库容量规则](architecture/store-rules.md)。特殊存储策略及更多动作槽仍须逐族开放。检查点的数据域和版本清单前置，先保存当前完整已提交状态，随新增领域扩展迁移与日志；不等待全部规则开放后才开始恢复通路。

D6 首片已允许 link transfer 的 JS 处理链正常拒绝一次动作；后续 D4/D6 片已开放 [creep transfer/withdraw](architecture/creep-resource-slots.md) 与 [creep move](architecture/creep-move-slot.md) 的 before/wrap/replace/after 与正常拒绝。局部恢复执行者/目标库存、link 冷却、实验室槽容量、新增事件、移动请求登记及 Mod state，保留此前成功动作并继续结算。link tick、attackController、容量族和其他动作尚未开放正常拒绝，见[Mod 执行](architecture/mod-execution.md#正常动作拒绝)。

[ADR-014](decisions/ADR-014-contribution-plane.md)已把扩展基础安排在玩法扩面与首次持久化格式冻结之前，不能继续只列作 M4 的末端功能。Mod 作者使用 JS SDK；对象、指令、规则、候选状态和提交协议须共同设计，服务与客户端贡献随 M3 补齐。

实施范围见[贡献平面方案](plans/mod-plane-batches.md)，机制见 [JS Mod 设计](designs/mod-system.md)。默认玩法仍须满足同一玩家契约；首版采用共同可信的玩法 VM。投影协议整体设计、分步接入，玩家任务并发另行验证；三方对照基准与扩展基础建设并行推进。新增封闭分派或硬编码白名单须同时提供具名扩展入口，或登记明确的例外依据。

## M2.5 开发套件

实现 `kkscreeps sim / test / watch / deploy`，设计见[开发套件](designs/dev-kit.md)。场景、断言与世界推进应使用和服务器一致的运行及结算通路，使玩家能复现错误并验证自己的代码。

当前 `sim run` 已接入版本化 JSON 场景、多玩家模块、初始 Memory、最终状态断言和报告，见[交付记录](research/notes/sim-run-2026-10-04.md)。`init/watch/replay/test/deploy`、检查点恢复与任意 bot 的确定性复现尚未实现。

验收目标是固定场景中上千 tick 的集成测试秒级完成。报告须记录玩家代码、世界规模、构建配置、预热与耗时，不能把这个目标理解为任意 bot 或任意世界的耗时保证。当前部分 CLI 命令仍为占位入口，接口名称存在不代表工具已经完成。

## M3 网络、客户端与多用户

实现认证、账号、代码上传、房间订阅与控制台等核心 HTTP/sockjs 接口，使多个玩家在同一世界运行。市场端点随已有引擎能力接入，避免用空壳端点代替玩法。

自建客户端采用 Vue 3、自研渲染器和共享包，见[前端架构](architecture/frontend.md)。常量与 DTO 生成管线、构建产物嵌入和发布流程都属于交付范围；当前占位包不能证明它们已实现。房间观战先行，世界地图、回放与市场视图可在 M4 补齐。

验收须实际登录、上传代码、订阅房间并读取控制台，核心端点冒烟测试全部通过。官方客户端另做连接与协议验证，不能从玩家 API 通过率推导可连接；素材与发布条件也应按前端文档核实。

## M4 完整玩法与扩展

补齐 power、跨 shard 行为与通信、官方世界导入、地形生成，以及尚未完成的市场和其他玩法差异。完善 Mod 的服务及客户端能力、回放和历史界面，并根据负载需要实现多机部署后端，见[存储架构](architecture/storage.md)。

验收要求是全功能对照官方可玩并完成负载基准。后端选型不能自动证明跨进程原子提交、失败恢复或全域事务已经成立，相关故障场景必须单独验证。

## M5 生产化

按[性能验收方案](architecture/performance-acceptance.md)发布与官方及 xxscreeps 的正式对照报告，保留统一场景、正确性结果、原始数据和资源指标。

进行至少 72 小时的长跑，观察内存趋势、任务与句柄积压、延迟及故障恢复。验收要求内存稳定、没有已检出的泄漏；一次长跑不能证明所有路径都不存在泄漏。

公网部署前完成进程隔离、资源限额、内部宿主接口复查与恢复验证。seccomp 等平台机制须按部署系统选择，具体边界见[沙箱说明](architecture/sandbox.md)，不能把单进程开发环境直接视为公网安全方案。

## 验收原则

合入须通过适用的格式化、静态检查、测试和文档链接检查。当前 CI 另有完整 screeps-ok 回归任务，见[工作流](../.github/workflows/ci.yml)。新增生成管线时，应一并接入生成物新鲜度校验；常量与 DTO 的完整校验仍是交付要求。

兼容性验收同时检查通过数下限和逐用例回归，失败预期与回归豁免分别管理；不得靠扩大跳过范围提高通过率。偏差须按[兼容性要求](designs/compatibility.md)记录触发条件、可观察影响和验证依据。功能、bot、客户端与性能验收分别提供证据，不能相互替代。

历次批次、数字沿革与修订记录保存在[路线图历史记录](research/notes/roadmap-history.md)。历史记录按当时的实现理解，当前状态须核对源码与新报告。
