# 调研底稿

本目录保存源码分析、社区项目研究和技术实验的历史证据。报告结论适用于记录的版本、环境和场景；它们是决策依据，不是当前实现或完整系统收益的证明。已采用的方案见[决策记录](../decisions)，当前机制与目标设计见[架构文档](../architecture)。

01–04 对应最初三轮调研（第一轮 = 01 与 02，第二轮 = 03，第三轮 = 04）；05 是报告编写时的 Mod 扩展能力专项复查；06 在用户明确 JS-only 后复审整体架构，并以新鲜构建和 JS/native 交互实验提出实施路线；07 调查注释的可读性与事实准确性，并提出规范和整改方案。建议按问题选择阅读；`notes/` 是报告的原始证据，报告中关键结论可回溯到出处。

| 报告 | 问题 | 报告结论与适用范围 |
|---|---|---|
| [01 可行性研究](01-screeps-rust-feasibility.md) | Rust 重构可行吗？收益多大？ | 支持 Rust 重写方向；序列化、存储和任务等待是候选优化点，整体收益预估仍需正式对照 |
| [02 沙箱选型](02-sandbox-evaluation.md) | 哪个 JS 引擎能承载玩家代码？ | 实验支持选择 deno_core（V8）；QuickJS 有条件可行，Boa 未满足选型要求，均不代表完整游戏 API 验收 |
| [03 架构深挖](03-architecture-optimizations.md) | 官方还有哪些可优化的地方？ | 提供序列化、查询等局部实验与架构建议；单项倍率不能代表完整系统收益 |
| [04 游戏功能完整性](04-game-api-feasibility.md) | deno_core 能实现游戏的全部功能吗？ | 语言与宿主交互实验支持实现方向；寻路回调实验可行（地形直通已落地，回调通道与完整功能验收尚未完成） |
| [05 Mod 扩展能力](05-mod-extensibility-review.md) | 当前架构能达到官方与本地 xxscreeps 的 Mod 扩展能力吗？ | 当前尚未提供完整 Mod 通路；技术方向可保留，需要前移对象、意图、规则替换和事务等扩展契约 |
| [06 架构与 JS Mod 复审](06-architecture-and-js-mod-review.md) | 整体架构应如何改造，深度玩法能否全部用 JS Mod 实现？ | 保留 Rust/V8/房间模型，补全全域提交、并发派发、开放规则；JS/native 嵌套调用实测可行，完整实现仍待切片 |
| [07 注释质量调研](07-comment-quality-review.md) | 现有注释为什么难读，怎样避免误导维护者？ | 修复过期说明，分离历史与当前行为，形成[注释规范](../contributing/comment-style.md)和[术语词典](../glossary.md) |
| [08 开发进展与后续建议](08-development-progress-and-next-steps.md) | 当前做到哪里，接下来优先开发什么？ | 基于 2026-10-04 的源码与入库报告核查，建议 D3 寻路回调、核心经济场景和完整保存恢复，并指出全套件 95% 与后置玩法的覆盖冲突 |
| [09 JS Mod 完整玩法现状复查](09-mod-capability-current-audit.md) | 独立 JS 包与 xxscreeps 领域覆盖还差什么，优先级怎样调整？ | 基于 `1b40930` 与本地 xxscreeps `5ff0d1a6` 核查；13 个内建槽已接线，但自主对象、开放资源/能力、深层策略与服务/恢复仍有缺口 |

## notes/ 笔记索引

