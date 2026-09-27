# 沙箱选型决策报告（02）：玩家 JS 沙箱引擎

> 报告日期：2026-09-27。决策对象：Screeps Rust 重构中承载玩家不可信 JS 代码的沙箱引擎。
> 输入材料：4 份调研笔记（`docs/research/notes/sandbox-{denocore,quickjs,boa,alternatives}.md`）+ 3 份实测 spike 笔记（`docs/research/notes/spike-{rquickjs,deno_core,boa}.md`），以及工作流提供的复现状态。
> 置信级别标注（全文适用）：
> - **【实测·复现】**＝工作流对 `spikes/` 下三个 spike 重新运行，`cargo run --release` 均退出码 0（复现状态由工作流提供，见 §4）；数字引自 spike 笔记记录的最终运行输出。
> - **【实测·笔记】**＝调研笔记中记录的本机实测输出（调研阶段的 `/tmp` 工程，本次工作流未复跑）。
> - **【文档】**＝官方文档 / 源码 / API 签声称（含本地 clone 核对的 file:line）。
> - **【推断】**＝由实测数字算术推导（如倍数）。
>
> 本报告撰写过程未新运行任何基准；所有实测数字均可在"来源"列的笔记与其记录的复现命令核对。

---

## 0. 结论（先行）

**推荐：deno_core 0.412.0（rusty_v8 / V8 15.0）作为玩家沙箱主力。** rquickjs（quickjs-ng 0.16.2）有条件可行，保留为"低内存密度档"备选。**boa_engine 0.22.0 不推荐承载玩家负载**——内存硬上限这一硬要求官方机制不存在，spike 判定 fail。

推荐 deno_core 的理由（全部有实测支撑）：

1. **十项硬要求唯一全绿**（#10 安全隔离与所有候选一样是有条件满足）。硬中断与内存硬上限两块"硬骨头"用官方 API 开箱可用：跨线程 `IsolateHandle::terminate_execution()` 在 210.2ms 精准打断死循环、runtime 可复用；`heap_limits + near_heap_limit_callback` 在 17.1ms 优雅终止内存炸弹、峰值 RSS 73MB 封顶、进程存活【实测·复现，notes/spike-deno_core.md:48-98】。
2. **性能没有对手**：deno_core 的 fib(30) 为 4.9ms；同一负载下 rquickjs 的耗时（60.0ms）约为 deno_core 的 ~12 倍、boa（318.5ms）约 ~65 倍；objchurn/jsonchurn 上 rquickjs 分别慢 ~53 倍 / ~7 倍（§3 对比表）。CPU 预算制下这直接决定同样硬件能承载的用户数与玩家体感公平性。
3. **生产先例最强**：Supabase Edge Functions（256MB/2s CPU 硬限的多租户不可信代码）与 Deno Deploy/Subhosting 都是 deno_core + V8 isolate 模型，cpu_timer/自定义分配器/监督策略源码可直接抄【文档+生产验证，notes/sandbox-denocore.md:226-236】。
4. **工程成本可控**：`#[op2(fast)]` 宿主函数调用 ~5ns；每 isolate 边际内存 ~2.1MB；启动 snapshot 在**同一对照实验内**把 runtime 创建从 3356µs 压到 762µs（比值 4.40x；另一轮 4815→917µs = 5.25x。注意：该实验自身的无-snapshot 基线 ~3.4ms 与 §3.1 主表的 2157µs 是两种测法，不可互比，§3.4 澄清）【实测·复现，notes/spike-deno_core.md:143-168】。"每玩家常驻 isolate"与"每 tick 重建"两种架构的**成本与生命周期模型**（创建耗时/常驻内存/LIFO 销毁纪律/20 次建删无泄漏）均被实测支持；但推荐部署形态"一 isolate 一线程"**作为整体未经原型验证**（JsRuntime 非 Send，spike 明确列为未测项，见 §6.4 第 2 条）。
5. **风险已知且有解**：两个实测踩到的架构级坑——isolate 必须 LIFO 逆序销毁、`heap_limits` 单用会 FatalOOM（阴性对照 exit 133）——都已在 spike 中复现并给出规避方案（notes/spike-deno_core.md:170-194）。

