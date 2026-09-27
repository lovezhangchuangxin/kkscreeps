# deno_core 沙箱 × Screeps 玩家 API 功能组映射报告

> 调研日期：2026-09-27。范围：官方 engine/driver 源码（ground truth）+ xxscreeps 现代实现 + 本项目 spike/调研笔记的已验证边界。
> 路径约定：`engine/` = /Users/keqing/Desktop/projects/sources/screeps/engine/src/game/，`driver/` = /Users/keqing/Desktop/projects/sources/screeps/driver/lib/，`xx/` = /Users/keqing/Desktop/projects/sources/screeps/xxscreeps/packages/，`notes/` = docs/research/notes/。

## 0. 汇总矩阵

| # | 能力 | 官方实现（file:line） | xxscreeps 对照 | deno_core 方案 | 验证状态 | 风险 |
|---|------|----------------------|----------------|----------------|----------|------|
| 1 | 惰性对象访问（overlay） | 整 tick 数据 `ExternalCopy.copyInto`（driver/runtime/make.js:145,160）+ 原型 getter + 实例 memoize（engine/utils.js:509-529，creeps.js:71-93，rooms.js:1648-1661） | 真 overlay：房间序列化为 SAB blob（xx/schema/write.ts:308,328）+ 原型 getter 直读 buffer（xx/schema/overlay.ts:17-77），tick 末 detach（xx/driver/runtime/index.ts:212-213） | SAB 注入（实测）+ 沙箱内 JS getter 类照搬 xx 模式；类预打包进 startup snapshot | SAB 注入✅【笔记】；snapshot✅【复现】；**overlay 类入 snapshot + 每 tick 数据热替换端到端未测** | 中 |
| 2 | 原生计算 op（PathFinder） | `ivm.NativeModule` 注入（driver/runtime/user-vm.js:6-7,64）→ 沙箱内 JS 包装层把 roomCallback **作为参数传给原生**（driver/path-finder.js:114-134）→ C++ 搜索中途 `Nan::Call` 回调进沙箱 JS（driver/native/src/pf.cc:37-59，main.cc:38-45） | 同一 C++ 编译为 ivm NativeModule（xx/driver/sandbox/isolated/index.ts:22,81-91），同样原生回调（xx/driver/pathfinder/pathfinder.ts:57-74）；另支持玩家 wasm+WASI（xx/driver/runtime/module.ts:124-201） | `#[op2(fast)]` ~5ns✅；**op 执行中回调进 JS 未验证**（需 Global\<Function\> + HandleScope 重入） | fast op✅【复现】；回调重入见 spike 实验 G'（op_callback） | **高** |
| 3 | 大字符串/TypedArray 传输 | RawMemory 2MB/segments 100KB×10 以**字符串**随整包 copyInto/copyOut（driver/runtime/runtime.js:102-121,250-269,296；make.js:145）；Terrain 为 SAB 共享视图（make.js:34-35，user-vm.js:82-84，rooms.js:1223-1242） | **SAB UTF-16 零拷贝**：memory/segments 全程 Uint16Array SAB 往返，字符串懒编解码（xx/mods/meta/memory/memory.ts:107,289-308,465） | 采 xx 的 SAB-UTF16 模式（SAB✅已实测）；大字符串 v8::String 拷贝路径已有实验 H 数据 | SAB✅【笔记】；2MB 字符串 H 实测（Rust→JS 0.06ms / 回传 0.15ms） | 低-中 |
| 4 | intent 收集 | 沙箱内 intents 对象 set/push/pushByName/remove，0.2 CPU/条，免费 say/pull，可退费（runtime.js:60-100）；计入 getUsed（:202-204）；tick 末随 outMessage 一次 copyOut（:271-275） | IntentManager 纯沙箱 JS（xx/game/intents.ts:14-96），按房间组装一次传出 | **沙箱内 JS 层收集 + tick 末一次传出**（照抄），不用每 intent 一个 op | 双实现语义背书；出站序列化成本归入 #3 | 低 |
| 5 | 模块系统（多模块 CJS） | `globals.require` 跨 tick 持久 + cache + timestamp 失效（engine/game/game.js:504-512,551-580）；编译缓存跨 tick（driver/runtime/runtime-driver.js:64-94）；CJS 包装 `(function __module(module,exports){...})`（:67） | 沙箱内 makeRequire/makeModule：CJS flat loader + ESM + json + wasm（xx/driver/runtime/module.ts:12-344），缓存 Map 挂闭包跨 tick | ESM StaticModuleLoader✅【笔记】+ require 垫片✅【实验 I】（循环 require/缓存/global 持久全 PASS）；lodash 预置进 snapshot | 标准 CJS✅【实验 I】；`this===module.exports` 等罕见变体未测 | 低-中 |
| 6 | CPU 计量 | `isolate.cpuTime`（真 CPU，runtime.js:25-27）；getUsed=cpuTime+intentsCpu-基线（:202-204）；tickLimit 硬中断=ivm 墙钟 timeout（make.js:187）+宿主 5s 兜底（:300-307） | getUsed 用 `isolate.wallTime`（墙钟，xx/driver/sandbox/isolated/runtime.ts:52），tick 末用 cpuTime 结算（isolated/index.ts:139-142） | `#[op2(fast)]` 读 `CLOCK_THREAD_CPUTIME_ID`【笔记实测 ns 级】；硬中断=跨线程 terminate_execution【复现 210ms】 | 读数✅、中断✅；高频 getUsed 累计开销可忽略（~5ns×1e4=50µs） | 低 |
| 7 | console/错误/调试 | 多参数格式化=`toString().join(' ')`（弱格式化，engine/console.js:20-27）；error=e.stack 文本重写 `__module`/`_consoleN:M`（runtime-driver.js:109-120）；inspector 直连（user-vm.js:29-33） | 注入真 util.inspect/formatWithOptions（`ivm-inspect`，isolated/index.ts:93-99）；stack 截断到玩家入口帧（runtime/index.ts:136-139）；source-map-support + inline map（source-map.ts:16-35） | 沙箱内 JS console 照抄官方（零依赖）；stack 文本重写照抄；实验 K：stack 行列号捕获✅、自制收集型 console✅ | console/stack✅【实验 K】；deno_core inspector 未测 | 低-中 |
| 8 | Global reset/跨 tick 状态 | isolate 按 userId 常驻；重建时机=代码更新（user-vm.js:23-26）/自杀 halt（:17-19,68-71）/OOM（make.js:285-291）/3min 空闲（user-vm.js:160-166）；类/require.cache/玩家 global 跨 tick 保留（rooms.js:401 `if(globals.Room) return`）；Memory getter 每 tick 重定义（game.js:479-500） | 同模型；代码更新 tick **跳过执行一 tick**（xx/engine/runner/instance.ts:172-198）；Game 每 tick 新建、tick 末 `globalThis.Game=undefined`（runtime/index.ts:122,179） | 常驻 JsRuntime 的 globalThis 语义一致；重建=snapshot 下 <1ms✅；**必须遵守 LIFO 销毁纪律**（spike §8.1） | 常驻/复用/terminate 后复用✅【复现】；LIFO 下玩家生命周期管理实操未原型 | 低-中 |