- [Mod 增量深度复审与架构质量检查](notes/mod-increment-deep-review-2026-10-07.md)：`e8ed252..d8af3e6` 的恢复、确定性、压制、类型与路线反例；补查建筑字段全入口校验、批量管理开销及职责边界，合计 5 项 P1、6 项 P2，含最小夹具与机器证据，尚未修复。
- [G01 移动竞争槽](notes/g01-move-contest-2026-10-06.md)：整房竞争优先级槽、请求矩阵输入与默认排序回退。
- [G02.a 移动准入槽](notes/g02-move-admission-2026-10-06.md)：房间级准入槽、原生门槛预填与终值消费。
- [G02.b 移动障碍槽](notes/g02b-move-obstacle-2026-10-06.md)：单向追加阻挡与让位链传播。
- [G02.c 移动预检槽](notes/g02c-move-precheck-2026-10-06.md)：请求级单向否决与"等同从未登记"语义。
- [G03.a 整房仲裁链预演](notes/g03a-move-arbitration-2026-10-06.md)：请求视野扩展与 admission.evaluate 只读链预演。
- [G04.a 移动代价槽](notes/g04a-move-cost-2026-10-06.md)：疲劳数值替换与原生回退。
- [G06.a Mod 运行 trace](notes/g06a-mod-trace-2026-10-06.md)：trace 开启意愿、恢复后启用与诊断快照边界。
- [G08.a 建筑自定义字段与生命周期](notes/g08a-building-fields-2026-10-06.md)：字段存储键与写面、freeze 五门、玩家面六层直供、停用语义定论与 bl/bk 双包冷恢复。
- [G07.a 玩家预检规则·战斗射程首片](notes/g07a-combat-ranges-2026-10-06.md)：射程规则双端同源、常量注入、reach/medic 双包与四场景双通道冷恢复验收。
- [G07.b ext 意图压制与可扩展意图盘点](notes/g07b-ext-intent-suppression-2026-10-07.md)：suppressed_by 声明/冻结校验/存在性执行接线、SDK suppressedBy、suppress/free 双包四场景验收。
- [G04.a 移动代价槽](notes/g04a-move-cost-2026-10-06.md)：疲劳数值替换、沼泽原生对照与行军节奏验收。
- [G03.b 拉拽登记策略](notes/g03b-pull-registration-2026-10-06.md)：配对级否决槽、add_pulling 双射修复与遍历顺序无关化、anchor/leash 双包验收。
- [G29.a 玩家类型生成](notes/g29a-mod-types-2026-10-06.md)：冻结计划确定性 TS 声明、--check 新鲜度门与输出守卫。
- [G22.a 状态清单与 schema 门禁](notes/g22a-state-inventory-2026-10-06.md)：检查点逐域清单、钉死键树门禁（交付时 99 键，G22.c 后 100）、键树折叠规则、墙钟订单夹具修正与变异验证。
- [G22.c 活动执行策略的持久身份](notes/g22c-execution-strategy-2026-10-06.md)：组合报告域分隔摘要、ModVm 恢复自检、检查点 `execution_strategy` 键（99→100）与 sim 报告 v5。
- [G23.a 检查点格式版本与显式迁移](notes/g23a-checkpoint-migration-2026-10-06.md)：必填 `checkpoint_format_version`、解码前迁移链（v1→v2 收编 last_auto_disable 旧形态）、格式≥2 执行策略硬门、版本职权四层与钉死键 100→101。
- [G24.a 无开放轮次的包/API 切换](notes/g24a-package-switch-2026-10-06.md)：替代分片换装执行层、引擎侧换计划收养（内容校验充当残留硬门）、停用投影与故障过滤、工作者常量封闭集重注入、sim `mods_switch` 场景字段与冷恢复生效集推导。
- [G22.b 确定性玩法时间/RNG](notes/g22b-determinism-2026-10-07.md)：无状态派生随机流（seed+域+tick+序号）、轮作用域重置、SDK 确定性 Date/Math.random、seed 注入面与 restore/switch 种子错位修复。
- [G25.a 持久输入与玩家结果日志](notes/g25a-input-log-2026-10-07.md)：轮级提交后追加日志（意图/写回/墙钟/CPU 桶）、链哈希信封与撕裂尾检测、校验读取器与 sim --input-log。
- [G25.b 结算回放与完整状态核对](notes/g25b-replay-2026-10-07.md)：检查点+日志段配对规则（接缝/头段种子门）、回放内核与逐位等值核对、settle_external 规范化、sim --verify-replay（replay_failed 退出语义）。
- [G26.a 自动检查点](notes/g26a-auto-checkpoint-2026-10-07.md)：周期保存与 RPO 恢复选择（目录布局/最新可用降级/零段合法终态/失败语义）。
- [JS 受击策略第二片](notes/n2c-incoming-policy-2026-10-06.md)：公式替换、独立护甲/护盾与免疫、来源摘要、拒绝回退、缓存重试及冷恢复。
- [受击策略第一片：原生 TOUGH](notes/n2c-tough-damage-2026-10-06.md)：累计减伤、治疗/死亡/库存、JS 主伤害/反击、拒绝和受伤后的冷恢复。