**部署组合建议**（承 deno_core 调研笔记 §9；每项括号内为验证状态，区分"本机实测"与"仅有生产先例"）：
- 每用户常驻 isolate ＋ 一 isolate 一线程【生产验证（Supabase 同款）；**整机型未本地实测**——spike 验证的是"单线程宿主持 10 个 isolate ＋ 独立看门狗线程"，JsRuntime 非 Send，见 §6.4 第 2 条】；
- 看门狗线程 `terminate_execution`【实测·复现】；
- 内存三件套：`heap_limits` + near-heap 回调终止【实测·复现】＋ **自定义 ArrayBuffer 分配器上限**【生产验证（Supabase `external_memory.rs` 同款）；**本地未实测**——spike 的内存炸弹是 V8 堆内对象分配，堆外 ArrayBuffer 路径未覆盖，见 §3.3 注与 §6.4 第 5 条】；
- 构建期 startup snapshot（Game 常量/lodash 底座）【实测·复现】；
- ESM 原生 / CJS 垫片【实测·笔记，标准模式】；
- runner 进程分片（无密钥、限额销毁重建）＋ `v8_enable_sandbox` feature ＋ Linux seccomp【文档/生产实践（Deno Deploy、Supabase），未本地验证】。
- CPU 计量走"线程 CPU 时钟记账" op（V8 无 isolate 级 CPU API；`CLOCK_THREAD_CPUTIME_ID` 的 ns 级读数路径在调研工程实测【实测·笔记】）。

**备选路线定位**（来自 notes/sandbox-alternatives.md:9-16，文档级）：isolated-vm Node 边车（9.5/10，进程隔离最彻底，唯一能复用官方 driver 语义的过渡路线，硬前提 ≥7.0.1——2026-08 刚修过 critical 级 guest→host 逃逸 GHSA-864f）；rusty_v8 裸绑（能力等价 deno_core，代价是自建全部设施 + 承受 0.x API churn，实测撞上 19 个编译错误）；wasmtime+Javy（安全最强但慢 ~28x，只适合工具侧沙箱）。

---

## 1. 候选与版本底座

| 候选 | 精确版本（Cargo.lock 实测锁定） | 引擎内核 | 角色 |
|---|---|---|---|
| **deno_core** | deno_core 0.412.0 / deno_v8 0.4.0 / v8(rusty_v8) 150.4.0（引擎串 `15.0.245.2-rusty`）/ serde_v8 0.321.0 / deno_ops 0.288.0 / tokio 1.53.1 | V8 15.0（JIT） | **主力推荐** |
| **rquickjs** | rquickjs(-core/-sys/-macro) 0.14.0（2026-09-18），内嵌 vendored **quickjs-ng 0.16.2**（quickjs.h:1450-1452；运行时 `JS_GetVersion()="0.16.2"`） | quickjs-ng（纯解释器，无 JIT） | 低内存密度档备选 |
| **boa_engine** | boa_engine 0.22.0（全家桶同版）/ regress 0.12.0 / temporal_rs 0.2.6 | boa（纯 Rust 解释器，无 JIT） | 不推荐承载玩家负载 |

关键事实：rquickjs 0.14.0 **已移除后端开关，quickjs-ng 是默认且唯一后端**（.gitmodules 指向 quickjs-ng/quickjs；sys 无后端 feature）【实测·笔记，notes/spike-rquickjs.md:9-18】。想走 bellard 原版（同机基准比 ng 快 1.4~2x【实测·笔记，notes/sandbox-quickjs.md:128-145】）需要换绑定，本次未实测。

环境：三份 spike 同机（macOS 24.6.0 arm64，Apple Silicon 10 核，rustc 1.98.1），**全部 release 构建**（`cargo run --release`；rquickjs 的 release profile 另有 `lto = true`、`codegen-units = 1`，内嵌 quickjs C 代码按 OPT_LEVEL=3 编译【notes/spike-rquickjs.md:48】）。注意区分：deno_core **调研阶段**的 `/tmp/sb-research/denocore-bench` 实验是 debug 构建（notes/sandbox-denocore.md:4、:188），其数字未进入 §3.1 主表，详见 §6.4 第 4 条的澄清。

---

## 2. 十项硬要求 × 引擎判定矩阵

判定符号：✅ 满足 / ⚠️ 有条件满足 / ❌ 不满足 / ❓ 存疑（证据矛盾，待实测）。

