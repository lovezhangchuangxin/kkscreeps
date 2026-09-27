# 沙箱方案调研：deno_core + rusty_v8（V8）

> 调研日期：2026-09-27。调研员：deno_core 方案。
> 方法：官方文档 + crates.io/GitHub 源码（本地 clone 核对 file:line）+ **本地实测**（macOS arm64，Rust 1.98.1，deno_core 0.412.0 / rusty_v8 150.4.0 / V8 15.0.x，debug 构建）。
> 置信级别标注：【文档声称】= 官方文档/README；【生产验证】= 有生产环境在用且有公开证据；【社区经验】= 博客/issue；【实测】= 本次调研在本机运行验证。

---

## 0. 结论速览

**总判定：可行（有少量需组合方案补齐的点）**。V8 引擎本身（性能/语法/GC/终止/内存上限/SAB 共享）全部满足；CPU 计时与硬超时用"每 isolate 独占线程 + 线程 CPU 记账"方案（Supabase 生产同款）；CommonJS 玩家代码需 JS 垫片（已实测可行）；安全隔离需接受"同进程内隔离不是硬安全边界"，按 Deno 生产经验做进程分片加固。

| # | 硬要求 | 判定 | 关键证据 |
|---|--------|------|----------|
| 1 | CPU 时间计量 | ✅ 满足（线程记账方案） | 实测：JS 内 op 读线程 CPU 时间，ns 精度；Supabase 生产同款 |
| 2 | 硬超时终止 | ✅ 满足 | 实测：跨线程 terminate_execution 305ms 打断死循环，宿主存活，runtime 可复用 |
| 3 | 内存硬上限 | ✅ 满足（需组合方案） | 实测：heap_limits + near_heap_limit_callback 干净终止，兄弟 isolate 无恙；仅 heap_limits 会 FatalOOM 崩溃（实测发现） |
| 4 | 按用户持久复用 | ✅ 满足 | JsRuntime 常驻实测 10 个并存；create_snapshot 快照 API；Supabase per_worker 策略 |
| 5 | 多模块玩家代码 | ✅ 满足（CJS 需垫片，已实测） | deno_core ESM-only；实测 require() 垫片 + ESM 相对导入均通过 |
| 6 | 全局注入 | ✅ 满足 | extension/ops 体系 + PinScope 直接注入对象（实测注入 SAB 到 globalThis） |
| 7 | 共享只读大缓冲 | ✅ 满足 | 实测：SAB BackingStore 跨 2 个 isolate 零拷贝共享，单份内存 |
| 8 | 语法兼容 ES2020+ | ✅ 满足 | 实测 ES2022 全套（可选链/BigInt/私有字段/命名捕获组）；微任务 run_event_loop 排空实测 |
| 9 | 执行性能 | ✅ 满足 | V8 JIT 本尊（Node/Chrome 同引擎）；fast ops（V8 Fast API）降 op 开销 |
| 10 | 安全隔离 | ⚠️ 有条件满足 | 能力模型默认零授权；同进程隔离非硬边界【文档声称+生产实践】；需 V8 sandbox feature + 进程分片 |

---

## 1. 版本与 2026 年活跃度【文档声称+实测】

| 组件 | 最新版 | 发布日期 | 节奏 |
|------|--------|----------|------|
| deno_core | **0.412.0** | 2026-09-16 | 每月 2–4 次（crates.io versions API：0.411→08-27、0.410→08-06、0.409→07-23…） |
| rusty_v8（crate 名 `v8`） | **152.2.0** | 2026-08-20 | 跟 Chrome major，每 4 周一个新 major + 旧线 patch（README "Versioning"） |
| rusty_v8 仓库 HEAD | 15.2.124.1 (V8) | 2026-09-25（`git log` 本地 clone） | 极活跃 |

