# G02.c 意图静态预检场景

第 0 tick 四个移动者并行发出请求：`w1`（`move`）向右进入西区边缘格 (6,30)；`strong`（`move,move`，功率 2 重量 1）与 `weak`（`move`）竞争 (40,19)（原生 strong 胜出）；`free`（`move`）独自向西。之后各 tick 只记录位置。

| 场景 | 预检行为 | w1（西区） | strong/weak（竞争格） |
|---|---|---|---|
| `scenario-plain.json` | 原生静态预检（结构/私有 rampart，均不触发） | 移动 `(6,30)` | strong 胜出 `(40,19)`、weak 原地 |
| `scenario-fenced.json` | fenced 包 replace：`targetX<10` 否决 | 原地 `(5,30)` | 同原生 |
| `scenario-courtesy.json` | courtesy 包 after：`movePower>weight` 否决 | 移动 | strong 原地、**weak 独占竞争格 `(40,19)`** |
| `scenario-combined.json` | fenced replace + courtesy after 并集 | 原地 | strong 原地、weak 移动 |

- **请求级语义**：预检否决的是**请求登记**（竞争输入构造之前移除，等同从未登记）——courtesy 否决 strong 的请求后 weak 成为 (40,19) 的唯一登记者而胜出；对照障碍槽（G02.b）挡下**胜者**后无人可动，两级语义不同。
- 输入为整房请求摘要（`moverId/owner/originX/originY/targetX/targetY/movePower/weight/pulled/pulling` + 目的格 `terrainWall/terrainSwamp/hasRoad/objects[]` 占位摘要），按对象 ID 升序、不截断；这些场景中的请求均通过原生静态预检；G02.d1 起公共输入也包含暂存静态拒绝，见[现行契约](../../../docs/architecture/mod-move-precheck.md)。
- 能力 `ctx.precheck.isBlocked/block` 单向收紧；正常拒绝整房丢弃否决（等同没进过 JS）。被否决请求不参与竞争评分、不让位、不占格（spawn 放置的 `is_tile_busy` 读不到它），与原生挡下的请求下游一致。

```bash
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-precheck/scenario-combined.json --ticks 3 --report /tmp/precheck.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-move-precheck/scenario-courtesy.json --ticks 3 --kkpv --report /tmp/precheck-courtesy.json
```

`kk-cli` 的 `sim_tests/move_precheck.rs` 核对四场景 × JSON/KKPV 的完整 final、PlanHash 与组合身份（四身份互异，内容选择与组合分别冻结；初批内建槽 17→18、revision v11；现行为 18 槽、revision v15）；在第 1 tick 后保存检查点、退出进程、加载再续跑，确认位置与 namespace 计数恰一次延续。策略包在 [../mod-move-precheck/](../mod-move-precheck/)。
