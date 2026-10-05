# Mod 可建造建筑

可信 JS Mod 经公共 SDK 声明**可建造建筑**：配方（成本向量、耐久、寻路语义、仓库容量、生产配方）是纯数据，由宿主通用执行器结算——工地创建、多资源建造支付、完工变换、受击/维修/拆解、库存与生产全部不需要作者提供处理器。第二个同类包（不同规则）不经任何新 Rust 分支即可安装，本批以 [smelter](../../mods/official/smelter/README.md) 与 [panel](../../mods/official/panel/README.md) 双官方包验证。

```js
export default function register(api) {
  api.resources.define('smelter:ore');
  api.resources.define('smelter:ingot');
  api.buildings.define('smelter:foundry', {
    playerClass: 'SmelterFoundry',
    cost: { energy: 5000, 'smelter:ore': 200 },
    hitsMax: 5000,
    pathing: 'obstacle',            // obstacle | plain | road
    capacity: 1000,                 // 仓库总容量；省略则无仓库
    production: {
      interval: 12,                 // 周期 tick 数（房间绝对时间取模）
      inputs: { energy: 60, 'smelter:ore': 30 },
      outputs: { 'smelter:ingot': 10 },
    },
  });
}
```

## 声明与冻结

- 键即完工结构的 `structureType`（命名空间键含冒号，结构上不可能与内建结构名冲突）；同一键不能再作对象类型。
- 成本/生产输入/输出引用的资源必须在合并目录中（内建 `RESOURCES_ALL` 或任一包声明——跨包引用按 manifest 依赖闭包装载后可用）。
- 数值域：成本数量 1..=1e9、耐久 1..=u32 上限、容量 1..=u32 上限、周期 1..=1e5 tick；有输入或产出的生产必须声明容量。玩家类名须为合法 JS 标识符且在建筑间唯一。
- 声明经 SDK 形状预检与 op 权威转换（未知成员 fail-loud），冻结后进 `BuildingCatalog` 并随贡献参与 PlanHash；组合报告新增 `buildings` 字段，`report_version` 为 4。

## 建造与生产结算

完工结构是 `RoomObject::Generic`（`structureType` 为命名空间键），因此工地、废墟、`FIND_STRUCTURES`、`FIND_MY_STRUCTURES` 与两条投影通道沿用既有 Generic 通路：

| 环节 | 行为 |
|---|---|
| 工地创建 | 玩家 `createConstructionSite` 预检白名单并入目录类型；总进度 = 最大成本量；无 RCL 配额（逐级配额未开放为配方面）；同格造价互斥把目录类型计为造价对象 |
| 建造支付 | WORK 效果（含 boost）推进进度，各成本资源按累计比例支付（`floor(amount·p/total)`），完工恰好付清。**boost 语义与内建有意的分歧**：支付恒锚定进度比例，boost 加速进度的同时按同比例提高支付速率（内建是"能耗按未强化效果"，boost 越多越省）；单资源成本仅在无 boost 时与内建 1:1 一致 |
| 完工 | 移除工地、插入 Generic 结构：耐久/归属/`store`+`storeCapacity`（有容量才有 store 键）来自配方 |
| 生产 | 每 `interval` tick（`time % interval == 0`）检查：全部输入在库且产出合计不超容量时原子消耗产出；无逐建筑相位状态，检查点恢复后语义不变 |
| 受击/维修/拆解 | 耐久上限取配方；维修/拆解/销毁/受袭开关/tower 维修门对目录类型开放（玩家面 dismantle 预检同样认目录类型）；拆解按成本构成占比瓜分伤害量/200——分母是成本总量，回收总值与内建单资源口径一致，超容掉落沿既有目录序 |
| 寻路 | `obstacle` 阻断（矩阵 255、仲裁与移动登记同样阻挡），`road` 按道路成本（矩阵 1、不阻挡），`plain` 不影响；工地同语义参与建造预检 |
| 搬运 | transfer/withdraw 经 Generic 仓库链路，容量取 `storeCapacity`；产物/原料与内建资源同槽 |
| 工地移除 | 按各资源已付量的一半返还（命名掉落堆），与内建 progress/2 退能量同口径 |

## 玩家面

`player.js` 经 `api.buildings.bind(typeKey, Class)` 绑定玩家类（finalize 强制全部建筑绑定；只能绑定本包命名空间的类型，与 emit 目录同律）。安装清单 `gw.modApi.buildings` 携带键、寻路语义与成本资源键，供工地预检、代价矩阵、结构注册白名单、拆解预检与对象装配消费；预检 canary 与真实玩家同源。`api.buildings.define` 没有 tick/字段/生命周期——建筑状态就是库存与耐久，自定义行为应通过挂载既有槽（如 `builtin:creep.build` 的 wrap）表达。

绑定类的两条契约：**应继承 `OwnedStructure`**（只继承 `RoomObject` 时 `owner`/`my` 及 attack/repair/transfer 等 `instanceof Structure` 预检会拒绝，而引擎结算本身放行）；实例级内建 getter 会遮蔽类原型上的同名属性——`store`/`energy`/`energyCapacity` 已由内建 Store 语义提供（投影直供 `store/storeCapacity` 键），自定义读数请用 `api.view.field` 或另起属性名，不要在原型上重定义这些名字。

## 事务与恢复

- 建造族槽的日志化边界覆盖完工变换：JS 拒绝时工地移除、结构插入、事件、ID 游标与多资源支付整体撤销；生产是纯 Rust 结算，无拒绝面。
- 玩家晚期写回失败不发布候选；缓存重试不重复执行玩家代码，生产在同一 tick 恰好结算一次。
- 建筑与工地随房间快照持久；`BuildingCatalog` 由计划再生，PlanHash 覆盖配方身份——冷恢复续跑与连续运行等价（真实 CLI 子进程验证，见[检查点](checkpoints.md)）。

## 当前边界

- 逐建筑 RCL 配额、逐资源容量（`storeCapacityResource`）、建筑自定义字段与建筑面 tick 处理器未开放；生产无事件与相位偏移（周期锚定 `time % interval`）。
- 生产配方的容量可行性在冻结期校验（容量须容纳单周期全部输入与全部产出），容量不足的配方整体拒绝。
- 建筑没有独立的 FIND 常量（经 `FIND_STRUCTURES`/`FIND_MY_STRUCTURES` 查询）；`isActive` 对配额表外类型恒真。
- 市场与跨房对声明资源的拒绝边界不变（见[资源目录](mod-resource-catalog.md)）。

## 验收

- Rust：冻结语义与组合报告（`kk-mod-api` freeze_cases、`kk-mod-host` declarations）、结算闭环 11 项（`kk-engine` `mod_building_family`）、事务边界 2 项（`kk-runtime` `mod_building_world`）。
- 独立 JS 场景 [sim-mod-buildings](../../compat/fixtures/sim-mod-buildings/README.md) 三件套（smelter / smelter+panel / 无 Mod 对照）：真实玩家 VM 完成建造、投料生产、产物搬运、受击维修与寻路矩阵断言；双投影通道全等；检查点续跑等价（`kk-cli` `sim_tests/buildings.rs`）。证据见 [N2b 交付记录](../research/notes/n2b-mod-buildings-2026-10-05.md)。
