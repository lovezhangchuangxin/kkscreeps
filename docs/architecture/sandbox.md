# 玩家代码沙箱

`kk-runner` 使用 deno_core 创建 V8 实例，执行玩家 CommonJS 模块和表达式，收集指令、Memory 与日志。玩家代码不可信；执行预算和堆限制用于约束资源消耗，不代表整个宿主进程已经具备完整的安全隔离。

选型依据见 [ADR-002](../decisions/ADR-002-deno-core-sandbox.md)。同属 V8 可以减少语言引擎差异，但版本、内建对象、模块加载和宿主 API 仍须分别验证，不能据此推导与官方完全一致。

## 实例与线程生命周期

`PlayerThreads` 为每名玩家创建独立线程和沙箱，跨 tick 保留 V8 全局变量与模块缓存。当前行为如下：

| 操作 | 对实例的影响 |
|---|---|
| 正常执行、普通脚本异常 | 继续复用沙箱；异常由运行结果报告 |
| 更新模块表 | 替换 Rust 模块数据并重建 require 缓存，不重建 V8；缓存重置失败不回滚已替换的模块表 |
| 超时或近堆上限中断 | 清理执行状态后继续复用实例，不自动重建整个沙箱 |
| `Game.cpu.halt()` | 中断本次执行、跳过写回并保留实例；当前实现不生成费用 |
| `evict_idle(duration)` | 按调用方指定的闲置时间回收线程；没有内置的“三分钟自动销毁”调度 |
| shutdown / teardown | 回收玩家线程和实例 |

闲置时间按最近一次命令发送记录，不等于最近一次游戏动作。V8 的 Enter/Exit 栈属于线程局部；同一线程内创建多个实例须满足销毁顺序约束，不同玩家线程之间没有全局逆序要求。

所有注册玩家是否每 tick 执行属于协调层策略。当前 `EmbedEngine::run_players` 只执行请求的玩家；该入口按可配置的上限并发派发玩家，结果收齐后才进入世界结算。调用链见[游戏循环](game-loop.md)。

## 执行预算与计费

默认配置的单位为毫秒：CPU limit 为 20，桶上限为 10000，单 tick 最大预算为 500，初始桶余额为 10000；调用方可通过 `SandboxConfig` 修改。

执行前余额非负时，预算为 `min(max_per_tick, limit + bucket)`。正常提交增加 limit，再扣除向上取整的用量。超时或近堆上限中断生成完整预算费用；主动 halt 的处理不同，不能将所有中断概括为“整份预算计费”。

余额为负时跳过玩家执行，并暂存回充候选。回充先比较原余额：低于 `−2×limit` 时恢复到该值，否则增加 limit。例如 limit=20、余额=−580 时，需要三次停跑回充才能到 0。世界结算成功后才提交费用或回充，丢弃候选不会改变已提交余额。

正常计量使用 macOS/Linux 的执行线程 CPU 时钟。`Game.cpu.getUsed()` 返回本轮世界注入起点以来的线程 CPU 毫秒数，再加指令费用；正常账单包含写回计算。排队、睡眠和其他线程 CPU 不计入，初值内存注入不在正常计费区间。沙箱创建时检查时钟可用性，其他平台明确报不支持；运行与写回在 `sandbox.rs`，每轮注入在 `sandbox/inject.rs`，玩家 CPU API 在 `prelude/runtime/shared.js`，完整模块边界见[源码组织](source-organization.md)。墙钟预算继续用于活性保护，超时仍计完整预算。

每个沙箱有常驻看门狗。更新期限、检查执行代次和投递 V8 终止请求共用互斥锁，防止旧代次在 disarm 返回后再次投递；下一次执行仍需清除已投递的终止标志。中断受调度与 V8 执行边界影响，不保证恰在某个墙钟瞬间生效。

共用看门狗在同一锁内登记工作线程的等待状态：空闲等待或新期限早于现有定时唤醒时通知，其他期限更新复用已有等待。解除武装撤销期限和代次，无须为此唤醒；退出始终通知并回收线程。普通玩家模式到期仍只投递一次，玩法预算模式则持续监控到解除武装。

## 内存与字符串限制

