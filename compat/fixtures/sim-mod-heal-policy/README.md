# 独立 JS 治疗策略场景

`../mod-heal-policy/flat` 将近程/远程治疗都改为固定 23，`scaled` 将治疗量改为缺失生命的一半（向上取整）；两者各自是公共 SDK 包，没有宿主专用分派。场景单选其中一个包，plain 为无 Mod 对照；同时选两个包会按既有多个 replace 冲突规则拒绝。

玩家在 time=0 以原生攻击制造 60 伤害，time=1 治疗，后续观察身体耐久。场景须运行至少三轮；验收使用四轮。目标初始 hits=200，预伤害后 140；flat 最终 163，scaled 170，plain 近程/远程分别为 152/144。治疗恢复首个受损 MOVE 部件，另一个部件保持 100；每个 Mod 的 applied=1。

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-heal-policy/scenario-flat-heal.json --ticks 4 --report /tmp/flat-heal.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-heal-policy/scenario-scaled-rangedHeal.json --ticks 4 --kkpv --report /tmp/scaled-ranged-heal.json
```

六个场景（flat/scaled/plain × heal/rangedHeal）覆盖 JSON/KKPV。CLI 测试另在预伤害后、治疗完成后分别保存检查点，退出进程再续跑；完整 final（排除实际 CPU 桶）、namespace state、组合和 PlanHash 与连续四轮相等。
