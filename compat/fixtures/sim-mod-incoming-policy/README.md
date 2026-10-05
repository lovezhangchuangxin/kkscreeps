# JS 受击公式与护盾组合场景

玩家连续两轮攻击同一 `[TOUGH, MOVE]` 目标，TOUGH 强化 GHO2；每次主攻击 30，目标初始 200 hits。至少运行 3 轮，第三轮从真实玩家 API 读取受击后的身体和事件。两包均使用公共 SDK，宿主没有按包 ID 分派的规则。

| 场景 | 受击行为 | 最终 hits / TOUGH hits |
|---|---|---|
| `scenario-plain.json` | 原生默认减伤，每轮扣 15 | 170 / 70 |
| `scenario-armor.json` | armor 替换为按活跃 TOUGH 层数计算的公式，每轮扣 10 | 180 / 80 |
| `scenario-shield.json` | 默认 TOUGH 后吸收 18 点护盾，先扣 0 再扣 12 | 188 / 88 |
| `scenario-combined.json` | armor 后吸收护盾，先扣 0 再扣 2 | 198 / 98 |

执行者均保持 200 hits；目标没有 ATTACK 部件，因此没有原生反击。事件 damage 保持 30。护甲包 applied=2；护盾包按目标 ID 保存剩余池，最后为 0、applied=2。公式替换后不再次应用原生 TOUGH；正常拒绝与数值边界见[JS 受击策略](../../../docs/architecture/mod-incoming-policy.md)。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-incoming-policy/scenario-combined.json --ticks 3 --report /tmp/incoming.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-incoming-policy/scenario-shield.json --ticks 3 --kkpv --report /tmp/incoming-shield.json
```

`kk-cli` 的 `sim_tests/incoming_policy.rs` 核对四场景 × JSON/KKPV 的完整 final（只排除实际 CPU 桶）、PlanHash 与组合；在第一轮受击后保存、退出进程、加载再运行两轮，确认部件耐久与护盾 state 恰一次延续。护盾没有使用不可恢复的 VM 全局变量。
