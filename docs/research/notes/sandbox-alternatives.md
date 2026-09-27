# Screeps 沙箱备选方案横向扫描（wasmtime+Javy / rusty_v8 裸绑 / isolated-vm Node 边车 / 2025-2026 新方案）

> 调研日期：2026-09-27。负责人：备选方案调研员（沙箱系列之横向扫描）。
> 姊妹笔记：`docs/research/01-screeps-rust-feasibility.md`（总体可行性，其中 §4.1 有两处结论被本次调研修正，见 §0）。
> 置信级别标注：【实测】= 本会话在本机（macOS arm64, darwin 24.6.0, node v24.11.0, rustc 1.98.1）实际执行验证；【文档】= 官方文档/README/docs.rs/release notes；【社区】= issue/博客/非官方来源；【推断】= 基于证据的推理，未直接验证。

---

## 0. 执行摘要与两处对既有报告的修正

| 候选 | 十项硬要求判定 | 一句话结论 |
|---|---|---|
| **A. wasmtime + Javy（JS→WASM）** | 7/10 满足，性能/共享缓冲/原生模块化是硬缺口 | 语义兼容性比传闻好（**eval 实测可用**），安全与硬限最强，但吞吐慢 ~28x、无 SAB、CommonJS 需预打包；不适合做主力玩家沙箱，适合做"极限内存档"或工具侧沙箱 |
| **B. rusty_v8 裸绑（不经 deno_core）** | 10/10 能力满足，全部 API 已核实 + 冒烟实测通过 | 能力上与 deno_core 等价（同一引擎），代价是自建全部运行时设施 + 承受 v152 API 大改式的维护税；仅在需要极致控制（自定义计量/内存布局）时值得 |
| **C. isolated-vm 放进 Node 边车进程** | 9.5/10 满足（跨进程零拷贝地形是唯一降级点） | 官方引擎的原班方案 + README 明文推荐的部署形态；进程级隔离最彻底；2026-08 刚修过一个 **critical** 级 guest→host 逃逸（GHSA-864f），必须用 ≥7.0.1 且持续跟进 |
| **D. 新方案（rquickjs/quickjs-ng、Boa、StarlingMonkey、LLRT、celld 等）** | rquickjs ~6/10；Boa 更少；StarlingMonkey 未实测 | rquickjs 是唯一值得保留观察的低内存档；Boa 上生产还早；StarlingMonkey 是 Javy 的潜在替补（SpiderMonkey 完整度更高）；celld/Deno Sandbox 是"V8 隔离即服务"的形态参考 |

**修正 1（重要）**：`01-screeps-rust-feasibility.md` §4.1 表格称 wasmtime+Javy "eval/new Function 受限，不兼容"。**实测推翻**：Javy v9.1.0 编译的模块里 `eval("6*7")===42`、`new Function("return 11*11")()===121` 均正常（见 §2.3，QuickJS 解释器连同 parser 一起编进了 wasm）。真正的缺口是性能、SAB、CommonJS 与调用约定，而不是动态求值。
**修正 2**：同表称 deno_core 为可选主力。本次查明 deno_core 的 GitHub 仓库确于 2026-02-27 归档，但**只是并入 denoland/deno monorepo**，crate 本身仍在活跃发版（0.412.0，2026-09-16，crates.io API 本会话查询）——"deno_core 已死"的说法不成立；该结论属于姊妹调研范围，此处仅作交叉信息修正。

**本机实测清单**（全部在本会话执行，可复现）：
1. Javy v9.1.0 + wasmtime v49.0.1：eval/new Function/可选链/BigInt/Promise 微任务/硬超时/内存硬限/CommonJS 报错/性能对照（§2）。
2. rusty_v8 v152.2.0 裸绑 spike：heap_limits、host 函数注入、ES2020 语法、Explicit 微任务策略、thread_safe_handle 终止死循环（§3.3）。
3. isolated-vm@7.0.1（node v24.11.0 源码编译安装）：runSync 超时、memoryLimit、cpuTime、SAB 跨 isolate、snapshot、持久复用、ESM 模块（§4.3）。

---

## 1. 版本与活跃度总表（GitHub/crates.io/npm API，2026-09-27 查询）

