# Mod 资源领域能力

可信 JS Mod 可在 `builtin:link.transfer`、`builtin:creep.transfer`、`builtin:creep.withdraw` 的 before/wrap/replace/after 中读取同房候选库存并执行转账。资源能力与[资源动作槽](creep-resource-slots.md)共用整链提交及[正常拒绝](mod-execution.md#正常动作拒绝)边界。`builtin:link.tick`、Mod 自有槽和其他动作没有这项能力。

## SDK 与结果

每个处理器的 `ctx.resources` 是不可枚举、不可替换的属性，包含冻结的 `read` / `transfer` 函数。它不进入 ctx 的 JSON 写回，不改变原来的 `Object.keys(ctx)` 顺序。函数只能在所属处理器同步执行期间使用；保留到该处理器返回后调用会导致候选故障。底层 op 继续检查本次整槽 token 和绑定，不能跨槽或 tick 使用。

```js
export default function register(api) {
  api.handlers.replace('builtin:creep.transfer', { id: 'limited' }, ctx => {
    const resource = ctx.args.resourceType;
    const source = ctx.resources.read(ctx.target.id, resource);
    const target = ctx.resources.read(ctx.args.id, resource);
    const amount = Math.min(ctx.args.amount ?? source.amount, source.amount, target.free ?? 0, 10);
    if (amount === 0) return api.decline('full');
    const result = ctx.resources.transfer(ctx.target.id, ctx.args.id, resource, amount);
    ctx.state = { moved: (ctx.state?.moved ?? 0) + result.credited };
  });
}
```

`read(id, resourceType)` 返回 `{ amount, capacity, free }`，均为非负安全整数；墓碑、废墟没有接收容量，后两项为 `null`。读取当前候选，因此能看到此前玩家及 Mod 动作的写入。支持已完成孵化的 creep、spawn、extension、类型化 storage、带合法库存字段的 Generic 仓库，以及作为来源的 tombstone/ruin。资源名须在现有 `RESOURCES_ALL` 中；不存在的对象、其他房间、未接线类型和非法库存/容量均为故障。

`transfer(from, to, resourceType, amount, loss = 0)` 精确扣除来源 `amount`、到账 `amount - loss`，返回 `{ debited, credited, lost }`。两端须不同，ID 和资源须为原始字符串，数量须为原始数字且为非负 JS 安全整数，`loss <= amount`。它不自动截取请求量；库存不足、接收余量不足或请求为零时返回三个零，不改库存、不产生事件、也不扣损耗。Mod 可先读余量自行钳制。允许显式全额损耗，此时有效目标即使已满也能接受零到账；墓碑/废墟仍不能作为目标。

转账先在两份对象副本上验证全部变化，成功后一起写入候选。Generic 的库存合计、容量及类型化资源槽的 u32 范围均须可表示。成功产生 `EVENT_TRANSFER`，`data.amount` 为到账量；有损耗时额外记录 `data.debited` 与 `data.loss`。损耗不是普通失败的隐式费用。

SDK 在资源转账前验证并暂存当前 `ctx.state` / `ctx.target.fields`，转账后刷新上下文及保留的库存引用。直接改写 creep 库存或 link 库存字段仍会被只读门拒绝；link 的 `cooldown` 字段保持原独立写能力。namespace state 与资源转账一同参与拒绝和整轮候选发布。捕获资源错误再返回 `api.decline` 不能掩盖故障。

## 容量与策略

creep 使用当前 CARRY/boost 容量及全资源合计；spawn/extension 仅接收能量，storage 使用已接线容量。Generic 使用 `store`、`storeCapacity`、`storeCapacityResource`，不新增结构类型白名单。独立槽按对应库存计算余量，显式零容量禁止借用共享池；未绑定资源共享总容量扣除独立槽后的剩余池，合并计算所有未绑定资源库存。实验室首种矿物入账绑定矿物槽，取空后释放，可再装另一种矿物；与 native 共用解析和实验室写入函数，数值来自冻结[仓库规则](store-rules.md)。

资源能力校验数值、库存、容量及同房候选边界，距离、归属、准入和损耗比例由可信 Mod 决定。它可把一条内建资源动作替换为自定义转账或同房多对象路由。玩家 API 原预检和意图归属校验仍先执行，Mod 不能靠这项结算能力让玩家采集器接受原本非法的调用；改写玩家准入规则需要相应玩家面扩展。

默认 native 的策略与容量算法保持本批之前的行为；领域能力更严格地验证安全整数和独立零容量，并计算未绑定资源共用的剩余池。本批共用的是仓库解析和实验室槽写入，不能据此声称默认 native 已统一所有新校验。尚无通用资源创造/销毁命令、任意新资源目录、跨房转账或任意 Generic JSON 整写。

## 局部恢复与整轮失败

运行时为每条 Hosted 资源动作维护首次触达对象日志。每次转账先保存两端原始 RoomObject，重复访问仅保留首次记录；最多触达 256 个不同对象，含返回零的转账尝试。读取不占日志，限制的是 Rust 堆上的对象副本数量，不是逐 Mod 内存硬上限或字节上限。

正常拒绝时运行时恢复日志中所有对象并丢弃本链 namespace 暂存；外层引擎恢复整链开始前的原执行者和请求目标，并截去本链事件。后者覆盖 native 先写后首次被资源能力触达的情况。恢复保存整个对象，包含实验室绑定容量及 link 冷却；此前成功动作的库存、事件和 state 保留，后续动作继续结算。资源能力不分配 ID、不增删对象、不改位置、空间索引、玩家、市场或其他工作队列。默认无扩展 Native 路径没有资源日志，也不进入 JS。

非法调用及超出日志上限走整轮故障路径，世界、玩家 Memory/segments/CPU 与 Mod state 均不发布，污染 realm 销毁。晚期玩家写回失败同样拒绝全部资源候选；重试使用原玩家执行缓存，在重建的 Mod realm 中重新结算。此机制仍是内存候选与错误恢复，不是进程崩溃检查点。

`kk-mod-api/src/resources.rs` 定义结果协议；`kk-engine/src/settle/resources.rs` 负责副本预检及精确转账，`resources/generic.rs` 共用 Generic 解析和实验室槽更新；`kk-mod-host/src/chain/resources.rs` 校验数字及访问本次宿主能力；`kk-runtime/src/embed/mod_chain/resources.rs` 限定房间、维护触达日志并失效字段缓存。验收矩阵与结果见[资源领域交付记录](../research/notes/d4-resource-domain-2026-10-05.md)。
