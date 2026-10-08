# 结构受击槽夹具（G09.c）

两个独立包，共用 builtin:mod.receiveDamage 结构受击槽（介入点在结构
扣减前，输出修改后的伤害量；目标形状经 ctx.target 的耐久信封承载）：

- **objguard-warden**（减伤型）：`objguard-warden:aegis` 耐久 200；
  Replace 把当次伤害减半（30 → 15），正常拒绝回退默认伤害。
- **objguard-logbook**（记录型）：`objguard-logbook:monolith` 耐久
  200；After 观察者不改伤害，记录当次输入（伤害/来源/扣减前耐久读值）。

两包同计划共存（sim-mod-objguard 的 combined 场景）时，减伤中心对全部
Mod 目标生效，观察者读到链尾最终伤害量——独立包经公共槽组合，互不
感知。sim 场景见 [sim-mod-objguard](../sim-mod-objguard/README.md)；
Rust 侧断言（双通道、组合身份、冷恢复）见
`crates/kk-cli/tests/sim_tests/obj_guard.rs`。
