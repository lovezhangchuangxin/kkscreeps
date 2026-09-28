# PathFinder.roomCallback 玩家侧通道设计（M2 轨 C，C2 批）

状态：**C2 已实装宿主侧（kk-pathfinder）**；玩家侧接线（prelude → op 通道）为轨 A
A4/A5 的输入，本批不实装。所有官方行为主张均给出 `~/Desktop/projects/sources/screeps/`
的 file:line 取证；官方没有的语义不臆断（我方自有面的偏差显式登记于 §6）。

---

## 1. 玩家视角的行为定义（先对错，再做法）

`PathFinder.search(origin, goal, {roomCallback, ...opts})` 的官方语义，逐条取证：

| # | 行为 | 官方出处 |
|---|---|---|
| 1 | goal 为空/空数组 → 早退 `{path:[], ops:0}`（无 cost/incomplete 键） | engine/src/game/path-finder.js:60-62 |
| 2 | 九选项钳制（`x\|\|default` 回退 + min/max 夹逼） | driver/lib/path-finder.js:90-96 |
| 3 | roomCallback 非 function → 忽略（同无回调） | driver/lib/path-finder.js:116-119 |
| 4 | 回调包装：`ret = cb(房名)`；`ret===false` → false；truthy → `ret._bits`；其余 → undefined | driver/lib/path-finder.js:120-129 |
| 5 | 回调触发时序：房间**首次触达**时调用、每房至多一次、首房 = origin 所在房；参数为房名字符串（native 给地图坐标，JS bridge 换算房名） | pf.cc:22-64（触发）、driver/lib/path-finder.js:121-122（换算） |
| 6 | 返回 false = 封锁该房间 | pf.cc:49-52 |
| 7 | 返回 undefined / 非 TypedArray / 非 2500 长度 TypedArray = 该房间纯地形（**静默**） | pf.cc:47-58（`length()==2500` 检查在 :55） |
| 8 | 回调抛错 → 整个 search 返回 `{path:[], ops:0, cost:0, incomplete:false}`（异常被吞，不外抛） | pf.cc:43-46 → pf.cc:563-567 → driver/lib/path-finder.js:135-136 |
| 9 | 回调内可嵌套 `PathFinder.search`（两层；更多层堆分配兜底，官方注释每实例 2.16MB） | driver/native/src/main.cc:9-14 |
| 10 | CostMatrix：set/get 越界与 deserialize 长度错误全部**静默**（TypedArray 语义，无逐轴 0..50 检查——唯一边界是平铺索引 `xx*50+yy` 与长度） | engine/src/game/path-finder.js:22-32,44-48 |
| 11 | serialize 产物 = 普通 Array（625 个数字，平台 LE）——玩家 JSON 化进 Memory 的持久化形态 | engine/src/game/path-finder.js:40-42 |
| 12 | 长时间回调无独立超时：靠驱动层 CPU 限额（isolate 终止经 `IsExecutionTerminating` → search 返回 Undefined → 空 result） | pf.cc:558-561 |

## 2. 官方调用链（谁在哪一层）

```
玩家 JS: PathFinder.search(origin, goal, opts)
  └ engine/src/game/path-finder.js:59-64        （空 goal 早退；其余直通 driver）
  └ driver/lib/path-finder.js:86-142  search    （选项钳制、goal 归一、回调包装、结果映射）
  └ main.cc:29+  NAN_METHOD(search)             （实参解包 → pf->search）
  └ pf.cc:455-602  path_finder_t::search        （A*/JPS 本体；回调在 :37-59 触发）
```

回调数据形态的层间契约：JS bridge 交给 native 的是 `ret._bits`（**Uint8Array 引用**，
非 serialize 产物），native 只认 2500 长度（pf.cc:54-57）且**直接引用不拷贝**——
矩阵存储存活到 search 返回（v8 端靠 `room_data_handles` 防 GC，pf.cc:484-487,53）。

## 3. 我方分层与 C2 已实装部分（kk-pathfinder 内）

分层不变（见 `crates/kk-pathfinder/src/lib.rs` 模块文档）：
`native/pf.cc`（官方原样）→ `native/compat/nan.h`（值/回调原语）→ `native/kk_ffi.cc`
（C ABI 搬运）→ `src/ffi.rs`（unsafe 集中地）→ `src/lib.rs`（安全 API）。

C2 批实装（均配锁值测试，见 §7）：

1. **回调契约扩展**（`kk_ffi.h kk_pf_room_callback_t`）：返回码 0/1/2/3；`2` 经
   `out_matrix`+`out_len` 交出矩阵**真实长度**——错误长度由未改动的官方 pf.cc:54-57
   静默裁决（与官方 JS bridge 无条件透传 `_bits` 一致），我们的代码不做长度判断。
