# G08.a 建筑字段与生命周期场景

`gauge`（`bl:gauge`，字段 heat/cycles + 每 tick 产 5 能量 + tick 槽）与
`beacon`（`bk:beacon`，无生产纯 tick，signal 翻转）两个互不依赖的包。
结构经场景手放（不带自定义键——完工默认只在建造流写入，首写由 tick
整集补齐）；玩家代码先于结算，`--ticks 4` 时末轮玩家读到 3 轮结算效果，
世界终态含第 4 轮。

| 场景 | 选择 | 末轮玩家 memory | 世界终态 |
|---|---|---|---|
| `scenario-plain.json` | 无 Mod（原生对照） | `hasGauge/hasBeacon` 均 false | link 原样 |
| `scenario-gauge.json` | gauge | heat=3、cycles=3、energy=15、cls=`Gauge` | `bl:heat`=4、store.energy=20 |
| `scenario-beacon.json` | beacon | signal=1、flashes=3、cls=`Beacon` | `bk:flashes`=4 |
| `scenario-combined.json` | gauge + beacon | 两单包值同时成立 | 两终态同时成立 |

- **玩家面记录直供**：清单装配按存储键（含冒号命名空间）暴露属性；
  玩家类在其上加语义糖（`gauge.heat`）。
- **先产后 tick**：tick 读到的 `store.energy` 是本轮生产后的候选值
  （引擎/运行时测试另以观察序列钉死）。
- **字段域独立于生产面**：beacon 无仓库无生产，字段与 tick 照常。

```bash
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-building-fields/scenario-combined.json --ticks 4 --report /tmp/bf.json
target/debug/kkscreeps sim run --scenario compat/fixtures/sim-mod-building-fields/scenario-gauge.json --ticks 4 --kkpv --report /tmp/bf-kkpv.json
```

`kk-cli` 的 `sim_tests/building_fields.rs` 核对四场景 × JSON/KKPV 的完整
final、PlanHash（四身份互异）与组合报告，以及第 2 tick 保存检查点、退出
进程、加载再续跑的冷恢复一致。策略包在 [../mod-building-fields/](../mod-building-fields/)。
