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

调用 next 前，先校验并写入外层 state/目标字段；内层从新候选读取上下文。内层返回后刷新外层 ctx，随后外层可以继续修改。类型相同且没有共享引用的对象/数组保留容器身份，使 `const fields = ctx.target.fields`、`const store = fields.store` 等引用看见内层修改；类型改变时替换值，已经脱离上下文的旧容器引用不提供继续跟踪保证。处理器返回时再次校验写入。

权威候选是 JSON 树，不保存 JS 引用别名。外层若把同一容器挂在多个路径上，刷新按 state、args、target 的顺序遍历，成员按宿主投影的键序或数组索引序遍历：首次遇到的兼容路径复用旧容器，其余路径使用独立候选值。原先共享的路径可能不再相等，保存的旧引用跟随首次复用的路径；不能要求同一个旧容器同时跟随多个不同的候选值。该规则也覆盖 state 与目标字段之间的别名，避免把内层或 native 的更新覆盖回旧值。

刷新只复用自有数据属性，不读取继承成员或调用旧属性 getter；`__proto__` 等 JSON 数据键通过自有属性写入，不改变对象原型。ctx 与 target 记录只刷新宿主定义的成员，处理器附加的本地函数、缓存和循环引用保留；它们不会传给其他处理器或进入候选。写回仅序列化 state 和 target.fields，这两处仍须是可序列化的候选数据。

接收者、原意图参数和 native 函数由 Rust 的本次调用固定。修改 JS 的 `ctx.target.id` 或 `ctx.args` 不会重定向 native。当前不开放通过 next 替换参数或指定另一接收者。

Mod 对象整字段写入遵守类型描述符，位置字段经写入门同步对象信封与空间索引。内建 link 记录可读，当前只允许改写 `cooldown`（u32 整数，可删除恢复缺省）；身份、坐标、资源和其他字段只读。新的内建写面需经对应领域校验开放，不提供任意 Generic JSON 整写接口。

## 能力寿命与借用

本次调用的 `ChainHost` 拥有 RoomCtx 克隆，暂存于 OpState。它只在匹配调用标识时有效，允许的绑定集合来自当前冻结槽；native 中心最多执行一次，JS replace 槽没有 native 能力。退出 JS 后，宿主无论成功或失败都取出并销毁能力，之后才能取回唯一拥有的房间候选。

RoomCtx 是 `Rc<RefCell<Working>>`。native/op 访问自行取得短借用，进入 JS 前不持候选借用。RefCell 只提供动态检查，不能宣称由类型证明整个借用纪律。当前普通 op 不回入 JS，也不重新借用 ModRealm，因此 wrap 嵌套不需要 reentrant op。

后续若 native 算法需要 JS 生命周期回调，应在回调边界拆分为可调度的 native 步骤；确实需要 native→JS 嵌套的独立接口仍可使用 op scope 与明确重入契约。当前决定不禁止所有重入，只移除贡献链不需要的回入。

## 尚未完成的范围

候选回滚不回滚 Mod 模块全局。持久状态须走 ctx.state，模块全局只用于定义与可重建缓存；当前结算重试仍复用 realm，未实现失败后自动重建。共享可信 VM 的故障半径仍是整分片，故障分型/停用策略、注册与结算资源预算、多对象写 API、其他生命周期槽、持久化和 outbox 尚未完成。

默认路径与功能回归、真实混合链和失败候选分别验收。小对象链测试不代表密集 Mod 的成本；性能要求见 [性能验收](performance-acceptance.md)，本次选择与取舍见 [ADR-015](../decisions/ADR-015-mod-chain-execution.md)。