| 项目 | 最新版本 | 发布时间 | 仓库最后推送 | stars | 状态 |
|---|---|---|---|---|---|
| bytecodealliance/wasmtime | v49.0.1 | 2026-09-24 | 2026-09-26 | 18,659 | 极活跃 |
| bytecodealliance/javy | v9.1.0 | 2026-07-30 | 2026-09-02 | 2,752 | 活跃 |
| denoland/rusty_v8 | v152.2.0（V8 15.2） | 2026-08-20 | 2026-09-26 | 3,950 | 极活跃（跟随 V8 mainline） |
| crates.io/v8（rusty_v8 crate） | 152.2.0 | 2026-08-20 | — | — | 活跃 |
| laverdet/isolated-vm | npm 7.0.1（GitHub tag v6.0.2 之后走 npm 发版） | npm 2026-08-05 | 2026-09-26 | 2,929 | 活跃（v5 2024-05 → v6 2025-05 → v7 2026-05） |
| denoland/deno_core | crate 0.412.0 | 2026-09-16 | repo 已并入 deno monorepo | — | 活跃（仅仓库迁移） |
| quickjs-ng/quickjs | v0.17.0 | 2026-09-18 | 2026-09-24 | 3,813 | 极活跃 |
| DelSkayn/rquickjs | crate 0.14.0 | 2026-09-18（crates.io） | 2026-09-25 | 1,019 | 活跃（现绑定 quickjs-ng） |
| boa-dev/boa | v0.22 | 2026-08-28 | 2026-09-26 | 7,569 | 活跃 |
| awslabs/llrt | v0.9.0-beta | 2026-09-06 | 2026-09-25 | 8,804 | 活跃（自称 experimental） |
| bytecodealliance/StarlingMonkey | —（无 release tag 查询） | — | 2026-09-24 | 289 | 活跃 |
| denoland/celld | — | 创建于 2025-04-25 | 2026-09-26 | 4,819 | 极活跃（新项目） |

---

## 2. 候选 A：wasmtime + Javy（玩家 JS 编译为 WASM 再由 Rust 宿主执行）

### 2.1 技术构成与工作方式【文档】

- Javy 是"JS → QuickJS 字节码 → WASM"工具链：前端用 **swc** 解析 JS，运行时内核是 **QuickJS（经 rquickjs 绑定）编译进 wasm32-wasi**（证据：javy 依赖升级 issue "Bump rquickjs from 0.10.0 to 0.11.0" #1106；`docs/docs-using-dynamic-linking.md` 明言 "Dynamically linked Javy modules are tied to QuickJS since they use QuickJS's bytecode representation"）。
- 两种产物：静态链接（引擎内嵌，模块大）；动态链接 `-C dynamic`（引擎放在共享 plugin.wasm，玩家模块仅 **1–16 KB**，README:20）。动态链接模块要求宿主提供 plugin 的 imports（`cabi_realloc`/`invoke` 等），wasmtime `--preload` 或 embedder 手动接。
- 默认支持 **ES2023** + 少量扩展 API（`docs/docs-using-js-api-support.md`：JSON（可选 SIMD 加速）、TextEncoder/Decoder（部分）、console（部分）；NodeJS API 不支持）。
- 调用约定是 **WASI command 模式**：输入 stdin、输出 stdout（`docs/docs-using-invoking.md`）；WIT 导出**只支持无参数、无返回值的函数**，"Only ESM exports are supported (Node.js/CommonJS exports are not supported)"（`docs/docs-using-exports.md`）。
- 事件循环（Promise 微任务）默认关闭，`-J event-loop=y` 开启（`javy build -J help` 输出）。

### 2.2 中断与内存机制【文档+实测】

- **epoch 硬中断**：wasmtime `Config::epoch_interruption` + `Store::set_epoch_deadline` + 后台线程 `increment_epoch`（docs.rs `wasmtime::Config`/`wasmtime::Store` 方法清单本会话核实：`epoch_interruption`/`set_epoch_deadline`/`epoch_deadline_trap`/`epoch_deadline_async_yield_and_update`/`increment_epoch`）。CLI 侧 `-W timeout=2s`。
- **燃料计量**：`consume_fuel`/`set_fuel`/`get_fuel`（docs.rs 核实）——按 wasm 指令数计费，**确定性**但≠CPU 时间，且计费本身有运行时开销。
- **内存硬限**：`-W max-memory-size=<字节数>`（wasm 线性内存上限）；embedder 侧等价 `Store::limiter`（ResourceLimiter）。
- 本机实测（javy 9.1.0 + wasmtime 49.0.1，全部通过 `/tmp/sbx/t/` 脚本执行）：
  - 死循环 `for(;;){}` + `-W timeout=2s` → **2.026s wall 后 `wasm trap: interrupt`**，wasmtime 进程存活退出码 1，宿主可继续跑其他模块；
  - `new Array(20M).fill(1.1)`（160MB）在 `-W max-memory-size=134217728`（128MB）下 → trap "out of memory"，宿主存活；
  - **不设上限时默认无内存上限**（无界增长直到被 timeout 打断）——嵌入时必须显式设限；
  - QuickJS 引擎层的 OOM（无 wasm 上限、涨到引擎自限时）是 **JS 可 catch 的异常**（实测输出 `JS caught: out of memory`）。

### 2.3 Screeps 语义十项判定

