# 自有对象耐久夹具（G09.b）

siege 与 vault 是两个相互独立的耐久对象包：分别声明 `objhits-siege:barrel`
（hitsMax 300，另有不设耐久的 `objhits-siege:decoy` 对照类型）与
`objhits-vault:cache`（hitsMax 200 + capacity 100，耐久×库存双信封并存）。
类型经 `api.objects.define` 的 `hitsMax` 声明耐久；对象创建即满血携带
hits/hitsMax 信封，耐久只能经伤害动作（creep attack/rangedAttack、塔）
变动，字段更新拒绝写信封键；扣至 0 即移除并发 objectDestroyed 事件
（type 为类型键，不产废墟）；玩家类经 `api.view.field` 读同一投影信封，
attack 预检的 hits 在场门与结算 object_hits>0 门同源。

场景与断言见 [sim-mod-objhits](../sim-mod-objhits/README.md)；Rust 侧
双通道/组合/冷恢复断言见 `crates/kk-cli/tests/sim_tests/obj_hits.rs`。

```bash
cargo build -p kk-cli
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-objhits/scenario-combined.json --ticks 4 --report /tmp/objhits.json
target/debug/kkscreeps mod inspect --root compat/fixtures/mod-objhits --report /tmp/objhits-inspect.json
```
