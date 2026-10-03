# Mod 处理链执行

当前支持 link transfer/tick 与对象域 Mod 意图。冻结计划决定处理器顺序，Rust 候选保存权威数据，可信 Mod realm 保存 JS 函数。玩家 VM 与玩法 realm 分离；管理员安装的 Mod 共用 realm，Mod 之间没有安全隔离。

## 两条执行路径

没有扩展的 link 槽保留 `Native` 直调，不进入 JS。包含 JS 贡献的槽和 Mod 自有动作槽交给 `SlotExecutor` 整槽执行：runtime 一次进入 realm，JS 按冻结顺序运行 before、wrap、中心、after。中心是 JS process/replace，或一个只修改候选的 Rust native 步骤。

```text
kk-engine：选择 Native 或整槽宿主
  ├─ Native：短借用 Working → Rust 默认规则 → 返回
  └─ 整槽宿主：一次进入 JS
       before → 外层 wrap → 内层 wrap → center → 内层退出 → 外层退出 → after
       JS next：本 VM 内的闭包
       native center：普通 op → 短借用 Working → 修改候选 → 返回 JS
```

`kk-engine` 不依赖 V8，提供冻结槽描述、权威接收者/参数、RoomCtx 与 native 函数。`kk-mod-host` 不依赖引擎，通过 `ChainHost` 读写候选；`kk-runtime` 实现领域校验与两者的适配。链描述在注册冻结后安装进 JS，运行时只传槽键和本次调用标识，不重新排序或序列化整条链。

Rust 自定义处理器仍可经原有纯 Rust 执行器安装。它与 JS 执行器遵守相同链语义；JS 不通过 Rust 续行闭包回入 realm。

## 组合与故障语义

- before 按冻结顺序执行；wrap 从外到内，中心为唯一 replace 或默认实现；after 使用冻结后的逆序，整条 wrap 退栈后执行一次。
- 不调用 next 取消内层，属于成功处理；整槽成功返回仍执行 after。这与已有 Rust 槽约定一致，不以“是否抵达中心”判断成功。
- next 仅在所属处理器的同步执行期有效，最多一次。重复调用、内层异常、候选写入失败均记录为本次调用故障；处理器捕获错误也不能把候选变为成功。
- 回调返回 Promise 或 thenable 属故障；失败链不执行 after。after 自身故障同样使整轮候选失败，已经发生的 native/JS 候选写入不发布。
- JS 每槽最多 64 层 wrap，注册时拒绝超限。该限制约束递归栈深度，不等于 CPU/内存预算。

处理器自身正常处理的错误不自动变成故障；以上不可吞掉的错误来自 next 与宿主候选访问边界。未来正常动作拒绝与异常故障的分型仍按 SDK 设计推进。

## 当前候选与上下文同步

每个 JS 处理器按绑定的 namespace 从 Rust 候选读取自己的 `ctx.state`，目标字段来自当前权威对象。不同 Mod 的 state 分开保存，目标字段共享同一个候选。

runtime 在一次整槽能力内缓存已解析的目标字段，供重复读取及无变更写入的比较使用。改写字段前失效，进入 native 前也失效；内层返回后的读取重新取得 native 修改后的候选。缓存不跨槽、对象或 tick，不缓存 namespace state；改变字段值仍走原有领域校验。JS 上下文仍按下面的同步边界读写和刷新。

`ChainHost::read` 返回类型化的 `ChainContext` / `ChainTarget` 固定封套，直接装配已拥有的 JSON 值。serde_v8 对封套走 struct 路径，使用静态成员键；args、state、fields 仍是开放 JSON 值。封套沿用原默认 JSON Map 键序（args/room/state/target/tick，target 内 fields/id），两条转换路径都创建空原型对象。缓存字段和固定 args 仍克隆出独立视图。写回保持动态 JSON 与领域校验，在目标字段校验成功后移走 op 交付的 state 值，不额外深克隆。两端仍经 serde_v8 转换，不共享 JS 与 Rust 的可变对象，也不省略同步边界。

调用 next 前，先校验并写入外层 state/目标字段；内层从新候选读取上下文。内层返回后刷新外层 ctx，随后外层可以继续修改。类型相同且没有共享引用的对象/数组保留容器身份，使 `const fields = ctx.target.fields`、`const store = fields.store` 等引用看见内层修改；类型改变时替换值，已经脱离上下文的旧容器引用不提供继续跟踪保证。处理器返回时再次校验写入。

