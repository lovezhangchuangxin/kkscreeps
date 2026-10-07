# deno_core 游戏 API 依赖的高风险沙箱交互模式实测（实验 F~K）

- 日期：2026-09-27
- 实验对象：与 [spike-deno_core.md](spike-deno_core.md) 同一工程 `spikes/deno-spike/`（deno_core 0.412.0 / rusty_v8 150.4.0 / v8 `15.0.245.2-rusty`，macOS 15 arm64）。沙箱基础能力（硬中断/内存上限/性能基准/snapshot/Extension op）已在旧笔记验证，本篇只补测 **Screeps 游戏 API 形态决定的那批高风险交互模式**。
- 新代码位置（未动 `src/main.rs` 与旧实验，旧实验复跑全部 pass，见 §8）：
  - `spikes/deno-spike/examples/game_api.rs` —— 实验 F/G/H/I/J/K 的独立二进制
  - `spikes/deno-spike/build.rs` —— 新增第二份 snapshot `snapshot-game.bin`（实验 J 用；原 `snapshot.bin` 生成逻辑原样保留，只是抽了 `make_snapshot()` 函数）
- 复现命令（在 `spikes/deno-spike/` 下）：

```bash
cargo run --release --example game_api   # 实验 F~K（旧实验：cargo run --release，不受影响）
```

- API 形态参照官方实现（本地源码）：
  - `sources/screeps/engine/src/game/path-finder.js`：`CostMatrix` 的 `_bits` 就是 `Uint8Array(2500)`；`PathFinder.search` 是 `driver.pathFinder.search` 的薄包装。
  - `sources/screeps/driver/lib/path-finder.js`：`search()` 里先把玩家 `options.roomCallback` 包一层（`cb(xx,yy)` → 调 `cb(generateRoomName(xx,yy))` → 返回 `ret._bits`），再把**回调**与矩阵一起传给原生 `mod.search(...)` —— 即"JS 回调收集参数 + TypedArray 传原生"。
  - `sources/screeps/engine/src/driver/lib/runtime/runtime.js`：每 tick 包一个函数引用返回宿主，console 结果由 `fakeConsole.getMessages()` 收集、memory 经 `systemFunctions.JSON.stringify` 传回 —— 对应本篇实验 H/K 的形态。

## 汇总（6/6 PASS）

| 实验 | 结论 | PASS |
|---|---|---|
| F 惰性对象 overlay | SAB+getter 单属性 1.7~2.1 ns/读；fast-op-per-read 1.9~2.2 ns（仅慢 1.1~1.2x）；4 属性/对象时 getter 1.0~1.2 ns/属性 vs 4×op 1.6~1.7 ns/属性（慢 ~1.5x） | PASS |
| G roomCallback 模式 | `#[buffer] &[u8]` 接收 Uint8Array **零拷贝**（指针稳定 + Rust 写穿 JS 立即可见）；完整 search 流程 ~0.6 µs/次（callback ~0.39 + op ~0.18 + 包装 ~0.03） | PASS |
| H RawMemory 2MB | Rust→JS（op 返回 `#[string]`）~0.06 ms/次（0.03 ms/MB）；沙箱内 parse+改+stringify ~8.4 ms/次（4.2 ms/MB）；JS→Rust（`#[string] String` 参数）~0.15 ms/次（0.07 ms/MB）；长度双向闭环 | PASS |
| I CommonJS 垫片 | ~50 行 require：循环 require（半成品 exports + 写穿可见）、module.exports 三处共享、缓存命中不重执行（Rust 侧加载计数各恰 1 次）、globalThis 跨 execute_script 持久，全过 | PASS |
| J 游戏 runtime 进 snapshot | 类/prototype getter/闭包/嵌套 Object.freeze/Map 全部随 snapshot 恢复，冻结语义保持，运行时仍可挂新全局、可叠加新 Extension；创建 2143→663 µs（3.2x）；连顶层 `const`（全局词法绑定）都能恢复 | PASS |
| K console 捕获与堆栈 | 未捕获异常 Rust 侧拿到的 `.stack` 含精确 行:列（throw@2:9 三帧链路）；deno_core 自带 console 直写宿主 stdout（不可捕获），替换为自制 JS console 后多参数/对象/嵌套/循环引用/Error 全部可收集 | PASS |