## 1. 惰性对象访问（overlay 模式）

**官方（重要澄清：官方没有 `makeRoomObject` 函数，也不是 overlay）**。官方是"copy-in + 原型 getter"：
- 每 tick 全量 runtimeData（rooms/roomObjects/users/market/memory…）由宿主 `new ivm.ExternalCopy(data)` → `dataRef.copyInto()` 整体拷入沙箱（driver/runtime/make.js:144-160；数据内容见 driver/runtime/data.js:200-261）。
- 沙箱内 `game.init` 遍历 `runtimeData.roomObjects` 逐个 `new globals.Creep(i)` 等构造（engine/game/game.js:260-299）；`RoomObject` 基类工厂在 engine/game/rooms.js:1648-1661。
- 惰性来自 `utils.defineGameObjectProperties`（engine/src/utils.js:509-529）：getter 定义在 **prototype** 上，首次访问执行映射函数并把结果 memoize 为实例属性 `this['_name']`，数据源是已拷入的 `runtimeData.roomObjects[id]`。
- 类与原型只建一次（rooms.js:401-403 `if(globals.Room) return`），跨 tick 复用；只有 runtimeData 每 tick 换。
- 唯一的"共享内存"是全图地形：宿主侧放进 SharedArrayBuffer（make.js:34-35），`ExternalCopy.copyInto({release:true})` 注入（user-vm.js:81-84），沙箱内 `_setStaticTerrainData` 为每房建 2500 字节 Uint8Array 视图（runtime.js:45-49），`Room.Terrain.getRawBuffer` 直接从该视图 new TypedArray（rooms.js:1223-1242）。

