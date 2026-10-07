# G02.d1 静态移动放行场景

初轮四个 creep 向右：wall 进入 `(11,10)` 的墙结构，rampart 进入 `(31,20)` 的敌方私有 rampart，free 进入空格 `(41,30)`，occupied 尝试进入同时有墙和静止己方 creep 的 `(21,40)`。后续轮只记录位置，结构保留。

| 场景 | wall | rampart | free | occupied |
|---|---|---|---|---|
| plain | 原地 `(10,10)` | 原地 `(30,20)` | `(41,30)` | 原地 `(20,40)` |
| ghost | `(11,10)` | `(31,20)` | `(41,30)` | 原地，动态占格仍挡 |
| seal | 原地 | 原地 | 原地 `(40,30)` | 原地 |
| combined | `(11,10)` | 原地，seal 否决 | 原地，seal 否决 | 原地，动态占格仍挡 |

```bash
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-static/scenario-combined.json --ticks 3 --report /tmp/static.json --input-log /tmp/static.jsonl --verify-replay
```

`kk-cli/tests/sim_tests/move_static.rs` 核对四场景 × JSON/KKPV 的 final、四个计划身份、初轮保存后新进程续跑等值及两段运行的回放 passed。final 比较只剥离实测 CPU bucket；位置与 namespace 恰一次由场景断言验证。动态取消链、墙地形、己方工地、拉拽、跨房、能力门和失败重试另由引擎/运行时专项测试覆盖。

公共包见[mod-move-static](../mod-move-static/README.md)，实现与剩余边界见[移动预检架构](../../../docs/architecture/mod-move-precheck.md)。