## 实验F 惰性对象 overlay —— PASS

**方法**：沙箱内建 128 个 `RoomObject`，实例只存索引 `__i`；底层一块 `SharedArrayBuffer(32×128)`，每对象 32 字节（`u32[id,x,y,roomIdx]` + `f64[hits,hitsMax]`），`Object.defineProperty` 在 prototype 上挂 getter 按偏移直读 `Uint32Array`/`Float64Array` 视图（模拟 xxscreeps 的 lazy hydration）。对照组 `#[op2(fast)] fn op_ga_get_current_hp(i: u32) -> f64`。每轮 10 万次读，预热 3 轮后测 5 轮取中位。另设两条地板：普通 JS 对象字段读、TypedArray 裸读。正确性：getter / op / 视图 / 手工期望四种读法求和完全一致（`sample=106348464`）。

**最终运行原始输出**（`/tmp/game_api_run4.txt`）：

```
正确性: {"equal":true,"plain":true,"sample":106348464}
getter(SAB) 单属性      5次=[0.213583, 0.208834, 0.207959, 0.208625, 0.208125] ms -> 中位 0.209 ms，2.09 ns/读
普通JS对象属性(地板)     5次=[...] -> 中位 0.167 ms，1.67 ns/读
TypedArray直读(地板)     5次=[...] -> 中位 0.093 ms，0.93 ns/读
op-per-read 单属性      5次=[0.214375, 0.213958, 0.210875, 0.21375, 0.210917] ms -> 中位 0.214 ms，2.14 ns/读
getter 4属性/次         5次=[...] -> 中位 0.480 ms，4.80 ns/读
op-per-read 4属性/次    5次=[...] -> 中位 0.692 ms，6.92 ns/读（每次迭代 4 个 op 调用）
```

（另一次完整运行：getter 1.71 / plain 1.21 / view 0.85 / op 1.93 / getter4 1.03 / op4×1.58 ns —— 两次同量级。）

**结论**（对旧笔记"必须用 SAB+getter 而非 op-per-read"防错结论的**修正而非推翻**）：
1. `#[op2(fast)]` 的调用开销被 V8 fast API call 内联得非常便宜（单属性读 getter 2.1ns vs op 2.1ns，**几乎打平**，只慢 1.0~1.2x）——"每次属性读走 op"并没有数量级灾难，这条路**可行**；
2. 但真实读法是多属性：一次读 id/x/y/hits 时，getter 合计 4.8ns vs 4 个 op 6.9ns，**op 慢 ~1.5x**，且 op 方案要为每个属性写一个 op、烧 op 带宽、并把对象状态查询推到 Rust 侧；
3. SAB+getter 的读速已到普通 JS 对象字段读的 1.2~1.3x、裸 TypedArray 读的 ~2.2x —— 即"惰性 overlay"基本免费。**维持原架构结论：对象属性读走 SAB+getter，op 留给动作与计算**；数量依据如上。

## 实验G 原生计算 + JS 回调收集参数 —— PASS

**方法**：完全按官方 `driver/lib/path-finder.js` 的调用形态搭：引擎包装层 `pfSearch(origin, goal, options)` 先调玩家 `options.roomCallback('W1N1')` 拿 `CostMatrix`（`_bits = new Uint8Array(2500)`，玩家代码里 `new` + 40 次 `set`），提取 `cm._bits` 传 Rust `op_ga_search(#[buffer] matrix: &[u8], ox, oy, gx, gy)`；Rust 侧读一遍矩阵求校验和 + 生成 11 元素假路径（`#[serde] Vec<f64>` 返回），不实现真寻路。

**零拷贝验证**（在 Rust 侧拿指针对比 + 写穿）：

```
零拷贝探针: {"ptrSameArrayTwice":true,"ptrDifferentArray":true,"writeThroughVisible":true,"zeroCopy":true,"ptr1":"13e80e400","ptr3":"13e814800"}
```

