# boa_engine（纯 Rust JS 引擎）作为 Screeps 类沙箱的调研笔记

- 调研日期：2026-09-27
- 调研员：Boa 调研员
- 版本基线：**boa_engine 0.22.0**（crates.io 2026-08-28 发布）+ main 分支（commit 39cd112，2026-09-20）
- 证据类型标注：【实证】= 本次在本地构建运行验证（命令与输出见附录 B）；【文档声称】= 官方文档/源码/官方数据仓库；【社区经验】= issue/PR/第三方
- 本地实证环境：macOS 15 arm64（Apple Silicon），rustc 1.98.1，release 构建，node v24.11.0 作为 V8 JIT 对照

---

## 0. 一句话结论

boa_engine 0.22 的语法兼容性（test262 96.01%）、全局注入、微任务排空、跨 Context 共享只读数据、Context 持久复用（重建成本仅 0.25ms）全部实测通过，且纯 Rust、零 C/C++ 依赖；但 **同步硬中断 API 缺失**（只能用 budget 协作式中止，且**原生调用如正则是中断盲区**，实测灾难性正则超时 6.9s 无法中止）、**无每 Context/每堆内存上限 API**（GC 是线程本地单例堆、配置结构体私有）、**性能距 V8 JIT 4~22 倍差距**（官方基准对 v8-jitless 差 12 倍、对 QuickJS 差 5.7 倍）。可用于沙箱原型与低吞吐场景，直接承载 Screeps 全量玩家负载需要接受明确的性能与硬限制缺口。

---

## 1. 项目现状（2026-09）

| 维度 | 数据 | 来源 |
|---|---|---|
| 最新版本 | 0.22.0（2026-08-28 发布），workspace 版本号 0.22.0（main 已是 0.23 开发中） | crates.io API【文档声称】；`/tmp/boa-research/Cargo.toml` `[workspace.package] version = "0.22.0"` |
| 发布节奏 | 0.18(2024-03) → 0.19(2024-07) → 0.20(2024-12) → 0.21(2025-10) → 0.21.1(2026-03) → 0.22(2026-08) | crates.io API【文档声称】 |
| 仓库活跃 | 7,569 stars / 662 forks / 3,935 commits；pushed_at 2026-09-26（每天有提交，含大量社区 PR 合入） | GitHub API【文档声称】 |
| 开放事项 | 160 open issues + 55 open PRs；v0.23 milestone 开放（内容是 API 审计类：`Rust inheritance is broken` #5372、`[1.0 blocking] Implement Copy for JsValue` #5371、`Audit public APIs` #4524） | GitHub API【文档声称】 |
| test262 一致性 | **96.01% 总体通过（51439/53578），0 panic，1,648 项按配置忽略**；按版本：ES5 99.51%、ES6 99.19%、ES12(ES2021) 98.74%、ES17(ES2026) 98.32% | boa-dev/data 官方数据仓库 `test262/refs/heads/main/latest.json`，对应 main 分支 commit 39cd112（2026-09-20）【文档声称】 |
| 忽略的特性 | ShadowRealm、decorators、import-defer、explicit-resource-management、Intl.DisplayNames 等（`test262_config.toml`） | 仓库 `test262_config.toml`【文档声称】 |
| 许可 | Unlicense OR MIT（极宽松） | 仓库 Cargo.toml |
| 支持策略 | 1.0 前只支持最新 0.x + main | SECURITY.md【文档声称】 |

> README 自述仍称 "experimental"，但一致性已过 90%（README 写 "more than 90% of the latest ECMAScript specification"）。

---

## 2. 十项硬要求逐条判定

### ① CPU 时间计量 —— ✅ 满足（实证）
- 注入的原生函数在玩家代码执行中随时可读宿主时钟，微秒级源精度。【实证】T1：`getUsed()` 在脚本内两次调用返回 1.839ms / 14.733ms，单调递增。
- 引擎自带 `Context::clock()` 与 `Clock` trait（`core/engine/src/context/mod.rs:567`，builder `:1146`），可注入自定义时钟；0.22 起 `StdClock` 已改为 `Instant` 单调（CHANGELOG PR #4841）【文档声称】。
- Screeps 语义（`Game.cpu.getUsed()` 返回 ms 浮点）就是"原生函数读宿主计时器"，与实测路径一致。注：毫秒精度输出即可，宿主侧计数源可达微秒。

