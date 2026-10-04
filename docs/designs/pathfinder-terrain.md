# 寻路地形直通（D2）：PathFinder.search 接入原生搜索

[player API 到原生寻路的回调通道](pathfinder-callback.md)（D3）之前的切片：
把 `PathFinder.search` 及其下游（`Room.findPath`、`RoomPosition.findPathTo`/
`findClosestByPath`、`Creep.moveTo`）从直线桩换成 kk-pathfinder 的真实地形搜索。
本文档定义接线架构、玩家可见形状与已知分歧；回调时序/异常/静默契约归
pathfinder-callback.md。

## 官方语义对照

上游版本与 kk-pathfinder `src/lib.rs` 头注释相同（engine a90d26f / driver
cf63d8a）。关键锚点：

- 空目标早退在 engine 层：`engine/src/game/path-finder.js:60-62`——`!goal ||
  (Array.isArray(goal) && !goal.length)` 返回 `{path: [], ops: 0}`（两键，无
  cost/incomplete）。
- 选项清洗、目标归一、坐标转换在 driver 层：`driver/lib/path-finder.js:86-141`。
  `toWorldPosition` 的 `x|0` 强转与 0..49 检查（:59-70）、parseRoomName 正则
  与 0..255 检查（:10-21）、`Math.max(0, goal.range|0)`（:106）。这些含 JS
  数值语义的转换必须留在 JS 侧逐字复刻，错误文案 `Invalid room position` /
  `Invalid room name` 原样抛出。
- 原生结果到玩家形状在 driver 层：`ret === undefined → {path:[],ops:0,cost:0,
  incomplete:false}`、`ret === -1 → {…incomplete:true}`、`ret.path.map(
  fromWorldPosition).reverse()`（:135-141）。`fromWorldPosition` 产出
  **RoomPosition 实例**；反转由原生 C++ 从终点回溯的路径序决定——kk-pathfinder
  的 `finish()` 已按行走序返回，玩家面不得二次反转。
- `Room.findPath` 默认走新寻路：`engine/src/game/game.js:68` 的
  `_useNewPathFinder: true` 初值使 `rooms.js:869` 进入 `_findPath2`（rooms.js:223-302）：
  range 默认 1、跨房 maxOps 20000 缺省、plainCost 2/swampCost 10（除非
  ignoreRoads）、终点贴近回填、房界截断。
- `RoomPosition.findPathTo` 委托 `room.findPath`（rooms.js:1396-1428），房间
  不可见时抛 `Could not access room <name>`；`findClosestByPath` 走
  `_findClosestByPath2`（rooms.js:304-374）：range 1 多目标搜索 + maxRooms 1 +
  终点 isNearTo 反查。
- `Creep.moveTo` 全流程（`engine/src/game/creeps.js` moveTo）：`_move` 记忆缓存
  （reusePath 5/serializeMemory true 缺省、过期/换房失效、按当前位置截断消费）、
  noPathFinding 出口、findPathTo 路径、空路径 ERR_NO_PATH、visualizePathStyle
  绘制、`move(path[0].direction)`。

## 接线架构

```
玩家沙箱 path-finder.js ──op_kk_pathfinder_search(serde 参数)──▶ kk-runner
    ▲                                                          pathfinder.rs
    └────── RoomPosition 数组（含房间名还原）◀── SearchResult ──┤ 参数清洗后的
                                                               │ 坐标/选项
              begin_tick_plan 每 tick 注入 TickTerrain 到 OpState
              JSON 通道: world.rooms[].terrain{rev,payload} → b64 解码 → pack
              KKPV 通道: 持 SharedKkpvBundle 句柄，首次搜索懒解码公开地形块
```

- **op 通道**：deno_core `#[op2]` + `#[serde]` 参数/返回值。JS 侧先按 driver
  逐字清洗（含 `|0`、`||` 缺省、min/max 钳制），op 只接清洗后的纯数值；
  返回 `{path: [[xx,yy]…], ops, cost, incomplete}`（世界坐标、行走序），
  JS 映射回 `gw.RoomPosition`。错误（原生 JSError 等）以 op 错误抛回 JS。
- **地形来源**：寻路地形是世界公共数据，两通道输入相同——
  - JSON 通道：`world.rooms[room].terrain.payload`（行序 2500 字节 b64，含
    全部注册房，不可见房也只发名称与地形，`kk-world/src/world_json.rs` 的
    `terrain_json`）。`rev` 即 terrain_epoch：Sandbox 以 rev 缓存打包产物，
    稳态零解码。
  - KKPV 通道：公开地形束承载全部注册房（`kk-world/src/kkpv.rs`
    `kkpv_public_terrain_blocks`）。`SharedKkpvBundle` 暴露只读字节视图，
    kk-runner 内解析块框架（u32 块数 + 长度目录 + 160 字节块头）取每块
    房名与 2500 字节地形段；首次搜索懒解码并缓存于本 tick 的 TickTerrain。
