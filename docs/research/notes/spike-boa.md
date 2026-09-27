# Spike 笔记：boa_engine（纯 Rust JS 引擎）实测

> 调研日期：2026-09-27
> 实验对象：boa_engine 0.22.0（crates.io 当前最新稳定版）
> 目的：评估 boa 作为 xscreeps 玩家 JS 沙箱的五个关键能力——性能、硬中断、内存硬上限、创建开销/常驻内存、默认安全面。
> 复现命令（在仓库根目录）：`cargo run --release --manifest-path spikes/boa-spike/Cargo.toml`
> 实验代码：`spikes/boa-spike/src/main.rs`（单文件，实验 A–E 一次运行全部打印）

---

## 0. 精确依赖版本（来自 `spikes/boa-spike/Cargo.lock`）

| crate | 版本 | 说明 |
|---|---|---|
| boa_engine | **0.22.0** | 引擎本体（crates.io 最新稳定版，2026-09-27 查询） |
| boa_ast / boa_parser / boa_gc / boa_interner / boa_macros / boa_string | 0.22.0 | boa 全家桶同版 |
| sysinfo | 0.39.6 | RSS 测量（crates.io 最新稳定版） |
| regress（boa 依赖） | 0.12.0 | 正则引擎 |
| temporal_rs（boa 依赖） | 0.2.6 | Temporal 实现 |

工具链：rustc 1.98.1 (48a229cea 2026-09-01)，cargo 1.98.1，macOS 24.6.0 arm64（M 系，10 核）。
boa_engine 默认 feature：`float16`、`xsum`、`temporal`（无 `intl`，无 `fuzz`）。

---

## 1. 关键 API 事实查证（读源码，非文档转述）

以下结论全部来自对 `~/.cargo/registry/src/.../boa_engine-0.22.0/src/` 与 boa GitHub main 分支的实际检索：

1. **没有时间式中断机制**。0.22.0 源码全树 `grep -rni interrupt` **0 命中**；main 分支 `core/engine/src/context/mod.rs`（1326 行）对 `interrupt|memory_limit|heap_limit` 同样 0 命中。
   GitHub 佐证：issue **#3442**「Cancel/Interrupt active evaluation」**仍 open**；PR **#5475**「Add Arc\<AtomicBool\> interrupt flag」与 PR **#5380**「HostHooks::on_tick for interrupting sync interpreter」均**关闭且未合并**（`merged: false`）。即官方中断能力至今未落地。
2. **存在计数式运行限制** `RuntimeLimits`（`src/vm/runtime_limits.rs`）：`loop_iteration`（默认 `u64::MAX` 即不限制）、`recursion`（默认 512）、`stack_size`（默认 10KB）、`backtrace_limit`。执行点在 `src/vm/opcode/iteration/loop_ops.rs:13-24`——`IncrementLoopIteration` 操作码在**每次循环回边**检查，超限抛 `RuntimeLimitError::LoopIteration`（错误文案见 `src/error/mod.rs:309`："reached the maximum number of iteration loops on this execution"）。计数器在 `CallFrame` 上（`src/vm/call_frame/mod.rs:69`），**每调用帧累计、跨帧重置**。
3. **另有一个指令预算机制但藏在 `fuzz` feature 后**：`ContextBuilder::instructions_remaining`（`src/context/mod.rs:1184-1189`，`#[cfg(feature = "fuzz")]`），VM 每条指令递减、归零抛 `EngineError::NoInstructionsRemain`（`src/vm/mod.rs:781-790`）。非默认 feature、非时间式，本次未实测运行（仅源码查证）。
4. **没有任何内存上限 API**。`Context`/`ContextBuilder`/`boa_gc` 公开面均无 heap limit；`boa_gc` 公开 API 仅 3 个函数（`src/lib.rs:119,534,550`：`finalizer_safe`、`force_collect`、`has_weak_maps`），GC 内部的 `bytes_allocated`/`threshold` 全部私有（`src/lib.rs:55-77`），宿主无法读取或配置。
5. **GC 堆是 thread_local 的**（`boa_gc/src/lib.rs:45` `thread_local!(static BOA_GC ...)`）：同一线程上所有 Context 共享一个 GC 堆 ⇒ 多 Context 之间没有内存隔离，边际常驻极小（见实验 D）。
6. `Context` 不是 `Send`（内部 Rc），无法把正在跑脚本的 Context 移交或从别的线程干预——这是"没有时间中断"难以自行补救的根因。

---

## 2. 实验A：性能基准

