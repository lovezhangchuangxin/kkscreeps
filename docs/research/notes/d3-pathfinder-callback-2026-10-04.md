# D3 玩家寻路回调：实现与验证（2026-10-04）

[契约与 Mod 槽位设计](../../designs/pathfinder-callback.md)｜[批次登记](../../plans/archive/mod-plane-batches.md)｜[原始验收摘要](d3-pathfinder-callback-2026-10-04.json)。本批接续 D2 地形直通，完成玩家回调通道；不代表完整玩法、存储或 Mod 寻路能力已交付。

## 实现

- `PathFinder.search` 按官方 driver 包装 `roomCallback`：首次触达才调用，每房最多一次；false 封房，truthy 取 `_bits`（包括 getter），非函数及非法矩阵静默忽略。TypedArray 检查字节长度 2500，不强制 Uint8Array；视图偏移参与取数。
- `op_kk_pathfinder_search` 使用 `#[op2(reentrant)]`；读取地形后释放 OpState 借用，局部宿主保存回调 Global、异常 Global 和矩阵。内层搜索各有宿主、值 Arena、异常与取消状态，超过两层沿用原生堆分配路径。
- 新的 `unsafe trait SearchHost` 提供显式错误/取消和有拥有者的矩阵指针；原有不可变 Rust `search` / `search_with_room_callback` API 保持原契约。矩阵仅在同步房间回调期间修改，C++ 算法当时暂停，搜索结束前存储不能移动或释放。
- 玩家桥持有 backing store，并将字节复制到固定 `Box<[Cell<u8>;2500]>`。每次房间回调返回后同步所有旧矩阵，再接纳新矩阵，保留共享缓冲和嵌套搜索修改旧矩阵的可见性。不能只在首次返回时复制，也不对可写 V8 内存建立 `&[u8]` 借用。detach 后搜索仍持有旧 backing store。
- 普通异常以原值重新抛出（含非 Error 对象）；终止异常在 TryCatch 中 `rethrow`，保持玩家不可捕获。原生主循环的 `IsExecutionTerminating` 通过 C ABI 取消回调读取真实 isolate；取消状态 4 对应 `Error::Cancelled`，不能当作成功的空路径。
- Room wrapper 使用按 Room 身份保存的 WeakMap，每 tick 重建、按 ignore 选项缓存基础矩阵。结构、creep、安全模式、私有敌方 rampart、自有障碍工地、swamp 对象和道路按官方规则叠加；costCallback 收到克隆副本，返回 CostMatrix 才替换。findPath/findPathTo/findClosestByPath/moveTo 共用此通路。avoid/ignore 警告使用现有每 tick 去重机制。

## 两个接入问题及修复

**C++ 符号冲突。** 首次玩家 false 回调触发原生 `Value::IsBoolean` 后出现崩溃，调试栈进入真实 V8 的 `api.cc`。compat 层原先也在 `namespace v8` 内定义同名类/方法，与 deno_core 同进程链接发生 ODR 冲突。实际命名空间改为 `kk_pf_v8`，通过 `namespace v8 = kk_pf_v8` 保持上游 pf.cc/pf.h 原样，消除同名 mangled 符号。

**终止被转成普通错误。** 回调死循环测试在玩家 try/catch 内包裹 search，发现 TryCatch 捕获终止后析构清理状态，宿主错误变成可捕获普通异常，玩家随后继续。`has_terminated()` 分支显式重挂终止后，测试得到 `TickOutcome::Terminated`，catch 与后续赋值均不执行，下一 tick 的搜索可以正常复用沙箱。

## 兼容性门禁

重建 release CLI，显式指定 `KKSCREEPS_BIN`，固定 screeps-ok `34a630efa42908c117c19c25d2350fa2387bd633`：**1887 passed / 120 failed / 907 skipped / 2914 total（64.76%）**。地板 1879→1887，skip 不变。

相对 D2 的 10 个转绿用例：ROAD-TRAVERSAL-002、PATHFINDER-004/005/012/023、COSTMATRIX-007/008、ROOMPOS-FIND-002/007/009。墙上路与 costCallback 两项 D2 豁免已清理。