- 来源：`curl https://crates.io/api/v1/crates/deno_core` 与 `/crates/v8`（2026-09-27 执行）。
- deno_core 累计下载 809 万，近 90 天 170 万；rusty_v8 累计 2850 万。
- **仓库迁移**：deno_core 独立仓库已于 **2026-02-27 归档**，合并进 [denoland/deno 单仓库](https://github.com/denoland/deno) `libs/core`（README 只剩一句话指向新家；API `repos/denoland/deno_core` 返回 `archived: true`）。crate 发布未中断。
- **新架构（重要）**：deno 单仓库新增 **`libs/deno_v8` 门面 crate（0.4.0）**，`deno_core 0.412.0` 已经由它间接依赖 rusty_v8；该门面设计了**双后端**：`v8 = rusty_v8`（V8）或 `quickjs = v8x_backend`（实验性 QuickJS），feature 面一一镜像（`libs/deno_v8/Cargo.toml`：`rusty_v8 = { package = "v8", version = "150.4.0", optional = true }`、`v8x_backend = { package = "v8x", version = "=149.4.0-rc.4", optional = true }`）。
- **版本锁定现实**：`deno_v8` 要求 `v8 ^150.4.0`（≥150.4.0 且 <151），deno 单仓库 `Cargo.lock` 实际解析 **150.4.0（V8 15.0.x）**——比 rusty_v8 最新 152.2.0 落后约 2 个 Chrome major（约 8 周）。Deno 自身 changelog：`2.9.4 / 2026.07.23 — feat: upgrade V8 to 150.2.0 (#36098)`（Releases.md:235）。
- rusty_v8 版本规则（README 原文）："Rusty V8's major version aligns with Chrome's major version… a new major version every 4 weeks"，即 152.x ↔ Chrome 152 ↔ V8 15.2。

---

## 2. 十项硬要求逐条详证

### 2.1 CPU 时间计量 ✅（方案：线程 CPU 记账）

- **V8/rusty_v8 没有 isolate 级 CPU 时间 API**【实测核对】：rusty_v8 全源码 grep 仅 `collect_cpu_profiler_sample`（`src/isolate.rs:1529`，CPU Profiler 采样，非计时）。
- **正确方案：每个 isolate 独占一个线程，用 OS 线程 CPU 时间记账**：
  - Linux：`clock_gettime(CLOCK_THREAD_CPUTIME_ID)` 读数 + `timer_create(CLOCK_THREAD_CPUTIME_ID, SIGEV_SIGNAL)` 做 CPU 时间硬闹钟。
  - **生产范本（【生产验证】）**：Supabase edge-runtime（deno_core 构建、跑全球生产 Edge Functions）有专门的 `crates/cpu_timer`：
    - `get_thread_time()` = `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` 返回 ns（`crates/cpu_timer/src/lib.rs:196`）；
    - `CPUTimer::new()` = `timer_create(CLOCK_THREAD_CPUTIME_ID, SIGEV_SIGNAL→SIGALRM)` 每 isolate 一个（`lib.rs:96-112`）；
    - `reset(initial_expiry_ms, interval_ms)` 武装定时器（`lib.rs:152-176`）；SIGALRM → 监督任务 → 关停 worker（`crates/base/src/worker/supervisor/strategy_per_request.rs:104-160`，`ShutdownReason::CPUTime`）。
  - **macOS 也支持**【实测】：`clock_gettime(CLOCK_THREAD_CPUTIME_ID)` 在 macOS 14/15 可用（本机 C 程序实测输出 `0.002064958 s`）。
- **Game.cpu.getUsed() 语义实现**：tick 开始时记线程 CPU 基线；`Game.cpu.getUsed()` 实现为注册的 op，返回 `get_thread_time() - baseline`。
- 【实测】在 deno_core 0.412.0 中注册 `#[op2(fast)] fn op_thread_cpu_ns() -> f64`，JS 内循环 5e6 次 sqrt 前后各读一次：
  ```
  [js] Game.cpu.getUsed()-style reading: 25.524166 ms CPU (ns=25524166), x>0=true
  ```
  精度：ns 级（远超 Screeps 需要的 ms 精度）。另一次运行 13.88ms / 8.72ms，读数稳定可区分。
- 注意：op 在 isolate 线程上执行，读"当前线程"CPU 时间天然就是该 isolate 的 CPU 时间（前提：该线程只跑这一个 isolate——Supabase 模式，也是 V8 isolate 的常规用法）。

### 2.2 硬超时终止 ✅

- rusty_v8 `IsolateHandle::terminate_execution()` 文档原文（`src/isolate.rs:2685`）："**This method can be used by any thread even if that thread has not acquired the V8 lock**"。配对 `cancel_terminate_execution()` / `is_execution_terminating()`（isolate.rs:1120-1139、2685+）。`Isolate::thread_safe_handle()`（isolate.rs:1109）。
- 【实测】（`/tmp/sb-research/denocore-bench`，deno_core 0.412.0）：
  ```
  [terminate] other thread calling terminate_execution()...
  [terminate] terminate_execution() returned true
  [terminate] infinite loop returned after 305.301291ms, is_err=true   ← 300ms 闹钟后立即打断
  [terminate] runtime reusable after cancel_terminate: true            ← 同一 runtime 复用成功，执行 6*7=42
  ```
  宿主进程存活；`execute_script` 返回 `Err`（"Uncaught Error: execution terminated"）；`cancel_terminate_execution()` 后 **同一 runtime 可继续复用**（超时玩家不必销毁 isolate，亦可选择销毁重建）。
- 生产佐证【生产验证】：Supabase edge-runtime `strategy_per_worker.rs:332-344`（CPU 超限）与 `runtime/mod.rs:1817`（`v8_isolate().terminate_execution()`）。
- 与墙钟超时配合：tokio `sleep` 分支监督（strategy_per_request.rs:186-196 的 `wall_clock_duration_alert`），CPU 闹钟与墙钟双保险。

### 2.3 内存硬上限 ✅（必须组合方案，单用 heap_limits 会崩进程）

三件套（前两件 rusty_v8 原生，第三件 Supabase 加码）：

1. **`CreateParams::heap_limits(initial, max)`**（`rusty_v8 src/isolate_create_params.rs:137`）——V8 老生代硬顶。
2. **`add_near_heap_limit_callback`**（`src/isolate.rs:1963`；类型签名 isolate.rs:579）——接近上限时回调（isolate 线程、GC 内），在此 `terminate_execution()` 并**临时抬高返回值**，让 V8 以"execution terminated"干净展开而不是 FatalOOM。
3. （Supabase 生产做法）**自定义 ArrayBuffer 分配器** 给堆外内存也设上限：`CustomAllocator`（`ext/runtime/external_memory.rs:11`，计数超限返回 null → JS 得到可捕获的 RangeError/分配失败，并 wake 监督者）+ **GC prologue 回调检查** used_heap+external_memory（`crates/base/src/runtime/mod.rs:178-210` `MemCheck::check`、`mod.rs:2305-2320` 回调注册）。

【实测】关键对照（本机，32MB 上限，故意无界分配）：

- **只设 heap_limits，无回调**：
  ```
  # Fatal JavaScript out of memory: Reached heap limit
  ==== C stack trace ==== ... FatalProcessOutOfMemory ...
  exit=133   ← 整个进程崩溃（兄弟 isolate 一起死）
  ```
  这是本次调研踩到的**最重要坑**：V8 到达硬上限的默认路径是 FatalOOM（CrashTraversal），不会抛出可捕获的 RangeError。
- **heap_limits + near_heap_limit_callback（回调里 terminate + 临时 +64MB）**：
  ```
  [js] sibling isolate alive, round 0
  [js] limited isolate allocating until 32MB heap limit...
  [heaplimit] near-heap-limit callback fired: true; execute_script is_err: true
  [heaplimit] error surfaced to host: Uncaught Error: execution terminated
  [heaplimit] limited isolate dropped, host still alive
  [js] sibling isolate alive, round 1
  [js] sibling isolate alive, round 2
  [heaplimit] sibling isolate finished all rounds unharmed
  exit=0
  ```
  超限 isolate 被强制终止且可正常 drop，**另一线程上的兄弟 isolate 三轮探测全部无恙**。✅ 完全符合"超限强制终止且不影响其他 runtime"。

生产数字【生产验证】：Supabase 托管 Edge Functions 每用户 worker **256MB 内存上限、每请求 2s CPU 时间**（官方文档 https://supabase.com/docs/guides/functions/limits，2026-09 抓取："Maximum Memory: 256MB / Maximum CPU Time: 2s"）。Deno Deploy 每 application 512MB（docs pricing_and_limits）。

### 2.4 按用户持久复用 + snapshot ✅

- **持久复用**：`JsRuntime`（含 V8 isolate + context + 全局状态）是普通 Rust 对象，按 user_id 缓存在 HashMap/池即可跨 tick 复用。【实测】同进程创建 10 个 JsRuntime 并存（见 §3 内存数字）；terminate + cancel 后同一 runtime 继续跑（§2.2）。代码变更才重建：对 isolate 做代码模块懒加载（StaticModuleLoader 换新 map / 重新 mod_evaluate），或直接丢弃重建（空 isolate 约 3MB、创建耗时可忽略，见 §3）。
- **Snapshot**【文档声称+API 核对】：
  - deno_core `create_snapshot(CreateSnapshotOptions, warmup_script)`（`libs/core/runtime/snapshot.rs:186`、`CreateSnapshotOptions` :119）——build.rs 期生成启动快照：把 Game 常量、lodash、全局脚手架全部预编译进快照，`RuntimeOptions::startup_snapshot` 加载（官方示例 `libs/core/examples/snapshot/`）。支持 `startup_snapshot` 叠加、`with_runtime_cb` 定制、warmup 预热脚本（snapshot.rs:186-260 全流程）。
  - rusty_v8 完整绑定 `SnapshotCreator`（CreateBlob/SetDefaultContext/AddContext/AddData，`src/snapshot.rs:15-41`）。
  - 定位：**构建期快照**（全玩家共享的宿主侧底座）。玩家代码快照无必要（代码变更稀疏 + isolate 重建便宜）；若坚持可为每用户做 runtime snapshot，但需绕开 deno_core 的静态 op 注册模型，性价比低——**推荐：底座快照 + 每用户常驻 isolate**。
- 生产佐证【生产验证】：Supabase `strategy_per_worker.rs`（每个 worker 常驻服务多请求）；isolated-vm README 记载 Screeps 官方 isolate "can persist for several days at a time"——同模型。
- 2026-06 起 deno_core 还有 V8 code cache（Releases.md:49 "hash source contents for V8 code cache"；:744 "V8 code cache for residual lazy-loaded ESM modules"）进一步降二次加载成本。

### 2.5 多模块玩家代码 ✅（ESM 原生；CommonJS 用垫片，均实测）

**deno_core 模块加载模型（额外必查项）**：
- **ESM-only**。加载器 trait `ModuleLoader`（resolve/load，`libs/core/modules/loaders.rs:61`）；实现有 `FsModuleLoader`（:537）、**`StaticModuleLoader`（:619，从 HashMap<specifier, code> 同步加载——正适合玩家模块表）**、`NoopModuleLoader`。`ModuleType` 只有 JavaScript/Json（loaders.rs），没有 CommonJS。
- **CJS 支持不在 deno_core**：Deno CLI 的 CJS 互操作在 `ext/node`（deno 单仓库 `ext/node`，cjs-module-lexer 方案），那是 deno runtime 层，deno_core 裸嵌入拿不到。路径有两条：
  1. **加载前转换**：玩家提交 CommonJS → esbuild/swc 打包成 ESM（xxscreeps 亦用转译思路）；最稳，语法兼容性最好。
  2. **运行时垫片**（零依赖，已实测）：isolate 内注入 `require()`：op `op_cjs_load(name)` 从 Rust 侧模块表取源码，JS 侧 `new Function('module','exports','require', code)` 执行 + 缓存。
- 【实测】输出（`./target/debug/denocore-bench modules`）：
  ```
  [js] CJS: require(main) => 42        ← util.js 被 main.js require，语义正确
  [js] ESM: 6*7 = 42                   ← main.js 相对导入 ./lib/math.js，StaticModuleLoader 两模块图
  ```
- 玩家 API 形态（`require('main')` 相对模块、`module.exports`）与 Screeps 官方一致；`_.`（lodash3）可打进快照或垫片全局。

### 2.6 全局注入 ✅

- **op 体系**：`#[op2]`/`#[op2(fast)]` 宏把 Rust 函数注册为 `Deno.core.ops.*`（hello_world 示例：`libs/core/examples/hello_world.rs`），`extension!`/`Extension` 组织成扩展；fast 变体走 **V8 Fast API**（C++/Rust 直调，绕过 JSON 序列化）——寻路等热路径 Rust 原生函数的正确注入方式。`deno_ops` crate（`libs/ops`，README）为现代写法工厂。
- **任意对象注入**：op 内可拿 `&mut v8::PinScope`（op2 合法特殊参数，`deno_ops-0.288.0/op2/signature.rs:1514`），直接操作 globalThis。【实测】`op_attach_terrain(scope)` 把 SharedArrayBuffer 挂到 `globalThis.__terrain`，`global.set` 返回 `Some(true)`。
- Game/Memory/RawMemory/常量对象：启动脚本（快照内）构建对象骨架 + 每 tick 用 op/serde_v8 填充数据。serde_v8（deno 单仓库 `libs/serde_v8`）提供 Rust↔JS 值转换。

### 2.7 共享只读大缓冲（全图地形表）✅

- 机制：`v8::BackingStore`（引用计数、进程级共享）+ `SharedArrayBuffer::with_backing_store(scope, &SharedRef<BackingStore>)`（`rusty_v8 src/shared_array_buffer.rs:73`）——每个 isolate 包一个 SAB 壳，底层数据同一份。`UniqueRef::make_shared()`（`src/support.rs:126`）。
- 【实测】（`./target/debug/denocore-bench sab`）：
  ```
  [sab-rust] store byte_length=4096 data_is_some=true
  [js] A: terrain byteLength=4096 terrain[8]=42 (written by Rust, no copy)
  [js] A wrote terrain[9]=99
  [js] B sees terrain[8]=42, terrain[9]=99 (written by A, zero-copy across isolates)
  [sab] Rust sees byte_len=4096 terrain[8]=42 terrain[9]=99 — one allocation shared by 2 isolates + host
  ```
  Rust 写一次、isolate A 读写、isolate B（另一个 JsRuntime）读到——**零拷贝、单份内存**确认。
- 只读控制：不暴露原始 SAB 而是包一层 DataView/受限 op，或用 `Atomics`/标志位防止玩家写（SAB 本身可写，需宿主设计只读视图——社区通行做法是注入 Uint8Array 只读代理或干脆 ArrayBuffer 拷贝小表；地形表只读语义要自己在注入层做）。
- 相关：`IsolateGroup`（`rusty_v8 src/isolate_group.rs`）文档：默认所有 isolate 同组共享指针压缩 cage（"most efficient configuration in terms of speed and memory use"），组内堆总量上限 4GB（不含 ArrayBuffer 堆外）。

### 2.8 语法兼容 ES2020+ / 微任务排空 ✅

- V8 15.0 = Chrome 150 引擎，完整支持 ES2024 及试验性提案（V8 feature support 与 Chrome 同步）。
- 【实测】（`./target/debug/denocore-bench syntax`）一条脚本同时跑：可选链 `o?.a?.b ?? -1` → 42、BigInt `10_000_000n*2n` → 20000000、命名捕获组 → 2026、**class 私有字段 `#p`（ES2022）** → 1、flatMap、WeakRef 全部通过。
- 微任务：deno_core 设 `MicrotasksPolicy::Explicit`（`libs/core/runtime/setup.rs:284`），由事件循环在各阶段 `perform_microtask_checkpoint`（jsruntime.rs:2427-2584 多处）。Screeps 式单 tick 同步模型：`execute_script`/`mod_evaluate` 后 `run_event_loop(PollEventLoopOptions::default())` 跑到空闲即微任务全排空。【实测】`Promise.resolve().then(...)` 的回调在 tick 内被执行打印（`[js] microtask drained; ...`）。

### 2.9 执行性能 ✅

- 就是 V8 JIT 本尊（Node/Chrome 同款引擎同款优化管线）——"接近 V8 JIT"对本方案是恒等式。唯一开销：op 边界（fast ops 显著低于普通 ops；deno 有 5ns 级 fast call 的公开 benchmark 传统，具体数字未在本机复测，标注【文档声称】）。
- GC：分代 GC，热路径小对象在新生代，`heap_limits` 同时约束 GC 行为；对 Screeps 负载（大量短命 creep 决策对象）正是 V8 的舒适区。
- 【实测】本次 bench 中 JS 计算循环（5e6 次 sqrt + 计时 op 调用）在 debug 构建宿主下也毫秒级完成，无异常开销可见；严谨吞吐对比需专门 benchmark（未做，如实说明）。
- 量化差距的对象是 QuickJS/Boa 等解释器方案（差 5-50 倍量级，见其他调研员笔记）；V8 方案不存在此差距。

### 2.10 安全隔离 ⚠️（有条件满足：能力默认为零 + 需进程分片加固）

- **能力模型 = 默认零授权**：deno_core 裸嵌入不注册任何 fs/net/process op，玩家代码没有任何获得宿主能力的通道（`Deno.core.ops` 只有你注册的）。文件系统/网络/宿主内存访问在未注入时**根本不存在**，而非被权限拦截。【文档声称：README "TypeScript support and lots of other functionality are not available at this layer"】
- **但同进程内的 isolate 不是硬安全边界**【文档声称，Deno 官方】：
  - Deno Deploy 架构博客（deno.com/blog/anatomy-isolate-cloud）原话："**While V8 isolates alone do not provide a perfect sandbox**, they do a good enough job for many use cases"；其生产做法是 **每个 deployment 独立 V8 isolate + 独立进程**、seccomp 过滤 syscall、网络控制面/数据面分离、Rust 宿主、资源配额监控（"The Runner as an Isolate Hypervisor"一节）。
  - isolated-vm README 同样警告：leak `Reference`/`ExternalCopy` 等对象给不可信代码会被用作跳板；"it's a good idea to keep instances in a different process than other critical infrastructure"。
- **V8 Sandbox**【文档声称】：V8 上游 `src/sandbox/README.md`（chromium.googlesource.com，main 分支抓取）："A low-overhead, in-process sandbox for V8… restricting the code executed by V8 to a subset of the process' virtual address space"；"The sandbox is enabled by default on supported configurations"（V8 上游构建；**rusty_v8 需显式开 cargo feature `v8_enable_sandbox`**，且要求 pointer compression——crates.io v8 152.2.0 features 列表核对）。威胁模型明确：假设攻击者已能在沙箱地址空间内任意读写，保护的是进程其余部分；沙箱仍在演进（有专项 fuzzing/测试基建）。
- **对本项目的落地建议**：官方 Screeps 的威胁面与 Deno Deploy 类似。建议：
  1. runner 进程只跑玩家 isolate（无密钥、无 DB 连接），host 与 runner 分离（Deno Deploy 模式）；
  2. 每 runner 进程限 N 个用户 isolate，超限/异常即销毁重建；
  3. 开 `v8_enable_sandbox` feature + 开 `v8_enable_pointer_compression`；
  4. Linux 上加 seccomp 配额（Deno 生产同款）；
  5. CPU/内存监督线程独立于 isolate 线程（本 bench 已示范跨线程 terminate）。

---

## 3. 多 isolate 单进程内存现实数字（额外必查项）【实测】

测试：deno_core 0.412.0（V8 15.0.x），macOS arm64（M 系列），**debug 构建**，每 isolate 含 deno_core 默认 JS 底座（core.js 等）+ 2 个自定义 op 扩展，`ps -o rss` 读数（`/tmp/sb-research/denocore-bench isolate_mem`，多次运行取代表值）：

```
[isolate_mem] RSS before any isolate: 2.9 MB
[isolate_mem] after isolate 1: 20.6 MB     ← 首个 isolate 含 V8 平台/线程池初始化 ≈ +17.7MB
[isolate_mem] after isolate 2: 23.9 MB     ← 之后每个增量：
[isolate_mem] after isolate 5: 33.2 MB        ~2.2–3.5 MB / isolate
[isolate_mem] after isolate 10: 49.0 MB
```

结论：
- **每空 isolate 增量 ≈ 3MB（2.2–3.5MB）**；首隔离一次性 ~18MB（V8 platform）。10 用户单进程 ~50MB 起步，加玩家代码与 256MB 上限预算后，单进程跑 20–50 用户、RSS 控制在数 GB 内是现实规模；IsolateGroup 默认同 cage，组内 JS 堆总和上限 4GB（rusty_v8 isolate_group.rs 文档；堆外 ArrayBuffer 不计入，正好地形表是堆外共享的）。
- 生产参照【生产验证】：Supabase 给每 user worker 配 256MB 内存上限（其 runtime 用 heap_limits + CustomAllocator 强制）；Deno Deploy 每 app 512MB。
- 注意：数字来自 debug 构建与 macOS；Linux release 会略低（未测，如实说明）。Screeps 官方 driver（isolated-vm）同为 V8，量级一致。

**实测踩坑记录（同进程多 isolate 的现实约束）**：
1. 同一线程上创建两个 JsRuntime 后**交替** execute_script，panics：`PinnedRef<HandleScope> and Context do not belong to the same Isolate`（rusty_v8 scope.rs:950）。→ **一个 isolate 一个线程**（Supabase 架构同款：每个 worker 独立线程 + 独立 tokio current-thread runtime）。顺序使用"最新创建的 runtime"则正常。
2. 程序退出时一次性 drop 10 个 runtime 出过 `Fatal error in v8::HandleScope::CreateHandle()`（退出路径的 drop 顺序问题）——宿主应显式、有序销毁 runtime（在各自线程与 tokio 上下文内 drop），不要依赖进程退出时的批量析构。此为 bench 现象，非稳定复现的 deno_core 缺陷，如实记录。
3. `#[op2]` 的 op 名不能与 deno_core 内置 op 重名（如 `op_print`——ops_builtin.rs:234），debug 构建会直接 panic 提示 duplicate。

---

## 4. CPU 时间计量与硬超时完整方案（汇总）

（对应额外必查"isolate 级 CPU 时间、线程时间记账"）

| 层 | 机制 | 状态 |
|----|------|------|
| 读数（getUsed） | op → `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` − tick 基线 | 【实测】ns 精度，JS 内可调 |
| 软限（tick 边界） | 每 tick/请求结束比对累计值，超软限标记 | 【生产验证】Supabase `CPUUsageMetrics::Leave` 累计（strategy_per_request.rs:141-160） |
| 硬限（执行中打断） | Linux：POSIX per-thread CPU timer SIGALRM；macOS/其他：监督线程轮询/墙钟 | 【生产验证】cpu_timer crate（Linux）；terminate_execution 跨线程【实测】 |
| 终止 | `IsolateHandle::terminate_execution()`（任意线程）→ "execution terminated" | 【实测】305ms 打断死循环，可 cancel 后复用 |
| 兜底 | 墙钟 timeout（tokio select） | 【生产验证】strategy_per_request.rs wall_clock 分支 |

Game.cpu.getUsed() 毫秒精度需求远低于 ns 级读数能力；V8 自身无 isolate-CPU API 这一事实只影响"如果想要跨线程汇总"，线程记账已覆盖 Screeps 语义。

---

## 5. 生产环境先例与负载【生产验证】

| 案例 | 用法 | 证据 |
|------|------|------|
| **Supabase Edge Functions**（open source `supabase/edge-runtime`，deno_core 构建） | 全球生产多租户不可信代码：每用户 worker 一个 deno_core isolate，per_request/per_worker 双监督策略，256MB/2s CPU 硬限 | 仓库 2026-09-25 仍在合并 PR（本机 clone git log）；官方 limits 文档；cpu_timer/CustomAllocator/MemCheck 源码 |
| **Deno Deploy / Subhosting** | 30+ 边缘节点；每 deployment 独立进程 V8 isolate；seccomp；网络分层；资源配额由 runner（isolate hypervisor）执行 | deno.com/blog/anatomy-isolate-cloud（2022-09）；docs.deno.com/subhosting（"securely run code written by their customers"） |
| **Subhosting v2（2026）** | v1 API 已于 2026-07-20 关停，v2 = api.deno.com/v2 + SDK `@deno/sandbox`/`sandbox-py` | docs.deno.com/subhosting/manual（"Sunsetting on July 20, 2026"） |
| **Deno Sandbox（2026 新产品）** | 不可信代码改为 **Linux microVM**（<1s 启动，allowNet 白名单 + secrets 注入） | docs.deno.com/sandbox |
| （旁证）Vercel Edge Functions / Netlify Edge / GitHub Codespaces webview 等基于 Deno/V8 isolate 的边缘运行时 | 大规模请求级负载 | 行业常识级【社区经验】，未逐一核验 |

**行业信号**：Deno 2026 年把"最不可信负载"（AI 生成代码执行等）迁到 microVM（Deno Sandbox），而"函数型"用户代码仍留在 V8 isolate（Subhosting/Deploy）。Screeps 玩家代码属"无 I/O 能力、纯计算"型，配合能力归零 + 进程分片，isolate 方案是恰当档位。

## 6. 沙箱逃逸 / CVE 历史

- **deno_core / rusty_v8 本体：无公开 CVE 记录**（GitHub Advisory Database 按 "deno" 检索 2026-09-27，逐条核对：无一条针对 deno_core crate 或 V8 绑定层）。
- **deno CLI 层的 advisories 都是"权限执行缺陷"，非引擎逃逸**，且全部依赖 deno 的 fs/net API 存在——裸 deno_core 嵌入（不注入这些能力）天然不受影响：
  - GHSA-cpgj-f7g3-2pp2 `fetch()` DNS 解析检查缺失（sandbox bypass，DNS rebinding 类）
  - GHSA-83pc-3rw9-qpwj WebSocket API 同类
  - GHSA-8xpq-cjcf-3wh9 macOS APFS Unicode 归一化绕过 read 权限
  - GHSA-7xh3-mhg9-jcw8 / GHSA-4c96-w8v2-p28j / GHSA-hmh4-3xvx-q5hr / GHSA-m3c4-prhw-mrx6：Windows spawn/child_process 命令注入（不完整过滤）
  - GHSA-968w-xfqw-vp9q BYONM package.json main 路径穿越绕过 --allow-read
- **V8 引擎本体**：常规 n-day 流（Chrome 安全公告节奏）。Deno 官方立场（Deploy 博客）：跟随 V8 更新 + 遵循 V8 官方 untrusted-code 建议 + 进程/seccomp 分层。缓解路径见 §2.10 与 §7。
- 结论：**没有"从 JS 打穿 V8 isolate 逃到宿主"的公开先例被用于攻击 deno 系产品**；真正的历史问题都在"宿主把能力暴露给了 JS"这一层。对本方案启示：能力归零 + 不把任何 v8 handle/引用泄漏给玩家代码（isolated-vm README 同款警告）。

## 7. rusty_v8 版本升级策略与安全补丁节奏（额外必查项）

- **节奏**【文档声称+crates.io 数据】：major 跟 Chrome（4 周），major 内 semver 兼容。2026 实际：152.2.0(08-20)、152.1.0(08-08)、152.0.0(08-01)、150.4.0(07-26)…149.4.0(06-11，晚于 150.0.0 06-08 发布)——**旧线会继续出 patch**（为在用 major 回携修复）。
- **二进制供应**：默认 cargo build 从 GitHub Releases 下载预编译 libv8 静态库（README "Binary Build"），`V8_FROM_SOURCE=1` 自编译。rusty_v8 crates.io 包体 36MB（crates.io API：crate_size 36569477）。
- **deno_core 锁线现实**：deno_core 0.412.0 → deno_v8 0.4.0 → `v8 ^150.4.0`（Cargo.lock 实证解析 150.4.0）。**你无法在不升级 deno_core 的情况下跳到 152.x**。安全响应路径：
  1. V8 n-day 时等 rusty_v8 在 150.x 出 patch → `cargo update -p v8`（semver 内，无 API 破坏）；
  2. 或升级 deno_core（每 2-4 周就有新版，通常随附 V8 bump——Deno Releases.md 记载 2.9.4 升 V8 150.2.0）。
- **运维建议**：CI 里 pin deno_core 并监控其 `v8` 依赖变化；把"升级 deno_core"当作季度例行 + 安全事件驱动。风险面与 Node LTS 跟 V8 的模式一致（isolated-vm README 也建议跟 node 的 V8 point release）。

## 8. 与官方 isolated-vm 方案的能力对照

isolated-vm（laverdet，npm 7.0.1 / 2026-08-05，"**maintenance mode**"）是 **Node.js 库**；Screeps 官方生产在用（README "Who Is Using isolated-vm"：Screeps "run arbitrary player-supplied code in secure environments which can persist for several days"；另 Fly.io、Algolia Crawler）。Node 20+ 需 `--no-node-snapshot`。

| 维度 | deno_core + rusty_v8 | isolated-vm |
|------|----------------------|-------------|
| 宿主语言 | **Rust**（原生集成，无 FFI 桥） | Node.js（Rust 宿主需 NAPI 桥/子进程） |
| 引擎与升级 | V8 15.0，Deno 专业团队维护，4 周 major 节奏 | V8 随 Node ABI，维护者单人、maintenance mode |
| 项目活跃度（2026-09） | deno_core 月更、rusty_v8 HEAD 3 天内；crates 月下载 170 万 | npm 半年 3 版；issue 积压 |
| 超时终止 | `terminate_execution` 任意线程【实测】 | `isolate.terminateExecution`（同类 V8 API） |
| 内存上限 | heap_limits + near_heap_limit_callback【实测】 + CustomAllocator【生产验证】 | `isolate.setMemoryLimit`（V8 heap limit 同源；无 AB 分配器限流，需自建） |
| CPU 计时 | 无内置；线程记账方案成熟（Supabase 生产）【生产验证】 | 无内置；社区用 watchdog 线程 |
| 模块 | ESM 原生 + CJS 垫片【实测】 | Script 字符串执行 + ESM（`isolate.compileModule`）；CJS 自己 eval |
| 快照 | 构建期 create_snapshot（含 warmup）【文档声称】 | 无快照（Node 的 snapshot 与其冲突，要求 --no-node-snapshot） |
| 数据传递 | serde_v8 / ops / SAB 零拷贝【实测 SAB】 | Reference/ExternalCopy（专为跨隔离设计，API 成熟） |
| 安全论调 | 同进程非硬边界；Deno 生产加进程+seccomp【文档声称】 | 同样明示非完美沙箱，同样建议进程分离 |
| Screeps 语义先例 | 无直接先例（但 Supabase 常驻 worker 同构） | **官方 Screeps 即此方案** |

结论：若宿主选 Rust，deno_core 在**维护强度、升级节奏、快照、fast ops、生产多租户规模**上全面占优；isolated-vm 的优势是 Node 宿主与 Screeps 官方同款语义。二者隔离强度同级（同一 V8 isolate 机制）。

## 9. Rust 宿主按用户持久复用的最佳实践（含 snapshot/预热）

综合 Supabase 生产代码 + 本次实测：

1. **每用户 isolate 独占线程 + 独立 current-thread tokio runtime**（Supabase：worker = thread + tokio_rt；实测避免同线程交替 scope panic）。宿主调度器按 user_id → worker 的池管理；空闲超时回收（Deno Deploy runner 同款："terminates deployments that have been running for a while without receiving traffic"）。
2. **构建期 startup snapshot**：`create_snapshot` 打入 Game 常量/lodash/全局底座 + `warmup_script` 预热 JIT（snapshot.rs:186 起）；运行时 `startup_snapshot` 加载。
3. **玩家代码进入**：代码变更 tick 时换 `StaticModuleLoader` map（新 specifier 集）重跑入口模块；或整 isolate 重建（成本 ≈3MB + ms 级创建，实测销毁重建亦干净）。
4. **tick 执行协议**：`execute_script`/`mod_evaluate` → `run_event_loop` 排空微任务 → 读线程 CPU 结算（软限判定）→ 归还线程池。监督由**独立监督任务**（tokio select：SIGALRM 通道 / 墙钟 / 取消令牌）执行，超限 `thread_safe_handle().terminate_execution()`（Supabase supervisor 同构）。
5. **内存三件套**：heap_limits(0, 256MB) + near_heap_limit_callback（terminate+临时抬限）+ 自定义 AB 分配器上限（照抄 Supabase external_memory.rs 模式）。
6. **销毁纪律**：isolate 在其线程与 tokio 上下文内显式 drop；不要依赖进程退出批量析构（实测退出路径批量 drop 曾触发 HandleScope fatal）。
7. **能力纪律**：除显式注册的 ops 外不暴露任何 v8 Global/Local 句柄给玩家侧；Deno.core 命名空间注入后可考虑 `disable_ops`（示例 `disable_ops.rs`）收窄默认面。

## 10. 风险与缺口清单

1. **CJS 兼容**：垫片已验证，但玩家包若用了奇特 CJS 变体（circular require 的未完成 exports、`this === module.exports` 等）需按 Node 语义补齐垫片细节（本次只验证了标准模式）。稳妥路线是提交期 esbuild 转 ESM。
2. **heap_limits 单用即崩**：必须搭配 near_heap_limit_callback（§2.3），这是文档不显眼、实测才暴露的点。
3. **同进程安全上限**：若威胁模型包含国家级/持续攻击者，参照 Deno Sandbox 走 microVM；Screeps 类社区负载，进程分片 + V8 sandbox feature 已对齐 Deno/Supabase 生产水位。
4. **版本锁线**：deno_core → deno_v8 → v8 ^150.4.0，V8 安全补丁到达依赖 Deno 团队节奏（月级，历史记录良好）。
5. **macOS 上 POSIX CPU timer 不可用**（Supabase cpu_timer cfg linux）：开发机（mac）只能用轮询/墙钟监督，生产 Linux 才有 SIGALRM 硬闹钟。CLOCK_THREAD_CPUTIME_ID 读数 mac 可用【实测】。
6. 性能专项 benchmark（大对象负载、与 Node 同负载对比）未做；V8=Node 引擎故差距理论上为零，标注未测。

## 11. 实测复现（命令与输出）

测试工程：`/tmp/sb-research/denocore-bench`（依赖 `deno_core = "0.412.0"`、tokio、libc；本报告全部实测输出的来源）。构建：`cargo build`；运行子命令：`syntax | cpu | terminate | isolate_mem | heaplimit | sab | modules`。关键输出均已内联上文（§2.1–§2.8、§3）。环境：macOS 24.6 arm64、Rust 1.98.1、deno_core 0.412.0（解析 v8 150.4.0）、debug 构建。

核对过的关键源码位置（本地 clone，commit 2026-09-25/27）：
- rusty_v8：`src/isolate.rs:1109`（thread_safe_handle）、`:1120`（terminate_execution）、`:1963`（add_near_heap_limit_callback）、`:2685`（IsolateHandle::terminate_execution 任意线程）、`src/isolate_create_params.rs:137`（heap_limits）、`src/shared_array_buffer.rs:73`（with_backing_store）、`:119`（new_backing_store）、`src/snapshot.rs:15-41`（SnapshotCreator 绑定）、`src/isolate_group.rs`（共享 cage/4GB/沙箱互斥说明）、README.md（V8 15.2.124.1；Versioning；Binary Build）。
- deno_core（deno 单仓库 `libs/core`，0.412.0）：`runtime/jsruntime.rs:739`（JsRuntime::new）、`:2018`（execute_script）、`:2303`（run_event_loop）、`runtime/snapshot.rs:119/186`（CreateSnapshotOptions/create_snapshot）、`modules/loaders.rs:61/537/619`（ModuleLoader/Fs/Static）、`runtime/setup.rs:284`（MicrotasksPolicy::Explicit）、`examples/hello_world.rs`、`examples/snapshot/`。
- deno_v8 门面：`libs/deno_v8/Cargo.toml`（rusty_v8 150.4.0 / v8x quickjs 双后端）。
- Supabase edge-runtime：`crates/cpu_timer/src/lib.rs:96/112/152/196`、`ext/runtime/external_memory.rs:11`、`crates/base/src/runtime/mod.rs:830-880`（UserWorker heap_limits+allocator+mem_check）、`:170-215`（MemCheck）、`crates/base/src/worker/supervisor/strategy_per_request.rs:104-196`、`strategy_per_worker.rs:332-344`。

## 12. 参考链接

- crates.io API：`https://crates.io/api/v1/crates/deno_core`、`/crates/v8`（版本/日期/下载量）
- deno_core（现居 deno 单仓库）：https://github.com/denoland/deno/tree/main/libs/core （原 https://github.com/denoland/deno_core 已归档）
- rusty_v8：https://github.com/denoland/rusty_v8 （docs.rs/v8）
- Deno Deploy 架构博客（isolate hypervisor/seccomp/安全论调）：https://deno.com/blog/anatomy-isolate-cloud
- Deno Subhosting 文档（v1 2026-07-20 sunset、v2 SDK @deno/sandbox）：https://docs.deno.com/subhosting/manual/
- Deno Sandbox（microVM 新品）：https://docs.deno.com/sandbox/
- Deno Deploy 限额（512MB/app）：https://docs.deno.com/deploy/pricing_and_limits/
- Supabase Edge Functions 限额（256MB / 2s CPU）：https://supabase.com/docs/guides/functions/limits
- Supabase edge-runtime 源码：https://github.com/supabase/edge-runtime
- V8 Sandbox README：https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/sandbox/README.md
- isolated-vm（对照）：https://github.com/laverdet/isolated-vm
- V8 官方"运行不可信代码"安全建议（Deploy 博客引用）：v8.dev 安全文档
- Deno Releases（V8 升级记录）：https://github.com/denoland/deno/blob/main/Releases.md