2. **回调 panic catch_unwind**（`ffi.rs` trampoline）：panic → 返回码 3 → compat 层
   置 v8 挂起异常 → **官方 pf.cc 自己的**异常链处理（pf.cc:43-46 → :563-567）→ 空
   result。unwind 永不穿越 extern "C"（Rust ≥1.81 那是 abort）。
3. **CostMatrix 官方静默语义**（`lib.rs`）：变长 `Vec<u8>` 底座（长度恒为 4 的倍数）；
   set 平铺越界 = 无操作、get 越界 = `None`（↔ undefined）、deserialize 任意长度恒
   成功、索引 checked 运算（官方 float64 大索引同判越界，不 panic）。
4. **load_terrain 重入拒绝**：搜索栈内（即回调内）调用返回
   `Error::ReentrantTerrainLoad`（官方无此面，防死锁，§6.1）。
5. **重入读地形锁**（`TerrainLock`）：同线程嵌套搜索不阻塞（官方支持两层嵌套，
   main.cc:9-14），写者与读者互斥。std RwLock 的递归读在有等待写者时可能死锁，
   故自制（死锁不可能性论证见 lib.rs）。

## 4. 玩家侧通道设计（轨 A 接线；本批不实装）

### 4.1 op 面命名与数据编码

对照 kk-runner 既有命名纪律（`crates/kk-runner/src/ops.rs:4-6`：一律 `op_kk_` 前缀）：

| 建议 op | 形态 | 说明 |
|---|---|---|
| `op_kk_pathfinder_search` | `#[op2(fast)]`/`#[op2]`，入参 origin/goals/九选项/回调句柄 id，返回 path/ops/cost/incomplete | 一次完整搜索。**选项钳制必须发生在宿主 Rust 侧**（`SearchOptions::clamped` 已备）——玩家传的 `plainCost: 0` 等经官方 `\|\|` 语义回退（driver/lib/path-finder.js:90-96），JS 侧不要预钳 |
| `op_kk_pathfinder_matrix_get/set`（可选） | CostMatrix 的构造/读写面 | 若 prelude 不在 JS 侧本地实现 CostMatrix 而走宿主对象表；见 4.3 决策点 |

**CostMatrix 的两条通道**（勿混）：

- **持久化通道**（玩家可见，进 Memory）：官方 serialize 产物是**普通 Array**
  （`Array.prototype.slice.apply(new Uint32Array(...))`，engine path-finder.js:40-42），
  玩家 `JSON.stringify` 后存储；deserialize 接受任意 array-like（:44-48，长度不校验）。
  op 面等价物建议：`#[buffer] Uint8Array`（2500 字节原始 `_bits` 形态，等价信息量、
  免 JSON 数字精度面），prelude 在 JS 侧做 `Array ↔ Uint8Array` 的官方形态转换。
- **native 通道**（宿主内部，热路径）：官方直接传 `_bits` 引用（driver/lib/path-finder.js:125-127）。
  我方等价物 `RoomDecision::CostMatrix(&CostMatrix)`——**同一引用语义，不拷贝**；
  A4 接线时宿主把玩家回调产出的矩阵存进 tick 级 arena，引用存活到 search 返回
  （官方靠 v8 `room_data_handles` 防 GC，pf.cc:484-487）。

### 4.2 回调机制：推荐案与备选

**案 A（推荐）：宿主持 `v8::Global<Function>` 同步重入。**
prelude 的 `PathFinder.search` 把 roomCallback（经 `typeof` 校验，非 function 忽略
——driver/lib/path-finder.js:116-119）注册进 OpState（Global 句柄 + 回调 id），调
`op_kk_pathfinder_search`；宿主 op 内把「调玩家函数」包装成 Rust 闭包传
`search_with_room_callback`：回调触发 → 宿主用 HandleScope 重入 v8 同步调用玩家
函数 → 返回值按官方包装器语义（§1 #4）映射 `RoomDecision` → 玩家 JS 异常转回调
Err → trampoline 的 rc=3 → 官方异常链（§1 #8）。
- 可行性前提（**A4 接线时验证**）：deno_core 0.412 的 op2 是否支持在 op 签名中注入
  `&v8::HandleScope`（或经 `OpState` 拿 isolate）。kk-runner 已有宿主侧 scope 用法
  （sandbox.rs:526-528 `deno_core::scope!`），重入 v8 本身不是禁区（v8 支持嵌套执行）。
