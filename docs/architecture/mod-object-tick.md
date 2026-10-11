# Mod 自有对象 tick

当前公共 SDK 允许独立 JS 包为自有对象定义同步 `tick(ctx)`。它在真实对象 tick 阶段执行，不需要玩家发射指令；对象可由宿主管理接口、sim 初始化或处理器的[同房对象能力](mod-objects.md)创建。preTick、原生对象通用增删、房间和世界生命周期尚未开放。

## 声明与组合

```js
export default function register(api) {
  api.objects.define('example:counter', {
    version: 1,
    playerClass: 'Counter',
    visibility: 'room-visible',
    positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'u32', default: 0 },
    },
    tick(ctx) {
      ctx.target.fields.value++;
      ctx.state = { visits: (ctx.state?.visits ?? 0) + 1 };
    },
  });
}
```

`tick` 可省略，提供时必须为函数。SDK 从类型键派生 `example:counter.tick` 槽及 `example:counter.tick.process` 绑定，注册转换将槽加入类型的 `lifecycle_slots`，阶段为 `ObjectTick`。后缀也计入键长上限，过长键在注册时失败。装配只接受自有类型自己的具名 tick、正确阶段以及原 process 或已安装 replace 中心，不接受任意命名生命周期或借用内建槽。

其他包可以用现有 `api.handlers.before/wrap/replace/after` 挂到 `example:counter.tick`，顺序和 replace 冲突由同一冻结计划决定。wrap 的 `next()` 至多一次，省略 next 可以跳过内层，成功返回后仍执行 after。停止一个贡献 replace 的 Mod 后，活动槽回退原 process；停止类型所属 Mod 后，整个生命周期链跳过，schema、对象、已提交 state、规则与玩家 API 保留。停用不等于卸载，详见[Mod 执行](mod-execution.md)。

## 执行与候选

当前房间按房间名稳定顺序结算（满足资格门时房间间并行、克隆流式逐房派发后仍按该顺序归并，房内全串行，见 [tick 契约](tick-contract.md)）；每房先执行对象动作、扩展意图和移动仲裁，再固定对象 ID 列表，按 `ObjId` 升序执行对象 tick。列表中的已删除对象跳过，同类型存活对象每轮一次。当前房间循环让动作阶段创建的对象进入本轮 tick；tick 阶段新增对象不在该列表内，下轮才取得资格。同房对象能力已验证这些出生资格、删除后的引用失效与索引恢复。

`ctx` 包含 `tick`、`room`、`target.id`、`target.fields`、`state`、`objects` 和 `args`；生命周期的 `args` 固定为 null，不执行玩家归属授权。目标字段读取动作之后的当前候选。自有字段写入沿 descriptor 校验、位置同步和发布前验证；`ctx.state` 只写当前处理器命名空间，跨对象和房间共享同一轮候选 state。只有整个 tick 成功才发布。

进入 JS 前释放 Rust 候选借用；每次读取、写入、next 前后同步沿现有整槽协议进行。生命周期没有 native 中心调用和资源/移动/战斗/建造领域能力；上下文上的这些方法执行时显式拒绝，不能把自有 tick 当作绕过动作恢复边界的入口。JS 函数须同步，Promise/thenable、异常、超预算及被捕获的宿主故障均使整轮候选失败。

生命周期不支持局部正常拒绝，返回 `api.decline(code)` 是故障。对象和 namespace state、其他房间、玩家 Memory/CPU 写回沿同一整轮提交边界撤销；runtime 销毁失败 realm，重建后重结算缓存玩家结果，不重复运行玩家脚本。JS 闭包及全局变量不属于权威可恢复状态，须将玩法状态写入 `ctx.state` 或声明字段。

## 官方样例和验收

[clockwork](../../mods/official/clockwork/README.md) 是公共 SDK 的官方生命周期原型，提供自主计数对象、玩家类及 FIND/LOOK 查询。它可复制到任意作者目录，经相同包加载器装载；宿主没有按官方 ID 放行的分支。它本身不含自主增删；[nursery](../../mods/official/nursery/README.md) 另提供自主增删原型。两者均尚无生产库存、玩家建造或完整内容选择能力。

[sim 场景](../../compat/fixtures/sim-mod-tick/scenario.json) 可直接运行双房玩家闭环，格式见[本地模拟](local-simulation.md)。测试覆盖无意图更新、动作后字段、处理链、替换/停用、异常/超时、晚期写回失败及缓存重试。验收数字和原始报告见[交付记录](../research/notes/n1a-object-tick-2026-10-05.md)。
