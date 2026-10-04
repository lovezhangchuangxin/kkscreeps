# D5 玩家执行边界收敛（2026-10-05）

本批基于 `8a94c53`，落实贡献平面方案 D5：玩家执行及重试由运行时负责，引擎只接受显式指令与玩家写回候选。现行接口与调用方责任见 [tick 契约](../../architecture/tick-contract.md#玩家执行与结算接口)。

## 实现与迁移

删除 `Engine::tick()`、`Engine.sandboxes` 及引擎的 `Sandbox`、`WorldView`、`ScriptedSandbox` 公开面。旧路径会忽略基础设施故障、不消费 Memory/segments/CPU 候选，结算重试还会重新执行沙箱；现在不能再通过这一接口绕过正式运行时。

`RunSpec`、`RunOutcome`、`RunResult` 从引擎移至 `kk-runtime/src/run.rs`，继续由 `kk-runtime` 根导出；移除无调用方的 `EngineSandbox` 重导出。引擎保留三种显式结算入口，共用原候选阶段与发布点，内部事务类型没有扩大可见性。不增加引擎到运行时的依赖。

旧测试沙箱改为测试内显式提交 `IntentBatch`：保留多轮采集/升级、失败回滚和真实玩家 move/harvest 的世界转移断言。原冒名市场用例迁至运行时；旧脚本表自身的回放测试随组件移除。运行时汇总按结果表中的注册玩家键过滤全部五类意图，忽略自报身份和外来玩家键，不重新绑定外来指令。对象所有权仍由结算器校验。

## 验证与复审

- 新增全部意图族的注册身份过滤、冒名市场扣费测试；引擎所有权测试直接尝试操作他人 creep。两条编译失败文档用例防止重新公开旧 tick 或沙箱注册表。
- 在原串行 `embed_engine_suite` 中新增双玩家、双房间 JSON/KKPV 场景。在 segments 写回点注入失败，检查完整 PlayerState 和对象快照不变；改传抛错代码重试仍只消费缓存，移动、Memory、segments 与 CPU 桶一起发布，下一轮确认全局计数仍为一次。
- 既有真实工作者失联、部分重建失败、故障门、显式重建及玩家级停跑/注入失败测试继续参与门禁。执行结果分类、派发与回执协议未改写。

最终源码构建 release CLI 并显式指定该二进制，重建适配器；screeps-ok 固定在 `34a630efa42908c117c19c25d2350fa2387bd633`。机器结果与二进制 SHA-256 见[验收数据](d5-player-execution-2026-10-05.json)。

- `cargo test -p kk-engine -p kk-runtime`：362 通过、0 失败、1 忽略。
- `KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check`：格式、workspace Clippy、Rust、JS、文档链接及源码规模检查均通过；Rust 650 通过、0 失败、11 忽略，compat Node 14 通过，307 个主项目源码/测试文件均不超过 1000 行。
- 完整 ratchet：2914 用例，1918 通过、89 失败、907 跳过，下限 1918；逐条比较开发前 `HEAD` 和 `HEAD~1`，状态变化均为 0，没有新增跳过或豁免。完整报告见[兼容性结果](../../../compat/reports/kkscreeps.json)。
- TooAngel/Overmind/WASM 分别完成 2000/2000/10 tick，无中断、无惩罚跳过、无缺失 API。Overmind 保留既有首 tick `DirectiveHarvest.create` 错误，归一随机 harvest 名后与入库报告相同；TooAngel 仍输出既有角色日志。固定世界 bot-check 不证明完整玩法可玩。
- `sim run` 经济场景 JSON/KKPV 均提交 2000 tick、七条断言全部通过，最终状态一致；GCL 和控制器进度各增加 1468，四个替补孵化，初始两个 creep 已消失。

提交前复审核对类型归属、依赖方向、全部旧调用方、身份过滤、完整写回、缓存重试、基础设施故障门和串行测试组织，未发现剩余阻断问题。本批有意删除旧 Rust 公开 API：下游纯结算改用 `tick_with_intents`，完整玩家轮次改用 `EmbedEngine::run_players`，执行类型从 `kk-runtime` 导入。玩家 JS API 与 Mod SDK 没有新增限制。本批未实现磁盘恢复，也未做正式性能测量。
