# 建筑 RCL 配额覆盖夹具

quota-raise 覆盖 `builtin:structure.quota.tower.rcl8` 为 20（官方 6）；quota-lower 覆盖 `builtin:structure.quota.constructedWall.rcl8` 为 3（官方 2500）。两包相互独立、只经公共 SDK `api.rules.override` 声明，无按包 ID 的 Rust 分支；覆盖不同规则键，可在同一冻结计划共存（同键双包覆盖才是冲突，由冻结期拒绝）。

配额族是 G11.a 首片：`builtin:structure.quota.{type}.rcl{N}` 逐类型逐级标量规则（默认值 = 官方 `CONTROLLER_STRUCTURES` 表），玩家面经计划派生折叠表注入同名常量，结算侧经 `StructureQuotaSnapshot` 快照读取，两侧同源于同一冻结计划。场景与断言见 [sim-mod-quota](../sim-mod-quota/README.md)。

在仓库根目录执行：

```bash
cargo build -p kk-cli
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-quota/scenario-combined.json --ticks 3 --report /tmp/quota-sim.json
target/debug/kkscreeps mod inspect --root compat/fixtures/mod-quota --report /tmp/quota-inspect.json
```
