# deno_core（rusty_v8）能力实测笔记

- 日期：2026-09-27
- 后续篇：[spike-deno-game-api.md](./spike-deno-game-api.md)（实验 F~K：Screeps 游戏 API 形态的高风险交互模式——惰性 overlay / roomCallback 零拷贝 / 2MB RawMemory / CommonJS 垫片 / 游戏 runtime snapshot / console 与堆栈，6/6 PASS）
- 后续篇：[spike-deno-inspector.md](./spike-deno-inspector.md)（实验 M~Q：V8 inspector + deno_inspector_server 断点调试全链路 / 暂停×看门狗 / 全局面 diff / 快照可调试 / 多 isolate 多 target，5/5 PASS；关闭本篇 §11 的 inspector 未测项）
- 实验对象：`deno_core` 0.412.0（V8 绑定 `deno_v8` 0.4.0 → `v8` crate 150.4.0，即 rusty_v8；引擎版本串 `15.0.245.2-rusty`，来自 `v8::V8::get_version()` 运行时输出）
- 代码位置：`spikes/deno-spike/`（独立 crate，不在 workspace 内）
- 复现命令（在本目录 `spikes/deno-spike/` 下）：

```bash
cargo run --release
# 阴性对照（验证"不注册回调时会怎样"，会故意 abort 进程，属预期）：
cargo run --release --example negctl
```

## 0. 精确依赖版本（Cargo.lock 实测锁定）

| 包 | 版本 | 说明 |
|---|---|---|
| deno_core | 0.412.0 | crates.io 最新稳定版（2026-09-16 发布） |
| deno_v8 | 0.4.0 | deno_core 0.412 的 v8 绑定（re-export rusty_v8） |
| v8（rusty_v8） | 150.4.0 | 预编译静态库，构建期从 dl.deno.land 下载 |
| serde_v8 | 0.321.0 | |
| deno_ops | 0.288.0 | `#[op2]` 过程宏 |
| deno_error | 0.7.1 | |
| sysinfo | 0.39.6 | RSS 测量 |
| tokio | 1.53.1 | deno_core 的 GC/延迟任务需要 runtime context |
| rustc / cargo | 1.98.1 (48a229cea 2026-09-01) | macOS 15 (darwin 24.6.0, arm64) |

二进制启动时自报版本行（build.rs 解析 Cargo.lock 注入编译期）：
`deno-spike | deno_core=0.412.0 deno_v8(v8)=150.4.0 sysinfo=0.39.6 deno_error=0.7.1 tokio=1.53.1`

## 1. 实验A 性能基准 —— PASS

方法：JS 工作负载代码**原样**取自任务描述；先执行一次预热（让 V8 JIT 完成优化），再各测 5 次取中位数。计时用 Rust `std::time::Instant` 包住 `execute_script`（含一次 Rust↔JS 边界，微秒级，相对毫秒级负载可忽略）。`fib(30)` 无需降级到 `fib(27)`——V8 下仅 ~5ms。

最终一次运行的原始输出（`cargo run --release`，2026-09-27）：

```
校验: fib(30)=832040 objchurn=74850000 jsonchurn=19343
fib(30)   5次=[5.35, 4.96, 4.74, 4.63, 4.57] ms -> 中位 4.74
objchurn  5次=[0.41, 0.90, 0.89, 0.92, 0.33] ms -> 中位 0.89
jsonchurn 5次=[6.49, 6.37, 6.34, 6.49, 6.22] ms -> 中位 6.37
```

多次运行汇总（同机 4 次完整运行）：fib30 中位 4.3–5.3 ms；objchurn 0.4–0.9 ms（受 GC 时机影响抖动明显）；jsonchurn 6.4–7.9 ms。

正确性验证：`fib(30)=832040` ✓；`objchurn=200×Σ(3j, j=0..499)=74,850,000` ✓；`jsonchurn=19343`（字符串长度，随引擎实现稳定）✓。

## 2. 实验B 硬中断 —— PASS（本次验证的最高风险项，两条硬骨头之一）

