# 贡献链驱动模型实验（2026-10-03）

承接 [C.4 前置片记录](c4-player-api-2026-10-03.md)。起因是一项架构承诺在实现中被
换掉了模型，而门禁无法察觉（见 §4）。本记录是一次最小实验的事实与结论。

## 1. 命题

结算期的贡献链是 `before* → wrap* → 中心 → after*`，其中：

- `before` / `wrap` / `replace` / `after` 位上的处理器多数是 JS 函数；
- 中心（`link.transfer` / `link.tick` / Mod 自有槽的 `process`）多数是
  Rust 内建实现。

设计文档 §5 写的是「允许 JS `next()` 同步调用 native、native 再触发生命周期
JS，但所有此类 op 必须显式 reentrant」。已落地的实现却把 `next()` 的载荷做成了
**Rust 闭包**（`NextAction = Box<dyn FnMut()>`），内层链要再进 JS 时闭包必须重新
借用 `ModRealm`，而外层 `invoke_wrap` 的 `RefMut` 仍压在栈帧上——两个 `&mut`
共存，在禁止 unsafe 的前提下无解。

于是产生一个待决问题：**这个 reentrant 机制是必需的，还是在解决一个不存在的问题？**

## 2. 实验

新增两个 spike 文件（不属于 Rust workspace，独立 crate）：

- `spikes/deno-spike/examples/js_driven_chain.js`
- `spikes/deno-spike/examples/js_driven_chain.rs`

形态：**链由 JS 驱动**。宿主只提供链的描述数据（各链位的处理器键 + 中心形态），
`runSlot` 一次调用跑完整条链。`wrap` 之间的续行是 JS 闭包；Rust native 中心是
链内一次**普通 op 调用**。

与 `js_mod_host.rs` 的方向差异：那里 next 的载荷是传给 Rust native 的 JS 回调
（Rust 驱动链）；这里反过来。真系统的链是 JS 优先的，`wrap` 之间本就不该有
Rust 回入。

**全文 `op2(reentrant)` 标记数为 0**，op 表只有 `op_chain_log` 与
`op_chain_native_center` 两个普通 op。

## 3. 结果

| # | 场景 | 断言 | 结论 |
|---|---|---|---|
| 1 | 两层 JS wrap + Rust native 中心 | 轨迹 `before, A.enter, B.enter, native(id=7), B.exit, A.exit, after`；候选值 0→3 | 嵌套续行与候选写入均正确 |
| 2 | 中心换成 JS `replace` | 轨迹同形，中心位为 `js-center` | 驱动路径与中心形态无关 |
| 3 | 外层 wrap 吞掉 `next()` | 轨迹仅 `before, swallow`，中心与 after 均未执行 | 与引擎「取消内层」语义一致 |
| 4 | 重复 `next()` | 宿主侧收到 `Error: next() 至多调用一次` | 响亮失败并穿透回宿主 |
| 5 | 内层在内层成功后抛错 | 宿主收到错误；轨迹 `before, A.enter, boom.enter, native(id=7)`，中心的候选写入仍在 | 异常炸候选的前提成立（回滚归宿主） |

结论：**两层 wrap 嵌套 + Rust native 中心共存，全程不需要 reentrant op。**
宿主→JS 的回入点只有入口那一次。

## 4. 附带发现：`after` 时点陷阱

首轮实验断言失败，暴露出一个非显然的语义问题。

JS 递归的自然写法把 `after` 挂在基例（`depth == wraps.length`）上，实测轨迹是

```
before, A.enter, B.enter, native(id=7), after, B.exit, A.exit
```

`after` 在**最内层**执行，wrap 尚未退栈。而引擎语义（`contrib::run_wrapped`、
`action_chain_below` 的文档）是：`after` 在整条 wrap 链**退栈后**才执行。

这不是实现瑕疵而是**两种控制流的天然差异**——Rust 的 `after` 在闭包返回后跑，
JS 递归的基例是最内层。JS 驱动形态必须让 `runChain` 返回 `{consumed, value}`，
由 `runSlot` 在**外层**决定是否跑 `after`，否则 `after` 时点会静默错位。

任何「把 Rust 执行器翻译成 JS 递归」的方案都要正面处理这一条。

## 5. 推翻的假设

实验同时推翻了我在 C.4.5 前置片中采用的模型。原判断是「C.4.5c 的 Rust 闭包载荷
需要重做但方向正确」，实际是**方向就错了**：

- 已落地：`op_mod_next`（`reentrant`）、`NextSlot`/`NextAction`、
  `ModRealm::invoke_wrap`、`action_chain_below`、`transfer_chain` /
  `tick_chain` / `ext_chain` 三个 `'static` 导出。这些机制的存在前提是
  「Rust 在链中驱动」，而该前提不成立。
- 仍必要：C.4.5a（`Working` 去生命周期化）与 C.4.5b（处理器边界迁 `RoomCtx`）。
  JS 驱动形态下每个 native 步骤仍是一次独立 op，取短借用即可；`RoomCtx`
  正是让这个纪律可表达的载体。

已落地的 6 个测试覆盖的是「JS wrap → 纯 Rust 内层链」，该路径在现有实现下确实
通；挂掉的是「内层链再含 JS wrap」。所以现有代码不是废品，但**无法靠加功能延伸**
到嵌套。

## 6. 未决

1. **是否按 JS 驱动链重做**： entails 删除 §5 所列机制，并把 native 处理器
   暴露为可从 JS 调用的 op。工作量集中但不小，且会推翻刚合入的实现。
2. **设计文档 §5 的 reentrant 条款随之改写**：JS 驱动下 wrap 续行不再需要
   reentrant；「native → JS」这个方向在别处（如玩家生命周期）仍然存在，条款
   应收窄而不是删除。
3. **本实验只证明了机制可行**，没有证明按真实引擎形态重做是廉价的。真实链的
   `before`/`after` 也可能是 native（不止中心），需要一并暴露为 op。