- [N2c JS 反击表](notes/n2c-retaliation-policy-2026-10-06.md)：原生与 JS 主攻击组合、实际命中、结构荆棘、反击致死重试和进程冷恢复。
- [N2c JS 群体伤害策略](notes/n2c-mass-policy-2026-10-06.md)：战斗分页、完整批量预检、多目标和孵化对象恢复、双通道与进程冷恢复。
- [N2c JS 治疗策略](notes/n2c-heal-policy-2026-10-05.md)：独立固定/比例治疗包、延迟身体恢复、局部拒绝与缓存重试、治疗前后进程冷恢复。

- [主项目结构治理](notes/source-structure-2026-10-05.md)：17 个超长源码/测试文件按职责拆分、串行入口保持及文件规模门禁。
- [D4 资源领域能力](notes/d4-resource-domain-2026-10-05.md)：精确转账、显式损耗、同房多对象恢复、能力过期与晚期写回重试。
- [D5 玩家执行边界](notes/d5-player-execution-2026-10-05.md)：删除旧引擎沙箱入口、结果类型归位、全意图族身份过滤与双玩家缓存重试。
- [D4/D6 creep 资源动作槽](notes/d4-creep-resource-slots-2026-10-05.md)：双向处理链、权威只读面、实验室恢复、竞争与停用重试。
- [D6 link 动作正常拒绝](notes/d6-link-decline-2026-10-05.md)：多对象与事件恢复、跨 Mod state、竞争、异常优先及缓存重试。
- [D4 仓库容量规则](notes/d4-store-rules-2026-10-05.md)：JS 数值覆盖、建造与槽切换、玩家安装预检一致、竞争与失败重试。
- [真实 bot 场景执行入口](notes/sim-run-2026-10-04.md)：版本化场景、多玩家模块与 Memory 初始化、已提交状态断言、失败退出及连续经济场景。
- [核心经济链与 GCL 输入](notes/economy-gcl-2026-10-04.md)：通用仓库容量、实验室槽切换、初始 GCL、真实玩家闭环和写回失败重试。
- [D3 寻路回调记录](notes/d3-pathfinder-callback-2026-10-04.md)：同步 V8 重入、可变矩阵、取消、官方差异及验收。

- [D2 寻路地形直通记录](notes/d2-pathfinder-terrain-2026-10-04.md)：原生搜索接线、TLS 物理内存回归定位与修复、棘轮 1859→1879 与三基线归因。
- [玩家闭环提交前复审](notes/mod-player-review-2026-10-04.md)：原生目标效果、常驻报告门禁、负面测试与 v2 独立采集。
- [玩家与 Mod 连续闭环采集](notes/mod-player-workload-2026-10-04.md)：逐玩家真实动作核验、恢复后连续执行、两种通道的同负载原始窗口。
- [双玩家与 Mod 的恢复闭环](notes/mod-multiplayer-recovery-2026-10-04.md)：双房间、两种投影、玩家类方法、串行参考、自动停用与晚期写回失败。
- [native/empty 长窗口与平衡采集](notes/mod-native-balanced-2026-10-04.md)：ABBA/BAAB、逐对与逐轮口径、60 个原始窗口及同二进制 A/A；历史差异仍未归因。
- [看门狗通知合并](notes/mod-watchdog-notify-2026-10-04.md)：保留逐调用预算、减少重复唤醒，资源与兼容性门禁及全部配对窗口。
- [Mod 逐 tick 诊断与桌面测量波动](notes/mod-tick-diagnostics-2026-10-04.md)：同二进制 A/A、全部原始 CPU/切换记录和普通/诊断门禁；历史回归仍未归因。
- [Mod 连续故障计数与可配置自动停用](notes/mod-auto-disable-2026-10-04.md)：干净失败边界、可信来源筛选、跨 VM 计数与缓存重试。
- [Mod 显式停用与回退](notes/mod-disable-execution-2026-10-04.md)：停用依赖闭包、保留冻结目录、恢复原始中心与缓存玩家重试。
- [Mod 转换、拒绝与故障来源边界复审](notes/mod-boundary-review-2026-10-04.md)
- [Mod 执行故障来源与跨 VM 恢复诊断](notes/mod-fault-attribution-2026-10-04.md)
- [Mod 自有动作正常拒绝与局部候选恢复](notes/mod-intent-decline-2026-10-04.md)
- [Mod 转换边界：注册重入、故障留存与结果转换](notes/mod-conversion-boundaries-2026-10-04.md)

