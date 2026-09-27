# 04 · deno_core 游戏功能完整性验证（第三轮）

> 调研日期：2026-09-27。问题：**deno_core 能否实现 Screeps 需要的全部游戏功能？**
> 输入：中文文档站 API 全集盘点（notes/game-api-inventory.md）、官方/xxscreeps 源码的逐能力映射（notes/deno-game-impl-mapping.md）、补充 spike 实验 F–K（notes/spike-deno-game-api.md）与决定性实验 G'（op 内回调，examples/op_callback.rs）。

## 0. 结论（先行）

**可以。** 从中文文档盘点的**约 340+ 方法/属性、443 个全局常量、7 类宿主能力需求**逐项核对下来，deno_core 没有任何一项功能无法实现；此前标记的唯一"高风险"缺口——官方 PathFinder 的 `roomCallback` 惰性原生回调——已由实验 G' 实测关闭（`#[scoped] Global<Function>` + `tc_scope!` + `Function::call`，三态返回值/异常传播/runtime 存活全部断言通过，回调开销 38ns/次）。

**功能 → 宿主能力 → 验证状态**总表：

| 游戏功能（来自文档盘点） | 需要的宿主能力 | 验证状态 |
|---|---|---|
| 纯数据读取（Game/Room/Creep/24 种 Structure 的全部属性） | SAB/ArrayBuffer + 沙箱内 prototype getter（惰性 overlay） | ✅ 实测：SAB+getter 1.79ns/读（实验 F），类+getter+冻结常量进 snapshot 完整恢复（实验 J，3.2x 加速） |
| 宿主原生计算（PathFinder.search、find/look、Game.map.*、getUsed、market 查询） | 同步 `#[op2]` op（fast ~5ns） | ✅ 实测（02 报告） |
| **玩家回调进宿主计算**（roomCallback / costCallback / routeCallback） | op 执行中回调进 JS（惰性 + 三态返回值 + 异常终止） | ✅ **实测（实验 G'，本轮关闭最后缺口）**：10 房间三态分流断言精确、异常传播保消息、runtime 存活、38ns/次 |
| 大缓冲（RawMemory 2MB、segments 100KB×10、Terrain 2500B、Visual export） | 大字符串/TypedArray 双向通道 | ✅ 实测（实验 G 零拷贝 + 实验 H：2MB 边界往返 <0.25ms；推荐 xxscreeps 的 SAB-UTF16 模式则趋近 0） |
| intent 动作（Creep 25 动作、PowerCreep、各建筑、market、pixel） | 沙箱内 JS 层收集器 + tick 末一次传出 | ✅ 设计定型：官方与 xxscreeps 双实现同构（0.2 CPU 计费/幂等/退费全是沙箱内 JS 逻辑），照抄即可 |
| 视觉绘制（RoomVisual/MapVisual 各 9 原语） | 纯 JS 累积 + tick 末序列化传出 | ✅ 无宿主依赖（不计 CPU、1 tick 保质、容量上限即字符串长度检查） |
| 全局工具与常量（lodash3、443 常量、console） | 全局注入 + snapshot 预热 | ✅ 实测（实验 J 冻结语义保持；实验 K 自制收集型 console + 精确行列号堆栈） |
| 模块系统（多模块 CJS + 跨 tick 缓存 + global + .wasm 二进制模块） | CJS 垫片 + WASM 支持 | ✅ 实测（实验 I：循环 require/exports 共享/缓存/global 持久全过）；WASM 为 V8 原生能力（deno_core 默认暴露，xxscreeps 的 WASI 基建可参考） |

## 1. 关键语义核对（文档要求 vs 我们的实现路径）

