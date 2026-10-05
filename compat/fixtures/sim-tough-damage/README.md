# 强化 TOUGH 的真实玩家场景

双方使用 `[TOUGH, ATTACK, MOVE]` 并给第一个部件强化 GHO2。玩家在 tick 0、1 各攻击一次，tick 2 观察结果，至少运行 3 轮。

- `scenario-native.json`：原生每次主攻击与反击各 30；双方各扣 15，两轮后 hits 270、TOUGH 70。
- `scenario-js.json`：复用公共 SDK 的独立 `flat-damage` 包，每次主伤害 45，无默认反击；单轮减免 `round(45×0.5)=23`，实际扣 22，两轮后目标 hits 256、TOUGH 56，执行者保持 300。

玩家事件仍观察减伤前伤害 30/45。宿主按受伤前身体合并当轮伤害，治疗随后合并；JS 的 amount 不表示减伤后的最终扣血量。接口边界见[受击结算](../../../docs/architecture/creep-incoming-damage.md)。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-tough-damage/scenario-native.json --ticks 3 --report /tmp/tough-native.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-tough-damage/scenario-js.json --ticks 3 --kkpv --report /tmp/tough-js.json
```

`kk-cli` 的 `sim_tests/tough_damage.rs` 核对 JSON/KKPV 完整 final（仅排除实际 CPU 桶），并在第一轮受伤后保存、退出进程、加载再跑两轮，与连续运行比较世界、部件 boost/hits、Memory、Mod state、PlanHash 和组合计划。没有新增持久化状态域。