权威候选是 JSON 树，不保存 JS 引用别名。外层若把同一容器挂在多个路径上，刷新按 state、args、target 的顺序遍历，成员按宿主投影的键序或数组索引序遍历：首次遇到的兼容路径复用旧容器，其余路径使用独立候选值。原先共享的路径可能不再相等，保存的旧引用跟随首次复用的路径；不能要求同一个旧容器同时跟随多个不同的候选值。该规则也覆盖 state 与目标字段之间的别名，避免把内层或 native 的更新覆盖回旧值。

刷新只复用自有数据属性，不读取继承成员或调用旧属性 getter；`__proto__` 等 JSON 数据键通过自有属性写入，不改变对象原型。ctx 与 target 记录只刷新宿主定义的成员，处理器附加的本地函数、缓存和循环引用保留；它们不会传给其他处理器或进入候选。写回仅序列化 state 和 target.fields，这两处仍须是可序列化的候选数据。

写入 op 先核验调用能力与绑定，再反序列化载荷。serde_v8 可执行载荷 getter，故该入口显式 `nofast, reentrant`；转换期间释放 OpState 和候选借用，getter 的嵌套 op 仍逐次核验能力。转换结束后重新检查故障；getter 内捕获的候选错误、转换本身失败都留存为整槽故障，阻止后续写回及候选发布。转换异常仅格式化 serde 错误，不读取 JS 异常属性或堆栈钩子。候选 host 方法与 native 中心仍只执行 Rust 逻辑。

转换中的 getter 同样处于整轮结算 CPU/墙钟预算内；死循环由原预算中断并毒化 realm。诊断计时不改变候选故障或预算处置：开启和关闭均覆盖嵌套读写与故障边界。

接收者、原意图参数和 native 函数由 Rust 的本次调用固定。修改 JS 的 `ctx.target.id` 或 `ctx.args` 不会重定向 native。当前不开放通过 next 替换参数或指定另一接收者。

Mod 对象整字段写入遵守类型描述符，位置字段经写入门同步对象信封与空间索引。内建 link 记录可读，当前只允许改写 `cooldown`（u32 整数，可删除恢复缺省）；身份、坐标、资源和其他字段只读。新的内建写面需经对应领域校验开放，不提供任意 Generic JSON 整写接口。

## 能力寿命与借用

本次调用的 `ChainHost` 拥有 RoomCtx 克隆，暂存于 OpState。它只在匹配调用标识时有效，允许的绑定集合来自当前冻结槽；native 中心最多执行一次，JS replace 槽没有 native 能力。退出 JS 后，宿主无论成功或失败都取出并销毁能力，之后才能取回唯一拥有的房间候选。

RoomCtx 是 `Rc<RefCell<Working>>`。native/op 访问自行取得短借用，进入 JS 前不持候选借用。RefCell 只提供动态检查，不能宣称由类型证明整个借用纪律。当前普通 op 不回入 JS，也不重新借用 ModRealm，因此 wrap 嵌套不需要 reentrant op。

后续若 native 算法需要 JS 生命周期回调，应在回调边界拆分为可调度的 native 步骤；确实需要 native→JS 嵌套的独立接口仍可使用 op scope 与明确重入契约。当前决定不禁止所有重入，只移除贡献链不需要的回入。

## VM 故障恢复

`kk-runtime` 持有分片创建时的包字节、注册配置、冻结 PlanHash 和可执行绑定清单。整轮引擎结算返回错误时，立即销毁 Mod realm；不仅处理 JS 异常，也覆盖 Mod 已运行后的市场、校验与玩家写回故障。结算之前的测试拒绝未运行 Mod，不销毁健康 realm。成功 tick 继续复用 realm。

下次结算前重新建立 realm，从上述内存快照装载与注册，核对原 PlanHash 和绑定清单，全部通过才发布替代 VM。恢复不重读磁盘、不重新安装引擎计划或玩家 API，不修改已提交世界或玩家状态。注册失败、计划漂移或替代 VM 建立失败时保持空槽；玩家执行前也检查恢复门，不能消耗缓存候选或开始新的玩家轮。

