# 独立 JS 反击策略

flat-counter 为目标声明固定反击 7；thorns-counter 根据执行者有效、未强化 ATTACK 部件的基础功率计算 20% 向上取整。两个包均通过公共 SDK 声明表后 request 原目标，保留 native 主攻击；plain 无 Mod。两个包分别选择，同时选择按多 replace 冲突拒绝。

执行者和目标都是 ATTACK+MOVE、200 hits，只在 time=0 近战攻击一次，主攻击均为 30。flat/thorns/native 的执行者最终分别为 193/194/170，目标均为 170。两个 Mod 的 applied=1，amount 分别为 7/6。

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-retaliation-policy/scenario-flat.json --ticks 3 --report /tmp/flat-counter.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-retaliation-policy/scenario-thorns.json --ticks 3 --kkpv --report /tmp/thorns-counter.json
```

CLI 验证三场景各两通道的完整 final（排除实际 CPU 桶）与 PlanHash；两包各两通道在反击后保存检查点、退出进程再续跑，完整 final/组合/PlanHash 与连续三轮相等。这个荆棘例子仅覆盖未强化原生近战功率；接口接收作者预计算的数值，并不提供逐命中实际伤害回调。
