# sim-mod-switch：无开放轮次的包切换夹具

验证场景字段 `mods_switch`（见[包切换](../../../docs/architecture/mod-switching.md)）：

- **scenario-add.json**：official 根，`nursery`（period 2）→ 第 2 轮后增至 `nursery + clockwork`；断言切换后世界续跑、emitter 计数连续。Rust 侧（`crates/kk-cli/tests/sim_tests/switch.rs`）另断言报告 `mods_switches` 的 applied 状态、增删差集与前后 plan_hash/执行策略身份变化。
- **scenario-remove.json**：mod-composition 根，全选 → 第 2 轮后仅 `base`；z-gate 的 `LINK_CAPACITY=700` 覆盖随包移除，bot 记录的玩家常量回落原生 800——证明计划派生常量按新计划对已运行玩家生效；`base` 的 state 计数跨切换连续。
- **冷恢复**：add 场景三段式（直跑 6 / 2 轮后保存 / 续跑 4），续跑以切换后包集合身份通过 ModIdentity/plan_hash/执行策略三道门，终态与直跑等价且不重放切换。

双通道（JSON/KKPV）由 Rust 测试循环覆盖。
