# 源码组织与规模约束

主项目源码和测试按领域职责组织；责任变化、重复解析、跨层状态写入或独立测试需求出现时就应复审边界，不等待文件接近行数门槛。单文件 1000 行上限只是兜底约束。职责、接口和依赖共同决定模块边界。拆分应保留领域语义、注释、公开 API 和事务边界，不能通过压缩代码、删除必要注释或仅把连续代码搬到 `include!` 片段来过关。超过 700 行的文件由检查输出职责复审预警（不挡门）；行数是复审信号，不是拆分理由，也不得把批次编号写入文件名或标识符。

`pnpm run check:source-size` 检查 `crates/compat/apps/packages/scripts` 下的 Rust、JavaScript、TypeScript 与 C/C++ 源码，包含 Git 跟踪和未忽略的新文件；已删除文件跳过。这项检查已纳入 `pnpm run check`。第三方固定版本 `crates/kk-runner/assets/lodash-3.10.1.js` 保留原文，例外在检查脚本中显式列出。`spikes/` 独立实验不属于主项目门禁；其中 inspector 实验的长文件保留为实验记录，不能照搬为生产模块。

## 生产模块导航

| 入口 | 子模块职责 |
|---|---|
| `kk-schema/src/kkpv.rs` | 公开值模型、限制和错误；`kkpv/wire.rs` 共享布局，`encode.rs` 编码与束校验，`view.rs` 单块校验与字段解码，`merge.rs` 同版本覆盖合并 |
| `kk-world/src/world/mod.rs` | 世界容器与读入口；`mutation.rs` 管理隔离编辑和发布，`checkpoint.rs` 管理保存恢复 |
| `kk-world/src/projection/mod.rs` | JSON 玩家投影装配；`visibility.rs` 负责可见性和字段投影，`object.rs` 负责对象编码，`find.rs` 负责查询匹配；旧 `world_json` 路径重导出保留 |
| `kk-world/src/effects.rs` | 对象效果权威条目、输入归一、有效期与双投影；具体 power 公式在 engine 消费者 |
| `kk-world/src/domain/` | Generic 字段/仓库解析与房间领域值；JSON、KKPV 和结算共享只读访问，不把写入规则放入投影层 |
| `kk-mod-api/src/contribution/mod.rs` | 声明、规则合并、处理器排序、冻结、计划与指纹；构建实现私有，领域消费者从 crate 根读取稳定公共契约 |
| `kk-mod-api/src/movement/` | 登记、预检、仲裁、准入、障碍和代价的领域表示；crate 根保留旧模块路径 |
| `kk-engine/src/settle/movement/` | 移动请求、策略候选、静态判定、仲裁及输入；同领域代码共同导航 |
| `kk-runtime/src/executor.rs` | 玩家执行接口、线程注册与待收集结果；crate 根只负责模块入口和公开导出 |
| `kk-world/src/kkpv.rs` | 权威世界和 JSON 参考数据到块的转换；单元测试独立在 `kkpv/tests.rs` |
| `kk-engine/src/intent.rs` | 动作集合、执行优先级与输入清洗；测试按 JSON 收集、动作语义和市场输入分组 |
| `kk-engine/src/settle/storage.rs` | storage 对象 tick 容量更新，复用 engine 的控制器配额与 power 查表；world 仅保存、校验和投影容量 |
| `kk-engine/src/settle/powers/use_power.rs` | 公共施放门、目标效果与成功提交尾部；`use_power/extension.rs` 持有灌能排序与资源更新，`shield.rs` 持有临时 rampart 放置 |
| `kk-world/src/domain/deposit.rs` | deposit 放置时钟归一及数值表示校验，插入/编辑/发布/恢复同源；采集和冷却公式由 engine 负责 |
| `kk-engine/src/settle/mod.rs` | 世界引擎状态和控制入口；`working.rs` 候选构造和上下文，`working/` 按对象、字段、移动、掉落、链快照与首触日志组织，`room_loop.rs` 房间循环，`install.rs` 冻结计划接线，`pipeline/` 持有候选阶段/事务/写回/校验/发布 |
| `kk-engine/src/settle/contrib.rs` | 槽、处理器和续行类型及运行入口；`contrib/assemble.rs` 冻结表装配，`incoming.rs` 受击数值链与默认回退，`execute.rs` 内建动作/tick 链执行与 Hosted 局部恢复（资源两端/事件、移动请求登记、战斗/建造族对象/延迟队列/链内增删） |
| `kk-engine/src/settle/creep/combat/damage_policy.rs` | 单体策略伤害的表示校验、战斗视图；默认/native 和策略应用共用 damage.rs 的生命值与死亡路径 |
| `kk-engine/src/settle/creep/combat/heal_policy.rs` | 策略治疗表示校验，native/JS 共用队列、日志与事件登记；最终身体恢复由 damage.rs 处理 |
| `kk-engine/src/settle/creep/combat/retaliation_policy.rs` | 不可变反击表、执行前表示域预留与原生/JS 主攻击组合 |
| `kk-engine/src/settle/incoming.rs` | 受击输入摘要、JS 安全整数与独立数值候选；不直接改世界 |
| `kk-engine/src/settle/damage.rs` | 共用伤害/治疗应用、死亡与库存掉落；`damage/tough.rs` 独立计算原生 TOUGH 累计减伤，不写候选 |
| `kk-engine/src/settle/creep/combat/batch_policy.rs` | 战斗分页与群体批量完整预检/应用；直接候选不变测试在 batch_policy/tests.rs |
| `kk-engine/src/settle/resources.rs` | 资源领域读取与副本预检转账；`resources/generic.rs` 负责 native 容量与实验室槽写入；只读 Generic 解析复用 `kk-world/src/domain/generic.rs` |
| `kk-engine/src/settle/install.rs` | 安装入口与两种执行表适配；`install/candidate.rs` 独占规则、目录、绑定与执行表准备和统一发布，`install/validation.rs` 负责准入校验 |
| `kk-engine/src/settle/pipeline/mod.rs` | `txn.rs` 独占世界候选，`stages.rs` 阶段状态，`writeback.rs` 玩家候选，`validation.rs` 发布前校验，`engine_tick.rs` 阶段推进与统一发布；内部候选访问限定在 settle |
| `kk-mod-host/src/package/mod.rs` | `snapshot.rs` 磁盘发现/快照与摘要，`manifest.rs` 元数据/版本，`dependencies.rs` 依赖序，`selection.rs` 内容选择，`loader.rs` 同快照包内模块装载；realm 只消费包管理入口 |
| `kk-mod-host/src/realm.rs` | VM 所有者与销毁顺序；`realm/bootstrap.rs` 捕获 SDK，`registration.rs` 负责逐包注册与冻结，`installation.rs` 安装规则/链，`execution.rs` 限定能力调用周期，`budget.rs` 统一注册/结算预算，`registry.rs` 独占注册阶段与首故障 |
| `kk-mod-host/src/convert.rs` | 单包贡献候选及最终绑定校验；`convert/` 分别解析对象/字段挂载、动作、处理器、规则、资源/建筑，共享值/schema/字段读取；不在转换层创建 VM 或发布世界 |
| `kk-mod-host/src/execution_plan.rs` | 链安装与诊断共用的活动顺序/中心回退选择 |
| `kk-mod-host/src/rng.rs` | 玩法随机流 op 与域序号状态；无状态推导在 `kk-core/src/rng.rs` 的 `gameplay_draw`（协议见[确定性玩法时间与随机协议](gameplay-determinism.md)） |
| `kk-mod-host/src/composition.rs` | 冻结来源、规则候选与选中来源、处理器/manifest 停用闭包 |
| `kk-cli/src/mod_tools/` | 作者命令参数与 init/inspect/test/types；`registration.rs` 共用包发现、配置校验与选择输入准备 |
| `kk-cli/src/probes/` | 默认负载、显式对照、Mod 链和玩家联合负载；进程采样/环境元数据由命令族共享，不依赖某个负载 |
| `kk-cli/src/sim/` | 场景模型与校验、断言精度、已提交观察、执行编排、检查点与回放各自负责对应契约 |
| `kk-cli/src/support/` | 只读路径与输入保护、原子 JSON/声明写入、完整字节执行身份；命令先检查路径再准备输入 |
| `kk-mod-api/src/typegen.rs` | 冻结计划到 TS 声明；`typegen/symbols.rs` 独立维护模块级命名分配与冲突消歧 |
| `kk-mod-api/src/objects.rs` | 对象视图/分页封套和管理初始化/候选创建共用的字段准备 |
| `kk-engine/src/settle/objects.rs` | 同房 Mod 候选读写、命名空间检查；日志与索引恢复在 `working/chain_snapshot.rs` |
| `kk-mod-host/src/chain/objects.rs` | 对象 op 的同步访问、可重入字段转换与首个故障保留 |
| `kk-runtime/src/embed/mod_chain/objects.rs` | 自有槽能力门控、绑定 namespace 和候选适配 |
| `kk-mod-host/src/chain/resources.rs` | 资源 op 的同步能力访问、数字校验与故障来源 |
| `kk-mod-host/src/chain/movement/mod.rs` | 移动领域宿主接口；`request.rs` 负责请求，其余子模块负责各策略 op |
| `kk-mod-api/src/incoming.rs` | 受击输入与来源的共享表示、数值/来源上限 |
| `kk-mod-host/src/chain/incoming.rs` | 受击数值 op、原始浮点校验与能力故障归属 |
| `kk-mod-host/src/chain/combat.rs` | 战斗读取/改向/取消/策略伤害与治疗 op 的同步能力访问与故障来源 |
| `kk-mod-host/src/chain/combat_batch.rs` | 伤害批量/反击表共用有界转换、重入后重新检查能力与战斗分页 op |
| `kk-runtime/src/embed/mod_chain.rs` | 整槽 VM 调用与 trace/故障转交；`call.rs` 一次归一化参数/native/领域能力，`completion.rs` 根据能力统一暂存状态与完成/拒绝恢复，`capability.rs` 持有唯一领域状态 |
| `kk-runtime/src/embed/mod_chain/resources.rs` | 同房资源能力与字段缓存失效；首触日志和恢复复用 engine 的 `ObjectJournal`；移动委托在 `mod_chain/movement.rs`，字段投影/写入在 `mod_chain/fields.rs`，受击数值能力在 `mod_chain.rs`；战斗门控、日志委托与 consumed 标记在 `mod_chain/combat.rs` |
| `kk-engine/src/settle/market.rs` | 市场状态和结算入口、成交调度；`market/orders.rs` 订单生命周期，`terminal.rs` 发送/成交共用的库存、费用与交易写入 |
| `kk-runtime/src/run.rs` | 玩家执行参数、结果分类和未提交的指令/Memory/CPU 候选；由 crate 根导出，结算引擎不拥有这些类型 |
| `kk-runtime/src/embed.rs` | 分片状态、管理和诊断入口；`embed/setup.rs` 配置校验与创建，`embed/placement.rs` 原子批量放置，`execution.rs` 玩家候选和失败重试，`embed/input_log.rs` 逐轮提交后的输入日志（G25.a），`embed/replay.rs` 段重建与回放配对（G25.b），`projection.rs` KKPV 与市场视图，`mod_chain.rs` Mod 候选桥，`switch.rs` 无开放轮次的包切换 |
| `kk-runner/src/sandbox.rs` | V8 实例生命周期、运行与写回；`sandbox/prelude.rs` 内嵌脚本清单、基础环境和 Mod 玩家面装配，`sandbox/host.rs` 捕获宿主句柄与生命周期调用，`inject.rs` 每轮 JSON/KKPV 与 CPU 窗口注入 |
| `kk-runner/src/prelude/game/game.js` | 玩家对象索引与 Game 装配、当前实例；`map.js` 持有每 tick 地图/路由/绘图实例，`intents.js` 持有指令清单与虚拟计费；`memory.js` 持有 RawMemory/segments 状态与写回，`market.js` 持有每轮市场视图缓存与指令 |
| `kk-runner/src/prelude/runtime/world-tick.js` | JSON/KKPV 共用世界注入与 tick 初始化顺序；一次性捕获 Memory/CJS/指令桥，并由 Game 入口传入刷新函数，向宿主提供既有 memoryWorld 句柄 |
| `kk-runner/src/prelude/game/creep/movement.js` | 移动参数解析、路径解码与 `_move` 缓存生命周期；入口分别安装路径静态与移动方法，传入同一规则常量/部件检查 |
| `kk-runner/src/prelude/game/targets.js` | 当前 tick 的 creep/structure 存在性检查及 lodash 口径序列化对象拒绝；消费者加载期捕获，公开实例判定仍由动作完成 |
| `kk-runner/src/prelude/runtime/deprecation.js` | 共用弃用提示与本 tick 去重状态；捕获原 console，原结构 tick 钩子在孵化名称重置后调用诊断重置 |
| `kk-runner/src/prelude/game/structure-api.js` | 结构动作与孵化/诊断 tick 重置编排；孵化及字段安装阶段在原位置显式调用 |
| `kk-runner/src/prelude/game/structures/fields.js` | 私有通用字段表、类型选择及 Mod 建筑字段装配；专用结构资源/时间读面，按原顺序安装原型字段与结构装配器 |
| `kk-runner/src/prelude/game/structures/spawn.js` | 独占本 tick 孵化名称、成本/能量计算、Spawning 和待出生视图，以及创建/续命/回收方法；装配与重置经一次性捕获的函数连接 |
| `kk-runner/src/prelude/game/room.js` | 只装配公开属性、Memory/事件日志及玩家原型；`room/queries.js` 持有单房间类型/空间索引和出口缓存，`pathfinding.js` 持有按 Room 身份隔离的矩阵，`terrain.js` 安全拷出地形，`visual.js` 持有房间/map 绘图缓冲，`creation.js` 提供创建预检与指令方法 |