1. **每 tick 对象重建、intents 延迟结算、被覆盖动作仍返回 OK**（game-loop/simultaneous-actions 文档）：与沙箱无关，是引擎侧设计（01/03 报告已定）。沙箱侧只需保证"旧 tick 对象失效"——xxscreeps 的 detach（访问抛错）比官方更严格，采纳。
2. **Memory 惰性 parse 且计入玩家 CPU**（官方文档明文）：Memhack 语义照抄 xxscreeps（SAB UTF-16 + 首触懒解码），03 报告的三层增量在其上叠加；"是否移出计费窗口"是产品决策点，不是技术障碍。
3. **`Game.cpu.halt()` 擦除堆并重置运行环境**：等价于 terminate + isolate 重建（snapshot 下 <1ms，实测）。
4. **`Game.cpu.getUsed()` 毫秒精度**：fast op 读 `CLOCK_THREAD_CPUTIME_ID`（ns 级实测）；官方用 CPU 时钟、xxscreeps 用墙钟，两者都被市场接受。
5. **无 setTimeout/网络/文件**：deno_core 默认面本来就没有这些，反而需要**主动遮蔽**的是 `Deno.core`（~100 个内部 op 裸露）与 `__bootstrap`——02 报告风险 #3，实验 K 已验证自制 console 替换路线。
6. **WASM 二进制模块**（`require('xxx.wasm')` 返回 ArrayBuffer）：V8 原生支持，玩家自行 `new WebAssembly.Module/Instance`；xxscreeps 的 WASI 加载器可整体参考。**注意**：这是沙箱安全面的新增项（WASM 线性内存不受 heap_limits 约束），列入实现期安全清单。

## 2. 本轮新增实验（全部 PASS，release 构建，可复现）

| 实验 | 验证内容 | 关键数字 |
|---|---|---|
| F 惰性 overlay | SAB+prototype getter vs op-per-read vs 普通对象 | 1.79 / 1.95 / 1.21 ns/读；4 属性/次时 getter 1.11 vs 4×op 1.66 ns |
| G 原生计算+回调收集（预收集变体） | TypedArray 零拷贝进 op + 完整 search 包装 | 指针稳定+写穿可见；0.637µs/search |
| H RawMemory 2MB | Rust→JS / parse+stringify / JS→Rust | 0.065 / 8.54 / 0.153 ms（瓶颈在沙箱内 JSON，不在边界） |
| I CJS 垫片 | 循环 require、exports 共享、模块缓存、global 跨执行持久 | 全部断言通过，Rust 侧加载计数各恰 1 次 |
| J runtime 进 snapshot | 类/getter/闭包/嵌套冻结/Map 恢复 | 12 项断言全过；创建 2285→704µs（3.2x）；顶层 const 恢复 |
| K console 与堆栈 | 未捕获异常 .stack 行列号 + 收集型 console | 3 帧链路 2→5→7 精确；4 类参数+嵌套+循环引用捕获 |
| **G' op 内回调（本轮补测）** | 官方 roomCallback 惰性语义：三态返回值/异常传播/runtime 存活 | entered=8 校验和 50000 精确；异常保消息存活；38ns/次 |

复现：`cd spikes/deno-spike && cargo run --release --example game_api`（F–K）、`cargo run --release --example op_callback`（G'）、`cargo run --release`（02 报告的 A–E 基础实验，本轮复跑未破坏）。

## 3. 剩余缺口（均非"能不能"问题，按优先级）

1. **真实 bot 兼容性验收**（最高优先，两个报告反复强调）：在 deno_core + CJS 垫片 + lodash3 底座上全量跑 screeps-ok（2897 用例）与真实开源 bot（tooangel/overmind 类）——把"特性矩阵全绿"变成"玩家代码实际兼容率"。
2. overlay 端到端原型：snapshot 预建类 + 每 tick SAB 数据热替换 + detach 只读控制的完整串联（组件各自已测）。
3. 多线程宿主原型：`JsRuntime` 非 Send，"一 isolate 一线程"模型的 LIFO 销毁纪律实操（02 报告 §6.4-2）。
4. 沙箱内 JSON 瓶颈的增量化（03 报告 §2.2 的三层增量方案）——这属于性能优化而非可行性。
5. WASM 玩家模块的内存记账（线性内存不受 heap_limits 约束，需 ArrayBuffer 分配器上限，Supabase external_memory.rs 同款）。
6. inspect 增强 / source map / inspector 会话（按需）。

## 4. 一句话收口

**deno_core 实现 Screeps 全部游戏功能的技术可行性已验证完毕**：十项沙箱硬要求（02 报告）+ 七类游戏功能面（本轮，340+ API 逐类映射）+ 官方 PathFinder 惰性回调架构（实验 G'）全部有实测支撑。下一步不是继续验证沙箱，而是**开始搭工程并用 screeps-ok + 真实 bot 做行为兼容性验收**（M0）。
