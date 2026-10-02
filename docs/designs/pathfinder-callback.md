# 玩家寻路回调接入设计

本文定义玩家 `PathFinder.search` 到原生寻路器的接入契约，重点说明回调时序、矩阵存活、异常传播和预算中断。

当前 [kk-pathfinder](../../crates/kk-pathfinder/src/lib.rs) 已提供原生搜索与 Rust 房间回调。[玩家 prelude](../../crates/kk-runner/src/prelude/30_game/path-finder.js) 已实现 `CostMatrix`，但 `search` 仍是直线步进桩：只读取首个目标，忽略房间、障碍和搜索选项，不调用 `roomCallback`，并返回 `incomplete: false`。因此原生测试通过不代表玩家寻路已经兼容。

上游依据使用 `screeps/engine`、`screeps/driver` 的文件与符号定位；下文行号是研究时的辅助位置，核实时须确认采用的源码版本。仓库内的原生来源与接口说明见 `kk-pathfinder`，实验结果不能替代玩家通路的集成验收。

## 1. 目标玩家行为

`PathFinder.search(origin, goal, {roomCallback, ...opts})` 的官方语义，逐条取证：

| # | 行为 | 官方出处 |
|---|---|---|
| 1 | goal 为空/空数组 → 早退 `{path:[], ops:0}`（无 cost/incomplete 键） | engine/src/game/path-finder.js:60-62 |
| 2 | 搜索选项归一与范围限制（`x\|\|default` 回退 + min/max 夹逼） | driver/lib/path-finder.js:90-96 |
| 3 | roomCallback 非 function → 忽略（同无回调） | driver/lib/path-finder.js:116-119 |
| 4 | 回调包装：`ret = cb(房名)`；`ret===false` → false；truthy → `ret._bits`；其余 → undefined | driver/lib/path-finder.js:120-129 |
| 5 | 回调触发时序：房间**首次触达**时调用、每房至多一次、首房 = origin 所在房；参数为房名字符串（native 给地图坐标，JS bridge 换算房名） | pf.cc:22-64（触发）、driver/lib/path-finder.js:121-122（换算） |
| 6 | 返回 false = 封锁该房间 | pf.cc:49-52 |
| 7 | 返回 undefined / 非 TypedArray / 非 2500 长度 TypedArray = 该房间纯地形（**静默**） | pf.cc:47-58（`length()==2500` 检查在 :55） |
| 8 | 回调抛错 → **异常上抛给玩家**（JS 层 `PathFinder.search()` throw；玩家未捕获时中断本轮脚本） | pf.cc:43-46 ReThrow（TryCatch::ReThrow 重挂）→ pf.cc:563-567 占位 Undefined → V8 api 边界挂起异常胜过返回值（builtins-api.cc）→ driver/engine 两层 JS 无 try/catch 直达玩家；driver:135-136 的 undefined 分支须与异常状态区分 |
| 9 | 回调内可嵌套 `PathFinder.search`（两层；更多层堆分配兜底，官方注释每实例 2.16MB） | driver/native/src/main.cc:9-14 |
| 10 | CostMatrix：set/get 越界与 deserialize 长度错误全部**静默**（TypedArray 语义，无逐轴 0..50 检查——唯一边界是平铺索引 `xx*50+yy` 与长度） | engine/src/game/path-finder.js:22-32,44-48 |
| 11 | serialize 产物 = 普通 Array（625 个数字，平台 LE）——玩家 JSON 化进 Memory 的持久化形态 | engine/src/game/path-finder.js:40-42 |
| 12 | 长时间回调无独立超时：靠驱动层 CPU 限额（isolate 终止经 `IsExecutionTerminating` → search 返回 Undefined——但玩家观察不到返回值：终止异常同样胜出，脚本被终止而非收到空 result） | pf.cc:558-561 |

## 2. 上游调用链与数据形态

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

## 3. 当前原生接口

分层不变（见 `crates/kk-pathfinder/src/lib.rs` 模块文档）：
`native/pf.cc`（官方原样）→ `native/compat/nan.h`（值/回调原语）→ `native/kk_ffi.cc`
（C ABI 搬运）→ `src/ffi.rs`（unsafe 集中地）→ `src/lib.rs`（安全 API）。