**xxscreeps 才是真 overlay**：房间状态序列化为 SAB blob（xx/schema/write.ts:308,328），`RoomSchema.read(blob)` 得到挂在 blob 上的 Room（xx/driver/runtime/index.ts:103-107），getter 由 `injectGetters` 注入原型——**整数成员不 memoize 每次直读 buffer，非整数 memoize 为实例属性**（xx/schema/overlay.ts:29-76）；tick 末 `detach()` 把视图换成抛错 getter 吊销访问（buffer-view.ts:38-47；runtime/index.ts:210-213）；并用冻结 TypedArray 构造器防止玩家改共享地形（runtime/index.ts:43-60）。

**deno_core 判定**：SAB BackingStore 跨 isolate 零拷贝已实测（notes/sandbox-denocore.md §2.7，2 isolate/4096B）；沙箱内 JS 层 getter 是纯 JS、零宿主依赖，照搬 xx 模式**成立**。实验 F 已实测：SAB+prototype getter 1.7~2.1ns/读，op-per-read 慢 1.0~1.2x（单属性）/1.5x（4 属性）——op-per-read 无数量级灾难但 SAB+getter 更优，维持"属性走数据、op 走动作"。snapshot：实验 J 已验证类/getter/冻结常量/闭包全部随 snapshot 恢复（创建 3.2x 加速）。
**残留缺口**：① 每 tick 数据热替换 + detach 只读控制的完整端到端（组件全过）；② SAB 注入的可写性需自制只读策略（冻结/detach）；③ N isolate 规模压测（02 报告 §6.4-3）。

## 2. 原生计算 op（PathFinder.search / Game.map / room.findPath）

**官方注入链**（确切证据）：宿主 `new ivm.NativeModule(native.node)` → `nativeMod.create(context)` → `context.global.setIgnored('_nativeMod', ...)`（user-vm.js:6-7,39-64）；沙箱内 `pathFinder = pathFinderFactory.create(global._nativeMod)`（runtime-driver.js:124-125）；引擎侧 `PathFinder.search` 只是薄校验后转调 `driver.pathFinder.search`（engine/game/path-finder.js:57-65；CostMatrix 定义在同文件 18-48）。

**roomCallback 调用模式——确凿证据：原生执行中途回调进沙箱 JS，不是预收集**：
- JS 包装层（driver/path-finder.js:114-134）把玩家回调包成 `(xx,yy) => { const ret = cb(generateRoomName(xx,yy)); return ret===false ? false : ret._bits }`，**作为第 3 个参数传给 `mod.search(...)`**（:134）。它不做任何预收集。
- C++ 侧 `search` 把 `info[2]` cast 成 `v8::Function`（native/src/main.cc:40）；搜索进入每个新房间时 `Nan::Call(*room_callback, ...)` **在 A* 主循环中途回调进沙箱**，玩家返回 `false` 则拉黑该房、返回 Uint8Array(2500) 则直接取其 TypedArray 数据指针作 cost matrix，回调抛异常则 `ReThrow` 并以 js_error 终止搜索（native/src/pf.cc:36-59）。
- 其余"计算"是纯 JS：`Game.map.findRoute` 是沙箱内 JS A*（engine/game/map.js:69-100 + path-utils.js 的 Heap/OpenClosed）；`room.findPath` 也是 JS 层（rooms.js:863 起）。
- 宿主启动时把全图地形 2bit 打包后 `mod.loadTerrain`（driver/path-finder.js:23-46；make.js:31）。
- xxscreeps 用同一 C++ 源码编译为 ivm NativeModule（xx/packages/pathfinder/src/ 与 module/iv.cc），路径同构（xx/driver/pathfinder/pathfinder.ts:57-74）；差异仅在于额外支持玩家 wasm+WASI 模块（xx/driver/runtime/module.ts:124-201）。

