# 路线图

本页定义里程碑的范围与验收条件，不安排日期。先验证玩家运行时和结算机制，再完成单机玩法、开发工具、联网服务与生产部署。局部实验用于选择方案，完整系统的性能结论由端到端基准验证。

当前各组件的能力见[项目说明](../README.md)；具体机制与尚未实现的设计见[架构文档](architecture/README.md)。路线图中的目标不代表已经交付。

## 里程碑

| 里程碑 | 范围 | 验收条件与进展 |
|---|---|---|
| M0 兼容验收与工程基建 | CI、适配器和 bot 环境测试 | 已完成：可执行完整套件并出报告，两个真实 bot 在桩环境下运行，缺失 API 清零或登记 |
| M1 引擎骨架 | 类型化世界、单房间结算、嵌入接口 | 已完成：建立 screeps-ok 基线，CI 检查通过数下限与逐用例回归 |
| M2 单机可玩 | 沙箱、玩家 API、存储与完整世界 | 2A 已达到 ≥40% 门槛；2B 以 ≥95% 为中间检查点、≥99% 为后续覆盖目标，最终要求约定范围内功能全部通过验收、剩余例外逐项登记范围/证据/理由，并验证真实 bot 全功能运行 |
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

当前通过数下限以 [ratchet.json](../compat/reports/ratchet.json) 为准，不在路线图重复维护数字。M1 的实现与验收记录见[引擎骨架方案](plans/archive/m1-engine-skeleton.md)和 [ADR-011](decisions/ADR-011-engine-skeleton.md)。

## M2 单机可玩

M2 分为两道关卡，避免把运行环境可用与完整玩法可玩混为一谈。

- **2A：沙箱与核心玩家 API。** 接入玩家运行时、CPU 预算、Memory、世界投影和核心动作；screeps-ok 通过率达到 40%。该关卡已有验收记录，见[单机可玩方案](plans/archive/m2-playable.md)和 [ADR-012](decisions/ADR-012-m2a-sandbox-runtime.md)。CPU、内存及宿主隔离的具体边界以[沙箱说明](architecture/sandbox.md)为准。
- **2B：完整玩家 API、存储与完整世界。** 补齐玩法与玩家接口，在真实负载下完成存储选型和落盘，提供可启动完整世界及机器人对手的命令。验收要求（2026-10-09 调整）：screeps-ok 通过率 ≥95% 仅作为中间检查点，≥99% 为后续覆盖目标；最终完成要求约定范围内功能全部通过验收，每个未通过/跳过用例都有明确的范围、证据和可审查理由。通过数地板只升不降，不得靠扩大跳过范围、削弱断言或随意豁免达标；真实 bot 的经济、战斗、存活及重启能力单独验收，固定世界 bot-check 不能替代。

运行协调、失败重试、双房双玩家验证和结构整理已有阶段性成果，详见[方案索引](plans/README.md)。这些成果不能替代 2B 的整体验收；固定世界 bot-check 也不能证明孵化、建造和经济链已经正常推进。

玩家 API 扩面见 [M2B 方案](plans/archive/m2b-expansion.md)。terminal 与市场已开展独立实现和验收，见[终端与市场方案](plans/archive/2b-terminal-market.md)，后续应按剩余差异补齐，不能继续把整个市场视为 M4 才开始的能力。

核心仓库搬运与分片 GCL 输入已接入；两种玩家投影分别通过连续 40 tick 的采集、容器搬运和升级守恒场景，记录见[核心经济链与 GCL](research/notes/economy-gcl-2026-10-04.md)。现已提供 [sim run](architecture/local-simulation.md) 和可复跑的采集、搬运、升级、寿命到期后孵化替补场景；完整外部 bot 的玩法验收、特殊存储规则开放及数据库/迁移/日志恢复仍须继续推进；已提供显式[基础检查点](architecture/checkpoints.md)。