**方法**：`std::time::Instant` 只包住 `ctx.eval(...)`（Context 创建在计时外）；每次重复用**全新 Context**（排除编译缓存）；JS 代码按任务书原样使用。fib(27)/fib(30) 各跑 3 次，objchurn/jsonchurn 各 1 次未计时预热 + 5 次计时，**取中位数**。四个工作负载的返回值均通过期望值断言（fib30=832040，fib27=196418，objchurn=74850000，jsonchurn 长度 19343）。

**最终记录跑（`cargo run --release`，本机空闲时段，输出已存档）原始输出**：

```
A.fib(27)  3 次: [75.5, 73.0, 72.0] ms, 中位 73.0 ms, 结果=196418
A.fib(30)  3 次: [311.4, 320.1, 318.5] ms, 中位 318.5 ms, 结果=832040
A.objchurn 5 次(另1次未计时预热): [90.2, 85.3, 89.0, 91.5, 86.7] ms, 中位 89.0 ms, 结果=74850000
A.jsonchurn 5 次(另1次未计时预热): [391.0, 380.5, 380.5, 378.4, 385.5] ms, 中位 380.5 ms, 结果(=big.length)=19343
```

**跨 5 次完整运行的中位数波动**（同机，无人工限频；前两跑恰逢后台编译负载）：
fib(30)：589.6 / 454.1 / 304.1 / 308.4 / 318.5 ms；objchurn：197.1 / 134.6 / 84.7 / 87.4 / 89.0 ms；jsonchurn：408.8 / 548.2 / 376.2 / 380.2 / 380.5 ms。机器空闲时数字稳定（±3%），有后台负载时可劣化 ~2x——对比其他引擎时应取空闲时段数字。

**结论**：boa 是纯解释器（无 JIT），fib(30) 约 318ms。作为量级参照：这比 V8 JIT（~10ms 级）慢约 30 倍，与 QuickJS 解释执行同量级。objchurn（对象分配密集）89ms 与 jsonchurn（Rust 原生 JSON 内建）380ms 表现尚可。对 screeps 场景（每 tick 每玩家 20-500ms CPU 预算）意味着单线程吞吐偏低，但 boa 的角色定位本来就是"多进程并行的小沙箱"而非单线程极限性能。

## 3. 实验B：死循环硬中断

**方法**：boa 无时间式中断，用**等价机制** `loop_iteration_limit`：先以预算 10,000,000 试跑同形状的空体 `while(true){}`，按耗时线性换算出 ≈200ms 的迭代预算，再正式跑。每次跑都验证：宿主存活、错误类型、runtime 可复用性。

**原始输出（最终记录跑）**：

```
B.1 校准试跑: budget=10000000 iterations, elapsed=57.1 ms, err="RuntimeLimitError: reached the maximum number of iteration loops on this execution..."
B.2 迭代预算=35049390 -> 死循环在 203.3 ms 被终止, err="RuntimeLimitError: ..."
B.3 错误类型识别: as_engine()=Some("RuntimeLimitError: reached the maximum number of iteration loops on this execution") 是LoopIteration=true
B.4 中断后同一 Context 继续执行 6*7 => Ok("42") (可复用: true)
B.5 JS 侧 try/catch 拦截结果: Err("RuntimeLimitError: ...")   <- JS 拦不住，错误穿透 try/catch 交给宿主
```

（B.2 的 203.3ms 来自最终记录跑；五次完整运行分别终止于 200.2 / 194.4 / 195.9 / 199.3 / 203.3 ms，全部命中 200ms±5%。）

**结论**：
- 等价机制**可用**：校准后的迭代预算在 200ms±5% 处精确终止 `while(true){}`；宿主进程存活；错误可在 Rust 侧精确识别（`JsError::as_engine() == EngineError::RuntimeLimit(RuntimeLimitError::LoopIteration)`）；**同一 Context 中断后可继续执行新代码**（6*7=42）。
- 该错误**JS 侧 try/catch 拦不住**（B.5 的 Err 就是穿透 try/catch 后 eval 返回的），对沙箱是加分项：玩家代码无法吞掉终止信号。
- **局限（必须如实记录）**：① 不是墙钟中断，预算按"每毫秒循环回边数"校准，只对同形状循环精确；纯非循环指令风暴（如大字符串拼接、正则回溯 ReDoS，见 boa issue #4643 open）不受 `loop_iteration_limit` 约束；② 计数器每调用帧独立，一个帧跑满预算只杀该帧，恶意脚本可用多次函数调用分摊（但每次调用仍是有限预算，总量有界）；③ 无法从另一个线程触发（Context 非 Send）。官方时间式中断（issue #3442）至今未实现。
- 判定：**interrupt=pass（等价机制，非时间式）**。

## 4. 实验C：内存硬上限