| # | 硬要求 | 判定 | 证据与置信 |
|---|---|---|---|
| 1 | CPU 计量 | **有条件满足** | 无内建 per-instance CPU 时间 API；两条自建路径：(a) fuel 计指令（确定性，≠时间）；(b) 每实例绑定专用线程读线程 CPU 时钟（wasmtime 单线程执行 instance 时线程时间≈实例时间）。【文档】docs.rs Store 方法清单 +【推断】 |
| 2 | 硬超时终止 | **满足** | epoch interruption；【实测】2s 死循环被 trap、宿主存活（上节） |
| 3 | 内存硬上限 | **满足** | 【实测】max-memory-size 硬限 trap，不影响其他 instance（wasm 线性内存天然按实例隔离） |
| 4 | 按用户持久复用 | **有条件满足** | 【文档】wasmtime Store/Instance 可跨调用长期持有（QuickJS 堆在线性内存中持久）；**无 snapshot 机制**，冷启动靠 `InstancePre` 预实例化 + 动态链接小模块（1–16KB）；玩家代码变更=重编译 wasm（服务端 swc 编译，可缓存）。实例化/重建成本未实测【文档】wasmtime InstancePre 存在 |
| 5 | 多模块玩家代码 | **不满足（需预编译层）** | 【实测】`require('./other.js')` → "Error: require is not defined"；【文档】仅 ESM，且 WIT 导出无参无返回值。补救：服务端 esbuild 把玩家模块包打成单文件再 javy build（Shopify Functions 即此模式） |
| 6 | 全局注入 | **有条件满足** | 【文档】plugin 可扩展内置对象（docs-using-extending）、动态链接经 import namespace 注入宿主函数；Game/Memory 需经 plugin 编译进引擎或经 stdin 传入；Rust 原生函数→wasm import 天然可行（寻路可做成 import，避免 JS 侧实现） |
| 7 | 共享只读大缓冲 | **不满足** | Javy 的 QuickJS 运行时是单线程 WASI 构建，无 worker/SAB 暴露（docs-using-js-api-support 的 API 表无 SharedArrayBuffer；wasmtime 支持 shared memory 但 JS 层看不到）。替代：每实例拷贝地形子集，或 `terrain(x,y)` host fn 按需读。【文档+实测（API 表）】 |
| 8 | 语法兼容 ES2020+ | **满足** | 【实测】可选链 `?.`、`??`、BigInt、**eval、new Function** 全部通过；【实测】Promise.then 默认构建直接 trap（"Pending jobs in the event queue"，javy plugin-api/src/lib.rs:118-125 的 EVENT_LOOP_ENABLED 分支），`-J event-loop=y` 构建后微任务正常排空（输出 "async then: 5"）。**构建选项是语义的一部分，漏开即全服玩家 Promise 崩** |
| 9 | 执行性能 | **不满足** | 【实测·本机单点】混合基准（2e7 循环 + fib(28) 递归 + 2e6 对象创建）：node v24（V8 JIT）0.096s vs javy+wasmtime(Cranelift) 2.72s wall ≈ **28x 慢**。CPU 预算制下玩家在同样 ms 预算内只能做 ~1/28 的事，公平性/体感变化剧烈。GC 为 QuickJS 引用计数+循环收集，小对象 churn 压力可控但解释器本身慢 |
| 10 | 安全隔离 | **满足** | wasm 线性内存隔离；不 preopen 目录/不开 socket 即无 FS/网络（wasmtime WASI 按 capability 授权）。CVE 面：wasmtime 10 条 GHSA 全部在 **WASI 胶水层**（见 §2.4），零 preopen 部署对绝大多数免疫 |

### 2.4 CVE / 逃逸历史【文档：GitHub Security Advisories API 本会话查询】

wasmtime 仓库 10 条 advisory（代表性）：GHSA-vqjp-4c8c-hfgg **high**（文件系统沙箱逃逸：路径/软链尾部斜杠绕过，2026-08-20 披露，patched 24.0.13/36.0.14/46.0.3/47.0.4）；CVE-2026-58494（WASI 硬链接/改名绕过 FilePerms）；其余 medium/low 为 WASI 流/燃料记账类。**核心执行引擎（非 WASI 层）无公开逃逸记录**；wasmtime 有 CNCF 沙箱正式评估与 Responsible Disclosure 流程（SECURITY.md）。javy crate OSV 查询 0 条。

### 2.5 生产先例【文档】

- **Shopify Functions**：商家 JS → Javy → wasm，生产运行多年；开源的 Rust 执行器 `Shopify/function-runner`（最新 v9.2.2，2026-08-04）**钉死 wasmtime =36.0.14**——恰好是 GHSA-vqjp 的修补版，说明其跟进安全版时间以天计。负载特征：短生命周期、单次调用、内存限额严格——与 Screeps 的"每 tick 长驻执行"不同。
- 同族（JS→wasm）：Fastly JS Compute + Fermyon Spin 用 **StarlingMonkey**（SpiderMonkey 内核，见 §5.3）。
- 未找到任何"JS-in-wasm 跑长驻多 tick 交互式沙箱"的公开生产先例。

