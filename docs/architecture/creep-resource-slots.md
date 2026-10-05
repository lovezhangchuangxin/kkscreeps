# Creep 资源动作槽

`builtin:creep.transfer` 与 `builtin:creep.withdraw` 已开放 JS before/wrap/replace/after 和正常拒绝。执行仍由原玩家 API 收集意图、原清洗器转换参数；原 `EXECUTION_ORDER`、同名覆盖、动作互斥及 creep 归属校验保持生效。动作派发改用短借用的 RoomCtx，进入 JS 前释放 Working 借用。

```js
export default function register(api) {
  api.handlers.wrap('builtin:creep.withdraw', { id: 'quota' }, (ctx, next) => {
    const used = ctx.state?.used || 0;
    if (used + ctx.args.amount > 100) return api.decline('quota');
    next();
    ctx.state = { used: used + ctx.args.amount };
  });
}
```

上例按请求数量累计 namespace 配额；native 会按实际库存、容量再次钳制，故请求数不等于实际提取量。Mod 也可在 `next()` 后拒绝，撤销已经发生的 native。正常拒绝中止剩余处理器，继续其他动作和每轮 tick。规则、玩家返回码、异常优先级与全域提交语义见[Mod 执行](mod-execution.md#正常动作拒绝)。

## 调用读面与写面

`ctx.target.id` 是执行 creep 的权威 ID；`ctx.args` 带目标 `id`、`amount`、`resourceType`。native 使用 Rust 绑定的原始参数，修改这些 JS 本地值不会改变接收者或 native 请求。`next()` 返回时刷新权威参数和字段，原 store 引用也会反映 native 的结果。

`ctx.target.fields` 是候选中的只读视图，包含 `name/owner/x/y/spawning/hits/fatigue/store/storeCapacity`。store 与玩家投影共用正量资源合并函数（保留 energy=0），capacity 用同一 CARRY/boost 计算。字段回写须与当前候选完全相同，非法修改属于领域写故障；namespace `ctx.state` 可正常读写并按动作暂存。目标资源对象没有任意字段写能力；受控库存读写通过独立的 [ctx.resources](mod-resources.md) 开放。

动作槽可实现观察、配额、准入、取消、拒绝以及 Mod 状态处理。后续资源领域片已允许 replace 通过 `ctx.resources` 自行转账、声明损耗和路由同房多个对象，见[资源能力](mod-resources.md)。完整 creep 对象/意图描述目录、可调整 native 参数的能力和其他领域写 API 仍待后续。

## 局部恢复边界

三个内建资源槽（link transfer、creep transfer/withdraw）共用引擎的整链边界：调用前保存执行者和请求目标的原始 RoomObject 记录（自指去重）以及事件长度。拒绝时恢复两份记录并截去本次新增事件，runtime 丢弃所有参与 namespace 的暂存 state，并通过首次触达日志恢复资源 API 额外访问的对象。此前动作的库存、事件与 Mod state，以及此前 drop 分配的 ID/资源对象仍保留；后续 withdraw、移动和其他动作继续执行。

creep transfer/withdraw 的 native 只改两对象库存、实验室矿物绑定/释放的容量字段并新增事件，不创建对象、分配 ID、改变位置、玩家、市场或工作队列。保存完整记录覆盖 Generic、creep、spawn、extension、storage、tombstone、ruin 的既有库存形态；空间索引没有本次写入。扩展写面前必须同步审计恢复范围。JS 全局变量不回滚，玩法权威状态须放在 ctx.state。

native 的安全模式、rampart、nuker/powerBank 和 terminal 干扰检查仍是默认行为。replace 或不调用 next 可以取消默认中心；特殊存储策略尚无独立处理槽，不能把动作钩子解释为这些策略都已逐项开放。

## 装配与默认路径

历史 `builtin::link()` 入口现在聚合 link、creep 资源、移动与战斗槽及仓库容量规则；规则仍 25 条，内建槽共 10 个。各 native 绑定由安装门显式校验，builtin revision/内容摘要纳入新描述。自有意图不能挂载内建派发槽，即使已安装 replace 也会在装配期拒绝；它们由 Mod 对象派发，内建动作则由类型化对象派发。自有意图装配跳过全部内建槽，避免把 creep native 当成 Mod process；纯 Rust 安装表按冻结槽分配处理器，各动作使用各自 native 函数。JS 宿主沿用一次进入 VM 的整链组合。移动槽的协议另见[移动动作槽](creep-move-slot.md)，战斗槽见[战斗动作槽](creep-combat-slot.md)。

无扩展时走 Native，不保存局部快照、不暂存 namespace、不进入 JS。creep 动作改用短借用，不能据此声称默认开销为零；性能须另行测量。link tick 的正常拒绝、其他 creep 动作槽和保存恢复不属于本片。

验收见[交付记录](../research/notes/d4-creep-resource-slots-2026-10-05.md)。
