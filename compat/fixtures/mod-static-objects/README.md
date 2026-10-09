# 计划级静态对象通行示例（当前）

`ghost` 与 `deepwalk` 是两个独立包，各自声明 `builtin:movement.obstaclesPassable = 1`；选择其中一个即可让内建静态障碍类型（含 source/mineral）、Mod obstacle 建筑及己方障碍工地在登记/仲裁和默认寻路矩阵中放行。生物占格和 portal 默认寻路避让保持。

`ramparts` 独立声明 `builtin:movement.privateRampartsPassable = 1`，放行非公开他人 rampart。自有/公开 rampart 原生即通行；它可与 ghost 或 deepwalk 组合。ghost 与 deepwalk 同时选择会因同规则覆盖冲突被拒，等值声明也不例外。

两个规则默认 0，值域只能为整数 0/1。墙地形另用 `builtin:terrain.wallPassable`；三轴可以组合。声明是计划级单值，未开放逐房粒度，不改变建造/孵化放置、物理查询或玩家显式 CostMatrix。玩家默认 moveTo/findPath 与 NPC 搜索消费同一冻结快照；裸 PathFinder.search 原生只查地形，自定义矩阵由调用者负责。

回归：[static_objects.rs](../../../crates/kk-runtime/tests/mod_chain_tests/static_objects.rs) 覆盖 JSON/KKPV、两独立包/两轴组合、默认矩阵与真实移动、Mod 建筑完工、powerCreep、动态占格、不可逆否决、输入写回失败缓存重试及冷恢复。