### 2.6 小结

wasmtime+Javy 的真实短板排序：**性能（28x）＞ 无 SAB ＞ CommonJS/调用约定 ＞ 无快照**。它的独特优势是安全模型（capability-based WASI + epoch/fuel 双硬限 + 内存硬限全实测通过）与极端部署弹性（单二进制 Rust 宿主，无 V8）。合理定位：**工具链沙箱（跑玩家提交的离线脚本/回放分析）或超小内存档**，而非默认玩家 tick 沙箱。

---

## 3. 候选 B：rusty_v8 裸绑（不经 deno_core）

### 3.1 现状【文档】

- rusty_v8 v152.2.0（2026-08-20），跟 V8 mainline（当前对应 V8 15.2），仓库 2026-09-26 仍在推送——**这是 Rust 生态里离"官方 Screeps 引擎"最近的路线：同一颗 V8**。
- deno_core 并未消亡（见 §0 修正 2）。选 rusty_v8 裸绑的理由只能是：不想要 deno_core 的 JsRuntime/op/extension/模块加载框架，想要自定义 isolate 生命周期、自定义内存/snapshot 管线、最小二进制。

### 3.2 API 能力面（docs.rs v152 方法清单逐项核实，2026-09-27）

- 硬超时：`Isolate::terminate_execution` / `cancel_terminate_execution` / `is_execution_terminating` / **`thread_safe_handle`**（跨线程终止——watchdog 线程用）✔
- 内存：`CreateParams::heap_limits(initial, max)`（builder 风格，**在创建时设定** old/young 代上限）、`heap_limits_from_system_memory`、`stack_limit`；`Isolate::add_near_heap_limit_callback` / `memory_pressure_notification` / `set_oom_error_handler` / `get_heap_statistics` ✔
- 快照：`Isolate::snapshot_creator` / `snapshot_creator_from_existing_snapshot`（SnapshotCreator 全套）+ `CreateParams::snapshot_blob`（启动即从快照建 isolate）✔
- 微任务：`set_microtasks_policy(Explicit)` + `perform_microtask_checkpoint()` ✔
- 模块/动态 import：`set_host_import_module_dynamically_callback` / `set_host_import_module_with_phase_dynamically_callback` ✔
- 注入：`Function::new`（host 函数）、`SharedArrayBuffer::new_backing_store_from_ptr / get_backing_store / with_backing_store`（外部后备存储=零拷贝共享）、`set_allow_generation_from_strings` / `set_modify_code_generation_from_strings_callback`（控制 eval/new Function 权限）✔
- 注：`set_heap_limits` 不在 Isolate 上（必须走 CreateParams），与旧版资料不同。

### 3.3 本机冒烟实测【实测】

spike 源码 `/tmp/v8spike/src/main.rs`（v8 = "152.2.0"，release 构建，退出码 0）。结果：

```
script result: {"v":12345,"opt":7}        ← host 函数注入 readHost() + 可选链 + ?? 全部工作
microtask: synchronously after run() = pending (expect pending)   ← Explicit 策略下 run() 不执行微任务
microtask checkpoint done (after scope)   ← tick 末手动 perform_microtask_checkpoint()
infinite loop after terminate: returned_none = true   ← thread_safe_handle().terminate_execution() 后死循环立即中断
is_execution_terminating: false → cancel_terminate_execution() 复位，isolate 可继续用
```

即：**CPU 硬中断、微任务排空控制、内存上限参数、host 函数注入、ES2020 语法在裸绑下全部一次跑通**。CPU 计量需自建（V8 无 per-isolate CPU 时间 API；用 `std::time::Instant` 或线程 CPU 时钟 libc::clock_gettime(CLOCK_THREAD_CPUTIME_ID) 在 host 函数里读出注入 Game.cpu.getUsed()——与官方 Screeps 用 isolated-vm 的 cpuTime 同构）。

### 3.4 裸绑的真实代价【实测】

- **API churn 实测撞上**：按旧式 API（HandleScope::new + Context 包裹）写的第一版 spike 在 v152 下 **19 个编译错误**；v152 引入 `v8::scope!` 宏 + `PinScope` + `MapFnTo<FunctionCallback>` 新回调签名（对照官方 `benches/function.rs` 才改对）。rusty_v8 无语义化版本承诺（0.x），跟随 V8 月度节奏——**每次升级都可能要改绑定代码**。deno_core 替你吃掉这部分（它就是 rusty_v8 的最大用户）。
- 需要自建：模块加载器（ESM graph/CommonJS shim）、op 调度与序列化、快照构建工具链、错误序列化、栈深度防护（CreateParams::stack_limit 有了，但 JS 侧递归策略要自己定）。
- 安全面：与 isolated-vm 共享同一颗 V8 的 CVE 面（V8 本体 + 绑定层）。rusty_v8 crate OSV 查询 0 条，但 V8 引擎本体的 CVE 由 Chromium 公告跟踪，需要随 rusty_v8 版本跟进；isolated-vm 的 GHSA-864f（§4.4）证明**绑定胶水层本身也会出 critical 洞**——裸绑等于自己写胶水、自己背这个风险类目。

