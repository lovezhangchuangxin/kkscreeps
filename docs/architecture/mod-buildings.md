# Mod 可建造建筑

可信 JS Mod 经公共 SDK 声明**可建造建筑**：配方（成本向量、耐久、寻路语义、仓库容量、生产配方）、**自定义字段**与**生命周期 tick** 全部经声明面表达——工地创建、多资源建造支付、完工变换、受击/维修/拆解、库存与生产由宿主通用执行器结算，字段读写与 tick 由宿主按冻结契约驱动。第二个同类包（不同规则）不经任何新 Rust 分支即可安装，本批以 [smelter](../../mods/official/smelter/README.md) 与 [panel](../../mods/official/panel/README.md) 双官方包验证。

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
    fields: {                       // 自定义字段：声明即获玩家面直供
      heat: { type: 'u32', min: 0, max: 1000, default: 7 },
      label: { type: 'string', maxBytes: 64, default: '' }, // 首次 tick 整体回写须有值
    },
    tick(ctx) {                     // 生命周期槽：先产后 tick
      ctx.target.fields['smelter:heat'] = Math.min(1000, (ctx.target.fields['smelter:heat'] ?? 7) + 1);
      ctx.target.fields['smelter:label'] ??= ''; // 手放结构也可能缺默认字段
      // 整体回写前补齐所有声明字段（包括 Missing 默认）：
      // undefined 直接运算得 NaN，会被整集校验拒绝
    },
  });
}
```

## 声明与冻结

- 键即完工结构的 `structureType`（命名空间键含冒号，结构上不可能与内建结构名冲突）；同一键不能再作对象类型。
- 成本/生产输入/输出引用的资源必须在合并目录中（内建 `RESOURCES_ALL` 或任一包声明——跨包引用按 manifest 依赖闭包装载后可用）。
- 数值域：成本数量 1..=1e9、耐久 1..=u32 上限、容量 1..=u32 上限、周期 1..=1e5 tick；有输入或产出的生产必须声明容量。玩家类名须为合法 JS 标识符且在建筑间唯一。
- 声明经 SDK 形状预检与 op 权威转换（未知成员 fail-loud），冻结后进 `BuildingCatalog` 并随贡献参与 PlanHash；组合报告新增 `buildings` 字段，`report_version` 为 4。
- 冻结层对成本、生产输入和生产输出分别拒绝重复资源名，并按资源名排序；直接构造 Rust 贡献也遵守同一约束。每个向量内资源唯一，同一资源可同时出现在输入和输出中，按净变化生产；只改变向量排列不改变 PlanHash，数量或资源变化仍改变身份。
- **字段**：字段名禁点号与冒号（绑定 `{ns}:{类型名}.field.{name}` 依赖名字段单射）；字段键与建筑键同命名空间且建筑内唯一（目录门）；schema 嵌套与默认值在冻结期校验；字段表按键稳定排序并参与 PlanHash（默认值/可见性/槽集变化均改变身份）。可见性首片只支持 `public`。
- **tick**：`tick(ctx)` 为函数即声明生命周期槽，槽键与绑定从建筑键派生（`{key}.tick` / `{key}.tick.process`，手写其他值拒绝）；阶段为 ObjectTick；process 为必需可执行绑定。

## 建造与生产结算

完工结构是 `RoomObject::Generic`（`structureType` 为命名空间键），因此工地、废墟、`FIND_STRUCTURES`、`FIND_MY_STRUCTURES` 与两条投影通道沿用既有 Generic 通路：

| 环节 | 行为 |
|---|---|
| 工地创建 | 玩家 `createConstructionSite` 预检白名单并入目录类型；总进度 = 最大成本量；无 RCL 配额（逐级配额未开放为配方面）；同格造价互斥把目录类型计为造价对象 |
| 建造支付 | WORK 效果（含 boost）推进进度，各成本资源按累计比例支付（`floor(amount·p/total)`），完工恰好付清。**boost 语义与内建有意的分歧**：支付恒锚定进度比例，boost 加速进度的同时按同比例提高支付速率（内建是"能耗按未强化效果"，boost 越多越省）；单资源成本仅在无 boost 时与内建 1:1 一致 |
| 完工 | 移除工地、插入 Generic 结构：耐久/归属/`store`+`storeCapacity`（有容量才有 store 键）来自配方；有 `Value` 默认的自定义字段落键（存储键 `{ns}:{name}`，与原生键结构上不相交），`Missing` 默认不写键 |
| 生产 | 每 `interval` tick（`time % interval == 0`）检查全部输入在库，再按 `当前库存合计 - 本次输入合计 + 本次产出合计` 校验最终容量，通过才原子消耗产出；满仓原料可转换，同资源输入/输出按净变化计算，溢出时不扣输入；无逐建筑相位状态，检查点恢复后语义不变 |
| 生命周期 tick | 声明了生产或 tick 的目录建筑进入房间循环的 `mod-building` 分派：**先产后 tick**，tick 读到的是本轮生产后的候选值。`ctx.target.fields` 是完整记录（原生键 + 冒号自定义键）：原生键（无冒号）任何漂移都拒绝——耐久/库存/归属不经此写面；自定义键按字段表整集校验（缺字段、域外值、未声明键均拒绝），合法写以「当前原生键 + 新自定义键」重组记录。正常拒绝（`api.decline`）在生命周期槽升格为故障：整轮候选丢弃 |
| 受击/维修/拆解 | 耐久上限取配方；维修/拆解/销毁/受袭开关/tower 维修门对目录类型开放（玩家面 dismantle 预检同样认目录类型）；拆解按成本构成占比瓜分伤害量/200——分母是成本总量，回收总值与内建单资源口径一致，超容掉落沿既有目录序 |
| 寻路与出生 | `obstacle` 阻断（矩阵 255、仲裁、移动登记与出生出口检查同样阻挡），`road` 按道路成本（矩阵 1、不阻挡），`plain` 不影响；工地同语义参与建造预检。出生的指定出口与其他出口都按目录检查，任何主人的障碍工地均阻挡出生，保持与普通移动只挡自家工地的区别 |
| 搬运 | transfer/withdraw 经 Generic 仓库链路，容量取 `storeCapacity`；产物/原料与内建资源同槽 |
| 工地移除 | 按各资源已付量的一半返还（命名掉落堆），与内建 progress/2 退能量同口径 |

## 玩家面

`player.js` 经 `api.buildings.bind(typeKey, Class)` 绑定玩家类（finalize 强制全部建筑绑定；只能绑定本包命名空间的类型，与 emit 目录同律）。安装清单 `gw.modApi.buildings` 携带键、寻路语义、成本资源键与**自定义字段存储键表**，供工地预检、代价矩阵、结构注册白名单、拆解预检与对象装配消费；预检 canary 与真实玩家同源。

**字段玩家面记录直供**：结构装配器为清单内的字段存储键（含冒号）定义实例级 getter（与内建字段同一 `rawData` 回查机制，两投影通道同源），玩家经 `gauge['bl:heat']` 读值，记录缺键（`Missing` 默认未写）读到 `undefined`；绑定类可在原型上补语义糖（`get heat() { return this['bl:heat']; }`）。服务端 tick 的行为仍只能经 `tick` 声明表达——玩家类只读。`mod types` 声明生成为每个有字段的建筑输出经全局符号分配的 `{构造器}Fields` 接口（碰撞时加数字后缀）（存储键成员，`Missing` 默认为可选成员），供玩家交叉类型使用（见[类型生成](mod-types.md)）。

绑定类的两条契约：**应继承 `OwnedStructure`**（只继承 `RoomObject` 时 `owner`/`my` 及 attack/repair/transfer 等 `instanceof Structure` 预检会拒绝，而引擎结算本身放行）；实例级内建 getter 会遮蔽类原型上的同名属性——`store`/`energy`/`energyCapacity` 已由内建 Store 语义提供（投影直供 `store/storeCapacity` 键），自定义读数请用 `api.view.field` 或另起属性名，不要在原型上重定义这些名字。

## 事务与恢复

- 建造族槽的日志化边界覆盖完工变换：JS 拒绝时工地移除、结构插入、事件、ID 游标与多资源支付整体撤销；生产是纯 Rust 结算，无拒绝面。生命周期 tick 的字段写面同样在候选事务内：原生键漂移或整集校验失败使整轮丢弃，字段终值随 Generic 记录持久，无需独立检查点面。
- 玩家晚期写回失败不发布候选；缓存重试不重复执行玩家代码，生产在同一 tick 恰好结算一次。
- 建筑与工地随房间快照持久；`BuildingCatalog` 由计划再生，PlanHash 覆盖配方、字段与槽身份——冷恢复续跑与连续运行等价（真实 CLI 子进程验证，见[检查点](checkpoints.md)）。
- **停用语义**：停用所属包后生命周期 tick（Mod 槽）随停用掩码停止，自定义键停在前值成为惰性数据；生产是 plane 上的 host 规则，与资源/仓储规则同口径存续（停用不改写已冻结的 plane），库存照常推进。

## 当前边界

- 逐建筑 RCL 配额、逐资源容量（`storeCapacityResource`）未开放；生产无事件与相位偏移（周期锚定 `time % interval`）。
- 字段可见性只支持 `public`（owner/server-only 裁切未开放）；字段无玩家面写路径（玩家类只读，写入只能来自服务端 tick）。
- 生产配方的容量可行性在冻结期校验（容量须容纳单周期全部输入与全部产出），容量不足的配方整体拒绝。
- 建筑没有独立的 FIND 常量（经 `FIND_STRUCTURES`/`FIND_MY_STRUCTURES` 查询）；`isActive` 对配额表外类型恒真。
- 市场与跨房对声明资源的拒绝边界不变（见[资源目录](mod-resource-catalog.md)）。

## 验收

- Rust：冻结语义与组合报告（`kk-mod-api` tests/freeze，含字段/槽声明面）、结算（`kk-engine` `mod_buildings`（含 lifecycle/regressions 子模块））、字段写面单测（`kk-engine` `settle::buildings::tests`）、事务边界（`kk-runtime` `mod_building_world` + `mod_building_fields`：tick 读写、原生键漂移整轮失败、decline 故障、停用回退）。
- 独立 JS 场景 [sim-mod-buildings](../../compat/fixtures/sim-mod-buildings/README.md) 三件套（smelter / smelter+panel / 无 Mod 对照）：真实玩家 VM 完成建造、投料生产、产物搬运、受击维修与寻路矩阵断言；双投影通道全等；检查点续跑等价（`kk-cli` `sim_tests/buildings.rs`）。证据见 [N2b 交付记录](../research/notes/n2b-mod-buildings-2026-10-05.md)。
- 字段与生命周期场景 [sim-mod-building-fields](../../compat/fixtures/sim-mod-building-fields/README.md) 四件套（bl:gauge / bk:beacon / 组合 / 无 Mod 对照）：两独立包的玩家面字段直供、先产后 tick、世界终态与冷恢复（`kk-cli` `sim_tests/building_fields.rs`）。证据见 [G08.a 交付记录](../research/notes/g08a-building-fields-2026-10-06.md)。
- 满仓生产、出生障碍与夹具干净源码验收见 [review 修复记录](../research/notes/n2b-mod-buildings-2026-10-05.md#n2b-review-修复记录)。

## 公共编辑与恢复校验

活动目录建筑的 Generic 记录在公共编辑、冷恢复和候选发布时统一验证已存在的冒号字段：类型、范围与未声明键均拒绝。手工放置可以省略字段，不会自动补默认；无变更 tick 也不能绕过已有值的校验。移除包后的惰性 Generic 建筑不再按活动目录校验（去包策略见包切换文档）。生命周期整集回写仍要求所有声明字段，须先补齐缺键；上方完整 JS 样例由集成测试直接读取并执行首次 tick。
