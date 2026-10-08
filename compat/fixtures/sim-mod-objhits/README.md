# 自有对象耐久场景（G09.b）

三场景：siege（barrel 耐久 300 + 无耐久 decoy 对照）、vault（cache 耐久
200×容量 100 双信封）、combined（两包同计划共存）。bot（main.js）四轮
探针（T0 发动作并读初值、T1 读提交并续击/转入、T2 再击核对双信封互不
扰动、T3 终值）：raider 单 ATTACK 部件近战每击 30——barrel
300→270→240→210；sniper 单 RANGED_ATTACK 部件远程每击 10——cache
200→190→180→170；向无耐久 decoy 攻击得 ERR_INVALID_TARGET(-7)；
cache 转入 50 能量后 hits 不受资源动作扰动——全部经玩家 API 预检与 Mod
玩家类的投影信封读值核对，与结算同源。对象 tick 把执行期耐久读值记入
namespace state（mod_states 断言在 Rust 侧补充核对）。

夹具包见 [mod-objhits](../mod-objhits/README.md)；Rust 侧断言（双通道、
组合身份、冷恢复）见 `crates/kk-cli/tests/sim_tests/obj_hits.rs`。

```bash
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-objhits/scenario-combined.json --ticks 4 --report /tmp/objhits.json
```