### 3.5 与 deno_core 的取舍结论

| 维度 | rusty_v8 裸绑 | deno_core |
|---|---|---|
| 引擎/性能/隔离 | 相同（同一 V8，同一 snapshot/terminate 能力） | 相同 |
| 隔离复用/快照 | 原生 API 全套（已核实），但工具链自建 | JsRuntime + snapshot 工具链现成（含构建期快照） |
| CPU 计量/看门狗 | 都要自建（两者都不提供 per-isolate CPU 时间） | 同左；Deno Subhosting 的 WatchDog 是产品级参考【社区】 |
| API 稳定性 | rusty_v8 0.x 高频变动（本会话实测撞上） | deno_core 同样 0.x，但升级由 Deno 项目整体消化 |
| 依赖体积/构建 | 最小（V8 静态库） | + 一层框架 |

**建议**：主力仍应选 deno_core（工具链收益 > 控制收益）；裸绑保留给"快照编排、自定义 isolate 池、内存压力策略"等 deno_core 覆盖不到的点——两者不冲突，deno_core 本身就构建在 rusty_v8 上，可混合使用。

---

## 4. 候选 C：保留 isolated-vm，但放进 Node 边车进程（与 Rust 主进程通信）

### 4.1 架构与理由【文档+推断】

```
Rust 主进程（engine/storage/backend）
   │  stdin/stdout 长度前缀二进制帧 或 unix socket（照抄官方 common/lib/rpc.js 的帧式，换二进制）
   ▼
Node 边车（无特权用户、无 FS/网络白名单、cgroup 限内存）
   └── per-user ivm.Isolate 池（memoryLimit 256MB + SAB 地形 + snapshot 暖启动 + cpuTime 计量）
```

- isolated-vm README **Security 节原文建议**："keep instances of isolated-vm in a different nodejs process than other critical infrastructure"——边车恰是其官方推荐的爆炸半径控制法。
- 与官方架构同构：官方 runner 就是"Node 进程里的 isolated-vm"；Rust 化的收益在于主进程侧（存储/结算/网络）而非沙箱侧重写。这是**唯一不需要重写 7119 行沙箱 API 胶水（game/）就能先跑通**的路线：driver/lib/runtime 的 runtime.bundle 思路可直接搬运。
- 代价：跨进程通信税（每 tick 玩家可见世界要过一次 IPC——官方现状同样如此，见姊妹报告 §1.4 热点 4；Rust 版可用二进制帧+SAB 地形把税压到比官方 JSON 拷贝低）。
- 部署约束【文档】：v7 `engines: node >=24`（npm 元数据）；**Node 20+ 必须加 `--no-node-snapshot` 启动**（README:74）；本机 node v24.11.0 源码编译安装成功（无 prebuild 二进制，需 Xcode CLT / MSVC 工具链）。

### 4.2 v7（2026-05）新能力【文档：main 分支 commit log】

`importModuleDynamically` isolate 选项（动态 import 进沙箱）、`onUnhandledRejection` 选项、promise transfer 语义修补、"Forbid user code in ExternalCopy transfer"（2026-07-31 安全加固，即 GHSA-864f 的修复前置）、Node 26.4+ 兼容、模块specifier 非 ASCII 修复。项目处于"作者持续投入"状态（最新 commit 2026-09-26）。

### 4.3 本机实测【实测：node v24.11.0 + isolated-vm@7.0.1，`/tmp/ivm-test/test.js`、`sab.js`】

| 测试 | 结果 |
|---|---|
| 硬超时：死循环 + `runSync(ctx,{timeout:200})` | **245ms 抛 "Script execution timed out"**；**超时后同一 isolate 继续跑新脚本成功**（可复用，不必重建） |
| 内存：`new ivm.Isolate({memoryLimit:64})` + 无限分配 | 抛 "Isolate was disposed during execution due to memory limit"，**宿主存活**，该 isolate 报废需重建（与超时语义不同） |
| CPU 计量：`isolate.cpuTime`（bigint 纳秒） | 3e7 次累加循环 delta = **1654.22ms CPU**，纳秒精度。实现为每 isolate 独立 executor 线程 + `CLOCK_THREAD_CPUTIME_ID` 累计（src/isolate/executor.cc:42-64，README:275） |
| SAB 共享：同一 SAB 经 `ivm.ExternalCopy` 传入两个 isolate | A 写 `[0]=77,[100]=42`，B 读到 77，**宿主 Uint8Array 同步可见** —— 零拷贝跨 isolate 实证（README:574-578：ExternalCopy 对 SAB 只拷引用，后备存储可活得比创建它的 isolate 更久） |
| 快照：`ivm.Isolate.createSnapshot(...)` + `new ivm.Isolate({snapshot})` | 暖启动后直接调快照里的函数成功（getA()===1）。注意 README:209-225：快照含机器码，不可接受不可信输入生成 |
| 持久复用 | 同一 context 连续三次 eval，`counter` 跨调用保持（=2）——按用户跨 tick 复用成立 |
| ESM：`compileModule` + `instantiate` + `evaluate({timeout})` + `namespace` 引用 | 全部工作；namespace 经 Reference 暴露 |

