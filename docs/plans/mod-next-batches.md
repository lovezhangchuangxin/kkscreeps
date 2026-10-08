# Mod 后续开发任务与接手台账

本页只维护**当前状态、执行队列、未完成范围和最近验证**。长期目标与全量领域范围见[完整玩法计划](mod-complete-gameplay.md)，现行机制见[架构索引](../architecture/README.md)，已完成批次的过程见[历史记录](archive/mod-history-2026-10.md)。G01–G30 是稳定任务编号，不是串行开发顺序。

## 当前接手位置

- 更新日期：2026-10-09；实现基线含 `13daad3`（FORTIFY/SHIELD）与本批 deposit 完整业务（随本提交交付）。接手先核对实际 HEAD 与工作区。
- 主线：M2B 默认玩法兼容；M2E lab/boost、M2F factory、M2G nuke、M2H.a power 基座、deposit 采集链已交付。M2H.b 当前 **18/19 powers** 完整接通，剩余 1 个（OPERATE_OBSERVER，依赖观察业务）见下表。
- 下一片：**observer 与 OPERATE_OBSERVER**（动作、范围/RCL/owner、下一轮限时视野、双投影权限与恢复）。每片贯通施放、真实消费者、玩家投影、失败边界和冷恢复。
- 兼容率 2759/2914≈94.7%（95% 中间检查点差 10 例：observer 组之外还有零散红逐例归因）。
- Mod 扩展：移动策略、对象/建筑/资源、库存/耐久/受击、作者 CLI/类型、输入回放和检查点已有基础；未完成范围以任务表为准。默认玩法已有消费者不代表对应 Mod 替换能力已经开放。
- 网络服务、持久 storage 后端与真实游戏客户端仍未实现；renderer R0–R6 已交付，R7 最终矩阵/长测未收口。现有 bot-check 不能代替持续推进世界中的可玩验收。
- M2B 验收标准（2026-10-09 调整）：screeps-ok ≥95% 为中间检查点、≥99% 为后续覆盖目标；最终完成要求约定范围内功能全部通过验收，剩余例外逐项有明确范围、证据和可审查理由，见[路线图](../roadmap.md)。

## 当前执行队列

| 顺位 | 任务 | 首片与完成条件 |
|---|---|---|
| 1 | observer 与 OPERATE_OBSERVER | 动作、范围/RCL/owner、下一轮限时视野、双投影权限与恢复；含最后一个 power 消费者（19/19 收口） |
| 2 | 其他兼容差异与真实 bot | 按最新报告逐例归因；keeper/NPC 单独拆片；真实场景验证采集、搬运、孵化、建造、升级、存活与重启 |
| 独立候选 | G02.d3 后续、G27.a | 前者贯通结构/rampart 通行与寻路；后者提供异步后台生命周期，随后才接 storage/outbox/网络 |

维护优化已完成的轮次不作为新功能前置。任务首次实施前按业务闭环拆片；G30.a 覆盖清单与 G30.d 同功能参考随领域滚动，不等最终收口才补证据。

## M2H.b power 进度