### ② 硬超时终止 —— ⚠️ 有条件满足（存在真实盲区）
现状事实链：
- **没有任何已发布的同步中断 API**。【文档声称】全部历史 tag（v0.17/0.19.1/0.20/v0.22）源码 grep `interrupt` 为空（仅 CLI 的 readline 提示）。issue **#3442 "Cancel/Interrupt active evaluation" 自 2023-10-30 开放至今**（22 条评论，最后更新 2026-08-10，无 milestone）；两个实现 PR 均未合并：#5380（HostHooks::on_tick，2026-05-31 关闭未合并）、#5475（`Arc<AtomicBool>` 中断旗标，2026-08-10 当天关闭未合并）。维护者方向是"async 化 Promise jobs / 原生函数"（jedel1043 2026-08-10 评论）而非取消令牌。【社区经验+文档声称】
- **可行路径 A（实测通过）**：`Script::evaluate_async_with_budget(ctx, budget)`（`core/engine/src/script.rs:206-217`，VM 侧 `core/engine/src/vm/mod.rs:953-987`，每消耗 budget 个"时钟周期"让出一次执行权）。宿主用自己的执行器驱动 future，每次让出检查墙钟，超时即**停止 poll 并 drop future**。【实证】T2a：`while(true){}` 在 100ms 死限被中止，**实际中止时刻 100.000125ms（超调仅 125ns）**，让出 226,236 次，宿主进程存活；中止后该 Context 甚至还能继续用（`1+1=>2`，后续 10 次求值全部正确）——不过官方不保证此路径，稳妥做法是超时后重建 Context（实测 `Context::default()` 仅 **0.25ms**，重建极廉价）。
  - 注意：`Context::run_jobs()` 的 Promise 任务目前**没有** budget 版本（维护者在 #3442 里确认 "Promise jobs don't currently have a way to execute asynchronously"，答应做 `run_async` 委托）【社区经验】。玩家在微任务里写死循环会绕过 budget —— 需用 B 方案兜底。
- **可行路径 B（实测通过）**：`RuntimeLimits`（`core/engine/src/context/mod.rs:595`；定义 `core/engine/src/vm/runtime_limits.rs`）。`set_loop_iteration_limit(n)` 可杀死任意循环，抛出的 `RuntimeLimitError` **JS 层 catch 不住**（`EngineError` 设计为"cannot be caught from within ECMAScript code"，`core/engine/src/error/mod.rs:363-372`）。【实证】T2c：limit=1,000,000 时 `try{while(true){}}catch(e){...}` 直接以 Err 结束，`caught=false`。默认值：loop=无限制、recursion=512、stack_size=10KB（`runtime_limits.rs:16-27`）。
- **真实盲区（实测确认）**：单个 opcode 内的原生执行不可让出。【实证】T2b：`/(a+)+$/` 匹配 `'a'.repeat(28)+'b'` 在 60ms 死限下跑了 **6.9 秒、0 次让出**（polls=0，整个正则回溯发生在一次 `RegExp.test` 内）。同风险还有：巨型字符串拼接/`JSON.parse` 大输入、TypedArray 批量拷贝等单 opcode 原生路径。缓解：宿主包一层正则（注入限步数的原生 regex API）、`host_hooks().max_buffer_size()`（`core/engine/src/context/hooks.rs:223`）限 ArrayBuffer 尺寸、对字符串长度设限。500ms 级 CPU 预算下这是必须正面的工程项。
- 结论：JS 字节码层面能做到**亚微秒超调**的软实时中止 + 不可捕获的循环限额；原生调用盲区需逐个堵。与 V8 `TerminateExecution`（可中断解释器/JIT 代码但同样难中断个别原生路径）相比弱一档。

