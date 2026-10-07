# Creep 移动动作槽

`builtin:creep.move` 已开放 JS before/wrap/replace/after 和正常拒绝。槽切点在意图登记阶段：处理器在一次进入 JS 的整槽调用中观察/改写本执行者的移动请求登记，实际位移仍由既有的移动仲裁与对象 tick 执行阶段完成。执行仍由原玩家 API 收集意图、原清洗器转换参数；原 `EXECUTION_ORDER`、动作互斥及 creep 归属校验保持生效。

```js
export default function register(api) {
  api.handlers.wrap('builtin:creep.move', { id: 'one-way' }, (ctx, next) => {
    // 限制特定 creep 只许向南移动；其余执行者照常走默认登记。
    if (ctx.target.fields.name === 'courier' && ctx.args.direction !== BOTTOM) {
      ctx.movement.request(BOTTOM);
      return; // 不调用 next：取消默认中心，登记最终值为本次请求
    }
    next();
  });
}
```

## 调用读面与请求能力

`ctx.target.id` 是执行 creep 的权威 ID；`ctx.args` 为 `{ id, direction }`——`direction` 是玩家指令的方向（1..8 或 null），`id` 是"朝相邻 creep 移动"形态的目标。修改这些 JS 本地值不会改变 native 的接收者或原始请求；`next()` 返回后刷新。

`ctx.target.fields` 是候选中的只读视图，与资源槽共用 `name/owner/x/y/spawning/hits/fatigue/store/storeCapacity` 投影；creep 字段没有写面。`ctx.resources` 资源能力不在本槽开放。

`ctx.movement` 是本槽专属的受控移动请求能力（不可枚举、不可替换，只在所属处理器同步执行期有效）：

| 方法 | 语义 |
|---|---|
| `pending()` | 当前执行者已登记的请求方向（1..8），未登记为 `null` |
| `request(direction)` | 以 1..8 的整数登记或替换本执行者的请求；返回是否登记成功 |
| `cancel()` | 取消本执行者的请求登记 |

`request` 与 native 意图共用同一预检：越界或孵化中返回 `false`；静态结构障碍（含私有敌方 rampart）无移动预检贡献时直接拒绝，有贡献时返回登记成功并暂存原生拒绝，交由 [移动预检](mod-move-precheck.md)统一决策。登记成功不保证获准位移。creep 冲突、疲劳门槛与 MOVE 部件检查由后续仲裁处理。登记时同步记录意图时疲劳。后到的有效登记覆盖先前登记；`pending()` 在 wrap 内可观察覆盖。pull 的登记经独立 [拉拽登记槽](mod-move-pull-registration.md)，被拉者随行仍是自己的 move 意图。

实际位移、道路磨损、工地踩踏、疲劳结算、跨房过渡和空间索引更新都发生在其后的仲裁与对象 tick 阶段；本槽没有任意坐标写入或传送能力。

## 正常拒绝边界

引擎在整链开始前保存执行者的请求登记（目的格、格内登记序与原生静态拒绝快照）和意图时疲劳记录；正常拒绝时按快照恢复本执行者的登记并撤销 `old_fatigue` 写入，同时沿用资源动作的执行者/目标对象与事件边界。恢复不触碰其他执行者的请求、任何拉拽关系或移动队列整体，因此此前其他 creep 的有效请求保持原样参与仲裁。runtime 丢弃本链全部参与 namespace 的暂存 state；此前动作、后续动作与整轮发布语义见[Mod 执行](mod-execution.md#正常动作拒绝)。

无扩展时走 Native 直调，不保存快照、不进入 JS。验收见[交付记录](../research/notes/d4-move-slot-2026-10-05.md)。
