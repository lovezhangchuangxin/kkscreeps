# G07.a 战斗射程规则场景

`atk`（p1，`attack,move`）距离 2 攻击 `foe`（p2，`move`）；`medic`（p1，`heal,move`）距离 2/4 治疗 `ally`/`pal`（p1，`tough`，满血——治疗量不可观，治疗族以射程门返回码为证；结算侧命中由 `foeHits` 证）。每 tick 提交全部动作，`--ticks 4`、`time>=3` 起记录（玩家代码先于结算，该时点已结算 3 轮）。

| 场景 | 规则覆盖 | attack 门 | heal/rangedHeal 门 | 常量（A,H,RH） | 3 轮后 foe | 终态 foe |
|---|---|---|---|---|---|---|
| `scenario-plain.json` | 原生（1/3） | `-9` | `-9,-9` | `1,1,3` | 100 | 存活 |
| `scenario-reach.json` | reach：attack.range 1→3 | `0` | `-9,-9` | `3,1,3` | 10 | 第 4 轮击杀 |
| `scenario-medic.json` | medic：heal.range 1→3、rangedHeal.range 3→5 | `-9` | `0,0` | `1,3,5` | 100 | 存活 |
| `scenario-combined.json` | reach + medic | `0` | `0,0` | `3,3,5` | 10 | 第 4 轮击杀 |

- **同源生效**：覆盖值经冻结计划同时进入 Rust 结算（Plane 快照）与玩家全局常量（`ATTACK_RANGE` 等）；玩家 API 射程门与结算距离判定读到同一生效值，不会出现「门放行但结算拒」或反向撕裂。
- **规则族独立**：`attack.range` 不影响治疗族，`heal.range`/`rangedHeal.range` 互不牵连；`rangedMassAttack` 的三格面积语义（1/0.4/0.1 档）不随单目标射程变化（引擎与运行时测试钉死）。
- 值域 0..=50，越界覆盖在冻结时拒绝。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-combat-ranges/scenario-combined.json --ticks 4 --report /tmp/ranges.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-combat-ranges/scenario-reach.json --ticks 4 --kkpv --report /tmp/ranges-kkpv.json
```

`kk-cli` 的 `sim_tests/combat_ranges.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份（四身份互异），以及第 2 tick 保存检查点、退出进程、加载再续跑的冷恢复一致；门语义测试断言上表全部取值。策略包在 [../mod-combat-ranges/](../mod-combat-ranges/)。
