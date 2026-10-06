# JS 整槽处理链接线（2026-10-03）

承接链驱动模型实验与最新提交复核。选择依据见 [ADR-015](../../decisions/ADR-015-mod-chain-execution.md)，现行机制见 [Mod 处理链执行](../../architecture/mod-execution.md)。

## 原因与选择

原 NextAction 闭包在内层 JS 派发时需要重新借用 ModRealm；外层 RefMut 仍存在，try_borrow_mut 只能转成结算错误。贡献链主要组合同 VM 的 JS 函数，采用 JS 驱动更直接。Rust 无扩展 Native 路径保留，kk-engine 与 V8 仍不互相依赖。

近期提交的候选按值所有权、RoomCtx 短借用边界和冻结载荷共享继续保留：临时 ChainHost 需要拥有候选句柄，计划不应逐次深拷贝。但 RefCell 只在运行时检查借用，不能把签名改成 &RoomCtx 解释为类型已经保证进入 JS 前释放借用。原 Rust 闭包续行与静态导出则不再服务当前执行路径，删除而非继续补工厂。

复核也确认两个问题：把带 after 的 action_chain_below 用作续行会提前并重复运行 after；单次 next 只抛错/返回 Err 时，处理器捕获后可以成功提交。此次删除静态续行接口，JS/Rust 两层都留存 next 故障。

实验原文采用“中心抵达才 after”的语义，与既有 Rust 测试的“整槽成功才 after”不同；其中 consumed 还没有传播内层是否取消。这次明确保留既有 Rust 语义：取消中心可成功，after 在 wrap 全部退栈后一次执行，故障不执行 after。实验仍保留为原始证据，不以其有限场景代替真实引擎验证。

## 实现

- kk-engine 增 SlotExecutor/SlotCall 与整槽装配入口；无扩展 link 仍直调 native，自有槽与 JS 贡献槽进入宿主。
- kk-mod-host 在冻结后安装链描述；一次 invoke_chain 执行整个槽，普通 read/write/native/fault op 操作临时 ChainHost。删除 op_mod_next、NextSlot/NextAction 与 invoke_wrap。
- kk-runtime 固定原接收者/参数/native 步骤；处理器按 namespace 读取 Rust 候选。next 前写回，next 后刷新外层上下文，保留类型不变的嵌套对象/数组引用。
- 每次调用绑定独立标识与当前槽函数集；native 最多执行一次，replace 槽没有 native 能力；成功/取消/失败都清空 OpState 中的候选句柄。
- Mod 字段走既有 schema 与位置写入门。内建 link 当前完整记录可读，仅 cooldown 可写（u32/可删除），其余内建写能力后续按领域开放。
- wrap 注册上限 64；Promise/thenable、重复 next、内层异常与候选写失败不能因处理器捕获而提交。

## 真实链验证

`kk-runtime/tests/mod_chain_tests.rs` 的八项测试（部分在 JSON/KKPV 或多场景循环下执行）：

1. 两层 JS wrap + link native tick：before、外入、内入、native、内出、外出、after，外层写入在 native 前生效，旧字段/state 引用刷新。
2. link transfer：native 读取原参数与接收者，store 旧引用刷新，真实源/目标资源变化正确。
3. JS replace 中心被 wrap 包裹，native 不执行。
4. Mod counter process + 两层 wrap，全候选状态和字段共享，双通道相同。
5. 外层/内层取消均不执行中心，after 恰好一次。
6. 捕获重复 next/内层异常、外层在 next 后抛错、thenable、坏 cooldown：世界/时间/代际/Mod 状态回滚，重试仍失败。
7. 留存的旧 next 不会消费后续调用的 native 能力。
8. 不同 Mod namespace 状态独立，目标字段同一候选。

宿主另测调用后能力销毁、未绑定读与 replace 非法 native 调用即使捕获仍失败、65 层启动拒绝。收尾加入 after 断言后复现：直接捕获 op 错误虽然会失败候选，却仍执行 after；补上故障后的读写/native 拒绝门后，该测试与全仓检查通过。引擎扩展槽另测 Rust 捕获重复 next/中心 Err 仍使候选失败，并补齐原 link 取消用例的真实 after 断言。

## 已知边界

没有完成玩法 VM 自动重建、资源预算、故障停用、所有默认规则槽、多对象写 API、磁盘恢复与 outbox。模块全局不能承担持久状态；这次改动不把候选回滚扩展为任意 JS 堆回滚。

