# sim-mod-terrain-walls：G02.d3 计划级墙地形静态通行声明

四个场景共享同一地形（x=25 全高墙列）与玩家代码（`main.js`）：

- `scenario-native.json`：不装声明包，墙按官方物理不可通行——moveTo
  目标不可达，沿墙面绕行靠近但不越墙列（6 tick 后位于 (24,29)），
  PathFinder.search 报 incomplete、部分路径止于墙前。
- `scenario-ghost.json` / `scenario-deepwalk.json`：两个独立作者包各自
  声明 `builtin:terrain.wallPassable = 1`，穿墙 6 步抵达 (28,25)，
  search 完整——证明声明面不按包 ID 特判。两者覆盖同一规则，**不能
  共装**（freeze 对同规则双包一律判覆盖冲突，即使值相同；单轴规则
  没有冗余共装语义）。
- `scenario-combined.json`：ghost 与 tireless（fatigueLimit，不同轴
  规则）跨规则组合安装，两规则同时生效，行为与单包一致。

断言要点：位置/寻路结果随声明变化；`getTerrain().get(25,25)` 在全部
场景保持 `TERRAIN_MASK_WALL`（物理查询不随派生通行视图改写）。测试
见 `crates/kk-cli/tests/sim_tests/terrain_walls.rs`（双通道 + 检查点
冷恢复 + 回放核对 + 四 plan_hash 身份互异）。