目录分组对应执行或状态边界：`settle` 的领域规则（creep、spawn、structures、room、market、resources 等）与 `pipeline` 的世界候选提交分开，`working` 仍负责房间候选；不把各领域改为一个中心分派。`kk-mod-api/contribution` 是启动期计划构建，`validate` 的值校验同时服务运行期，因此保留公共独立入口；resources/buildings/extension/player_surface 等是冻结计划的只读消费目录，按领域保留。`kk-mod-host/package` 负责来源字节和包选择，realm 负责 VM 生命周期，convert 负责声明转换，chain 负责候选能力。包装载不重新读盘，SDK 虚拟 URL 和字节保持；各组通过父入口协作，消费者不导入实现子模块。

小而职责单一的模块可平铺（例如 Mod API 的 outcome、combat/incoming 值协议和 host 的错误/预算/诊断类型）。是否建目录取决于共同状态、执行流程和修改责任，不以文件数量统一决定。`builtin.rs`/`builtin/store.rs` 的原始源码字节参与内建贡献身份，本轮不改其声明或内容，沿用既有 `builtin_capabilities` 加载方式。调整这些文件即使不改变规则也会改变 PlanHash，须单独评审恢复与版本影响。

Rust 子模块通过父模块保留既有公开路径；跨子模块的内部操作使用受限可见性，不能因拆文件把内部候选暴露为新的外部写能力。KKPV 合并通过视图的公开读方法访问目录与元数据，保持视图内部字段私有。

