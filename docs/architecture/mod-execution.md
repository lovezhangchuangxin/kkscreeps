# Mod 处理链执行

当前支持 link transfer/tick、creep transfer/withdraw 与对象域 Mod 意图。冻结计划决定处理器顺序，Rust 候选保存权威数据，可信 Mod realm 保存 JS 函数。玩家 VM 与玩法 realm 分离；管理员安装的 Mod 共用 realm，Mod 之间没有安全隔离。

Generic 仓库另已开放 10 条容量数值规则，贯穿放置、建造、实验室槽切换、终端 tick 与标准玩家常量，见[仓库容量规则](store-rules.md)。这组消费者不增加动作处理槽，默认路径仍是 Rust 结算。

## 两条执行路径

没有扩展的内建槽保留 `Native` 直调，不进入 JS。包含 JS 贡献的槽和 Mod 自有动作槽交给 `SlotExecutor` 整槽执行：runtime 一次进入 realm，JS 按冻结顺序运行 before、wrap、中心、after。中心是 JS process/replace，或一个只修改候选的 Rust native 步骤。creep 资源槽的只读字段、装配与能力范围见[资源动作槽](creep-resource-slots.md)。

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

处理器自身正常处理的错误不自动变成故障；以上不可吞掉的错误来自 next 与宿主候选访问边界。正常拒绝已开放于 Mod 自有对象动作和三个内建资源动作，范围与局部回滚见下节。

## 正常动作拒绝

`SlotOutcome` 区分 `Completed` 与 `Declined { binding, code }`；异常仍由 Result 的 Err 返回。Mod 自有对象动作的 before/wrap/process/replace/after，以及内建 `builtin:link.transfer`、`builtin:creep.transfer`、`builtin:creep.withdraw` 的 before/wrap/replace/after，可以 `return api.decline('full')`，也可导入 `@kkscreeps/mod` 的 `decline` 工厂。代码为 1..128 字节的 ASCII 字母、数字或 `._:-`。只有工厂产生的返回值表示拒绝；普通 `{code:...}`、返回码等不改变原有语义，throw 拒绝标记仍是异常，Promise/thenable 仍是故障。

拒绝中止本次动作的剩余处理器；已经执行的 before、内层中心与 after 写入也一起撤销。wrap 即使捕获内层拒绝，也不能继续回写；该 wrap 后续抛出异常、或已有候选/转换故障时，异常优先，整轮失败，不能用拒绝掩盖故障。wrap 不调用 next 的主动取消仍是 Completed，保持原来执行 after 的约定。

runtime 的自有动作适配器在调用前只保存目标对象字段，所有参与 namespace 的 state 在本次调用内暂存；正常完成后移交 state。拒绝时丢弃暂存 state，通过 `update_mod_fields` 恢复对象字段、信封坐标和位置索引，保留本轮之前已完成动作的结果。当前自有动作没有 native/ID/玩家/事件写面，因此这个边界足以撤销全部写入，不克隆整个房间。

三个内建资源动作的局部边界由引擎包住整个 Hosted 链：开始前保存执行者、目标对象原始记录和事件长度；拒绝后恢复记录并截去本次新增事件。自指只保存一份记录；保存的是本次动作开始时的候选，因此共享目标上此前成功动作的入账不会被撤销。runtime 将所有参与 namespace 的 state 暂存到正常完成，拒绝时丢弃。当前 native 只改库存、link 冷却、实验室槽容量并追加传输事件；JS 只可改 link cooldown，creep 字段只读。没有 ID、位置、玩家、市场或其他工作队列写入。以后扩大写面必须同步扩大恢复边界。无扩展的 Native 路径不保存快照、不进入 JS。

```js
export default function register(api) {
  api.handlers.wrap('builtin:link.transfer', { id: 'quota' }, (ctx, next) => {
    const used = ctx.state?.used || 0;
    if (used + ctx.args.amount > 100) return api.decline('quota');
    next();
    ctx.state = { used: used + ctx.args.amount };
  });
}
```

上例按请求数量累计本 Mod namespace 的配额（不代表 native 实际转入量），也可在 `next()` 后拒绝并撤销 native。拒绝后继续其他动作及每轮 tick，并正常发布世界、Memory 与 CPU 候选，保留本轮玩家执行费用；不重建健康 Mod VM。JS 全局变量不在局部恢复范围内，权威玩法状态须放在 `ctx.state`。拒绝代码与实际绑定保留在整槽宿主的类型化结局中，当前不额外发射玩家事件，也不追改玩家此前的 `transferEnergy()` 返回码。

