# 结构受击槽场景（G09.c）

三场景：warden（减伤型 Replace——aegis 耐久 200，30/击减半为 15）、
logbook（记录型 After——monolith 默认伤害 30/击落地，观察者记输入）、
combined（两独立包同计划：减伤中心对全部 Mod 目标生效，两目标同为
155）。bot（main.js）四轮探针（T0 发动作并读初值、T1 读提交并续击、
T2 再击、T3 终值）：brawler/skirmisher 各以单 ATTACK 部件近战经营一个
目标；包缺席时对应 creep 空转，三场景共用同一探针。

夹具包见 [mod-objguard](../mod-objguard/README.md)；Rust 侧断言（双通
道、mod_states 观察、组合身份、冷恢复）见
`crates/kk-cli/tests/sim_tests/obj_guard.rs`。

```bash
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-objguard/scenario-combined.json --ticks 4 --report /tmp/objguard.json
```
