# deno_core inspector + deno_inspector_server 断点调试实测笔记

- 日期：2026-09-28
- 前置篇：[spike-deno_core.md](spike-deno_core.md)（实验 A~E，§11 "inspector 未测" 项由本篇关闭）、[spike-deno-game-api.md](spike-deno-game-api.md)（实验 F~L）
- 实验对象：`deno_core` 0.412.0（`deno_v8` 0.4.0 → `v8` crate 150.4.0）+ `deno_inspector_server` 0.33.0（该 crate 依赖 `deno_core ^0.412.0`，版本精确对齐；Deno CLI `--inspect` 同款实现，hyper + fastwebsockets）
- 代码位置：`spikes/inspector-spike/`（独立 crate，不在 workspace 内）
- 复现命令（在本目录 `spikes/inspector-spike/` 下）：

```bash
cargo run --release
# 判定：末行 "exit 0" = M/N/O/P/Q 五项全 PASS；任一 FAIL 时进程退出码 1；300s 全局死锁保护退出码 2
```

## 汇总（5/5 PASS）

| 实验 | 问题 | 结论 |
|---|---|---|
| 实验M | CDP 断点全链路（服务器/发现/断点/暂停/求值/恢复）可行吗 | PASS |
| 实验N | 断点暂停 × 看门狗怎么共存 | PASS：开着看门狗必被掐；debug 档 = 暂停前 disarm |
| 实验O | inspector 开关是否泄漏玩家可见全局 | PASS：遮蔽后两档全局面完全一致（70 == 70） |
| 实验P | 启动快照 runtime 可调试吗 | PASS：断点命中且暂停帧内可求值快照全局 |
| 实验Q | 多 isolate 独立 target + LIFO 销毁 | PASS：A 暂停不阻塞 B；销毁后 target 自动注销 |

## 0. 精确依赖版本（Cargo.lock 实测锁定）

| 包 | 版本 | 说明 |
|---|---|---|
| deno_core | 0.412.0 | 与 workspace 同版本（ADR-002 纪律） |
| deno_inspector_server | 0.33.0 | CDP over WebSocket 服务（HTTP `/json`、`/json/list`、`/json/version` + WS upgrade），0.33 依赖 `deno_core ^0.412.0` |
| v8（rusty_v8） | 150.4.0 | 引擎版本串 15.0.245.2-rusty |
| tungstenite | 0.26.2 | 辅助线程同步 WS 客户端（注意 0.26.2 的 `Message::Text` 已是 `Utf8Bytes`） |
| ureq | 2.12.1 | `/json/list` HTTP 探测（default-features = false，纯 http） |
| serde_json | 1.0.151 | CDP 消息编解码 |
| rustc / cargo | 1.98.1 | rust-toolchain.toml 锁定 |
| 平台 | macOS arm64（darwin 24.6.0） | |

## 1. 实验M CDP 全链路 —— PASS

**方法**：主线程按生产路径镜像建 runtime（`RuntimeOptions { inspector: true, create_params: heap_limits(256MB) }` + near-heap 双闸门 + 常驻看门狗 + 遮蔽 `Deno`/`__bootstrap` + 收集型 console 垫片，见 `spikes/inspector-spike/src/main.rs` 的 `DebugRuntime`）；`InspectorServer::new("127.0.0.1:0", …)` 起服后 `register_inspector(module_url, rt.inspector(), false)` 取回 ws URL；辅助线程用同步 WS 客户端执行 `Runtime.enable → Debugger.enable → Debugger.setBreakpointByUrl`（urlRegex 匹配脚本名，行号=断点行的 0-based）；主线程 `execute_script` 执行含断点行的脚本（阻塞在暂停里），辅助线程收到 `Debugger.paused` 后 500ms 发 `Debugger.resume`，脚本继续跑完。全程机器可判定。

**关键输出**（逐字）：

```
   execute: ok=true elapsed=506ms err=None
   paused: line=Some(6) col=Some(3) reason=Some("other") hitBreakpoints=1 bpId=Some("2:5:0:kk-spike-m\\.js$") bpLocations=0
   scriptParsed urls=["kk-mask", "ext:core/mod.js", "ext:core/01_core.js", "ext:core/02_timers.js", "file:///tmp/kk-spike-m.js"] breakpointResolved=["2:5:0:kk-spike-m\\.js$"] msgsTotal=9 firstMsgs=[…"Debugger.breakpointResolved", "Debugger.paused", "Debugger.resumed"]
   trace=["before", "after", "end"] __r=Some(Number(42)) resumed=true helperErrors=[]
```