| # | 硬要求 | deno_core (V8) | rquickjs (quickjs-ng) | boa_engine |
|---|---|---|---|---|
| 1 | CPU 时间计量 | ✅ 线程记账：V8 无 isolate 级 CPU API（源码核对），op 读 `CLOCK_THREAD_CPUTIME_ID`（真 CPU 时间），ns 精度【实测·笔记】 | ✅（附条件）注入原生函数读宿主时钟，JS 内可见——**实测证据为墙钟**（Instant），见表下注【实测·笔记】 | ✅（附条件，同左）注入原生函数读宿主时钟，µs 源，实测亦为墙钟【实测·笔记】 |
| 2 | 硬超时终止 | ✅ 跨线程 terminate_execution，210.2ms 终止，不可 catch，runtime 复用=true【实测·复现】 | ✅ interrupt handler 抛**不可 catch** 的 `InternalError: interrupted`，210ms 命中，复用 ok【实测·复现】 | ⚠️ 仅等价机制：`loop_iteration_limit` 校准预算 200ms±5% 终止、JS 拦不住、可复用；**非时间式**，正则回溯实测 6.9s 零让出（盲区）【实测·复现+笔记】 |
| 3 | 内存硬上限 | ✅（必选组合）`heap_limits`+near-heap 回调：17.1ms 终止、峰值 RSS 73MB、exit 0；**单设 heap_limits = FatalOOM abort（阴性对照 exit 133）**【实测·复现】 | ✅ 分配器级硬限：64MB 处 24ms 强停、RSS 钉 59.6MB、OOM 后无需重建可继续；附条件：OOM 异常玩家可 catch → 需"OOM 即杀"策略【实测·复现+笔记】 | ❌ **官方无任何内存上限 API**（0.22 与 main 均无）：1e6 对象 RSS +216MB 零拦截；补救仅全局分配器 abort（整进程粒度、不可恢复）【实测·复现】 |
| 4 | 按用户持久复用 | ✅ 常驻 isolate 实测 10 个并存；snapshot 配对实验 3356→762µs（4.40x，另一轮 5.25x；口径说明见 §3.4）【实测·复现】 | ✅ runtime+context 长寿命；创建 51µs；无堆快照，等价=模块字节码缓存（只缓存宿主自编译产物，**绝不接受玩家字节码**——官方 SECURITY.md 红线）【实测·复现+文档】 | ✅ 跨 tick 复用实测；Context 重建仅 ~0.25ms，无需快照【实测·笔记】 |
| 5 | 多模块玩家代码 | ✅ ESM 原生（StaticModuleLoader）；CJS 需垫片，`require()` 垫片+ESM 相对导入均实测通过【实测·笔记】 | ✅ ESM 原生（含 top-level await 实测）；引擎无内建 require（typeof undefined 实测），CJS 需宿主注入【实测·笔记】 | ✅ ESM 完整（MapModuleLoader）；CJS 需宿主 ~50 行垫片（官方示例）【实测+文档】 |
| 6 | 全局注入 | ✅ extension/ops 体系 + PinScope 直注 globalThis（SAB 注入实测）；fast op ~5ns/call【实测·复现+笔记】 | ✅ `globals().set` 注入 Rust 函数实测被 JS 调用【实测·笔记】 | ✅ register_global_callable/property/class【实测·笔记】 |
| 7 | 共享只读大缓冲（全图地形表） | ✅ SAB BackingStore 跨 2 isolate 零拷贝、单份内存（Rust 写→A 读写→B 读）【实测·笔记】 | ❓ SAB 默认暴露（typeof=function 实测）；`SharedArrayBuffer::from_source_shared` API 源码级存在但**未压测**；两份笔记就是否可跨 Runtime 共享结论相反（见 §5 矛盾 2）【文档，证据冲突】 | ✅ SAB 跨 Context 真共享 + 宿主 `Arc<Vec<u8>>` 注入只读 getter，4 Context 共读实测【实测·笔记】 |
| 8 | 语法兼容 ES2020+ | ✅ ES2022 全套实测（可选链/BigInt/私有字段/命名捕获组）；微任务 run_event_loop 排空实测【实测·笔记】 | ✅ ES2020→ES2025 实测全过（含 Iterator helpers/Float16Array）；**Intl 不支持**（官方明确不做）【实测·笔记】 | ✅ 16/16 实测全过；test262 96.01%（官方数据）【实测·笔记+文档】 |
| 9 | 执行性能 | ✅ V8 JIT 本尊；fib30 4.9ms【实测·复现】 | ⚠️ 计算密集慢 ~12x（fib）/ 7x（JSON）；调研口径 15~25x【实测·复现+笔记】 | ⚠️ fib30 318.5ms（慢 deno ~65x）；官方基准对 quickjs 差 5.7x、对 v8-jitless 差 12x【实测·复现+文档】 |
| 10 | 安全隔离 | ⚠️ 能力默认零；但 `Deno.core`（~100 个内部 op，含 op_print/op_panic）+ WebAssembly/SAB/`__bootstrap` 裸露，生产需遮蔽或 skip_op_registration；同进程 isolate 非硬边界（Deno 官方立场），需进程分片 + V8 sandbox feature【实测·复现+文档】 | ⚠️ 默认沙箱面最干净（70 个纯内建、WebAssembly 不存在、`import('std')` 被拒）；但 C 引擎有持续 CVE 史（含 ≤0.12.1 的 RCE 级 CVE-2026-37630；本 spike 内嵌 0.16.2 已含修复）【实测·复现+文档】 | ⚠️ 纯 Rust 内存安全面最小（唯一 CVE 是已修的 panic-DoS）；但 boa_gc 堆 thread_local 共享，同线程多 Context 无内存隔离；Context !Send 无法跨线程干预【实测+文档】 |