原生接口已经提供以下行为，相关测试见第 7 节：

1. **回调契约扩展**（`kk_ffi.h kk_pf_room_callback_t`）：返回码 0/1/2/3；`2` 经
   `out_matrix`+`out_len` 交出矩阵**真实长度**——错误长度由未改动的官方 pf.cc:54-57
   静默裁决（与官方 JS bridge 无条件透传 `_bits` 一致），我们的代码不做长度判断。
2. **回调 panic catch_unwind**（`ffi.rs` trampoline）：可展开的普通回调 panic →
   返回码 3 → compat 层置本次调用的异常标志 → pf.cc 异常处理 → C ABI status 3 →
   Rust `Err(JSError(None))`。此捕获依赖 panic=unwind，且载荷析构不能再次 panic。
3. **CostMatrix 官方静默语义**（`lib.rs`）：变长 `Vec<u8>` 底座（长度恒为 4 的倍数）；
   set 平铺越界 = 无操作、get 越界 = `None`（↔ undefined）、deserialize 任意长度恒
   成功、索引 checked 运算（官方 float64 大索引同判越界，不 panic）。
4. **load_terrain 重入拒绝**：搜索栈内（即回调内）调用返回
   `Error::ReentrantTerrainLoad`（官方无此面，防死锁，§6.1）。
5. **重入读地形锁**（`TerrainLock`）：同线程嵌套搜索不阻塞（官方支持两层嵌套，
   main.cc:9-14），写者与读者互斥。std RwLock 的递归读在有等待写者时可能死锁，
   故自制（锁的递归读与写入约束见 lib.rs）。

## 4. 玩家通道设计

### 4.1 op 面命名与数据编码

对照 kk-runner 既有命名纪律（`crates/kk-runner/src/ops.rs:4-6`：一律 `op_kk_` 前缀）：

| 建议 op | 形态 | 说明 |
|---|---|---|
| `op_kk_pathfinder_search` | `#[op2]`，具体签名按 V8 回调需要确定，入参 origin/goals/搜索选项/回调句柄 id，返回 path/ops/cost/incomplete | 一次完整搜索。JS 入口先按上游完成有符号 int32 等数值归一，Rust 侧再应用缺省值与范围限制；`SearchOptions::clamped` 已有后一步。不能把负值直接转成 u32 后钳制 |
| `op_kk_pathfinder_matrix_get/set`（可选） | CostMatrix 的构造/读写面 | 现有 CostMatrix 在 JS 本地实现；只有选择宿主对象表时才需要这些 op |

**CostMatrix 的两种数据用途**：

- **持久化通道**（玩家可见，进 Memory）：官方 serialize 产物是**普通 Array**
  （`Array.prototype.slice.apply(new Uint32Array(...))`，engine path-finder.js:40-42），
  玩家 `JSON.stringify` 后存储；deserialize 接受任意 array-like（:44-48，长度不校验）。
  这一通道留在 JS 中，不应把序列化产物改成 2500 元素数组。
- **native 通道**（宿主内部）：上游直接传 `_bits`（driver/lib/path-finder.js:125-127）。
  当前 Rust `RoomDecision::CostMatrix(&CostMatrix)` 借用 Rust 矩阵，C++ 不复制字节；这不等于已经具备借用 V8 TypedArray 的生产接口。玩家接入须另外设计可写 JS 矩阵的传递方式；当前不可变 Rust 借用不能直接套用到可能在后续回调中改变的 V8 缓冲区。

上游持有矩阵引用到搜索结束。后续房间回调或嵌套搜索可能修改先前返回的矩阵，原生算法之后读取时会看到修改；只在首次返回时复制一次会改变这一行为。如果使用复制方案，须保持这些后续修改的可见性，并处理多个房间共享同一缓冲区的情况。零拷贝方案则须证明存活期与访问安全，不能在仍持有 Rust 不可变借用时进入可能写同一内存的 JS。

### 4.2 回调机制：推荐案与备选