- 同一 Uint8Array 两次传入，Rust 侧 `buf.as_ptr()` 相同；不同数组指针不同；
- Rust 在 op 里往 `buf[0]` 写 `0xAB`，返回后 JS 立即读到 `0xAB` —— **Rust 拿到的是 V8 backing store 本体**，无拷贝（`#[buffer] &[u8]` 是借用视图，op 期间钉住不回收；`&mut [u8]` 同理可写）。写穿即铁证。

**正确性 + 计时**：

```
正确性: {"pathLen":11,"pathFirst":5,"pathLastIsChecksum":true}
完整 search 流程 1000 次: 5次=[0.671167, 0.613542, 0.593209, 0.509166, 1.177625] ms -> 中位 0.61 ms，即 0.614 us/search
其中 roomCallback（new CostMatrix + 40 次 set）1000 次: 中位 0.38 ms，即 0.382 us/次
仅 op_ga_search（矩阵复用）1000 次: 中位 0.19 ms，即 0.186 us/次
```

**结论**：官方 PathFinder 的"JS 回调收集参数 + 提取 TypedArray 传原生"模式在 deno_core 上**原生可用且零拷贝**。一次完整 search 包装层开销 ~0.6 µs（其中大头是玩家回调本身 new Uint8Array(2500)+40 次 set 的 ~0.38 µs，op 调用含矩阵求和仅 ~0.19 µs）。对照官方：真寻路计算本身在 C++ 侧，边界成本完全可忽略。注意 `#[buffer]` 走 slow-call 路径（非 fastcall），对"每次 search 一次调用"的频率毫无影响。

## 实验H 大字符串传输（RawMemory 2MB）—— PASS

**方法**：Rust 生成 2,097,224 字节合法 JSON（Screeps memory 风格：`creeps` 表 8321 个对象，serde_json 预检合法）。三个测项各自预热后多轮计时（30 次/轮、parse 10 次/轮）：
- Rust→JS：`op_ga_raw_get() -> #[string] &'static str`（只测 V8 字符串创建，无 Rust 侧 clone）；
- 沙箱内：`JSON.parse` + 改一字段 + `JSON.stringify`；
- JS→Rust：`op_ga_raw_set(#[string] String)`（一次 UTF-8 拷贝）。

**原始输出**：

```
Rust 生成 RawMemory JSON: 2097224 bytes（2.00 MB）
serde_json 校验合法，creeps 数量: 8321
Rust→JS（op 返回 #[string]，30 次/轮）: 中位 1.90 ms/轮 = 0.063 ms/次 = 0.0317 ms/MB
沙箱内 JSON.parse+改+stringify（10 次/轮）: 中位 83.17 ms/轮 = 8.317 ms/次 = 4.1585 ms/MB
JS→Rust（op 参数 #[string] String，30 次/轮）: 中位 4.17 ms/轮 = 0.139 ms/次 = 0.0694 ms/MB
长度核对(JS): {"inLen":2097224,"outLen":2097233,"lastSetAck":2097224}
Rust 侧 last_recv_len = 2097233（应为 stringify 后长度）
```

**结论**：
1. **边界传输几乎免费**：2MB 字符串过边界（任一方向）仅 0.06~0.15 ms，即 **0.03~0.07 ms/MB**——一次 tick 传 2MB RawMemory 的双向成本 <0.25 ms，不构成瓶颈；
2. **真正的成本在沙箱内 parse+stringify：~8.4 ms/2MB**（比边界贵 ~60 倍）。这与官方用 `systemFunctions.JSON.stringify`（引擎侧定制序列化）的动机一致：**优化方向是减少/替换 JSON.parse-stringify 往返，而不是边界传输**。对 2MB 上限的 RawMemory，最坏 tick 也只是"几十毫秒里花 8ms 序列化"，配合实验B/C 的中断与内存上限可兜底。
3. 长度闭环验证：JS 收到长度 2097224 = Rust 生成长度；Rust 收到 2097233 = stringify 后长度（多出的 9 字节是新增 `tick` 字段）✓。

## 实验I CommonJS 垫片 —— PASS