**矩阵读法**：deno_core 9✅+1⚠️；rquickjs 7✅+2⚠️+1❓；boa 6✅+3⚠️+1❌。boa 的 ❌ 落在硬要求上，直接出局主力竞争；rquickjs 的 ❓（跨 runtime SAB）与 ⚠️（性能）都不否决其备选地位。

**#1 的名实说明（墙钟 vs CPU 时间）**：三家引擎均无 per-runtime CPU 时间 API，#1 的实现路径都是"宿主注入原生函数"。deno_core 的实测读数是线程 CPU 时钟（`CLOCK_THREAD_CPUTIME_ID`，真 CPU 时间）；rquickjs/boa 的实测读数是宿主单调时钟（Instant，**墙钟**）。后两者的 ✅ 依赖等价条件：部署模型为"每用户沙箱独占一个线程、且该线程只跑该用户代码"时墙钟≈CPU 时间（quickjs 笔记明确此等价条件，notes/sandbox-quickjs.md:168-169）。若需严格 CPU 时间，同一注入点换成 `CLOCK_THREAD_CPUTIME_ID` 即可（macOS/Linux 均可用，该读数路径已在 deno 调研实测），但**此换法未在 rquickjs/boa 上单独实测**——它是注入层的选择问题，不是引擎能力缺口。

---

## 3. 实测数字对比（含方法与置信级别）

### 3.1 主基准表

三个 spike 使用**逐字相同的 JS 工作负载源码**，同一台机器（macOS arm64, release）。计时方法有差异（见注），跨引擎倍数为【推断】（对实测中位数做除法）。

| 指标 | deno_core | rquickjs | boa_engine | 置信 |
|---|---|---|---|---|
| fib(30) 中位 (ms) | **4.9**（多轮 4.3–5.3） | **60.0**（min 59.6 / max 60.5） | **318.5**（**为实测 fib(30)**，3 次取中位；另补测 fib(27)=73.0 作降载参照，非换算来源） | 【实测·复现】 |
| objchurn 中位 (ms) | **0.3**（多轮 0.3–0.9） | **15.934** | **89.0** | 【实测·复现】 |
| jsonchurn 中位 (ms) | **6.6**（多轮 6.4–7.9） | **47.720** | **380.5** | 【实测·复现】 |
| 正确性断言 | 832040 / 74850000 / 19343 ✓ | 同左 ✓ | 同左 ✓ | 【实测·复现】 |
| 创建 runtime (µs) | 2157（实验 D"创建+触碰脚本"口径，20 次中位多轮 2.0–2.6ms；首建含 V8 平台初始化 2922；snapshot 对照实验另测得纯创建基线 ~3.4ms，两者是不同测法，见 §3.4） | **51**（min 50） | 109 热态（冷 ~800–1000） | 【实测·复现】 |
| 10 实例常驻边际 (MB) | 19.5（≈2.1/个） | 1.6 热堆 / 3.94 冷进程（≈0.39/个） | 3.1 冷进程（≈0.35/个，共享 GC 堆） | 【实测·复现】 |

方法注（影响可比性，如实说明）：
- deno_core：预热 1 次 + 5 次取中位，`Instant` 包住 `execute_script`（含一次边界开销）【notes/spike-deno_core.md:31-46】。
- rquickjs：warmup+多轮取中位（fib 1+5、objchurn 2+30、jsonchurn 2+20），`Instant` 围绕单次调用【notes/spike-rquickjs.md:50-62】。
- boa：每次重复用**全新 Context**（排除编译缓存），只计 eval；fib(30) 与 fib(27) 均为**直接实测**（各 3 次取中位，返回值 832040 / 196418 断言通过），fib(27) 是补充参照项而非换算来源；obj/json 5 次+1 预热取中位【notes/spike-boa.md:40-56】。
- 三者都以中位数报告、都做了返回值断言；负载相同、方法近似，倍数量级可信，个位数差异不可信。

### 3.2 性能倍数（以 deno_core 为 1x）【推断】

| 负载 | deno_core | rquickjs | boa_engine |
|---|---|---|---|
| fib(30) 递归（调用密集） | 1x | ~12.2x | ~65.0x |
| objchurn（对象分配密集） | 1x | ~53x | ~297x |
| jsonchurn（JSON 序列化密集） | 1x | ~7.2x | ~57.7x |

对 Screeps 的含义：单 tick 每玩家 ~20ms CPU、500ms 总预算的世界里（调研口径），12x 差距意味着 rquickjs 能承载的"重计算玩家密度"约为 V8 的 1/12，boa 约 1/65。典型 Screeps 玩家逻辑（遍历几百 creep + 原生寻路）多数在解释器预算内，但纯 JS 寻路/A* 玩家在 rquickjs 上会明显吃亏——若选该档必须引导使用注入的原生寻路。objchurn 的 53x/297x 差距对"大量短命对象"的 Screeps 负载形态尤为相关。

