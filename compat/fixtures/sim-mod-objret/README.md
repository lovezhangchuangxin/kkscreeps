# 自有对象反击场景（G09.d）

三场景：thorn（spike 耐久 300 + 反击 0.5）、reactor（core 耐久 200 +
反击 1.0 全额 + 容量 100）、combined（两包同计划共存）。bot（main.js）
四轮探针：raider 近战每击 30 吃 15 反击（200→185→170→155），sniper
远程每击 10 吃全额 10（200→190→180→170），spike/core 耐久正常扣减、
core 转入 50 能量不受反击扰动——反击的可观测面是攻击者同轮 hits 扣减
（延迟队列同轮 tick 落账），与结算同源。对象 tick 把执行期读值记入
namespace state（mod_states 断言在 Rust 侧补充核对）。

夹具包见 [mod-objret](../mod-objret/README.md)；Rust 侧断言（双通道、
组合身份、冷恢复）见 `crates/kk-cli/tests/sim_tests/obj_ret.rs`。

```bash
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-objret/scenario-combined.json --ticks 4 --report /tmp/objret.json
```