- **打包方向**：kk-core 地形为行序 `tiles[y][x]`（位掩码 wall=1/swamp=2/
  lava=4），kk-pathfinder `pack_terrain` 要求列序 `i = x*50 + y` 的 625 字节；
  转换在 kk-runner `pathfinder.rs` 一处完成。位值取低两位（kk-pathfinder
  `pack_terrain` 契约）：wall|swamp(3) 按障碍，lava(4) 低两位为 0 按平原。
- **进程级地形表**：kk-pathfinder 的 C++ 地形表进程全局（`load_terrain`
  覆盖同名房）。kk-runner 维护 `LOADED: room → 625 字节` 做内容比对：
  每次 tick 地形打包后，与 LOADED 逐房**内容比对**（非存在性），差异才
  `load_terrain`——天然覆盖 set_terrain 与跨分片重建（createShard）的陈旧
  表项失效。
- **有限世界墙环**：官方 driver `init` 载入全部分片房地形，搜索触达任何
  合法房都有地形；本仓世界只含注册房。原生 `room_index_from_pos` 对无地形
  房直接抛 `Could not load terrain data`（`native/pf.cc`），且该检查先于
  roomCallback。因此搜索前把每个注册房 8 邻域中**不在世界内**的房间按
  全墙地形载入（等价官方在世界边缘被墙环绕时的行为）：边界 frontier 触达
  只得到 obstacle，路径以 incomplete 收尾而不是抛错。墙环房间同样进
  LOADED 内容比对（期望值恒为全墙），跨分片残留的真实地形会被墙覆盖回。

## 玩家可见形状与已知分歧

| 项 | 行为 | 状态 |
|---|---|---|
| `PathFinder.search` 返回 | `{path, ops, cost, incomplete}`；path 元素为 RoomPosition 实例、不含起点、行走序 | 与官方一致 |
| 空/无目标 goal | `{path: [], ops: 0}` 两键 | 与官方一致 |
| `options.roomCallback` | **忽略**（含返回 false/CostMatrix 的效果） | 分歧，D3 接线 |
| `Room.findPath` 结构代价 | 地形直通：不叠结构/creep/道路代价矩阵（官方 `_findPath2` 经 `getPathfindingGrid2` 生成 CostMatrix） | 分歧，D3 接线 |
| 未注册房（墙环外）起点 | 原生抛 `Could not load terrain data` | 分歧：官方全域有地形；目标房在墙环外只得到 incomplete，不抛错 |
| 同进程多引擎并发搜索 | C++ 地形表进程共享，跨引擎并发搜索会串地形 | 边界登记：embed CLI/ratchet/bot 均单引擎单进程；多引擎仅出现在串行单元测试 |
| lava(4) 格 | 低两位→平原可走 | 上游打包契约；生成世界不产 lava |
| `PathFinder.use` | 保持桩（计数）；模式切换无观察面 | 与 xxscreeps 一致 |
| 垃圾数值选项（如 `heuristicWeight: 'abc'` → NaN） | JS 清洗同官方产 NaN；Rust `SearchOptions::clamped` 将 NaN 归 1.2 后搜索。官方把 NaN 传入 C++，double→整数转换未定义（x86 实效近似权重 0 的泛洪） | 分歧（垃圾输入域）：官方自身为未定义行为，本仓取确定性 1.2 |
| `opts.avoid`/`opts.ignore` 弃用警告 | 静默忽略（官方 `register.deprecated` 打 console 警告后同样置 undefined，rooms.js:231-238/:331-336） | 分歧：console 输出面缺失，功能面等价；补齐随回调通道批 |
| KKPV 公开束 Rust 解析校验面 | 与沙箱解码器（kkpv.js decodePublicBundle）一致拒绝私有块/非 2500 地形段/重复房名/带对象块；块头 tick 与信封 time 的交叉校验只在沙箱解码器（Rust 侧无信封） | 边界：宿主可信数据 + JS 校验先行抛错 |

## 验收

- screeps-ok：PATHFINDER-007/008/009/010/011/012/013/014/015/020/022 与
  LEGACY-PATH-005 转绿；PATHFINDER-004/005/023（回调族）维持红归 D3；
  棘轮地板只升不降。
- bot 三基线（tooangel 2000t 0 错、overmind-dist 2000t 1 错既有、wasm 10t）：
  PathFinder.search/findPathTo/findClosestByPath 退出桩计数后基线形态变化
  须逐项归因登记（路径真化改变 bot 行为本身属预期）。
- Rust 单测：b64 解码、行序→列序打包（含 kk-core 位掩码）、KKPV 公开束
  解析对照 kk-world 参考编码、墙环计算、LOADED 内容比对与变更重载。
- sandbox 集成测：JSON 与 KKPV 两通道的搜索结果等同；带墙避让路径。