Prelude 按职责使用 `runtime/`、`game/`、`mods/` 与 `host.js`，JS 文件统一 kebab-case。执行顺序只由 `sandbox/prelude.rs` 的显式清单与装配阶段决定，不依赖数字前缀或目录排序。基础环境先装配 lodash/常量、游戏 API 和宿主入口，再按配置装配扩展意图；宿主句柄捕获后，在受预算保护的 Mod 安装期注入清单、加载安装运行时，捕获安装入口并删除 `__kkp`，然后执行作者模块。无 Mod 路径也在收尾删除 `__kkp`。V8 脚本来源名同步使用职责名称，仅影响诊断定位，不改变执行顺序。

Room 子段先装配地形、绘图、查询、创建和寻路，再执行 Room 入口，随后保持其余玩家 API 的原加载顺序。Room 入口在装配期捕获工厂；查询索引和出口缓存每 tick 新建，原始对象列表仅传给创建预检和私有寻路缓存，不挂为玩家属性。方法属性插入、原型设置、probe 与 WeakMap 登记顺序保持，地形拷贝和索引装配仍使用捕获的内建函数。

JS 的 Memory、市场、地图、指令和 Game 分别持有闭包。加载清单先执行 Memory/市场，再注册地图/指令工厂和世界注入桥，最后装配 Game；跨模块函数在加载期捕获，Rust 捕获宿主句柄后删除临时 `__kkp`。tick 中不能重新查找这一全局。RawMemory 重建、CJS 重绑、Memory getter 安装的先后顺序保持不变，两个世界投影通道共享该流程。Game 入口先刷新初始实例、登记 currentGame，再初始化指令收集器并连接宿主桥；每 tick 在原始对象索引建立后重置指令，在 Memory/CJS 和地形/API 重置之后刷新 Game。地图工厂每次装配新建路由缓存和惰性 MapVisual，保持公开方法与探针的插入顺序。