注意一个 API 细节：SAB 不能直接 `global.setSync('x', sab)`（抛 "A non-transferable value was passed"），必须包 `new ivm.ExternalCopy(sab)`，在沙箱内 `.copy({transferIn:true})` 取用——这正是 GHSA-864f 的攻击面所在（见下）。

### 4.4 CVE / 逃逸历史【文档：GitHub Advisories + OSV，本会话查询】

| 公告 | 等级 | 内容 | 影响版本 / 修复 |
|---|---|---|---|
| **GHSA-864f-rcv7-6rh4** | **critical** | ExternalCopy 结果**类型混淆** → guest 可达的宿主内存破坏 + **控制流劫持**（guest→host 逃逸级） | <=7.0.0；**patched 7.0.1 / 6.2.0**；披露 2026-08-07 |
| CVE-2022-39266（GHSA-2jjq） | high | CachedDataOptions 传入不可信 V8 cached data 可绕沙箱 RCE | fixed 4.3.7 |
| CVE-2021-21413（GHSA-mmhj） | high | Reference/transferable 误用可访问 nodejs isolate | fixed 4.0.0 |

三点推论：【推断】
1. **官方 Screeps driver 把 isolated-vm 锁死在旧 commit**（姊妹报告 §1.7"锁死 commit"）——若锁定版本 <7.0.1/6.2.0，则官方私服本身暴露在 GHSA-864f 下（是否触发取决于是否把 ExternalCopy 结果交到玩家可达路径；需按实际版本核对，此处仅为风险提示）。
2. critical 洞出现在"注入/编组胶水层"而非 V8 本体——**任何自写绑定（含裸 rusty_v8）都有同类风险面**；wasmtime 的安全记录优势结构性成立。
3. 边车进程（无特权 + 无 preopen + cgroup）把逃逸的爆炸半径从"主进程 RCE"降到"边车进程内"——这正是 README Security 的建议。

### 4.5 十项判定（isolated-vm 边车）

| # | 判定 | 依据 |
|---|---|---|
| 1 CPU 计量 | **满足** | 【实测】cpuTime 纳秒精度；注入 host Callback 在玩家代码内同步读数即可实现 Game.cpu.getUsed() |
| 2 硬超时 | **满足** | 【实测】runSync timeout 245ms 强断且 isolate 存活可复用；紧急情况 dispose()（README:261） |
| 3 内存硬限 | **满足** | 【实测】memoryLimit 触发自动 dispose，他 isolate 不受影响 |
| 4 持久复用+快照 | **满足** | 【实测】全局跨 run 保持；createSnapshot 暖启动成功（README:212-225） |
| 5 多模块 | **满足** | 【实测】ESM compile/instantiate/evaluate/namespace；v7 新增动态 import；CommonJS 需 loader 层（官方 runtime.bundle 同款做法） |
| 6 全局注入 | **满足** | 官方 Screeps 生产验证多年（姊妹报告 §1.4）；Callback/Reference/ExternalCopy/NativeModule（xxscreeps 用后者把 C++ 寻路注入沙箱）。风险：注入面即攻击面（GHSA-864f 教训） |
| 7 共享只读大缓冲 | **有条件满足** | 【实测】边车进程内：一份全图地形 SAB 零拷贝给所有用户 isolate（=官方做法）。**跨进程退化**：SAB 不能跨进程，Rust 主进程 ↔ 边车之间要么每边车一份地形拷贝（地形静态，可接受），要么 mmap 文件 + 原生注入（isolated-vm 未暴露该口子）【推断】 |
| 8 语法兼容 | **满足** | V8 本体（与官方玩家体验完全一致，含 JIT 语义） |
| 9 性能 | **满足** | V8 JIT；架构税在 IPC（可用二进制帧+SAB 压到低于官方现状的 JSON 深拷贝） |
| 10 安全隔离 | **满足（带前提）** | 进程级隔离最彻底（README 明文推荐）；前提：固定 ≥7.0.1 并订阅安全更新；边车无 FS/网白名单；不把 ivm 对象泄漏给玩家代码（README Security 明示这是头号误用） |

---

## 5. 候选 D：2025–2026 新嵌入式 JS 方案扫描

