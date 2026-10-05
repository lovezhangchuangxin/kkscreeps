# smelter 官方内容包

新资源生产建筑的正式官方候选：使用公共 JS SDK（`@kkscreeps/mod`）声明
`smelter:ore` / `smelter:ingot` 资源与 `smelter:foundry` 熔炉，不依赖任何
包 ID 特权或私有 Rust 入口。

## 玩法规则

- 熔炉由玩家创建工地建造：成本 `{energy: 5000, smelter:ore: 200}`，
  两资源按进度比例累计支付（1 点 ≈ 1 能量 + 0.04 矿）。
- 每 12 tick 一个生产周期（配置 `interval`）：库存同时有 60 能量与 30 矿
  且容量足够时，产出 10 锭（配置 `yield`）。
- 仓库容量 1000（全资源共享），耐久 5000，可受击、维修、拆解与销毁；
  阻挡寻路与移动。
- 完工结构即 `structureType === 'smelter:foundry'` 的普通结构：
  进入 `FIND_STRUCTURES` / `FIND_MY_STRUCTURES`，可被 transfer/withdraw。

## 玩家面

`player.js` 绑定 `SmelterFoundry extends Structure` 并暴露
`SMELTER_ORE` / `SMELTER_INGOT` 常量；`foundry.ore` / `foundry.ingots`
读当轮库存。

## 运行

见 `compat/fixtures/sim-mod-buildings/`（建造、生产、搬运、受击维修与
寻路的端到端场景）。
