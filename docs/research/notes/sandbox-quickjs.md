# QuickJS 家族沙箱调研笔记（quickjs-ng / bellard 原版 / rquickjs）

- 调研日期：2026-09-27
- 调研人：QuickJS 调研员（动态工作流子代理）
- 方法：本地克隆三仓库逐行读源码（引用 file:line）+ 本机构建实测（Apple Silicon / darwin 24.6.0 arm64，clang，cargo 1.98.1）+ 官方站点/文档/NVD/OSV 抓取
- 置信级别标注：【文档声称】= 官方文档或 README 自述；【生产验证】= 有已知生产部署证据；【社区经验】= 社区报告未逐一复核；【一手实测】= 本次调研在本机实际运行得到（最强证据）

---

## 0. TL;DR

| # | 硬要求 | 判定 | 关键证据 |
|---|--------|------|----------|
| 1 | CPU 时间计量 | ✅ 满足 | 引擎无内置 CPU 计量，宿主注入原生函数读单调时钟（rquickjs 实测 JS 内可见，纳秒级 Instant）；轮询粒度不影响读数精度 |
| 2 | 硬超时终止 | ✅ 满足 | JS_SetInterruptHandler 抛**不可 catch** 的 InternalError（源码+实测）；轮询间隔实测 ~85µs；宿主与 runtime 均存活可复用（实测） |
| 3 | 内存硬上限 | ✅ 满足（附条件） | JS_SetMemoryLimit 是分配器级硬上限（实测 4MB 上限 runtime 与 36MB runtime 并存互不影响）；但 OOM 可被玩家 catch，需宿主配 OOM 即杀策略 |
| 4 | 按用户持久复用 | ✅ 满足 | runtime+context 长寿命即可；创建成本实测 0.63ms；无 V8 式堆快照，等价机制=模块字节码缓存（JS_WriteObject） |
| 5 | 多模块玩家代码 | ✅ 满足 | ESM 原生（实测含 top-level await）；CommonJS 需宿主自行实现 require 原生函数（引擎内 require 为 undefined，实测） |
| 6 | 全局注入 | ✅ 满足 | rquickjs 实测注入 Rust 原生函数并被 JS 调用；Game/Memory/常量/lodash3 均为普通全局对象注入 |
| 7 | 共享只读大缓冲 | ✅ 满足 | SharedArrayBuffer 默认暴露（两分支实测 typeof=function）；rquickjs 有 SAB::from_source_shared（宿主内存零拷贝）；引擎有 JS_SetSharedArrayBufferFunctions 挂钩 |
| 8 | 语法兼容 ES2020+ | ✅ 满足 | 实测 ES2020→ES2025 特性全过（可选链/私有字段/可变 ArrayBuffer/迭代器 helper 等）；Intl 不支持（官方明确不做） |
| 9 | 执行性能 | ⚠️ 有明确差距 | 本机 bench：比 V8 JIT 慢 3~25x（数值循环 25x、JSON 6x、排序 5x、字符串 2.8x）；bellard 分支比 ng 快 1.4~2x |
| 10 | 安全隔离 | ⚠️ 有条件 | 引擎核心无 IO；runtime 间逻辑隔离实测成立；但是 C 代码，2024-2026 持续有内存安全 CVE（含 ≤0.12.1 的 RCE 级 CVE-2026-37630），进程内多用户需接受引擎 0day 风险或加进程/wasm 兜底 |

**总体判定：有条件可行**——十项中 8 项直接满足，性能（#9）与隔离（#10）需在架构上正视：多数 Screeps 玩家代码在 500ms 预算内可跑完，但重计算玩家明显吃亏；安全上 QuickJS 比 V8 攻击面小得多，但历史 CVE 证明"JS 源码可达内存破坏"真实存在，不可单靠引擎边界跑不可信代码。

---

## 1. 项目现状与活跃度（截至 2026-09-27）

| 项目 | 最新版本 | 发布日期 | 仓库最新提交 | 活跃度结论 |
|------|---------|---------|-------------|-----------|
| quickjs-ng/quickjs | v0.17.0 | 2026-09-18 | 19dbe85, 2026-09-24 | 非常活跃；约每 2 月一版（2026 年已发 0.12→0.17 共 9 版）；40+ 贡献者、400+ PR（官方 docs 自述）；每 PR 跑 50+ 构建配置 + test262【文档声称】 |
| bellard/quickjs | 2026-06-04 | 2026-06-04 | a38171d, 2026-09-26 | 活跃，但单人维护（Fabrice Bellard）；VERSION 文件与官网 tarball 均为 2026-06-04，官网自称该版"bench-v8 比上一版快 42%"【文档声称】 |
| DelSkayn/rquickjs | 0.14.0 | 2026-09-18（crates.io） | 38e02b0, 2026-09-25 | 非常活跃：2026-06 至 09 连发 0.12.1/0.12.2/0.13.0/0.14.0；crates.io 累计下载 457 万；0.13.0 起捆绑 quickjs-ng 0.16.2，master submodule 已指向 v0.17.0 (6d46d07)【一手实测：git log + crates.io API】 |

三仓库本周（9/24-9/26）均有提交，无弃维迹象。

### 1.1 分支关系