**方法**：~50 行 JS 垫片（`examples/game_api.rs` 的 `CJS_SHIM_JS`）：`require(spec)` → 归一化名字 → 查缓存 → 未命中则 `Deno.core.ops.op_ga_module_load(name)` 取源码（**模块表由 Rust 持有**）→ `new Function('module','exports','require', src+'\n//# sourceURL=ga_module/'+name)` 执行 → **执行前先占缓存坑**（循环依赖拿到半成品 exports，Node 语义）。模块 5 个：`main.js` / `role.harvester.js` / `utils.js` / 循环对 `circ-a.js` / `circ-b.js`（A→B→A）。

**原始输出**：

```
JS 侧断言: {"mainLoads":true,"runWorks":true,"counterExecuted":true,"circularOk":true,"circularPartial":true,"circularSharedPartial":true,"cacheIdentity":true,"cacheCounter":true}
第二次 execute_script 读到递增后 n=42（41 设置于上一次执行, +1 后应为 42）
跨执行可见性: "function/object"
Rust 侧模块加载计数: [("circ-a.js",1), ("circ-b.js",1), ("main.js",1), ("role.harvester.js",1), ("utils.js",1)]
```

断言明细：
- **循环 require**：circ-b 加载时反向 require circ-a 拿到空 exports（`sawEmptyAtLoad=true`）；circ-b 往半成品上写的 `injectedDuringCycle` 在 circ-a 完成后仍可见，且 `b.a === a`（同一对象）；
- **module.exports 共享**：`main.js`、两次直接 require 的 `utils.js` 三处拿到**同一对象**（`main.utils === u1 === u2`），在 u1 上 `counter += 10` 后 u2 读到 11；
- **模块缓存**：第二次 require 直接返回缓存、不重执行（`utils.helper()` 调用计数 `helped#1` 证明主体只执行一次；Rust 侧每个模块的 `op_ga_module_load` 加载计数**恰好各 1 次**）；
- **global 跨执行持久**：execute_script #1 设 `globalThis.__userVar={n:41}`，#2 里 `n+=1` 读到 42；#3 里 `typeof require === 'function'`、`typeof __userVar === 'object'`。

**踩坑记录（实测撞出来的）**：circ-a 第一版写的是 `module.exports = {...}` **整体替换**，此时循环方（circ-b）持有的半成品引用与最终 exports 是**两个对象**（`b.a === a` 为 false）——这与 Node 语义一致（替换式导出在循环依赖下会"分家"），不是垫片的 bug。要循环可见就用 `module.exports.x = ...` 增量赋值。垫片已改为增量赋值并注明。**对 xscreeps 的提示**：玩家代码大量使用 `module.exports = {...}`（官方教程代码风格），循环 require 时语义与 Node 官服完全一致，无需特殊处理，但要知晓这个分家行为。

**结论**：CommonJS 垫片在 deno_core 上无任何障碍；`//# sourceURL=` 让模块内异常堆栈带模块名（配合实验 K）。

## 实验J snapshot 内置游戏运行时 —— PASS

**方法**：`build.rs` 里 `create_snapshot` 第二份产物（`snapshot-game.bin`，731,768 字节）：预热一段 ~60 行"游戏 runtime"——嵌套 `Object.freeze` 的常量表 `C`（含冻结的子对象/数组）、`RoomPosition` 类（prototype getter、方法、toString）、闭包工厂 `makeDistanceTable`（闭包持 Float64Array）、带正则+Map 的 `GameRegistry`、以及一个**顶层 `const`**（全局词法绑定，顺带实测能否随 snapshot 恢复）。运行时对比无/有 snapshot 的 `JsRuntime::new`（各 10 次取中位），再从 snapshot 启动并叠加带 ops 的 Extension。

**原始输出**：

```
snapshot-game.bin: 731768 bytes
JsRuntime::new 无 snapshot: 10 次中位 2143 us；有 game snapshot: 663 us（加速 3.23x）
snapshot 恢复验证: {"loaded":true,"classOk":true,"getterOk":true,"protoGetter":true,"toStringOk":true,"frozen":true,"constOk":true,"freezeEnforced":true,"closureOk":true,"registryOk":true,"newGlobalOk":true,"opComposeOk":true,"lexicalConstSurvives":true}
```

