# sim 意图压制夹具（G07.b）

包（`../mod-intent-suppression/`）：

- `suppress`：counter 类型 + `suppress:bump`（置 amount）+ `suppress:mark`
  （置 mark，**声明被 bump 压制**）。
- `free`：同形对照，不声明压制。

bot 按 memory 的 `mode`/`prefix` 每轮发射扩展意图；期望（结算后每 tick 稳定）：

| 场景 | 包 | 发射 | 期望 value | 说明 |
|---|---|---|---|---|
| both | suppress | bump(3)+mark(7) | 3 | mark 被压制 |
| alone | suppress | mark(7) | 7 | 压制者缺席则执行 |
| invalid | suppress | bump(99)+mark(7) | 0 | 压制只看键存在（bump 越界被丢弃仍压制 mark） |
| free | free | bump(3)+mark(7) | 7 | 无压制：键序先置 3 后置 7 |
| combined | suppress+free | 两类型各齐发 | 3 / 7 | 两包同计划共存，压制按包内声明各自生效 |

验收：sim_tests/intent_suppression.rs——五场景（含组合选择）×JSON/KKPV 双通道、
第 2 tick 检查点冷恢复续跑等值、suppress 三场景同计划身份/free 单选与组合三个身份互异。
引擎级语义矩阵（含跨对象隔离）见
kk-engine c3_ext_intent_family/suppression.rs；SDK/冻结校验与冷恢复见
kk-runtime c3_counter_chain_family。