### ③ 内存硬上限 —— ❌ 不满足（引擎层无此能力）
- `boa_gc` 是 **thread_local 单例堆**（`core/gc/src/lib.rs:47-49` `thread_local!(static BOA_GC: RefCell<BoaGc>)`）：同线程所有 Context 共享一个 GC 堆，跨 Context 无内存隔离、无法给单个用户设 256MB 上限【文档声称】。
- `GcConfig`（threshold/used_space_percentage）是**私有结构体**，默认 threshold=1MB、按 70% 使用率自动增长（`lib.rs:55-73, 189-198`），源码注释明写 `// TODO: Add a configure later` —— 连全局限额都还没有 API【文档声称】。公开的只有 `boa_gc::force_collect()`（`lib.rs:534`）与 `has_weak_maps`。
- 也没有 `bytes_allocated` 之类的公开统计（`GcRuntimeData` 私有）→ 无法实现"按用户内存计量/告警"【文档声称】。
- 实测补充：【实证】T9：30 万个小对象 (`{x,y,s}`) RSS 从 28MB→104MB（约 **250B/对象**，明显胖于 V8 的隐藏类内联布局），`globalThis.keep=null` + `force_collect()` 后回落 47MB（GC 本身工作正常，mark-sweep）。
- 可能的补救：每用户/每分片独立 OS 线程（Context 为 `!Send`，见 ⑧ 架构段）天然分堆；再加自定义全局分配器（如 mimalloc/tikv-jemalloc 前置 per-thread 配额层）做计数与熔断——需要自研，非引擎能力。进程级 cgroups/RSS 看门狗杀进程是终极兜底。

### ④ 按用户持久复用 —— ✅ 满足（实证）
- 【实证】T3：同一 Context 跨"tick"复用，tick1 写 `globalThis.Memory`，tick2 直接读到；小脚本 eval 均摊 **2.5µs/次**。
- **无需 snapshot**：boa 没有 V8 snapshot/代码缓存机制（全仓 grep 无对应实现；引擎每脚本是 parse→bytecode→解释执行），但 `Context::default()` 全量内建对象构建仅 **0.25ms**（实测均值，50 次）——比 V8 建 isolate+context 快一个数量级量级，"代码变更才重建 / 超时即重建"策略完全可负担【实证】。
- 注意：同一段顶层 `let`/函数声明脚本不能对同一 Context 二次 eval（"duplicate lexical declaration"，脚本级词法声明冲突是正确语义）——玩家 main 应包在函数里或每 tick 换 Source、或在模块体系里跑【实证，T8 首次运行时踩到】。
- 缓存编译产物：`Script`/`CodeBlock` 是 `Gc` 句柄，可按用户缓存复用（`Script::parse` 一次、多次 `evaluate`）【文档声称，API 可见；未单独压测】。

### ⑤ 多模块玩家代码 —— ✅ 基本满足（ESM 原生；CommonJS 需宿主自实现 ~50 行）
- ESM：完整模块系统（`core/engine/src/module/`：source/synthetic/namespace；`ModuleLoader` trait 支持**异步**加载 `core/engine/src/module/loader/mod.rs:172-176`，内置 `SimpleModuleLoader`（磁盘）、`MapModuleLoader`（specifier→Module 映射）、`IdleModuleLoader`（禁用加载））。【实证】T10：两个 .mjs 文件（`import {ROOM, dist} from './util.mjs'`）用 SimpleModuleLoader + `Module::load_link_evaluate` + `run_jobs` 跑通，promise Fulfilled。0.22 还实现了 Import Attributes（PR #4549）、import.defer/import.source 解析（PR #5035）【文档声称，CHANGELOG】。
- CommonJS：引擎/运行时**均无内置 `require`**。官方示例 `examples/src/bin/modulehandler.rs` 演示用 `register_global_callable("require")` + `module.exports` 对象模拟——Screeps 玩家模块包（main.js + 相对 require）就是照此实现一个内存模块表。【文档声称+实证路径明确】
- 懒编译注意：0.22 的字节码编译器会**立即编译所有嵌套函数**（无 lazy function compilation），玩家大 bundle 启动时全量 parse+compile（issue #5522，2026-09 社区分析指出）【社区经验】。

