# sim 内容选择场景

三个场景共用 `main.js` 与[内容选择夹具](../mod-content/README.md)，验证同一包根目录在不同显式选择下的实际差异：

- `scenario-alpha.json` 只选 alpha：node 每轮 +1（默认规则），beta/gamma 无玩家 API、常量、类型与 state。
- `scenario-beta.json` 只选 beta：alpha 自动加入并使用覆盖 step=3（node +3/轮），beacon 每轮 +1，beta 玩家面可用。
- `scenario-alpha-gamma.json` 选 alpha+gamma：两个无依赖包组合安装，gamma 的 link tick 处理器写自有 state。

断言按 100 tick 换算：玩家每轮先于同轮 Mod tick 执行，故 memory.value 比最终字段少一轮增量。加 `--kkpv` 可验证另一投影通道。

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-content/scenario-alpha.json --ticks 100 --report /tmp/content-alpha.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-content/scenario-beta.json --ticks 100 --report /tmp/content-beta-sim.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-content/scenario-alpha-gamma.json --ticks 100 --report /tmp/content-alpha-gamma.json
```

报告的 `composition.selection` 与 `mods.selection` 分别记录展开结果与输入；`mod_states` 中未选包为 null。契约见[内容选择](../../../docs/architecture/mod-content-selection.md)。