`builtin:link.tick` 仍显式拒绝使用 decline，其他内建资源动作尚未开放可拒绝处理槽。纯 Rust callback 仍使用原成功/异常签名，正常拒绝目前由整槽宿主提供。直接使用 ModRealm 的可信宿主须自行恢复候选；SlotExecutor 的自有动作须由宿主恢复全部写入，内建资源动作的对象/事件由引擎恢复，宿主负责 namespace 暂存且不得写出该边界。自有动作验证见[正常拒绝记录](../research/notes/mod-intent-decline-2026-10-04.md)，内建多对象验证见[link 拒绝记录](../research/notes/d6-link-decline-2026-10-05.md)与[creep 资源槽记录](../research/notes/d4-creep-resource-slots-2026-10-05.md)；执行来源的结构化记录见下节。

## 注册声明转换

声明 op 先检查当前注册包、密封状态与已留存故障，再转换载荷；注册尚未开始或已经结束时不访问载荷 getter/Proxy。该 op 与候选写入一样显式使用 `nofast, reentrant`，转换期间不持有 OpState/Registry 借用，允许 getter/Proxy 合法嵌套声明。

转换失败留存在 Registry；即使注册函数或模块求值代码捕获 op 错误，后续声明与外层嵌套声明仍失败。模块求值后、注册回调后都核验故障，不能用剩余声明冻结部分计划。失败清空待收集声明并密封 realm，重试必须新建实例。转换异常仅格式化 serde 错误，不读取 JS 异常属性或堆栈。证据见 [转换边界记录](../research/notes/mod-conversion-boundaries-2026-10-04.md)。

## 当前候选与上下文同步

每个 JS 处理器按绑定的 namespace 从 Rust 候选读取自己的 `ctx.state`，目标字段来自当前权威对象。不同 Mod 的 state 分开保存，目标字段共享同一个候选。

runtime 在一次整槽能力内缓存已解析的目标字段，供重复读取及无变更写入的比较使用。改写字段前失效，进入 native 前也失效；内层返回后的读取重新取得 native 修改后的候选。缓存不跨槽、对象或 tick，不缓存 namespace state；改变字段值仍走原有领域校验。JS 上下文仍按下面的同步边界读写和刷新。

`ChainHost::read` 返回类型化的 `ChainContext` / `ChainTarget` 固定封套，直接装配已拥有的 JSON 值。serde_v8 对封套走 struct 路径，使用静态成员键；args、state、fields 仍是开放 JSON 值。封套沿用原默认 JSON Map 键序（args/room/state/target/tick，target 内 fields/id），两条转换路径都创建空原型对象。缓存字段和固定 args 仍克隆出独立视图。写回保持动态 JSON 与领域校验，在目标字段校验成功后移走 op 交付的 state 值，不额外深克隆。两端仍经 serde_v8 转换，不共享 JS 与 Rust 的可变对象，也不省略同步边界。

调用 next 前，先校验并写入外层 state/目标字段；内层从新候选读取上下文。内层返回后刷新外层 ctx，随后外层可以继续修改。类型相同且没有共享引用的对象/数组保留容器身份，使 `const fields = ctx.target.fields`、`const store = fields.store` 等引用看见内层修改；类型改变时替换值，已经脱离上下文的旧容器引用不提供继续跟踪保证。处理器返回时再次校验写入。

权威候选是 JSON 树，不保存 JS 引用别名。外层若把同一容器挂在多个路径上，刷新按 state、args、target 的顺序遍历，成员按宿主投影的键序或数组索引序遍历：首次遇到的兼容路径复用旧容器，其余路径使用独立候选值。原先共享的路径可能不再相等，保存的旧引用跟随首次复用的路径；不能要求同一个旧容器同时跟随多个不同的候选值。该规则也覆盖 state 与目标字段之间的别名，避免把内层或 native 的更新覆盖回旧值。

刷新只复用自有数据属性，不读取继承成员或调用旧属性 getter；`__proto__` 等 JSON 数据键通过自有属性写入，不改变对象原型。ctx 与 target 记录只刷新宿主定义的成员，处理器附加的本地函数、缓存和循环引用保留；它们不会传给其他处理器或进入候选。写回仅序列化 state 和 target.fields，这两处仍须是可序列化的候选数据。