**Mod 拥有值写入封套**（2026-10-04）：[省去外层 Map 与转换 panic 清偿](notes/mod-write-envelope-2026-10-04.md)（动态遍历保留 getter/未知值故障，直接移交 state/fields，Symbol 子进程与候选回滚）。

**Mod 成对采集**（2026-10-03）：[可配置窗口与成对口径](notes/mod-paired-sampling-2026-10-03.md)（默认更长窗口、交替两端次序、逐轮 P99/CPU 和倍率、证据保护）。

**Mod 写入转换计时**（2026-10-03）：[直接计时与 getter 预算边界](notes/mod-write-conversion-2026-10-03.md)（诊断 v3、包含 getter 的真实转换计时、嵌套计数与 CPU/墙钟中断）。

**Mod 上下文复审**（2026-10-03）：[getter 边界与数值门禁](notes/mod-context-review-2026-10-03.md)（重入 abort、转换故障留存、先核验能力再转换，以及无效测量拒收）。

**Mod 类型化读取封套**（2026-10-03）：[固定封套与直接转换计时](notes/mod-context-envelope-2026-10-03.md)（固定上下文成员走 serde struct 路径、开放载荷保持 JSON、v2 阶段门禁与前后对照）。

**Mod 上下文复制优化**（2026-10-03）：[拥有值装配与写回](notes/mod-context-copy-2026-10-03.md)（减少重复 Value 遍历/克隆、JSON 类型与链语义验证、最终 release 前后对照）。

**Mod 预算复审与密集负载**（2026-10-03）：[清偿与纯结算对照](notes/mod-budget-review-and-perf-2026-10-03.md)（超时异常转换持续中断、完整链计数与全对象状态验收、native/空包/完整链重复测量）。

**Mod 热路径归因**（2026-10-03）：[包含计时与字段缓存](notes/mod-attribution-2026-10-03.md)（诊断/基线分离、候选解析缓存失效、24 次前后交替对照与原始数据）。

**Mod VM 资源预算**（2026-10-03）：[预算与故障验收](notes/mod-vm-budget-2026-10-03.md)（共用 CPU 时钟/看门狗、整轮累计限额、V8 安全点与候选失败恢复）。

**Mod VM 故障恢复**（2026-10-03）：[恢复与故障验收记录](notes/mod-vm-recovery-2026-10-03.md)（固定包快照、计划核验、污染 VM 销毁、玩家缓存复用与恢复门）。

**沙箱调研**（02 报告证据）：[sandbox-denocore.md](notes/sandbox-denocore.md)、[sandbox-quickjs.md](notes/sandbox-quickjs.md)、[sandbox-boa.md](notes/sandbox-boa.md)、[sandbox-alternatives.md](notes/sandbox-alternatives.md)

**沙箱实测**（02/04 报告证据，对应 `spikes/` 下可复跑的实验工程）：[spike-deno_core.md](notes/spike-deno_core.md)（中断/内存/性能基准）、[spike-rquickjs.md](notes/spike-rquickjs.md)、[spike-boa.md](notes/spike-boa.md)、[spike-deno-game-api.md](notes/spike-deno-game-api.md)（游戏 API 依赖的交互模式六组实验 + 指令回调实验 + 预算桶仿真）、[spike-deno-inspector.md](notes/spike-deno-inspector.md)（V8 inspector 断点调试：CDP 全链路 / 暂停×看门狗 / 全局面 diff / 快照 / 多 isolate 多 target）；另有 [rquickjs-run-output.txt](notes/rquickjs-run-output.txt)（rquickjs 安静负载轮的原始输出存档）

**架构深挖**（03 报告证据）：[arch-processor.md](notes/arch-processor.md)（引擎结算热路径）、[arch-runner.md](notes/arch-runner.md)（玩家运行时固定开销）、[arch-xxscreeps.md](notes/arch-xxscreeps.md)（社区重写项目的增量设计）、[arch-storage-backend.md](notes/arch-storage-backend.md)（存储与网络层，含官方闭源存储组件的解包实证）、[arch-community.md](notes/arch-community.md)（社区与官方一手情报）

