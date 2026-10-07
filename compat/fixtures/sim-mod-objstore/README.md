# 自有对象库存场景（G09.a）

三场景：hauler（crate 300 容量 + 无容量 marker 对照）、vault（cache 100
容量）、combined（两包同计划共存）。bot（main.js）四轮探针（T0 发动作、T1 读提交并补满、T2 满载/提取、T3 终值）：转入 200、补满
100（预检余量门）、超容请求得 ERR_FULL(-8)、提取 50 回读、向无容量 marker
转入得 ERR_INVALID_TARGET(-7)、vault 转入 100——全部经玩家 API 预检与
Mod 玩家类的投影信封读值核对，与结算同源。对象 tick 把执行期库存读值记入
namespace state（mod_states 断言在 Rust 侧补充核对）。

夹具包见 [mod-objstore](../mod-objstore/README.md)；Rust 侧断言（双通道、
组合身份、冷恢复）见 `crates/kk-cli/tests/sim_tests/obj_store.rs`。

```bash
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-objstore/scenario-combined.json --ticks 4 --report /tmp/objstore.json
```