（引文为逐字节选，中间省略处以 … 标注；完整原样输出以本机复跑为准——见篇首复现命令。）

1. ✓ `RuntimeOptions.inspector: true` → `JsRuntime::inspector(): Rc<JsRuntimeInspector>`（jsruntime.rs:1430）→ `register_inspector` 的整条链路开箱即用，**无需其它引擎级开关**；
2. ✓ `/json/list` 发现端点工作（注册前 0 target、注册后可见、带 `webSocketDebuggerUrl`）——【文档】Chrome `chrome://inspect` 的 "Discover network targets" 消费该端点；响应里的 `devtoolsFrontendUrl` 字段由 deno_inspector_server 的 `get_frontend_url` 生成（lib.rs），可直开 DevTools 前端；
3. ✓ 断点在第 6 行精确命中（line/col/hitBreakpoints 齐全）、`Debugger.resume` 后脚本完整执行、`__r` 等终态正确；
4. ✓ `Debugger.scriptParsed` 对所有脚本流出（含 deno_core 内部 `ext:core/*`），`breakpointResolved` 正常推送。

## 2. 实验N 暂停 × 看门狗 —— PASS

**方法**：同一断点脚本两轮。(a) 反面：执行前 `watchdog.arm(500ms)` 不解除，辅助线程持有暂停 1.5s；(b) 正面（debug 档语义）：`arm(10s)` 后立即 `disarm()`，持有暂停 2s。

**关键输出**：

```
   (a) arm(500ms)不解除: ok=false elapsed=1506ms err=Some("Uncaught Error: execution terminated") paused=Some(4) reused_ok=true
   (b) arm→disarm后执行: ok=true elapsed=2002ms trace=["before", "after", "end"] __r=Some(Number(42)) paused=Some(4) resumed=true
```

**结论**：

1. ✓ **开着看门狗打断点必被掐**：terminate 标志在暂停期间照常投递，但展开时机有一层【推断】——elapsed 是 1506ms（≈持有时长）而非 500ms，据此推断**被 park 的暂停线程要等到下一条 CDP 消息到达（resume）被唤醒后终止才展开**（未做"暂停期间发无关消息"的隔离对照，归未尽事项）。看门狗 terminate 抢救不了调试会话，也不该抢救；
2. ✓ 中断后 `cancel_terminate_execution()` + 再执行新代码验证 runtime 可复用（`reused_ok=true`）；
3. ✓ **debug 档语义 = 该玩家当 tick 暂停前 disarm**（或干脆不 arm）：暂停 2s 后正常恢复、脚本完整跑完、`Debugger.resumed` 通知收到。这是 M2-2A 沙箱接入的硬约束实证。

## 3. 实验O 全局面 diff —— PASS

**方法**：`inspector: true` 与 `false` 各建一个 runtime，同一段遮蔽垫片（`delete globalThis.Deno; delete globalThis.__bootstrap;`）后取 `getOwnPropertyNames(globalThis)` 排序对比。

**关键输出**：

```
   inspector:true=70 个全局, false=70 个全局, 仅true=[], 仅false=[]
```

**结论**：✓ 开启 inspector **不改变**遮蔽后的玩家可见全局面——sandbox.md 的全局面白名单测试不需要为 debug 档做豁免。

## 4. 实验P inspector × 启动快照 —— PASS

**方法**：build.rs 用 `deno_core::snapshot::create_snapshot` 生成含全局函数（`__snapFib`）与常驻结构的快照；`RuntimeOptions { inspector: true, startup_snapshot: Some(…) }` 建 runtime，断点行命中后在暂停帧 `Debugger.evaluateOnCallFrame` 求值快照全局。

**关键输出**：

```
   paused=Some(3) evals=[("__snapFib(10)", Number(55), true), ("SNAPSHOT_PRELOADED", Bool(true), true)] trace=["call", "done"]
```

