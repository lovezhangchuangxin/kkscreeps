# ADR-010 适配器子进程与引擎嵌入接口

- **日期**：2026-09-27
- **状态**：已接受；嵌入编排随后由 ADR-013 移至 kk-runtime

## 背景

screeps-ok 动态加载 TypeScript 适配器，而本项目玩家沙箱和结算使用 Rust。两者需要调用桥梁；适配器与本地模拟应共用引擎嵌入接口，避免维护不同规则实现。

## 决策

1. **使用引擎子进程。** 适配器启动 `kkscreeps embed`，通过 stdin/stdout 的逐行 JSON 调用和请求 id 匹配响应。一个测试执行域复用连接，测试串行使用；teardown 重置世界和玩家线程，保留进程，stdin 关闭时退出。
2. **选用互补 bot 夹具。** TooAngel 检查多模块 CommonJS 与 lodash 使用，Overmind 使用固定发布产物检查 bundle 形态；合成 WASM 夹具另外检查二进制模块加载。具体版本与缓存限制见[验收工具](../../compat/README.md)。
3. **M0 使用最小桩世界。** 初期只实现精确场景存取和时间计数，使适配器在引擎规则尚未完成时能够出报告。该选择仅适用于 M0，后由 M1 的类型化世界和真实结算替代。
4. **脚本准备锁定上游。** 上游检出到忽略目录 `.compat/screeps-ok`，报告入库；不使用 submodule，也不把上游代码纳入本仓历史。固定版本以[准备脚本](../../scripts/setup-screeps-ok.sh)为准。

## 嵌入接口

当前入口是 `kk-runtime::EmbedEngine`，由协调层连接玩家执行与结算。适配器通过 CLI 传输层调用场景创建、对象放置、地形设置、玩家运行、世界推进、查询和日志读取等方法；本地模拟计划直接调用同一库接口。

这是适配器和模拟的共用入口，不是所有引擎调用的唯一公共 API。bot-check 使用真实沙箱和固定世界投影，但不推进结算，不能当作嵌入式完整玩法验收。

若替换为进程内绑定或其他传输，须保持嵌入接口的语义并记录决策变化。当前消息使用自定义 `ok/result/error` 字段，不是标准 JSON-RPC 2.0，格式见[embed_main.rs](../../crates/kk-cli/src/embed_main.rs)。

## 玩家运行语义与错误

成功的普通 `runPlayer` 调用推进一轮：注入已提交世界、执行真实玩家代码、收集候选、结算并提交写回。结算失败时缓存输出供重试；重试不会再次执行玩家代码，也不能把全流程称作无条件原子执行，边界见 [tick 契约](../architecture/tick-contract.md)。

适配器按上游 `Symbol.for('screeps-ok.RunPlayerError')` 标记传递 syntax、runtime、serialization 错误。代码未执行时抛普通 Error，不把它当作表达式返回 undefined。capability、预期差异和跳过以当前配置为准，M0 全 false 的记录不是当前状态。

RPC 超时会关闭连接并拒绝等待请求，下次调用启动新进程；旧世界不自动恢复。子进程和管道的 unref 与退出清理减少遗留进程，但不代替超时或崩溃恢复协议。

## 理由与证据

子进程避免引入进程内绑定构建链，也使引擎退出不会直接结束测试执行进程；连接中断仍须报告失败，不能计作测试通过。

- [适配器](../../compat/src/adapter.ts)、[连接管理](../../compat/src/rpc.ts)：当前调用与错误处理。
- [嵌入入口](../../crates/kk-runtime/src/embed.rs)：当前世界推进与提交机制。
- [M0 基线](../../compat/reports/m0-screeps-ok-baseline.md)、[bot 环境报告](../../compat/reports/m0-bot-missing-api.md)：初期验收范围。