**调研结论（代码证据）**：
- rusty_v8 提供 `Isolate::thread_safe_handle() -> IsolateHandle`，其内部 `unsafe impl Send/Sync`（`v8-150.4.0/src/isolate.rs:2165-2175`），`IsolateHandle::terminate_execution()` 文档明确写着 *"This method can be used by any thread even if that thread has not acquired the V8 lock"*（isolate.rs:2259 附近）。这就是官方支持的跨线程终止路径——**不需要**把 isolate 搬到独立 OS 线程跑。
- 与 Deno Subhosting 的"watchdog"模型一致：deno.com 博客《How we built a secure, performant, multi-tenant cloud platform》原文 *"we've built other mechanisms, such as a 'watch dog', to monitor resources and terminate suspicious tenants"*（https://deno.com/blog/build-secure-performant-cloud-platform）。

**实验实现**：主线程 `execute_script("while (true) {}")` 阻塞执行；看门狗线程 sleep 200ms 后调用 `handle.terminate_execution()`。

原始输出：

```
execute_script 用时 201.2 ms 后返回（看门狗 terminate 返回值=true），宿主线程存活
捕获中断错误: "Uncaught Error: execution terminated"
中断后同一 runtime 继续执行新代码: 结果="42function"（清标志前 is_execution_terminating=false）
```

结论：
1. ~200ms 处被强制终止，误差 <10ms；宿主线程/进程存活；
2. `execute_script` 返回 `Err`，`exception_message == "Uncaught Error: execution terminated"`（该错误 JS 侧 `try/catch` 不可捕获，是"硬"终止）；
3. **同一 runtime 可复用**：deno_core 在错误转换时已自动 `cancel_terminate_execution()`（deno_core-0.412.0/src/error.rs:1664，实测 `is_execution_terminating()==false`），随后执行新代码 `(6*7)+(typeof Array)` 得到 `"42function"` ✓；保险起见我又显式调了一次 `cancel_terminate_execution()`（幂等无害）。

## 3. 实验C 内存硬上限（64MB）—— PASS（硬骨头之二）

**调研结论（代码证据）**：
- `RuntimeOptions::create_params: Option<v8::CreateParams>`（deno_core-0.412.0/src/runtime/jsruntime.rs:602）→ `v8::CreateParams::heap_limits(initial, max)` 支持 per-isolate 堆上限。
- **关键陷阱**：rusty_v8 文档原文（`v8-150.4.0/src/isolate_create_params.rs:131-133`）：*"If the garbage collections do not help and the callback does not increase the limit, then V8 will crash with V8::FatalProcessOutOfMemory."* —— 只设 `heap_limits` 不注册 `near_heap_limit` 回调，超限时是**进程 abort**，不是优雅错误。
- 正确姿势在 deno_core 自己的测试里（deno_core-0.412.0/src/runtime/tests/misc.rs:565 起 `test_heap_limit_cb`）：回调内 `cb_handle.terminate_execution()` 并返回 `current_limit * 2`（让当次分配完成、随即因终止标志展开）。deno_core 还提供了闭包版封装 `JsRuntime::add_near_heap_limit_callback`（jsruntime.rs:2176，文档注释明言 *"Use this to prevent V8 from crashing the process when reaching the limit"*）。

**实验实现**：因最坏路径是 abort，把实验放进子进程跑（父进程 60s 超时保护 + kill），JS 为无限 `a.push(new Array(10000).fill(1.5))`。

原始输出（父进程转发子进程输出）：

```
[child] MEMTEST_BEGIN heap_limit_bytes=67108864
[child] MEMTEST_RESULT terminated=true elapsed_ms=17.3 peak_rss_mb=71.7 cb_fired=1 heap_used_mb=53.2 heap_limit_mb=122.0 err="Uncaught Error: execution terminated"
```

结论：
1. 上限生效：near-heap 回调触发 1 次（`cb_fired=1`），执行 17–35ms 即被强制终止（多次运行 17.1/17.3/17.8/35.3ms）；
2. 内存被钉死在 ~72–74MB 峰值 RSS（64MB 上限 + V8 外部开销），没有吃系统内存；
3. 终止方式与实验B 相同（`execution terminated`），子进程正常 exit 0 —— 优雅可恢复。

**阴性对照**（`cargo run --release --example negctl`，只设 `heap_limits` 不注册回调）：