**方法**：C.1 主进程实测无上限行为：全新 Context 执行 100 万次 `a.push({x:i})`，观察是否有任何引擎层拦截 + RSS 变化（sysinfo + 自写计数分配器双重测量）。C.2 补救路径实测：`#[global_allocator]` 计数分配器设 64MB 存活字节硬上限，超限先打印再 `std::process::abort()`；为不杀死主进程，该模式以子进程（`BOA_SPIKE_CHILD=memcap`）运行，父进程检查其 stdout/stderr 与退出信号。

**原始输出（最终记录跑）**：

```
C.1 扩张 1,000,000 个对象: 结果=Ok("1000000") 耗时=382ms 无任何引擎报错; RSS 29.6 -> 246.3 MB (GC后 254.9 MB); 分配器峰值存活 173 MB
C.2 分配器硬上限补救(子进程 64MB): 触发=true, 子进程退出[signal=6 (SIGABRT=6)], stderr: MEMCAP_EXCEEDED live_bytes=67108920 cap_bytes=67108864
    [child] MEMCAP_CHILD_START cap=67108864 live_start=298884
```

**结论**：
- **官方机制不存在**：100 万对象让 RSS 从 29.6MB 涨到 246.3MB（+216MB），全程零报错零拦截；`force_collect()` 后 RSS 不降（数组仍被 JS 变量持有，且 boa_gc 不向 OS 还页）。判定：**memory_limit=fail**。
- **补救路径实测可行**：全局分配器钩子在存活字节 67,108,920（超上限 56 字节）处立即触发，进程以 SIGABRT(signal 6) 终止——不会吃光系统内存。**代价**：① 粒度是整个进程（boa_gc 堆 thread_local 共享 + 非 GC 的宿主内存都算在内），无法只杀一个 runtime；② abort 不可恢复，需 supervisor 重启；③ 要与进程隔离配合（每 runtime 一进程）才是干净的方案。另一条路是改用 `fuzz` feature 的指令预算间接限制分配速率（未实测）。
- 开发过程翻车记录（如实）：第一版分配器在超限分支用 `eprintln!` 打印——`eprintln!` 自身会分配→递归重入 `alloc`→每层都超限→无限递归，子进程死于 **stack overflow** 而非预期 abort（stderr 原文：`thread 'main' has overflowed its stack / fatal runtime error: stack overflow, aborting`）。改为栈缓冲手写数字 + 裸 `stderr().write_all`（零分配）+ 重入保护后修复。第二版 udec 把数字写缓冲区尾部却拷贝头部，打印出空白；第三版修正。已固化在 `spikes/boa-spike/src/main.rs` 的 `report_cap_exceeded`。

## 5. 实验D：创建开销与常驻内存

**方法**：创建耗时取 20 次 `Context::default()` 的中位数（进程内首个为冷启动单列）；RSS 两组口径——主进程内（受前期实验堆残留影响）与**冷进程对照**（子进程分别持有 1/10 个 Context 自报 RSS，更干净）。

**原始输出（最终记录跑）**：

```
D.1 Context::default() 冷启动 814 us, 热态中位数 109 us (20 次, min 104 / max 242)
D.2 RSS: 基线 127 MB -> 持有1个 126 MB -> 持有10个 127 MB; 边际(10-1)=1 MB; (10-基线)=0 MB   <- 主进程, 共享GC堆+残留
D.4 冷进程子进程: [HOLD_RSS n=1 rss_mb=6.9 live_mb=0.3] / [HOLD_RSS n=10 rss_mb=10.0 live_mb=2.9]
D.4 冷进程边际(10个-1个)=3.1 MB
```

**结论**：创建一个 runtime 热态 **~109-121µs**、冷启动 ~0.8-1.0ms（含全部 ECMAScript 内建注册）——非常便宜。冷进程 1 个 Context 总 RSS 6.9MB，10 个共 10.0MB，**边际仅 ~3.1MB（≈0.35MB/个）**：因为 boa_gc 堆按线程共享，Context 本身只带 interner/realm 等轻量结构。推论：同线程多 Context **不省内存隔离**（共享堆意味着一个玩家把堆撑大，同线程所有玩家一起遭殃）；"每玩家独立内存域"只能靠每玩家一线程（thread_local 堆天然隔离）或每玩家一进程。

## 6. 实验E：安全速查

**方法**：全新默认 `Context` 内跑 typeof 探针 + 列出 `globalThis` 全部自有属性 + 实际调用尝试。

**原始输出（最终记录跑，节选）**：