### 5.1 rquickjs 0.14 + quickjs-ng 0.17（Rust 直嵌 QuickJS）

- 活跃：quickjs-ng v0.17.0（2026-09-18）月度发版；rquickjs 0.14.0（2026-09-18），已切换绑定 quickjs-ng（README）。
- 能力【文档+docs.rs 核实】：`Runtime::set_memory_limit`（JS_SetMemoryLimit 硬限）、`set_max_stack_size`、`set_interrupt_handler`（**字节码回跳点级硬中断**，官方 QuickJS 机制）、`set_gc_threshold`、`memory_usage`、async Rust 集成（Promise↔Future）、自定义 allocator、模块 loader、`embed` 宏**编译期嵌入字节码**（≈快照的替代：官方文档称 runtime 完整生命周期 <300µs【社区/上游文档】，冷启动本来就便宜）。
- 短板：无 JIT（姊妹报告引 5–10x 慢于 V8）；ES2020 近满分但 ES2021+ 部分支持【文档】；**跨 runtime 的 SAB 共享没有 API**（QuickJS 的 SAB+Atomics 面向 os.Worker 线程，stdlib.md：worker 间 postMessage 共享 SAB；rquickjs 未暴露跨 Runtime 共享后备存储的口子）→ 要求 7 不满足【文档+推断，未实测跨 runtime 行为】；CPU 计量需自建（interrupt handler 里读线程时钟）。
- 生产参考：**AWS LLRT**（Rust+QuickJS 的 Lambda 运行时，v0.9.0-beta 2026-09-06，README 自称 "experimental ... intended only for evaluation purposes"）——大厂背书但明确实验性质【文档】。
- 十项快评：1✅(自建) 2✅ 3✅ 4🟡(embed 宏) 5🟡(ESM✅/CJS 需 loader) 6✅ 7❌ 8🟡(ES2020) 9❌ 10🟡(引擎内存安全尚可但无进程级隔离、审计面小)。定位：**低内存档候选**（姊妹报告 §4.1 的 rquickjs 建议仍然成立）。

### 5.2 Boa v0.22（纯 Rust 引擎）

- 活跃（v0.22 2026-08-28），README 自我定位 **"experimental"**，自称支持 >90% 最新 ES 规范。
- 亮点【文档：CHANGELOG】：**SharedArrayBuffer（PR #3384）与 Atomics（PR #3394，含 waitAsync #4339）已实现**；`RuntimeLimits`（core/engine/src/vm/runtime_limits.rs：loop_iteration / recursion(默认512) / stack_size / backtrace）提供**确定性循环迭代硬限**——解释器 VM 逐迭代检查，天然防死循环。
- 短板：无 JIT、性能未具量化数据（README 未给，测试262 conformance >90% 但生产可用性自认 experimental）；微任务经 job queue 手动排空【文档：boa 有 job queue 机制】；无 OS 级隔离。十项：性能/成熟度不过关。定位：**观察名单**；其 RuntimeLimits 设计值得抄进任何自建沙箱。

### 5.3 StarlingMonkey（SpiderMonkey → WASM component）

- Bytecode Alliance 项目，活跃（pushed 2026-09-24）。README：**生产用于 Fastly JS Compute 与 Fermyon Spin JS SDK**；内核 SpiderMonkey（比 QuickJS 完整度更高的 JS 引擎），面向 WASI 0.2 component 模型，带标准 fetch/Streams/TextEncoding 与事件循环。
- 与 Javy 同族同优劣（wasm 内存隔离、epoch 中断可用），但引擎方言从 QuickJS 换成 SpiderMonkey（`eval`、正则、日期等语义更接近 Firefox）。**未实测**——若将来认真考虑"JS→wasm"档位，应以 StarlingMonkey 替补 Javy 做同套十项评测【文档】。

### 5.4 其他

- **denoland/celld**（2025-04 新项目，4819 stars，极活跃）：自托管分布式 Durable Objects，每节点**内嵌 V8** 跑 Cloudflare Workers 应用。不是可嵌入库，而是"V8 isolate 即服务"的完整竞品形态；对本案的启示是"每对象一个带 SQLite 的 V8 cell"的隔离/状态管理模式。
- **Deno Sandbox**（商业产品，2026）：Linux microVM 跑不可信代码（AI agent 场景）——行业在"不可信 JS"上向 VM 级隔离演进的佐证；Screeps 私服规模用不上， MMO 形态可作终局参考。
- **Deno Subhosting**：deno_core 系多租户不可信 JS 的产品级先例（姊妹报告已引，本会话在 deno.com 导航再次确认产品在售）。
- 未发现其他 2025-2026 新出现的、值得纳入的嵌入式 JS 引擎（检索了 miniv8——仅 0 star 个人项目，与传闻中的"Deno MiniV8"不符，**该传闻不实**；mozjs/Constellation 类 SpiderMonkey Rust 绑定无 2026 活跃迹象）。