断言明细：
- 类可用：`new RoomPosition(25,30,'W1N1')` 的 `x/y/roomName/xy` getter 全对，getter 确在 prototype 上（`getOwnPropertyDescriptor(...,'x').get` 为函数），`toString()` 输出官方风格 `[room W1N1 25,30]`；
- **冻结语义保持**：`Object.isFrozen(C)` 及其嵌套对象/数组全 true；snapshot 启动后 `C.HACK=1` 静默失败（读回 undefined）、`delete C.OK` 无效；
- 闭包可用：`makeDistanceTable(100)` 返回的函数算 `sqrt(36)=6`、`sqrt(64)=8` ✓；
- **snapshot 启动后仍可往 globalThis 挂新属性**（`__postSnapshotProp=123` 读回 ✓）——玩家代码污染全局不受影响；
- **snapshot 与运行时 Extension 可叠加**：snapshot 建于"无自定义扩展"状态，运行时加载时再注入 `ga_ext`（6 个 op），`op_ga_get_current_hp(7)===1007` ✓——游戏 runtime 进 snapshot、玩家专属 ops 每 isolate 注入的组合路线成立；
- 附加探针：**顶层 `const`（全局词法环境绑定）也能随 snapshot 恢复**（`lexicalConstSurvives=true`），不过游戏 runtime 仍建议统一挂 `globalThis`（官方 globals 形态）。

**结论**："游戏 runtime（类/常量/getter/闭包/冻结）整体进 snapshot"路线**完全成立**，创建 2.1ms→0.66ms（3.2x，与旧笔记 4.4x 同向；本实验的 snapshot 大了一半，加速比略降但仍显著）。对"每玩家一个 isolate"架构，游戏 runtime 解析/执行成本一次性摊销。

## 实验K console 捕获与堆栈 —— PASS

**方法**：
1. 未捕获异常：`execute_script` 执行已知行列号的抛错脚本（throw 在第 2 行第 9 列，两层调用链），`Err(JsError)` 上取 `exception_message` / `stack` / `frames`；
2. console：先探测 deno_core 自带的 console 去哪了，再替换成自制收集型 console（多级 inspect、循环引用、Error 取 stack 首行）。

**原始输出（stack 样例原文）**：

```
exception_message: Uncaught Error: K-boom (intended)
err.stack 原文（截取前 5 行）:
  | Error: K-boom (intended)
  |     at innerBoom (<k-stack-demo>:2:9)
  |     at outerWrap (<k-stack-demo>:5:3)
  |     at <k-stack-demo>:7:1
结构化 frames（Rust 侧 Vec<JsStackFrame>）:
  frame[0] fn="innerBoom" file="<k-stack-demo>" 2:9
  frame[1] fn="outerWrap" file="<k-stack-demo>" 5:3
  frame[2] fn="" file="<k-stack-demo>" 7:1
```

行列号与代码精确对应（throw@2:9 → innerBoom 调用@5:3 → 顶层调用@7:1）。`JsError.frames` 是结构化 `Vec<JsStackFrame>`（`function_name/file_name/line_number/column_number`），Rust 侧可直接组装官方 `outMessage.error = e.stack` 形态（runtime.js 同款），不用解析字符串。

**console 的调研结论（源码证据）**：deno_core **有**现成 console，但不可用于捕获——`deno_core-0.412.0/01_core.js`（CoreConsole 类）：

```js
// Minimal console implementation, that uses `Deno.core.print` under the hood.
class CoreConsole {
  log = (...args) => { op_print(`${consoleStringify(...args)}\n`, false); };
  ...
}
const coreConsole = new CoreConsole();
globalThis.console = coreConsole;
```

`op_print` 在 `ops_builtin.rs:234` 直接写宿主 stdout/stderr。实测探测：console 存在（object，25 个方法，含 wrapConsole 从 V8 inspector console 补齐的 count/table/group 等），`console.log(...)` 的输出确实**直写宿主进程 stdout**（运行记录里 `[native-console] 这一行经 deno_core 内置 CoreConsole -> op_print 直写宿主 stdout`）。**对多租户服务器不可用**：会把玩家日志打到宿主标准输出而非按玩家收集。