默认 V8 堆配置上限为 256 MiB。近堆上限回调请求终止，并临时扩大上限以便执行清理；随后按配置值请求恢复。V8 实际生效上限仍受当前堆状态影响，不能把配置值当作精确的进程内存上限。

TypedArray backing store、WebAssembly 线性内存和宿主分配不都受此堆限制覆盖。墙钟看门狗也不是内存总量限额；堆外分配治理和进程隔离尚需另行实现。

JavaScript 写入限制采用字符串 `length`，即 UTF-16 码元计数：RawMemory.set 上限为 `2×1024×1024`，segment 数据为 `100×1024`，活动 segment 最多 10 个。不能将这些值直接称为 UTF-8 字节上限；Memory 自动序列化也不具有与 RawMemory.set 完全相同的长度检查路径。

## 控制台输出与弃用告警

玩家 `console.*` 是收集型实现：经宿主 op 推入每 tick 缓冲（行数与字节上限封顶、超限仅提示一次），不直写进程 stdout。embed 协议 `captureConsoleLogs(handle)` 返回该玩家**最近一次完成结算的运行**的控制台行（仅文本、按发射序），随检查点保存。

弃用告警复刻官方 `register.deprecated`（engine game.js:113-120）：按消息文本在 tick 内去重、经 `console.log` 输出，重置挂在每 tick 装配（perTickReset）——同一文本跨 API 合并、下一拍重发。六个触发点与官方逐字同位：`Game.map.isRoomAvailable`/`getTerrainAt`（告警先于参数校验与 pos 对象解构）、`PathFinder.use(false)`（use(true) 静默）、`findPath`/`findPathTo`/`findClosestByPath` 的 `avoid`/`ignore`（告警后 `findPath`/`findPathTo` 置空选项、`findClosestByPath` 仅告警不置——选项本就不被新寻路消费，路径照常）、`renewCreep` 强化体（全部拒绝门之后、意图照常生效）。登记分歧：官方 `use(false)` 会把同 tick 后续寻路切到旧寻路器（旧分支真实执行 avoid/ignore 且不告警），kk 仅实现新寻路、不建模该开关；官方告警经调用时全局 console 解析（玩家重绑 console 会改道），kk 装配期捕获收集通道属有意加固。

## 世界读取与写回

当前协调器构造按玩家视野过滤的世界 JSON，再注入沙箱。JavaScript 对象包装器从当前原始对象表读取属性，部分 getter 按对象缓存；缓存条件和跨 tick 行为并不完全相同，不能承诺所有旧引用总能读取最新字段。

首次访问 Memory 时解析 RawMemory，并将结果保存在本轮 `_parsed`；写回可能执行玩家自定义 `toJSON`。序列化 Memory 后才检查 segment 写入与请求，因而 `toJSON` 中的 segment 修改可能在本轮生效。写回结果只是候选，实际提交由协调器负责。

目标是通过有权限的共享快照减少世界物化与复制，但不可变字节句柄、固定偏移 getter、市场增量传输和 Memory 局部重编码尚未完整实现。目标布局与生命周期见[快照契约](snapshot-contract.md)，收益须由[端到端性能验收](performance-acceptance.md)验证。

## 模块加载与原生调用

表达式入口采用独立的 `(code,module,exports)` 函数作用域直接 eval，`module.exports` 初始与 `exports` 同引用，且与上传 main 的导出无关；每次请求创建新的控制台模块，var/let/const 不跨请求保留，显式 global/globalThis 写入仍跨 tick 存活。求值器由 Function 在全局环境创建，无法捕获 host 的 gw/op/序列化闭包；module/exports 不添加到全局。直接 eval 使用玩家当前全局 eval，覆写它影响自家表达式求值。来源：锁定官方 driver 的 `runtime-driver.js::evalCode(returnValue)`；返回值仍使用宿主既有 JSON 契约，不采用官方的字符串化结果。

Embed 分片创建为每名玩家安装真实的空 `main.loop` 模块，供表达式场景 require 与缓存查询使用；显式 `set_player_modules` 整体替换默认表，不补造上传代码缺失的 main。控制台请求只求值表达式，不额外执行上传 loop；生产玩家任务的 loop 通道继续使用 `require('main').loop()`。模块源码随工作者检查点保存，冷恢复重新加载，V8 导出引用与控制台局部变量不持久化。