### ⑥ 全局注入 —— ✅ 满足（实证）
- 【实证】T1/T5：`register_global_callable` 注入原生函数、`register_global_property` 注入对象/值（`core/engine/src/context/mod.rs:252, 286, 319`）；另有 `register_global_class`（Rust `Class` trait，examples/src/bin/classes.rs）。
- 原生捕获数据：`NativeFunction::from_copy_closure_with_captures`（captures 需 `Trace`），或 `context.insert_data()` 宿主数据 + `get_data` 读取，或 `#[derive(Trace, Finalize, JsData)]` 自定义类型 + `#[unsafe_ignore_trace]` 包 `Arc`（SharedArrayBuffer 实现即此写法，`builtins/array_buffer/shared.rs:29-36`）。
- lodash3 一类纯 JS 库直接 eval 进全局即可。寻路等 Rust/C++ 原生函数注入为 JS 可调用对象：T5 的 `terrainGet` 模式实测通过；C++ 侧经 FFI→Rust wrapper 同理（但那就引入 C 依赖，见"纯 Rust 优势"段）。

### ⑦ 共享只读大缓冲 —— ✅ 满足（实证，两条路都通）
- **SharedArrayBuffer 真共享**：0.22 已实现 SAB + Atomics builtin（`core/engine/src/builtins/mod.rs:92-95, 258, 315, 396`），底层 `Arc<Inner>` 持有 `AlignedBox<[AtomicU8]>`，源码注释明言 "implements Send and Sync … can be shared between threads running different JS code"（`shared.rs:23-36`）。【实证】T4：Context A 创建 SAB 并写入，把 **同一个 SAB 对象**注册进 Context B 的全局：B 读到 A 的写值(42)，B 写入后 A 读到(99) —— 跨 Context 零拷贝共享成立。限制：从宿主自有内存构造 SAB 的 `create_shared_byte_data_block` 是 `pub(crate)`（`shared.rs:557`），宿主无法直接把已有 `Vec<u8>` 包成 SAB；做法是让 JS 侧 `new SharedArrayBuffer(n)` 后由宿主经 TypedArray 拷入，或走下一条路【文档声称+实证】。
- **原生对象零拷贝（更推荐）**：宿主持有 `Arc<Vec<u8>>`/`Arc<[u8]>`（或 mmap 的地形表），注入只读 getter 原生函数或 JsData 对象给任意多个 Context —— 内存只占一份。【实证】T5：4 个 Context 共读同一 `Rc<Vec<u8>>` 全部正确。
- 与 xxscreeps/官方 driver 的 terrain2 SAB 方案同构可行。

### ⑧ 语法兼容 ES2020+ —— ✅ 满足（实证 + 官方数据）
- 【实证】T7 全过：可选链 `?.`/`?.()`、`??`、逻辑赋值 `??=/||=/&&=`、BigInt、class 私有字段 `#v`、静态类字段、flatMap/replaceAll/matchAll、`Promise.allSettled`、globalThis、async 函数、展开/剩余、生成器 `yield*`、`Symbol.asyncIterator` —— 16/16 通过，0 失败。
- 【文档声称】test262 ES12(2021)=98.74% … ES17(2026)=98.32%，总体 96.01%。Screeps 玩家代码（ES2017+ 常见语法）覆盖面足够；忽略清单里影响面最大的是 ShadowRealm/decorators/explicit-resource-management（`using`），主流玩家代码用不到。
- **微任务排空**：`Context::run_jobs()` 一次调用排空当前任务队列。【实证】T6：`Promise.then` 回调与 `await null` 之后的代码，eval 返回时未执行（`[sync]`），`run_jobs()` 后全部执行（`[sync,t1,aw]`）——"单 tick 同步跑完"= eval + run_jobs 的固定组合，语义可控。
- 兼容性风险点：`async` 原生函数/Job 的 budget 化尚未提供（见②），ShadowRealm 未实现（想用它做隔离的不行）。

### ⑨ 执行性能 —— ⚠️ 明确量化：显著慢于 V8/QuickJS
官方基准（boajs.dev 数据仓库，bench-v8 套件，加权分越高越好，最新 2026-09-27，每日 CI 更新）【文档声称】：

| 引擎 | 总分 | 相对 boa |
|---|---|---|
| porffor | 2595 | 12.3x |
| v8-jitless | 2542 | **12.0x** |
| quickjs | 1200 | **5.7x** |
| libjs | 840 | 4.0x |
| sm-jitless | 768 | 3.6x |
| kiesel | 381 | 1.8x |
| duktape | 359 | 1.7x |
| **boa** | **211** | 1x |

