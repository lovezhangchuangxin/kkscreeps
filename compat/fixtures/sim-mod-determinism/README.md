# sim-mod-determinism 夹具

G22.b 确定性玩法时间/RNG 的场景级验收夹具。包内容见
[mod-determinism](../mod-determinism/README.md)（dice：每 tick 抽值写 state）。

- `scenario-dice.json`（seed 12345）与 `scenario-dice-alt.json`（seed 999）：
  黄金抽值断言钉死协议输出（`/mod_states/dice/draws/0` 等）；两场景黄金值
  相异即种子生效。断言刻意只取 tick 1 就存在的指针（init 域与前两抽），
  任意 tick 数（≥1）的运行都可通过——冷恢复分相运行不因终态断言误红。
- `main.js`：玩家空 loop（协议验收不依赖玩家输入）。
- Rust 测试 `crates/kk-cli/tests/sim_tests/determinism.rs`：双场景黄金通过 +
  异种子完整对照；6 tick 直通 vs 3+检查点+3 续跑逐位等值（双进程）。

场景 `seed` 字段缺省 0（与既有场景行为一致）；检查点续跑时种子来自载荷。
