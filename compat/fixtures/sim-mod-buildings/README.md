# sim-mod-buildings 场景

N2b 建筑面端到端验收：同一 `main.js` 服务三个世界，经公开
`kkscreeps sim run` 驱动真实玩家 VM（建造、投料、搬运、受击维修、
寻路矩阵读取全部是玩家可见行为）。

| 场景 | 安装内容 | 验证 |
|---|---|---|
| `scenario.json` | smelter | 多资源建造闭环（工地创建→按比例支付→完工）、生产投料、产物搬运回仓、受击 6 次后维修至满、代价矩阵 obstacle=255 |
| `scenario-combined.json` | panel + smelter | 两个规则不同的包并存：panel 单资源成本、road 寻路（矩阵=1）、免费生产；smelter 行为不回归 |
| `scenario-plain.json` | 无 | 工地创建与资源搬运在玩家预检被 `ERR_INVALID_ARGS` 拒绝，无任何残留对象 |

## 运行

在仓库根目录构建 CLI 后：

```bash
cargo build -p kk-cli
target/debug/kkscreeps sim run \
  --scenario compat/fixtures/sim-mod-buildings/scenario.json \
  --ticks 320 --report /tmp/sim-mod-buildings.json
```

组合与对照场景分别替换 `scenario-combined.json` / `scenario-plain.json`
（对照场景 `--ticks 8` 即可覆盖全部断言分支）。断言经
`/players/p1/memory/*`（运行期玩家可读值）钉住行为，避免依赖内部表示。

自动化验证（双投影通道全等、检查点续跑）见
`crates/kk-cli/tests/sim_tests/buildings.rs`。