- 语义保真：回调与搜索同栈同步执行，玩家在回调里读世界/抛错/嵌套 search 的时序
  与官方一致；回调跑在 isolate 内 → 天然受 kk-runner watchdog/CPU 限额管
  （对照官方 §1 #12 的 IsExecutionTerminating 路径——pf.cc:558-561 在 isolate 终止
  时返回 Undefined → 空 result；我方 compat 层该函数恒 false，因为官方 JS 引擎
  终止面在沙箱层，不在本 crate）。

**案 B（备选）：两段拉取协议。** search op 分帧：宿主跑到「需要房间决策」时经 op
返回值把待决房名交回 JS，prelude 收集决策后续跑。代价：一次同步调用拆成 N 次 op
往返 + prelude 维护半成品状态；**回调与搜索不再同栈**，玩家在回调里抛错/观测的
时序面全部变形。仅当案 A 的 scope 注入不可行时兜底，且需在本文档登记差异。

**案 C（否决）：预取矩阵。** JS 先为可能触达的房间预生成矩阵再随 op 传入。官方
回调是懒触发（每房至多一次、触达才调，pf.cc:22-64）——预取会执行**本不该执行的
玩家回调代码**（副作用可见：日志/缓存/Memory 写），玩家可见差异，否决。

### 4.3 处置矩阵（回调异常/超时/重入/非法返回）

| 情形 | 官方行为（file:line） | 我方处置 | 状态 |
|---|---|---|---|
| 回调返回 false | pf.cc:49-52 封房 | `RoomDecision::Blocked` | C2 已实装+锁值 |
| 返回 undefined/非 TypedArray | pf.cc:47,55 → 无矩阵 | `RoomDecision::None` | C2 已实装+锁值 |
| 返回非 2500 长度矩阵 | pf.cc:54-57 静默忽略 | 长度透传，**官方代码**裁决 | C2 已实装+锁值 |
| 回调 throw（JS） | §1 #8 链 → 空 result incomplete:false | 案 A：v8 异常 → rc=3 → 同一段官方 pf.cc 异常链 | 链路 C2 已实装（panic 面）；JS 面 A4 接 |
| 回调 panic（宿主 Rust 面） | 官方无 Rust 面 | catch_unwind → rc=3 → 同上；绝不 abort | C2 已实装+锁值 |
| 回调内嵌套 search | 官方支持两层（main.cc:9-14，第三层起堆分配 2.16MB/实例） | `TerrainLock` 重入读 + kk_ffi.cc 双实例池（复刻 main.cc）；第三层起同样堆分配 | C2 已实装+锁值（两层） |
| 回调内 load_terrain（宿主 API） | 官方无此面（地形仅启动期载入，driver/lib/path-finder.js:23-46） | `Err(ReentrantTerrainLoad)` | C2 已实装+锁值；差异 §6.1 |
| 回调超时/CPU 限额 | 官方无独立超时；isolate 终止 → pf.cc:558-561 → Undefined → 空 result | 案 A 下回调跑在 isolate 内，由 kk-runner watchdog 终止；本 crate 无超时面（不臆断） | 设计定案，A4/A5 接线 |
| 回调返回非法类型（数字/字符串等 truthy） | 包装器交出 `ret._bits`=undefined → native 视 undefined → 无矩阵（driver/lib/path-finder.js:125-127） | 案 A 宿主闭包按官方包装器逐字复刻（`ret._bits` 语义：非 CostMatrix 对象取 undefined） | A4 接线时锁值 |

### 4.4 prelude 包装器（轨 A 实装清单）

```js
// 复刻 driver/lib/path-finder.js:120-129（逐行对照）：
cb = function(cb) {
    return function(xx, yy) {          // xx/yy = 地图坐标，宿主闭包传入
        let ret = cb(generateRoomName(xx, yy));  // 房名换算在 JS 侧（宿主给地图坐标）
        if (ret === false) return ret;           // → RoomDecision::Blocked
        else if (ret) return ret._bits;          // → 矩阵引用（非 2500 长度由 pf.cc:55 裁决）
    };                                           // 其余 → undefined → None
}(cb);
```

- `generateRoomName` 可用宿主 `generate_room_name` 的 op 暴露或 JS 侧复刻
  （driver/lib/path-finder.js:51-56，公式简单，建议 JS 侧复刻减少 op 往返）；
- CostMatrix 类（constructor/set/get/clone/serialize/deserialize）语义已由
  kk-pathfinder 锁值；JS 层只需做数值归一（`xx|0`、`Math.min(Math.max(0,val),255)`、
  NaN→0——engine path-finder.js:22-32）后经 op 落到宿主矩阵，或整类 JS 本地实现 +
  `#[buffer]` 通道（4.1 决策点：推荐后者，2500 字节 Uint8Array 本地操作零 op 开销，
  serialize 时才过边界）。