**结论**：✓ 快照 runtime 完全可调试，暂停帧的调试求值能直接访问快照固化的全局——对"游戏 runtime 进快照"（sandbox.md 启动加速方案）无调试代价。

## 5. 实验Q 双 isolate 双 target —— PASS

**方法**：两线程各建一个 runtime（一实例一线程，kk-runner 模型；先 A 后 B，ready 顺序同步保证创建序），各注册独立 target；A 断点暂停期间 B 执行 2×10⁷ 迭代计算脚本（跑两轮：一轮在 A 暂停期间、一轮在 A 恢复后）；结束 B 先 A 后销毁（LIFO 纪律）。

**关键输出**：

```
   /json/list: 2 个 target, 含A=true 含B=true
   A paused_at_4=true A done ok=true | B run1 ok=true run2 ok=true
   LIFO销毁: B先=true A后=true | A paused line=Some(4) | 销毁后 target 数=0（预期 0）
   helper errors: A=[] B=[]
```

**结论**：

1. ✓ **每玩家一个 isolate = 每玩家一个独立 DevTools target**，`/json/list` 同时可见两个，会话互不干扰；
2. ✓ A 在断点暂停（3s）期间 B 照常执行完毕——**暂停一个玩家不会阻塞世界**，这是单进程多 isolate 结构对"多玩家服上 debug"的天然优势（【文档】xxscreeps 的 `sandbox: unsafe`（node:vm 同 isolate）+ `node --inspect` 路径会暂停整个进程——其 docs/architecture.md）；
3. ✓ runtime Drop 自动触发 target 注销（deno_inspector_server 的 `add_deregister_handler` 通道），LIFO 销毁后 `/json/list` 清空；一个 InspectorServer 贯穿全部 5 个实验、6 次 runtime 注册/注销循环（M×1 + N×2 + P×1 + Q×2，实验 O 的两个 runtime 不注册）无异常。

## 6. 踩坑记录（都是实测撞出来的，含症状与根因）

1. **【最重】泵事件循环绝不能带 `wait_for_inspector: true`**：空闲事件循环 + 挂着 WS 会话（deno_inspector_server 建的是 `InspectorSessionKind::NonBlocking { wait_for_disconnect: true }`）时，`run_event_loop(PollEventLoopOptions { wait_for_inspector: true })` 会走 Deno 的优雅退出分支——`inspector.context_destroyed(主上下文)`（deno_core 0.412 runtime/jsruntime.rs:2641-2647），**此后 scriptParsed / 断点全部失效且不可恢复**。症状：收到一条 `Runtime.executionContextDestroyed`、`setBreakpointByUrl` 永远 `locations: []`、零 scriptParsed。修复：泵送一律 `wait_for_inspector: false`（会话握手与消息分发挂在事件循环每次 poll 上，不依赖该标志）。
2. **断点按 URL 绑定，脚本名必须是绝对文件路径**：deno_core 的 `resource_name_to_url`（inspector.rs，V8InspectorClientImpl）只把能 `Url::from_file_path` 的资源名转成 `file://` URL，其它（如裸名 `"spike-m"`）→ 无 URL → `setBreakpointByUrl` 的 urlRegex 永远匹配不上（实测裸名时 `locations: []`、不断点；改为 `/tmp/kk-spike-m.js` 后 `scriptParsed` 报告 `file:///tmp/kk-spike-m.js` 并命中）。**生产含义：kk-runner 的玩家模块脚本名需绝对路径化（如 `/main.js`），DevTools 断点才开箱可用**——已列入 M2-2A 设计约束。
3. **CDP 请求/响应只在 isolate 线程泵事件循环时被处理**（`poll_sessions_from_event_loop` 挂在 `run_event_loop` 上）：等待握手/enable/setBreakpoint 往返期间必须 pump；**暂停期间例外**——V8 的 `runMessageLoopOnPause` 在 `poll_sessions(None)` 里以 `thread::park` 独立运转（inspector.rs:474-478），`execute_script` 阻塞路径可行。空等 Ready 不泵送的最初版本表现为 helper 的 `Runtime.enable` 永无响应、后续超时级联。
4. **cargo 1.98.1 下根 workspace 的 `exclude = ["spikes/*"]` 不再豁免"从包目录内运行 cargo"**（报 `current package believes it's in a workspace when it's not`，deno-spike 同样复现）：spike crate 需自带空 `[workspace]` 表（boa-spike 一直如此）。根 exclude 保留作文档语义。
5. **teardown 竞态**：runtime 销毁时服务端先以 close_going_away 关 WS，helper 的收尾 `Debugger.disable` 撞 "Sending after closing is not allowed"——属预期竞态，收尾步骤按 best-effort 处理（不计入 errors）。
6. 同名脚本二次 `execute_script` 的顶层 `let` 会 SyntaxError（同全局词法环境重声明）——Q 的计算脚本须 IIFE 包裹。这也是生产侧多 tick 重跑玩家 `loop` 只调函数不重编译模块的原因之一。

