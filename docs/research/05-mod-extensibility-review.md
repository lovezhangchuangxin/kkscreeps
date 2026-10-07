# Mod 扩展能力架构调研

**日期：2026-10-01。结论依据：本地源码审读、当前二进制探针、定向测试。本文是调研与建议，不改变已接受的 ADR，也不表示建议的接口已经实现。**

> **后续需求更新（同日）**：用户明确 Mod 脚本必须为 JS。本文保留初次调研历史：官方/xxscreeps 能力证据与当前阻塞点仍适用；§6.1、§7 中外部 Rust 编译期 Mod 的建议已撤回，相关原生入口只能作为 JS 宿主的引擎内部服务。最新整体架构评估见 [06](06-architecture-and-js-mod-review.md)，最新 JS-only 实现方案见 [mod-system.md](../designs/mod-system.md)，用户确认的约束已记录到 [ADR-007](../decisions/ADR-007-mod-system.md)。

**结论：当前项目还不能像本地 `xxscreeps` 那样通过独立 Mod 扩展完整玩法，也没有实现与官方私服基本等价的 Mod 能力。现有 Rust + deno_core + 房间快照方向可以承载这项需求，但需要补齐跨层扩展契约；仅在 M4 加一个 JS 加载器和 tick 事件总线不够。应在继续扩充玩法、冻结持久化格式、实现网络和客户端之前建立关键扩展边界。**

这里区分三件事：修改本项目源码能够开发新玩法；另写程序调用现有公开 Rust API 能够做部分管理和模拟；第三方通过自己的 Mod 包注册新玩法且不修改核心源码。前两项已经有基础，第三项才是本次需求的判据。

## 1. 调研范围与需求口径

| 对象 | 检查版本与范围 |
|---|---|
| kkscreeps | `/Users/keqing/Desktop/projects/games/kkscreeps`，HEAD `f89a5ea7ffe8b2aa6b93f6756588234cc810f2af`；检查开始时工作树干净 |
| xxscreeps | `/Users/keqing/Desktop/projects/sources/screeps/xxscreeps`，HEAD `4795a3328320aa3f7c02e9866ca2665da331db65`；以其 `packages/xxscreeps`、`packages/client`、`packages/redis` 的本地实现为准；已有未跟踪 `.python-version`、`src/` 未修改 |
| 官方私服 | 同级 `common`、`driver`、`engine`、`launcher`、`renderer` 克隆及 `backend-npm` 发布包；同时核对官方网页和官方 Mod 示例 |

官方本地版本：common `2fb779b26eef9b4b0f412584f6bd47c897949766`；driver `cf63d8adf902663e2ebddd7f8c5b7baa425dc928`；engine `a90d26fc9eb943147799e65d0bf9d4d0d79a2722`；launcher `a5452f195a35ecd319a4645817f1793cbf85580d`；renderer `a2db4a76bb8f4c70e0c2a9e3b7d22a35a8c6b504`；backend-npm `3.3.0`。本地 xxscreeps 的能力不自动代表任意上游版本的能力。

本报告把“基本一致”解释为：**能够重新实现同类 Mod 的可观察功能，不要求源代码、数据库结构、内部事件名或 Node.js 依赖兼容。** 覆盖服务器管理、游戏规则、新对象、新资源、新动作、玩家 API、存储、网络、图形与界面。原生模块需要重新编译可以作为一种交付方式，但应明确它与“安装 JS 包、重启即可生效”的便利性差异。

官方 Mod 还能直接修改内部 JS 方法；任意内部补丁与有限稳定 API 不能天然一一等价。合理的能力承诺是开放下面列出的完整业务域、允许包裹或替换默认实现，并为尚未覆盖的深层领域保留可信原生实现入口。不能仅凭“底层可改源码”宣称全部 Mod 已支持。

## 2. 官方实际开放了什么