### 3.3 能力实测对照（CAPABILITY 摘要）

| 能力 | deno_core | rquickjs | boa_engine |
|---|---|---|---|
| 硬中断（目标 200ms） | 210.2ms 终止，`Uncaught Error: execution terminated`，宿主存活，**runtime 复用=true**（deno_core 自动 cancel）【实测·复现】 | 210.1ms 终止，`InternalError: interrupted` **不可 catch**，同 runtime 复用 ok（需先复位标志位）【实测·复现】 | 203.3ms 终止（五轮 194.4–203.3，200ms±5%），`RuntimeLimitError::LoopIteration` JS 拦不住、同 Context 可复用；**预算需按循环形状线性校准**【实测·复现】 |
| 内存上限（64MB） | 17.1ms 终止，峰值 RSS 73.0MB，回调触发 1 次，子进程 exit 0；**阴性对照（只设 heap_limits）V8 FatalOOM abort，exit 133，峰值仍仅 ~74MB**【实测·复现】。**覆盖范围注**：测试负载为 V8 堆内对象分配（`new Array(10000).fill(1.5)` 循环），`heap_limits`/near-heap 回调路径对**堆外 ArrayBuffer 分配**是否同等生效未在本次验证（峰值 RSS 73MB 超出 64MB 堆限的部分即含堆外开销余量，但未做 ArrayBuffer 炸弹测试）；Supabase 生产为此另加自定义 ArrayBuffer 分配器上限【生产验证，本地未实测】 | 24ms 强停，出错时 RSS 59.6MB 钉在上限下，进程存活；OOM 后不重建仍可 eval；调研补充：OOM 异常可被玩家 catch（可继续空转烧 CPU）→ 宿主需 OOM 即杀【实测·复现+笔记】 | **fail**：1e6 对象 RSS 29.6→246.3MB（+216MB）零拦截零报错；补救实测=全局分配器 64MB 硬上限 abort（signal=6，超限 56 字节即触发），粒度整进程、不可恢复【实测·复现】 |
| 默认安全面 | process/require/fetch/XHR/WebSocket/fs 全 undefined；**但 `Deno.core` ~100 内部 op 裸露（op_print 可写宿主 stdout）、WebAssembly/SAB/Atomics 可用、`__bootstrap` 挂全局**——生产必须遮蔽【实测·复现】 | **最干净**：globalThis 仅 70 个纯 ECMAScript 内建，WebAssembly 也不存在，`import('std')` 被拒（could not load module），全部宿主探针 undefined【实测·复现】 | 干净：61 个纯内建（含 Temporal），process.exit/require 实际调用均 ReferenceError；SAB/Atomics 存在【实测·复现】 |

### 3.4 deno_core 附加实测（选它才拿得到的数字）

- **startup snapshot**（build.rs `create_snapshot`，产物 722KB）：**同一对照实验内**，无 snapshot 基线 10 次中位 **3356µs** → 有 snapshot **762µs**，配对比值 **4.40x**（另一轮 4815µs → 917µs = **5.25x**）；预热的全局 fib/常量/正则完整恢复（fib(20)=6765 ✓）【实测·复现，notes/spike-deno_core.md:143-156】。
- **创建耗时两个数字的关系（避免误读）**：§3.1 主表的 2157µs（实验 D，多轮 2.0–2.6ms）与 snapshot 实验的无-snapshot 基线 ~3.4ms **都是真实的无-snapshot 创建耗时，但测法不同**——实验 D 是"创建 + 执行一句小脚本触碰页面"的 20 次中位；snapshot 实验是纯创建的 10 次中位，且两组数字来自不同运行轮次。因此 snapshot 的加速倍数只取 snapshot 实验自身的配对基线（3356→762 = 4.40x），**不要**用主表 2157µs 去除 762µs。两口径各自内部一致，无矛盾。
- **Extension 同步 op**：`#[op2(fast)]` u32 参数 ~5–6ns/call（1e6 次连调均值 5ns）；`#[serde] Vec<f64>` ~77–136ns/call；正确性 42/15 验证通过【实测·复现，notes/spike-deno_core.md:158-168】。寻路等热路径 Rust 原生函数的注入成本可忽略。
- **isolate 记账**：持 1 个 Δ0.9MB、第 2..10 个平均 2.2MB/个；LIFO 逆序销毁 10 个后 RSS 回落、再建新 runtime 正常【实测·复现】。

### 3.5 方差与环境噪声（如实）

