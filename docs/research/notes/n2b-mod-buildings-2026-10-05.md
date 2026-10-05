# N2b Mod 建筑交付记录（2026-10-05）

基线 f84b130（N4a）。本批三个提交：声明域（`api.buildings.define` 冻结目录）、引擎消费面（工地/建造/完工/伤害/维修/拆解/寻路/生产）、玩家面与官方内容包。验收环境与命令如下；数字取自实际运行输出。

## 提交与范围

| 提交 | 范围 |
|---|---|
| N2b-1 | kk-mod-api `buildings` 域（描述符/目录/PlanHash/组合报告 v4）、SDK 与 op 转换、冻结语义测试 |
| N2b-2 | Plane/Engine/Movement 装配目录；工地创建、多资源累计支付、完工 Generic、hitsMax/维修/拆除/销毁门、寻路语义、生产周期、场景放置默认值；`mod_building_family` 11 项 |
| N2b-3 | ModPlayerApi buildings 清单与 41 段绑定、预检白名单/配额跳过/互斥/代价矩阵/结构注册/类装配；官方包 smelter+panel；sim-mod-buildings 三场景；CLI 测试 4 项；事务边界测试 2 项 |

## 机器验证

- `cargo test --workspace`：**838 通过 / 0 失败**（基线 809 + 本批新增 29：冻结 9、注册声明 2、结算 11、事务 2、CLI 4、mod-player 清单 1）。
- 三场景人工核对（`target/debug/kkscreeps sim run`）：
  - `scenario.json`（320 tick）：`grid_foundry=255`、`foundry_hits=5000`（受击 6 次后维修回满）、`foundry_ingots=40`、`storage_ingots=100`（产物真实搬回仓库）、`foundry_owner=p1`。
  - `scenario-combined.json`（320 tick）：另得 `grid_panel=1`、`panel_energy=200`（免费生产至容量停止）、`panel_hits=300`；smelter 各值不回归；组合报告 `buildings=[panel:collector, smelter:foundry]`、selection `[panel, smelter]`。
  - `scenario-plain.json`（8 tick）：`site_rejected / withdraw_rejected / no_sites / no_foundry` 全真，世界仅存内建对象。
- 双通道全等：`sim_tests/buildings.rs` 内 JSON/KKPV 两份报告 `final.world`、`plan_hash` 逐项相等（主场景与组合场景）。
- 检查点：`cold_resume_matches_uninterrupted_building_world` 以真实子进程 170 tick 保存退出、恢复续跑 150 tick，最终世界（含建筑/工地/库存）与一次跑完 320 tick 的参考全等（仅剥离实测 CPU 桶）。
- 事务边界：建造槽 after 拒绝后工地进度/多资源支付/事件/结构插入整体回滚；`fail_next_writeback(RawMemory)` 后缓存重试，生产恰好一次、`Memory.n=1`（玩家代码不重复执行）。

## 关键实现决定

1. **载体选 Generic 结构**：结构类型本是字符串，工地/受击/废墟/投影/FIND 对字符串天然开放——阻挡点集中在成本白名单、hitsMax 表、维修/拆除门、寻路表、transfer 白名单与 RCL 配额，逐一按目录放开。`RoomObject::Mod` 保持对象域语义不动。
2. **多资源支付锚定累计比例** `floor(amount·p/total)`：单资源成本严格退化为内建 1:1；boost 加速进度同时按比例提高支付速率，完工恰付清，无需逐工地付款账本。
3. **生产是宿主原生周期**（`time % interval`，无相位状态）：检查点恢复零额外状态；玩法差异（配方/周期/寻路语义）全部由声明表达，第二个包零 Rust 改动。
4. 玩家类必须 `api.buildings.bind`（finalize 强制），推荐继承 `OwnedStructure` 取得 `owner`/`my`。

## 已知边界

登记在[架构文档](../../architecture/mod-buildings.md#当前边界)：逐级 RCL 配额、逐资源容量、建筑自定义字段/tick、生产事件、建筑专属 FIND 常量未开放；市场/跨房对声明资源维持 N2a 拒绝。D 批遗留（link tick 拒绝、attackController、D7 性能）不变。

## 后续

N2c 行为策略（具名伤害公式/移动仲裁替换入口）；建筑 RCL 配额与生产事件按真实包需要开放。