孵化模块先注册工厂，结构 API 入口捕获工厂并传入共用目标检查/弃用诊断。孵化视图、方法、专用通知及装配器分阶段安装，使跨结构的原型/字段注册顺序保持；入口保留原位置的单个 tick 重置钩子，先重置孵化名称，再清空弃用诊断。重置钩子只捕获重置函数，不持有安装接口对象。名称数组由孵化闭包持有，不能在通用结构装配器中直接写入。创建成功的待出生对象仍只是本 tick 玩家视图，Rust 结算负责真正发布。

结构字段模块持有通用字段表及字段装配选择规则，入口只捕获核心字段、装配器与专用字段三个安装函数。专用时间/资源字段和 Mod 建筑字段沿用 raw 回查，不复制为入口状态；实例级兜底属性仍可遮蔽对应原型 getter，缺失字段、资源容量及地形/对象类型排除保持。字段阶段与孵化/动作的交错顺序保持，Spawning 在 InvaderCore getter 使用前登记。

目标检查及弃用诊断在 registry 之后、Room/Creep 之前注册，消费者在加载期捕获；结构模块不再替早于自身加载的 API 提供运行期函数登记。存活检查每次查询当前 rawById，不捕获某一 tick 的 Map；实例类型和动作顺序仍由调用方决定。Creep.moveTo 与结构共享 lodash 判定的序列化对象拒绝，资源 API 保留预置 Object/primitive-string 的独立口径，不能合并成同一种拒绝语义。