```
# Fatal JavaScript out of memory: Reached heap limit
→ v8::internal::V8::FatalProcessOutOfMemory → 进程 abort（exit 133，SIGTRAP）
/usr/bin/time -l 实测 maximum resident set size = 77,660,160 B（≈74MB）
```

即：即使走最坏路径，内存仍被限制在 ~77MB（不会吃光系统内存），但进程会直接死。**结论：`heap_limits` + `add_near_heap_limit_callback(终止)` 是生产必选组合。**

## 4. 实验D 创建开销与常驻内存 —— PASS

方法：首次 `JsRuntime::new` 含 V8 平台一次性初始化；随后建 20 次（每次建完执行一句小脚本触碰页面）取中位。RSS 用 sysinfo 0.39.6 读本进程（`ProcessRefreshKind::nothing().with_memory()`），先 drop 掉此前所有 runtime 测基线，再分别持 1 / 10 个 runtime（每个建后执行含 `new Array(8192)` 的小脚本）。

原始输出（最终运行）：

```
首次 JsRuntime::new（含 V8 平台初始化）: 2927 us
后续 20 次 create+小脚本: 中位 2086 us, min 1971, max 2667
基线 RSS: 35.7 MB
持 1 个 runtime:  RSS Δ = 0.9 MB
持 10 个 runtime: RSS Δ = 20.4 MB；第 2..10 个平均边际 2.2 MB/个
按 LIFO 逆序销毁 10 个 runtime ...
全部销毁后 RSS 回落至 Δ=18.8 MB
```

结论：
- 创建一个可用 runtime ≈ **2ms**（不含首建的平台初始化 ~3ms，一次性的）；
- 边际内存 ≈ **2 MB/runtime**（10 个共 ~20MB）；持有 1 个时 Δ 仅 ~1MB，第 2 个起每个 ~2MB（V8 isolate 的堆页+代码区按需增长）；
- 销毁后 RSS 不完全回落（18.8/20.4）——V8/分配器保留页面，进程级驻留，"每 tick 新建 isolate" 的模式会稳定在这个水位而非无限增长（20 次建删循环后基线仍 35.7MB 可证）。

## 5. 实验E 安全速查 —— PASS（默认无宿主能力，但 `Deno.core` 裸露）

方法：默认 `RuntimeOptions`（不加任何扩展）建 runtime，JS 侧探测。

原始输出（节选）：

```
process = "undefined"      require = "undefined"     fetch = "undefined"
XMLHttpRequest = "undefined"   WebSocket = "undefined"
Deno = "object"（只有 Deno.core 内部对象）
WebAssembly = "object"    SharedArrayBuffer = "function"
globalThis 属性 = AggregateError,Array,...,WebAssembly,__bootstrap,console,...（纯 ECMAScript 内建 + console + __bootstrap + Deno）
DenoCoreOps = op_abort_wasm_streaming,op_add,op_add_async,...（约 100 个内部 op：op_print/op_encode/op_read/op_write/op_serialize/op_timer_now/op_eval_context/op_panic 等）
```

结论：
- **默认完全碰不到** `process` / `require` / `fetch` / `XMLHttpRequest` / `WebSocket` / 文件系统（没有任何 `readFile`/fs 形态的 API，也没 `setTimeout` —— 定时器只有内部 op_timer_*，未挂到全局）；
- 需要注意的攻击面：
  1. `Deno.core.ops.*` 裸露约 100 个内部 op（如 `op_print` 可直接写宿主 stdout，`op_eval_context`、`op_serialize/op_deserialize`、`op_panic` 等）。`op_read/op_write` 面向的是资源表（resource table），默认表为空所以读不到真实 fd，但**这些内部 op 未做任何鉴权**，生产上应在玩家 context 里删掉/遮蔽 `Deno.core`（或 `skip_op_registration` 后只注册白名单 op）；
  2. `WebAssembly`、`SharedArrayBuffer`、`Atomics` 默认可用（CPU 型攻击面，靠实验B/C 的中断+内存上限兜底）；
  3. `__bootstrap` 挂在全局（deno_core 内部实现细节，可 `delete globalThis.__bootstrap`）。

## 6. 附加实验1：startup snapshot —— 4.4–5.3x 启动加速，预热状态完整恢复