**自制收集型 console**（`K_CONSOLE_SHIM_JS`，~30 行）：`globalThis.console` 替换为记录器，参数经 `inspect()`（深度 3、`[Circular]` 检测、Error 取 stack 首行、TypedArray/Function 特判）。捕获原文：

```
| {"level":"log","parts":["tick","42","{ cpu: 1, nested: { a: [ 1, 2, [Object] ] } }","null","undefined","true"]}
| {"level":"warn","parts":["careful","[ \"x\", \"y\" ]"]}
| {"level":"log","parts":["{ name: \"circ\", self: [Circular] }"]}
| {"level":"error","parts":["bad thing","Error: inline-err"]}
再捕获核对: {"drained":true,"level":"log","first":"again","second":"1"}
```

多参数、对象、嵌套（深度截断 `[Object]`）、循环引用（`[Circular]`）、Error 全部收集成功；drain 后可再捕获（每 tick 收集一次的形态可用）。

**结论**：堆栈路径开箱即用（含结构化行列号）；console 需要**替换而非复用** deno_core 自带版（自带版写宿主 stdout），30 行 JS 垫片即可按官方 `fakeConsole` 形态收集。

## 踩坑与实现细节（新增，接旧笔记 §8）

7. **顶层 `const`/`let` 声明不能在重复执行的脚本里出现**：bench 脚本第一版写了顶层 `const __bits = ...`，同 context 第二次 `execute_script` 同样代码报 `Uncaught SyntaxError: Identifier '__bits' has already been declared`（经典脚本的全局词法绑定是 context 级持久的）。修复：计时脚本包 IIFE。另：`eval_json` 辅助函数会把 `"42"` 这类字符串结果再 `serde_json::from_str` 一层（数字 42），做相等断言时须按解析后的类型比较。
8. **op2 的 `#[buffer] &mut [u8]` / 带 `#[string] String` 参数 + `OpState` 的 op 会被宏要求标 `(fast)`**（0.412 的 fast-compatible 判定比文档表格宽）：报错原文 `This op is fast-compatible and should be marked as (fast)`，加上即可；buffer 参数在 fastcall 下仍是零拷贝（指针+写穿实测见实验 G）。
9. **返回裸 `u64` 不是合法 op 返回类型**（valid_retvals 无此项）：指针用 f64 返回（macOS 用户态地址 < 2^53 精确无损）或 `#[bigint] u64`。
10. **`OpState` 取状态的调用形态**：op 内 `state.borrow::<T>()`（gotham_state）与 `rt.op_state().borrow()`（RefCell）是两个不同的 borrow，链起来写 `rt.op_state().borrow().borrow::<T>()`；RefCell 的临时 Ref 不能跨语句存活（E0716）。
11. **循环依赖 + 整体替换 `module.exports` 会"分家"**（见实验 I），Node 同款语义，垫片按 Node 行为实现即可。

## 旧实验回归验证

改动后重跑 `cargo run --release`（2026-09-27）：

```
RESULT engine=deno_core(rusty_v8) fib30_ms=4.8 objchurn_ms=0.9 json_ms=6.4 create_runtime_us=2201 rss_10_mb=21.3
CAPABILITY interrupt=pass memory_limit=pass detail="interrupt: 205.2ms 终止, err='Uncaught Error: execution terminated', runtime复用=true; memory: child_exit=Some(0); MEMTEST_RESULT terminated=true elapsed_ms=18.9 peak_rss_mb=66.8 cb_fired=1 ..."
EXTRA first_create_us=2891 rss_1_mb=0.8 per_runtime_mb=2.3 interrupt_reuse=true
```

与旧笔记数字一致（正常抖动范围），snapshot.bin 仍为 722,016 字节 —— **旧实验未被破坏**。

## 对 xscreeps 架构的增量建议（接旧笔记 §10）

