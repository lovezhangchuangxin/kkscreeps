# 自有对象库存夹具（G09.a）

hauler 与 vault 是两个相互独立的库存对象包：分别声明 `objstore-hauler:crate`
（capacity 300，另有不设容量的 `objstore-hauler:marker` 对照类型）与 `objstore-vault:cache`
（capacity 100）。类型经 `api.objects.define` 的 `capacity` 声明总库存容量；
对象创建即携带 store/storeCapacity 信封（初始 `{"energy":0}`，与 Mod 建筑
完工同形），库存只能经资源动作（creep transfer/withdraw、mod 资源槽）变动，
字段更新拒绝写信封键；玩家类经 `api.view.field` 读同一投影信封。

场景与断言见 [sim-mod-objstore](../sim-mod-objstore/README.md)；Rust 侧
双通道/组合/冷恢复断言见 `crates/kk-cli/tests/sim_tests/obj_store.rs`。

```bash
cargo build -p kk-cli
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-objstore/scenario-combined.json --ticks 4 --report /tmp/objstore.json
target/debug/kkscreeps mod inspect --root compat/fixtures/mod-objstore --report /tmp/objstore-inspect.json
```