2026-10-09 新增 [TooAngel 单房经济与冷恢复验收](../compat/README.md#tooangel-单房经济与冷恢复)：锁定的未修改 bot 在 JSON/KKPV 各完成 2000 tick 连跑与 1000+1000 tick 跨进程续跑，验证实际采集、转账、建造、升级、死亡后补员和输入回放。TooAngel 首片不包含战斗或多房，不关闭 M2B 整体验收。随后新增 [Overmind 原版单房经济与冷恢复](../compat/README.md#overmind-持续世界与冷恢复) 和 [普通 Invader 持续战斗](../compat/README.md#普通-invader-持续战斗与冷恢复)：前者通过同样的六阶段经济/寿命矩阵，后者以实际位置/hits 与事件验证 NPC 行为及 200 tick 直跑/冷恢复等值。真实 bot 战斗、多房、长期存活与存储/完整世界仍未收口。

并发调度、统一提交和共享视图遵循 [tick 契约](architecture/tick-contract.md)与[快照契约](architecture/snapshot-contract.md)。这些文档分别说明现状和目标；提出契约不等于生产通路已经完整接入。端到端基线按[性能验收方案](architecture/performance-acceptance.md)采集，须先确认夹具实际产生动作与状态变化。

## JS Mod 与架构扩展

官方内容将按可选包和玩法预设交付，其中一部分直接使用公共 JS SDK，见 [ADR-016](decisions/ADR-016-official-content-packs.md)和[内容包设计](designs/content-packs.md)。首个生命周期/生产示例作为官方内容候选，用真实玩法验证公共接口、依赖组合与恢复；已有原生玩法逐族迁移，尚未提供完整内容选择。完整默认预设保持兼容性，精简/自定义组合发布实际能力；故障停用不等同于卸载内容。

独立 JS 包创造完整玩法、覆盖调研版本 xxscreeps 扩展领域，并提供更完善的组合、恢复和开发工具，是已确认的目标，见 [ADR-007](decisions/ADR-007-mod-system.md)。2026-10-05 的[现状复查](research/09-mod-capability-current-audit.md)基于 `1b40930` 核对了 13 个内建槽及其边界：动作改向/取消尚不等于伤害公式、建造配方或移动仲裁可替换。后续按[完整玩法计划](plans/mod-complete-gameplay.md)前移自有对象生命周期、候选对象操作、开放资源/能力和作者工具；N1a 已交付[自有对象 tick](architecture/mod-object-tick.md)、显式 Mod 场景装载和官方生命周期原型；N1b 已交付[同房对象操作](architecture/mod-objects.md)及 nursery 自主增删原型；N1c 已开放[handler 依赖与组合诊断](architecture/mod-composition.md)及 mod inspect；N1d 已交付[JS 有效规则读取](architecture/mod-rules.md)；N1e 已交付[新世界内容选择](architecture/mod-content-selection.md)；N2a 已交付[命名空间资源目录](architecture/mod-resource-catalog.md)。N2b 已交付[可建造建筑](architecture/mod-buildings.md)（G08.a 续交付建筑自定义字段与生命周期 tick，同文档），N2c 已交付[JS 单体伤害策略](architecture/combat.md#单体伤害)、[自定义治疗](architecture/combat.md#治疗)、[群体伤害策略](architecture/combat.md#群体伤害)和[反击表](architecture/combat.md#反击)四个切片。剩余默认动作槽和策略按完整玩法需求继续开放。

受击策略第一片已补齐[原生强化 TOUGH](architecture/creep-incoming-damage.md)：原生/JS 主伤害与反击在对象 tick 按受伤前身体合并减伤，再与治疗结算。第二片已开放 [JS 受击策略](architecture/combat.md#受击)：公式替换、护盾/免疫组合、来源摘要、拒绝默认回退与冷恢复；内建槽增至 14 个。实际伤害驱动的反击尚未开放，下一方向推进移动仲裁。

D5 已收敛玩家执行边界：引擎只接受显式指令和写回候选，玩家执行结果类型归到运行时，旧无参数 `Engine::tick` 与沙箱注册表已删除。运行时按注册身份过滤所有意图族，失败重试复用整轮候选，见 [tick 契约](architecture/tick-contract.md#玩家执行与结算接口)。

D4 资源领域后续片已开放 [ctx.resources](architecture/mod-resources.md)：同房候选库存读取、精确转账与损耗、额外对象局部拒绝日志和晚期写回失败缓存重试。

D4 移动片已开放 [creep.move 槽](architecture/creep-move-slot.md)：意图登记阶段的 before/wrap/replace/after、受控移动请求能力（改向/取消登记）与按执行者粒度的正常拒绝，实际位移仍走既有仲裁与执行阶段。

D4 战斗片已开放 [五个战斗槽](architecture/creep-combat-slot.md)（attack/rangedAttack/rangedMassAttack/heal/rangedHeal）：`ctx.combat` 目标改向/取消（恰一次消耗约束）、延迟伤害/治疗队列条目、rampart 重定向受击对象与结构摧毁废墟/对象移除（链内增删日志+ID 游标回退）的局部拒绝恢复；attackController 与容量等其他动作槽仍须逐族开放。

D4 建造族片已开放 [三个建造族槽](architecture/creep-build-slot.md)（build/repair/dismantle）：`ctx.build` 目标改向/取消（恰一次消耗约束；request 后刷新处理器视图）、完工变换（工地移除+结构插入）与 dismantle 击毁废墟/超容掉落面（链内增删日志+ID 游标回退+执行者格掉落面快照）的局部拒绝恢复；attackController 与容量族仍须逐族开放。

D4 首片已开放 Generic 仓库的 10 条容量数值，贯穿放置、建造、玩家常量、实验室槽切换和终端 tick，见[仓库容量规则](architecture/store-rules.md)。特殊存储策略及更多动作槽仍须逐族开放。检查点的数据域和版本清单前置，先保存当前完整已提交状态，随新增领域扩展迁移与日志；不等待全部规则开放后才开始恢复通路。2026-10-05 已交付[基础检查点](architecture/checkpoints.md)：显式保存当前完整已提交状态、严格身份核验、冷恢复与 sim 续跑；输入日志（2026-10-07，[契约](architecture/input-log.md)）、结算回放核对与 sim 级周期保存/恢复选择（2026-10-07，[周期保存与恢复选择](architecture/checkpoints.md#周期保存与恢复选择g26a)）已交付；升级迁移已由检查点格式版本链承担（G23.a）；服务器级自动周期调度、数据库后端与 outbox 后续扩展。

D6 首片已允许 link transfer 的 JS 处理链正常拒绝一次动作；后续 D4/D6 片已开放 [creep transfer/withdraw](architecture/creep-resource-slots.md) 与 [creep move](architecture/creep-move-slot.md) 的 before/wrap/replace/after 与正常拒绝。局部恢复执行者/目标库存、link 冷却、实验室槽容量、新增事件、移动请求登记及 Mod state，保留此前成功动作并继续结算。link tick、attackController、容量族和其他动作尚未开放正常拒绝，见[Mod 执行](architecture/mod-execution.md#正常动作拒绝)。

[ADR-014](decisions/ADR-014-contribution-plane.md)已把扩展基础安排在玩法扩面与首次持久化格式冻结之前，不能继续只列作 M4 的末端功能。Mod 作者使用 JS SDK；对象、指令、规则、候选状态和提交协议须共同设计，服务与客户端贡献随 M3 补齐。

实施范围见[贡献平面方案](plans/archive/mod-plane-batches.md)，机制见 [JS Mod 设计](designs/mod-system.md)。默认玩法仍须满足同一玩家契约；首版采用共同可信的玩法 VM。投影协议整体设计、分步接入，玩家任务并发另行验证；三方对照基准与扩展基础建设并行推进。新增封闭分派或硬编码白名单须同时提供具名扩展入口，或登记明确的例外依据。

## M2.5 开发套件

实现 `kkscreeps sim / test / watch / deploy`，设计见[开发套件](designs/dev-kit.md)。场景、断言与世界推进应使用和服务器一致的运行及结算通路，使玩家能复现错误并验证自己的代码。

当前 `sim run` 已接入版本化 JSON 场景（含世界 seed，玩法随机/时间协议见[确定性玩法时间与随机协议](architecture/gameplay-determinism.md)）、多玩家模块、初始 Memory、最终状态断言和报告，见[交付记录](research/notes/sim-run-2026-10-04.md)。显式检查点保存/冷恢复与 sim 续跑已接入。`init/watch/replay/test/deploy` 子命令与自动迁移尚未实现；结算回放核对已由 `sim run --verify-replay` 提供（G25.b），任意 bot 的服务器级确定性回放仍属后续。

验收目标是固定场景中上千 tick 的集成测试秒级完成。报告须记录玩家代码、世界规模、构建配置、预热与耗时，不能把这个目标理解为任意 bot 或任意世界的耗时保证。当前部分 CLI 命令仍为占位入口，接口名称存在不代表工具已经完成。

## M3 网络、客户端与多用户

实现认证、账号、代码上传、房间订阅与控制台等核心 HTTP/sockjs 接口，使多个玩家在同一世界运行。市场端点随已有引擎能力接入，避免用空壳端点代替玩法。

自建客户端采用 Vue 3、自研渲染器和共享包，见[前端架构](architecture/frontend.md)。常量与 DTO 生成管线、构建产物嵌入和发布流程都属于交付范围；当前占位包不能证明它们已实现。房间观战先行，世界地图、回放与市场视图可在 M4 补齐。

验收须实际登录、上传代码、订阅房间并读取控制台，核心端点冒烟测试全部通过。官方客户端另做连接与协议验证，不能从玩家 API 通过率推导可连接；素材与发布条件也应按前端文档核实。

**渲染器子项**（不代表整个 M3 完成）：

- [x] F2 R0–R6 房间观战、RoomVisual、交互及公共API冻结；
- [ ] F2 R7 性能/内存验收与整体收口：阶段生命周期修复、顶罩裁剪/复用、同tick门控、真实回放与三路清偿复审已完成；用户暂缓长时间测试，完整矩阵、压力soak及冻结指标最终验收待办，见 [短测报告](research/notes/renderer-r7-short-verification-2026-10-01.md)。
  放大黑屏已修复并通过双后端短GPU回归，见[缩放记录](research/notes/renderer-zoom-verification-2026-10-01.md)；该修复不代替最终性能/内存验收。
  RoomVisual节点/两份几何缓存与背景门控已接续优化，见[复核记录](research/notes/renderer-visual-reuse-verification-2026-10-01.md)；缓存份数有界不等于10k heap稳态。

## M4 完整玩法与扩展

补齐 power、跨 shard 行为与通信、官方世界导入、地形生成，以及尚未完成的市场和其他玩法差异。完善 Mod 的服务及客户端能力、回放和历史界面，并根据负载需要实现多机部署后端，见[存储架构](architecture/storage.md)。

验收要求是全功能对照官方可玩并完成负载基准。后端选型不能自动证明跨进程原子提交、失败恢复或全域事务已经成立，相关故障场景必须单独验证。

## M5 生产化

按[性能验收方案](architecture/performance-acceptance.md)发布与官方及 xxscreeps 的正式对照报告，保留统一场景、正确性结果、原始数据和资源指标。

进行至少 72 小时的长跑，观察内存趋势、任务与句柄积压、延迟及故障恢复。验收要求内存稳定、没有已检出的泄漏；一次长跑不能证明所有路径都不存在泄漏。

公网部署前完成进程隔离、资源限额、内部宿主接口复查与恢复验证。seccomp 等平台机制须按部署系统选择，具体边界见[沙箱说明](architecture/sandbox.md)，不能把单进程开发环境直接视为公网安全方案。

## 验收原则

Mod 后续工作使用[30 项任务与接手台账](plans/mod-next-batches.md)维护当前执行位置、依赖、实际验收和剩余范围。AI 接手及每批结束主动更新台账，并将里程碑变化同步到本路线图；架构契约仍以 architecture 为准。

执行按台账的子片队列而非父编号串行：作者工具/类型、覆盖审计及默认参考前置；新增写入域当批补齐保存恢复；持久输入/outbox 与存储的原子发布一并验收。M3 最小网络与客户端是实际开发工作，不是默认已就绪的 Mod 前置。覆盖台账有遗漏时继续拆片，不以现有 30 个父任务的首片完成代替 M4/M5 收口。

合入须通过适用的格式化、静态检查、测试和文档链接检查。当前 CI 另有完整 screeps-ok 回归任务，见[工作流](../.github/workflows/ci.yml)。新增生成管线时，应一并接入生成物新鲜度校验；常量与 DTO 的完整校验仍是交付要求。

兼容性验收同时检查通过数下限和逐用例回归，失败预期与回归豁免分别管理；不得靠扩大跳过范围提高通过率。偏差须按[兼容性要求](designs/compatibility.md)记录触发条件、可观察影响和验证依据。功能、bot、客户端与性能验收分别提供证据，不能相互替代。

历次批次、数字沿革与修订记录保存在[路线图历史记录](research/notes/roadmap-history.md)。历史记录按当时的实现理解，当前状态须核对源码与新报告。

2026-10-01 更新：M3 renderer 新增本地官方房间完整 100tick 回放，短窗口定位并修复重复对账/顶罩 source churn/Graphics 释放，369 测全绿，三路评审通过，[阶段证据](research/notes/renderer-r7-short-verification-2026-10-01.md)。用户暂缓长时间验收，R7 与 M3 最终验收继续待办；短测没有证明 10k 稳态。

2026-10-07 复审范围纠偏：移动准入 G02 仍为部分完成；墙地形/结构等静态阻挡放行、玩家预检与仲裁复检贯通已登记 G02.d，属于完整 JS 玩法验收的剩余消费者。稳定化中的 UTC Date、协议身份、哈希游标、建筑记录校验和批量初始化不代表这些新玩法已经开放；具体进度见持续台账。

2026-10-07 G02.d1 静态移动放行已交付（完整门禁与 release ratchet 通过）：公共 JS 包可放行墙/结构/私有 rampart/己方工地，准入预演与仲裁同源，动态占格及组合否决保持。剩余疲劳/MOVE 玩家门与 moveTo/PathFinder 寻路消费者登记 G02.d2；G02 父项仍部分完成。契约见[移动预检](architecture/movement.md#请求预检)，最终检查与提交以持续台账为准。

2026-10-07 G02.d2 移动玩家门双端冻结已交付（完整门禁与 release ratchet 通过）：fatigueLimit/minActiveMoves 两条规则同源驱动玩家面 move()/moveTo() 预检与仲裁原生预填；moveTo 不穿墙、PathFinder 不感知放行已由回归测试钉死。声明式寻路规则登记 G02.d3 设计；G02 父项仍部分完成。契约见[玩家预检规则](architecture/player-precheck-rules.md)，最终检查与提交以持续台账为准。

2026-10-08 G02.d3 首片墙地形静态通行声明已交付（完整门禁与 release ratchet 通过）：`builtin:terrain.wallPassable` 规则同源驱动仲裁静态墙项与玩家面原生寻路地形（三通道），物理地形查询保持原值，全墙保护环不放开；默认零行为变化（棘轮零转红）。结构/rampart 通行声明属后续切片，G02 父项仍部分完成。契约见[静态通行声明](architecture/movement.md#静态通行声明g02d3)，最终检查与提交以持续台账为准。

G27.a 后台首片已交付独立线程/V8、受控异步生命周期、已提交世界副本读面、有限只读探针和作者 CLI，见[后台服务](architecture/background-services.md)。M3 的耐久输入/outbox、storage/provider、HTTP/WS 与客户端仍按原验收范围推进，不从后台首片推导为完成。