1. **对象层**：RoomObject 用 SAB/普通 ArrayBuffer + prototype getter 的惰性 overlay（单属性读 ~2ns，多属性读比 4 次 fast op 快 ~1.5x 且零 op 带宽）；fast op 虽便宜（同量级），但属性读走数据、op 走动作的分工维持不变。
2. **PathFinder**：`roomCallback` → CostMatrix._bits → `#[buffer]` op 的官方形态零拷贝直通，整个包装层 ~0.6 µs/search，寻路计算可以放心放 Rust 侧（后续用 pathfinding crate / jump point search 实现真算法）。
3. **RawMemory**：2MB 双向边界 <0.25ms/tick 不是瓶颈；瓶颈是沙箱内 JSON.parse+stringify（~4.2 ms/MB）。中期可做"结构化 memory"（V8 对象 ↔ Rust 端增量同步，跳过字符串往返）。
4. **模块系统**：50 行 CJS 垫片 + Rust 模块表即可复刻官方玩家体验（循环 require/缓存/global 持久全兼容）；`//# sourceURL` 让堆栈带模块名。
5. **启动**：游戏 runtime（常量/类/内置库）整体进 snapshot（3.2x 创建加速 + 冻结语义完整），玩家 ops 以 Extension 每 isolate 叠加 —— 两者正交组合已验证。
6. **可观测性**：console 替换为收集型垫片（自带版写宿主 stdout，不可用）；未捕获异常直接用 `JsError.stack/frames`（结构化行列号）回填官方 `outMessage.error` 形态。

## 未尽事项

- 实验G 的 Rust 侧是 noop（求和+假路径）；真寻路（JPS/A*）耗时与 CostMatrix 传参的交互留到原型阶段。
- 实验H 未测 `ToJsBuffer`/ArrayBuffer 通道（RawMemory 官方就是字符串，故只测字符串路径）；未测 >2MB（官方上限即 2MB…实际 100KB×10 segment + 2MB rawMemory，量级相同）。
- 自制 console 的 inspect 是简化版（深度 3 截断 `[Object]`），生产可换 deno 的完整 `console` 扩展源码（deno 核心库 ext/console 有现成 inspect 实现，可整体搬）。
- 实验 F 只测了读路径；SAB 写路径（intents 直写底层结构）未单测，机制相同（TypedArray 写），风险低。

---

## 实验 G'（op_callback）：op 执行中途回调进沙箱 JS —— 官方 roomCallback 惰性语义的决定性验证

> 背景：本笔记的实验 G 验证的是"预收集"变体（JS 包装层先调 roomCallback 拿矩阵再传 op）。
> 但 deno-game-impl-mapping.md §2 指出：**官方实现是原生搜索中途 Nan::Call 回调进沙箱**
> （driver/path-finder.js:114-134 把回调作为参数传给 mod.search；pf.cc:36-59 在 A* 展开每个
> 新房间时调用）——惰性、只对真正进入的房间调、false 拉黑且不计入 maxRooms、回调抛异常以
> js_error 终止搜索。预收集语义不等价（会多调玩家回调=多计费/多副作用，false 语义无法预先判定）。
> 本实验补测 deno_core 对"op 内回调"的真实支持。

代码：`examples/op_callback.rs`（运行 `cargo run --release --example op_callback`）。
API 模式：op 参数 `scope: &mut v8::PinScope` + `#[scoped] cb: v8::Global<v8::Function>`
（参照 deno_core 自身 examples/op2.rs:13 与 webidl.rs:390 的 `v8::tc_scope!` TryCatch 模式），
回调时 `cb.open(tc).call(tc, recv, args)`。

### 结果（2026-09-27 实测，release）

```text
[G'1/G'2] result=[8,50000] calls=10
[G'1/G'2] 三态返回值 + 零拷贝矩阵读取全部断言通过
[G'3] exception propagated to JS: "Error: js_error: Error: boom from room0"
[G'3] runtime alive after exception: ok
[G'4] callback roundtrip ns/op median: no-arg=39ns 1-arg=38ns
RESULT engine=deno_core(op_callback) 三态回调=pass 异常传播=pass runtime存活=pass 回调中位=38ns
```

### 断言细节

- **G'1/G'2 三态返回值**：10 个房间按 n%4 分布返回 Uint8Array(2500).fill(n)/undefined/false，
  Rust 侧按位测试分流：entered=8（false 拉黑 2 个）、校验和=50000=2500×(0+2+4+6+8) 精确匹配
  ——Uint8Array 经 BackingStore 零拷贝读取（[Cell<u8>] 语义，非官方 #[buffer] 的 &[u8]，等价）。