**deno_core 判定**：`#[op2(fast)]` ~5ns/call 已实测，数据进出无问题（实验 G：TypedArray 零拷贝✅）。**关键缺口是 op 执行中回调进 JS**：deno_core 的 op2 fast 路径只收原始类型。路线：
- (a) op 内接 `v8::Global<v8::Function>` 参数（op2 `#[global]` 属性），Rust 侧在 op 栈内 `Function::call` 回调（同线程同 isolate，V8 可重入）——**spike 实验 G'（op_callback）专测此项**；
- (b) 预收集模式（实验 G 已验证性能 ~0.6µs/search）：**语义不等价**——官方是惰性、只对真正进入的房间调回调（预收集会多调玩家回调=多计费/多副作用），且 `false` 不计入 maxRooms 的语义无法预先判定；
- (c) 沙箱内 wasm pathfinder：回调变纯 JS 调用，但失去原生性能。
**处置**：(a) 通过则照官方架构；(a) 失败则 fallback (c)（xx 的 WASI 基建可参考），(b) 仅作过渡。

## 3. 大字符串 / TypedArray 传输

**官方**：RawMemory.get 返回的字符串是 copyInto 进来的副本（runtime.js:102-121，2MB 上限）；tick 末 segments 校验（≤10 条、各 ≤100KB，runtime.js:250-269）后连同全部结果经 `new ivm.ExternalCopy(outMessage).copyInto()` 一次拷出（:296）。Terrain 不暴露位串，`getRawBuffer` 返回 2500 字节 Uint8Array（拷贝）。CostMatrix.serialize 返回普通 Array（engine/path-finder.js:40-48，给玩家存 Memory）。

**xxscreeps 的现代化方案（推荐照抄）**：Memory 主串与 segments 全程以 **Uint16Array + SharedArrayBuffer** 承载——初始化 SAB copy 进（memory.ts:315-325），`RawMemory.get` 懒解码成字符串并缓存（:106-108），tick 末 `utf16IntoBuffer` 写回 SAB、以 `new Uint8Array(memory.buffer, 0, len<<1)` 零拷贝出站（:289-308）；segments 出站 `utf16ToBuffer(string, SharedArrayBuffer)`（:465）；foreign segment 懒解码 getter（:416-437）。彻底消灭 2MB 字符串跨边界拷贝。

**deno_core 判定**：SAB 通道已实测；采 xx 的 SAB-UTF16 模式则 #3 风险趋近于零。实验 H 补充了官方式字符串通道的真实数字：2MB Rust→JS 0.06ms、沙箱内 parse+stringify 8.4ms、JS→Rust 0.15ms——**边界成本可忽略，瓶颈在沙箱内 JSON 本身**（这正是 03 报告 Memory 增量化的主题）。

## 4. intent 收集

官方机制全貌：intents 对象在 `_start` 内每 tick 重建（runtime.js:63-100）——`set(id,name,data)` 同对象同名幂等（只首次计 0.2 CPU）、`push(name,data,maxLen)`（notify 上限 20）、`pushByName`、`remove` 可退费；`say/pull` 免费（freeMethods，:61）。计费实时并入 `getUsedCpu`（:202-204）。tick 末 `intentsList` 随 outMessage 一次传出（:274-275），宿主侧 `storeIntents` + `0.2*intentsCount` 复核（make.js:98-111,192-194）。xxscreeps 的 IntentManager 同构（kCpuCost=0.2、save 幂等计费、remove 退费，xx/game/intents.ts:9-88），tick 末按房间打包一次传出（xx/driver/runtime/index.ts:195-204）。

**deno_core 判定：沙箱内 JS 层收集 + tick 末一次传出明显更优**。理由：① 0.2 计费/免费名单/幂等/退费逻辑天然是沙箱内 JS（两个实现均如此，直接照抄）；② `getUsed` 需要实时叠加 intents.cpu（runtime.js:203），走 op 上报会引入跨边界一致性；③ 每 intent 一个 op 虽仅 ~5ns，但对象参数走 serde 慢路径（77-136ns+）且宿主要复刻全部计费语义。风险：低（唯一成本是 tick 末一次出站序列化，归入 #3 的 SAB/大对象通道）。

## 5. 模块系统（玩家多模块 CommonJS）

官方：`globals.require` 挂在 global（跨 tick 持久），带 `cache`（预置 lodash：`require.cache = {lodash: _}`，game.js:510）与 `timestamp`（代码更新才换 require，game.js:504-512）；requireFn 为 flat namespace（去 `./`，game.js:551-580），binary 模块 base64→Buffer。执行层 `evalCode` 把模块包成 `(function __module(module,exports){ code })(__module, __module.exports)`，按 user+module+timestamp 缓存编译后的 ivm Script 跨 tick 复用（runtime-driver.js:64-94），timeout+5ms 下限 30ms（:40-45）。每 tick `require('main')`（缓存命中）→ `module.exports.loop()`（game.js:519-533）；控制台表达式经 `(function(code,module,exports){ return ""+eval(code) })` 执行（runtime-driver.js:59）。

