# JS Mod 完整玩法目标现状复查（2026-10-05）

## 范围与结论

用户确认的目标：开发者通过独立 JS Mod 创造完整玩法，覆盖本地调研版本 xxscreeps 已有扩展领域，并提供更完善的组合、恢复和开发工具。覆盖的是业务能力，不是 Node 内部接口或 Mod 源码兼容。

本项目核查基线为 `1b40930`；本地 xxscreeps 为 `5ff0d1a682170f6b5f9c8bac3e703767625988e0`，位置 `/Users/keqing/Desktop/projects/sources/screeps/xxscreeps`。本次核查源码与入库验收证据，文档修订已通过完整 `pnpm run check`（格式、Clippy、Rust/JS 测试、文档链接与源码规模）；没有重新执行完整 screeps-ok 或跨项目性能基准。结论不代表上游最新版本。

**方向保留，优先级调整。** Rust 默认内核、可信 JS 宿主、冻结贡献计划、独立玩家 VM 和统一候选提交已经形成可用基础。主要缺口从“JS 能否参与结算”转为“JS 能否独立完成玩法”。继续逐个包装默认动作有价值，但不能用槽位数量替代开放数据模型、生命周期、领域算法和服务扩展的验收。

## 本项目已完成什么

| 能力 | 核查结果与证据 |
|---|---|
| 独立 JS 包 | 包发现、ESM 装载、依赖拓扑、内容指纹、注册冻结、共享可信 realm 已接入；见 [Mod 执行](../architecture/mod-execution.md)与[装载交付](notes/c1-mod-loader-2026-10-03.md) |
| 组合执行 | before/wrap/replace/after、JS next 续行和 native 中心已接入；多 replace 冲突与依赖校验由冻结阶段处理；JS 作者表达处理器依赖的接口仍有缺口 |
| 新对象与玩家 API | ModObj、字段 schema、对象域玩家意图、服务端管理创建、player.js、FIND/LOOK、可见性裁切、JSON/KKPV 投影已接入；见[对象交付](notes/c3-mod-objects-2026-10-03.md)与[玩家 API 交付](notes/c4-player-api-2026-10-03.md) |
| 默认规则与动作 | 13 个内建槽：link transfer/tick、creep transfer/withdraw/move、五个战斗动作、三个建造动作；除 link tick 外，12 个动作槽有正常拒绝；另有 link 数值规则和 10 条 Generic 仓库容量规则 |
| 候选领域操作 | 三个资源动作内可同房多对象转账；move 可改向/取消请求；combat/build 可改向/取消并执行原生效果；见[资源能力](../architecture/mod-resources.md)、[移动槽](../architecture/creep-move-slot.md)、[战斗槽](../architecture/creep-combat-slot.md)、[建造槽](../architecture/creep-build-slot.md) |
| 内存恢复 | 动作正常拒绝恢复该动作写入；异常丢弃整轮候选、重建污染 VM、复用玩家结果；显式停用及可配置连续故障停用已接入，schema/规则仍保留 |
| 工程与测试工具 | 有界玩家派发、线程 CPU 计费、场景模拟、Mod 性能探针、文件规模与文档检查已有实现；sim 尚不能加载 Mod 包 |