**案 A（推荐）：宿主持 `v8::Global<Function>` 同步重入。**
prelude 的 `PathFinder.search` 把 roomCallback（经 `typeof` 校验，非 function 忽略
——driver/lib/path-finder.js:116-119）注册进 OpState（Global 句柄 + 回调 id），调
`op_kk_pathfinder_search`；目标宿主 op 将调用玩家函数的操作接入原生房间回调；现有
`search_with_room_callback` 需要补齐矩阵存活与显式错误通道：回调触发 → 宿主用 HandleScope 重入 v8 同步调用玩家
函数 → 返回值按官方包装器语义（§1 #4）映射房间决策。当前 Rust 回调只返回 `RoomDecision`，没有显式错误变体；玩家异常通道须扩展接口，在搜索结束后重新抛出保存的异常对象，不能通过 Rust panic 传递 JS 异常。
- [op_callback 实验](../../spikes/deno-spike/examples/op_callback.rs)已使用 `&mut v8::PinScope` 和 `#[scoped] v8::Global<v8::Function>` 验证 op 内同步调用 JS。实验只模拟房间遍历，没有连接原生搜索；它把异常转成新错误消息，也没有证明原异常对象、栈和矩阵视图偏移能正确保留。生产接入仍须验证这些边界。
- 语义保真：回调与搜索同栈同步执行，玩家在回调里读世界/抛错/嵌套 search 的时序
  应保持上游可观察的同步时序；回调跑在 isolate 内 → 可由 kk-runner watchdog 请求终止
  （对照官方 §1 #12 的 IsExecutionTerminating 路径——pf.cc:558-561 在 isolate 终止
  时返回 Undefined；当前 compat 层该函数恒 false，原生搜索并未接收沙箱终止标记）。

V8 终止请求主要在 JS 执行检查点生效。当前原生 compat 层不检查真实 isolate 的终止状态，不能假设看门狗会及时打断没有回调的 C++ 搜索。接入必须验证纯搜索与长时间回调分别如何取消；`maxOps` 限制搜索展开次数，不限制单次玩家回调耗时。不得把超时转成成功的空路径。

矩阵读取须遵守 TypedArray 的 byteOffset、byteLength 和 backing store 生命周期，不能直接把整个底层缓冲区当作 `_bits`。嵌套搜索需要各自保存回调句柄、矩阵与异常对象，内层返回时不能释放外层仍使用的数据。普通可写矩阵的借用还须满足 Rust 别名规则；能读 backing store 不等于已经证明零拷贝接口安全。

**案 B（备选）：两段拉取协议。** search op 分帧：宿主跑到「需要房间决策」时经 op
返回值把待决房名交回 JS，prelude 收集决策后续跑。代价：一次同步调用拆成 N 次 op
往返 + prelude 维护半成品状态；**回调与搜索不再同栈**，玩家在回调里抛错/观测的
时序面全部变形。仅在同步回调方案无法满足接口与安全要求时考虑，且需在本文档登记差异。

**案 C（否决）：预取矩阵。** JS 先为可能触达的房间预生成矩阵再随 op 传入。官方
回调是懒触发（每房至多一次、触达才调，pf.cc:22-64）——预取会执行**本不该执行的
玩家回调代码**（副作用可见：日志/缓存/Memory 写），玩家可见差异，否决。

### 4.3 返回值、异常与重入行为