CommonJS 加载器按上传模块名加载，只去掉 `./` 前缀，不解析 `../` 或自动补 `.js`。缓存直接保存 exports；初始化期间使用哨兵，循环加载会抛错，不返回 Node 风格的半成品导出。模块执行失败删除其缓存后重抛，后续加载可再尝试。二进制模块返回 Buffer。

模块有效且入口符合缓存检查时可跨 tick 复用；每轮检查 require、timestamp、cache 和 main.loop，条件不满足则整体重建缓存。全局提供 lodash 3.10.1；Buffer 是项目实现的子集，不能据此承诺完整 Node 环境。

当前实例创建时执行 lodash、常量和 prelude 脚本，没有预生成的游戏运行时启动快照。玩家 `PathFinder.search` 经专用 op 同步调用 `kk-pathfinder` 原生搜索（地形随每 tick 世界注入，见 [寻路地形直通](../designs/pathfinder-terrain.md)）；roomCallback 已同步接线，完整契约见 [寻路回调设计](../designs/pathfinder-callback.md)。每次 `#[op2(reentrant)]` 调用在局部宿主保存回调、矩阵和异常，进入玩家函数前释放 OpState 借用；嵌套调用建立独立宿主。

原生算法持有固定 `Box<[Cell<u8>;2500]>` 的指针；宿主按 TypedArray 的 byteOffset/byteLength 取 backing store，每次玩家房间回调后重新同步所有此前矩阵，保留共享矩阵和嵌套修改的可见性。Backing store 在搜索结束前保持存活；可缩放缓冲收缩后不可读的矩阵区域清零为地形默认值，避免残留旧障碍或越界读取。Rust 不对可写 V8 内存建立不可变切片借用。原异常对象经 Global 句柄重新抛出；终止异常在 TryCatch 内重挂，原生主循环同时轮询真实 isolate 终止状态，不能转成成功空结果或可捕获普通错误。

Room 的 find/look、创建、寻路、事件日志及序列化方法安装在 Room.prototype，实例只持有公开数据属性；玩家包装原型会作用于当前和后续 tick 的实例。房间查询索引、出口缓存和事件缓冲由私有 WeakMap 按实例身份保存，原型方法的 call/apply 根据接收者取上下文，避免借用方法时串房；探测代理和原对象登记同一上下文。每 tick 重建实例及其缓存，冷恢复重建 V8 原型，不保存玩家包装函数。来源：官方 engine/src/game/rooms.js 的 Room.prototype 与 privateStore。

Flag 继承 RoomObject（锁定官方 engine 4.3.0 `flags.js:28`），即时创建和后续投影实例均能读取玩家安装的 RoomObject 原型 getter。Overmind 用 `id || name` 的 ref 补丁生成持久任务引用；冷恢复保留引用 Memory，由重新装载的玩家代码重建原型补丁。

`Room.findPath` 家族按实际 Room 身份保存每 tick、每组选项的基础矩阵；障碍、私有 rampart、自有障碍工地、道路及安全模式分支对齐官方。玩家 costCallback 收到克隆矩阵，返回 CostMatrix 时替换该次副本。可信 Mod 寻路成本槽位仍为目标设计，不能从玩家回调已接入推导 Mod 已可改寻路。

## 宿主与故障边界

Rust 捕获生命周期函数的私有 V8 句柄后删除临时 `__kkp`，并遮蔽 Deno 和 `__bootstrap`。当前没有向玩家提供文件系统、网络或进程操作 API；这只是宿主暴露面的约束，不等于操作系统级隔离。

生产 RunJob 使用派发时绑定的绝对期限，覆盖排队与整条命令；回收不无限等待，并检查回执实际完成时间。线程退出、命令超期均成为基础设施故障，协调器停止新派发并保留已确认结果。旧诊断与管理命令仍有阻塞 recv，未统一提供期限。期限也不能杀死在 Rust op 等原生区段永久卡死的线程，线程销毁仍需 join。外部适配器的 RPC 超时可以终止整个 embed 子进程，不能替代引擎内部的故障恢复协议。

面向公网的进程分组、系统调用限制、堆外内存治理和安全升级流程属于部署与安全设计，当前未完整接入。世界失败时 V8 全局状态不会回退，通信故障也不提供跨线程原子提交保证，详见 [tick 契约](tick-contract.md)。
