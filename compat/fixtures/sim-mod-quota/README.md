# 建筑 RCL 配额场景（G11.a）

RCL 8 单房四场景：native（无 Mod，锚定官方配额）、raise（tower rcl8=20）、
lower（constructedWall rcl8=3）、combined（两包同计划）。

bot（main.js）三轮探针：首轮连建 10 tower / 5 wall 工地——单轮内预检只看
已提交对象（全放行），结算按配额收口（native 提交 6 个 tower，raise 提交
10 个）；次轮各补建一个，此刻预检读「已提交数 vs 注入配额表」，是预检与
结算同源的判定点（10 个已提交 tower：官方 6 拒 `-14`，覆盖 20 放行 `0`；
3 个已提交 wall：官方 2500 放行，覆盖 3 拒）；第三轮回读已提交世界的
工地数核对结算门。注入表读值 `CONTROLLER_STRUCTURES[type]['8']` 与停止码
一并断言。

夹具包见 [mod-quota](../mod-quota/README.md)。Rust 侧断言（双通道一致、
组合身份、冷恢复）见 `crates/kk-cli/tests/sim_tests/quota.rs`。

```bash
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-quota/scenario-combined.json --ticks 3 --report /tmp/quota.json
```