| 情形 | 官方行为（file:line） | 我方处置 | 状态 |
|---|---|---|---|
| 回调返回 false | pf.cc:49-52 封房 | `RoomDecision::Blocked` | 原生接口已有回归测试 |
| 返回 undefined/非 TypedArray | pf.cc:47,55 → 无矩阵 | 映射为 `RoomDecision::None` | Rust 决策已有；任意 JS 类型映射待接入 |
| 返回非 2500 长度矩阵 | pf.cc:54-57 静默忽略 | 长度透传，**官方代码**裁决 | 原生接口已有回归测试 |
| 回调 throw（JS） | §1 #8 链 → 抛异常，Undefined 是 C++ 占位返回 | 保存 JS 异常、经显式错误通道中止搜索并重新抛出 | Rust panic 路径已覆盖；JS 异常保留待接入 |
| 回调 panic（宿主 Rust 面） | 官方无 Rust 面 | panic=unwind 且载荷析构不再次 panic 时，catch_unwind → rc=3 → Rust Err | 原生接口已有回归测试 |
| 回调内嵌套 search | 官方支持两层（main.cc:9-14，第三层起堆分配 2.16MB/实例） | `TerrainLock` 重入读 + kk_ffi.cc 双实例池（复刻 main.cc）；第三层起同样堆分配 | 原生接口已有回归测试（两层） |
| 回调内 load_terrain（宿主 API） | 官方无此面（地形仅启动期载入，driver/lib/path-finder.js:23-46） | `Err(ReentrantTerrainLoad)` | 原生接口已有回归测试；差异 §6.1 |
| 回调超时/CPU 限额 | 官方无独立超时；isolate 终止 → pf.cc:558-561 → Undefined（玩家观察不到返回值，脚本被终止） | 验证 JS 回调终止与原生搜索取消；当前原生层不检查真实 V8 终止标记 | 玩家通道待接入与验证 |
| 回调返回非法类型（数字/字符串等 truthy） | 包装器交出 `ret._bits`=undefined → native 视 undefined → 无矩阵（driver/lib/path-finder.js:125-127） | 案 A 宿主闭包按官方包装器逐字复刻；取属性可能执行 getter 或抛错，不能只按 instanceof CostMatrix 判断 | 玩家接入时验证 |

### 4.4 prelude 包装器

```js
// 目标包装器；generateRoomName 将原生地图坐标转换为房间名。
function wrapRoomCallback(callback) {
  return function (xx, yy) {
    const result = callback(generateRoomName(xx, yy));
    if (result === false) return false;
    if (result) return result._bits;
    return undefined;
  };
}
```

- `generateRoomName` 可用宿主 `generate_room_name` 的 op 暴露或 JS 侧复刻
  （driver/lib/path-finder.js:51-56，公式简单，建议 JS 侧复刻减少 op 往返）；
- 保留现有 JS CostMatrix 的数值归一、平铺索引和序列化行为；搜索入口读取回调结果的 `_bits`，再按选定的复制或借用策略交给原生层。不要把 `serialize()` 引入每次搜索的传输路径。

## 5. 微基准（2026-09-28，本机 Apple Silicon，release 档）

`tests/bench.rs::bench_callback_overhead`（W5N3→W8N5 跨房远征，maxOps 100000，
2000 次/轮，8 次回调/搜索）：

| 轮次 | 无回调 | 带回调（None 决策） | 差 |
|---|---|---|---|
| 1 | 279.31 µs/次 | 279.99 µs/次 | +0.68 |
| 2 | 278.88 µs/次 | 276.90 µs/次 | -1.99 |
| 3 | 278.33 µs/次 | 281.31 µs/次 | +2.98 |

这三轮结果的差值正负都有，未显示稳定的回调成本；不能据此保证回调开销低于某个固定值，也不能得出通道无需优化的结论。该测试只覆盖 Rust 回调返回 `None`，不包括 V8 重入、矩阵读取或玩家回调执行。完整接入后须按实际搜索与回调负载重新测量。

参考：短程单房 search 1.83 µs/次（release，`bench_short_search`）。

## 6. 宿主差异与接入边界

### 6.1 `load_terrain` 重入拒绝（`Error::ReentrantTerrainLoad`）

官方地形只在进程启动期载入（driver/lib/path-finder.js:23-46），搜索期**不存在**
载入口——故无官方语义可对照（不是「官方允许/禁止」，是「面不存在」）。我方
允许多次载入（测试/多世界需要），搜索持有读锁时同线程再等待写锁会死锁，因此回调栈内的载入请求直接返回 Err。玩家 JS 不暴露 load_terrain。

### 6.2 嵌套搜索的异常隔离

每次 C ABI 调用拥有独立的值存储和异常状态。`ArenaGuard` 在入口保存外层的
`arena`、异常标志和消息，创建本层状态；返回时恢复外层。