xxscreeps：CJS/ESM 双轨（main.js→flat require；main.mjs/main.wasm→ESM loader），makeRequire 含循环检测、main.loop 补全、require.cache 的 Proxy 视图（xx/driver/runtime/module.ts:229-303）；ESM linker + .json/.wasm(WASI) 支持（:305-344, 82-201）。

**deno_core 垫片具体做法**：
- ESM 玩家：`StaticModuleLoader`（HashMap<specifier, code>），代码更新换 map 或重建 isolate；
- CJS 玩家：**不建议走 op 逐次取源码**，直接把 xx 的 `makeRequire` 移植为沙箱内运行时（它只依赖一个 `evaluate(source, filename)` 原语，deno_core 侧用 `execute_script` 实现即可）；模块缓存放 `globalThis.require.cache` 天然跨 tick，timestamp 失效语义照抄官方；
- `global` 持久：常驻 isolate 的 globalThis 即持久，与官方一致；`require.initGlobals`（game.js:522-524）与 `_.runInContext`（game.js:475-477）需一并复刻；
- lodash 底座与 require.cache 预置项进 snapshot（snapshot 预热已实测）。
**实验 I 已验证**：循环 require（半成品+写穿可见）、exports 三处共享、缓存命中不重执行、global 跨 execute_script 持久——全 PASS。**残留风险**：`this===module.exports`、`Object.defineProperty(require.cache, ...)` 等罕见变体；snapshot 内 require 基建 + 运行时模块表注入组合。

## 6. CPU 计量注入

官方 `getUsed` 的实现是 `Number(isolate.cpuTime)/1e6 + intents.cpu - startTime`（runtime.js:25-27,202-204）——ivm 的 isolate.cpuTime 是引擎侧提供的真 CPU 时间。`Game.cpu` 暴露 getUsed/tickLimit/limit/bucket/getHeapStatistics/halt（engine/game/game.js:146-157）。硬中断两层：ivm `run.apply(..., {timeout})` 墙钟硬中断（make.js:187；超时错误文本被改写为 "CPU time limit reached"，runtime-driver.js:105-107）+ 宿主 `setTimeout(max(5000, mainLoopResetInterval))` 兜底清 isolate（make.js:300-307）。玩家自杀 `Game.cpu.halt` → `_halt` Reference → `isolate.dispose()`（user-vm.js:68-71），下 tick 重建（:17-19）。xxscreeps 的 getUsed 反而用墙钟 `isolate.wallTime`（xx/driver/sandbox/isolated/runtime.ts:52），CPU 真值仅用于 tick 末结算（isolated/index.ts:139-142）——说明墙钟读数在产品上可接受。

调用频率场景：玩家代码常在每 creep 循环、日志节流、决定是否继续搜索时调用 getUsed，一 tick 几十至上千次。官方是属性读+算术（≈0）；deno_core 方案是 fast op（~5ns），累计影响完全可忽略（1e4 次=50µs）。tickLimit 配合：宿主看门狗线程按"预算-已用"调 `terminate_execution()`（跨线程已实测 210ms 级精度）。注意 macOS 无 POSIX per-thread CPU timer，开发环境用轮询/墙钟（02 报告 §6.1-8）。**待定标**：getUsed 用 CPU 时钟（官方）还是墙钟（xx）——两者都被市场接受，建议 CPU 时钟保真。

## 7. console / 错误 / 调试

- **console 多参数格式化**：官方极简——`_.map(arguments, i => i.toString() : JSON.stringify(i)).join(' ')`（engine/console.js:20-27），并叠加 visual 通道（addVisual 字符串累积，500KB/1MB 上限，console.js:57-95）；消息 tick 末随 outMessage 传出（runtime.js:277-285）。xxscreeps 注入真 `util.formatWithOptions/inspect`（ivm-inspect NativeModule，isolated/index.ts:93-99）并重写 console 全家（console.ts:4-57；环形 50 条+重复折叠 print.ts:20-71）。
- **未捕获异常**：官方 `_start` 闭包 catch 后取 `e.stack || e.toString()`（runtime.js:241-244）；文本层重写去掉 `__module` 包装帧与 `_consoleN:M`（runtime-driver.js:109-120）。xx 截断到玩家入口帧（runtime/index.ts:136-139）。
- **source map**：官方无（模块名即 filename，runtime-driver.js:23）；xx 有完整方案——runtime 自带 map + 玩家模块从源码尾部 inline `sourceMap_mappingURL` 提取，source-map-support 安装进沙箱（xx/driver/runtime/source-map.ts:16-35）。
- **inspector**：官方 `ivm.Isolate({inspector})` 直连（user-vm.js:29-33）；xx 同（isolated/index.ts:20,78,119-121）。