- quickjs-ng 是 2023 年从 bellard 原版 fork（官方 README："after it went dormant"——fork 时 bellard 版已 2 年没动）。**但 2024 年起 bellard 本人恢复高频维护**，两分支并行发展并互相回流特性（ng v0.16.0 "Ported Bellard's register-based regexp engine and rope strings"，见 GitHub Releases）。
- rquickjs 默认 submodule 指向 quickjs-ng（.gitmodules: `url = https://github.com/quickjs-ng/quickjs.git`）【一手实测】。
- 两分支均为**纯解释器（computed-goto dispatch + 操作码融合 + 内联缓存），无 JIT**。bellard 仓库全文检索 "JIT" 零命中（Changelog 自 2017 年起无 JIT 条目），官网自述 "Fast interpreter"【一手实测 grep + 官网抓取】。

### 1.2 关键 API 在两分支的实现对比（同源同构）

| 机制 | quickjs-ng (19dbe85) | bellard (a38171d) |
|------|---------------------|-------------------|
| 中断计数器初值 | quickjs.c:556 `JS_INTERRUPT_COUNTER_INIT 10000` | quickjs.c:512 同名同值 |
| 中断抛错 | quickjs.c:8646-8649 `JS_ThrowInterrupted` → **JS_SetUncatchableError** | quickjs.c:7880-7884 同 |
| 内存上限 | quickjs.c:2436-2439 `JS_SetMemoryLimit` → `rt->malloc_state.malloc_limit` | 同 API |
| OOM 抛错 | quickjs.c:8559-8567 `JS_ThrowOutOfMemory` → InternalError（**未**设 uncatchable，可被玩家 catch） | 同 |
| SAB/Atomics | 默认启用（quickjs.c:75-77 `CONFIG_ATOMICS`），quickjs.c:2237 注册 SAB 类、38556 暴露构造器、62418-62699 完整 Atomics 含 wait/notify | 同样默认启用（quickjs.c:72） |
| can_block 开关 | quickjs.h:1189-1190 `JS_SetCanBlock`；quickjs.c:62761 在 js_atomics_wait 入口检查 | 同 |

> 实务结论：两分支在这些沙箱原语上语义一致，以下以 quickjs-ng 为主线，分支差异单独标注。

---

## 2. 必查项一：JS_SetInterruptHandler 触发粒度

**机制（源码，quickjs-ng quickjs.c）**：
- `js_poll_interrupts`（:8666-8672）是内联函数：每次执行到"轮询点"就把 `ctx->interrupt_counter` 减一，减到 0 才真正调用一次宿主 handler 并重置为 10000（`__js_poll_interrupts` :8653-8663）。
- 轮询点覆盖：解释器主循环的**所有跳转类操作码**（OP_goto/goto16/goto8、OP_if_true/if_false，:19158-19195 等）、**每次函数调用入口**（js_call_c_function :18079）、instanceof/原型链行走（:8889）、RegExp 执行、Array.prototype.sort、TypedArray 排序循环（:43591）、String.repeat 等长循环内建函数（多处 `js_poll_interrupts` 调用点）。
- handler 返回非 0 → `JS_ThrowInterrupted` 抛 InternalError "interrupted" 且 **JS_SetUncatchableError**（:8646-8649）→ 玩家 try/catch 无法吞掉，执行立即回退到宿主。