Creep 移动工厂在加载期捕获原 Room 序列化器及 registry/lodash，私有解析函数和路径解码器不依赖玩家可替换的静态方法。入口先在原位置安装 Room 路径解码静态方法，再捕获共享规则常量、定义字段，最后在原动作位置安装 move/moveTo/moveByPath；采集/战斗及资源/控制器动作沿用原顺序。移动模块直接维护调用方的 Memory `_move`，不增加独立 tick 缓存，不改变 noPathFinding、缓存过期/截断、可视化及指令边界。

V8 生命周期账本位于 `kk-vm/src/isolate_order.rs`，玩家沙箱与 Mod realm 共用；`kk-mod-host/src/realm/lifecycle.rs` 只负责冷恢复最后切换时的 Enter/Exit 配对。

检查点按所有权分层：`kk-world/src/world/checkpoint.rs` 保存世界/房间信封、扩展与对象效果；`kk-engine/src/settle/checkpoint.rs` 保存账户/市场/Mod state 并复用发布校验；`kk-runtime/src/embed/checkpoint.rs` 装配替代分片并核验身份，`checkpoint/identity.rs` 保存包/工作者身份；`kk-runtime/src/checkpoint.rs` 负责文件信封和同步发布；`kk-cli/src/sim/checkpoint.rs` 只处理场景、alias 与原始断言起点。

## 测试组织

公共夹具留在测试族父模块，案例按规则、执行链、领域写入、安装门、故障/拒绝/停用等职责分组；Mod 链测试的共享工厂与放置/投影/ID 契约见 [mod_chain_tests 夹具契约](../../crates/kk-runtime/tests/mod_chain_tests/README.md)。多文件测试目标统一采用 `tests/<name>/main.rs` 形态（cargo 自动发现，`--test <name>` 目标名不变），根内 `mod` 声明按同目录规则解析，不使用 `#[path]` 挂接；kk-pathfinder 的单文件根 + 共享 `tests/support/`（裸 `mod support;`）是早于本规范的既有形态，同样不使用 `#[path]`，保留。结构拆分本身保持原测试注册数量；后续功能测试在对应子模块显式注册。`sandbox_tests` 与 `embed_engine_suite` 的串行场景仍由原入口按原顺序调用，子模块不新增独立 `#[test]`，避免改变 V8 生命周期和进程级计数的约束。`kk-vm::test_support::ScenarioSuite` 输出场景名称和耗时；开发时可用 `KKSCREEPS_SCENARIO` 子串筛选，未匹配时报错，子集结果明确标记。完整门禁会清除该变量。

职责复审贯穿开发：新增规则、协议形态或故障恢复场景应放到对应模块；只有实际形成新领域边界时才增加模块。阶段记录见[结构治理验收](../research/notes/source-structure-2026-10-05.md)；当前维护改动的范围与验收见[维护结构优化](../plans/archive/structure-governance-2026-10.md)。

## 权威状态与 SDK 模块

