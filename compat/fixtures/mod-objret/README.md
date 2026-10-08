# 自有对象反击夹具（G09.d）

thorn 与 reactor 是两个相互独立的反击对象包：分别声明 `objret-thorn:spike`
（hitsMax 300 + retaliationRatio 0.5，官方 powerBank damage×0.5 同构）与
`objret-reactor:core`（hitsMax 200 + retaliationRatio 1.0 + capacity 100，
耐久×反击×库存三声明组合）。反击任意攻击类型触发、不耗自身耐久：creep
攻击者走延迟队列（同轮 tick 落账），塔/结构攻击者立即扣减且禁递归；
HIT_BACK 事件先于主攻击事件。反击比率随类型冻结（(0,1] 毫精度），要求
耐久声明。

场景与断言见 [sim-mod-objret](../sim-mod-objret/README.md)；Rust 侧
双通道/组合/冷恢复断言见 `crates/kk-cli/tests/sim_tests/obj_ret.rs`。

```bash
cargo build -p kk-cli
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-objret/scenario-combined.json --ticks 4 --report /tmp/objret.json
```