## 验证结果

机器结果与全部独立性能样本见 [验证数据](js-chain-validation-2026-10-03.json)。

- `pnpm run check` 最终通过：Rust 539 passed / 7 ignored（包含 doctest），workspace/all-targets Clippy 与格式检查通过，JS workspace 19 passed；补齐验证记录后链接检查为 205 个 Markdown / 1039 个本地链接通过。
  - （2026-10-06 注：当时完整 check 确含 doctest；现两档门禁均已移除该阶段，改 doctest 时单独 `cargo test -p <crate> --doc`，见 AGENTS.md。）
- release CLI 重建、适配器构建与完整 ratchet：锁定上游 `34a630efa42908c117c19c25d2350fa2387bd633`，1854 passed / 153 failed / 907 skipped，基线 0 转红 / 0 转绿。失败与跳过仍是既有能力缺口，不表示完整兼容。
- bot-check：tooangel/overmind 各完成 2000 tick，wasm 完成 10 tick；硬中断、停跑与缺失 API 均为零。Overmind 首 tick 保留既有 1 轮 console error；tooangel 的 log 级内部错误也仍存在，不能将工具通过等同于完整业务正确。

无 Mod 性能以提交 `2205b30` 的精确源码归档重建基线，与本次改造二进制交替运行；每版每场景独立五次，预热 500 / 采集 3000 tick。测量期间不运行其他构建或验证任务，机器频率与外部应用负载未控制。

| 场景 | CPU core-ms/tick 中位数：旧 → 新 | wall p50 ms 中位数：旧 → 新 | settle p50 ms 中位数：旧 → 新 |
|---|---|---|---|
| 8 玩家 JSON | 1.9800 → 1.9738 | 0.7543 → 0.7512 | 0.0804 → 0.0799 |
| 32 玩家 JSON | 12.7307 → 12.7748 | 5.2060 → 5.2245 | 0.3445 → 0.3462 |
| 8 玩家 KKPV | 2.5161 → 2.5222 | 0.9694 → 0.9756 | 0.0748 → 0.0750 |

CPU 变化为 −0.31% / +0.35% / +0.24%，采样 RSS p50 变化在 −0.44% 至 +0.60% 内；当前样本没有显示明确的默认路径退化，也不据此宣称加速。性能和 bot 使用的候选二进制在最后宿主故障门补丁前保存，该补丁仅作用于启用 JS 槽的 op；最终 release 另外记录哈希并重跑全仓及 ratchet。原始与最终二进制哈希均在数据中。

`perf-probe` 没有 Mod 负载入口，这些数据只验证默认路径。密集 Mod、VM 故障重建、长期 GC 与多对象领域写能力仍须分别验收；本次未据小对象测试声明完整 Mod 性能或生产安全达标。

## 复审清偿：上下文刷新（2026-10-03）

随后复审以真实引擎探针确认三项缺陷：继承的 `__proto__` 被递归刷新，意外改写 VM 对象原型；外层共享同一容器的多个路径被反复刷新，导致内层候选更新丢失；刷新整个 ctx 删除本地 helper。原 11 项 runtime/host 链测试仍通过，三个边界探针全部失败，说明原覆盖未包含这些形态。

修复用自有数据属性描述符取得旧值，不遍历继承对象；一次刷新共享 WeakSet，按固定路径顺序只复用每个旧容器一次，剩余路径使用独立 JSON 候选值。ctx/target 只刷新权威成员，写回只包含 state/target.fields，本地函数与循环引用不进入 Rust 候选。共享引用的拆分与旧引用边界回写现行架构，不承诺一个旧对象同时跟踪多个不同的候选值。

新增四项真实引擎回归，均覆盖 JSON/KKPV：共享对象/数组与旧引用、普通/自定义原型及二次 tick、本地 ctx/target helper 与循环缓存、state/目标字段之间的别名。runtime 链 12 项与 host 链 3 项通过；完整 `pnpm run check` 为 Rust 543 passed / 7 ignored、JS 19 passed、Clippy/fmt/doc-links 通过。release 与适配器已重新构建；兼容性结果记录在验证数据的 review_fixes 部分。

前文性能与 bot 数据保留对应当时二进制，本次清偿验收覆盖功能、回滚及兼容性，密集 Mod 成本仍未测量。