`run_players` 仍复用原玩家结果和 TickInput；Mod 处理链在新 realm 中重新结算。`needs_mod_recovery` 独立于玩家缓存与引擎 NeedsRecovery；`rebuild_mod_realm` 可显式恢复而不处置这两类状态。`discard_pending_round` 仅放弃玩家候选与引擎恢复状态，不能解除 Mod 恢复门，相关世界/市场修改仍被拒。新建分片或 teardown 清空整个生命周期。

持久状态须走 ctx.state，模块全局只用于定义与可重建缓存；恢复从已提交 ctx.state 继续，丢弃全部 Mod 模块全局。重建并不让任意全局状态、Math.random 或墙钟依赖变成可回放输入，Mod 必须自行遵守确定性与缓存重建约束。此保证针对返回错误，不包括 panic、永久卡死、OOM 或进程崩溃。恢复与重试证据见 [VM 恢复记录](../research/notes/mod-vm-recovery-2026-10-03.md)。

## 资源预算

`RegisterOptions.vm_limits` 固定分片玩法 VM 的总配置，创建与恢复均以 `ModRealm::with_options` 使用同一份限额与可选诊断句柄。它不改变规则 PlanHash；恢复配置属于 runtime 私有启动快照。直接使用宿主时，构造限额与诊断句柄须与注册选项一致，注册失败仍密封 realm。没有诊断需求时仍可用 `new`/`with_limits` 构造。

默认注册预算为执行线程 CPU 500 ms、墙钟 2 s；每轮结算累计预算为 CPU 100 ms、墙钟 1 s；V8 已用堆加 V8 统计到的堆外内存限额为 128 MiB。CPU/墙钟限额可配置为正值且不超过一小时，内存为 16 MiB..=4 GiB。默认值是初始保护配置，密集 Mod 负载下需要重新测量与调节。

注册预算覆盖模块求值、注册回调、冻结和链安装。runtime 在每次实际结算前开始累计，结束后关闭该轮；不同房间、对象和槽共享额度。每次进入 Mod 宿主时扣除已使用 CPU/墙钟，仅调用期间的执行线程 CPU 和墙钟累计，其中包含同步 native op；两次调用之间的纯 Rust 规则不计入 Mod VM 额度。直接 `invoke`/`invoke_chain` 未开启整轮累计时使用单次限额；直接宿主调用方须用 begin/end_settlement 绑定轮次。

常驻看门狗每 1 ms 请求一次 V8 安全点检查，在执行线程读取 CPU 时钟及 V8 堆/堆外计数；每次返回还检查用量，避免短调用在监控间隔内逃过累计门。墙钟到期即留存故障，并持续检查/中断直到调用返回后解除武装。deno_core 的异常转换会临时取消 V8 终止状态，错误属性 getter 与格式化钩子仍可执行 Mod JS，该窗口不能撤掉预算。持续投递和 disarm 仍共用代次锁；玩家普通看门狗保持单次投递语义。近堆上限回调立即请求中断，并临时增加 16 MiB 余量用于传播终止。任何限额故障保持毒化标记，不能清除后复用 VM；整轮候选失败，随后沿上述销毁/重建路径处理。

检查与 V8 中断具有采样及调度延迟，不能保证恰在阈值字节/纳秒停止。Rust/native 阻塞不能由 V8 terminate 强制打断，须在返回后拒绝候选；单次巨大分配、V8 未统计的外部分配、Rust 包快照/候选与进程 RSS 不属于此内存计数的硬边界。不可中断原生调用、进程级 OOM、panic 与进程崩溃仍需后续进程隔离与持久化恢复。共享 realm 不提供逐 Mod 的精确资源账本。机制与故障验收见 [预算记录](../research/notes/mod-vm-budget-2026-10-03.md)。

## 尚未完成的范围

共享可信 VM 的故障半径仍是整分片；确定性异常或超限的 Mod 在新 VM 中仍会失败。故障分型/停用策略、逐 Mod 资源归因、进程级限额、多对象写 API、其他生命周期槽、持久化和 outbox 尚未完成。

默认路径与功能回归、真实混合链和失败候选分别验收。小对象链测试不代表密集 Mod 的成本；性能要求见 [性能验收](performance-acceptance.md)，本次选择与取舍见 [ADR-015](../decisions/ADR-015-mod-chain-execution.md)。