- rquickjs spike 期间兄弟 spike 并行编译，load 3.8→35，中位数最多漂 6 倍（fib 367ms），但**所有轮次 fib min 稳定 59.6–63ms**——单线程性能地板可靠，跨引擎对比应取同窗口【实测·笔记，notes/spike-rquickjs.md:64-74】。
- boa 空闲时段 ±3%，后台负载可劣化 ~2x【实测·笔记，notes/spike-boa.md:53-54】。
- deno_core objchurn 受 GC 时机影响抖动明显（0.3–0.9ms）【实测·笔记】。
- 结论：§3.2 的倍数（7x–297x）远超各引擎自身的噪声区间，相对排序稳健；绝对毫秒值仅在"同机同窗口"意义下有效。

---

## 4. 复现状态

工作流提供的复现结果（本报告撰写时未再次运行，如实声明）：

| 引擎 | 复现命令 | 结果 |
|---|---|---|
| rquickjs | `cd spikes/rquickjs-spike && cargo run --release` | ✅ 成功，退出码 0 |
| deno_core | `cd spikes/deno-spike && cargo run --release`（一次运行打印全部结果） | ✅ 成功，退出码 0 |
| boa_engine | `cargo run --release --manifest-path spikes/boa-spike/Cargo.toml` | ✅ 成功，退出码 0 |

三个 spike 均为单次运行打印全部实验（A–E）+ RESULT/CAPABILITY 汇总行；rquickjs 另有 `hold N` 子命令（冷进程 RSS，`/usr/bin/time -l`），deno 另有 `--example negctl`（阴性对照，故意 abort）。原始输出存档：`docs/research/notes/rquickjs-run-output.txt`（rquickjs 安静代表轮）。

---

## 5. 【实测】与【文档】矛盾的澄清（按规则如实呈现，实测优先）

1. **rquickjs 的语法兼容性**：备选扫描笔记称 quickjs "ES2020 近满分但 ES2021+ 部分支持【文档】"（notes/sandbox-alternatives.md:221），而 quickjs 专项笔记本机实测 ES2020→ES2025 全部通过、仅 Intl 缺失（notes/sandbox-quickjs.md:107-122）。**以实测为准：判定 ✅**；备选笔记的文档级结论过时（针对旧版 quickjs 或 ng 早期版本）。
2. **rquickjs 跨 Runtime 的 SAB 共享**：两份笔记直接矛盾——备选笔记称"rquickjs 未暴露跨 Runtime 共享后备存储的口子 → 要求 7 不满足【文档+推断，未实测】"（notes/sandbox-alternatives.md:221）；quickjs 笔记源码级确认 `SharedArrayBuffer::from_source_shared`（core/src/value/array_buffer.rs:191-198）与引擎 `JS_SetSharedArrayBufferFunctions` 钩子存在，但明确标注"未跑大表压测"（notes/sandbox-quickjs.md:203-208）。**两份证据都没跑过实际共享行为，本报告判定 ❓ 待实测**（列入 §6 后续验证）。此项不否决 rquickjs 备选地位，也不影响 deno_core 主力路线（其 SAB 跨 isolate 零拷贝已有完整实测）。
3. **deno_core spike 两次汇总行的数字差异**：工作流复现行 fib30=4.9 / objchurn=0.3 / rss_10=19.5，spike 笔记 §9 记录行 fib30=5.3 / objchurn=0.9 / rss_10=20.4（notes/spike-deno_core.md:198）。两次都是真实运行，全部落在笔记记录的多轮范围（fib 4.3–5.3、objchurn 0.3–0.9、rss 19.4–20.7）内；objchurn 0.3 略低于笔记 §10 所写"0.4–0.9"下沿，属单次样本正常抖动（GC 时机敏感项）。本报告以工作流复现行为主表数字、笔记范围为波动区间，两者不矛盾。
4. **"deno_core 已死"的传闻不成立**：GitHub 仓库 2026-02-27 归档只是并入 denoland/deno monorepo（`libs/core`），crate 仍月更（0.412.0，2026-09-16；crates.io 近 90 天下载 170 万）【文档，notes/sandbox-denocore.md:38-41 + notes/sandbox-alternatives.md:19 修正 2 交叉印证】。
5. **QuickJS OOM 的"可恢复"语义**：spike 实验 C 显示 OOM 后 runtime 无需重建即可继续 eval（notes/spike-rquickjs.md:94-102），但调研笔记 sandbox_test3 实测 OOM 异常**可被玩家 catch**（[B1]）且超限后 runtime 处于退化状态、社区一致建议重建（notes/sandbox-quickjs.md:81-97）。两者不矛盾而是互补：**机制上可继续 ≠ 策略上应继续**——生产策略应为"OOM 即销毁重建"（成本仅 ~51µs），spike 的"可继续"只证明宿主不会被卡死。

---

## 6. 风险与建议的后续验证

### 6.1 deno_core（推荐方案）风险清单