写入 op 先核验调用能力与绑定，再反序列化载荷。serde_v8 可执行载荷 getter，故该入口显式 `nofast, reentrant`；转换期间释放 OpState 和候选借用，getter 的嵌套 op 仍逐次核验能力。转换结束后重新检查故障；getter 内捕获的候选错误、转换本身失败都留存为整槽故障，阻止后续写回及候选发布。转换异常仅格式化 serde 错误，不读取 JS 异常属性或堆栈钩子。候选 host 方法与 native 中心仍只执行 Rust 逻辑。

写入宿主接收拥有的 `ChainWrite { state, target_fields }`，转换阶段不再为根和 target 构造临时 JSON Map；开放的 state/fields 仍为 Value。沿用动态 MapAccess 而非固定字段访问，保持自有键顺序、getter/Proxy 行为、undefined 省略及缺失成员语义。未知成员仍完整反序列化，不能用忽略成员的方式绕过其错误或深度限制。

固定版本 serde_v8 对 Symbol 的分类会 Rust panic。注册声明、整槽写入、直接处理器返回值、密封绑定清单和诊断探针结果统一经过同步转换的 unwind 边界，归一为转换错误；不格式化 panic 载荷，OpState 与候选借用在此之前已释放。整槽写入转换完成后仍重新检查故障，阻止 host 写回/after/发布；runtime 按候选失败流程销毁 realm。注册失败密封 realm，直接 invoke/probe 的普通转换错误返回 Err，由调用方处理；资源超限仍毒化 VM。此边界不覆盖其他 Rust panic、使用 panic=abort 的外部构建或进程崩溃。

转换中的 getter 同样处于整轮结算 CPU/墙钟预算内；死循环由原预算中断并毒化 realm。诊断计时不改变候选故障或预算处置：开启和关闭均覆盖嵌套读写与故障边界。

接收者、原意图参数和 native 函数由 Rust 的本次调用固定。修改 JS 的 `ctx.target.id` 或 `ctx.args` 不会重定向 native。当前不开放通过 next 替换参数或指定另一接收者。

Mod 对象整字段写入遵守类型描述符，位置字段经写入门同步对象信封与空间索引。内建 link 记录可读，当前只允许改写 `cooldown`（u32 整数，可删除恢复缺省）；身份、坐标、资源和其他字段只读。新的内建写面需经对应领域校验开放，不提供任意 Generic JSON 整写接口。

## 能力寿命与借用

本次调用的 `ChainHost` 拥有 RoomCtx 克隆，暂存于 OpState。它只在匹配调用标识时有效，允许的绑定集合来自冻结槽经停用掩码筛选后的执行链；native 中心最多执行一次，有效中心仍为 JS replace 时没有 native 能力。退出 JS 后，宿主无论成功或失败都取出并销毁能力，之后才能取回唯一拥有的房间候选。

RoomCtx 是 `Rc<RefCell<Working>>`。native/op 访问自行取得短借用，进入 JS 前不持候选借用。RefCell 只提供动态检查，不能宣称由类型证明整个借用纪律。当前普通 op 不回入 JS，也不重新借用 ModRealm，因此 wrap 嵌套不需要 reentrant op。

后续若 native 算法需要 JS 生命周期回调，应在回调边界拆分为可调度的 native 步骤；确实需要 native→JS 嵌套的独立接口仍可使用 op scope 与明确重入契约。当前决定不禁止所有重入，只移除贡献链不需要的回入。

## VM 故障恢复

`kk-runtime` 持有分片创建时的包字节、注册配置、冻结 PlanHash 和可执行绑定清单。整轮引擎结算返回错误时，立即销毁 Mod realm；不仅处理 JS 异常，也覆盖 Mod 已运行后的市场、校验与玩家写回故障。结算之前的测试拒绝未运行 Mod，不销毁健康 realm。成功 tick 继续复用 realm。

下次结算前重新建立 realm，从上述内存快照装载与注册，核对原 PlanHash 和绑定清单，全部通过才发布替代 VM。恢复不重读磁盘、不重新安装引擎计划或玩家 API，不修改已提交世界或玩家状态。注册失败、计划漂移或替代 VM 建立失败时保持空槽；玩家执行前也检查恢复门，不能消耗缓存候选或开始新的玩家轮。