官方文档明确把自定义规则、行为和图形列为私服 Mod 的用途。[官方 Community Servers 文档](https://docs.screeps.com/community-servers.html)

源码与示例进一步确认，它的扩展范围包括：

| 能力 | 一手证据（路径相对于官方各仓库） |
|---|---|
| 加载服务器模块、修改共享配置 | `common/lib/config-manager.js`：读取 MODFILE，调用每个模块导出的配置函数 |
| 改游戏常量、节拍、CPU 配额 | `launcher/init_dist/example-mods/change-constant.js`、`tick-duration.js`；`driver/lib/index.js` 的引擎配置 |
| 加管理命令、定时任务、HTTP 路由、通知、历史存储 | `launcher/init_dist/example-mods/cli-method.js`、`cronjob.js`、`http-endpoint.js`、`notifications-mailer/`、`room-history.js` |
| 更换内部服务方法 | `launcher/init_dist/example-mods/driver-method.js`：保存原实现后覆盖方法；存储也可更换，见[官方服务器说明](https://github.com/screeps/screeps) |
| 扩展玩家沙箱 | `launcher/init_dist/example-mods/player-sandbox.js`；`driver/lib/runtime/make.js` 的 playerSandbox 事件 |
| 增加对象类、属性、原型方法、FIND/LOOK | `driver/lib/index.js` 的 registerCustomObjectPrototype；`engine/src/game/game.js`、`rooms.js` 的自定义对象装配和查询 |
| 增加新指令并在宿主清洗后执行 | `common/lib/system.js` 的 customIntentTypes；`engine/src/utils.js` 的 storeIntents；`engine/src/processor.js` 的意图事件 |
| 在默认处理前后改状态、取消原处理 | `engine/src/processor.js` 的 processObject / preProcessObjectIntents / processObjectIntents / postProcessObject / processRoom；示例设置 `_skip` 禁止默认对象处理 |
| 自定义图形、文字、贴图、动画、对象详情面板 | `launcher/init_dist/example-mods/custom-objects/`、`renderer/`；`backend-npm/lib/game/server.js` 的 customObjectTypes 和 renderer 配置 |

最适合作为最低验收样本的是[官方自定义指令对象示例](https://github.com/screeps/launcher/blob/master/init_dist/example-mods/custom-objects/intent-custom.js)：同一个 Mod 声明对象字段、玩家方法、FIND 常量、指令参数、宿主处理器、图形和详情面板。它验证的是整条链，不是单个回调。

规则替换也不能遗漏：[官方 process-object 示例](https://github.com/screeps/launcher/blob/master/init_dist/example-mods/process-object.js)可以阻止 Source Keeper 生成。这类能力需要取消或替换默认处理，事后观察事件不能实现它。

## 3. xxscreeps 为什么可以做到深度扩展

其扩展体系不是仅有一个 hooks 工具，而是所有关键层都接受 Mod 贡献。下面路径均相对于本地 xxscreeps 根目录。

| 层 | 实际扩展机制与证据 |
|---|---|
| 包加载 | `packages/xxscreeps/config/mods.ts`：Manifest 声明 dependencies / provides / types；递归依赖、循环检测、稳定加载序；不是给核心写死一个导入列表 |
| 贡献聚合 | `config/loader.ts` 的 11 种提供面：backend、config、constants、driver、game、main、processor、schema、storage、terrain、test；生成聚合模块和类型声明 |
| 数据模型 | `engine/schema/index.ts` 的 registerStruct / registerVariant / registerEnumerated：可以给已有对象加字段、加入新对象变体、扩展资源等枚举，并生成读写与升级器 |
| 玩家 API | `game/symbols.ts`：全局类和变量注册、Game/Room 初始化、runtimeConnector.initialize/receive/send |
| 宿主与沙箱传输 | `engine/runner/symbols.ts`：runnerConnector.initialize/refresh/save、runnerWorker；Memory 功能本身经此连接 |
| 指令 | `engine/processor/index.ts`：接收类型 + 指令名 + 参数 + before/after/type/internal 约束；不是封闭的核心动作枚举 |
| 对象行为 | 同文件的 registerObjectPreTickProcessor / registerObjectTickProcessor：回调带 next，允许包裹之前的处理器，也可以不继续默认链 |
| 房间、分片 | `engine/processor/room.ts`、`shard.ts`：房间处理、分片启动初始化、分片 tick、周期执行 |
| 建造与对象生命周期 | `mods/classic/construction/game.ts` 的 registerBuildableStructure；`mods/classic/logistics/storage.ts` 展示新建筑的创建、放置条件、障碍与生命周期注册 |
| 持久化 | `engine/db/storage/register.ts`：按 URI scheme 与用途注册后端；`packages/redis/storage.ts` 提供真实 Redis 实现 |
| 业务域替换 | `utility/hook.ts` 的 makeProviderRegistration；`mods/meta/messages/model.ts` 可替换消息存储，`mods/meta/notifications/transport.ts` 可替换通知传输；同一槽位重复替换报冲突 |
| 网络与客户端数据 | `backend/symbols.ts` 的 route / subscription / middleware / roomSocket / mapStats / sendUserInfo / version；`backend/index.ts` 的对象、地图、地形渲染绑定；version 数据可贡献功能与菜单 |

一个可核对的纵向实例是 `mods/classic/chemistry/`：index 声明依赖和六类贡献，schema 注册矿物资源与动作记录，game 注册 Lab 对象，processor 注册反应和强化等指令，backend 提供渲染数据，test 提供验收。

另一个实例是 `mods/meta/stats/schema.ts`：直接给已有 Room schema 增加统计字段。**只支持“新增对象”而不支持“扩充已有对象和房间”仍然弱于参考项目。**

它也有边界：hooks 在被取用后禁止继续注册；schema 在组装后关闭；本次未发现通用卸载与运行中热重载契约。无需把热重载作为官方等价目标的起步前提。其指令实现接受顺序约束，但不能据此推断已经提供完善的全局顺序冲突检测；下面建议的图校验属于我方应定义的契约。

## 4. 当前项目的架构与实际阻塞点

### 4.1 有利基础已经存在

- crate 分层让 schema、世界、结算、沙箱、协调器、存储和网络的职责相对清楚，能确定每类贡献应该放在哪里。
- `Sandbox`、`PlayerExecutor`、`WorldSource` 提供了执行和读取的抽象边界，便于测试和未来替换实现。
- `Working` 持有结算工作副本与空间索引，并有对象插入、删除和移动辅助方法，可以演进为 Mod 事务上下文。
- `TickInput` 显式携带墙钟输入；协调器缓存失败轮 RunResult，重试不重跑玩家代码；这为确定性 Mod 处理提供基础。
- Generic 能携带额外 JSON，命名资源栈也已有载荷空间，可以作为扩展数据的过渡基础。
- 前端设计已明确保留服务端下发渲染元数据的扩展点，方向符合需求。

这些都是改造的有利条件。它们尚未组成第三方 Mod 的公共契约：例如 EmbedEngine 使用自己的具体 World，WorldSource 的存在并不等于已支持注册世界后端。

### 4.2 七个必须补齐的边界

| 边界 | 当前源码事实 | 对 Mod 的影响 |
|---|---|---|
| 对象与 schema | [room.rs](../../crates/kk-schema/src/room.rs) L399 起声明封闭 RoomObject；tag 是固定 u8；[schema 宏](../../crates/kk-schema/src/lib.rs)展开固定 enum 和 match | 外部 crate 可以声明自己的 schema，却不能把变体追加到既有 RoomObject；已有 Creep/Room/Player 字段也无贡献入口 |
| 指令协议 | [intent.rs](../../crates/kk-engine/src/intent.rs) L23 的 Action、L335 的 RoomIntent、L447 的 MarketIntent 均为固定类型；L495 起的生产清洗丢弃未知动作 | 即使给玩家类加一个方法，也没有合法的新动作通道；参数、冲突关系、执行器无法独立注册 |
| 行为与默认规则 | [settle/mod.rs](../../crates/kk-engine/src/settle/mod.rs) L486 起固定阶段与类型分派；[creep/mod.rs](../../crates/kk-engine/src/settle/creep/mod.rs) L51 起固定动作 match；[structures.rs](../../crates/kk-engine/src/settle/structures.rs) L58 起只识别已写入的结构类型 | 没有新增处理器、包裹默认处理、跳过默认处理、替换规则或声明阶段依赖的公开机制 |
| 玩家 API 与投影 | [sandbox.rs](../../crates/kk-runner/src/sandbox.rs) L43 起的 PRELUDE_SEGMENTS 固定内嵌，L190 起只组装固定 extension；[_registry.js](../../crates/kk-runner/src/prelude/game/registry.js) 的类映射与捕获表固定；内部 __kkp 在启动后删除 | 有内部 assembler 表，却没有 Mod 注册和分发入口；不能独立安装自定义构造器、字段 getter、查询与收发通道 |
| 规则数据 | [types.rs](../../crates/kk-world/src/types.rs) L65 的建造成本、L73 的障碍白名单来自编译常量；[creep/store.rs](../../crates/kk-engine/src/settle/creep/store.rs) L22 等读静态资源表；[structures.js](../../crates/kk-runner/src/prelude/game/structures.js) L94 等捕获冻结副本 | 修改玩家全局常量不能改变 Rust 结算；新资源、新可建造建筑、规则倍率必须同时穿过多个硬编码判定 |
| 全域提交与持久化 | 目前房间工作副本、玩家候选、墙钟重试已有逻辑提交；[kk-storage](../../crates/kk-storage/src/lib.rs)仍为说明性占位，[snapshot 契约](../architecture/snapshot-contract.md)明确持久化和字节共享形态待实现 | 缺 Mod 命名空间存储、事务候选、全域检查点、schema 迁移和副作用交付；现有内存回退不能直接外推为 Mod 的磁盘恢复保证 |
| 服务端与客户端 | [kk-backend](../../crates/kk-backend/src/lib.rs)为占位，Cargo.toml 没有依赖；[game 包](../../apps/game/package.json)、[renderer 包](../../packages/renderer/package.json)为占位 | 尚无路由/认证/订阅/命令/任务/资产/图形/面板的可调用贡献机制；设计承诺尚未变为实现 |

`kk_room_object!` 宏减少了核心新增变体时的机械同步工作，它不会把 Rust 的封闭 enum 变成可由第三方包扩充的类型。`pub` 方法、Rust 的 `mod` 关键字，以及 prelude 文件拆分也不能当作插件系统已经存在的证据。

Generic 的注释有部分历史过渡表述：实际已有 tower/link/rampart/terminal 等手写结算分支。因此准确结论是“Generic 中部分已知类型由核心特殊处理，任意自定义类型没有注册处理器的机制”，不是“所有 Generic 都被忽略”。

### 4.3 实测：Generic 数据到玩家对象的链路没有打通

先执行 `cargo build -p kk-cli`，然后使用当前 debug 二进制创建临时内存世界：一个 `myobject(counter=7)` 和一个 `structureType=myfactory(counter=9)`。没有修改引擎代码，也没有安装任何 Mod。

| 观察 | 当前结果 |
|---|---|
| 管理侧 getObject | 两个对象保留自定义 counter 和原始 type/structureType |
| 玩家 Game.getObjectById | 两个 id 都能找到，但 counter 均为 undefined |
| 玩家对象构造器 | myobject 为 Object；myfactory 为通用 Structure |
| 玩家 MyObject | 不存在 |
| 玩家内部 __kkp | 不可访问，这是正常的隔离边界，应保持 |
| 玩家 Room.find(10000) / 管理侧 findInRoom(10000) | 都返回空数组 |
| 再经过六个 tick | myobject.counter 保持 7 |

最后一项不是说“放一个对象就应自动自增”：没有声明行为自然不会运行。它与固定结算分派、没有行为注册接口一起说明，Generic 本身只提供载荷，不能完成官方示例的自动 tick 行为。自定义类、字段和查询缺失则直接由探针观察到。原始结果另外记录了 Game.customObjects 不存在；官方的 customObjects 是内部 register 集合，并非普遍公开的 Game 属性，因此该记录不作为兼容差距证据。

复现脚本：[mod-extensibility-probe-2026-10-01.py](../../scripts/benchmark/research/mod-extensibility-probe-2026-10-01.py)。完整结果：[mod-extensibility-probe-2026-10-01.json](notes/mod-extensibility-probe-2026-10-01.json)。运行方式：

```sh
cargo build -p kk-cli
python3 scripts/benchmark/research/mod-extensibility-probe-2026-10-01.py
```

另外执行了 `cargo test -p kk-engine from_json`（7 通过）和 `cargo test -p kk-engine global_unknown_action_dropped_and_missing_fields_none`（1 通过）。现有对抗测试明确保证未知对象动作 `explode` 被丢弃；全局未知动作同样被丢弃。**不能为 Mod 简单取消清洗：必须由可信注册表扩充合法动作，而继续拒绝未注册动作。**

## 5. 与需求逐项对照

下表的“未提供”指独立 Mod 接口未提供，不表示编辑核心源码也做不到。源码重编能力与 Mod 扩展能力分开评估。

| 所需能力 | 官方 / 本地 xxscreeps | 当前 kkscreeps | 补齐层面 |
|---|---|---|---|
| 包发现、依赖、启停、版本、配置、类型声明 | 官方模块加载；xxs 有 Manifest 与贡献聚合 | ADR 有方向；实现未提供 | 宿主注册与组装 |
| 管理命令、定时任务、统计、通知、历史 | 有真实入口与示例 | 部分引擎数据已有；Mod 入口未提供 | 运行宿主、网络、存储 |
| 修改数值、资源集合、建造表、CPU 配额 | 可配置/可贡献 | 静态规则与单个沙箱配置；统一覆盖未提供 | RuleSet / Catalog |
| 新对象、已有对象/房间/用户附加字段 | 官方动态记录；xxs schema 贡献 | Generic 载荷部分支持；公共 schema 扩展未提供 | schema、世界、检查点 |
| 新玩家类、属性、原型、全局 API | 支持 | 原型可由玩家补丁修改；服务器 Mod API 未提供 | runner 安装与投影 |
| 自定义 FIND、LOOK、Game 集合、视野 | 支持或由玩法模块定义 | 固定查询/类/视野判定 | 对象语义注册 |
| 新对象/房间/世界指令、参数与冲突 | 支持 | 固定枚举；未知名丢弃 | 意图描述与调度 |
| 取消、包裹、替换默认对象行为 | 支持 | 固定 Rust 函数调用 | Handler / Provider |
| 新可建造建筑、资源、战斗与物流交互 | 官方可修改；xxs 有建造注册与资源贡献 | named store / Generic 有局部基础；交互规则硬编码 | 领域能力与规则注册 |
| 房间/世界 tick、NPC、世界生成、跨房逻辑 | 有事件/提供面 | 固定结算与受限构造接口 | 生命周期与世界事务 |
| 自有数据持久化、替换底层或业务域存储 | 官方可替换；xxs 有实际后端和 provider | 存储 crate 占位；仅基准候选 | 存储与业务域 provider |
| HTTP、WS、认证、中间件与静态资产 | 有 | backend 占位 | Backend 注册 |
| 图形、动画、文字、对象详情、菜单 | 官方示例；xxs 后端 render/version 贡献 | 前端设计保留元数据；实现占位 | 客户端贡献协议 |
| 默认玩法也能被拆成同等模块 | xxs 的核心玩法主要由 Mod 组装 | crate/文件分层，但玩法由核心固定组装 | 内建模块使用同一注册体系 |

所以当前可以说“架构具有改造基础”，不能说“Mod 扩展性已与官方基本一致”，也不能用 vanilla 行为测试通过率代替 Mod 能力验收。

## 6. 建议的目标架构

### 6.1 两种实现语言使用同一组贡献契约

保留 ADR-007 的两层方向，但避免把它理解为两个能力互不相通的系统：

| 形态 | 建议用途与边界 |
|---|---|
| Rust 编译期模块 | 热路径、复杂原生算法、低层存储与网络 provider；外部 crate 通过公共注册接口参与组装，需重新构建；不要求动态 Rust ABI |
| 可信 JS Mod 宿主 | 管理与外部服务，也能声明对象、动作、字段、处理器和规则替换；安装包并重启生效；与玩家 isolate 分离；通过明确宿主服务操作候选状态 |
| 玩家 API 贡献 | 安装到每个玩家沙箱的类、getter、方法、常量与接收/发送逻辑；这是 Mod 的客户端代码面，不携带服务器管理权限 |
| Web 客户端贡献 | 图形元数据、资产、详情字段及需要时的界面模块；经握手发布当前 Mod 能力和版本 |

**建议 JS 与 Rust 对象、意图和状态贡献进入同一注册表。Rust 是性能选项；若所有新玩法都必须改核心或写 Rust 重编，运行时 JS 层仍达不到官方安装式 Mod 的能力口径。** 采用 deno_core 不会自动提供 Node 的文件、网络、npm 或子进程能力；这些须成为可信 Mod 宿主的明确服务或原生 provider。无需为旧 Mod 实现整套 Node 兼容。

### 6.2 注册、冻结与组装

每个包声明 Mod ID、版本、引擎 API 范围、依赖、冲突、配置 schema、贡献面和数据迁移。启动流程是解析包 → 校验依赖与冲突 → 汇总注册 → 校验顺序 → 冻结规则/schema → 安装沙箱和客户端贡献 → 运行。

至少需要以下贡献类型，名称只是设计草案：

| 注册面 | 必须包含的语义 |
|---|---|
| ObjectType / ObjectExtension | 新类型；扩展已有类型；字段 schema 和版本；创建、销毁、迁移；所有权、可见性、查询、通行、受击、Store 等能力 |
| IntentDescriptor | 对象/房间/世界作用域；参数 schema；宿主清洗与授权；覆盖/队列行为；计费；冲突组；执行阶段与顺序；处理器 |
| RuleSet / Catalog | 数值覆盖、资源、建造、RCL 配额、障碍与领域规则；从同一有效配置生成宿主判定和玩家常量 |
| Lifecycle / Handler / Provider | 观察；前后处理；带 next 的包裹；取消默认处理；明确替换一个默认实现；初始化和关闭 |
| RuntimeConnector | 初始化、tick 输入和输出、自有玩家持久通道、注入 JS 片段和类型定义；构造器捕获前完成安装 |
| Storage / Backend / Client | 自有命名空间与迁移；底层及业务域实现替换；路由、认证、订阅、命令、任务、图形、资产、面板与能力握手 |

不要把全部贡献统一成随意的 `priority: number`：依赖与 before/after 应形成可检查的顺序关系；循环与矛盾启动失败；同一替换槽位多个所有者应报冲突或要求配置显式选择。两个 Mod 包裹同一行为时，应有稳定的顺序和清楚的 next 语义。

### 6.3 保留内建类型的高效表示，增加真正开放的扩展表示

不必把全部原生对象改成动态 JSON，也不必马上把整个世界改成 ECS。建议：

1. 保留内建 typed 变体和默认规则的快速路径。
2. 为新对象建立扩展包络：稳定的命名空间类型键、数据版本、全局 id/坐标、受校验载荷。可以从 Generic 演进，但要正式加入 descriptor、处理器与投影注册。
3. 为已有对象、Room、Player、世界提供命名空间附加字段/组件，按需要访问；对象生命周期负责附加数据清理，检查点负责一致保存。仅建立新对象包络不足以实现 xxs 的 Room 统计字段贡献。
4. 快照记录激活 Mod 清单、类型映射、版本和内容指纹。扩展键不能依赖本轮偶然加载序，也不能要求每个 Mod 去抢核心 u8 tag。
5. 定义缺失 Mod 的处理策略：必要行为缺失时明确拒绝启动，或显式进入只读恢复；不能静默丢字段、把应处理的对象变成无行为对象。卸载需要依赖检查和数据迁移。
6. 未来字节快照为扩展载荷保留目录和 arena/类型桶约定；玩家包装器仍按 id + 当前代际定位，遵守现有可见性规则。是否列式优化由实际负载决定。

具体障碍是“自定义对象如何参加现有游戏规则”：能保存一个新建筑还不够，它是否可建造、会不会挡路、能否被攻击维修、能否接受新资源、是否授予视野，都需要明确的能力描述和必要的规则覆盖。这些不能继续由多个核心字符串白名单各自决定。

### 6.4 指令需要开放通道，同时保持可信清洗

保留官方动作的静态快路径，增加命名空间扩展动作 ID 和经 descriptor 校验的参数通道。对每个注册动作同时组装：玩家方法 → 收集器 → RunResult → 宿主清洗 → 授权与对象能力判断 → 顺序与冲突调度 → 候选状态变更。

玩家预检查用于返回正确结果，宿主检查必须独立成立；玩家可以改原型和伪造输入。计费、同动作覆盖和多动作冲突都要在定义中写清楚。不能只在 prelude 中新增方法而遗漏宿主清洗，也不能开放一个跳过授权的通用 JSON 动作入口。

规则覆盖同样必须两侧一致：例如可信 Mod 扩展资源表后，Rust transfer/withdraw 与玩家 Store/动作预检查要消费同一有效 Catalog。当前玩家私改 RESOURCES_ALL 不生效是正确的隔离行为；可信 Mod 应在冻结前通过专用注册修改有效规则。

### 6.5 Mod 状态必须参与同一轮事务

```mermaid
flowchart LR
    A[已提交世界和玩家状态及 Mod 状态] --> B[玩家与 Mod 输入采集]
    B --> C[RunResult 和可重放 TickInput]
    C --> D[房间规则与 Mod 候选处理]
    D --> E[跨房全局规则与 Mod 候选处理]
    E --> F[校验并统一提交全部域]
    F --> A
    F --> G[提交后通知和外部副作用]
```

建议明确 TickTxn 覆盖房间、玩家、市场、全局 ID、Mod 存储和待交付事件。所有可失败的规则处理完成后再发布。当前 `settle_market` 直接操作 `self.market`，之后没有会返回结算错误的 Mod 钩子；**将来若在它后面加入可失败处理器，就必须先把市场也纳入候选事务，不能沿用现状然后宣称完整回退。** 这是新扩展的设计约束，不是本次证明的现有失败。

重试从同一已提交 Mod 状态及同一 TickInput 重算候选，或复用候选结果；不能依赖已经递增的 Mod isolate 全局变量。超时/异常要有明确停顿或禁用策略，失败不提交部分房间或 Mod 数据，重建宿主也从已提交状态恢复。

数据库写入、通知、HTTP 调用等副作用应由提交后的事件/任务执行。需要可靠交付时采用持久 outbox 与幂等键；这提供可重试交付，不承诺外部系统无条件 exactly-once。定时任务、管理员命令和外部响应若会改变玩法，应分配到 tick 边界并记录为输入；观察与统计可在提交后运行。

这部分可以保留比官方内部事件更明确的事务纪律，并不降低“能做相同玩法”的能力。若允许可信 Mod 任意即刻执行外部副作用，应把这种模式与确定性重放的保证边界写清楚。

### 6.6 网络与客户端也需要闭环

前端已有元数据扩展方向，应在 M3 正式协议中补充 Mod 能力握手、对象增量字段、图形/动画/文字、动态资产地址和详情面板 schema。嵌入默认 Web 客户端不应迫使每个换贴图的 Mod 重编服务器。

原生 HTTP 路由与 JS 回调可以通过后台宿主调度器连接；需要同时覆盖认证、订阅、管理命令、周期任务和自有存储。复杂界面是否加载扩展模块可以后续分批完成，但官方对象图形和详情面板样例应进入最小闭环验收。

“官方客户端能连接 vanilla 世界”与“官方客户端能显示 Mod 的自定义内容”是两个验收面。如果后者也在目标内，须适配官方 customObjectTypes/renderer metadata 等客户端扩展协议；仅让自建客户端显示新对象不能证明此项达标。Mod API 自建不妨碍单独实现这层客户端协议适配。

## 7. 对现有决策与排期的影响

[ADR-007](../decisions/ADR-007-mod-system.md)的“不兼容官方 Mod”仍可成立。需要补强的是能力承诺：目前写“保证将来能基于它开发 mod 即可”，把 JS 层定位为管理、统计、轻量玩法，没有写官方能力覆盖清单，也没有把它施加到当前玩法架构。

建议在后续正式决策中明确三点：

1. 不追求旧 Mod 代码和 Node/数据库兼容；追求官方同类 Mod 功能可实现，以上矩阵成为验收面。
2. Rust 与可信 JS 共享贡献模型；JS 可声明新对象、动作和规则替换，Rust 保留复杂原生服务与性能实现。
3. [ADR-003](../decisions/ADR-003-player-compatibility.md)的玩家零差异约束用于默认 vanilla 配置；服主显式启用的 Mod 可以改变玩法，并在握手和配置中声明。需要保持 vanilla 基线与 Mod 专用验收分别成立。

**建议把核心扩展契约从 M4 前移；完整管理界面、插件分发和运营工具仍可留在 M4。** 不应为调研直接重写旧 ADR，本报告也没有做这一变更。

| 顺序 | 交付范围 | 可评审的完成条件 |
|---|---|---|
| 第一步：当前玩法继续扩面前 | 定义 Manifest、注册/冻结、Object/Intent/RuleSet/Handler、事务候选；实现可信 JS 宿主的最小路径 | 外部包注册一个 counter 对象与 increment 动作；新增/包裹/替换都走公共 API；核心不用加该对象的 match 分支 |
| 第二步：同批验证可替换性 | 将现有一个 Generic 结构家族迁移成内建贡献；选 rampart 或 link 等现有路径，默认处理挂同一注册体系 | 外部 Mod 能包裹或替换其行为；默认配置保住棘轮；失败回退和双玩家权限验证通过 |
| 第三步：首次存储格式冻结前 | 扩展字段、命名空间状态、版本/指纹、迁移、候选全域提交 | 自定义对象及已有对象附加字段往返不丢；缺失/升级 Mod 有显式行为；检查点和重试一致 |
| 第四步：M3 实现过程中 | 路由/认证/订阅/资产/图形/面板及 handshake；生成沙箱和编辑器类型 | 自定义对象服务器、玩家脚本、客户端三面可用；扩展数据按权限投影 |
| 第五步：M4 完善 | 管理命令、任务、存储/业务域 provider、世界生成与复杂玩法、分发工具 | 官方示例能力组和自建复杂 Mod 全覆盖；原生深层模块不需要修改核心业务实现 |

上述顺序是减少返工的依赖排序，不是工期估算。没有必要一次把整个引擎重写成插件，也没有依据保证加几个 trait 就能完成。先用纵向样本迫使关键层开放，再迁移内建功能，是更可验证的路径。

## 8. 达标验收：用真实 Mod 证明能力

| 样本 | 必须证明的能力 |
|---|---|
| 计数器对象（官方同类示例） | 外部包注册 schema、玩家构造器与 counter getter、自定义 FIND/LOOK、increment 指令、每 tick 行为、图形、文字和详情；玩家输入不能越权 |
| 修改已有规则 | 包裹或替换现有结构/对象处理，显式跳过默认逻辑；修改一个规则数值后玩家预检和宿主结算一致；移除 Mod 后默认行为恢复 |
| 已有对象与 Room 字段扩展 | 不修改核心 struct 即增加字段；多玩家视野过滤；死亡/删除清理；快照与升级不丢数据 |
| 新建筑与新资源 | 可放置、可建造、有正确 RCL/障碍/容量；Creep 转移、攻击、修理、销毁与物流按声明工作；新资源不被旧白名单误丢 |
| 管理、通知与历史 | 注册命令、任务、路由、WS；提交后通知/统计；自有持久命名空间；可替换通知和历史存储 |
| 跨房与全局 | 一个 Mod 影响两房或市场；第二房/全局处理失败时世界、玩家、市场、Mod 状态一起回退；重试无重复提交 |
| 存储 provider | 外部模块替换默认后端或一个业务域存储；原子性和恢复语义不下降，不只写一个数据库 trait |
| 组合与生命周期 | 依赖、顺序、字段冲突、替换槽位冲突、缺失 Mod、数据迁移均可诊断；配置开关有效；默认配置的官方行为基线不退化 |
| 性能 | 无 Mod、少量简单 JS Mod、大量对象 Mod 三档测量；区分投影、跨 isolate 传输、处理和提交耗时；按类型/房间批量调用，避免每字段一次宿主往返 |

验收时，至少计数器和已有规则替换由独立 JS 包完成，新对象新增不得要求编辑核心 enum 或分派；同时有一个独立 Rust 模块验证原生注册通路。若只有维护者改完核心后打包成“Mod”，这项验收不成立。

## 9. 结论的证据强度与限制

- **源码确认**：两份目标项目的注册/组装方式、封闭类型和分派、占位组件、ADR 与排期差距。
- **实测确认**：当前 Generic 的管理查询与玩家字段差异、自定义 FIND/类缺失、原有意图清洗测试；结果已存档。
- **架构推断与建议**：前移扩展边界可减少返工；共享贡献契约、扩展包络、候选事务、JS 批处理能作为实现路线。这些尚未由完整 Mod 原型或性能基准证明。
- 没有启动 xxscreeps/官方完整服务器，也没有运行本项目完整行为套件；本次没有玩法代码变更，定向检查足以复核所述当前边界，但不构成全部 Mod 或全部游戏语义的验收。

当前最该调整的是“把 Mod 当作 M4 外加设施”的安排：让后续玩法从现在起接受公共贡献，再用官方纵向示例和规则替换 Mod 验证。项目的核心技术方向可以保留；需要补的是贯穿模型、规则、沙箱、事务、存储和客户端的扩展能力。
