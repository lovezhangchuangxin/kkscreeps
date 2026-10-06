# Mod 运行 trace 夹具

两个场景共用 `../mod-trace` 的两个包，供 `sim run --mod-trace` 验收：

- **scenario-gatekeeper.json**（选 gatekeeper）：pass 意图计数 + hold 意图正常拒绝（`api.decline('gate-closed')`）；断言 state、玩家读回值与两次 emit 返回码（OK 记录、拒绝在结算侧表达）。trace 应含 `slot_completed`（`gatekeeper:pass`）与 `slot_declined`（绑定 `gatekeeper:hold`、code `gate-closed`）。
- **scenario-breaker.json**（选 breaker）：link tick 前置处理器抛错；sim 默认无自动停用阈值，故障轮持续缓存重试，报告 `status=run_failed` 且 `mod_trace` 含 `slot_fault`（槽 `builtin:link.tick`、绑定在 composition 活动链内）与 `settlement_retry`。

不开 `--mod-trace` 时报告 `mod_trace` 为 null；开启时事件缓冲有界（超限丢弃并计数），条目只含 tick/房间/目标/槽/阶段与结果摘要。契约见[运行 trace](../../../docs/architecture/mod-trace.md)。

```bash
cargo build -p kk-cli
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-trace/scenario-gatekeeper.json --ticks 2 --mod-trace --report /tmp/trace-gate.json
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-trace/scenario-breaker.json --ticks 2 --mod-trace --report /tmp/trace-break.json
```