方法：build.rs 调 `deno_core::snapshot::create_snapshot`（0.412 从 crate 根挪到了 `snapshot` 模块，0.411 是根路径，踩坑见 §8），`with_runtime_cb` 里预热全局 `fib`/`SNAPSHOT_PRELOADED=true`/正则对象；产物 722,016 字节 include 进二进制。运行时各建 10 次取中位。

原始输出：

```
无 snapshot: 10 次中位 3356 us；有 snapshot: 762 us（加速 4.40x）
snapshot 恢复验证: {"preloaded":true,"fib20":6765,"regex":true}
```

（另一轮运行为 4815 vs 917 us，加速 5.25x。）

结论：**snapshot 把 runtime 创建从 ~3.4ms 压到 ~0.8ms**，且预热进 snapshot 的全局函数/常量/正则直接可用（`fib(20)=6765` ✓）。注意：JIT 机器码不进 snapshot（那是 V8 code cache 另一机制），加速来自免解析/免执行 bootstrap JS。对"每 tick 一个玩家 isolate"的服务端，这是把 D 实验的 2ms 进一步压到 <1ms 的正道。

## 7. 附加实验2：Extension 同步 op（Rust 函数暴露给 JS）—— PASS，fast op 低至 ~5ns/call

原始输出：

```
正确性: {"add":42,"sum":15}
op_spike_add(u32,u32) 1e6 次连续调用: 求和=500000500000.0, 平均 5 ns/call
op_spike_sum(serde Vec<f64>) 1e5 次连续调用: 求和=400000, 平均 77 ns/call
```

结论：`#[op2(fast)]`（原始类型参数）≈ 5–6 ns/call——与原生 JS 函数调用同量级，宿主函数注入零负担；带 `#[serde]` 参数的慢路径 ≈ 77–136 ns/call。op2 宏会**强制** fast 兼容的 op 标注 `(fast)`，否则编译报错（见 §8）。

## 8. 踩坑记录（都是实测撞出来的，含报错原文）

1. **多 isolate 必须 LIFO 逆序销毁**（重大架构约束）。正序 drop 两个及以上 runtime 时进程直接 abort：
   ```
   # Fatal error in v8::HandleScope::CreateHandle()
   # Cannot create a handle without a HandleScope
   ```
   用裸 rusty_v8（不经 deno_core）复现时 rust 层 panic 给出了原因（`v8-150.4.0/src/isolate.rs:2350`）：
   ```
   v8::OwnedIsolate instances must be dropped in the reverse order of creation.
   They are entered upon creation and exited upon being dropped.
   ```
   即 isolate 创建即 `Enter`、drop 即 `Exit`——是个栈。**对 xscreeps 的影响**：每个玩家常驻 isolate 的模式里，"踢掉中间某个玩家"不能直接 drop 其 isolate，必须按 LIFO 顺序销毁（或改为每 tick 顺序创建/销毁、或玩家退出时整体重建剩余 isolate）。实验D 已按 LIFO 销毁验证安全（逆序 drop 10 个 + 再建新 runtime 全部 OK）。
2. **自定义 op 不能与内置 op 撞名**。我把 op 命名为 `op_add`，而 deno_core 默认 op 集里已有 `op_add`（见实验E 的 DenoCoreOps 列表），于是 `ext:core/ops` 合成模块出现重复导出，`JsRuntime::new` 直接 V8 CHECK 失败 abort：
   ```
   # Fatal error
   # Check failed: IsTheHole(exports->Lookup(name)).
   → v8::internal::SyntheticModule::PrepareInstantiate ... JsRuntime::new_inner
   ```
   修复：改名为 `op_spike_add/op_spike_sum`。
3. **0.412 API 位置变化**：`create_snapshot`/`CreateSnapshotOptions` 从 crate 根移到 `deno_core::snapshot::*`（0.411 在根）。
4. **fast op 强制标注**：`#[op2]` + 纯数值参数/返回值会编译报错 *"This op is fast-compatible and should be marked as (fast)"*，须写 `#[op2(fast)]`。
5. **GC 压力场景需要 tokio runtime context**：deno_core 自身测试注释明言（misc.rs:560 附近）"The GC pressure ... makes V8 post delayed foreground tasks (memory reducer), which require a tokio runtime context"；setup.rs 的 `spawn_delayed_task` 在无 tokio handle 时会 `std::process::abort()`。 spike 的 main 里先 `tokio::runtime::Builder::new_current_thread().enable_all().build()` + `enter()` 再建任何 JsRuntime。
6. sysinfo 0.39 的 API：`refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_memory())`，`process(pid)`（按值），`Process::memory()` 返回字节 RSS。

