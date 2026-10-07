# 权威状态编辑与候选发布

Engine 的世界、玩家、市场、Mod state 和最近事件只读，通过 `world()`、`players()`、`market()`、`mod_states()` 和 `last_events()` 获取。`edit_world`、`edit_players`、`edit_market`、`edit_mod_states` 编辑隔离副本；闭包返回错误或校验失败时，原状态、版本和缓存保持。存在开放轮次或待恢复候选时拒绝编辑。账户、市场和 namespace 状态的成功编辑同样推进世界 generation，旧玩家回执不能继续使用。

World 不再公开整房可变引用或对象可变引用。`edit_room` 只提供对象、扩展字段和事件；`edit_object` 提供单个记录并在发布前检查键与 ID 一致。地形通过 `set_terrain` 修改，仍只允许首次玩家 tick 前。`replace_room` 用于导入单房隔离候选，拒绝改变地形、重复 ID、非法坐标和不合法的分配游标。跨房移动应使用 `commit_tick_rooms`，避免逐房替换产生中间重复 ID。

生产结算一次提交全部房间。提交前核对覆盖集合、房间键、跨房 ID 唯一性、记录 ID、坐标、扩展对象存在性、地形保持和游标；所有对象 ID 非零且小于下一游标。generation 的 `u64::MAX` 保留为缓存失效哨兵，不可成为有效版本；时间、player_ticks 和 generation 的递增也先验证余量，再写入全部房间。引擎在此之前验证 Mod 类型、扩展字段和玩家/市场候选；房间提交成功后才移交账户、市场、Mod state 与事件。

分配器以 `Result` 报告耗尽。`u64::MAX` 是耗尽游标，不能分配给对象。共享候选留存首个致命分配错误；链内回退游标、捕获 JS 异常或返回 decline 均不清除此错误。房间结束与候选发布都检查它。整轮失败沿用缓存输入重试，玩家 Memory/CPU 和其他候选均不发布。

世界实例身份是进程内 `Arc<()>`，独立 clone/重建/reset 使用新身份，受控副本编辑发布保留原身份。它与 terrain_epoch 一起标识寻路缓存，不进入检查点。检查点字段、格式版本和 PlanHash 没有因此改变；恢复继续拒绝违反状态不变式的载荷。

## 房间候选与局部恢复

`Working::new(room, time, now_ms, state, plane)` 是房间候选的构造入口，统一建立空间索引、空延迟队列和移动候选。外部调用方不再复制瞬态字段清单；疲劳、伤害/治疗队列及链内变更记录仅对 engine 内部开放。

engine 的 `ObjectJournal` 保存资源和战斗/建造改向的首触对象及必要队列。runtime 只请求记录和恢复；外层 `ChainSnapshot` 继续负责索引、对象增删和游标，恢复顺序保持不变。正常 decline、宿主故障和整轮失败仍使用各自原有提交边界。

## Rust 调用方迁移

| 旧调用 | 当前调用 |
|---|---|
| `engine.world` / `engine.players` 等公开字段 | 对应只读 getter |
| `engine.world.insert(...)` | `engine.insert_object(...)`，处理 Result |
| `world.room_mut(name)` | `world.edit_room(name, closure)`，只编辑 RoomContents |
| `world.get_mut(id)` | `world.edit_object(id, closure)` |
| 逐房 `commit_room` + `advance_ticks` | `commit_tick_rooms(rooms, next_id)` |
| 直接修改账户/订单/namespace state | `edit_players` / `edit_market` / `edit_mod_states` |
| `World::advance_ticks` / `remove` / `bump_generation` | 处理 Result，时钟或版本耗尽时原状态保留 |
| `Working { ... }` 及外部访问瞬态队列 | `Working::new(...)`，经领域方法操作；恢复使用 `ObjectJournal` |
| `Working::insert_new` / `TickState::allocate_id` 的裸 ID | 处理 Result，创建失败立即结束本动作 |
| `load_terrain` + `search(origin, ...)` | 构造不可变 `TerrainSet`，`search(&terrain, origin, ...)` |

`register_room`、测试时间设置及编辑入口均可能失败，不能继续忽略返回值。管理员编辑用于初始化和有界修改，会复制候选；生产结算保持批量发布。性能证据和逐项验收见[质量优化记录](../research/notes/deep-quality-2026-10-06.md)。
