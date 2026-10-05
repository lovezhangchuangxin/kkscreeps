# 独立 JS 群体伤害策略

`../mod-mass-policy/flat` 通过公共 SDK 的分页列表筛选六格内有主敌方目标，一次批量施加固定 23；falloff 使用 `ceil(40/(distance+1))`。每个包单独选择；同时选中会按已有多 replace 规则明确冲突。plain 是无 Mod 原生对照。

玩家只在 time=0 发出 rangedMassAttack。目标为相邻 creep、四格外且有 rampart 覆盖的 creep、两格内敌方容器；己方 ally 和 rampart 由 JS 过滤。各目标初始 hits=100，rampart 为 1000。须至少两轮才能用 Memory 观察结算；验收用三轮。

| 规则 | 相邻 creep | 远处 creep | 容器 | ally / rampart |
|---|---|---|---|---|
| flat | 77 | 77 | 77 | 100 / 1000 |
| falloff | 80 | 92 | 86 | 100 / 1000 |
| native | 90 | 100 | 96 | 100 / 1000 |

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-mass-policy/scenario-flat.json --ticks 3 --report /tmp/flat-mass.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-mass-policy/scenario-falloff.json --ticks 3 --kkpv --report /tmp/falloff-mass.json
```

CLI 测试比较三场景各两通道的完整 final（排除实际 CPU 桶）和 PlanHash；另在批量完成后保存检查点、退出进程再续跑，比较全部目标、Mod state、组合与计划身份。applied=1、targets=3，恢复不重复施加。