两项 passed→failed 是被原先忽略回调所掩盖的官方差异，已在 parity 与 ratchet-allow 逐例登记，未扩大跳过范围或降低地板：

| 用例 | 本批行为与源码依据 | 套件自身的证据 |
|---|---|---|
| ROOMPOS-FIND-010 | vanilla `rooms.js::_findClosestByPath2` 固定 goal range 1，最后以 isNearTo 反查；不采用 opts.range。回调真正封闭目标四格邻域后返回 null | 固定版本 `adapters/vanilla/parity.json` 的 `roomposition-find-closest-by-path-range-ignored`，上游增强尚未发布 |
| UNDOC-MOVECACHE-004 | vanilla `creeps.js::moveTo` 在 fatigue+visualizePathStyle 时绕过早退；缓存 moveByPath 返回 ERR_TIRED 后仍落入 findPathTo，costCallback 调用一次 | 同文件 `movecache-fatigue-visualization-recomputes` 登记 vanilla 的重算行为，套件要求的修复尚未发布 |

这两项官方行为均有玩家集成断言，不能靠再次忽略回调获取通过。

## 测试与 bot

- `pnpm run check`：格式检查、workspace Clippy、Rust 测试与 JS workspace 测试；Rust **618 passed、11 ignored**，其中微基准默认忽略。compat Node 测试 14 passed，其他 workspace 测试随同执行。文档链接检查同步执行。
- 玩家集成测试覆盖 false/非法类型与长度、非 Uint8 的 TypedArray、非零 byteOffset、原异常与 getter 异常身份、四层嵌套、内层异常捕获后外层继续、共享矩阵修改、嵌套修改、detach、矩阵缓存隔离、安全模式、跨 tick 重建、终止后复用和 JSON/KKPV 等价。
- 可变矩阵测试以原生直接读取 `Cell` 指针为参考，并断言参考与“冻结首次副本”不同，再逐字段比较玩家 path/ops/cost/incomplete。这证明夹具能揭露遗漏后续同步的错误实现。
- 原生 host 测试覆盖显式错误、主循环取消、池复用和内层取消不污染外层。本批玩家预算测试覆盖死循环回调；未用单个巨大搜索的定时测量承诺硬中断延迟。
- TooAngel（正确模块目录 `compat/fixtures/tooangel/src`）2000/2000 tick、0 console-error；Overmind-dist 2000/2000、1 个既有首 tick console-error；WASM 10/10、0 error。三者均无硬中断、无惩罚跳过、无 missing_api。报告在 `compat/reports/bot-*.json`。
- Overmind 首 tick 的 `DirectiveHarvest.create` TypeError 与 D2 同形，随机 harvest 名字变化，错误位置与原因不变。桩触达变化：TooAngel FIND 108 次数 4667→4668、107 为 2007→2027；Overmind 114 为 378→381、107 为 38→43，新增 111 两次。路径加入真实障碍后 bot 的查询分支变化，没有新增错误；新增触达的 FIND 111 仍是既有桩。本命令仍不推进结算，不能证明经济链可玩。

## 玩家通道局部性能

Apple Silicon、release、一房平原 20 步路径；每个表达式连续搜索 1000 次，热身一轮后五轮轮换四种场景顺序。Sandbox 上限显式提高到 10000ms 以覆盖整段测量。复跑：

```bash
cargo test --release -p kk-runner --test pathfinder_perf -- --ignored --nocapture
```

| 场景 | 中位数 µs/次 | 五轮范围 |
|---|---:|---:|
| 无玩家回调 | 22.240 | 22.004–22.503 |
| roomCallback 返回 undefined | 22.935 | 22.705–23.080 |
| roomCallback 返回复用的 CostMatrix | 23.056 | 22.997–23.314 |
| Room.findPath 缓存基础矩阵 | 28.443 | 28.283–29.245 |

本负载的空回调与矩阵回调相对无回调中位数分别约 +0.695、+0.816 µs；Room wrapper 包含不同返回值构造及 JS 包装，不能将其全部差值归为矩阵复制。每场景断言总路径长度 20000。全部样本见 [原始窗口](d3-pathfinder-player-perf-2026-10-04.json)。