**deno_core 判定**：console 与 stack 重写都是沙箱内纯 JS，照抄官方即可。实验 K 已验证：未捕获异常 `.stack` 精确行列号捕获✅；deno_core 自带 console 直写宿主 stdout（不可捕获）→ 需遮蔽并注入自制收集型 console（30 行 JS 已验证：多参数/嵌套/循环引用/Error 格式化）。**残留**：inspect 增强格式化（自带实现）、source map 重映射（若提交期转译才需要）、deno_core inspector 会话。

## 8. Global reset 与跨 tick 状态

官方语义（全部取证）：
- **保留跨 tick**：isolate 内的一切——engine bundle 模块级变量（`if(globals.Room) return` 的类与原型，rooms.js:401；mapGrid 只建一次，runtime.js:206-208；staticTerrainData 每 isolate 一次）、runCodeCache（game.js:442-468）、`globals.require.cache`（玩家模块闭包与状态）、`globals._`（`_.runInContext` 实例）、玩家在 globalThis 上写的一切与 prototype 修改（custom-prototypes.js 供 mod 扩展原型）、玩家 `_.memoize` 缓存。
- **每 tick 重置**：runtimeData（新包 copyInto 后 `game.init` 重建 Game 与 Memory getter——Memory 是 configurable getter 每 tick 重定义、首次访问 JSON.parse 后自我替换为值属性，game.js:479-500）、intents（runtime.js:63）、Game 本体。
- **重建 isolate 的时机**：代码 timestamp 更新（user-vm.js:23-26）、`isolate.isDisposed`（玩家 halt 或超限，:17-19）、OOM/AB 分配失败（make.js:285-291）、3 分钟未用（user-vm.js:160-166）。
- xxscreeps 差异：代码更新 tick 直接跳过玩家执行一 tick（instance.ts:172-198）；`Game` 每 tick 新建、tick 末 `globalThis.Game = undefined`（runtime/index.ts:122,179）；旧 tick 游戏对象被 detach 成抛错对象（:210-213）——比官方"旧对象仍指向旧 runtimeData"更严格。

**deno_core 判定**：常驻 isolate 的 globalThis/堆语义与官方**天然一致**（同为常驻 V8 isolate 模型，isolated-vm README 也确认官方 isolate "persist for several days"）；每 tick 重置部分是纯 JS 层复刻，不涉宿主能力。重建成本：snapshot 下 <1ms、边际 ~2MB（实测）。**工程约束**：isolate 必须 LIFO 逆序销毁（spike §8.1 实测 fatal）——"踢中间玩家/玩家自杀重建"要按栈序处理或整批重建，这是与官方 `vms` map 随意 clear 最大的行为差异点；terminate 后同 runtime 复用已实测（cancel_terminate 自动）。

## 9. 缺口清单（需补实测，按优先级）

1. **op 内回调进 JS**（#2 roomCallback 路线 a）：Global\<Function\> 参数 + HandleScope 内 `Function::call` + 异常传播 + 重入深度——最高优先，决定 PathFinder 架构。→ 已由 spike 实验 G'（op_callback）补测，见 notes/spike-deno-game-api.md。
2. **overlay 端到端原型**（#1）：snapshot 预建 getter 类 + SAB 数据热替换 + 只读策略（冻结/detach/代理）+ N isolate 规模压测。
3. **SAB-UTF16 Memory 通道**（#3）：2MB SAB 往返 + 懒解码字符串的真实 tick 成本。
4. **CJS 垫片变体**（#5）：`this===module.exports`、require.cache 篡改、binary 模块（.wasm ArrayBuffer）；与 snapshot 组合。
5. **console/调试增强**（#7）：自带 inspect、source map 重映射、deno_core inspector 会话。
6. **多线程宿主 + LIFO 生命周期**（#8/#6）：代码更新/自杀重建在"一 isolate 一线程"模型下的销毁纪律。