`kk-engine/src/settle/state_api.rs` 提供只读 getters 和隔离编辑；`kk-world/src/world/mutation.rs` 管理内容编辑、结构验证与整轮提交。`kk-runtime/src/embed/mod_chain/capability.rs` 的枚举携带领域状态，宿主接口拆在 `kk-mod-host/src/chain/host_*.rs`。SDK 公共入口为 `sdk.js`，注册/规则/链/刷新/领域实现在 `sdk/`，只允许自身静态导入。

`kk-pathfinder/terrain_adapter.rs` 在 OUT_DIR 生成地形适配源码，固定 `native/pf.cc` 与 `pf.h` 不改写。搜索显式接收 TerrainSet，运行时所有权与缓存见[地形设计](../designs/pathfinder-terrain.md)。

管理初始化使用 `Engine::insert_objects` / `EmbedEngine::place_objects` 或一次 `edit_world`：按输入顺序分配 ID、读取此前已放置的控制器，末项失败不发布前缀，成功只推进一次编辑版本。sim 将全部场景对象作为一批放置；分片房间/控制器创建也在单份草稿上完成。单对象管理 API 保留，反复调用仍承担独立事务的复制成本。世界计划校验在无扩展字段时借用房间，避免额外整房复制。

移动静态判定在 `kk-engine/src/settle/movement/static_obstacles.rs`，由登记和仲裁共用；`movement/precheck.rs` 保存登记拒绝快照与 JS 决策，`movement/inputs.rs` 投影输入及提交瞬态放行。专项测试在 `kk-runtime/tests/mod_chain_tests/move_static.rs` 与 `kk-cli/tests/sim_tests/move_static.rs`，避免继续扩大原预检测试文件。移动玩家门快照 `MoveGate` 与规则声明同在 `kk-mod-api/src/builtin.rs`（战斗射程先例），专项测试在 `kk-runtime/tests/mod_chain_tests/move_gate.rs` 与 `kk-cli/tests/sim_tests/move_playergate.rs`。

开发中定向验证使用 `pnpm run check -- kk-cli --test sim_tests --filter move_static`：先发现实际测试，空匹配报错；格式、Clippy、当前 CLI 构建及工具/文档门禁仍执行，Rust 部分只验收所选目标。每步输出耗时供定位成本；无参数完整门禁保持全部目标。定向通过不代表完整验收，也不改变真实进程冷恢复场景的断言与轮数。

CLI 命令契约测试在 `kk-cli/tests/command_tests/`，embed RPC 和 mod init/inspect/test/types/输出保护按模块组织，共用 `command_tests` 测试目标。它们仅通过独立 CLI 子进程执行命令，夹具仍各自持有，减少测试入口的重复链接和启动；sim 场景继续保留独立的 `sim_tests` 目标。验证某一命令族可使用 `pnpm run check -- kk-cli --test command_tests --filter mod_types_tests`；原有同名独立测试目标已合并，历史验收命令保留当时记录。

CLI 的开发/测试构建对 `sha2` 依赖启用 `opt-level = 3`，ARM 后端特征在共同依赖 kk-mod-api 声明，使完整门禁、crate 子集和单独 CLI 构建统一选择；ARM 目标启用固定 `sha2 0.10.9` 的 `asm` 特征：该版本的 ARM SHA-256 硬件后端由此开关控制，运行时检测指令支持，缺失则回退软件实现。`sim run` 仍逐进程读取并计算完整可执行文件的 SHA-256，检查点身份校验保持；未复用摘要或跳过恢复门。硬件后端选择同样适用于 ARM release 构建。标准摘要向量与真实命令/检查点测试验证后端语义。

`kk-pathfinder` 的开发/测试包也单独启用 `opt-level = 3`，其 `cc` 构建脚本按包级 `OPT_LEVEL` 编译固定上游 C++ 算法；调试信息和断言仍继承原配置，其余 crate 的优化级别不变。原生黄金测试保留全部 24 个位置、两两搜索五轮和官方校验和，回调安全、重入及玩家 API 测试仍执行；release 原有优化级别不变。首次重建与 macOS 首次执行评估的等待不能当作后续增量门禁耗时。

`sim_tests` 的移动代价和竞争策略模块在同一测试进程内共享只读基准报告，每个场景仍由当前 CLI 分别运行 JSON/KKPV 通道；行为与组合断言分别保留，保存/恢复过程仍各起新进程。报告不跨构建持久缓存，收集失败返回错误。开发时按受影响领域选择 `--filter move_cost` 或 `--filter move_policy`；同时修改共享模拟/恢复机制时扩大到 `--test sim_tests`，改变构建配置时执行无参数完整门禁。