---

## 6. 与官方 isolated-vm 方案的能力对照（总收口）

官方线上栈 = "Node 进程 + isolated-vm（锁版本）+ V8 snapshot + 每 tick ExternalCopy 深拷贝"。四个候选对它的增量：

| 维度 | 官方 isolated-vm | A. wasmtime+Javy | B. rusty_v8 裸绑 | C. isolated-vm 边车 | D. rquickjs |
|---|---|---|---|---|---|
| 引擎语义（玩家体感） | V8 | QuickJS/wasm（慢28x） | V8（同源） | V8（同款库） | QuickJS（慢5-10x） |
| CPU 计量 | cpuTime+intent 计费 | 自建 fuel/线程钟 | 自建 | **同款 cpuTime**【实测】 | 自建 interrupt 钟 |
| 硬中断 | runSync timeout | epoch（实测 2s trap） | terminate（实测） | runSync timeout（实测，isolate 存活） | interrupt handler |
| 内存硬限 | memoryLimit | wasm 线性内存限（实测） | heap_limits | memoryLimit（实测） | set_memory_limit |
| 零拷贝地形 | 进程内 SAB【官方】 | ❌ | SAB 外部后备存储 API | 边车内 SAB✅ / 跨进程拷贝 | ❌ |
| 快照/冷启动 | V8 snapshot | InstancePre（无快照） | SnapshotCreator 原生 | createSnapshot（实测） | embed 字节码宏 |
| 进程级隔离 | ❌（runner 即宿主） | ✅ | ❌（进程内） | ✅（README 推荐） | ❌ |
| 沙箱 CVE 记录 | 3 条（1 critical 2026-08） | 0（引擎层） | 绑定层自担 | 同官方（≥7.0.1 已修） | 少但审计少 |
| Rust 单二进制 | ❌ | ✅ | ✅ | ❌（带 Node 边车） | ✅ |

**收口建议**（与姊妹报告 §4.1 的"deno_core 主 + rquickjs 档"框架兼容）：
1. 主力：deno_core（或 B 裸绑，若确需自定义计量/快照编排）——十项全满足，工程量集中在计量与 watchdog。
2. 过渡期捷径：**C（isolated-vm 边车）** 是唯一能先复用官方 driver/runtime 语义、边做 Rust 主进程边验证的路线，且安全形态（进程隔离）反而最好；硬前提是 ≥7.0.1 + 订阅 GHSA。
3. A（Javy）与 D（rquickjs）做低内存/工具侧档位；若认真做 JS→wasm 档，先用 StarlingMonkey 复测（预期比 Javy 快、方言更全，未验证）。
4. 任何自建绑定都必须抄两份作业：isolated-vm 的 CpuTimer（executor.cc:42-64）与 Boa 的 RuntimeLimits（循环/递归硬限）。

---

## 7. 证据来源索引

- 本会话 GitHub API / crates.io / npm 查询（版本与活跃度）：wasmtime v49.0.1、javy v9.1.0、rusty_v8 v152.2.0、isolated-vm npm 7.0.1（engines node>=24）、deno_core crate 0.412.0、quickjs-ng v0.17.0、rquickjs 0.14.0、Boa v0.22、LLRT v0.9.0-beta、StarlingMonkey、celld。
- 本会话本地实测产物：`/tmp/sbx/t/`（javy/wasmtime 全部测试脚本与 wasm）、`/tmp/v8spike/`（rusty_v8 spike）、`/tmp/ivm-test/`（isolated-vm 测试）。
- 文档：javy `docs/docs-using-{js-api-support,exports,invoking,dynamic-linking,runtime-requirements}.md`、`crates/plugin-api/src/lib.rs`（event loop）；isolated-vm `README.md`（行号引用见文中：74/186/192-193/212/261/275/301/325/574-578）、`src/isolate/executor.cc`、main 分支 commit log、npm time 数据；docs.rs：`wasmtime::Config/Store`、`v8::Isolate/CreateParams/SharedArrayBuffer`、`rquickjs::Runtime`；Boa `core/engine/src/vm/runtime_limits.rs`、CHANGELOG；quickjs-ng `docs/docs/stdlib.md`；StarlingMonkey/LLRT/celld/Shopify function-runner README。
- 安全库：GitHub Security Advisories API（wasmtime 10 条；isolated-vm 3 条含 GHSA-864f-rcv7-6rh4 critical，<=7.0.0 fixed 7.0.1/6.2.0，2026-08-07）；OSV API（v8/rquickjs/deno_core/javy crate 均 0 条）。
- 生产先例：Shopify function-runner v9.2.2（2026-08-04，wasmtime=36.0.14）；Fastly/Fermyon（StarlingMonkey README ADOPTERS 声称）；AWS LLRT（README 自称 experimental）；官方 Screeps MMO（isolated-vm，多年）。