分项（boa vs v8-jitless / vs quickjs）：Crypto 6.2x/5.3x、DeltaBlue 10.1x/5.4x、Richards 8.0x/4.8x、Splay 8.6x/4.5x、**RegExp 94x/8.8x**、RayTrace 14.4x/6.3x、NavierStokes 4.1x/4.7x、EarleyBoyer 18.8x/6.9x。boa 两年半内从 51 分涨到 211 分（4 倍），但无 JIT，JIT 也不在近期路线图上（#5522 讨论"哪些性能工作排在 JIT 前面"；已有 4-way 多态内联缓存、AST 优化器强度削减/死代码消除）。

本机实测（Apple Silicon，release，node 24 = V8 JIT 对照）【实证】：
- Screeps 风格负载（1000 creep 对象 × 50 tick 的 filter/sort/对象字典）：boa **51.6ms** vs node **12.3ms** → **4.2x**
- `fib(27)` 递归调用：boa **71.7ms** vs node **3.3ms** → **21.7x**（函数调用路径是短板，与官方 DeltaBlue/Richards 差距一致）
- budget=256 让出的额外开销：3M 次循环 111.4ms → 135.9ms（**+22%**；budget 可调大摊薄）
- 对象内存胖（~250B/对象，T9），大量小对象场景 GC 压力高于 V8（mark-sweep 无分代；分代 GC 在讨论中 #5522）

对 Screeps 量级的含义：单 tick 500ms 预算、每用户 CPU ~20ms 的世界里，4~6 倍吞吐差距意味着同样硬件承载的用户数约为 QuickJS 的 ~1/5、V8 的 ~1/10+；重度算法玩家（寻路、矩阵）会最先撞墙。