## 9. 最终汇总行（`cargo run --release` 原样输出）

```
RESULT engine=deno_core(rusty_v8) fib30_ms=5.3 objchurn_ms=0.9 json_ms=6.6 create_runtime_us=2086 rss_10_mb=20.4
CAPABILITY interrupt=pass memory_limit=pass detail="interrupt: 202.1ms 终止, err='Uncaught Error: execution terminated', runtime复用=true; memory: child_exit=Some(0); MEMTEST_RESULT terminated=true elapsed_ms=17.3 peak_rss_mb=71.7 cb_fired=1 heap_used_mb=53.2 heap_limit_mb=122.0 err="Uncaught Error: execution terminated""
EXTRA first_create_us=2926 rss_1_mb=0.9 per_runtime_mb=2.2 interrupt_reuse=true
NOTE fib_n=30 方法=预热1次后5次取中位(Rust Instant 含边界开销)
```

（各次运行数字有正常抖动：fib30 4.3–5.3ms / objchurn 0.4–0.9ms / json 6.4–7.9ms / create 2.0–2.6ms / rss_10 19.4–20.7MB；`heap_limit_mb=122.0` 是回调把 64MB 上限翻倍后的值，属预期。）

## 10. 结论与对 xscreeps 架构的建议

1. **两块硬骨头都啃下来了**：超时硬中断（跨线程 `IsolateHandle::terminate_execution`，~200ms 精准终止、错误可捕获、runtime 可复用）与内存硬上限（`heap_limits(0, 64MB)` + near-heap 回调内终止，~72MB 封顶、优雅终止）在 deno_core 上**开箱可用且有官方测试背书**，不需要魔改。
2. 性能是三引擎候选里最乐观的：fib30 ~5ms、op 调用 5ns，JSON/对象分配毫秒级——跑 Screeps AI 绰绰有余。
3. snapshot 把 isolate 创建压到 <1ms，配合 ~2MB/个 的边际内存，"每玩家一个 isolate"和"每 tick 一个 isolate"两种架构都可行。
4. 默认沙箱干净（无 process/require/fetch/fs），但 `Deno.core` 内部 op 裸露，生产需遮蔽或用 `skip_op_registration` + 白名单扩展重建。
5. 最大架构约束是 **isolate 的 LIFO 销毁顺序**（§8.1），设计玩家生命周期管理时必须遵守；备选是"每 tick 顺序化创建/销毁"（实验证明顺序建删 20 次无泄漏无崩溃）。

## 11. 未尽事项（n/a 与待办）

- **未测**：把 JsRuntime 放进独立 OS 线程逐玩家运行（`JsRuntime` 内含 `Rc`，非 `Send`，需线程绑定 + 消息通信，或 v8 `Locker` 抢占模型）。本 spike 的中断验证走的是"单线程宿主 + 看门狗线程 terminate"，已满足任务要求；多线程宿主模型留到原型阶段验证。
- **未测**：`SharedArrayBufferStore` / 跨 isolate 传输；V8 code cache（与 snapshot 互补的另一种缓存）。（`inspector` 调试协议已测——后续篇 [spike-deno-inspector.md](./spike-deno-inspector.md) 实验 M~Q 五项全 PASS。）
- objchurn 单次仅 ~0.4–0.9ms，如需更细的数字应加重复轮数（本笔记如实报告 5 次中位及原始 5 个样本）。
- RSS 基线（35.7MB）是"跑过实验A/B 之后"的进程内基线（受 V8 平台驻留影响），边际增量（1→10 个 runtime）才是有效数字；未另起纯净进程测绝对基线。
- negctl 阴性对照示例（`spikes/deno-spike/examples/negctl.rs`）故意 abort 进程，仅手动运行，不参与主输出。