`run_players` 仍复用原玩家结果和 TickInput；Mod 处理链在新 realm 中重新结算。`needs_mod_recovery` 独立于玩家缓存与引擎 NeedsRecovery；`rebuild_mod_realm` 可显式恢复而不处置这两类状态。`discard_pending_round` 仅放弃玩家候选与引擎恢复状态，不能解除 Mod 恢复门，相关世界/市场修改仍被拒。新建分片或 teardown 清空整个生命周期。

持久状态须走 ctx.state，模块全局只用于定义与可重建缓存；恢复从已提交 ctx.state 继续，丢弃全部 Mod 模块全局。重建并不让任意全局状态、Math.random 或墙钟依赖变成可回放输入，Mod 必须自行遵守确定性与缓存重建约束。此保证针对返回错误，不包括 panic、永久卡死、OOM 或进程崩溃。恢复与重试证据见 [VM 恢复记录](../research/notes/mod-vm-recovery-2026-10-03.md)。

## 执行故障诊断

`ModHostError.fault` 可携带 `ModFault { slot, binding, kind }`，原 context/message 与 Display 继续保留。kind 区分 handler、control、context、read、write_conversion、write、native、capability、resource 与 chain；来源由 SDK 调用位置及 Rust 候选边界填写，不解析异常消息中的处理器名。已安装链内的绑定才可上报；正常拒绝不产生故障记录。

ActiveChain 保留首个故障；内层错误经 next 传播、被 wrap 捕获后再次抛错，都不覆盖内层来源。读写与转换错误携带对应绑定；重复 next 属所属 wrap 的 control，JS 刷新上下文错误属 context。原生中心故障、非法绑定能力与无法定位的整槽错误不指定 Mod 绑定。预算 start/finish 的资源错误优先于普通调用错误，只记录正在调用的槽；预算由整轮累计、堆由共享 realm 使用，不能把触发超限时的 wrap 当作唯一责任 Mod。SDK 在读取异常属性之前先以备用文本认领来源并关闭候选访问，再允许补充一次诊断文本；Rust 已留存的故障与后续传播不再格式化异常。异常 message/toString 只执行一次，普通格式化抛错保留备用文本；向宿主抛出已取得的字符串，不再次访问原异常钩子。格式化中的资源超限仍由预算优先处理。

runtime 的 `last_mod_settlement_fault()` 返回最近一次实际结算中失败的 Mod 槽调用，包含 tick、房间、权威目标 ID 与完整宿主错误。在生命周期容器中保留这条记录，销毁、成功/失败重建、结算前拒绝均不清除；下一次实际引擎结算开始时清除，新故障再写入。未调用 Mod 的引擎错误、正常拒绝与成功结算不会留下旧 Mod 归因；新建分片/teardown 随容器清空。该接口是内存 Rust 诊断面，尚未接入 CLI 诊断协议或持久日志。

这些记录描述可信共享 VM 中的执行边界，不证明代码隔离、精确资源责任或主责 Mod。可信 Mod 可直接调用底层 op，上报绑定的成员校验不等价于不可伪造身份。自动策略仅在宿主明确配置阈值后使用这些可信来源，并排除共享资源与未知来源；它不提供恶意包隔离。显式停用与策略的边界见下文。测试与证据见 [故障来源记录](../research/notes/mod-fault-attribution-2026-10-04.md) 与 [边界复审修复](../research/notes/mod-boundary-review-2026-10-04.md)。

## 显式停用与回退

宿主 Rust API `EmbedEngine::disable_mod(id)` 在调用边界停用指定 Mod，并按 manifest dependencies 与 handler 的显式 depends_on 计算传递依赖者闭包，保守停用整个依赖 Mod。返回本次新增的 ID 集合，`disabled_mods()` 可读取累计集合。未知 ID 不改变状态；重复停用不销毁健康 VM。分片内只允许追加停用，重新启用须重建分片。CLI 命令和持久化事件尚未接入；下述自动策略复用相同停用机制。

停用集合存在 runtime 的生命周期容器中，与 realm、最近故障记录和玩家轮缓存分开。新增停用后先销毁 realm，沿原恢复门从完整包快照重新注册，核对原 PlanHash 与可执行绑定清单，再发布使用停用掩码的执行链。恢复失败保留停用集合、玩家缓存与原故障；可以再次恢复。失败候选不会因停用被直接发布，缓存玩家结果仍在新链上重新结算。