**功能盘点**（04 报告证据）：[game-api-inventory.md](notes/game-api-inventory.md)（从中文文档站盘点的完整玩家功能面）、[deno-game-impl-mapping.md](notes/deno-game-impl-mapping.md)（功能组到沙箱能力的逐项映射）

**实现复查**（2026-09-28 优化轮证据）：[m0-m1-code-quality-audit.md](m0-m1-code-quality-audit.md)

**Mod 扩展实测**（05 报告证据，2026-10-01）：[复现脚本](../../scripts/benchmark/research/mod-extensibility-probe-2026-10-01.py)、[探针结果](notes/mod-extensibility-probe-2026-10-01.json)（临时内存世界的 Generic 管理查询、玩家对象字段与自定义 FIND 观察）

**整体架构与 JS Mod 新实验**（06 报告证据，2026-10-01）：[当前构建性能采集](../../scripts/benchmark/research/architecture-perf-probe-2026-10-01.py)、[12 次原始性能结果](notes/architecture-perf-probe-2026-10-01.json)；[JS 宿主采集](../../scripts/benchmark/research/js-mod-host-probe-2026-10-01.py)、[5 次 release + debug 结果](notes/js-mod-host-probe-2026-10-01.json)；[Rust 宿主原型](../../spikes/deno-spike/examples/js_mod_host.rs)、[JS 行为原型](../../spikes/deno-spike/examples/js_mod_host.js)。均为研究证据，非生产 Mod 实装或正式性能验收。

**架构再思考复核**（ADR-014 证据，2026-10-01）：[architecture-rethink-2026-10-01.md](notes/architecture-rethink-2026-10-01.md)（对 05/06 断言的双路独立核实、xxscreeps 机制取证要点、定型结论与数字勘误表）

**贡献链驱动模型**（2026-10-03，C 批前置片证据）：[链驱动模型记录](notes/mod-chain-driver-2026-10-03.md)（JS 驱动链的最小实验：两层 wrap 嵌套 + Rust native 中心全程 0 个 reentrant 标记；附 `after` 时点陷阱）、[Rust 实验体](../../spikes/deno-spike/examples/js_driven_chain.rs)、[JS 驱动器](../../spikes/deno-spike/examples/js_driven_chain.js)。结论推翻已落地实现的 reentrant 方向，重做与否待定。

**KKPV 双通道规模矩阵**（2026-10-04，C.5 收口证据）：[规模矩阵记录](notes/kkpv-scale-matrix-2026-10-04.md)（4/8/32 玩家 × JSON/KKPV × 5 轮双重交错——通道序交换+规模序轮换：CPU/wall 亏损随规模收窄但 4..32p 无回本点、每玩家字节优势 −65%→−90% 单调扩大、RSS 8p→32p 交叉反超；32p 方向与 B.6 双样一致、幅度偏窄——跨会话比值不可比）、[采集脚本](../../scripts/benchmark/research/kkpv-scale-matrix-capture-2026-10-04.py)（门禁+原子归档，单测见 scripts/tests）、[全部原始报告](notes/kkpv-scale-matrix-windows-2026-10-04.json)。

**存储选型基准**（2026-09-29 M2-C3 骨架证据）：[storage-bench.md](notes/storage-bench.md)（redb vs libmdbx 微基准方法学、三档场景夹具、macOS arm64 数字（初轮 + 评审清偿后复跑）与选型判据草案——非正式选型，正式选型待 2A 后真实负载）

**测试套件评估**：[screeps-ok 初期质量评估](notes/screeps-ok-evaluation-2026-09-27.md)保存 ADR-006 的原始详细评估，当前采用与回归规则见 [ADR-006](../decisions/ADR-006-screeps-ok-ci.md)。

**本地共同负载对照**（2026-10-02）：[P2 对照记录](notes/p2-local-comparison-2026-10-02.md)保存本项目与本地 xxscreeps 的生产沙箱采集、工作量核对及测量边界；官方私服尚未接入。

