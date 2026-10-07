# 源码组织与规模约束

主项目源码和测试按领域职责组织；单文件超过 1000 行时须拆分。行数是触发职责复审的门槛，不是架构质量的唯一指标。拆分应保留领域语义、注释、公开 API 和事务边界，不能通过压缩代码、删除必要注释或仅把连续代码搬到 `include!` 片段来过关。

`pnpm run check:source-size` 检查 `crates/compat/apps/packages/scripts` 下的 Rust、JavaScript、TypeScript 与 C/C++ 源码，包含 Git 跟踪和未忽略的新文件；已删除文件跳过。这项检查已纳入 `pnpm run check`。第三方固定版本 `crates/kk-runner/assets/lodash-3.10.1.js` 保留原文，例外在检查脚本中显式列出。`spikes/` 独立实验不属于主项目门禁；其中 inspector 实验的长文件保留为实验记录，不能照搬为生产模块。

## 生产模块导航

| 入口 | 子模块职责 |
|---|---|
| `kk-schema/src/kkpv.rs` | 公开值模型、限制和错误；`kkpv/wire.rs` 共享布局，`encode.rs` 编码与束校验，`view.rs` 单块校验与字段解码，`merge.rs` 同版本覆盖合并 |
| `kk-world/src/kkpv.rs` | 权威世界和 JSON 参考数据到块的转换；单元测试独立在 `kkpv/tests.rs` |
| `kk-engine/src/intent.rs` | 动作集合、执行优先级与输入清洗；测试按 JSON 收集、动作语义和市场输入分组 |
| `kk-engine/src/settle/mod.rs` | 世界引擎状态和控制入口；`working.rs` 房间候选及领域操作，`room_loop.rs` 房间循环，`install.rs` 冻结计划接线，`engine_tick.rs` 世界阶段与发布，`validation.rs` 发布前校验 |
| `kk-engine/src/settle/contrib.rs` | 槽、处理器和续行类型及运行入口；`contrib/assemble.rs` 冻结表装配，`incoming.rs` 受击数值链与默认回退，`execute.rs` 内建动作/tick 链执行与 Hosted 局部恢复（资源两端/事件、移动请求登记、战斗/建造族对象/延迟队列/链内增删） |
| `kk-engine/src/settle/creep/combat/damage_policy.rs` | 单体策略伤害的表示校验、战斗视图；默认/native 和策略应用共用 damage.rs 的生命值与死亡路径 |
| `kk-engine/src/settle/creep/combat/heal_policy.rs` | 策略治疗表示校验，native/JS 共用队列、日志与事件登记；最终身体恢复由 damage.rs 处理 |
| `kk-engine/src/settle/creep/combat/retaliation_policy.rs` | 不可变反击表、执行前表示域预留与原生/JS 主攻击组合 |
| `kk-engine/src/settle/incoming.rs` | 受击输入摘要、JS 安全整数与独立数值候选；不直接改世界 |
| `kk-engine/src/settle/damage.rs` | 共用伤害/治疗应用、死亡与库存掉落；`damage/tough.rs` 独立计算原生 TOUGH 累计减伤，不写候选 |
| `kk-engine/src/settle/creep/combat/batch_policy.rs` | 战斗分页与群体批量完整预检/应用；直接候选不变测试在 batch_policy/tests.rs |
| `kk-engine/src/settle/resources.rs` | 资源领域读取与副本预检转账；`resources/generic.rs` 共享 Generic 仓库解析、native 容量与实验室槽写入 |
| `kk-mod-host/src/execution_plan.rs` | 链安装与诊断共用的活动顺序/中心回退选择 |
| `kk-mod-host/src/rng.rs` | 玩法随机流 op 与域序号状态；无状态推导在 `kk-core/src/rng.rs` 的 `gameplay_draw`（协议见[确定性玩法时间与随机协议](gameplay-determinism.md)） |
| `kk-mod-host/src/composition.rs` | 冻结来源、规则候选与选中来源、处理器/manifest 停用闭包 |
| `kk-cli/src/mod_inspect.rs` | 独立注册诊断与停用预览；`output_guard.rs` 共用只读路径与输入保护，`report.rs` 共用原子 JSON/声明写入 |
| `kk-mod-api/src/typegen.rs` | 冻结计划到 TS 声明；`typegen/symbols.rs` 独立维护模块级命名分配与冲突消歧 |
| `kk-mod-api/src/objects.rs` | 对象视图/分页封套和管理初始化/候选创建共用的字段准备 |
| `kk-engine/src/settle/objects.rs` | 同房 Mod 候选读写、命名空间检查；日志与索引恢复在 working.rs |
| `kk-mod-host/src/chain/objects.rs` | 对象 op 的同步访问、可重入字段转换与首个故障保留 |
| `kk-runtime/src/embed/mod_chain/objects.rs` | 自有槽能力门控、绑定 namespace 和候选适配 |
| `kk-mod-host/src/chain/resources.rs` | 资源 op 的同步能力访问、数字校验与故障来源 |
| `kk-mod-host/src/chain/movement.rs` | 移动请求 op 的方向校验与同步能力访问 |
| `kk-mod-api/src/incoming.rs` | 受击输入与来源的共享表示、数值/来源上限 |
| `kk-mod-host/src/chain/incoming.rs` | 受击数值 op、原始浮点校验与能力故障归属 |
| `kk-mod-host/src/chain/combat.rs` | 战斗读取/改向/取消/策略伤害与治疗 op 的同步能力访问与故障来源 |
| `kk-mod-host/src/chain/combat_batch.rs` | 伤害批量/反击表共用有界转换、重入后重新检查能力与战斗分页 op |
| `kk-runtime/src/embed/mod_chain/resources.rs` | 同房资源能力、首次触达日志与字段缓存失效；移动委托在 `mod_chain/movement.rs`，字段投影/写入在 `mod_chain/fields.rs`，受击数值能力在 `mod_chain.rs`；战斗门控、首触日志与 consumed 标记在 `mod_chain/combat.rs` |
| `kk-engine/src/settle/market.rs` | 市场状态和结算入口、成交调度；`market/orders.rs` 订单生命周期，`terminal.rs` 发送/成交共用的库存、费用与交易写入 |
| `kk-runtime/src/run.rs` | 玩家执行参数、结果分类和未提交的指令/Memory/CPU 候选；由 crate 根导出，结算引擎不拥有这些类型 |
| `kk-runtime/src/embed.rs` | 分片状态、管理和诊断入口；`embed/setup.rs` 配置校验与创建，`embed/placement.rs` 原子批量放置，`execution.rs` 玩家候选和失败重试，`embed/input_log.rs` 逐轮提交后的输入日志（G25.a），`embed/replay.rs` 段重建与回放配对（G25.b），`projection.rs` KKPV 与市场视图，`mod_chain.rs` Mod 候选桥，`switch.rs` 无开放轮次的包切换 |
| `kk-runner/src/sandbox.rs` | V8 实例生命周期、运行与写回；`sandbox/host.rs` 加载清单、宿主句柄与 Mod 玩家面安装，`inject.rs` 每轮 JSON/KKPV 与 CPU 窗口注入 |
| `kk-runner/src/prelude/30_game/game.js` | 世界注入、Game 装配和指令收集；`memory.js` 持有 RawMemory/segments 状态与写回，`market.js` 持有每轮市场视图缓存与指令 |