| # | 风险 | 证据 | 缓解 |
|---|---|---|---|
| 1 | **isolate 必须 LIFO 逆序销毁**（创建即 Enter、drop 即 Exit，正序 drop 实测 V8 fatal abort） | 【实测·复现】notes/spike-deno_core.md:172-182 | 玩家生命周期管理按栈序销毁，或"每 tick 顺序创建/销毁"（实测 20 次建删无泄漏）；已在实验 D 验证逆序安全 |
| 2 | **heap_limits 单用 = 进程 abort**（V8 默认走 FatalOOM 而非可捕获错误） | 【实测·复现】阴性对照 exit 133 | 生产必选组合：heap_limits + `add_near_heap_limit_callback`（terminate + 临时抬限），deno_core 官方测试同款 |
| 3 | `Deno.core` ~100 内部 op 裸露（op_print 写 stdout、op_panic、op_serialize 等，未鉴权） | 【实测·复现】 | 启动脚本删除/遮蔽 `Deno.core` 与 `__bootstrap`，或 `skip_op_registration` + 白名单扩展重建 |
| 4 | ESM-only，玩家 CommonJS 需垫片；奇特 CJS 变体（circular require 未完成 exports、`this===module.exports`）未验证 | 【实测·笔记】标准模式通过 | 提交期 esbuild/swc 转 ESM（最稳），或按 Node 语义补齐垫片细节 |
| 5 | V8 n-day 到达依赖 Deno 团队节奏（deno_core 锁线 v8 ^150.4.0，无法单独跳 152.x） | 【文档】notes/sandbox-denocore.md:250-257 | CI pin + 季度例行升级 + 安全事件驱动；与 Node 跟 V8 的模式一致 |
| 6 | 同进程 isolate 非硬安全边界（Deno 官方明示） | 【文档】 | runner 进程分片（无密钥/无 DB）、`v8_enable_sandbox` + pointer compression feature、Linux 上 seccomp（Deno/Supabase 生产同款） |
| 7 | 同线程多 JsRuntime 交替使用会 panic（scope 归属）；退出路径批量 drop 曾触发 fatal | 【实测·笔记】notes/sandbox-denocore.md:203-206 | 一 isolate 一线程（Supabase 同款）；显式有序销毁，不依赖进程退出析构 |
| 8 | macOS 开发机无 POSIX per-thread CPU timer（SIGALRM 硬闹钟仅 Linux） | 【文档+实测（读数可用）】 | 开发用轮询/墙钟监督，生产 Linux 用 cpu_timer 同款；`CLOCK_THREAD_CPUTIME_ID` 读数 mac 实测可用 |

### 6.2 rquickjs（备选档）风险清单

- **性能公平性**：12x（fib）差距下重计算玩家体感劣化；需引导原生寻路、或接受分档运营。
- **C 引擎 CVE 史**：quickjs 系 2020–2026 共 20 条 NVD 记录，quickjs-ng 专属 10 条，含 JS 源码可达的 RCE 级（CVE-2026-37630，≤0.12.1，本 spike 的 0.16.2 已含修复）——"同进程多用户"依赖"当前无已知漏洞"这一被反复打破过的前提；加固选项：及时升级 / 每用户独立进程 / quickjs 编 wasm（Golem 模式）。
- **绑定层坑**（实测撞上）：`ctx.with` 闭包内调 `rt.set_interrupt_handler` 会 RefCell 重入 panic；`JS_SetCanBlock`（禁 Atomics.wait）高层未包装需走 sys 裸绑定；中断异常 Display 泛化为 "Exception generated by QuickJS"，必须 `catch()` 取真实原因；**中断标志每 tick 必须复位**否则误杀。
- **未测项**：`parallel` feature 多线程并发、`set_max_stack_size`、跨 Runtime SAB 大表共享（矛盾 2）、AsyncRuntime。

### 6.3 boa_engine

硬伤明确（内存上限 ❌、时间式中断不存在 issue #3442 仍 open、thread_local 共享堆无用户间内存隔离），不推荐承载玩家负载。其纯 Rust 内存安全与最干净默认面（§3.3）值得保留在**工具侧**（离线脚本、回放分析）与观察名单；若将来正式考虑，用真实 bot 复测其 `evaluate_async_with_budget` 路径与 fuzz feature 指令预算。

### 6.4 建议的后续验证（按优先级）