槽清单的直接依据为 [builtin.rs](../../crates/kk-mod-api/src/builtin.rs)，拒绝资格与执行边界见[Mod 执行](../architecture/mod-execution.md#正常动作拒绝)。最近建造批次[机器报告](notes/d4-build-slot-2026-10-05.json)记录 Rust 715 passed / 11 ignored，screeps-ok 1918 passed / 2914 total、相对基线零回归，以及双投影 120 tick 建造场景。这组数字引用该批次入库结果，不是本次重新采集的完整验收，也不是完整 Mod 玩法验收。

## 与本地 xxscreeps 的领域覆盖对照

下列 xxscreeps 路径均相对其仓库 `packages/xxscreeps/`。`config/loader.ts` 的提供者领域包括 backend、config、constants、driver、game、main、processor、schema、storage、terrain、test；仅有领域名称不等于完整能力，以下结合实际注册消费者判断。

| 扩展领域 | xxscreeps 源码入口 | kkscreeps 当前差距 |
|---|---|---|
| 类型、附加字段、枚举、资源 | `engine/schema/index.ts`；`mods/classic/chemistry/schema.ts` 注册 ResourceType | 已有新对象/字段描述和投影；新资源目录没有 JS 注册闭环，资源路径仍读 RESOURCES_ALL；BodyKind 等内建表示仍封闭 |
| 对象生命周期、房间与世界规则 | `engine/processor/index.ts` 的 preTick/tick；`room.ts` 的 room tick；`shard.ts` 的 tick/initializer | 自有 JS 对象生命周期未派发；只有对象域玩家动作可执行；房间/玩家/世界域、服务端发射尚被装配门拒绝 |
| 新建筑与原生玩法交互 | `mods/classic/construction/game.ts` 的 registerBuildableStructure；各玩法的 processor | 不能仅凭 ModObj 自动加入建造、障碍、受伤、维修与库存通路；除 link 等已接线规则，建造成本仍读官方表 |
| 动作和深层算法 | `engine/processor/index.ts` 的 registerIntentProcessor 与对象处理器；各 classic Mod 实现领域行为 | 已开放的动作链可组合；移动仲裁、最终伤害应用、通用建造成本及市场全局算法尚无完整 JS 替换通路 |
| 玩家环境、运行桥与类型 | `game/symbols.ts`；`engine/runner/symbols.ts`；loader 的 types 聚合 | player.js 和查询入口可用；通用初始化/连接器、动态集合与 SDK/玩家类型生成尚未完整覆盖 |
| 后端、管理、客户端 | `backend/symbols.ts` 的 route/subscription/middleware/version 等 hook | kk-backend、apps/game 与客户端共享包仍为预留；尚无对应可安装 JS 服务/客户端贡献通路 |
| 存储与外部集成 | `engine/db/storage/register.ts` 的 registerStorageProvider；messages/notifications 业务 provider | kk-storage 未接入写盘；JS storage/业务 provider、后台 I/O、事务 outbox 尚未实现 |
| 地图与初始化、测试 | loader 的 terrain/main/test；shard initializer | 现有固定场景与探针不能替代 Mod 地形、世界初始化、服务器生命周期和作者测试套件 |

xxscreeps 的 hook/provider/prototype 组合机制与本项目冻结处理链不同，不能把它描述成所有动作都有统一 replace 接口。也不能仅从本次检查的处理器接口推断它没有任何恢复、回放或开发工具。本项目“更完善”的目标应由双方同场景的组合诊断、恢复和作者工作流验收证明；当前未达到全领域覆盖，更没有完成整体超越的验收。

## 会限制完整玩法的具体边界

### 1. 动作替换尚不等于算法替换

`ctx.combat.request(targetId)` 与 `ctx.build.request(targetId)` 执行完整原生预检和效果；伤害、治疗、进度与能耗仍由部件/boost 和默认规则决定。`ctx.movement` 修改登记，位移冲突仲裁仍在后续原生阶段。自定义护甲、能量护盾、不同建造配方、不同移动争议规则，不能只靠这些 request API 完成。

这是现阶段受控能力范围，不应变成最终创作上限。应把坐标/ID/索引/数值表示等宿主约束，与距离、伤害公式、准入、成本等玩法策略分开；替换策略后，玩家预检、清洗、结算和寻路不能再次按隐藏的 vanilla 规则否决。

### 2. 新对象有动作，没有自主生命周期和通用操作

[install.rs](../../crates/kk-engine/src/settle/install.rs) 明确只接对象域、末次覆盖、严格转换、玩家授权；无意图挂载的自有槽也被拒绝。[convert.rs](../../crates/kk-mod-host/src/convert.rs) 给自有类型写入空 `lifecycle_slots`。管理创建 ModObj 已实现，但玩法 JS 的通用对象查询、创建、删除和跨对象更新尚未开放。

结果是 counter 可以回应玩家动作，但“每轮自动生产、周期生成单位、对象到期销毁”的独立玩法仍缺通路。内部 native 已有增删日志，并不等于 JS 已获得这些操作。

### 3. 声明层比作者可用 SDK 更宽

[sdk.js](../../crates/kk-mod-host/src/sdk.js) 的 handler spec 只传 id/slot/kind，convert 固定 `depends_on` 为空。Rust 描述与停用闭包支持显式处理器依赖，JS 作者却不能声明它，仍只能借助包依赖与确定排序。需要公开真正能影响冻结图的 JS 契约和链诊断。

SDK 可声明数值规则及消费者，但当前处理器 ctx 没有读取冻结有效规则的公共入口；不能用捕获注册默认值冒充实际覆盖后的值。新资源、对象 trait 与业务策略同样要从 JS 声明贯通消费者，而不是停留在 Rust descriptor 的潜在表达能力。

### 4. 恢复覆盖普通错误，尚未覆盖进程重启

[txn.rs](../../crates/kk-engine/src/settle/txn.rs) 已把 namespace Mod state 纳入候选。模块全局不参与动作恢复，故障时重建 realm；停用不会删除数据或撤销有效规则。当前没有完整检查点、执行策略身份、迁移、缺包只读导出、持久输入日志和 outbox。房间二进制往返与 sim 报告都不能代替整个世界的保存恢复。

### 5. 作者缺少统一验证入口

[sim run](../architecture/local-simulation.md) 没有 Mod 包加载、seed、检查点恢复与热更新。现有链测试、mod-perf-probe 和 mod-player-probe 可以证明局部机制，但作者还没有“加载我的包 → 推进真实玩法 → 看组合链 → 注入故障 → 保存恢复”的公共工作流。更强工具不能只体现在内部 Rust 测试数量。

## 调整建议

1. 保留现有架构与已经通过的动作族；新增动作槽按完整玩法夹具的需求推进，不再作为唯一主线。
2. 优先接通自有对象生命周期和候选对象操作，同时补齐 JS 依赖表达、有效规则读取与 sim 的 Mod 装载。
3. 以“新资源生产建筑”贯穿资源目录、对象能力、建造/库存/受击/寻路/玩家面；再用规则示例验证移动仲裁或伤害公式确实可替换。
4. 提前建立检查点数据域与版本清单，先保存当前完整已提交状态；迁移、输入日志、outbox 随相应领域扩大，不等全部规则开放后才开始设计。
5. 房间/玩家/世界规则和后台服务分别接线。服务与客户端原本已有覆盖承诺，应随 M3 实现，不降格为可选增强。
6. 将组合诊断、故障复现、恢复与边界更新作为产品验收；首次热更新先支持 tick 边界失败保旧版，再考虑在线迁移和更强隔离。

实施拆分与首片交接要求见[完整玩法开发计划](../plans/mod-complete-gameplay.md)。本报告保存上述基线的事实，后续实现不追改本次缺口清单。