这些数据不是 D2/D3 二进制 A/B，也不覆盖跨房多矩阵、4/32 玩家或真实结算；正式规模与端到端性能仍归 D7。可变矩阵同步最坏会随触达房间数增加；目前优先保证行为，不能据本短程结果宣称所有场景没有回归。

## 后续边界

- Mod 的 `pathfinding.room_costs` wrapper 只完成设计，注册 SDK、可信 VM 投影准备与故障回退尚未实现；没有把玩家回调当作可信 Mod 执行入口。
- C++ 进程级地形表在多引擎并发时仍可能串地形；沿用 D2 登记，生产 CLI 一进程一引擎。本批不改地形所有权。
- 可缩放 backing store 收缩到原视图之外时，不可读区域归零为地形默认值，已在提交前复审补测。此为本地安全策略，不承诺官方原始指针越界后的行为；跨 Worker 的并发矩阵写入未提供支持。
- 下一轮优先完成普通容器的 transfer/withdraw 与 GCL 累计，再建立推进结算的真实 bot 经济场景；D4 调度、D5 查找索引、D6 清偿和 E 存储继续按原计划推进。


## 提交前 code review

按用户要求，在提交前逐层复查 C ABI、句柄真实类型、Rust 安全约定、V8 回调/异常、矩阵可变性、Room 缓存、兼容性逐例翻转和文档。

1. **修复矩阵收缩后的陈旧障碍。** 原 `MatrixSnapshot::refresh` 用 zip 只覆盖仍可读的字节，RAB 缩到零后所有旧字节保持不变，搜索继续使用已被移除的障碍。回归场景从房边 (49,25) 发起，初始矩阵在 x=48 封墙，触达下一房时将带非零 byteOffset 的 RAB 缩至零。修复前搜索返回空路径、ops=0、cost=4294967295；原生零代价参考返回非空路径、ops=422、cost=3。修复现在清零不可读尾部，逐字段与安全参考相等；参考与保留旧矩阵的结果不相等，证明夹具有判别力。上游直接指针在收缩后可能越界，故本例验证本地安全策略，不宣称对应官方未定义行为。
2. **清偿原生句柄的类型下转型。** 原兼容层 Arena 只构造 `Value`，`Local<Array/Object>::operator->/*` 却向下转换为未构造的派生对象；这不满足 C++ 下转型前提。T 仍用于句柄标签与编译期转换限制，访问器改为直接返回真实 Value 指针/引用，保留上游 pf.cc/pf.h 原样。该问题来自已有兼容层，D3 接回调后本轮一并清偿。
3. **修正 unsafe 文档约束。** ffi_search 原注释仍要求矩阵全程不可变，与 SearchHost 的“仅在 room 回调暂停期间可写”契约相冲突；现已明确稳定存储、别名、取消与独占调用约束。
4. **扩展异常与缓冲验证。** 补测 throw undefined/null/Symbol/number 的原值身份，以及 SharedArrayBuffer 的非零视图偏移；均在正式玩家搜索通路验证。原四层嵌套、getter 异常、detach、预算中断和两通道测试继续保留。

发现的问题已在提交前修复；复审不改变既有两项官方同红差异的归因，也不提高对 Mod 槽位或端到端性能的完成声明。

最终重建 release 后重新运行 `pnpm run check`（618 Rust passed、11 ignored；compat Node 14 passed）、完整 ratchet（1887/120/907、两项既定豁免、0 待归因）、三组 bot 及局部微基准，均达到本批门禁。最终 bot 无中断、无惩罚跳过，console-error 仍为 0/1/0；Overmind 错误在归一随机 harvest 名后与原 D3 相等。复审后微基准五轮全部样本见 [新窗口](d3-pathfinder-player-perf-review-2026-10-04.json)，四场景中位数依次为 22.472, 22.994, 23.086, 28.826 µs/次；未把跨会话结果当作正式 A/B。复查修复及验证后未发现剩余阻塞项。