1. **真实开源 bot 兼容性测试**（最高优先）：在 deno_core（CJS 垫片 + lodash3 底座）上全量跑 tick 循环，统计语法/语义失败率——这是把"ES2022 全过"从特性矩阵变成玩家代码实际兼容率的唯一办法。测试材料两类：**screeps-ok**（Screeps 行为一致性测试套件，github.com/misterwise/screeps-ok，本地已克隆于 `~/Desktop/projects/sources/screeps/screeps-ok/`，仓库盘点见 docs/research/01-screeps-rust-feasibility.md §0）与若干真实开源 bot（如 overmind 类重度 lodash/原型风格代码，需另行从 GitHub 拉取）。
2. **多线程宿主原型**：`JsRuntime` 非 Send，验证"每玩家 isolate 绑定独立 OS 线程 + 消息通信"模型下的 LIFO 销毁纪律与监督线程（deno spike 明确列为未测项）。
3. **SAB 地形表端到端**：deno_core 路线补"一份全图地形 BackingStore → N 个玩家 isolate 只读视图"的规模压测（调研阶段只测了 2 isolate 4096 字节）；若评估 rquickjs 档，同场补测其跨 Runtime SAB（解决矛盾 2）。
4. **生产环境复测**：Linux 下重跑实验 A/D。**构建配置澄清**（消除与 §1/§3.1 的表述冲突）：§3.1 主表的三引擎数字**全部来自 release 构建**的 spike（macOS 开发机）；"debug 构建"只适用于 deno_core **调研阶段**的 `/tmp/sb-research/denocore-bench` 实验（其 ~3MB/isolate 等内存数字**未进主表**，notes/sandbox-denocore.md:188-201 明示 debug 构建且"Linux release 会略低，未测"）。因此待复测的差异是"macOS 开发机 → Linux 生产机"的平台差异，而非构建配置差异。另需同窗口三引擎对齐复测一轮，以消除 §3.5 的负载方差。
5. **安全加固验证**：遮蔽 `Deno.core` 后的默认面复查（复用 spike 实验 E 的探针方法）；**ArrayBuffer 堆外分配炸弹**对 `heap_limits`/near-heap 回调路径的覆盖验证（本次内存实测负载为 V8 堆内对象，堆外路径未覆盖，见 §3.3 注），必要时照抄 Supabase 自定义 AB 分配器并实测；Linux 上 seccomp + cgroups 的 runner 分片样例。
6. **若保留 isolated-vm 边车过渡路线**：核对官方 driver 锁定的 commit 是否 ≥7.0.1（GHSA-864f critical 影响 ≤7.0.0），并订阅其安全公告（notes/sandbox-alternatives.md:185-197）。
7. **长跑 soak**：常驻 isolate 跨数千 tick 的 RSS 漂移与快照恢复一致性（对应官方 runner 每小时重启回收内存的现状痛点）。

---

## 7. 来源清单

**输入笔记（本报告全部结论的证据来源）**：
- `docs/research/notes/sandbox-denocore.md` — deno_core/V8 调研（十项判定 §0、内存三件套 §2.3、isolate 内存数字 §3、生产先例 §5、升级策略 §7、最佳实践 §9、风险 §10）
- `docs/research/notes/sandbox-quickjs.md` — QuickJS 家族调研（中断粒度 §2、内存语义 §3、ES 矩阵 §4、性能 §5、CVE §8、rquickjs 绑定质量 §9）
- `docs/research/notes/sandbox-boa.md` — boa 调研（中断现状 §2②、内存 §2③/§3.4、性能 §2⑨、test262 §3.1、生产先例 §3.7）
- `docs/research/notes/sandbox-alternatives.md` — 横向扫描（Javy §2、rusty_v8 裸绑 §3、isolated-vm 边车 §4、新方案 §5、收口 §6、对 01 报告的两处修正 §0）
- `docs/research/notes/spike-rquickjs.md` + `docs/research/notes/rquickjs-run-output.txt` — rquickjs 实测（后端锁定 §0、A–E 实验、汇总行 §8）
- `docs/research/notes/spike-deno_core.md` — deno_core 实测（A–E、snapshot/ops 附加实验、踩坑 §8、汇总行 §9）
- `docs/research/notes/spike-boa.md` — boa 实测（API 查证 §1、A–E、汇总行 §7）

**复现物**：`spikes/rquickjs-spike/`、`spikes/deno-spike/`（含 `examples/negctl.rs`）、`spikes/boa-spike/`（复现命令见 §4）。

**关键外部来源**（经笔记转引，原文已在各笔记的来源节列出）：Supabase edge-runtime 源码与限额文档（256MB/2s CPU）；Deno Deploy 架构博客（isolate hypervisor/seccomp）；rusty_v8 `src/isolate.rs`（terminate_execution 任意线程、near_heap_limit_callback）；quickjs-ng SECURITY.md（不可信字节码红线）与 NVD/OSV CVE 记录；boa 官方 test262/bench 数据仓库；isolated-vm GHSA-864f-rcv7-6rh4。

---

**一句话收口**：deno_core 用官方 API 同时拿到了"V8 性能 + 硬中断 + 内存硬限 + 快照"四件事且全部本机复现，两个架构级坑（LIFO、回调组合）已知有解；rquickjs 以 ~1/5 的单实例内存（0.39MB vs 2.1MB）和最干净的默认沙箱面换取 ~12x 的计算性能，值得作为密度敏感场景的备选档保留；boa 的内存硬上限缺口在十项硬要求上是 fail，出局。