Rust 子模块通过父模块保留既有公开路径；跨子模块的内部操作使用受限可见性，不能因拆文件把内部候选暴露为新的外部写能力。KKPV 合并通过视图的公开读方法访问目录与元数据，保持视图内部字段私有。

JS 的 Memory、市场和 Game 分别持有闭包。加载清单先执行 Memory 和市场，再装配 Game；Game 在加载期捕获桥函数，Rust 捕获宿主句柄后删除临时 `__kkp`。tick 中不能重新查找这一全局。RawMemory 重建、CJS 重绑、Memory getter 安装的先后顺序保持不变，两个世界投影通道共享该流程。

V8 生命周期账本位于 `kk-vm/src/isolate_order.rs`，玩家沙箱与 Mod realm 共用；`kk-mod-host/src/realm_lifecycle.rs` 只负责冷恢复最后切换时的 Enter/Exit 配对。

检查点按所有权分层：`kk-world/src/checkpoint.rs` 保存世界/房间信封与扩展；`kk-engine/src/settle/checkpoint.rs` 保存账户/市场/Mod state 并复用发布校验；`kk-runtime/src/embed/checkpoint.rs` 装配替代分片并核验身份，`checkpoint/identity.rs` 保存包/工作者身份；`kk-runtime/src/checkpoint.rs` 负责文件信封和同步发布；`kk-cli/src/sim/checkpoint.rs` 只处理场景、alias 与原始断言起点。

## 测试组织

公共夹具留在测试族父模块，案例按规则、执行链、领域写入、安装门、故障/拒绝/停用等职责分组；Mod 链测试的共享工厂与放置/投影/ID 契约见 [mod_chain_tests 夹具契约](../../crates/kk-runtime/tests/mod_chain_tests/README.md)。结构拆分本身保持原测试注册数量；后续功能测试在对应子模块显式注册。`sandbox_tests` 与 `embed_engine_suite` 的串行场景仍由原入口按原顺序调用，子模块不新增独立 `#[test]`，避免改变 V8 生命周期和进程级计数的约束。

接近 1000 行时应提前检查职责：新增规则、协议形态或故障恢复场景应放到对应模块；只有实际形成新领域边界时才增加模块。阶段记录见[结构治理验收](../research/notes/source-structure-2026-10-05.md)。

## 权威状态与 SDK 模块

`kk-engine/src/settle/state_api.rs` 提供只读 getters 和隔离编辑；`kk-world/src/world_mutation.rs` 管理内容编辑、结构验证与整轮提交。`kk-runtime/src/embed/mod_chain/capability.rs` 的枚举携带领域状态，宿主接口拆在 `kk-mod-host/src/chain/host_*.rs`。SDK 公共入口为 `sdk.js`，注册/规则/链/刷新/领域实现在 `sdk/`，只允许自身静态导入。

`kk-pathfinder/terrain_adapter.rs` 在 OUT_DIR 生成地形适配源码，固定 `native/pf.cc` 与 `pf.h` 不改写。搜索显式接收 TerrainSet，运行时所有权与缓存见[地形设计](../designs/pathfinder-terrain.md)。

管理初始化使用 `Engine::insert_objects` / `EmbedEngine::place_objects` 或一次 `edit_world`：按输入顺序分配 ID、读取此前已放置的控制器，末项失败不发布前缀，成功只推进一次编辑版本。sim 将全部场景对象作为一批放置；分片房间/控制器创建也在单份草稿上完成。单对象管理 API 保留，反复调用仍承担独立事务的复制成本。世界计划校验在无扩展字段时借用房间，避免额外整房复制。

移动静态判定在 `kk-engine/src/settle/movement/static_obstacles.rs`，由登记和仲裁共用；`precheck.rs` 保存登记拒绝快照与 JS 决策，`movement/inputs.rs` 投影输入及提交瞬态放行。专项测试在 `kk-runtime/tests/mod_chain_tests/move_static.rs` 与 `kk-cli/tests/sim_tests/move_static.rs`，避免继续扩大原预检测试文件。