## 7. 结论与对架构的建议

1. **可行性定案**：deno_core（V8）+ deno_inspector_server 的断点调试链路**完整可用且全程机器可验证**。玩家调试体验（Chrome DevTools / VS Code 经 CDP attach）在技术上无障碍——xxscreeps 当前版本没接通的 inspector，在我们的栈上是现成的。
2. **M2-2A（沙箱正式接入）追加三项设计约束**：① `SandboxConfig` 加 debug 档（`RuntimeOptions.inspector: true`）；② 玩家模块脚本名绝对路径化（如 `/main.js`，否则 DevTools 断点无法按 URL 绑定）；③ debug 档下该玩家当 tick 不 arm 看门狗（实验 N 实证 terminate 无法与暂停共存，且展开时机不可控）。
3. **M2.5（dev-kit）**：`sim --debug` 起 InspectorServer（随机端口）+ 每玩家 register 一个 target + watch 循环里泵送事件循环（`wait_for_inspector: false`）。多玩家/多 bot 场景每 isolate 独立 target（实验 Q），暂停一个 bot 不影响世界推进。
4. **版本纪律**：deno_inspector_server 与 deno_core 锁同批版本升级（0.33 ↔ 0.412 实测对齐；ADR-002 的"跟随 deno_core 发版"纪律自然覆盖）。
5. **安全边界**：inspector 不改变遮蔽后全局面（实验 O），但 debug 档仍应只在 sim/本地模式开放（默认关闭，兼容红线"附加能力默认关闭"）；公网服开放断点调试的鉴权与资源占用问题留 M5 安全加固再议。

## 8. 最终汇总行（cargo run --release 原样输出）

```
RESULT inspector-spike: M=PASS N=PASS O=PASS P=PASS Q=PASS
CAPABILITY deno_core inspector + deno_inspector_server: CDP断点全链路机器可判定(M) 暂停期看门狗必掐/豁免=disarm(N) 全局面无泄漏(O) 快照可调试(P) 每isolate独立target+LIFO(Q)
NOTE chrome-devtools-manual=人工附加验证见笔记（复跑不依赖）
exit 0
```

多轮完整复跑均 5/5 PASS、退出码 0；每轮约 12s（含 N 的 1.5s+2s 暂停持有与 Q 的 3s 暂停；独立复验两轮实测 12.06s / 12.09s），另做过负路径验证：篡改 M 的断点行期望为 Some(7) 后 M 确定性 FAIL、退出码 1，还原即恢复。

## 9. 未尽事项

- **Chrome DevTools / VS Code 真实 UI 附加未执行**：协议层已由程序化 CDP 客户端逐消息验证（与 DevTools 走同一协议同一端点），人工步骤为 `chrome://inspect` → Configure → 添加 `localhost:<port>` → inspect，或直接打开 `/json/list` 返回的 `devtoolsFrontendUrl`。标记为待一次性人工验证，不阻塞结论。
- VS Code js-debug 的 attach-by-ws-URL 形态未测。
- >2 个 isolate 的规模行为未测（Q 验证到 2）。
- source map 在 DevTools 中的显示未测（本 spike 未提交带 sourcemap 的转译产物）——归 M2.5 转译管线。
- 实验 N(a) 的"终止在暂停唤醒后才展开"为【推断】（由 elapsed 推出），未做"暂停期间发无关 CDP 消息"的隔离对照——归 M2-2A 实现期验证。
- debug 档下 CPU 计量口径（线程 CPU 时钟在暂停期间是否计入）未量化——M2-2A 实现期事项。
- inspector 会话自身的性能开销（非暂停期）未量化。
