# JS 处理器组合夹具

base 提供自有对象 tick 和玩家 API；z-gate 提供 before/wrap/after 及 link capacity=700；a-audit 通过 dependsOn 引用 z-gate 的同种处理器；dependent 的 manifest 依赖 a-audit。只有显式 handler/manifest 边形成停用依赖，没有按包 ID 的 Rust 分支。

虽然 a-audit 在字典序及当前注册序中早于 z-gate，冻结后 before/wrap 先执行 z-gate，after 反向。每轮目标 trace 为 ZA([C])az，实际对象中心 C 自主执行，两个玩家读取已提交结果。z-gate before 每轮重置 trace，避免把有限字段当无界日志。

在仓库根目录执行：

```bash
cargo build --release -p kk-cli
target/release/kkscreeps mod inspect --root compat/fixtures/mod-composition --report /tmp/composition.json
target/release/kkscreeps mod inspect --root compat/fixtures/mod-composition --disable z-gate --report /tmp/composition-disabled.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-composition/scenario.json --ticks 2000 --report /tmp/composition-sim.json
```

停用预览的集合为 a-audit、dependent、z-gate，base 原 process 保留，规则覆盖仍为 700。命令只预览，没有将预览掩码应用到上面的 sim。单独运行场景至少 100 轮才通过 visits>=200 的断言。增加 --kkpv 可以验证另一投影；2000 轮后两个对象 value=2000、base state visits=4000，玩家最后读取 value=1999。

这是公共组合入口的功能夹具，不是正式内容预设、生产建筑或性能比较；运行时故障后真实停用和缓存重试另由集成测试验证。契约见[组合架构](../../../docs/architecture/mod-composition.md)。
