# G02.d3 首片：计划级墙地形静态通行声明（2026-10-08）

基线 `f32f972`。开放内建规则 `builtin:terrain.wallPassable`（整数域
{0,1}，默认 0），让独立 JS 包以计划级声明放行墙地形；契约正文见
[移动领域·静态通行声明](../../architecture/movement.md#静态通行声明g02d3)。

## 输入范围与数值域

- 声明值：`api.rules.override('builtin:terrain.wallPassable', 1)`；
  冻结域 `Signed { min: 0, max: 1 }`，越界值在 freeze 层拒绝。
- 规则是计划级单值（非逐房/逐 tick）；builtin revision v16→v17，
  PlanHash 变化，旧检查点按既有身份门拒绝（无载荷格式变化）。

## 消费面（三点同源）

1. 引擎仲裁墙项：`static_obstacles::arbitration_blocked` 增加
   `walls_passable` 参数（`Movement` 持有，`Working::new` 从 Plane 注入；
   链预演 `simulate_survivors` 克隆同值）。登记阶段本就不查地形。
2. 预检槽输入 `native_static_blocked` 走同一函数；`terrainWall/
   terrainSwamp` 摘要保持物理值。
3. 玩家面原生寻路地形：`TerrainPack::native_with_walls` 打包后清
   每个真实房间两位码的 wall 位（1→平原、3→沼泽）；全墙保护环不
   随声明放开。Frozen 通道在 embed 构建时带引擎旗标（缓存键含旗标，
   切包后同世界代际也重建）；json/kkpv 通道经新增
   `SandboxConfig::terrain_walls_passable` 物化时应用，canary 与
   玩家工作者配置均从计划装配。

执行语义零新分支：疲劳沿既有位掩码判沼泽路径，墙=平原费率、
墙|沼泽=沼泽费率。物理面（getTerrain/look/map.getTerrainAt/
FIND_EXIT/放置检查/道路 hits 系数）保持原值。

## 实测验证

- 引擎：3 项新增（放行+原生对照、墙|沼泽沼泽费率、动态占格仍阻挡）；
  套件全绿。
- runner：pathfinder 单测 3 项新增（清位+墙环保留、false=物理表、
  kkpv 物化应用声明且物理包不变）；沙箱串行 59 场景新增
  `terrain_walls_passability_impl`（默认 `true|4|4|9|wall` 对照声明
  `false|10|10|15|wall`——默认分支部分路径止于墙前一格，与 G02.d2
  钉死的 moveTo 边界一致）。
- sim：[sim-mod-terrain-walls](../../../compat/fixtures/sim-mod-terrain-walls/README.md)
  四场景（native/ghost/deepwalk/combined）双通道 + 检查点冷恢复 +
  `--verify-replay` + 四 plan_hash 身份互异；物理查询四场景均读回
  `TERRAIN_MASK_WALL`。
- 兼容与 bot：默认零行为变化，完整 `pnpm run check`、release 重建后
  棘轮逐用例对照与三组 bot 指标见[台账当前接手位置](../../plans/mod-next-batches.md)。

## 边界与后续

- 组合语义（freeze 既有约束）：两包覆盖同一规则即冲突，即使值相同；
  单轴规则无冗余共装，组合验收用跨规则（wallPassable+fatigueLimit）。
  等值共装若要放开属 freeze 组合语义变更，另行设计拍板。
- 结构/rampart 通行声明、逐房粒度、墙位自定义代价属后续切片；
  G02 父项保持部分完成。
- 复审结论：三路后台评审（正确性 / 同源与遗漏 / 契约与规范）无
  P0/P1 代码问题。P1 均为文档层并已清偿：native 场景描述由「原地
  不动」改为「沿墙面绕行靠近不越墙列」（实测终点 (24,29)）；
  combined/deepwalk「同规则取同一生效值」表述纠正为「同规则双包
  被 freeze 一律判覆盖冲突，组合验收用跨规则」。P3 清偿：switch.rs
  封闭集注释补 MOVE 两键与通行旗标、三处批次号注释改行为描述、
  清位测试补 x≠y 取样（行序写 (41,3) 列序读 ii=41\*50+3）、引擎补
  结构障碍不随声明放行测试。P3 登记：Worker 检查点快照不含旗标
  （依赖 create 路径强制覆盖的隐式约定）、kk-world place 无
  builtin_terrain_passability 对应物（catalog 测试兜底）、跨房墙环
  端到端场景与 allowStatic×声明叠加测试缺口。
