# Mod 后续开发任务与接手台账

本页维护**当前状态、执行队列、未完成范围和最近验证**。长期目标见[完整玩法计划](mod-complete-gameplay.md)，现行机制见[架构索引](../architecture/README.md)，已完成批次的过程见[历史记录](archive/mod-history-2026-10.md)。G01–G30 是稳定任务编号，不是串行开发顺序；历史记录中的“下一步”“暂停”“未提交”只属于当时。

## 当前接手位置

- 本轮接手复审已完成（2026-10-12，HEAD `59710e0b`）：S2.3 补齐预算中断、耐久结果、失败保存、输入固定/回放、授权与配置迁移；6 项 Review 问题已清偿，完整门禁与兼容棘轮通过，已完成并纳入本次提交。下一接手入口 S2.4；不标 G27.b/M3 全域完成。

- 更新日期：2026-10-12；S1.1（54db7774）、S1.2（c27db2d3）、S1.3（78b28e15）、S2.1（f2001ed2）与 S2.2（59710e0b：代码边界事务）已实现：serve init/run、严格版本化配置、网格调度（anchor/pending 两点接线）、Paused/Persistence 故障语义、周期保存与收尾保存、stdio 控制、SIGTERM 优雅退出与租约互斥；两玩家/两房/nursery Mod 场景冷恢复续存验收（世界/Memory/CPU/入口/Mod 状态核对、JSON/KKPV 双通道、撕裂回退与同步失败不发布水位）；耐久命令面 v2（inbox 接收/幂等/预算/流生命周期、effects 恢复下限、enable-inbox 启用事务、FIFO 命令消费、code.replace CAS+即时屏障+崩溃窗口两段确认、链路级矩阵用例）见各实施记录。S2.3 玩家表达式与受限 Mod 输入已实现（本次提交，接手 Review 和完整门禁已完成，实施记录见下节）；下一入口 S2.4 outbox；G27.b/G28.a 等父项状态不变。
- 性能主线已按用户要求暂缓：50 玩家×6 个 RCL8 满功能成熟房，共 300 房。已交付 KKPV 共享编码/视图复用、信封直构、条件逐房并行、克隆-结算重叠、预校验 commit 与融合校验；完整紧凑存储/变化集/列式共享直读仍为目标。旧投影预取两版已撤回，恢复时从 run 注入协议重新核验归因，最终验收未完成。
- 最新标尺记录为 kk/xxs 耗时比中枢 1.0492、热窗 0.9859，来源为[归档接手记录](archive/mod-history-2026-10.md#接手位置沿革)；未找到随最新两片入库的同口径双端标尺明细，不能据此声称本轮独立复算或已正式领先。热态、分段收益及原始窗口须一起判断。
- 默认玩法：19/19 powers 已接通；[兼容机器报告](../../compat/reports/kkscreeps.json)为 2905/2914 passed、9 failed、0 pending，原始 success=false，按既有差异登记通过棘轮不等于套件全绿或 M2B 整体完成。地图生成、stronghold 战术 AI/create-creep、bot 对战、多房扩张运营与 Overmind 旗标出兵仍有缺口。
- 真实 bot 已有经济、防御、双房进攻和长期存活/冷恢复证据，见[验收说明](../../compat/README.md)。各报告绑定归档二进制，本 HEAD 未重跑；86400 tick 是 72h 等价游戏时间，不是 72h 墙钟服务长跑。
- Mod 已有对象/资源/建筑、部分战斗/移动策略、作者 CLI/类型、检查点与回放；默认玩法消费者已实现不代表 Mod 替换能力开放，父项缺口见下表。
- 服务线：G27.a 独立后台宿主/只读探针已交付；S1 常驻调度/无日志恢复与 S2.1–S2.3 耐久命令首片已实现；G27.b 的后台 enqueue/outbox、数据库/provider、HTTP/WS 和完整客户端仍未交付。renderer R0–R6 已交付，R7 最终矩阵/长测未收口，此前暂缓的长测不因本轮文档整理自动启动。

## 当前执行队列

以下仅安排未完成工作；性能主线按用户 2026-10-12 要求暂缓。用户已选择服务功能方向，S1 与 S2.1–S2.3 已实现并验证，当前实施入口为 S2.4；网络/后台 enqueue 与 outbox 的目标设计不视为已交付。服务线保留 G27.b → G28.a 的硬依赖；Mod 领域按子片依赖选择，不按父编号机械串行。

| 顺位 | 任务 | 首片与完成条件 |
|---|---|---|
| 1（进行中：S1 已交付，S2 进行中） | S1 常驻运行与无日志检查点恢复 | S1（54db7774/c27db2d3/78b28e15）已交付；S2.1（f2001ed2：inbox 接收面/幂等/预算/流生命周期、effects 恢复下限锚点、service v2 元数据与选择门、enable-inbox 启用事务含中断续接）与 S2.2（59710e0b：FIFO 命令消费、code.replace CAS/校验/即时屏障、崩溃窗口两段确认、rejected 不建屏障、统一冻结语义）已交付；S2.3 玩家表达式与受限 Mod 输入已实现并通过接手 Review（本次提交）；下一入口 S2.4；默认关闭审计日志，恢复后同目录续存；此片不标 G27.b 或 M3 完成 |
| 2（进行中：S2.1/S2.2 已交付） | S2/G27.b 耐久输入/outbox | S2.1（f2001ed2）与 S2.2（本次提交）已交付；S2.3 表达式/受限 Mod 输入已实现并验证（本次提交）；当前入口 S2.4 幂等投递；最终确认与外部投递等待耐久屏障，恢复不跨已确认下限；不等待 JS storage provider |
| 3（设计完成，写入依赖 S2） | S3/G28.a → G29.b 最小真实联网闭环 | S3.1 认证/命令/DTO → S3.2 授权房间订阅 → S3.3 两玩家实际客户端 → S3.4 分发/只读 provider；固定地图/预建玩家首片，官方 HTTP/sockjs 兼容另验，不标 M3 或 G21 全域完成 |
| 随服务验证 | Linux 服务容量与恢复验收（2GB 参考） | 50 玩家×6 房、300 占用房，另覆盖目标 9 区块地图的注册地形规模；短探针只覆盖 300 房，不能代表整幅地图或成熟 bot 负载。长窗观察 RSS/GC、tick 延迟、保存峰值与冷启动，区分合成 worker 与真实 bot；同机 xxscreeps 正式对照另按性能验收口径完成 |
| 独立候选 | 默认玩法与真实 bot 补验 | bot 对战、多房扩张运营、Overmind 旗标出兵；stronghold 战术 AI/create-creep 与地图生成另拆。9 个既有兼容失败按官方语义/套件仲裁继续保留证据，不为追求百分比改变已确认契约 |
| 独立候选 | Mod 深层玩法开放与客户端收口 | G02.d3 逐房/摘要、G03 完整仲裁、G08→G10 生产动作闭环；G26.b storage provider 依赖 G27.a，独立于 outbox。玩家面 actionLog/prelude/feed 消费、renderer R7 矩阵/长测未完成；用户此前暂缓长测，不因本轮调研自动补跑 |
| 暂缓（用户要求） | 50×6 性能优化与正式收口 | 恢复点为 run 相注入协议/列式共享快照；settle 小片按新证据选择。逐房投影预取两版已证伪撤回。恢复后开发用分钟级短窗和冻结 A/B；最终按[重构计划](performance-rearchitecture.md#测量分层2026-10-11-用户拍板快速开发不为验证等数小时)执行 500 预热 + 3000 采集 ×2 独立窗、等价收尾及真实 bot/混合 Mod 抽查。保留动态合法 Mod 的通用/有序路径；短窗领先不算正式达标。 |

玩家堆宿主/场景配置已交付；服务配置入口随首片接线，cpu_limit/bucket 是否一并暴露在服务配置设计时决定。官方 Memory/segments/ISM 限额保持现行常量。JSON 地形共享与 isolate LRU 已按 50×6 实测降为优化/保险项，不作为服务线前置。

G30.a 覆盖清单与 G30.d 同功能参考随领域滚动；新增写入域当片补齐保存恢复，不等最终收口。

## 任务台账

“部分完成”必须保留父项缺口；“未开始”表示 Mod 能力尚未交付，不否定该领域的默认玩法实现。依赖须在开始任务时核验。

| 编号 | 任务 | 状态 | 已完成与剩余范围 / 入口 |
|---|---|---|---|
| G01 | 移动竞争优先级 | 已完成 | [竞争排序](../architecture/movement.md#竞争排序)；排序能力不等于完整仲裁 |
| G02 | 移动准入 | 部分完成 | .a 准入、.b 障碍追加、.c 请求预检、.d1 静态放行、.d2 双端玩家门、.d3 墙与静态对象/rampart 通行声明已贯通；安全模式主人摘要已接通；剩余逐房粒度及其余摘要增强；[移动契约](../architecture/movement.md) |
| G03 | 完整房间仲裁 | 部分完成 | .a 请求视野/链预演与 pull 登记已开放；障碍槽第二预演已开放；剩余完整仲裁替换；[移动契约](../architecture/movement.md) |
| G04 | 移动代价与效果 | 部分完成 | .a 代价槽已交付；剩余疲劳恢复、重量评分要素；[移动契约](../architecture/movement.md) |
| G05 | 作者 CLI | 已完成 | mod init/test；[本地模拟](../architecture/local-simulation.md#作者包模板)，增强按实际痛点另拆 |
| G06 | trace 与故障复现 | 部分完成 | .a 槽 trace / --mod-trace 已交付；剩余过滤、停用阈值、一键复现；[trace](../architecture/mod-trace.md) |
| G07 | 玩家预检策略 | 部分完成 | .a 战斗射程、.b ext 压制声明已交付；剩余 EXECUTION_ORDER 策略化和参数策略；[预检规则](../architecture/player-precheck-rules.md) |
| G08 | 建筑字段与生命周期 | 部分完成 | .a 字段/tick 已交付；剩余 Mod 建筑 RCL、逐资源容量、专属 FIND、生产事件；[建筑契约](../architecture/mod-buildings.md) |
| G09 | 自有对象能力 | 部分完成 | .a 库存、.b 耐久/死亡、.c 结构受击、.d 声明反击已交付；实际伤害联动反击、owner、维修/掉落等增强按需拆片；[库存](../architecture/object-store.md)、[耐久](../architecture/object-durability.md) |
| G10 | 可编程生产与动作 | 未开始 | 依赖 G08 与资源/配方；具名动作、多配方、进度、冷却和原子支付/产出 |
| G11 | 配额与容量策略 | 部分完成 | .a 内建逐型逐级配额已交付；剩余资源容量、逐房/动态配额；[配额](../architecture/structure-quota.md) |
| G12 | 采集与再生策略 | 未开始 | 依赖实际采集/再生消费者；产量、消耗、耗尽、周期恢复和竞争 |
| G13 | 孵化、寿命与死亡 | 未开始 | 分拆成本/耗时、出生/出口、寿命、死亡产物；含孵化门槛 |
| G14 | 塔动作策略 | 未开始 | 依赖伤害/治疗/受击边界；公式、消耗、距离、改向与事件 |
| G15 | 控制器与成长 | 未开始 | 对象/账户候选边界；升级、占领/预定、控制器周期与 GCL |
| G16 | lab/factory 策略 | 未开始 | 依赖 G10/G11 相关能力；分拆化学/工厂，贯通配方、预检、库存、冷却和事件 |
| G17 | 跨房 Mod 资源 | 未开始 | 依赖 terminal 全局结算与资源目录；费用、两端库存、目标房和跨房原子性 |
| G18 | 市场扩展 | 未开始 | 依赖 G17 与订单/账户边界；余额、余量、费用、记录和游标 |
| G19 | 房间生命周期 | 未开始 | 分拆对象 preTick/房间阶段顺序与资格、受控房间操作；双房失败不发布 |
| G20 | 玩家/世界生命周期 | 未开始 | 依赖对应房间阶段与串行屏障；玩家/世界阶段、服务端命令、账户和跨域访问 |
| G21 | 地图与世界事件 | 未开始 | 依赖 G19/G20、G22.b；初始化/NPC/周期事件，地形/寻路缓存与恢复同源 |
| G22 | 执行身份/时间/RNG | 已完成 | .a 状态清单、.b 确定性时间/RNG、.c 执行身份；[状态清单](../architecture/state-inventory.md)与[检查点](../architecture/checkpoints.md) |
| G23 | 包/schema/state 迁移 | 部分完成 | .a 载荷版本/显式迁移已交付；剩余内容级迁移与 RNG 联动；[检查点](../architecture/checkpoints.md) |
| G24 | tick 边界更新/watch | 部分完成 | .a 无开放轮次的包/API 切换已交付；watch 未开始；[检查点](../architecture/checkpoints.md) |
| G25 | 输入日志与结算回放 | 已完成 | .a 日志协议/保存、.b 回放/完整状态核对；[输入日志](../architecture/input-log.md) |
| G26 | 自动检查点/storage | 部分完成 | .a 周期保存/keep/恢复选择已交付；.b provider 未实现，G27.a 前置已交付；[检查点](../architecture/checkpoints.md) |
| G27 | 后台服务/outbox | 部分完成 | .a 独立异步宿主与有限只读探针已交付，见[后台服务](../architecture/background-services.md)；.b 耐久输入首片（S2.1–S2.3）已实现；后台 JS enqueue/outbox 未实现 |
| G28 | Backend 贡献 | 未开始 | .a M3 最小真实服务；.b 路由/订阅/provider，依赖后台与输入边界 |
| G29 | 玩家类型与客户端 | 部分完成 | .a mod types / --check 已交付；.b 真实客户端、.c 资产/渲染/面板未开始；[类型生成](../architecture/mod-types.md) |
| G30 | 预设/覆盖/性能验收 | 未开始 | .a 覆盖清单、.d 同功能参考先滚动；.b 官方预设、.c 内容所有权迁移、.e 最终收口 |

## 子片依赖与实施边界

下表只列关键子片；已完成的前置仍须按实际源码与契约核验。

| 子片 | 硬依赖 | 交付边界 |
|---|---|---|
| G23 内容迁移 | G22.a/c；涉及 RNG 时加 .b | 替代候选迁移，失败保留原世界/计划；缺包与只读导出明确区分 |
| G24 watch | G23 对应更新能力 | 无开放轮次或待恢复玩家缓存；API/计划/工作者一起切换，失败保留原版 |
| G26.b storage provider | G26.a、G27.a | native / JS provider 等价原子发布与读取；异步 storage 属后台域 |
| G27.b 耐久输入/outbox | G27.a、G25.a、G26.a | 世界、输入消费游标、待输出记录原子耐久；确认后投递 |
| G28.a 最小真实服务 | G27.b | M3 认证/授权、代码上传、订阅、控制台；玩法写入走受控输入 |
| G28.b 业务 provider | G28.a | 可替换路由/订阅/provider；返回结果仍经过宿主授权 |
| G29.b 真实客户端 | G28.a | 实际 API/订阅消费者；JSON 投影不算图形客户端交付 |
| G29.c 客户端贡献 | G29.b | 可替换资源资产、渲染与面板，包选择和类型声明一致 |
| G30.b/c 内容预设/提取 | 对应能力已开放；存量变化依赖 G23 | 公开 SDK 内容；未选包不被 native fallback 恢复 |

G28 的协议设计和只读原型可提前；玩法写入必须先有输入/提交边界。G27.a → G26.b 与 G27.a → G27.b 是两条后续线，outbox 可先用 native 后端，不等待 JS provider。

### 移动与作者工具

- G02.d3 后续须同时接通 PathFinder、Room.findPath/moveTo、请求预检、仲裁默认与链预演；物理地形查询保持原值，不放开未注册房墙环或动态占格。tick 级 allowStatic/服务器 JS 回调不能同步进入玩家寻路。
- G03 完整替换须覆盖交换、循环、拉拽、阻塞传播、整房候选与拒绝回退。默认路径先选胜者再取消，不自动递补；不得静默截断请求或在排序比较器内执行有副作用 JS。
- G04 覆盖受伤后身体效能、疲劳生成/恢复、道路磨损、工地踩踏与跨房顺序；任意传送另设计能力。
- G06 trace 必须有界，注明记录量/成本/私有数据范围；诊断快照不冒充可回放日志。G07 当领域首片即接必要预检，后续再通用化，不形成循环等待。

### 对象、经济与世界

- G08/G09 贯通声明、建造/创建、字段、预检、投影、事件、索引与恢复；第二种对象不新增专用 Rust 变体。新的 body/effect/能力表示由 G30.a 单列，不假定现有表示永久封闭。
- G09 实际伤害反击先定义减伤/治疗前后基数、来源归因、非递归与同轮追加顺序；不能向已完成 tick 的对象挂队列后不消费。
- G10–G18 逐域确认消费者，覆盖满仓/耗尽、完整资源支付、多人竞争、冷却、全局账户及跨房原子性。孵化用真实 spawnCreep 验收，不伪造 spawning；默认 lab/factory/tower 已实现不等于策略开放。
- G19/G20 明确对象动作、仲裁、tick、房间与世界阶段的相对顺序、访问域和串行屏障；生命周期不默认继承动作式局部拒绝。玩家连接器保持沙箱隔离，不只相信 parallelSafe 声明。
- G21 的初始化与运行期更新分开；地形/投影/查询/寻路缓存 epoch/检查点同源，时间和随机按 G22 协议输入。

### 耐久、服务与收口

- G26 明确 RPO、单写者、发布与恢复选择；检查点不能承诺零丢失。G27.b 覆盖发布前、耐久确认后发送前、发送后确认前崩溃，先用模拟接收端验收幂等/去重。
- 后台响应进入后续 tick；不能直接写正在结算的权威世界。玩家上传代码不能按管理员可信 Mod 装载。
- G29 类型签名须有显式描述，不用全 any 替代；未选包不残留声明，生成物新鲜度进门禁。
- G30.a 展开 config/constants/schema/processor/game/driver/main/terrain/storage/backend/test 的真实消费者，以及 power/effects、observer、NPC、portal/跨 shard、intershardResource、flag/visual/messages 等未显式列出的子片。缺口可新增 G31+，不以父编号齐全宣布全量覆盖。
- G30.d 在重大重构前保存同功能参考，测相同规模的原生/JS/组合成本；不能删参考后才补基准。官方内容提取须明确常量/目录所有权、冻结冲突与默认兼容，native 加速保持公开语义等价。

## 已知限制与复审入口

- G09 的 owner、massAttack/dismantle/维修/死亡产物，以及私有 rampart 的预检/结算差异，仍按[库存](../architecture/object-store.md)和[耐久](../architecture/object-durability.md)的限制选型；不把四个首片交付标成全领域完成。
- G02.d3 历史登记的 worker 配置传递、跨房墙环端到端、allowStatic 与声明叠加等增强项，在相关切片开始时复核；安全模式主人摘要为可解释性增强，孵化门槛归 G13。按[移动契约](../architecture/movement.md)与实际测试核验。
- 控制器 INVULNERABILITY 的玩家/原生攻击与核弹消费门已接通；invaderCore/stronghold 已有生命周期与布防首片，战术 AI 和 G21 Mod 世界事件仍未交付，不能把默认 NPC 首片算作世界领域开放。
- upgradeController 零量动作/事件分歧已修复；RCL8 同轮满额仍拒绝，回归见控制器升级边界测试。
- 历史登记过 sim 并行冷恢复子进程偶发失败，隔离复跑通过且后续完整门禁通过；若再现须保留失败报告归因，不能把重跑成功当作已修根因。
- Overmind bot-check 保留基线已有的 1 个 console-error tick；不宣称零错误。keeper/NPC、多 shard 和其余能力门控缺口以最新兼容报告核对，旧批次数字不参与当前决策。
- 本地检查点的文件大小、单写者及耐久适用范围见[检查点契约](../architecture/checkpoints.md)；存储和真实网络服务不能从 embed 能力直接推导为已完成。

## 最近交付与实际验证

| 已提交切片 | 实际验证与证据 |
|---|---|
| S1.1 服务目录基座（本次提交） | kk-storage 6+runtime service 13 项新测试、原 checkpoint/sim 回归（sim_tests 106）、clippy/doc-link 干净；3 路 review（A 行为/B 持久化/C 边界）修复 4 项 P1 与 7 项 P2 后全绿，详见下方实施记录 |
| `ca347777` commit 转义门提升 | commit 三对 ABBA 同向降低 0.6–1.4ms；kk-world、300 房终态等价、棘轮零转红及完整 check；[证据](../../compat/reports/mature-comparison/2026-10-12/50x6-ugate-hoist/README.md) |
| `fc5d602c` 校验融合 | validate 10.41–12.22→3.01–3.20ms；kk-engine/kk-runtime、300 房终态等价、棘轮零转红及完整 check；热污染窗口不判 wall；[证据](../../compat/reports/mature-comparison/2026-10-12/50x6-validate-fusion/README.md) |
| `df170906` max ID 末键读取 | prepare 三对降低 1.5–2.7ms；世界/引擎、终态等价、棘轮与完整 check；[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-maxid-next-back/README.md) |
| `f0036d82` 克隆-结算流水 | settle 相邻对确认降低约 6ms；并行故障/确定性回归、终态等价、棘轮与完整 check；[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-clone-pipeline/README.md) |

这些结果属于对应提交，不是本次文档整理新执行的检查，也不能把分段收益直接相加成整拍加速。完整沿革与此前 bot 验证见[历史归档](archive/mod-history-2026-10.md)。

### 2026-10-12 文档优化实施记录

- 基线 `ca347777`，保留并归档上一轮调研记录；修订并行/KKPV/渲染器现状、实际 crate 依赖、检查点 v9/迁移与 v2 恢复语义、路线图和两窗验收说明。完成过程移入既有归档；台账从 HEAD 的 557 行/约 206KB 收拢到约 150 行/21KB，G01–G30 状态不变。
- 复审核对：迁移的接手沿革、10-10 调研/power、旧验证与10-09以后实施正文完整保留，仅调整相对链接；此前 Mod 归档与路线图历史正文未改写。默认玩法、Mod/服务、R7 与正式性能验收的剩余消费者继续按当前队列推进。
- 整理阶段验证：`node scripts/check-doc-links.mjs` 通过（331 个 Markdown、2216 个本地链接），`git diff --check` 通过；历史原文保留与父项状态比较通过。仅 Markdown 改动，无 Rust/JS/SDK/保存格式变更；提交前完整门禁见下一节，未重跑完整 screeps-ok、bot 或性能长测。
- 剩余工具差异：`compare.py` 默认五次及 `formal_window` 旧判据未改，README 已说明显式两窗命令与标记的区别；未来工具调整须另验证。本轮19份文档随本次提交交付，未新增交付文档或修改机器报告。

### 2026-10-12 主线调整与提交

- 用户要求提交文档整理，并暂缓性能、转向功能开发；已保留性能恢复点与未完成验收，不继续性能实验或长测。
- 功能建议优先常驻运行/无日志检查点恢复 → G27.b 耐久输入/outbox → 最小联网与房间客户端；若优先玩法扩展，则从 G08→G10 自定义建筑生产闭环拆首片。本轮仅建议与队列调整，未实现或标记任何功能完成。
- 提交前无参数 `pnpm run check` 完整门禁通过：Rust 格式/Clippy/workspace 测试、重建测试 CLI、门禁/维护/性能工具回归、JS workspace、331 个 Markdown/2216 个本地链接及 998 个源码/测试文件规模检查均通过。renderer 376、shared 4、compat 适配器 23 个测试通过；这不是完整 screeps-ok、bot 或性能验收。Clippy 保留现有 4 个未使用 import 警告，源码规模有 50 个超过 700 行的复审提示，均未阻挡门禁。

### 2026-10-12 服务三批设计记录

- 用户选择此前推荐的 S1 常驻/恢复、S2 耐久命令/outbox、S3 登录/房间客户端，要求深度设计。基线 `99cb63c6`，仅新增目标方案与拟议 ADR 并同步索引/范围；父项交付状态不变。
- 核对发现：sim 目录恢复要求日志；管理 world_snapshot 不裁切玩家视野；Sandbox.set_modules 失败不提供回滚；RoomFeed 尚非冻结网络协议且不接受同 tick delta。方案分别设计独立服务恢复、授权投影、替代工作者与网络薄适配。
- 核心取舍：周期游戏检查点 + 低频命令/副作用耐久屏障 + 独立恢复下限/投递确认；received 与 completed 分开，去重水位有界保留，后台不直接写世界。拆为 11 子片，首片建议 S1.1；代码/API/SDK/服务格式尚未改动。
- 实际验证：文档链接检查通过（333 个 Markdown、2243 个本地链接），`git diff --check` 通过；G01–G30 全部父项状态与设计基线 HEAD 一致，新增正文标题/代码围栏及 11 子片分配核对通过。复审补齐服务状态归属、入口模块保存、去重明细过期、v2 初次启用、代码激活通知、outbox 满额停顿与重试/去重期限语义。仅文档变更；本次提交前无参数 `pnpm run check` 完整门禁通过，包含 Rust 格式、Clippy、workspace 测试、CLI 重建、维护工具及 JS workspace 测试、文档链接和源码规模检查；不代表 S1–S3 已实现或已执行其未来验收。设计随本次提交交付，未启动性能或 renderer 长测。


### 2026-10-12 S1.1 服务目录基座实施记录

- 交付内容：发布原语从 kk-runtime/checkpoint 提取为 `kk-storage`（信封读取与 Broken/Fatal 错误分类、原子发布含阶段故障注入、目录单写者租约、临时文件清点）；`kk-runtime::service` 新增服务目录布局/校验（`dir`）、目录身份 identity.json v1（初始化完成标记 + world_id/种子/内容身份）、检查点内 `metadata.service` v1（含每玩家 entry）、`cp-<publishSeq>.json` 命名/序号分配（回滚不复用）、无日志恢复选择（撕裂/哈希损坏跳过计数，I/O/未来格式/身份不一致停止）、按完整版本计数的保留策略（保护集不占名额）。公共 `read_checkpoint` API 与 sim 行为不变。
- 实际验证：`cargo test -p kk-storage`（7 通过 + 1 子进程用例：争锁立即拒绝、硬退出释放、发布中断分类、临时文件匹配）；`cargo test -p kk-runtime --lib`（65 通过，含服务域 12 项：序号不复用、坏最新份跳过、四类身份门 + 引擎载荷 tick/种子交叉核、完整版本保留与恢复基线保护、真实引擎发布-选择-恢复往返）；`cargo test -p kk-cli --test sim_tests`（106 通过）；clippy 两 crate 干净；`node scripts/check-doc-links.mjs` 通过。提交前完整门禁见本次提交说明。
- Review 闭环（3 路 subagent，A 行为正确性/B 持久化与崩溃恢复/C 边界与测试）：确认成立并已修复——①信封层“合法 JSON 但结构不符”误归可跳过（按 serde 错误类别拆分 Syntax/Eof→跳过、Data→停止）；②`list_seqs` 吞目录迭代错误（改为传播，防止序号复用覆盖/静默降级）；③保留策略按文件名而非完整版本计数、降级恢复基线可被自己删除（改为完整版本窗口 + 窗口下界外才删）；④台账滞后（本条）。P2 修复：scenario_sha256 身份门与引擎载荷种子交叉核、hex 字符集校验、u64 序号上限、`max_bytes+1` 饱和、float_roundtrip 特性对齐、半初始化目录处置提示、init 容忍协议临时残留、临时名空段收紧、AGENTS.md 模块树同步。
- 复核闭环：A/B 两路 reviewer 对修复后最终改动逐项复核 PASS（A 侧另实证九种信封形态分类无漏网；唯一残留形态“完整信封+尾部垃圾归 Broken”仅外部篡改可造、已注释说明不阻塞）。C 路初审提出的台账更新（本条）与 AGENTS.md 模块树同步亦已落地。
- 平台证据：macOS XNU 的 flock“快速加锁/释放循环 × 并发 fork”会留无持有者残锁（约 250ms 自愈、约 4% 循环命中；两线程探针实证并记录在 lease.rs 注释）——生产形态每进程单次取锁不受影响，测试经串行化规避；Linux 侧未实证。
- 挂账到 S1.2：宿主在租约持有期内收编 publish/select/enforce_keep/remove_stale_temporaries 调用并清理残留临时文件；init 组合函数（先初始检查点耐久后写 identity）与 init 各阶段崩溃矩阵测试；发布“耐久未知”错误的可判别形态（PersistenceFault 映射）。


### 2026-10-12 S1.2 常驻宿主实施记录

- 交付内容：`kk-runtime::service::{config, host}`——严格版本化服务配置 v1（未知字段/未来版本/越界拒绝，相对路径相对配置文件解析）；宿主状态机（Running/Draining/Stopped + Paused{Settle,Execution}/Persistence 故障冻结推进）、纯函数调度器 `next_deadline`（网格锚+间隔锁定快速拍零漂移、超时延后一个间隔、不补拍不追积压；驱动两点接线 anchor/pending——等待中截止点命令打断保持不滑动、消费后记网格锚，组合级测试三件套锁定）、周期保存（tick/墙钟先到 + 脏拍门，序号按目录实况分配不缓存）、发布失败的目标文件实证判别（耐久未知 vs 旧版未受影响）、停止流程（Draining→无失败轮按需收尾保存→关引擎；失败轮不保存不丢弃；exit_wait 看门狗强制退出如实报告）；`EmbedEngine::pending_round_kind()` 区分结算失败/执行故障。`kk-cli::serve::{init,run,control}`——init 组合（目录校验→骨架→租约→清理残留→身份值→场景装载→初始检查点耐久→identity 最后写）、run 装配（validate_for_run→identity→租约覆盖全程→清理残留→恢复选择→堆一致性→替代引擎恢复→宿主）、逐行 JSON stdio 控制（status/retry/stop）、SIGTERM/SIGINT 原子标志、退出码 0/1/2。CLI 集成测试 5 项（真实子进程）：init→SIGTERM 保存退出 0→二次运行恢复续存（控制通道查询/停止）、半初始化目录 run/init 双拒、运行中目录第二进程租约被拒、只读检查点目录的持久化故障 SIGTERM 退出 2（权限恢复后收尾保存成功仍 2）、控制通道非法行/无故障 retry 得 ok:false 且继续推进。共享 sim 场景/Mod 装载（scene/mods 升 pub(crate)，行为不变）。
- 实际验证：`cargo test -p kk-runtime --lib service`（24 项：调度网格与驱动组合周期/命令打断不滑动/高频命令流不饿死/玩家错误继续/结算失败重试不重跑（Memory 单次执行证明）/执行故障重建重试/保存判定/保留推进/持久化冻结与实证/停止三态/init 校验临时残留清理复核）；`cargo test -p kk-cli --test serve_tests`（5 项子进程链路）；sim_tests 106 项回归通过（场景共享无行为变化）；完整 `pnpm run check` 通过；新架构文档 docs/architecture/service-host.md（现行契约）+ overall/README 索引同步。
- Review 闭环（3 路 subagent，A 行为正确性/B 持久化与崩溃恢复/C 边界与测试，修复后经提出方复核）：P1 调度缺陷两级——初审网格漂移（A/C 双实测：40ms 配置实测 47.83ms/34.6ms，原 target>now 分支在拍路径永不可达，每拍以拍后时刻为锚累积执行耗时）修为 previous+interval 网格后，A 复核又抓到命令打断滑动：等待前写入 target 使未消费截止点被当作已消费，每条窗口内命令把拍推后一个整间隔（A 实测 50ms 轮询 2s 仅 26 拍、20ms 命令流 0 拍），且首拍前无锚点时命令流可无限推迟首拍（我方第一版修复的新测试抓出）；终态为 anchor/pending 两点接线（pending 命令打断保持+首拍锚定、anchor 网格锚消费后记录），A 以同构虚拟时间模拟实证 20ms 命令流/40ms 间隔 2s 内 50 拍恒周期、命令处理越过网格点一次性损失一个间隔无追赶突发。P2 修复：最终 status 的 live_tick 停止冻结（teardown 后不归零，仅首次冻结）；持久化故障 stderr 同因只打印一次；退出码 2 语义补齐"曾在持久化故障下停止（即使收尾保存成功）"并修复驱动循环顶部停止码不按故障归类的竞态（权限恢复后暴露，集成测试锁定）；retry 增加 Running 阶段门；Paused 无缓存轮形态单列 Infrastructure（重试直接重进拍入口，run_players 内部 rebuild_mod_realm；C 核实此形态玩家未执行、世界为上一已提交拍）；watchdog 到点后 250ms 二次确认；控制通道非法输入/无故障 retry 的 ok:false 应答与继续推进入集成测试；serve_tests 全部子进程加 kill-on-drop 守卫、chmod 恢复提前；init 校验放行仅含协议临时残留的 checkpoints 并在清理后复核非空即拒（原清理为死代码）；next_deadline 兜底臂溢出处理与网格臂一致；AGENTS.md kk-cli 行补 serve init/run；service-host.md 补 mods.root 与场景模块的相对基准差异。
- P3 挂账（后续切片处理）：Settle 重试中写回构造失败转 Execution 重跑的边角归类；watchdog 不覆盖在途拍（由 exit_wait 窗容纳，文档已注明）；status 的 next_publish_seq 为诊断值可能过期；probe_checkpoint 假设无外部干预（注释已说明）；已提交后输入日志 IO 失败会被归为 Infrastructure 且脏拍少计 1（serve 未启用 input_log 当前不可达，S2 接线 CommittedLogError 时必须先处理）；stdio 控制行无长度上限（S3 网络入口一并设界）；stdout 管道断裂时 println panic（进程由调用方管理，S3 前无网络面）；`engine()` 访问器仅限装配后只读诊断的约束已注明；tick_interval_ms 下限未单设（25ms 轮询粒度为实际分辨率）。
- S1.1 挂账三项已收编：宿主 init/run 在租约持有期内收编全部存储调用并清理协议残留临时文件；init 组合函数固化“初始检查点耐久→identity 最后写”顺序；发布失败以目标文件实证消息提供耐久未知判别（PersistenceFault）。
- 遗留到后续切片：S1.3 将补两玩家/多房/Mod 场景与 JSON/KKPV 双通道冷恢复的完整状态等值验收（本批单玩家链路已覆盖机制正确性）；状态摘要含 `world_id/run_epoch/live_tick/durable_tick/publish_seq/dirty_ticks/last_save_ms/last_error/recovery_selected/skipped_broken`（方案字段齐全，snake_case）。

### 2026-10-12 S1.3 连续运行与冷恢复验收实施记录

- 交付内容：`crates/kk-cli/tests/serve_tests/recovery.rs` 5 项真实子进程+进程内核对用例（4 项链路验收+1 项负向拒绝）（夹具对齐 compat/fixtures/sim-mod-objects 形态：两玩家 p1/p2、W1N1/W2N2 各含 nursery:emitter、period 5/life 3、bot 每拍 Memory.loops 计数并落 lastTick）。①主链路（KKPV）：init→常驻 run→SIGTERM 保存退出 0→测试进程内经 service::checkpoints::select_latest+EmbedEngine::restore_checkpoint 复原最新检查点，核对世界时间=检查点 tick、模块入口 entries（两玩家 main）、两玩家 Memory（loops 计数与 lastTick 落点）、nursery born 计数、两房 Mod 对象、空请求继续结算一拍（CPU/写回活性）→同目录再 run 续存（live_tick 越过恢复点 +10 后控制停止）→再核对 loops 双玩家增长、born 单调、tick 不回退。②JSON 通道：同场景同核对面（业务状态双通道等值）。③撕裂回退：截断最新份→run 恢复选择跳过它回退次新完整版（skipped_broken=1 如实报告，live_tick 自次新起步继续）。④同步失败不发布水位：只读目录注入 Persistence 故障→冻结期间与停止后目录名字集合完全不变、退出 2。
- 关键约束锁定：serve config 必须声明与场景一致的 mods（root/configs），否则 run 恢复因检查点 ModIdentity 无装载输入被拒（exit 1）——负向用例 mod_checkpoint_run_without_mods_config_is_refused 钉住。
- 实际验证：`cargo test -p kk-cli --test serve_tests` 10 项全过（S1.2 5 项回归+S1.3 5 项）；完整 `pnpm run check` 通过。S1.3 无产品代码改动（纯验收测试），架构契约无需变更。
- Review 闭环（2 路 subagent，A 验收充分性/B 持久化恢复语义）：初审 A FAIL 4 项 P2、B PASS 附 1 项 P2，修复清单——①撕裂用例把发布序号当 tick 用的恒真谓词（A/B 一致抓出）：改为进程内 select_latest 精确断言次新 publish_seq+可完整复原（verify_recovered 参数化 skipped 期望），子进程侧断言 recovery_selected==次新文件名、live_tick 以次新 tick 为基线（同单位）增长；②JSON 通道补跨进程续存段（goal 要求双通道均覆盖连跑与续跑；run2 后再复原断言 loops 增长，证明 JSON 注入路径工作者重建与玩家真实执行）；③CPU 核对面由"空请求结算活性"升级为直接读数（两玩家 CPU 桶 validate_checkpoint+limit>0，与引擎恢复校验同口径）；④台账"由此用例锁定"的虚称（负向路径当时未执行）——补 10 行负向用例真正锁定。P3 顺手修：主链路 run2 断言恢复选择指向最新完整份文件名；Mod 对象核对升级为按房分布（两房各含 nursery:emitter，pos.roomName+typeKey）；截断形态注释更正（发布协议不产生终名截断，此为读取侧 Broken 分类的故障注入）；故障用例权限恢复加 Drop 守卫防早退泄漏。B 另实证：前缀截断确定性落入 Eof/Syntax→Broken 跳过分支、sha 失配分支已由 S1.1 单测覆盖不重复声明、名字集合不变是"未发布水位"的可靠代理（发布恒为新名 create_new+rename，序号实时分配不撞旧名）、restore_in_process 与 run.rs setup 同入口同语义。A/B 终态复核均 PASS（A 逐项核验断言 API 与区分力并独立复跑；B 确认撕裂分支确定性、水位代理可靠与同入口语义）；残留 3 项不阻塞 P3 已随手清偿：负向用例 stderr 断言收紧为"Mod 包缺失"特异子串、仓库根手工运行残留 run.err/run.out 清理、台账用例计数措辞对齐（5 项）；CPU 数值级往返等值由 kk-runtime 检查点单测分层承担。

### 2026-10-12 S2.1 耐久命令面（接收/屏障/恢复下限）实施记录

- 交付内容：**接收面**——`kk-runtime::service::inbox`（inbox.json v1 单一原子最新状态：CommandEnvelope 校验、幂等键 (principal,streamId,clientSeq) 同载荷返回原状态/异载荷冲突/淘汰后结果过期不重执行、clientSeq 跳号拒绝、单主体 ≤4 活跃流、预算 256 条/单主体跨流合计 8 条/16MiB 拒绝准入不驱逐、close 拒绝在途、退役留墓碑不复用、received 仅在文件+父目录同步后成立、损坏停服不回退）。**恢复下限**——`service::effects`（effects.json v1 锚点：屏障检查点 seq/barrier_id/inbox 消费水位 + sink 水位字段 S2.4 填值）；`RecoveryFloor` 接入 select_latest（v2 目录 v1 旧份跳过计 skipped_ineligible；只允许锚点本身或同屏障、水位不降的后继；无满足者停服）；host 保留策略恒保护锚点、普通保存经 `V2SaveContext` 继承屏障与水位（publish_with_meta）。**service v2**——metadata 追加 barrier_id/inbox_applied/code_versions/outbox_cursor（Option 平滑解码 v1 形状，v2 候选须齐备）；identity 支持双版本。**启用事务**——`kkscreeps serve enable-inbox`：租约内 恢复 v1 最新份→发布初始 v2 屏障检查点（世界不变）→空 inbox→effects 锚点→identity 最后切 v2；中断重跑续接（沿用屏障检查点、补齐缺失文件、inbox 保留不重置）；v2 目录 run 要求 inbox/effects 齐备否则拒启。
- 验收对照（方案 §4.1/子片表）：丢响应重试=同载荷 Duplicate 返回原 serverSeq/状态；重复异载荷=Conflict 不影响原命令；序号跳跃=Refused 且补齐后可续；明细过期=ResultExpired 不重执行；恢复下限不降级=floor 门单测（异屏障/低水位/仅剩 v1 份全部不可选，停服错误）；恢复后 inbox 不丢 received=跨 run 集成用例；崩溃矩阵第 1 行=inbox 经 kk-storage 原子发布（tmp 整写+rename+双 fsync，received 仅在同步后返回）+撕裂停服单测；第 2 行=received 后消费前重启仍在（消费面 S2.2 起才有，水位恒 0 不提前确认）。
- 实际验证（终态）：`cargo test -p kk-runtime --lib service` 39 项（inbox 12+effects 1+floor 门 2+伪 v2 形状 1+原 23）；`cargo test -p kk-cli --test serve_tests` 16 项（S2.1 新增 6：enable→v2 run 往返含屏障继承/齐全态中断续接含 received 保留/真实中断态（side 文件缺失）续接/缺 inbox|effects 拒启/锚点 keep 保护+停服/received 跨 run 不丢）；完整无参数 `pnpm run check`（含 Clippy）通过——初审时点的"通过"声明曾与 Clippy 实况不符，已在 Review 闭环节如实记录。文档：service-host.md 新增"耐久命令面"节、state-inventory.md 新增服务域耐久状态节。
- 范围声明：S2.1 只交付接收与恢复下限；命令消费/completed/rejected 业务终态、屏障推进事务（命令完成建立新屏障）、outbox 投递、HTTP 入口分别属 S2.2/S2.3/S2.4/S3。接收入口为进程内库 API，宿主驱动接线随消费面切片。
- Review 闭环（3 路 subagent，A 行为幂等/B 持久化崩溃恢复/C 边界测试契约）：A FAIL（P1=交付态 Clippy 拦截而下文声称完整 check 通过——虚报属实：修复轮只跑了 fmt+定向测试未重跑完整门禁；P2=关闭流的幂等重放被拒（closed 检查先于明细判别，丢响应结果不可找回）与 Duplicate 应答把 Rejected 坍塌为 Null 丢 reason；另 8 项 P3）；B PASS 附 P2（enable 续接扫描无身份门，外来 v2 形状文件被当续接证据——实测复现 exit 1 但目录锁死）与 8 项 P3；C FAIL（P1=同一 Clippy 虚报；P2=floor 的 v1 旧份跳过分支在全部测试中零执行而集成断言声称"由单测锁定"（又一例虚称——实际单测只覆盖异屏障 v2 形态）与 enable 把损坏当缺失静默重建违反自身停服契约）。修复清单：Clippy 三类（borrow/十参 allow/collapsible_if）+完整 check 真实重跑通过；幂等判别移到 closed 之前（关闭后旧身份可重放找回结果、新接收仍拒）；Duplicate 携带原终态 EntryStatus（含 rejected reason）；schemaVersion 超 u32 域拒绝；预算改主体级跨流合计 8 条（对齐方案"单玩家 8 条"，跨流用例锁定）；幂等摘要改命令规范形 command_hash（kind+schemaVersion+payload，异命令同载荷=Conflict）；floor v1 旧份分支补真执行用例（仅 v1 份+floor→恢复下限停服）；enable 扫描补身份门（world_id∧V2∧seq 自洽，外来文件停服）；inbox/effects 缺失与损坏区分（存在但非法=人工核验停服，不再静默重建）；effects 保留判据 seq+barrier 双比对；补真实中断形态集成用例（屏障在/inbox+effects 缺/identity v1→续接补齐后可运行）；重启重放取回原 serverSeq/同流跨主体拒绝/关闭流重放/异 kind 冲突四项单测；inbox 并发模型（租约内宿主单线程）与 clone-probe 成本注释；run.rs 与 enable.rs 身份读取均移入租约内；evict 前置契约（floor≤next_client_seq）注释；INBOX_SPEC 64MiB 转义余量注释；删 recovery.rs root() 死代码。B 终态确认另抓出 4 项修复脚本断言失败被误报成功未落盘（enable 头注释/effects 双比对/损坏区分/租约序）+host.rs doc 删错段——逐项重落盘并以写后回读验证，教训入记忆（见下）。终态复核：A/B/C 三路全部 PASS（A 独立复跑完整门禁 exit 0 并逐项核验幂等/预算/命令摘要/四项验收缺口补齐；B 实测扫描身份门拒外来文件、确认四项重落盘修复；C 端到端复跑完整门禁并确认 floor 真执行与损坏区分）。C 残留两处 P3 已顺手清偿：契约文档与台账的预算措辞同步为"单主体跨流合计 8 条"、enable 手写检查点文件名改用 checkpoint_file_name。不阻塞备忘四项：转义膨胀边界（已注释，S2.2 按实测收紧）、evict 前置契约仅注释无运行时断言（S2.2 接线时遵守）、完整门禁运行期间勿并发 cargo（sim executable_sha256 门会拦在途对，A 首跑 6 例失败即此因、隔离复跑 106/106）、retired 流重放 Refused 与 closed 语义区别（符合 §4.1 销毁语义）。终态验证：`cargo test -p kk-runtime --lib service` 39 项、`cargo test -p kk-cli --test serve_tests` 16 项、完整无参数 `pnpm run check`（含 Clippy）通过。

### 2026-10-12 S2.2 代码边界事务实施记录

- 交付内容：**命令消费面**——`kk-runtime::service::host::commands`：`drain_command_prefix(max)`（serve run 运行态拍间调用，按接收序 server_seq 全局 FIFO 消费最老未终态前缀，每轮固定批量 DRAIN_BATCH=4 防垄断）；每笔命令同步应用并立即持久化终态——completed 携带结果 JSON / rejected 携带原因；验证拒绝同样持久化终态（防队首毒消息阻塞），**rejected 不建屏障**（屏障只锚定 completed 效果的外部确认，拒绝的消费水位由 inbox 文件 applied_seq 表达）；未知 kind 一律 rejected；命令面存储/基础设施错误（inbox 读写/模块切换/屏障发布）统一 freeze_persistence 冻结推进（reason 如实记来源），不与拍推进交错重试。**code.replace 事务**——载荷 `{modules, expectedCodeVersion, entry?}`：CAS（expected≠当前即拒、过期不静默覆盖保旧版现役）→ 校验（非空模块表/字符串源码/总字节 ≤2MiB MODULE_BUDGET_BYTES/入口存在且为 JS，失败全拒不动现役）→ 切换工作者模块表（Memory/segments/CPU 不动、不推进世界时间）→ 宿主拍执行入口同步派生（entries+codes 双写，入口变更即切换 require 目标——漏写 codes 会tick 用旧入口）→ 发布**即时屏障**检查点（新屏障身份 b-<32hex>、codeVersion+1、消费水位）→ effects 锚点推进（沿用 sinks）→ 保存上下文 V2SaveContext 更新 + enforce_keep 保护新锚点。模块顶层在下一正常玩家执行阶段运行。**崩溃窗口两段确认**——屏障发布后 inbox 存档前中断：恢复落新屏障链（meta 水位已含该命令），drain 幂等补全（code.replace 结果=当前 codeVersion 由版本号重构，不重新执行）；effects 未更新则新屏障候选被恢复下限过滤、世界回旧链重新消费——无死锁。**驱动接线**——serve run drive 循环 Running 分支开头消费，每轮至多批量 4 条且消费后不回循环顶重入（防突发命令级联清空 inbox 拖死首拍）；drain 数据面 Err 均伴随 fault（未冻结错误兜底 freeze），下一轮 drive 持久化分支统一提示。
- 验收对照（方案 §4.2/子片表）：替代工作者（旧 Memory 不初始化覆盖、新代码下拍生效）=主链路单测+集成用例（m=n+102 断言 Memory 延续与新代码双证）；CAS 过期拒绝不阻塞=毒消息用例（第二条正确版本照常完成）；即时屏障=effects 锚点换新屏障+后续普通保存继承（select_latest floor 断言 code_versions 与 publish_seq+1）；崩溃窗口=幂等补全单测（终态回退 Received+水位归零后重 drain 补全 Completed 而非 CAS 拒绝——证明不重新执行）+发布失败冻结单测（终态保持 Received、fault=Persistence）；集成链路=两条命令（成功+CAS 拒）注入→serve run 消费→SIGTERM→inbox 终态/applied_seq=2/屏障推进（latest meta inbox_applied=1——rejected 不建屏障的正确形态）/恢复后新代码执行（Memory.m≥1000）。
- 范围声明：S2.2 交付 code.replace 一种命令的完整事务；玩家表达式与受限 Mod 输入（S2.3）、淘汰/evict 接线（消费推进时按 floor≤next_client_seq 契约接）、outbox（S2.4）、模块预算服务配置化与 HTTP 入口（S3）不在本批。进程内接收 API 的宿主接线仍待 S3 网络入口（测试经文件注入验收消费面）。
- 实际验证：`cargo test -p kk-runtime --lib service` 45 项（commands 6 项：主链路即时屏障/CAS 不阻塞/非法载荷保现役含畸形 expectedCodeVersion/未知主体拒绝/幂等补全/发布失败冻结）；`cargo test -p kk-cli --test serve_tests` 19 项（S2.2 新增 3 项：主链路集成 + 两条链路级崩溃窗口——矩阵 2 幂等补全真恢复（回退 inbox 终态→重启→补全不建新屏障）/矩阵 3 孤儿屏障（effects 回拨 enable 锚点→重启→floor 身份门过滤孤儿链→回旧链重新消费收敛）；进程内恢复经 recovery::mod_loadout 提取的共享装载辅助）；完整无参数 `pnpm run check`（含 Clippy）通过——过程中 Clippy 曾拦下 run.rs single_match 与测试辅助 unnecessary_cast 两处（首两轮 tail 管道退出码不可信已废弃，改 && 链真实传递）。文档：service-host.md 耐久命令面节升 S2.2 + 页首/边界节矛盾修正；state-inventory.md 服务域节与表格同步。
- Review 闭环（3 路 subagent，A 行为正确性/B 持久化崩溃恢复/C 边界测试契约）：初审 A/C FAIL、B FAIL，三路一致抓出 **P1=畸形 expectedCodeVersion（缺失/非 u64）走裸 Err 而非 Rejected 终态**——drain 该 Err 不设 fault，drive 形成无故障态活锁（A/B 真实子进程实测：tick 永久饿死、命令永无终态、SIGTERM 退出 0 掩盖、重启复现；单测的非法载荷恰好全带合法字段零覆盖）。修复：载荷校验改 reject 闭包（缺字段/字符串形态即时入用例锁定；负数形态在复核后补入——初审记录曾虚称三种齐备）+ drain 对未冻结 Err 兜底 freeze（Err 必伴随 fault 的契约由代码保证）+ drain 前置 fault 检查。B 对崩溃矩阵六窗口全部以真实二进制注入实证（received 前存续/幂等补全真恢复/孤儿屏障回旧链收敛/拒绝水位分离无误判/发布失败冻结重启收敛/冻结停止退出 2）——实现全对，缺链路级固化；已补矩阵 2/3 两条 serve_tests 用例。其余修复：A-P2 未知主体 code.replace 从整服冻结改为明确拒绝（授权类不升级持久化故障，入用例）；A/B-P2 publish_barrier 的 effects 损坏被 unwrap_or_default 吞掉违反自身停服契约——改 Err 上抛（夹具补初始 effects，满足"effects 在首次屏障前存在"不变量）；A-P2 命令冻结后收尾保存的 engine/meta 分歧检查点——评审 B 实证重启收敛（floor 同屏障同水位放行→CAS 随 meta 回滚→drain 先于 tick 重新消费），取契约如实修正路线（注释+边界节记录已知形态，不改行为——回滚模块表需新导出 API 且引入回滚自身故障模式）；B-P3 drain 后 continue 级联清空使 DRAIN_BATCH 防垄断失效——改不回循环顶；B-P3 幂等补全结果形状不对称（原生带 note）——结果统一 `{"codeVersion":N}`；A-P3 幂等补全门对 S2.3 不可推导 kind 的降级风险——代码注释登记约束；A/C-P3 host.rs derive_player_code 注释错位（tick 的 doc 挂错）修正；A/B/C-P3 stream_principal 每命令整读一次 inbox 冗余——drain 已加载状态直传 principal；A/C-P3 集成注入循环未断言 accept 结局——补 match 断言；B-P3 test.ping 用例意图偏移——断言升级为现行消费语义（unknown kind 明确拒绝+水位推进+明细保留）；C-P2 service-host.md 页首仍标 S1.2/边界节写"S2.2 未交付"与新节自相矛盾——页首升 S2.2 现行、边界节重写；C-P3 state-inventory 表格 identity/metadata 行仍只写 v1——同步 v2 口径；C-P3 幂等单测模拟方式——注释说明与真实恢复装配的数据同源等价。B 确认无问题项：水位 map 单调、孤儿屏障永不复出（随机 barrier_id+身份门）、序号不复用、enforce_keep 顺序（anchor_seq 更新先于调用）、run.rs 恢复时锚点保护两视图不脱节。C 确认无问题项：模块边界（kk-runtime→kk-runner 既有依赖）、单一所有者（drain 调用点必无开放轮）、行数全部门禁内（commands.rs 674/host.rs 483）、两套检查点选择入口的契约区分清晰、预算两层正交。终态复核：A/B/C 三路全部 PASS——A 确认活锁路径结构不可达（双防线+兜底无双重包装+P2-3 处置接受）并独立复跑 45/19；B 以真实二进制复验畸形载荷活锁已灭（tick 满速推进 25→98、Rejected 终态、applied_seq=1、退出 0、锚点不动）、确认两条链路用例忠实复刻其实验 B/C 形态（锚点双不变⟺无新屏障、effects 原副本逐字写回）、effects 上抛后崩溃语义收敛且已登记、freeze 组合无双重包装；C 确认文档三处逐句成立、45/19 可复算、行数（commands.rs 672/host.rs 481）与 fmt 门禁通过、新用例无恒真谓词。复核残留 P3 已全部清偿：负数载荷补入 invalid_payload（6 形态）；交付描述"有处理即 continue"与终态代码的矛盾修正；夹具注释失实修正（enable 实际顺序为屏障→inbox→effects，真实不变量是"effects 在首次屏障前存在"）；set_player_modules 基础设施 Err→freeze 映射无直接注入用例（publish_failure 只覆盖发布路径，未知主体拒绝已由前置校验拦截）落账为不阻塞备忘；service-host.md "Err 必伴随 fault"限定为数据面（phase 前置守卫为纯防御断言，drive 侧不可达）；test.ping 用例 doc 行更新为现行消费语义；孤儿用例 keep 窗口时序假设显式注释（保存密度增大时旧链份被清理则正确停服、用例响亮失败）。矩阵 5"发布失败冻结+收尾分歧检查点→重启收敛"的链路级固化与幂等补全门 S2.3 不可推导约束的运行时防御挂 S2.3 顺手处理。终态验证：`cargo test -p kk-runtime --lib service` 45 项、`cargo test -p kk-cli --test serve_tests` 19 项、完整无参数 `pnpm run check`（含 Clippy）通过。
### 2026-10-12 S2.3 玩家表达式与受限 Mod 输入实施记录

- 范围：console.expr 随玩家 RunJob 在 loop 前求值，共用预算；软异常继续、硬中断跳过主代码；8KiB 源码与 64KiB 结果预算。mod.input 绑定当前冻结对象意图，以配置授予玩家身份注入，消费时核验活动包/目录/schema/目标/归属，结算再次核验；玩家同对象同意图优先。首片全局 FIFO 每拍至多一条拍内命令，不宣称后台 JS enqueue 或 S2.4 outbox 已交付。
- 候选/耐久：Settle 候选固定表达式结果与宿主输入，重试不重新求值或换命令。提交后世界、水位和独立 `metadata.command_results` v1 结果窗口同份发布，effects 确认后写 inbox 终态；屏障已确认但终态未存档时恢复原结果，不重复副作用。屏障失败冻结并禁止普通/收尾保存已变化世界与旧水位。窗口在后继屏障清理已存档结果；旧 S2.2 缺树显式迁移为空，存在但未来/畸形/超预算/超消费水位拒绝，已确认表达式缺结果冻结。
- 配置：config_format_version 升至 2；v1 无新字段时显式迁移为空授予表，v1 携带 mod_grants 拒绝。引擎/目录/inbox/effects 版本不变，新增结果树有独立版本门。宿主意图加入审计日志最终扩展意图，包含无 RunJob 玩家，原玩家原始 wire 的其他部分保留，回放不执行 JS。
- 接手 Review 修复：P1 硬中断解除看门狗后仍执行主代码；P1 已耐久表达式在存档缺口重复副作用；P1 屏障失败后停止保存能发布世界与旧水位；P2 宿主输入未固定进失败候选且审计日志遗漏；P2 mod.input 未核验当前计划/目标/归属，原 accepted 夹具使用不存在的意图；P2 新配置字段未升版。均已修复并增加回归。模块边界按职责复审：proto/eval 管求值预算/写回，embed/host_inputs 管授权/合并，service/command_results 管独立结果树；未压缩代码规避行数门禁。
- 本轮实际验证：`cargo test -p kk-runtime --lib service` 61 项通过；`cargo test -p kk-cli --test serve_tests` 20 项在完整门禁中通过。最终无参数 `pnpm run check` 通过（Rust/JS workspace、Clippy、维护工具、文档链接及 1028 个源码/测试文件规模门禁）；新增完整引擎状态回放等值与配置重复字段拒绝均进入最终门禁。已重建 debug/release CLI，核验 screeps-ok 固定 commit，运行 fetch-bots、适配器 build 与 release `node compat/scripts/ratchet.mjs`：2905/2914 passed、9 既有 failed、0 pending，门禁通过；另与接手时报告逐例比对 2914 项状态零变化（原始 success=false，未宣称全绿）。兼容报告刷新，未跑此前暂缓长测。首次完整门禁发现新增测试单元素循环的 Clippy 错误，已修复；新增故障夹具调试时的 V8 LIFO/空 Memory/恢复窗口时序问题已改为正确生命周期和可发生的崩溃窗口。日志 `/tmp/kkscreeps-s23-review-check.log`；S2.3 复审无未清偿的阻塞项；用户要求提交后继续审查 S1.1 至今的整条服务链，跨批 Review 尚待执行，剩余交付范围见下条。
- 已完成并纳入本次提交，HEAD `59710e0b`；完成本片后下一入口 S2.4 outbox，S3 网络/后台 enqueue、淘汰与预算配置化仍未交付。此前性能/长测暂缓保持。

## 台账维护与每批验收




- 开始/调整范围/结束会话时更新接手位置、任务状态、剩余消费者和实际验证；只记录本批必要摘要。已完成的详细过程退出当前入口，普通修复使用提交说明和测试，不逐批新建交付文档。
- 当前页维护进度和依赖；现行机制回写 architecture，公共 SDK 行为变更更新用法/样例，里程碑范围变化更新 roadmap/完整玩法计划。历史结论不追改。
- 新增写入域当片更新[状态清单](../architecture/state-inventory.md)和检查点/版本策略，验证保存恢复与失败边界。预检、意图、结算与消费者同步接线，不等远期通用任务。
- 可替换玩法至少两种独立 JS 实现、原生对照与组合；工具用多包/保护失败案例，迁移/日志/storage 用版本与崩溃矩阵；按相关性覆盖拒绝、异常/预算、停用回退、晚期失败、缓存重试与冷恢复，JSON/KKPV 业务状态一致。
- 公式/准入属于玩法，ID/合法表示/索引/编码一致性/原子发布属于宿主约束；engine 不依赖 V8，mod-api 保持纯数据，同步玩法 VM 入口前释放候选借用。
- 首片明确输入规模、单位/数值域、权限、写面及 before/native/replace/拒绝关系；源码按职责组织，源码/测试不超过 1000 行。
- 验证与提交按 [AGENTS.md](../../AGENTS.md) 和 [compat/README.md](../../compat/README.md)；Rust 改动重建测试 CLI，提交前运行无参数完整门禁。兼容改动重建 release/适配器并运行 ratchet，另与实际开发基线逐例比较，不只看总通过数。