| 执行位置 | 所属 Mod 停用后的行为 |
|---|---|
| before / wrap / after | 从执行链与本次候选能力的绑定集合移除；剩余钩子维持原顺序 |
| replace，槽所属 Mod 仍启用 | 回退到槽声明的原始中心；builtin 槽恢复 native 能力，自有槽恢复原 process |
| 自有槽所属 Mod 已停用 | 丢弃该槽的动作，整个槽不调用任何钩子、process 或 native |
| 已声明对象、字段、规则、玩家 API | 保留原冻结目录与有效规则，不删除对象或 namespace 数据，不改写玩家常量 |

丢弃动作仍通过正常整轮发布提交玩家 Memory/CPU 候选，保留已发生的玩家执行费用；未新增逐意图收费或玩家拒绝事件。正常拒绝与停用丢弃都不产生 Mod 故障记录。所有 Mod 的模块求值与注册函数仍会执行；注册故障、共享资源超限、依赖全局状态的非确定性注册不能靠此机制恢复。这里只屏蔽可信包的结算执行，也不禁止诊断 `invoke` 或包间普通 JS 调用。停用状态尚无独立回放身份或输入日志，不能把保留 PlanHash 解释为执行策略也相同。验收与边界见 [Mod 显式停用记录](../research/notes/mod-disable-execution-2026-10-04.md)。

## 可配置自动停用

`RegisterOptions.fault_disable_threshold: Option<NonZeroU32>` 是 runtime 启动策略，默认 `None` 关闭自动停用；`NonZeroU32::new(3)` 表示连续三次可归责失败后停用。该策略不改变 PlanHash，重建保持原配置；直接使用 ModRealm 不执行此 runtime 策略。

仅在一次实际引擎结算返回、预算结束之后计数，每次失败至多记一次首个槽故障。handler、control、context、write_conversion、write 可计数，且绑定必须存在于启动 executable 清单、namespace 对应已安装且未停用的包。read、native、capability、resource、chain、缺绑定或未知绑定不计数；不根据异常文本推断来源。这里依赖可信 Mod 遵守 SDK 上报约定，成员校验不能证明不可伪造的身份。

连续计数存在 ModVm 容器中，不随 realm 或最近故障记录清除。相同 Mod 在相邻实际结算尝试中失败则累计（可以是不同绑定、不同对象或 tick）；另一 Mod 失败从 1 开始，成功发布或不可归责的实际结算失败清除计数。重建成功/失败、结算前拒绝和显式丢弃玩家轮本身不计数也不清除；正常拒绝不记故障，整轮成功时同样清除。计数采用饱和加法，默认关闭自动停用时仍保留诊断计数。

达到阈值后，本次调用继续返回失败，不重新发布失败候选，也不在 JS 调用期间修改链。runtime 停用来源 Mod 及依赖闭包，销毁 realm，返回错误明确包含新增停用 ID、连续次数和下一次重试说明。下一次调用沿恢复门重建链，再以原玩家结果和 TickInput 重新结算；未新增调用内部的无限重试或调度器。

`mod_fault_streak()` 返回当前序列；`last_mod_auto_disable()` 保留最近自动决定（触发故障含 tick/房间/目标/来源、次数、新增停用集合）。普通故障、成功结算和重建不擦除自动决定；新分片或 teardown 清空全部策略状态。该记录与每轮重置的 `last_mod_settlement_fault()` 分开；只保留最近一次决定，无无限增长事件队列。当前均为内存 Rust API，尚无 CLI 通知、持久输入事件或执行策略指纹。验收与范围见 [自动停用记录](../research/notes/mod-auto-disable-2026-10-04.md)。

双玩家、双房间的组合验收见[玩家与 Mod 恢复闭环](../research/notes/mod-multiplayer-recovery-2026-10-04.md)：JSON/KKPV 两种投影、串行/并发派发与反向玩家顺序下，第二房间处理器异常回滚两房世界和两名玩家状态；自动停用后的子集重试仍发布整个缓存轮。player.js 类方法、FIND/LOOK、字段裁切与归属校验经过实际玩家沙箱；晚期 Memory/segments 写回失败另验证全部候选回滚。这是有限场景的功能验收，不代表真实 bot、密集 Mod 性能或进程崩溃恢复已完成。

连续验收进一步覆盖恢复后的 24 轮执行，以及中途再次发生的 RawMemory/segments 写回失败。失败保留上一轮 console 与全部已提交状态，空请求重试仍提交两名玩家的缓存结果；后续正常轮核验 VM 序号、Memory、segments、通知和对象计数。`mod-player-probe` 提供独立房间内玩家/Mod/原生动作联合采集，逐轮核验工作量；范围与计时口径见[性能验收](performance-acceptance.md)，原始证据见[连续闭环记录](../research/notes/mod-player-workload-2026-10-04.md)。