### ⑩ 安全隔离 —— ⚠️ 引擎核心默认干净，但隔离是"库级"而非"进程级"
- 引擎核心（boa_engine）无文件系统/网络/环境访问：builtin 清单只有 ECMAScript 内建【文档声称】；默认 `Context::default()` 连 console 都没有。
- 网络与系统 API 全部来自可选运行时：`boa_wintertc`（WinterTC/TC55：console、timers、base64、encoding、**fetch**、url、store、abort、events、microtask）与 `boa_runtime`（额外 process.**cwd()**、text、url、message、test262 辅助）——**给不可信玩家用绝不能注册 fetch/process**【文档声称，`core/wintertc/src/lib.rs` 模块清单】。
- 宿主钩子利于收紧：`ensure_can_compile_strings`（可拒绝 `eval`/`new Function`）、`max_buffer_size`（限 ArrayBuffer 分配）、`create_global_object`（自定义全局）、`promise_rejection_tracker`（`core/engine/src/context/hooks.rs:123, 223, 172, 106`）【文档声称】。
- 越权面分析：不可信代码能触达的攻击面 = 解释器 VM + 内建（RegExp 引擎为 Rust `regress`、Temporal/i18n 为 ICU）+ 宿主注入物。**纯 Rust memory-safe** 大幅缩小了内存破坏类逃逸面（这是与 C/C++ 引擎的本质差异）；剩余风险是 panic 类 DoS（引擎把多数内部 panic 转为 `EngineError::Panic` 返回，CHANGELOG 多个 PR；但非全部，宿主仍应 `catch_unwind` 包裹每次 eval）【文档声称+分析】。
- **用户间隔离**：同线程所有 Context 共享一个 GC 堆（③），且 Context `!Send`（【实证】编译断言失败：`Rc<dyn HostHooks>` 等）；一个用户把内存打爆/触发 panic 会波及同线程其他用户。要硬隔离必须：每用户独立线程（天然分堆，panic 隔离仍需 catch_unwind）或独立进程（最强，含超时杀死能力）。SAB/原生注入对象是唯一的跨 Context 共享通道，只读共享安全（SAB 的 atomic 字节无撕裂）。
- CVE/逃逸历史：**仅 1 条** —— RUSTSEC-2024-0444 / GHSA-f67q-wr6w-23jq（boa_engine ≥0.16 <0.19，AsyncGenerator `then` getter 状态转移 panic，CVSS 3.3 7.3：C:L/I:L/**A:H**，属崩溃/DoS，非内存逃逸；0.19.0 已修）。boa_gc/boa_runtime/boa_parser 无记录【文档声称，OSV API 查询 2026-09-27】。另有持续 fuzz（tests/fuzz：parser-idempotency / bytecompiler-implied / vm-implied）与 CodeQL + cargo-audit CI【文档声称】。

---

## 3. 额外必查项汇总

### 3.1 test262 当前进度
96.01%（51439/53578，main@39cd112，2026-09-20 CI 产出，0 panic，忽略 1,648）。分版本见 §1 表。数据管道：main 每次推送自动跑全套并推送到 boa-dev/data（PR #4792）【文档声称】。

### 3.2 性能量化对比
见 §2⑨。要点：对 V8 JIT 本机实测 4.2x（对象密集负载）~21.7x（调用密集）；官方口径对 v8-jitless 12x、对 QuickJS 5.7x、对 SpiderMonkey-jitless 3.6x、对 duktape 1.7x；RegExp 极端弱（对 v8-jitless 94x）。

### 3.3 interrupt 机制现状（2026-09-27）
- 无已发布 API。issue #3442 open（2023-10 开，2026-08-10 仍活跃讨论）；PR #5380（on_tick hook，2026-05-31 关闭未合并）、PR #5475（AtomicBool，2026-08-10 关闭未合并）。
- 维护者认可的方向：**异步化**——`evaluate_async(_with_budget)` 已可用（2023-10 PR #3044 合入，无 feature gate），Promise jobs 的 `run_async` 与 async NativeFunction 在讨论中（jedel1043：将做 "run_async method that delegates to evaluate_async"）。
- `instructions_remaining()` 仅 `#[cfg(feature = "fuzz")]`（`context/mod.rs:452-458`），非宿主可用预算 API。
- 实用组合（本次实证）：budget-async 中止（JS 字节码层，超调 ns 级）+ `loop_iteration_limit`（不可 catch）+ 超时后重建 Context（0.25ms）。盲区=原生 opcode（正则实测 6.9s），需注入受限 regex 替代内建。

### 3.4 内存控制现状
- boa_gc：thread-local 单例 mark-sweep；`GcConfig` 私有（threshold 1MB 起、70% 用量自动扩，`core/gc/src/lib.rs:55-73, 189-198`）；公开 API 仅 `force_collect`/`has_weak_maps`/`finalizer_safe`。**无堆上限、无分代、无每 Context 记量**【文档声称】。
- 可用的间接手段：`max_buffer_size` hook 限 TypedArray/ArrayBuffer；字符串/数组长度无 hook（需自注入包装）；自定义全局分配器 + per-thread 配额（自研）；OS 级 cgroups/进程看门狗。
- 实测：GC 工作正常（47MB 可回收），但对象 footprint ~250B/个，比 V8 胖数倍。

### 3.5 路线图上与本用例相关的项
- v0.23 milestone：API 审计（JsValue Copy、Rust 侧继承、公共 API 审计）——嵌入 API 还会动。
- 性能线（#5522, 2026-09）：无 lazy 编译（大 bundle 启动痛点）、IC 扩展、调用 fast-path、分代 GC 候选——全部"在 JIT 之前"，即 **JIT 遥远**。
- 中断线：#3442 方向=async 化（对 Screeps 用例是对的方向：budget 驱动 + jobs run_async）。
- `boa_wintertc`（0.22 新增）：TC55 服务端 API 基线在补齐（fetch/timers/store…），说明项目目标是"可嵌入的服务端 JS 运行时"，与本用例定位吻合，但其中 fetch 等对沙箱是负资产。
- 无 snapshot/代码缓存计划（未见相关 issue/PR）。

### 3.6 纯 Rust 引擎的独特优势值多少
- **零 C/C++ 工具链**：交叉编译/静态链接一条 `cargo build --target` 搞定；无 vendor C 库、无 CC 工具链版本地狱。对"沙箱服务要以单一 Rust 二进制分发/交叉到各架构"价值高。
- **内存安全边界**：攻击面只剩 Rust 代码自身的逻辑 bug（历史上唯一的 CVE 是 panic-DoS 而非内存破坏），与 QuickJS（C，可写 escape 需赌 memset）对比是本质差异——对"跑不可信代码"是这个引擎最值钱的属性。
- **编译期裁剪**：feature flags（`intl`(ICU)、`annex-b`、`experimental`、`trace`、`fuzz` 等）可裁掉 Temporal/Intl 等大块；无 JIT 意味无运行时代码生成——**W^X 天然成立，攻击者拿不到可执行内存**（V8 JIT 沙箱逃逸的经典路径在 boa 不存在）。
- **可魔改性**：源码可读性高、无 JIT 复杂度，自建补丁（如把 budget 检查插进 opcode 循环、给 GcConfig 加上限）是几百行的可行工程——PR #5475 就是 200 行级别的示例（虽未合并）。
- 代价：性能差 4~12 倍、生态位小（97 个下游 crate）、`!Send` 单线程 API。

### 3.7 生产环境先例
crates.io 反向依赖 **97 个**【文档声称】。较知名：`atac`（终端 API 客户端，=0.21.0，嵌入式 JS 脚本）、`rusty_ytdl`（YouTube 签名解码 = 跑 YouTube 的混淆 JS，真实对抗性负载但单用户自托管）、`revm-inspectors`（以太坊 tracing，optional）、`clyde`（包管理器插件）、`spider_agent`（爬虫）、`arcis-compiler`（MPC 电路编译，dev-dep）。
**没有找到任何公开的"单进程承载大量不可信用户 JS"的生产先例**（区别于 QuickJS 有 lichess-cruncher/llrt、V8 有 Cloudflare Workers 的背书）。若有，也未见文档。此维度结论：社区经验级、无大规模背书。

### 3.8 CVE/沙箱逃逸史
仅 RUSTSEC-2024-0444（详见 §2⑩）。无已知逃逸类漏洞。

---

## 4. Rust 宿主中按用户持久复用的最佳实践（基于本次实证的建议架构）

1. **线程分片（shard-per-thread）**：Context `!Send` 且 boa_gc 是 thread_local —— 把用户哈希分到 N 个常驻线程，每线程一个 loop 持有 `HashMap<UserId, UserRuntime>`（Context + 缓存的 Script/CodeBlock + 注入的 Game/Memory/RawMemory/常量/lodash）。跨线程迁移用户=重建 Context（0.25ms，可接受）。SAB/原生地形表是唯一跨线程共享物（`Arc` 原子字节，实测跨 Context 读写一致）。
2. **每 tick 执行协议**：`script.evaluate_async_with_budget(ctx, budget)` 由宿主 executor 驱动（官方 smol/tokio 示例即此模式，`examples/src/bin/smol_event_loop.rs:290-322`）；每次 Pending 让出时累计墙钟，超预算→停止 poll+drop→（可选）丢弃该用户 Context 重建。之后 `run_jobs()` 排空微任务（**注意 jobs 无 budget 版本**——把玩家 main 限制为"同步脚本+短微任务"，或在 jobs 前也用 loop limit 兜底）。
3. **防呆三件套**：`loop_iteration_limit` 设为"合法代码不可能超过"的量级（如 1e7，防 while(1)）；`max_buffer_size` hook 限缓冲；`ensure_can_compile_strings` 视策略禁 `eval`/`Function`。每次 eval 外层 `catch_unwind`（EngineError::Panic 不保证覆盖所有 panic 路径）。
4. **正则盲区**：不要让玩家用内建 RegExp 跑长输入——注入宿主侧 `regMatch(pattern, str)`（Rust `regex` crate 有线性保证）并在 hook 层限制内建 RegExp 输入长度，或等 regress 有步数限制（未见计划）。
5. **重建与预热**：不需要 snapshot。用户代码变更→重新 parse+compile（`Script` 缓存在 UserRuntime）；超时/panic→整 Context 重建（0.25ms）+ 重放全局注入与玩家模块表（注入开销小，实测单次注入调用为 µs 级）。
6. **计量**：CPU=注入 `getUsed` 读宿主 `Instant`（T1）；内存=无引擎 API，用 OS RSS per-thread 不可得 → 只能全局 RSS 看门狗或自研分配器配额；建议按"线程分片总 RSS"监控并重启分片。
7. **CJS 层**：实现内存模块表版 `require`（参照官方 modulehandler.rs 示例），模块解析只认白名单 specifier；ESM 用 `MapModuleLoader` 装载玩家模块包。

---

## 5. 与官方 isolated-vm（V8）方案的能力对照

| 能力 | boa_engine 0.22 | isolated-vm / V8 isolate |
|---|---|---|
| 硬中断 | ❌ 无同步 API；budget-async 中止（实证，超调 ns 级）+ loop limit；原生 opcode 盲区（正则 6.9s 实证） | ✅ `TerminateExecution`（其他线程可调用），JIT/解释代码可中断；个别原生路径同样难中断 |
| 内存上限 | ❌ 无 API（thread-local 共享堆，config 私有） | ✅ `Isolate::set_memory_limit`（isolate 级，含近限制回调） |
| 快照/预热 | ❌ 无（但 Context 重建 0.25ms，实测） | ✅ snapshot/creator（毫秒~百毫秒级创建） |
| CPU 计量 | 注入原生函数（µs 源，实证） | 同法注入 + `Isolate::get_heap_statistics` |
| 隔离强度 | 库级：同线程共享 GC 堆；!Send；纯 Rust 内存安全 | isolate 级：每 isolate 独立堆+限额；V8 sandbox（指针压缩+堆外 cage）在演进；C++ 代码本身的 CVE 面更大 |
| 吞吐 | 0.08x V8-JIT（本机 4.2~21.7x 慢） | 1x 基准 |
| 逃逸 CVE 史 | 1 条（panic-DoS，已修） | V8 历年大量 RCE 级 CVE（浏览器场景攻击者多得多） |
| 工具链 | 纯 Rust cargo，交叉编译简单 | 需构建 V8（libclang/node-gyp/预编译二进制），体积大 |
| W^X | 天然无运行时代码生成 | JIT 页可执行（需 --jitless 换性能） |

---

## 附录 A：主要来源
- 源码（本地 clone github.com/boa-dev/boa@39cd112，unshallow 全历史）：`core/engine/src/script.rs:190-217`、`core/engine/src/vm/mod.rs:953-1032`、`core/engine/src/context/mod.rs:252-620`、`core/engine/src/context/hooks.rs`、`core/engine/src/vm/runtime_limits.rs`、`core/engine/src/error/mod.rs:363-372`、`core/engine/src/builtins/array_buffer/shared.rs:23-60,557`、`core/gc/src/lib.rs:47-73,189-198,534`、`core/engine/src/module/loader/mod.rs:148-235`、`core/wintertc/src/lib.rs`
- 官方数据：boajs.dev 数据仓库 github.com/boa-dev/data —— `test262/refs/heads/main/latest.json`（96.01%）、`bench/results/*.json`（每日基准，取 2026-09-27 值）
- crates.io API（版本/发布日期/总下载 508 万）、GitHub API + gh CLI（issue #3442、PR #5380/#5475、#5522、milestones、repo stats）
- OSV API（RUSTSEC-2024-0444 详情）
- 官方示例：`examples/src/bin/{runtime_limits,modulehandler,modules,smol_event_loop,classes}.rs`

## 附录 B：本次实证命令与结果摘要
- 工程：`/tmp/boa-sandbox-test`（boa_engine 0.22 + boa_gc 0.22，release，rustc 1.98.1），源 `src/main.rs`（10 组测试 T1–T10）
- 命令：`cargo build --release && ./target/release/boa-sandbox-test`（输出存 `/tmp/boa_test_output.txt`）
- 结果：T1 注入/计量 ✓；T2a 100ms 死限中止于 100.000125ms、226,236 次让出、中止后 Context 可复用 ✓；T2b 正则盲区 6.9s/0 让出 ✗（预期内的缺口）；T2c loop limit 不可 catch ✓；T3 跨 tick 持久 + Context 重建 0.25ms + eval 2.5µs ✓；T4 SAB 跨 Context 零拷贝 ✓；T5 原生共享缓冲 4 Context ✓；T6 run_jobs 排空微任务 ✓；T7 ES2020+ 16/16 ✓；T8 性能（51.6 vs 12.3ms；71.7 vs 3.3ms；budget 开销 +22%）✓；T9 RSS 28→104→56MB（GC 回收 47MB）✓；T10 ESM 双模块 Fulfilled ✓。`SUMMARY all_core_checks_ok=true`
- 性能对照：`node /tmp/boa_node_workload.js`、`node /tmp/boa_node_fib.js`（node v24.11.0）
- 附加验证：`Context: Send` 编译断言失败（examples/send_check.rs，`Rc<dyn HostHooks>` 不可 Send）