**开放领域定义**（2026-10-02）：[B.1 定义冻结](notes/b1-definition-freeze-2026-10-02.md)记录纯数据贡献层、link 默认描述与验证边界；生产引擎与玩家投影尚未接入冻结计划。

**文档历史**：[兼容性历史登记](notes/compatibility-register-history.md)、[路线图历史记录](notes/roadmap-history.md)保存正文整理前的完整记录，便于追溯旧结论；现行要求见[兼容性说明](../designs/compatibility.md)和[路线图](../roadmap.md)。

## 阅读注意

1. 报告中的结论按置信级别标注：**【实测】**（有可复现的实验或命令输出）、**【文档】**（有官方文档或源码出处）、**【推断】**（实测分量的线性外推或架构推理）——引用数字时请连同级别一起引用；
2. 02 报告的原始建议是"deno_core 主力 + QuickJS 备选档"，后来用户拍板放弃备选档，见 [ADR-002](../decisions/ADR-002-deno-core-sandbox.md)——报告保持历史原样，不做追改；
3. 笔记里的实验记录了精确的依赖版本、命令与原始输出，复跑方法见各 spike 工程目录。

- [JS 整槽处理链接线（2026-10-03）](notes/js-chain-execution-2026-10-03.md)：真实候选同步、嵌套 wrap 与 native、错误留存和兼容性验证。
- [N1a 自有对象 tick（2026-10-05）](notes/n1a-object-tick-2026-10-05.md)：自主生命周期、官方公共 SDK 原型、Mod 场景与提交恢复验收。
- [N1b 同房对象操作（2026-10-05）](notes/n1b-mod-objects-2026-10-05.md)：候选对象查询/创建/更新/删除、nursery 自主增删与双房故障恢复。
- [N1c 处理器依赖与组合诊断（2026-10-05）](notes/n1c-mod-composition-2026-10-05.md)：dependsOn 冻结图、mod inspect、停用闭包与双房场景核对。
- [N1d 有效规则读取（2026-10-05）](notes/n1d-mod-rules-2026-10-05.md)：执行期读取冻结有效值、注册期拒绝、恢复/停用一致与双房双通路场景。
- [Mod 规则与资源复审修复（2026-10-05）](notes/mod-review-fixes-2026-10-05.md)：规则表失败安装不留残留、拾取资源目录准入及文档示例双通路执行。

- [基础检查点与冷恢复](notes/basic-checkpoints-2026-10-05.md)：完整状态清单、严格身份核验、单文件发布与 sim 重启续跑。
- [N2b Mod 建筑（2026-10-05）](notes/n2b-mod-buildings-2026-10-05.md)：建筑声明域、多资源建造与生产结算、双官方包、三场景双通路与检查点续跑证据。
- [N2b review 修复（2026-10-05）](notes/n2b-review-fixes-2026-10-05.md)：验收夹具纳入版本控制、生产最终容量校验、出生障碍目录和干净源码复验。
- [N2b 资源向量质量修复（2026-10-05）](notes/n2b-resource-vector-quality-2026-10-05.md)：冻结层唯一性/排序身份、负库存复现、完整最终报告的冷恢复对照。
- [N2c JS 单体伤害（2026-10-05）](notes/n2c-single-target-damage-2026-10-05.md)：同房战斗读面、自定义公式/应用、双 JS 变体与拒绝/冷恢复证据。

- [深度质量优化、门禁与性能原始证据](notes/deep-quality-2026-10-06.md)

- [增量复审深度修复与结构优化](notes/mod-deep-repairs-2026-10-07.md)：R1–R11 处置、宿主协议身份、初态摘要/哈希游标、建筑记录校验与原子批量初始化；最终复审、完整门禁和兼容 ratchet 结果见记录及机器证据。

- [G02.d1 静态移动放行](notes/g02d1-static-movement-2026-10-07.md)：登记快照、仲裁/预演同源、独立 ghost/seal 包、双通道冷恢复及回放；G02 父项保留剩余玩家预检消费者。
- [G02.d2 移动玩家门双端冻结](notes/g02d2-move-playergate-2026-10-07.md)：fatigueLimit/minActiveMoves 规则化双端同源、方向形态返回码钉死与寻路边界回归（moveTo 不穿墙、PathFinder 不感知放行）。