## 资源预算

`RegisterOptions.vm_limits` 固定分片玩法 VM 的总配置，创建与恢复均以 `ModRealm::with_options` 使用同一份限额与可选诊断句柄。它不改变规则 PlanHash；恢复配置属于 runtime 私有启动快照。直接使用宿主时，构造限额与诊断句柄须与注册选项一致，注册失败仍密封 realm。没有诊断需求时仍可用 `new`/`with_limits` 构造。

默认注册预算为执行线程 CPU 500 ms、墙钟 2 s；每轮结算累计预算为 CPU 100 ms、墙钟 1 s；V8 已用堆加 V8 统计到的堆外内存限额为 128 MiB。CPU/墙钟限额可配置为正值且不超过一小时，内存为 16 MiB..=4 GiB。默认值是初始保护配置，密集 Mod 负载下需要重新测量与调节。

注册预算覆盖模块求值、注册回调、冻结和链安装。runtime 在每次实际结算前开始累计，结束后关闭该轮；不同房间、对象和槽共享额度。每次进入 Mod 宿主时扣除已使用 CPU/墙钟，仅调用期间的执行线程 CPU 和墙钟累计，其中包含同步 native op；两次调用之间的纯 Rust 规则不计入 Mod VM 额度。直接 `invoke`/`invoke_chain` 未开启整轮累计时使用单次限额；直接宿主调用方须用 begin/end_settlement 绑定轮次。诊断 `evaluate_probe` 的脚本、异常格式化和结果转换也使用结算预算；整轮累计开启时同样计入该轮，超限后不能继续使用探针绕过毒化门。

常驻看门狗每 1 ms 请求一次 V8 安全点检查，在执行线程读取 CPU 时钟及 V8 堆/堆外计数；每次返回还检查用量，避免短调用在监控间隔内逃过累计门。墙钟到期即留存故障，并持续检查/中断直到调用返回后解除武装。deno_core 的异常转换会临时取消 V8 终止状态，错误属性 getter 与格式化钩子仍可执行 Mod JS，该窗口不能撤掉预算。持续投递和 disarm 仍共用代次锁；玩家普通看门狗保持单次投递语义。近堆上限回调立即请求中断，并临时增加 16 MiB 余量用于传播终止。任何限额故障保持毒化标记，不能清除后复用 VM；整轮候选失败，随后沿上述销毁/重建路径处理。

每对象的预算 start/finish 与用量累计仍独立执行；看门狗另在同一代次锁内登记空闲/定时等待状态。已有定时唤醒不晚于新期限时，arm 更新期限与代次并复用等待；空闲等待或新期限更早时才通知线程。disarm 同步撤销期限/代次，线程可等到原定采样点再转为空闲；shutdown 始终通知并 join。这样连续短调用减少重复调度，监控周期与调用间 Rust 工作的计费边界保持原样。功能、兼容性与局部性能对照见[通知合并记录](../research/notes/mod-watchdog-notify-2026-10-04.md)。

检查与 V8 中断具有采样及调度延迟，不能保证恰在阈值字节/纳秒停止。Rust/native 阻塞不能由 V8 terminate 强制打断，须在返回后拒绝候选；单次巨大分配、V8 未统计的外部分配、Rust 包快照/候选与进程 RSS 不属于此内存计数的硬边界。不可中断原生调用、进程级 OOM、panic 与进程崩溃仍需后续进程隔离与持久化恢复。共享 realm 不提供逐 Mod 的精确资源账本。机制与故障验收见 [预算记录](../research/notes/mod-vm-budget-2026-10-03.md)。

## 尚未完成的范围

共享可信 VM 的故障半径仍是整分片；未显式停用的确定性异常或超限 Mod 在新 VM 中仍会失败。link tick、其他内建动作与纯 Rust callback 的正常拒绝、自动策略的 CLI/持久日志接线、逐 Mod 资源归因、进程级限额、多对象写 API、其他生命周期槽、持久化和 outbox 尚未完成。

默认路径与功能回归、真实混合链和失败候选分别验收。小对象链测试不代表密集 Mod 的成本；性能要求见 [性能验收](performance-acceptance.md)，本次选择与取舍见 [ADR-015](../decisions/ADR-015-mod-chain-execution.md)。
