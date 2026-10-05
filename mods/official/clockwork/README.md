# Clockwork 生命周期原型

这个官方样例只使用公共 JS Mod SDK，验证自有对象在没有玩家指令时也能更新，并把结果通过玩家类与查询呈现。当前是生命周期原型，不是正式生产建筑或完整玩法包。

`clockwork:counter` 的 `value` 默认 0，每轮增加 `step`，超过 1000000 后取模回到区间内。包配置只接受可选 `step`，为 1–100 的整数，默认 1。`ctx.state.visits` 记录已提交的对象 tick 次数。玩家可使用 `ClockworkCounter.value`、`FIND_CLOCKWORK_COUNTERS` 和 `LOOK_CLOCKWORK_COUNTERS`。

在仓库根目录构建并运行：

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-tick/scenario.json --ticks 50 --report /tmp/sim-mod-tick.json
```

夹具在两间房各放置一个对象，配置 step 为 2。50 tick 后 value 为 100 和 107，visits 为 100；最后一轮玩家读取的是结算前的值 98 和 105。把本包复制到普通 Mod 根目录的子目录即可通过同一装载路径使用，官方目录和包 ID 不赋予特殊权限。

对象目前由场景或宿主初始化；JS 创建/删除、资源生产、建筑能力、内容预设和存量数据迁移尚未开放。SDK 的现行契约见[自有对象 tick](../../../docs/architecture/mod-object-tick.md)，场景格式见[本地模拟](../../../docs/architecture/local-simulation.md)。