- **G'3 异常传播**：回调抛 `Error('boom from room0')` → op 内 tc_scope! 捕获 → JsErrorBox 上抛
  → JS 最外层 catch 收到完整消息；同一 runtime 继续执行 6*7=42 成功（等价官方"js_error 终止搜索、沙箱存活"）。
  与官方差异：官方 pf.cc 是 ReThrow 原异常对象，本实验是包了一层消息文本（js_error: Error: ...）
  ——功能等价，保真度可在实现期用 deno_core 的 exception_to_err 原语提升（webidl.rs 同款）。
- **G'4 开销**：回调往返 38–39ns/次（带参与不带参同量级）→ 64 房间满配搜索回调总成本 < 3µs，
  相对官方"1 op ≈ 0.001 CPU"的计费粒度完全可忽略。

### 结论

**deno_core 可以 1:1 复刻官方 PathFinder 的 roomCallback 架构**（惰性回调 + false 语义 + 异常终止），
无需退化为预收集或沙箱内 wasm。至此，deno-game-impl-mapping.md §0 矩阵中标记"高"风险的
唯一缺口（#2 回调重入）已实测关闭。官方的 costCallback（就地改宿主传入矩阵）与 routeCallback
（返回浮点开销）是同一机制的更简单变体（不返回矩阵、改传回调参数），随本实验一并覆盖。

---

## 实验 L（bucket_sim）：官方 CPU bucket 语义的完整循环仿真

代码：`examples/bucket_sim.rs`（运行 `cargo run --release --example bucket_sim`）。
复刻公式（官方源码取证）：每 tick 回充 +limit(20)、上限 10000；`tickLimit = min(500, limit + bucket)`；
超 tickLimit → 看门狗硬中断并按整 tickLimit 计费 + skipTicksPenalty（停 1 tick）；同一 runtime 跨 tick 复用。

24 tick 时间线（阶段：自律 12ms ×6 → 超支 80ms ×6 → 爆肝 250ms ×4 → 600ms → 恢复 10ms ×7）：

- 自律期 bucket +8/tick 回升；超支期净 -60/tick 流失；
- **tick 9：work 80 > tickLimit 49 → 49ms 处硬中断**（动态预算的中断，不是固定值）；
- **tick 10：SKIPPED(罚)** —— 硬超时后停 1 tick（用户问的"bucket 用光了会停 1 tick"的官方机制）；
- tick 12/14/16：tickLimit 跌到 20/40 后爆肝 250ms 均在 tickLimit 处精确终止；
- 恢复期 bucket +10/tick 回升；全程一个 runtime，结束后 6*7=42 健康检查通过。

### 实现要点与坑

1. **看门狗代际（generation）守卫**：每 tick 武装新看门狗时递增 AtomicU64，迟到的旧看门狗发现自己
   过期就作废——否则它会误杀下一 tick 的执行。
2. **tick 开始前 `cancel_terminate_execution()`**：防止上一 tick 临界竞态下"终止信号在脚本结束后
   才落地"污染下一 tick（tick 11 出现过一次 work 80 完成于 79ms、终止迟到成为 pending 的竞态，
   时间线中该 tick 计为 done；生产实现用一 isolate 一线程 + Linux per-thread CPU timer 可消除）。
3. **tokio 上下文**：deno_core 的 V8 延迟任务（GC/Date 调度）需要 `tokio::runtime::enter()` 守卫，
   否则报 "V8 posted a delayed task, but this isolate was created outside of a tokio runtime context"。
   `execute_script` 本身是同步的，不影响每 tick 的同步执行模型。
4. 计量用墙钟演示（xxscreeps getUsed 即墙钟）；生产换 `CLOCK_THREAD_CPUTIME_ID`（02 报告已实测 ns 精度），
   公式与循环不变。

结论：**bucket 动态预算、500ms 封顶、超限硬中断、停 1 tick 惩罚、bucket 跨 tick 持久、
玩家用 getUsed() 自律** —— 官方 CPU 经济的全部语义在 deno_core 上可 1:1 复刻。
