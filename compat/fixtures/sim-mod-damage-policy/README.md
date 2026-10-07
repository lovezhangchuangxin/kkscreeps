# 独立 JS 伤害策略验收

同一玩家脚本只在 tick 0 发起一次近战攻击，再观察两轮。默认攻击者为 200 hits，目标为一个 ATTACK 部件的 100 hits：

| 场景 | 策略 | 最终目标 / 执行者 hits |
|---|---|---|
| `scenario-flat.json` | 选择 flat-damage，固定 45 伤害 | 55 / 200 |
| `scenario-scaled.json` | 选择 scaled-damage，ceil(执行者 hits × 0.3 / (1 + 活跃 TOUGH 数)) | 40 / 200 |
| `scenario-plain.json` | 无 Mod，原生 30 伤害及 30 反击 | 70 / 170 |

两包只使用公共 SDK，位于相邻 `mod-damage-policy/`，Rust 不识别包 ID。护甲部件读面另由运行时测试验证。两个 replace 同时启用应冲突；这是一组互斥规则变体，不是默认官方内容预设。

在仓库根目录运行（JSON/KKPV 分别验证）：

```sh
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-damage-policy/scenario-flat.json --ticks 3 --report /tmp/damage-flat.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-damage-policy/scenario-scaled.json --ticks 3 --kkpv --report /tmp/damage-scaled.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-damage-policy/scenario-plain.json --ticks 3 --report /tmp/damage-plain.json
```

`--ticks 1 --save-checkpoint /tmp/damage-checkpoint.json` 提交了攻击，但 Memory 仍是结算前观测，所以退出码 2；随后 `--ticks 2 --load-checkpoint /tmp/damage-checkpoint.json` 与连续 3 轮的最终状态等价。CLI 集成测试逐包、逐通道验证完整 final 与 namespace state，只排除实际线程耗时导致不同的 CPU 桶。

现行能力及限制见[JS 伤害策略](../../../docs/architecture/combat.md#单体伤害)。