## 5. 微基准（2026-09-28，本机 Apple Silicon，release 档）

`tests/bench.rs::bench_callback_overhead`（W5N3→W8N5 跨房远征，maxOps 100000，
2000 次/轮，8 次回调/搜索）：

| 轮次 | 无回调 | 带回调（None 决策） | 差 |
|---|---|---|---|
| 1 | 279.31 µs/次 | 279.99 µs/次 | +0.68 |
| 2 | 278.88 µs/次 | 276.90 µs/次 | -1.99 |
| 3 | 278.33 µs/次 | 281.31 µs/次 | +2.98 |

结论：FFI 回调跳板（Rust 闭包 → extern "C" → compat 值包装 → 官方 C++ → 回程）
的开销在噪声以下（<0.4 µs/次回调），相对搜索本体（~280 µs）可忽略——**roomCallback
通道不需要性能优化**。玩家侧真正的开销在案 A 的 v8 重入（每次回调一次 JS 调用，
官方同款），A4 接线后另测。

参考：短程单房 search 1.83 µs/次（release，`bench_short_search`）。

## 6. 已登记差异（官方没有、我方自定的面）

### 6.1 `load_terrain` 重入拒绝（`Error::ReentrantTerrainLoad`）

官方地形只在进程启动期载入（driver/lib/path-finder.js:23-46），搜索期**不存在**
载入口——故无官方语义可对照（不是「官方允许/禁止」，是「面不存在」）。我方
允许多次载入（测试/多世界需要），同线程「持读份额再取写份额」必死锁，故回调
栈内调用返回 Err。玩家面不可达（load_terrain 是宿主 API，不在玩家 JS 面上）。

### 6.2 嵌套搜索的内层回调异常会波及外层

官方 v8 的 TryCatch 有词法作用域：内层 search 捕获自己的异常后外层继续。我方
compat 层的挂起异常标志是**线程级**（`t_pending_js_exception`，每次 FFI 入口由
ArenaGuard 复位）：嵌套场景下内层回调 panic 后标志残留为真，外层 pf.cc:43 误判
异常 → 外层也中止为空结果。方向安全（永不 abort/UB，只可能「多给一个空结果」），
且仅「嵌套 + 内层回调异常」双条件可达；单层回调（当前唯一玩家面形态）不受影响。
A4 若暴露嵌套面再评估是否按词法作用域改造 TryCatch。

### 6.3 CostMatrix 非法值的 JS 数值层归一（预登记，非本 crate 面）

官方 `set(-5)`→0、`set(NaN)`→0、`set(3.7)`→3 的归一发生在 JS 数值层（engine
path-finder.js:25 的 `Math.min(Math.max(0, val), 255)` + TypedArray 赋值转换）；
kk-pathfinder 的 `u32` API 只做上界钳制。负值/NaN/小数面由 prelude 的 CostMatrix
类负责（4.4），A4 接线时锁值。

## 7. C2 锁值测试索引

| 测试 | 锁定行为 |
|---|---|
| `tests/callback_safety.rs::cost_matrix_wrong_length_ignored_at_search` | 非 2500 字节矩阵经官方 pf.cc:54-57 静默忽略（= 基线路径）；2500 字节对照组生效 |
| `::room_callback_panic_first_room_maps_to_official_empty` | 回调 panic → 官方异常链 → `{path:[],ops:0,cost:0,incomplete:false}`；进程不 abort |
| `::room_callback_panic_second_room_maps_to_official_empty` | 异常时刻无关性（第二房 panic 同结果） |
| `::search_works_after_callback_panic` | panic 后池实例复位（pf.cc:565），后续搜索正常 |
| `::load_terrain_in_callback_rejected_not_deadlock` | 回调内 load_terrain → `Err(ReentrantTerrainLoad)`；线程+recv_timeout 防挂起；外层搜索正常完成 |
| `::nested_search_in_callback_works` | 嵌套两层 search 正常（官方 main.cc:9-14 语义） |
| `src/lib.rs::cost_matrix_out_of_range_silent` | 平铺索引越界静默；(0,50)/(1,0) 同字节别名；溢出索引不 panic |
| `src/lib.rs::cost_matrix_deserialize_any_length_silent` | deserialize 任意长度恒成功，界内可读/界外 None，serialize 往返 |
| `tests/official_golden.rs::official_profile_checksum` | 金标 11843305（官方 profile.js 全对场景）——C2 全部改动后保持 |