```
E.1 typeof 探针: {"process":"undefined","require":"undefined","fetch":"undefined","XMLHttpRequest":"undefined",
     "WebSocket":"undefined","Node":"undefined","Buffer":"undefined","Java":"undefined","WebAssembly":"undefined",
     "setTimeout":"undefined","setInterval":"undefined","readFile":"undefined","load":"undefined",
     "quit":"undefined","exit":"undefined","globalThisProps":61}
E.2 globalThis 全部属性(61个): globalThis,Infinity,NaN,undefined,Function,Object,Math,JSON,Array,Proxy,ArrayBuffer,
   SharedArrayBuffer,BigInt,Boolean,Date,DataView,Map,isFinite,isNaN,parseInt,parseFloat,Number,eval,Set,String,RegExp,
   TypedArray,Int8Array,...(全部 TypedArray)...,Symbol,Error,RangeError,ReferenceError,TypeError,SyntaxError,EvalError,
   URIError,AggregateError,Reflect,Promise,encodeURI,encodeURIComponent,decodeURI,decodeURIComponent,WeakRef,WeakMap,
   WeakSet,Iterator,Atomics,FinalizationRegistry,Temporal
E.3 实际调用 process.exit => "blocked:ReferenceError"; require('fs') => "blocked:ReferenceError"
```

**结论**：默认 `Context::default()` 是**纯 ECMAScript 环境**：61 个全局属性全部是语言内建（含 Temporal），`process/require/fetch/Buffer/Timer/WebAssembly` 全部 undefined，`process.exit()`/`require('fs')` 实际调用均为 ReferenceError。文件系统/网络/进程能力默认**完全不存在**，比"有但砍权限"的引擎少一层供给面。注意：`Atomics`/`SharedArrayBuffer` 存在（引擎内共享内存语义），但无宿主 IO 通道时风险有限；真实部署仍需按白名单注册游戏 API（screeps 模式）。

## 7. 汇总行（程序自动生成，最终记录跑原文）

```
RESULT engine=boa_engine fib30_ms=318.5 fib27_ms=73.0 objchurn_ms=89.0 json_ms=380.5 create_runtime_us=109 rss_10_mb=3.1
CAPABILITY interrupt=pass memory_limit=fail detail="loop_iteration_limit(等价机制,非时间式,校准预算=35049390) 实测终止于203ms; 官方时间式中断不存在(issue #3442 open, PR #5475/#5380 关闭未合并); 中断后同Context可继续执行=true; JS try/catch=Err(...); 内存: 官方无内存上限API(0.22与main均无); 实测1e6对象RSS增216MB无任何拦截; 补救路径实测: 全局分配器钩子64MB硬上限可强制终止进程(abort, 子进程signal验证), 但粒度为整进程且不可恢复, 需进程隔离配合; 安全: 默认Context无process/require/fetch/Buffer/Timer等宿主对象(globalThis仅ECMAScript内建,61个属性); process.exit/require尝试均为ReferenceError"
```

## 8. 未尽事项 / 未实测项

1. **`fuzz` feature 的 `instructions_remaining` 指令预算未实测运行**（仅源码查证，`vm/mod.rs:781-790`）。它是比 loop_iteration_limit 更鲁棒的指令级预算，但非默认 feature、非时间式、且预算只能在构建时设定。
2. **非循环指令风暴未测**：大字符串拼接 / 深正则回溯（boa issue #4643「RegExp execution lacks backtracking limit, allowing ReDoS」open）不受 loop_iteration_limit 约束——若选 boa，DoS 防护必须靠进程级（watchdog 杀进程）而非引擎级。
3. **异步/微任务风暴未测**：`ctx.run_jobs()` 的 job 队列洪泛是否受 RuntimeLimits 约束未验证。
4. **性能未做多机/多轮统计**：仅同机 4 次完整运行，空闲时段数字稳定（±3%），后台负载时可劣化 ~2x。
5. `rss_10_mb` 用冷进程对照口径（3.0-3.3MB）；主进程口径因共享 GC 堆与实验残留显示 ~0MB，已同时打印两组供交叉核对。
6. 与 rquickjs / deno_core 的同机对比数字由同 workflow 的其他 spike 提供，本笔记不代测。

## 9. 一句话结论

boa_engine 是五项里"安全面最干净、创建最便宜（109µs / 边际 0.35MB）"的选项，性能是解释器水平（fib30 ≈ 318ms，比 JIT 引擎慢一个数量级）；**硬中断只能靠计数式 loop_iteration_limit 校准模拟（可用、可恢复、JS 拦不住，但对非循环 DoS 无效），内存硬上限官方完全没有（fail），补救只有全局分配器 abort 钩子 + 进程隔离这条路**——若内存隔离是硬需求，boa 的 thread_local 共享 GC 堆意味着必须"每玩家一进程（或至少每玩家一线程）"来切分。