内层搜索返回 `Err` 后，Rust 回调可以处理该错误并返回房间决策，外层继续搜索。
如需传播错误，回调可主动 panic，使外层返回 `Err(JSError(None))`。本 crate 不自动
将 Rust 错误转换成 JavaScript 异常；JS 接口层须自行实现抛出或捕获。

测试 `crates/kk-pathfinder/tests/reentrancy.rs` 分别覆盖内层回调 panic 和地形缺失被
处理后外层继续，并检查超过两份缓存实例的四层嵌套搜索。

### 6.3 CostMatrix 的 JS 数值转换

官方 `set(x, y, val)` 在 val 为 −5、NaN、3.7 时分别写入 0、0、3 的归一发生在 JS 数值层（engine
path-finder.js:25 的 `Math.min(Math.max(0, val), 255)` + TypedArray 赋值转换）；
kk-pathfinder 的 `u32` API 只做上界钳制。负值/NaN/小数面由 prelude 的 CostMatrix
类负责（4.4），玩家接入时验证。

### 6.4 玩家接入的数值与异常边界

- **负索引平铺落界内**：官方 `set(-1,55,v)` 平铺 `-50+55=5` 在界内、实际写入
  `_bits[5]`（engine path-finder.js:25 直接平铺算术，无逐轴 0..50 校验）——
  prelude 的 CostMatrix.set 复刻清单须「保平铺算术、禁逐轴校验」。
- **`|0` 归一归属**：官方选项钳制经 `(x|0)||1`（int32 强转）——`plainCost:-3`
  官方得 1；若 op 通道用 ToUint32 传 -3 得 4294967293 再钳 254 即错。op 签名
  取 i32 或 prelude 先做 `|0`。
- **空 goals 两键形态**：官方 engine path-finder.js:60-62 对空 goals 返回
  `{path:[],ops:0}`（**无** cost/incomplete 两键）——prelude 在调 op 前特判。
- **deserialize 的整数转换**：官方 JS 对 >2^32 的词值取 mod 2^32（TypedArray
  赋值转换），Rust `CostMatrix::deserialize` 只接受 `&[u32]`，不负责负数、小数、NaN 或超过 u32 的 JS 输入转换——接入时验证这些输入的转换结果。
- **panic 捕获边界**：依赖 panic=unwind。panic=abort、panic hook 主动退出或 panic 载荷析构再次 panic 不在恢复保证内；修改编译策略时须重新评估。
- **回调异常的消息归属**：宿主层 `Err(JSError(None))` 不携带文案——玩家侧 JS
  包装层直接 rethrow 原生 JS 异常（保留玩家异常对象与栈）；地形缺失的
  `JSError(Some("Could not load terrain data"))` 文案照官方。

## 7. 原生回归测试

| 测试 | 锁定行为 |
|---|---|
| `tests/callback_safety.rs::cost_matrix_wrong_length_ignored_at_search` | 非 2500 字节矩阵经官方 pf.cc:54-57 静默忽略（= 基线路径）；2500 字节对照组生效 |
| `::room_callback_panic_first_room_maps_to_official_throw` | 可展开的普通回调 panic 返回 `Err(JSError(None))`；捕获边界见第 6.4 节 |
| `::room_callback_panic_second_room_maps_to_official_throw` | 异常时刻无关性（第二房 panic 同 Err） |
| `::search_works_after_callback_panic` | panic 后池实例复位（pf.cc:565），后续搜索正常 |
| `::load_terrain_in_callback_rejected_not_deadlock` | 回调内 load_terrain → `Err(ReentrantTerrainLoad)`；线程+recv_timeout 防挂起；外层搜索正常完成 |
| `::nested_search_in_callback_works` | 嵌套两层 search 正常（官方 main.cc:9-14 语义） |
| `src/lib.rs::cost_matrix_out_of_range_silent` | 平铺索引越界静默；(0,50)/(1,0) 同字节别名；溢出索引不 panic |
| `src/lib.rs::cost_matrix_deserialize_any_length_silent` | deserialize 任意长度恒成功，界内可读/界外 None，serialize 往返 |
| `tests/official_golden.rs::official_profile_checksum` | 金标 11843305（官方 profile.js 全对场景）；适用于原生搜索，不能证明玩家通道兼容 |