**粒度实测【一手实测】**（clang -O2 链接 libqjs.a，`sandbox_test3.c`，本机 Apple Silicon）：
```
[C] aborted numeric loop after 200.1 ms; interrupt handler called 2352 times => 11756 polls/sec
```
即热循环中宿主 handler 平均每 **~85µs 被调用一次**（另一轮测试 9902 次/秒 ≈ 100µs）。含义：
- **硬超时精度**：以 100ms 预算实测中断发生在 117.2ms / 200.1ms（超预算 0.1~17ms，含异常构造开销），超时误差为亚毫秒~十几毫秒级，可控。
- **对 Game.cpu.getUsed() 精度的影响**：getUsed() 本身是宿主注入的原生函数，每次调用直接读宿主单调时钟（如 `std::time::Instant`，纳秒级），**读数精度与轮询粒度无关**；轮询粒度只影响(a)超时判定的检查频率、(b)若宿主选择在中断回调里累计 CPU 值的更新粒度。推荐实现：getUsed() 每次调用现算 `start.elapsed()`，中断 handler 只做越界判断。
- **盲区**：轮询是协作式的——单次宿主原生 C 调用（你自己注入的寻路函数）执行期间不会被中断；`Atomics.wait` 会阻塞线程且不轮询。前者要求宿主原生函数自身短小有界，后者用 `JS_SetCanBlock(rt,false)` 直接让 Atomics.wait 抛错（quickjs.c:62761）。**注意 rquickjs 高层 API 未包装 can_block，需走 rquickjs-sys 裸绑定**（grep core/src 无 SetCanBlock，仅 sys/src/bindings/*.rs 有声明）。
- 中断 handler 挂在 **JSRuntime**（rt->interrupt_handler），计数器在 **JSContext**——一个 runtime 多 context 共享同一 handler。

---

## 3. 必查项二：JS_SetMemoryLimit 语义（超限后的状态）

**机制（源码）**：`JS_SetMemoryLimit` 只设置 `rt->malloc_state.malloc_limit`（quickjs.c:2436-2439）。引擎自有内存记账（malloc_size），每次分配检查超限则返回 NULL，触发 `JS_ThrowOutOfMemory`（:8559-8567）→ InternalError "out of memory"。与之配合的 GC 阈值是另一个旋钮 `JS_SetGCThreshold`（js_trigger_gc :1680-1700：超过 `malloc_gc_threshold` 时先跑一轮 GC 再分配）。

**实测语义【一手实测】**（`sandbox_test3.c`，quickjs-ng v0.17.0 构建）：
```
[B1] A (4MB limit) alloc loop -> OK[caught OOM at 1107]      ← 分配在第 1107 次被硬性拒绝，上限生效
[B2] B (no limit, 300k objects) -> OK[300000]                ← 另一 runtime 完全不受影响
[B3] A tiny eval after OOM -> OK[42]                          ← OOM 后 runtime 仍可执行小段代码
[B4] B memory_used after 300k objs: 36086788 bytes            ← B 实际用了 36MB，证明按 runtime 独立记账
```
另一次测试（sandbox_test.c）：OOM 后 `keep=null` + JS_RunGC，再分配 10 万对象仍失败（malloc_size 贴着上限未显著回落）。

**语义总结**：
1. **硬上限**：引擎经其分配器的每一字节都计入，超限即分配失败——比 isolated-vm 的 memoryLimit（官方自述"more of a guideline"，恶意代码可超 2-3 倍才被杀）更硬。
2. **OOM 异常可被玩家 catch**（[B1] 中玩家 catch 到了 "out of memory"）——内存上限**不会**强制终止执行。玩家无法借此突破上限（每次分配仍被拒），但可以继续空转烧 CPU。⇒ 宿主策略：在中断 handler 里检测"近期发生过 OOM"或直接 OOM 即销毁重建 runtime。
3. **超限后 runtime 处于退化状态**：小操作可用，大分配可能持续失败（GC 回收不彻底的残留）。官方与社区一致建议：**OOM 后销毁 runtime 重建**（重建成本实测 0.63ms，可接受）。此处"OOM 后重建"为【社区经验】+ 与实测现象一致。
4. 不经引擎分配器的宿主原生分配（Rust 侧结构体等）不计入上限——注入的原生函数内存需宿主自行限额。

---

## 4. 必查项三：ES2023+ 兼容矩阵

**官方声称**：
- quickjs-ng docs（es_features.md）："aims to support the latest available ECMAScript features once they hit the spec"，test262 结果指向 test262.fyi（该站是纯前端应用，本次无法抓取具体百分比——**未能取得官方 test262 通过率数字**，标注为未取得）。diff.md 列举已具备特性：Resizable ArrayBuffer、Float16Array、WeakRef、FinalizationRegistry、Iterator Helpers、Promise.try、Error.isError、Set operations、**V8 stack trace API（Error.captureStackTrace/prepareStackTrace/stackTraceLimit）**——后者对 Screeps 玩家代码兼容性是加分项。
- bellard 官网（bellard.org/quickjs，2026-09-27 抓取）："supports the ES2025 specification… Passes nearly 100% of the ECMAScript Test Suite tests"（ES2025 特性层面）；libregexp "fully compliant with ES2023"。2025-09-13 版加了 RegExp v flag/modifiers/escape、Float16Array、Promise.try；2026-06-04 版加了 resizable ArrayBuffer、ArrayBuffer.transfer、Iterator 对象与方法、Set methods、Math.sumPrecise、Uint8Array base64/hex。以上均为【文档声称】。
- Intl：两分支官方均明确**不支持**（quickjs-ng es_features.md："unlikely that QuickJS will ever support Intl"）。

**本机实测【一手实测】**（`es_check.js`，quickjs-ng v0.17.0 自建 vs bellard 2026-06-04 自建 vs node v24.11.0[V8] 对照，完整输出见附录）：

| 特性 | quickjs-ng v0.17.0 | bellard 2026-06-04 | node 24 (V8) |
|------|-------------------|--------------------|--------------|
| ES2020 可选链/nullish/BigInt/allSettled/globalThis | PASS | PASS | PASS |
| ES2021 逻辑赋值/replaceAll/数字分隔符 | PASS | PASS | PASS |
| ES2022 类字段/私有字段/#field/Array.at/hasOwn/Error cause | PASS | PASS | PASS |
| ES2023 findLast/toReversed/toSorted | PASS | PASS | PASS |
| ES2024 Object.groupBy/Promise.withResolvers/resizable AB | PASS | PASS | PASS |
| ES2024 RegExp v flag | 语法通过（与 V8 行为一致） | 语法通过 | 通过 |
| ES2025 Iterator helpers/Set union/Promise.try | PASS | PASS | PASS |
| SharedArrayBuffer / Atomics.wait / Atomics.pause | function/全有 | function/全有 | function/全有 |
| top-level await（ESM） | 实测通过 | 实测通过 | 通过 |
| Intl | **undefined（不支持）** | **undefined** | 有 |

**Screeps 适配结论**：async/await、可选链、私有字段等玩家常用语法全部覆盖，ES2023 矩阵远超需求（实测到 ES2025）；唯 Intl 缺失——官方 Screeps（Node/V8+ICU）有 Intl，迁移的玩家代码若用了 Intl（如 toLocaleString 排序）会有差异，需文档声明或宿主注入 polyfill。

---

## 5. 必查项四：与 V8 的性能差距（量化）

### 5.1 本机基准【一手实测】

方法：同一 `bench.js`（每项预热一次再计时），quickjs-ng v0.17.0（cmake Release）与 bellard 2026-06-04（make，-O2）对比 node v24.11.0（V8 JIT）。机器：Apple Silicon (arm64)。

| 基准 | quickjs-ng | bellard | V8(node24) | ng/V8 | bellard/V8 |
|------|-----------|---------|-----------|-------|-----------|
| 数值循环 sum 5e7 | 995 ms | 681 ms | 40 ms | **24.9x** | 17.0x |
| 对象创建/销毁 2e6 | 464 ms | 233 ms | 3 ms | ~155x* | ~78x* |
| 多态属性访问 5e6 | 306 ms | 200 ms | 13 ms | 23.5x | 15.4x |
| JSON 往返 2e4×50 | 1395 ms | 852 ms | 221 ms | 6.3x | 3.9x |
| 字符串拼接 2e5 | 25 ms | 16 ms | 9 ms | 2.8x | 1.8x |
| 数组排序 2000×300 | 139 ms | 103 ms | 29 ms | 4.8x | 3.6x |

\* 对象一项 V8 3ms 跑完 200 万次分配是**逃逸分析把分配整体消除了**，倍数虚高，仅作参考；其余项为真实差距。

结论（本机数据）：
- **解释器 vs JIT 的真实差距：计算密集 15~25x，库函数密集（JSON/排序）4~6x，字符串 2~3x**。几何平均约一个数量级。
- **bellard 2026-06-04 比 quickjs-ng v0.17.0 快 1.4~2x**（与本机 bench 全部六项一致），与 bellard 官网"bench-v8 快 42%"及 2026-06-04 changelog（自定义小块 malloc + 微优化）方向一致【文档声称+一手实测互证】。选型提示：若纯论解释吞吐，bellard 版目前占优；但 ng 的工程化（测试矩阵、发布节奏、安全响应、Windows/MSVC）更好。

### 5.2 第三方量化佐证

- **AWS LLRT**（awslabs/llrt，README）：自称"up to over 10x faster startup and up to 2x overall lower cost"（Lambda 场景），同时**明确承认**"many cases where LLRT shows notable performance drawbacks compared with JIT-powered runtimes"——大数处理、蒙特卡洛、数十万至百万次迭代的工作负载【文档声称，AWS 官方】。与我的 15-25x 计算密集差距数据吻合。
- **bellard bench-v8**：官方自述 2026-06-04 版比上一版快 42%（自家基准，未提供绝对值）【文档声称】。
- 未找到独立的、大规模的 QuickJS vs V8 第三方基准报告（web 搜索配额受限 + test262.fyi 无性能数据）——**标注：此项以本机实测为主要证据**。

### 5.3 GC 压力

QuickJS 为**引用计数 + 环回收器（mark-sweep，JS_RunGC）**，无分代、无 JIT 代码内存。对象析构随引用归零即时发生（无 V8 那种延迟扫描），`malloc_gc_threshold`（JS_SetGCThreshold / rquickjs set_gc_threshold）控制触发节奏；LLRT 默认 GC 阈值 20MB（README）。大量小对象创建/销毁实测 2e6 次 233~464ms，无长暂停现象（本机 bench 观测）；引用计数的代价是写屏障式的增减开销，含在上述基准内【一手实测】。

---

## 6. 十项硬要求逐条详证

### #1 CPU 时间计量 — 满足
- 引擎无内置 per-runtime CPU 时间 API（JS_ComputeMemoryUsage 只管内存，quickjs.h:590-607）。
- 标准做法：宿主在 tick 开始时记 `Instant`，注入原生函数 `Game.cpu.getUsed = || meter.start.elapsed()`。rquickjs 实测（rq-test）：
  ```
  [interop] pf=45 cpuVisible:true   ← JS 内两次调用 cpuUsed() 可见时间推进
  ```
  【一手实测】精度=宿主时钟精度（Instant 纳秒级），成本=每次调用一次原生 FFI（远小于 1µs 级）。
- 若要真"CPU 时间"而非墙钟：单线程专用 worker 跑该用户 runtime 时两者等价；或 `clock_gettime(CLOCK_THREAD_CPUTIME_ID)`。
- LLRT 同模式提供 performance.now【生产验证（AWS 官方项目）】。

### #2 硬超时终止 — 满足（附两个工程条件）
实测（C 与 rquickjs 双份）：
```
[A] try/catch vs interrupt -> EXCEPTION[InternalError: interrupted]   ← catch 不会吞掉
[A2] small eval after interrupt -> OK[42]                              ← runtime 立即可复用
[timeout] infinite-loop eval -> err: Some("Exception generated by QuickJS")   (rquickjs，100ms 预算)
[reuse-after-timeout] 2
```
【一手实测】宿主进程从未崩溃。条件：(a) `JS_SetCanBlock(false)` 禁 Atomics.wait（quickjs.c:62761）；(b) 宿主注入的原生函数必须自身有界（原生调用期间不轮询）。另注意 handler 是持续生效的——**每个 tick 必须重置 deadline**，否则上一 tick 的过期期限会杀死本 tick 的第一个循环（rquickjs 实测中复现该现象）。

### #3 内存硬上限 — 满足（附 OOM 策略条件）
见第 3 节。要点：上限是分配器级硬限制；按 runtime 独立记账互不影响（实测 [B1][B2][B4]）；OOM 异常可被玩家 catch ⇒ 宿主要么在中断回调中检测 OOM 计数、要么 OOM 即重建 runtime（重建 0.63ms）。rquickjs：`Runtime::set_memory_limit(256<<20)`（core/src/runtime/base.rs:158）直接透传。

### #4 按用户持久复用 — 满足（等价机制，非 V8 快照）
- **长寿命 runtime**：JSRuntime/JSContext 跨 tick 保存，玩家 global 对象天然存活（与官方 Screeps 的 Memory/global 语义一致）。
- **创建成本**：rquickjs 实测 `Runtime::new()+Context::full()` = **0.633ms**（含 600 个内建 atom、全部内建对象初始化）——即使"每 tick 重建"也在预算内，但为保留玩家 global 仍应持久化。
- **字节码缓存（等价 snapshot）**：`JS_WriteObject(JS_WRITE_OBJ_BYTECODE)`（quickjs.h:1261-1266）可把**编译后的函数/模块**序列化，`JS_ReadObject` 回读，省去每 tick 的 parse+compile。rquickjs 封装：`Module::write`（core/src/value/module.rs:448-473）与 bytecode 声明加载（:335-345）。
- **没有 V8 式堆快照**（不能序列化整个运行时堆/全局对象图）；JS_WRITE_OBJ_REFERENCE 可编码对象图但有安全限制。等价预热手段：预创建 runtime 池 / Golem 式 wasm+Wizer 全量快照（见生产案例）。
- ⚠️ **安全红线**：quickjs-ng SECURITY.md 明文："Loading untrusted bytecode (e.g. via JS_ReadObject with JS_READ_OBJ_BYTECODE) is equivalent to executing untrusted native code"——**只允许缓存宿主自己从玩家源码编译出的字节码，绝不能接受玩家上传的字节码**。rquickjs 遵循同一语义（module.rs:338 "User must ensure that bytes handed to this function contain valid bytecode"）。
- rquickjs Runtime/Context 实现 Send（base.rs:232-242；`parallel` feature 下 Sync）——可跨线程挪动以做 per-user runtime 池调度；但同一时刻的调用受内部 RefCell 互斥（见第 9 节坑 1）。

### #5 多模块玩家代码 — 满足
- ESM：引擎原生支持，实测 `import {helper} from './mod_a.mjs'` 与 top-level await 两分支均通过；模块加载器回调（js_module_loader）由宿主接管——rquickjs 提供 Resolver/Loader trait 组合（BuiltinResolver/FileResolver/ScriptLoader/NativeLoader，examples/module-loader 实测可编译运行）。
- CommonJS：引擎**无内建 require**（实测 `typeof require === "undefined"`）。官方 Screeps 本来就是自定义 require 包一层模块缓存；宿主注入原生 `require(name)` + 预编译模块表即可复刻（LLRT 在 quickjs 上实现了整套 require）【生产验证：LLRT】。
- 相对 require/模块隔离（各玩家模块互不可见）由宿主 loader 决定，无引擎障碍。

### #6 全局注入 — 满足
- rquickjs：`globals().set("nativePathfind", Func::from(fn))` 实测注入 Rust 函数并从 JS 调用（`[interop] pf=45`）【一手实测】；`Function::new(ctx, closure)` 支持闭包捕获 Rust 状态。
- Game/Memory/RawMemory/常量表 = 普通 JS 对象/冻结对象注入；lodash 3.x 是纯 ES5 库，直接 eval 进全局即可（官方 Screeps 即注入 lodash 3.x）。
- Rust/C++ 原生寻路：注入原生函数或带方法的 Class（rquickjs class 宏），或在 wasm/进程外跑。交叉数据传递用 ArrayBuffer/SAB 零拷贝（见 #7）。
- 注意：`Context::full` 会注入全部内建（含 FullIntrinsics）；不链 quickjs-libc 则无 std/os 模块（rquickjs 核心不绑定 libc 模块，`grep quickjs-libc core/` 无结果）。

### #7 共享只读大缓冲 — 满足
- SharedArrayBuffer 两分支默认暴露（实测 typeof=function，`CONFIG_ATOMICS` 默认启用：ng quickjs.c:75-77，bellard quickjs.c:72）。
- 引擎钩子：`JS_SetSharedArrayBufferFunctions`（ng quickjs.c:2482-2484）允许宿主自定义 SAB 的 alloc/free/clone——可让 SAB 直接背书宿主内存，多个 runtime 共享同一份地形表而引擎只存引用（字节码层 JS_WRITE_OBJ_SAB 支持序列化共享，quickjs.h:1263）。
- rquickjs：`SharedArrayBuffer::from_source_shared(ctx, src)`（core/src/value/array_buffer.rs:191-198）从宿主拥有的字节创建 SAB（from_external，零拷贝，析构回调归还所有权）【文档声称(docs.rs/源码注释)+可编译验证，未跑大表压测】。
- 只读策略：注入 `new Uint8Array(sab)` 视图 + 宿主侧保证只写一次；或用 array_buffer.rs:200+ 的 immutable-ArrayBuffer 构造。
- 替代零拷贝：`ArrayBuffer::from_source`（普通 AB 外部内存）。

### #8 语法兼容 — 满足
见第 4 节实测矩阵。微任务排空：宿主循环 `JS_ExecutePendingJob`（quickjs.h:1252）直到返回 0——rquickjs `Runtime::execute_pending_job`（base.rs:216）实测排空 promise 微任务：
```
[microtask] microtask ran: 42 (jobs executed: 1)
```
【一手实测】单 tick 内同步跑完 + 微任务排空的 Screeps 模型可直接实现。Intl 缺失为已知差异。

### #9 执行性能 — 有条件满足（差距已量化）
见第 5 节：计算密集 15~25x、库函数 4~6x 慢于 V8 JIT；对 500ms CPU 预算的影响——典型 Screeps 玩家逻辑（遍历几百个 creep、调原生寻路）每 tick 实际 JS 执行量小（官方服玩家平均 CPU 用量远低于预算），解释器可承受；重度玩家（纯 JS 寻路/A* 大图）会明显吃亏，需引导用注入的原生寻路。内存占用远小于 V8 isolate（无 JIT 代码缓存），LLRT 声称冷启动快 10x+。

### #10 安全隔离 — 有条件满足（需接受或加固）
正面：
- 引擎核心**不含任何 IO**（文件/网络仅存在于可选的 quickjs-libc，宿主不注入即无）；rquickjs Context::full 亦无 IO。
- runtime 间隔离实测成立（[B1][B2]：A 限 4MB OOM，B 同时用 36MB 无扰）。
- 无 JIT ⇒ 无 JIT-spray/JIT 编译器攻击面；代码量 ~7 万行 C，远小于 V8。
负面（必须正视）：
- **C 引擎的内存安全历史**：NVD 检索 quickjs 相关 CVE 20 条（2020-2026 持续）；quickjs-ng 专属 10 条，其中 **CVE-2026-37630（quickjs-ng ≤0.12.1，js_mapped_arguments_mark，"execute arbitrary code"）与 CVE-2026-3979（≤0.12.1，js_iterator_concat_return UAF）均为 JS 源码可达的内存破坏**【一手检索 NVD API，2026-09-27】；OSS-Fuzz 累计 31 条 OSV 记录（2025 年 17 条、2026 至今 2 条，多为 regexp/解析器内存错误）。
- **字节码反序列化 = 原生代码执行**（SECURITY.md 明示，见 #4）。
- rquickjs 自身的 Rust 健全性问题：0.13.0 修复了 TypedArray/ArrayBuffer 安全借用漏洞（#736，CHANGELOG）；经 SurrealDB 暴露的 CVE-2024-58366（rquickjs Exception::throw_type 格式串）说明**绑定层的错误路径同样可成为攻击面**。
- 结论与建议：同进程多用户 runtime 依赖"引擎+绑定当前无已知漏洞"这一前提，历史上该前提被反复打破。加固选项分层：①保持引擎及时升级（ng 补丁频繁）；②每用户独立 OS 进程/线程 + 地址空间隔离（rquickjs Send 支持调度）；③ quickjs 编译为 wasm 跑在 wasm 边界内（Golem 模式）——牺牲约 2-4x 性能换取硬边界。xxscreeps 用 V8 isolate 也面临同构风险（V8 CVE 更多但更新更快），并非 QuickJS 独有。

---

## 7. 生产案例

| 案例 | 用法 | 证据级别 |
|------|------|---------|
| **AWS LLRT**（awslabs/llrt，8.8k stars） | QuickJS(ng)+Rust 的 Lambda 运行时；自称快 10x 启动/省 2x 成本；README 明确"evaluation purposes"、无具名生产客户；无 JIT，承认重计算劣势 | 【生产验证：AWS 官方实验性产品，生产部署未见具名案例】 |
| **SurrealDB** | 嵌入 rquickjs 提供 scripting（--allow-scripting，默认关闭）；2024 年因此出 CVE-2024-58366（格式串）、2026 年 CVE-2026-63762（引擎 DoS）——反向证明其在生产数据库中真实承载不可信脚本 | 【生产验证：NVD CVE 记录+官方公告】 |
| **Golem**（golem.cloud） | TS/Scala 代理编译为 JS 跑在 **wasm-rquickjs**（quickjs 编进 wasm 组件），用 **Wizer 预初始化快照**加速启动；不透明持久化由 wasm 级 snapshot 实现 | 【生产验证：官方文档 learn.golem.cloud "Golem JavaScript Runtime (QuickJS)"（2026-09-27 抓取）】 |
| **CSPro**（美国人口普查局调查处理软件） | 嵌 quickjs-ng 执行调查逻辑 JS | 【生产验证：quickjs-ng 官方 projects.md 收录】 |
| 其他 | txiki.js（ng 作者 saghul 维护的运行时）、radare2、GodotJS、nx.js（Switch）、edbrowse、LLVM 生态若干绑定 | 【文档声称：projects.md】 |

> Screeps 类负载的直接先例：无公开的"多用户同进程 quickjs 沙箱游戏服"；最接近的是 SurrealDB（数据库内嵌多租户脚本）与 Golem（wasm 内 quickjs 跑用户代理）。

## 8. CVE / 沙箱逃逸历史汇总

- **NVD "quickjs"**（含两分支）：20 条（2020-2026），样本：CVE-2023-48184（JS_FreeValueRT UAF）、CVE-2025-62494（字符串加法类型混淆）、CVE-2025-62495（regexp 整数溢出）等【NVD API 2026-09-27】。
- **quickjs-ng 专属**：10 条；最近：CVE-2026-37630（≤0.12.1 RCE 级）、CVE-2026-3979（≤0.12.1 UAF）、CVE-2026-63762（DoS，与 SurrealDB 关联）。
- **rquickjs**：2 条（CVE-2024-58366 格式串；CVE-2026-63762 同上）。
- **OSS-Fuzz/OSV**：31 条（quickjs 项目持续接入 fuzzing；2025 年 17 条源于 fuzz 覆盖扩大与 regexp 引擎移植期）。
- **官方安全边界声明**：quickjs-ng SECURITY.md——威胁模型为"不可信 JS 源码 + 可信嵌入器"；**字节码硬ening 永远 out of scope**。
- 未发现公开的"QuickJS 沙箱逃逸 write-up"（越权拿宿主权限的完整链）；现存问题均为内存破坏类，理论可利用性未公开验证【社区经验：未见公开利用】。

## 9. rquickjs 绑定质量评估（0.14.0）

**优点**：
- API 覆盖完整：set_memory_limit / set_max_stack_size / set_gc_threshold / set_interrupt_handler / memory_usage / execute_pending_job / set_loader / promise rejection tracker / 自定义分配器（base.rs:50-216 逐行核对）。
- 模块系统：Resolver/Loader trait、字节码 Module::write/read（module.rs:335,448）。
- 并发：`unsafe impl Send/Sync for Runtime`（base.rs:232-242，`parallel` feature），RefCell+锁保护内部状态；0.13.0 修复 parallel 下 drop 死锁。
- 维护强度：2026 年 4 个小版本；安全修复有响应（#736）；crates.io 累计 457 万下载、0.12.2 单版本 40 万。
- 捆绑并跟进 quickjs-ng 上游（0.13.0 → ng 0.16.2；master → ng v0.17.0）。

**坑（本次实测/读码发现）**：
1. **重入 panic**：在 `ctx.with(|ctx| ...)` 闭包内调用 `rt.set_interrupt_handler(...)`/`rt.execute_pending_job()` 会 panic "RefCell already borrowed"（rquickjs-core-0.14.0/src/safe_ref.rs:35，实测复现）——Runtime 方法必须在 ctx.with 外调用。属于文档不足的 API 约束。
2. 高层未包装 `JS_SetCanBlock`（禁 Atomics.wait 需走 rquickjs-sys 裸 API）。
3. 中断异常信息呈现为泛化的 "Exception generated by QuickJS"（uncatchable 异常细节需自取）。
4. `full-async` 之外的异步调度需自带 executor（schedular/spawner 模块较新）。

**替代绑定**（未实测）：quickjs-rusty、HiRoFa/quickjs_es_runtime、quickjs-go 等（ng projects.md 收录）；Rust 侧选择 rquickjs 是事实标准。

## 10. 与官方 isolated-vm（V8 路线）对照

isolated-vm（laverdet/isolated-vm，Screeps 官方 server 即用它）：**维护模式**（README 自述 maintenance mode，重写版实验性不可用）；Node 20+ 需 `--no-node-snapshot` 启动；用户含 Screeps、Fly.io、Algolia、Tripadvisor【一手抓取 README，2026-09-27】。

| 维度 | QuickJS(ng)+rquickjs | isolated-vm (V8) |
|------|---------------------|------------------|
| 宿主语言 | Rust 进程内（c 无需 Node） | Node C++ addon（需 Node 16+，编译） |
| 执行性能 | 解释器，慢 15-25x（计算密集） | V8 JIT 全速 |
| 内存上限 | 分配器硬上限（实测），OOM 可 catch 需宿主策略 | memoryLimit 为"guideline"，恶意可超 2-3x（README 自述） |
| 硬超时 | interrupt 轮询 ~85µs 粒度，uncatchable（实测） | 每操作 timeout 选项，V8 取消式 |
| 快照/预热 | 模块字节码缓存；无堆快照；runtime 创建 0.63ms | createSnapshot 存在但 README 警告"不应使用"（新版 V8 不稳定，含机器码） |
| 共享内存 | SAB 原生 + 宿主 SAB 钩子 | SAB 引用共享（跨 isolate 传引用） |
| 攻击面 | ~7 万行 C，CVE 少而持续存在 | V8 巨大攻击面，CVE 多但补丁快；JIT 类攻击面 |
| 单实例开销 | ~MB 级、创建亚毫秒 | isolate 数 MB+、创建毫秒级 |
| 维护 | 三项目均本周有提交 | 维护模式 |

## 11. Rust 宿主按用户持久复用的推荐实践

1. **每用户一个 JSRuntime+JSContext**，常驻（玩家 global/Memory 跨 tick 存活，对齐官方 Screeps 语义）；用 `runtime.set_info(user_id)` 标记便于诊断。
2. **tick 流程**：重置 interrupt deadline → 注入/刷新 Game、常量 → `ctx.with(|ctx| run main loop)` → 排空微任务（execute_pending_job 循环）→ 收集结果。rquickjs 的 Runtime 方法全部在 ctx.with 之外调用（避坑 1）。
3. **CPU**：deadline 用 `Arc<AtomicU64>` 存每 tick 截止毫秒，interrupt handler 读它；getUsed() 现算 elapsed。
4. **内存**：set_memory_limit(256MB)+set_max_stack_size(512KB)+set_gc_threshold(20MB 起)；OOM 计数≥1 即销毁重建 runtime（重建 <1ms）。
5. **模块**：玩家源码 → 宿主编译 → 缓存字节码（key=user+code hash）；代码变更才重编译。ESM loader 用自定义 Resolver 拒绝访问其他用户命名空间；CommonJS 提供宿主 require。**绝不接受玩家侧字节码**。
6. **锁死**：`JS_SetCanBlock(false)`（rquickjs-sys 裸调）；不注入 fetch/fs/net；TypedArray 外部内存只读。
7. **地形共享**：一份 `Arc<Vec<u8>>`/mmap → 每用户 `SharedArrayBuffer::from_source_shared` 视图（或引擎 sab_funcs 全局钩子）。
8. **池化与回收**：idle 用户的 runtime 定期 JS_RunGC+LRU 销毁；rquickjs Runtime: Send 可跨线程迁移调度（parallel feature 下多线程各持锁串行）。
9. **可选硬隔离**：高敏部署可用 quickjs 编译到 wasm（wasmtime）跑用户代码，宿主进程只暴露 host-fn——Golem 已验证该路线（含 Wizer 预热快照）。

## 12. 未尽事项（如实声明）

- quickjs-ng 的官方 test262 通过率数字未能取得（test262.fyi 为纯前端站点无法抓取；本地未跑全量 test262——仓库 test262/ 子模块为空，全量套件未在本次时间预算内获取）。以本机 26 项特性实测 + 官方"nearly 100%"自述代替。
- LLRT 的绝对基准数字（ms/MB 表格）在 README 中以图片存在，未能解析；仅取得其自述倍数。
- rquickjs 的 SAB 大表共享未做压测（仅源码级确认 API 存在并编译通过）。
- 性能对比为单机微基准（Apple Silicon），非服务器级硬件、非长时负载。

---

## 附录 A：本次实际执行的命令与关键输出

```bash
# 克隆（均成功，均为本周提交）
git clone --depth 1 https://github.com/quickjs-ng/quickjs.git   # 19dbe85 2026-09-24
git clone --depth 1 https://github.com/bellard/quickjs.git     # a38171d 2026-09-26
git clone --depth 1 https://github.com/DelSkayn/rquickjs.git   # 38e02b0 2026-09-25

# 构建
cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build -j8   # quickjs-ng
make -j8 qjs                                                          # bellard 2026-06-04

# 沙箱原语 C 实测（sandbox_test3.c，输出见正文第 2/3 节）
clang -O2 sandbox_test3.c -o sandbox_test3 -Iquickjs-ng quickjs-ng/build/libqjs.a -lm && ./sandbox_test3

# ES 特性 + 基准（输出见第 4/5 节）
./quickjs-ng/build/qjs es_check.js ; node es_check.js
./quickjs-ng/build/qjs bench.js     ; ./quickjs-bellard/qjs bench.js ; node bench.js

# rquickjs 0.14 实测（输出见第 6 节引文）
cd rq-test && cargo run --release   # rquickjs = { version = "0.14", features = ["full-async","loader"] }

# CVE/版本数据
curl NVD API keywordSearch=quickjs / quickjs-ng / rquickjs（2026-09-27）
curl -s -X POST https://api.osv.dev/v1/query  (OSS-Fuzz quickjs: 31 条)
curl -s -A xscreeps-research https://crates.io/api/v1/crates/rquickjs/versions  # 0.14.0 2026-09-18
```

## 附录 B：关键源码索引（quickjs-ng @19dbe85）

- 中断：quickjs.c:556（计数器 10000）、:8646-8649（uncatchable）、:8653-8672（轮询）、:19158-19195（跳转操作码轮询）、:18079（函数调用入口轮询）
- 内存：quickjs.c:2436（SetMemoryLimit）、:8559-8567（ThrowOutOfMemory）、:1680（js_trigger_gc）
- Atomics/SAB：quickjs.c:75-77（CONFIG_ATOMICS 默认开）、:2237/:38556（SAB 注册/暴露）、:2482（SetSharedArrayBufferFunctions）、:62418-62699（Atomics 实现）、:62761（can_block 检查）
- 字节码：quickjs.h:1261-1280（flags+不可信字节码警告）、:1038（JS_Eval 输入需 NUL 结尾，**不支持 len=-1**——本次实测传 -1 会因 buf_end 回绕报 "unexpected end of string"，是嵌入者常见坑）
- 微任务：quickjs.h:1252（JS_ExecutePendingJob）
- rquickjs：core/src/runtime/base.rs:124/158/167/174/200/216/232-242、core/src/value/module.rs:335/448、core/src/value/array_buffer.rs:191-198、sys 子模块指向 quickjs-ng