公共效果表、施放共用门、JSON/KKPV 投影、当前格式检查点和候选回滚已实现，机制见[游戏循环](../architecture/game-loop.md#power-效果链当前首片)与[检查点](../architecture/checkpoints.md)。

| 状态 | Power / 消费者 |
|---|---|
| 已完成 18 项 | GENERATE_OPS；OPERATE_TOWER / DISRUPT_TOWER；OPERATE_SPAWN / DISRUPT_SPAWN；OPERATE_LAB；OPERATE_POWER；OPERATE_FACTORY；REGEN_SOURCE / DISRUPT_SOURCE；REGEN_MINERAL；OPERATE_STORAGE；OPERATE_EXTENSION；OPERATE_TERMINAL / DISRUPT_TERMINAL；OPERATE_CONTROLLER；FORTIFY / SHIELD |
| 待实现 | OPERATE_OBSERVER（依赖观察业务） |

剩余任务的具体门序和量值须核对锁定的官方源码与 POWER_INFO：

- 资源/容量：storage 容量与库存消费者同源；extension 为即时灌能，按目标房主、距离顺序和整批预算结算。
- observer：依赖真实观察和限时视野业务，不以当前房间可见替代观察效果。
- 战斗：FORTIFY/SHIELD 联动伤害、拆毁、临时 rampart 耐久与衰减；EFFECT_INVULNERABILITY 的伤害/dismantle/attackController/nuke/rangedMassAttack 等消费门同片盘点，其生产者属于后续 invader core/NPC 域，不计入 19 个 power。

## 任务台账

“部分完成”必须保留父项缺口；“未开始”表示 Mod 能力尚未交付，不否定该领域的默认玩法实现。依赖须在开始任务时核验。

| 编号 | 任务 | 状态 | 已完成与剩余范围 / 入口 |
|---|---|---|---|
| G01 | 移动竞争优先级 | 已完成 | [竞争排序](../architecture/movement.md#竞争排序)；排序能力不等于完整仲裁 |
| G02 | 移动准入 | 部分完成 | .a 准入、.b 障碍追加、.c 请求预检、.d1 静态放行、.d2 双端玩家门、.d3 墙通行声明已交付；剩余结构/rampart 通行、逐房粒度及摘要增强；[移动契约](../architecture/movement.md) |
| G03 | 完整房间仲裁 | 部分完成 | .a 请求视野/链预演与 pull 登记已开放；剩余障碍槽第二预演、完整仲裁替换；[移动契约](../architecture/movement.md) |
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
| G26 | 自动检查点/storage | 部分完成 | .a 周期保存/keep/恢复选择已交付；.b provider 待 G27.a；[检查点](../architecture/checkpoints.md) |
| G27 | 后台服务/outbox | 未开始 | .a 最小异步宿主；.b 耐久输入/outbox，依赖 G25.a/G26.a |
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
| G27.a 后台宿主 | 现有 VM/预算/已提交读面 | 受控异步生命周期与有限探针 I/O；不依赖 outbox，不直接改玩法 |
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
- attackController 的 EFFECT_INVULNERABILITY 预检门（官方 creeps.js:914-919）kk 未接——INVULN 生产者属后续 invader core/NPC 域，落地时须连同 attackController/伤害/拆毁等消费门一起盘点（FORTIFY/SHIELD 批已把结算侧免伤/回收/跳过门备好）。
- upgradeController 对无 WORK 但有能量的 creep，官方仍发 amount=0 的 EVENT_UPGRADE_CONTROLLER 与 actionLog，kk 提前返回不发（复审登记的既有分歧，暂无套件/bot 触达）。
- 历史登记过 sim 并行冷恢复子进程偶发失败，隔离复跑通过且后续完整门禁通过；若再现须保留失败报告归因，不能把重跑成功当作已修根因。
- Overmind bot-check 保留基线已有的 1 个 console-error tick；不宣称零错误。keeper/NPC、多 shard 和其余能力门控缺口以最新兼容报告核对，旧批次数字不参与当前决策。
- 本地检查点的文件大小、单写者及耐久适用范围见[检查点契约](../architecture/checkpoints.md)；存储和真实网络服务不能从 embed 能力直接推导为已完成。

## 最近交付与实际验证

最近功能批次：FORTIFY/SHIELD 效果链（`13daad3`，交付记录见该提交）与 deposit 完整业务（本提交）。deposit：harvest 的 Deposit 分支（预检 isNearTo/ERR_TIRED；结算量 WORK×HARVEST_DEPOSIT_POWER 含 harvest boost，官方分支无 amount 门——WORK 全熄的 amount=0 仍落动作日志、按当前累计重算冷却并刷新寿命）、超容掉落（资源堆落成当拍按 ceil(amount/ENERGY_DECAY) 衰减）、harvested 累计含 boost、decayTime 刷新 time+50000、冷却 ceil(0.001×harvested^1.2)（>1 才挂）经 pending 表在对象 tick 相落成（官方 _cooldown 临时字段同构，同 tick 后续采集读旧值仍成功）、寿命门 `time >= decayTime-1`（processor.js 移除相为实际边界）；放置归一把相对 cooldown/ticksToDecay 换算为绝对键（与 nuke 同形），查询快照面派生 lastCooldown 并折算 ticksToDecay/剩余 cooldown（rawData 投影通道仍透传绝对键由类 getter 折算，双通道口径自洽）。**capabilities.deposit 已翻转**：套件 deposit 组 44 例全绿（含修复 DEPOSIT-HARVEST-003/006 的放置相对冷却换算、DEPOSIT-006 寿命门、快照往返派生字段）。三路复审（结算保真/测试边界/全局影响）发现项当批清偿：amount=0 无门（去提前返回+引擎意图直注测试锁定结算侧静默门与零量流程）、EVENT_HARVEST 断言移回采集轮（事件 1-tick 生命期）、retry 注释改缓存重放语义、>1 冷却边界用同 tick 双 creep 锁定、到期拍 T=4/T=5 钉边、ERR_NOT_IN_RANGE 距离腿、lastCooldown=0/缺 decayTime→undefined、满仓全量掉落极值、creep.js 陈旧注释、bot 基线 find:114 归因纠正（FIND_MY_CONSTRUCTION_SITES 每轮随机漂移 381↔386，非 deposit 因果——bot 世界无 deposit）。FORTIFY 挂 constructedWall/rampart：非核弹伤害完全免疫（_damage 严格 >，免疫含扣血/摧毁/反击/事件）、dismantle 能量回收归零（SHIELD/FORTIFY/INVULN，>= 边界，伤害照常）、rangedMassAttack 跳过目标（>=）、attack/rangedAttack/dismantle 预检即拒 ERR_INVALID_TARGET（SHIELD 不在预检门）。SHIELD 无 ops 无目标：删自身位工地、过官方 checkConstructionSite 的 rampart 放置子集（同类不重叠/本格非墙/贴边三块边界格全墙）后插临时 rampart（hits=effect、hitsMax=0、nextDecayTime=endTime）；护盾可受伤、dismantle 无回收、衰减 tick 跳过同步并到期删除；POWER_INFO.energy 官方无消费者不扣。EFFECT_INVULNERABILITY 与 FORTIFY 共享全部消费门（生产者属后续 NPC 域）。三路复审（结算保真/测试边界/全局影响）发现项当批清偿：rangedAttack 预检效果门移到射程检查后（官方顺序，超射程且无敌返回 -9）、SHIELD 边界检查移到工地删除后（越界 pc 的工地同样先删）、贴边四角选边改顺序赋值（y 覆盖 x，官方与 room.rs 既有决策同形）、插入不带 notifyWhenAttacked、免伤正路径补塔攻击例（无预检门的唯一可达通道）、到期拍用绝对时间钉住（此前 tick 计数失真未落边界）、dismantle 回收加 carry 部件使 0/1 可区分、massAttack 用有主 rampart 钉 >= 边界、rampart 腿预检与 SHIELD 拒绝路径工地删除副作用补断言、attackController 的 INVULN 预检缺口登记。行为变化：无效果路径逐位不变（免伤/回收/跳过门仅活跃效果命中介入）。damage.rs 现 762 行超 700 复审线，下一批涉及时先拆分。

- 完整无参数 `pnpm run check`：Rust workspace 1421 passed / 0 failed（新增 kk-engine deposit_actions 2 例与 kk-runtime deposit_harvest 4 例强化）；JS workspace、工具、文档链接与源码规模通过；无 doctest 变更。
- release CLI 与适配器重建，固定 screeps-ok HEAD `34a630e`；全量 2914 例：2759 passed / 29 failed / 126 skipped，相对 `13daad3` 基线 +40 转绿 / 0 转红（deposit 能力开启），地板 2719→2759。
- 三 bot 完成 2000/2000/10 tick（修复后复跑），无中断/惩罚跳过/missing_api，行为键与基线全等；stub 计数 ±3 每轮随机漂移；Overmind 如上保留既有报错。
- 持久证据：[兼容机器报告](../../compat/reports/kkscreeps.json)、[棘轮配置](../../compat/reports/ratchet.json)、[TooAngel](../../compat/reports/bot-tooangel.json)、[Overmind](../../compat/reports/bot-overmind.json)、[WASM](../../compat/reports/bot-wasm.json)。原临时日志路径仅在历史过程保留，不作为长期证据入口。

## 台账维护与每批验收

- 开始/调整范围/结束会话时更新接手位置、任务状态、剩余消费者和实际验证；只记录本批必要摘要。已完成的详细过程退出当前入口，普通修复使用提交说明和测试，不逐批新建交付文档。
- 当前页维护进度和依赖；现行机制回写 architecture，公共 SDK 行为变更更新用法/样例，里程碑范围变化更新 roadmap/完整玩法计划。历史结论不追改。
- 新增写入域当片更新[状态清单](../architecture/state-inventory.md)和检查点/版本策略，验证保存恢复与失败边界。预检、意图、结算与消费者同步接线，不等远期通用任务。
- 可替换玩法至少两种独立 JS 实现、原生对照与组合；工具用多包/保护失败案例，迁移/日志/storage 用版本与崩溃矩阵；按相关性覆盖拒绝、异常/预算、停用回退、晚期失败、缓存重试与冷恢复，JSON/KKPV 业务状态一致。
- 公式/准入属于玩法，ID/合法表示/索引/编码一致性/原子发布属于宿主约束；engine 不依赖 V8，mod-api 保持纯数据，同步玩法 VM 入口前释放候选借用。
- 首片明确输入规模、单位/数值域、权限、写面及 before/native/replace/拒绝关系；源码按职责组织，源码/测试不超过 1000 行。
- 验证与提交按 [AGENTS.md](../../AGENTS.md) 和 [compat/README.md](../../compat/README.md)；Rust 改动重建测试 CLI，提交前运行无参数完整门禁。兼容改动重建 release/适配器并运行 ratchet，另与实际开发基线逐例比较，不只看总通过数。
