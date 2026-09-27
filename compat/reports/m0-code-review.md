# M0 代码评审报告（归档）

> 2026-09-27。范围：M0 全部已交付代码（kk-runner / kk-engine / kk-cli / compat / scripts / CI），
> 六个视角（并发与生命周期、沙箱安全与遮蔽、Rust 逻辑正确性、JS 运行时语义、TS/脚本/CI 工程、
> 测试充分性）。共 60 条评审记录：38 项 fix-m0 修复、16 项留待后续（M1×9 / M2×7）、
> 5 项重复否决、1 项不成立否决。每条发现的原始 where 为发现时行号；修复清单与留待清单的
> 行号已按当前工作区重新核对定位。

## 结论

**38 项 fix-m0 修复全部落实且经逐项代码核对成立（含 2 项 high 沙箱逃逸与 1 项 high 测试假绿的修复），本轮无新增改动；16 项已按里程碑登记留待后续，5+1 项复核否决无遗漏修复。**

## 修复清单（38 项，全部已应用；本轮逐项核对确认）

行号为当前工作区行号（git diff HEAD 可见全部改动）。

### 并发与生命周期（3 项）

- **并发-1**（medium）`crates/kk-runner/src/watchdog.rs:69-83`
  看门狗"代际检查"与 terminate 投递不原子，迟到的 terminate 可越过 disarm/arm 交界误杀下一个 tick 并被错误计费。
  修复：看门狗持 state 锁跨过 terminate 投递（terminate_execution 为非阻塞异步标志设置，持锁安全）；arm/disarm 的代际 fetch_add 移入 state 锁内（watchdog.rs:103/:115）——disarm 完成即意味着旧代际 terminate 已投递完毕，begin_tick 的 `cancel_terminate_execution()`（sandbox.rs:196）真正兜底。配套真实线程高频 arm/disarm 逼近窗口测试（sandbox_tests.rs:631-647，即测试-10）。
- **并发-2**（low）`crates/kk-runner/src/sandbox.rs:204-207`
  begin_tick 的 beginTick 世界注入脚本期间被硬中断时失败路径无善后，沙箱遗留"中毒"状态。
  修复：注入失败走统一的 `after_termination()` 善后（sandbox.rs:207）；set_modules（:166-167）/ set_raw_memory（:177-178）/ drain_missing_api（:293-294）/ drain_stub_calls（:298-299）另加 `clear_terminate_flag()` 轻量善后。
- **并发-3**（medium）`crates/kk-runner/src/prelude.js:394-407` + `crates/kk-runner/src/sandbox.rs:232-238`
  endTick 的 Memory 写回对 JSON.stringify 无保护，玩家一行代码（`Memory.big = 1n` / 循环引用）可让整条 tick 命令退化为基础设施 Err（报告与 console 丢失、bucket 漏记账）。
  修复：双层防护——prelude 侧 writeBackMemory 补 try/catch，序列化失败打 console.error 并跳过写回（保留上一 tick 存档，prelude.js:397-404）；宿主侧 end_tick 先 settle/清 tick_start 再返回脚本错误（sandbox.rs:232-238），结算与清理不因脚本错误跳过。

### 沙箱安全与遮蔽（8 项）

- **安全-1**（high）`crates/kk-runner/src/prelude.js:1042-1049`
  deno_core 0.412 的 `globalThis.__bootstrap` 命名空间（挂与 Deno.core 同一个 core 对象及 internals，104 个内部 op 含可一行 abort 宿主的 op_panic）未被遮蔽，完整沙箱破坏。
  修复：`delete globalThis.__bootstrap`（prelude.js:1049，op 引用已闭包捕获、不影响宿主通道）；配套测试断言 `typeof __bootstrap === 'undefined'` 并对全局面做 `getOwnPropertyNames` 白名单比对（sandbox_tests.rs:85-100 + :165-299 白名单函数），deno_core 升级新增残留全局会显形。
- **安全-2**（high）`crates/kk-runner/src/sandbox.rs:133-157`
  near_heap_limit 回调返回 `current_limit * 2` 且从不恢复，V8 持久改写堆上限——玩家循环"触发 OOM→再触发"可把 256MB 硬上限指数级抬升直至耗尽物理内存。
  修复：`restore_heap_limit`（remove+re-add 恢复堆上限并重新武装回调，sandbox.rs:151-157），在硬中断善后 after_termination（:259）调用；回调触发即置 near_heap_fired 诊断标志（:142）。
- **安全-3**（high）`crates/kk-runner/src/ops.rs:17-78`
  op_kk_console 无每 tick 行数/字节上限，玩家可在极短 tick 内向宿主注入远超其 JS 堆上限的内存（实测 300ms 注入 1.24GB），多玩家共进程时可 OOM 宿主。
  修复：每 tick 1000 行 / 1MB 字节闸门（CONSOLE_MAX_LINES/CONSOLE_MAX_BYTES，ops.rs:17-20），超限丢行并在首次超限时出告知行（:49-71）；begin_tick 重置闸门（sandbox.rs:201 调 reset_console_gate，ops.rs:73-78）。
- **安全-4**（medium）`crates/kk-runner/src/prelude.js:121-151`
  Buffer 垫片 utf8Decode 无 continuation byte 校验，非法 UTF-8 解码抛 RangeError（官方输出 U+FFFD）。
  修复：完整校验（overlong 拒绝、continuation 检查、码点上界与代理区检查），非法/截断序列产出 U+FFFD 替换字符。
- **安全-5**（medium）`crates/kk-runner/src/prelude.js:162-196`
  Buffer.from(input, encoding) 完全忽略 encoding，'base64'/'hex' 解码请求被按 UTF-8 处理，玩家静默拿错数据。
  修复：按 encoding 分派（utf8/hex/base64 实现解码，奇数长度与非法字符抛 TypeError），未支持的编码显式抛错不静默（prelude.js:192-195）。
- **安全-6**（medium）`crates/kk-runner/src/prelude.js:1033-1038`
  `__kk` 对象方法全部可覆盖：验收仪表可被归零造假、宿主 tick 通道可被换成抛错函数。
  修复：`Object.freeze(globalThis.__kk)` + 属性不可枚举/不可重写（prelude.js:1036-1038）；已知残余（冻结防不住玩家直接调用 `__kk.drainMissing()` 抢先清空仪表，根治需宿主侧闭包/独立 op）已登记 compatibility.md:34 灰区表，归 M2。
- **安全-7**（low）`crates/kk-runner/src/prelude.js:214-234`
  Buffer.alloc(size, fill) 的字符串 fill 被 TypedArray.fill 语义静默归零（官方按内容字节填充）。
  修复：字符串 fill 按其（按 encoding 编码的）字节循环填充（`alloc(8,'x')` → 8 个 120），Uint8Array fill 同款循环。
- **安全-8**（low）`crates/kk-runner/src/prelude.js:66`
  inspectValue 对 DataView 崩溃（ArrayBuffer.isView 分支调用 DataView 没有的 slice）。
  修复：DataView 单独分支输出 `DataView(byteLength)` 简单描述（prelude.js:66）。

### Rust 逻辑正确性（4 项）

- **正确性-1**（medium）`crates/kk-runner/src/budget.rs:54-63, 79-83`
  硬中断结算无条件触发下一 tick 跳帧惩罚且桶永不扣穿——官方只在桶被扣成负数（cpuAvailable<0）时才停一 tick。
  修复：settle_terminated 不再置 skip_next、只按整份 tickLimit 扣费且桶可为负（budget.rs:81-83）；负桶停跑由 begin_tick 的负桶判定处理（budget.rs:57-58，停跑 tick 照常回充）；sandbox.md:34 与 compatibility.md:12 的 CPU 经济条目已同步官方 cpuAvailable 语义；单测锁定（budget.rs:110-145）。
- **正确性-2**（medium）`crates/kk-engine/src/embed.rs:76-92, 233-238`
  createShard 静默丢弃 PlayerSpec 对象形式的玩家；runPlayers 对未注册 handle 静默跳过、结果映射缺项无报错。
  修复：兼容 PlayerSpec 对象（取 name 注册 handle，gcl/power 留 M1 规则实装、有注释说明，embed.rs:81-87）；runPlayers 未知 handle 显式 bail（:234-238）。
- **正确性-9**（low）`crates/kk-cli/src/bot_check.rs`
  死代码：tick_had_error 算出后 `let _ =` 丢弃。
  修复：删除，统计统一走 `any(level=="error")` 口径。
- **正确性-10**（low）`crates/kk-runner/src/proto.rs:268-280` + `crates/kk-cli/src/bot_check.rs:54-61`
  跳帧惩罚 tick 被报告为 Terminated，一次真实硬中断会被记成 2 个"硬中断"tick，且跳帧 tick 不排干仪表。
  修复：跳帧分支排干 console/missing_api/stub_calls 三仪表且 TickReport.plan 携带 SkipPenalty（保留 SkipPenalty→Terminated 的 outcome 映射契约——runPlayer 的 executed=false 依赖它）；bot_check 按 plan 三分类统计（completed / skipped_penalty / terminated）。

### JS 运行时语义（8 项）

- **JS语义-1**（high）`crates/kk-runner/src/prelude.js:434-441`
  RoomPosition 数值坐标形态（x, y 双参）的 getRangeTo 等因 roomName 比较恒返回 Infinity（官方视为同房间坐标正常计算）。
  修复：对齐官方 rooms.js:1588——仅目标带 roomName 且不等时才返回 Infinity，数值形态视为同房间（prelude.js:441）。
- **JS语义-2**（medium）`crates/kk-runner/src/prelude.js:431-436`
  isInRangeTo 三参形态把 range 绑定到 y 上（r=y），range 参数被忽略。
  修复：按原始第一参判断形态（`typeof x === 'object' ? y : range`），三参取第三参、两参取第二参（对齐官方 rooms.js:1362-1376）。
- **JS语义-3**（medium）`crates/kk-runner/src/prelude.js:339-344`
  CJS 模块执行失败后不删缓存，bot 一次瞬时初始化错误即永久复用半成品 exports（官方删缓存重抛、下一 tick 自愈）。
  修复：执行失败 `delete moduleCache[name]` 再重抛（Node 原生与官方 game.js:578-581 同款）。
- **JS语义-4**（medium）`crates/kk-runner/src/prelude.js:985-991`
  runExpr 用直接 eval 在 prelude 闭包内求值，玩家可访问捕获的 Deno.core.ops 引用（超常特权，违反 screeps-ok 契约）。
  修复：间接 eval `(0, eval)(code)` 在全局作用域求值（prelude.js:991），切断对闭包的访问。
- **JS语义-5**（medium）`crates/kk-runner/src/prelude.js:803-810`
  Game.structures 形态与官方双向偏差，且代码注释以"官方语义"之名记录了相反行为、if/else 两分支为死代码。
  修复：注释改正（"M0 桩收录全部结构；官方只收自家 owned 结构……M2 对齐"）+ 死代码清理；差异登记 compatibility.md:33 灰区表。
- **JS语义-9**（medium）`crates/kk-runner/src/prelude.js:586-590`
  Spawn=StructureSpawn、Energy=Resource 两个官方别名未实现（独立 stub 类，instanceof 为 false），与验收报告"含别名"的主张不符。
  修复：别名指向同一引用（`globalThis.Spawn = globalThis.StructureSpawn` 等，prelude.js:589-590）。
- **JS语义-10**（low）`crates/kk-runner/src/prelude.js:275-278, 318`
  循环依赖注释错误声称"Node/官方同款"，实际官方为抛 "Circular reference to module"（假绿方向：kk 宽松/官方严格）。
  修复：注释改为"Node 语义；官方为抛错，见 compatibility.md 灰区表"；compatibility.md:32 已登记（行为翻转本身留 M2 拍板）。
- **JS语义-11**（low）`crates/kk-runner/src/prelude.js:35-43, 89, 272, 631-634`
  缺失 API 探测仪表结构性漏报：`in`/hasOwnProperty 不触发 Proxy get trap；console、RoomPosition 实例（pos）、Buffer 静态成员不在 probe 面。
  修复：probe 补 has trap（:36-43）；console（:89）/ Buffer 静态（:272）/ pos 实例（:631-634）套 probe。未照单补 console.dir/trace/assert 桩（以官方实测为准，避免臆造官方面）。

### TS / 脚本 / CI 工程（7 项）

- **工具链-1**（medium）`scripts/setup-screeps-ok.sh:22-23`
  `sed -i ''` 为 BSD 专用语法，GNU sed（Linux）上脚本以退出码 2 中断，文档化的复现路径在 Linux 不可用。
  修复：`sed -i.bak` + `rm -f *.bak`（跨 BSD/GNU，:22-23）。
- **工具链-2**（medium）`compat/src/rpc.ts:67-72`
  sidecar 被信号杀死（SIGKILL/OOM）后 exitCode 恒为 null，死子进程被永久复用，后续 RPC 全部 ERR_STREAM_DESTROYED 且永不重启。
  修复：error/exit 回调置 `this.child = null`（:67/:69-72），下次 ensure() 重建子进程。
- **工具链-3**（low）`compat/src/rpc.ts:145-158`
  注册 SIGTERM/SIGINT 监听抑制 Node 默认终止行为，cleanup 只 kill 子进程不重发信号，宿主可能挂住不退。
  修复：cleanup 后 `process.kill(process.pid, sig)` 重发信号（:152-157）。
- **工具链-5**（medium）`scripts/fetch-bots.sh:15-21`
  tooangel 的 fetch+checkout HEAD 组合永不更新、夹具未 pin，跨机器/跨时间验收不可对账。
  修复：`TOOANGEL_PIN="87645a0…"` 固定 commit，fetch 后 checkout pin（与 SCREEPS_OK_PIN 同款机制，:15-21）。
- **工具链-6**（medium）`scripts/fetch-bots.sh:26-35`
  curl 无 --fail 无校验：release 资产 404 时 9 字节 "Not Found" 被写进 main.js 并被缓存条件固化。
  修复：`curl -fsSL` 下载到 .tmp + 最小长度校验（<1000 字节删半成品并 exit 1）+ 校验通过才 mv（:29-35）。
- **工具链-7**（low）`compat/src/parity.json`
  expected_failures/skips 用数组形态，与 screeps-ok loadParity 的映射 schema 不符（当前靠空值侥幸通过）。
  修复：改为 `{"expected_failures": {}, "skips": {}}`；compat/README.md 补登记规范与踩坑形态说明；dist/ 由 build 同步。
- **工具链-8**（low）`compat/src/capabilities.ts`
  多声明上游不存在的 structures/construction 两个 capability（对齐主张 29 项与 pin 契约 27 项不符）。
  修复：对齐 27 项 + 注释说明（capabilities.ts:3-5）；compat/README.md:12 修正为"27 项"。

### 测试充分性（8 项）

- **测试-1**（high）`crates/kk-runner/src/sandbox.rs:142, 273-276` + `sandbox_tests.rs:652-701`
  内存闸门测试假绿：断言的 Terminated 在看门狗兜底时同样产生，诊断标志 near_heap_fired 从未被写/读——最危险的回归（堆闸门失效=整进程崩溃）对测试完全失明。
  修复：near-heap 回调置 near_heap_fired（sandbox.rs:142）+ 公开访问器（:273-276）；测试断言 fired 为 true 且 elapsed 远小于 tickLimit 兜底线（区分内存闸门与看门狗路径）。
- **测试-2**（medium）`.github/workflows/ci.yml:31-38`
  适配器端到端 smoke 测试在两条 CI 线上都不执行（js job 无二进制结构性 skip，rust job 不跑 node）。
  修复：rust job 增加 `cargo build -p kk-cli` + 构建 dist + `node --test compat/test/smoke.test.mjs`（env KKSCREEPS_BIN 指向构建产物，ci.yml:31-38）。
- **测试-3**（medium）`crates/kk-runner/tests/fixtures/sample-bot/utils.js:8-10` + `sandbox_tests.rs`（cjs_and_loop 段）
  CJS 循环依赖"半成品 exports"语义无断言锁定（原断言与循环无关、任何实现下恒真）。
  修复：夹具加 lateProbe（循环点持有的半成品对象引用，验证后续赋值可见——能区分"半成品引用"与"循环点返回新空对象"的静默破坏形态）；测试双重断言（roleSeenDuringCycle 为 undefined + lateProbe.role 后续可见）。
- **测试-5**（medium）`crates/kk-cli/tests/embed_rpc_tests.rs`（新增）+ `crates/kk-engine/tests/embed_tests.rs:54-66`
  kk-cli 整 crate 零测试；embed RPC 传输层与"二次 create_shard 完全重置"（screeps-ok 每测试都走的路径）无覆盖。
  修复：新增 embed_rpc_tests.rs（RPC 往返 / rpc_id 回退（非法请求也带回原 id 的错误响应）/ EOF 退出）；embed_tests.rs 补二次 create_shard 后 run_player 正常服务的断言。
- **测试-6**（medium）`crates/kk-runner/tests/sandbox_tests.rs:374-383`
  Memory 跨 tick 写回-重解析生命周期（bot 状态生命线）在入库 CI 测试零断言。
  修复：断言 Memory.crossTick 与 Game.creeps['w1'].memory.job 跨 tick 一致（endTick 写回 → 下一 tick 懒重解析）。
- **测试-7**（low）`crates/kk-runner/tests/proto_tests.rs:131-135`
  crash_isolation 断言把"跳帧惩罚"折叠成"硬中断"，无法区分"每轮真死循环被掐"与"永久跳帧"的回归。
  修复：补 plan 断言——硬中断 tick 的 plan 必须是 Run（官方：桶充足不跳帧），与 SkipPenalty 区分。
- **测试-9**（low）`crates/kk-runner/tests/sandbox_tests.rs:704-730` + `m0-bot-missing-api.md:46`
  .wasm 二进制模块链路（require 返回 Buffer → WebAssembly 编译）repo 内 CI 零覆盖，且报告"已有自动化覆盖"的主张不实。
  修复：sandbox 测试内联合生成合成 .wasm 夹具（8 字节 \0asm 魔数头）走完整 require→Buffer→WebAssembly.Module 链路并断言内容一致；m0-bot-missing-api.md:46 登记项 3 的主张已修正（gitignored 的 wasm-bot 夹具仍为手动路径，如实注明）。
- **测试-10**（low）`crates/kk-runner/tests/sandbox_tests.rs:631-647`
  看门狗单测不驱动真实线程，"迟到掐断不误杀下一 tick"核心主张无直接测试场景。
  修复：真实线程高频 arm/disarm 逼近 deadline 交界窗口的测试（300 轮 arm 1ms→sleep 800µs→disarm→下一 tick 执行必须成功；与并发-1 的锁原子化修复配套）。

## 留待后续（16 项，逐条登记——后续里程碑的工作清单输入）

行号为当前工作区行号。M1 项 9 条、M2 项 7 条。

| id | 位置 | 问题 | 里程碑 | 理由 |
|---|---|---|---|---|
| 并发-4 | crates/kk-runner/src/proto.rs:115-117 | RunExpr 通道在未武装看门狗的情况下执行任意玩家代码：死循环永久挂死该玩家线程，Shutdown 与 Drop 的 join 都无法中断，进程无法收尾 | M1 | M0 调用方仅 screeps-ok 适配器探针与自家测试、无不可信输入；near_heap 回调可掐断持续分配型死循环，纯 CPU 死循环窗口极小。M1 引擎接入后 runExpr 成为常驻通道，需按 run_player 同款 begin→arm→end 编排 |
| 正确性-5 | crates/kk-engine/src/embed.rs:120-128 | createShard 不拒绝 `controller:false` 与 rcl/owner/safeMode 的互斥组合，静默吞掉控制器设置（契约要求抛错且不改变状态） | M1 | 属适配器层校验缺口而非游戏规则缺失（TS 侧一行等价校验即可）；M0 明确不要求用例通过，该用例已在失败池如实登记；M1 棘轮建基线时随契约区转绿 |
| 正确性-6 | crates/kk-runner/src/prelude.js:817 | 无 controller 的房间 `room.controller` 投影为 null，契约要求 undefined | M1 | 该用例已在基线失败池如实登记；M0 不要求用例通过，M1 棘轮驱动转绿 |
| 正确性-7 | crates/kk-engine/src/embed.rs:108-118 | createShard 对显式传入的 terrain 不强制四角墙（官方地图生成器不变量，vanilla 适配器对所有 terrain 强制） | M1 | M0 terrain 只存不用（无读取路径），无玩家可见影响；M1 接入地形读取时必须对齐（否则正规地图行为不一致） |
| 正确性-8 | crates/kk-engine/src/embed.rs:111 等 | terrain 数组无 2500 长度校验、非整数元素被 filter_map 静默丢弃，畸形地形截短后照样入库 | M1 | 同上，M0 无读取路径；M1 按 `terrain[y*50+x]` 读取时短数组将越界 panic，输入校验须前置 |
| JS语义-17 | crates/kk-runner/src/prelude.js:770 | structureType→类映射缺 'constructedWall' 键（只登记 'wall'）：官方 STRUCTURE_WALL='constructedWall' 的墙对象落到 Structure 兜底，instanceof StructureWall 为 false | M1 | M0 交付主张无涉（基线无墙用例、bot-check 内置世界无墙）；影响始于 M1+ 墙行为实装，补一个键即可 |
| 工具链-4 | compat/src/rpc.ts:110-120 | JSON-RPC 调用无超时：sidecar 主线程死锁时 pending 永不结算，上游 fixture 的 close() await 悬挂测试进程（外部 SIGTERM 可恢复） | M1 | M0 kk-runner 看门狗+内存双闸门已兜底最可能的挂死源（玩家 JS 死循环返回 terminated），剩余风险仅 Rust 侧死锁；M1 棘轮进 CI 后挂死即 CI 卡死，届时加超时 |
| 工具链-9 | compat/src/adapter.ts:118 | tick 忽略第二参 options（含上游 fixture 围栏无条件注入的 AbortSignal），违背 pin 契约"每 tick 前检查"——真引擎接入后超时测试的 tick 序列无法中止 | M1 | M0 tick 为 O(1) 计数无 between-ticks 窗口、无可观察影响；M1 引擎有真实 per-tick 循环时必须支持，否则超时用例全部挂死 |
| 测试-4 | crates/kk-cli/src/bot_check.rs:27-120 | bot-check 退出码不做验收判定（跑出 500/2000 仍 exit 0），关卡靠人眼读报告，与"机器可判定"总则不符 | M1 | M0 关卡口径含"或登记"的人工裁量（overmind 每 tick console 错误即登记项），硬编码阈值反而误判；m0-bot-missing-api.md:53 已排期"进 CI 的周期任务，M1 起每里程碑跑一次"，退出码阈值化是该工作的前置 |
| 安全-9 | crates/kk-runner/src/prelude.js:406 | writeBackMemory 序列化结果超 2MB 时静默丢弃整个 Memory 写回（无信号）；xxscreeps 为报错信号（vanilla 的 tick 末写回本身无检查） | M2 | 边缘场景（>2MB）、无崩溃无损坏、数据结局与 xxscreeps 一致；缺的是信号通道，应与 M2 Memory 正式实现（M2-2A）统一设计 |
| 正确性-3 | crates/kk-runner/src/prelude.js:646,662,678,715 + crates/kk-world/src/dumb.rs:152,158 | 多玩家分片下玩家视角缺失：所有对象 my 硬编码 true、Game.creeps/spawns/structures 装入全部玩家对象、FIND_MY_* 不按 owner 过滤 | M2 | 属玩家可见游戏行为（M0 明确不引入）；prelude 桩 M2 起被真正玩家 API 层取代，owner 语义随 M2 玩家 API 对照移植一并实现 |
| JS语义-7 | crates/kk-runner/src/prelude.js:384-388 | Memory 解析失败时静默重置为 {} 并在 tick 末写回，覆盖销毁玩家原始（损坏的）RawMemory；官方解析失败 Memory 为 null、不写回、原数据保留 | M2 | Memory 游戏语义属 M2 玩家 API 层（M2-2A）；M0 无从沙箱读回 rawMemory 的通道、无持久层后果；基线 25-memory 无解析失败用例，M2 重写时须按差异清单对齐 |
| JS语义-12 | crates/kk-runner/src/prelude.js:616-618 | store 垫片两处数值不符：getUsedCapacity() 无参只返回 energy 单项（官方返回全部资源和）；访问合法资源名误报进缺失清单并返回 undefined（官方返回 0） | M2 | store 精确语义属 M2 玩家 API 层对照移植（约 340 方法）；M0 哑世界 store 只有 energy、两 bot 未访问非 energy 资源，仪表未失真 |
| JS语义-15 | crates/kk-runner/src/prelude.js:351-352 | require.cache 的值形态与官方不同（kk 存 {exports} 包装，官方存 exports 本体），require.cache.main.loop 这类官方可用形态拿到 undefined | M2 | 少见玩家模式（三个 M0 bot 零使用）；上游现有断言不区分形态；M2 关卡（screeps-ok ≥95% + 缺口登记）会真正检验 |
| JS语义-16 | crates/kk-runner/src/prelude.js:726 | flag 的 memory 在 wrapObject 时立即求值（官方懒 getter）：世界有 flag 时 Memory 在 bot 代码前被强制解析，坏 RawMemory 的错误流混入 bot 前 | M2 | creep.memory/room.memory 均已是懒 getter，flag 是唯一例外；M0 桩环境 rawMemory 初始为空不触发；随 M2 玩家 API 层重写对齐 |
| 测试-8 | crates/kk-world/src/dumb.rs:137 + crates/kk-cli/src/bot_check.rs:143 + crates/kk-runner/src/prelude.js:879-884 | 玩家可见的 Game.cpu.bucket/tickLimit 与真实预算桶脱钩（两个真相源）：注入侧硬编码常量，budget.rs 真实结算从不回写，无一致性断言 | M2 | M0 哑环境 gcl/gpl/market 同为常量桩、验收不依赖 cpu 读数真值；真实回写需 M2 沙箱正式接入后的注入通道重设计 |

另有一项**已修复但留有登记残余**：安全-6 的深层防御（玩家直接调用 `__kk.drainMissing()` 抢先清空仪表，freeze 防不住）已登记 compatibility.md:34 灰区表，根治（宿主侧闭包/独立 op）随 M2 玩家 API 层重写。

## 复核否决（6 项）

| id | 否决理由 |
|---|---|
| 安全-10 | 与并发-1 重复（同一竞态的独立报告）。已随并发-1 的锁原子化修复一并闭合 |
| 正确性-4 | 与安全-2 重复（同一 near-heap 上限翻倍问题的正确性视角）。已随安全-2 修复 |
| JS语义-6 | 与安全-9 重复（Memory 超 2MB 静默丢弃）。同样留待 M2 |
| JS语义-8 | 与并发-3 重复（endTick 写回无保护）。已随并发-3 修复 |
| JS语义-14 | 与安全-5 重复（Buffer.from 忽略 encoding）。已随安全-5 修复 |
| JS语义-13 | 不成立（rejected）：buildGame 的 O(n²) 回填在 M0/M1 实际规模（个位数~低两位数对象）为微秒级；M1 唯一持续跑此 prelude 的场景是 screeps-ok 哑世界棘轮，评审预测的"M1 数百对象热点"与路线图矛盾——该代码按设计在到达该规模前（M2 玩家 API 层）被整体替换 |

## 验证记录

分三部分：修复轮验证（修复工程师执行）、验收冒烟（脚本素材）、本轮文档核对（本报告作者执行）。

**修复轮验证**（素材引用，非本报告作者执行）：

- `cargo fmt --all --check` 通过（exit 0，无需改动格式）；
- `cargo test -p kk-runner -p kk-engine -p kk-cli` 全绿（kk-runner 单测 6、m0_sandbox_suite 1、m0_thread_model_suite 1、embed_engine_roundtrip 1、embed_rpc_roundtrip 1，0 失败）；
- `cargo clippy -p kk-runner -p kk-engine -p kk-cli --all-targets` 无警告无错误；
- compat 侧 `pnpm run check`（tsc --noEmit）通过、`pnpm run build` 成功（dist/parity.json 已同步新形态）、`node --test test/smoke.test.mjs` 1 pass / 0 skip；
- 未跑完整 workspace 测试套件与 docs 链接检查（按分工留给脚本统一跑）。

**验收冒烟与基线**（素材引用）：

- `pnpm run check`（全套本地 CI：cargo fmt/clippy/test + JS 工作区测试 + 文档链接检查）在第 1 轮修复后通过；
- wasm-bot 10 tick 与 tooangel 300 tick 冒烟均通过（宿主稳定、无硬中断）；
- **基线重跑未完成确认**：全量套件重跑 exit=1（存在失败用例时 vitest 的退出码），但报告文件已产出，本报告作者解析 `compat/reports/kkscreeps.json` 得 passed=229 / failed=1659 / skipped=1026（numTotalTests=2914），与入库基线数字一致。README/roadmap/基线报告沿用的 229/1659/1026 为首份基线数字，本轮修复未重新跑出新的登记基线值——数字一致性以"重跑报告与旧数字相同"为旁证，非正式重跑确认。

**本轮文档核对**（本报告作者执行，2026-09-27）：

- 逐项核对 38 项修复的代码证据（Read/sed 对照 watchdog.rs、sandbox.rs、prelude.js 全部关键段、ops.rs、budget.rs、embed.rs、proto.rs、bot_check.rs、rpc.ts、fetch-bots.sh、setup-screeps-ok.sh、parity.json、capabilities.ts、ci.yml、sandbox_tests.rs、proto_tests.rs、embed_tests.rs、embed_rpc_tests.rs、utils.js 夹具），全部成立；
- `python3` 解析 `compat/reports/kkscreeps.json`：passed 229 / failed 1659 / skipped 1026（与 README.md:7、roadmap.md:22、基线报告一致）；
- `grep -c ': boolean;' compat/src/capabilities.ts` = 27（与 compat/README.md:12"27 项"一致）；
- 核对 `crates/kk-cli/src/embed_main.rs` 方法分派共 12 个具名方法（createShard/placeObject/setTerrain/runPlayer/runPlayers/tick/getObject/findInRoom/getGameTime/captureConsoleLogs/getControllerPos/teardown），与 ADR-010:33-34 方法集一致；
- `git diff HEAD --stat` 复核全部改动面（24 文件 +964/-167，另有新增 crates/kk-cli/tests/）；
- 文档核对发现并已修的不一致：README.md:28"九条 ADR"→"十条"（docs/decisions/ 实有 ADR-001..010）；docs/architecture/sandbox.md 模块系统段"留待真实机器人代码验收（M0）"已过时→改为 M0 已验收+灰区登记现状；安全边界段"遮蔽复查列入实现期验证项"已兑现→改为 __bootstrap 遮蔽+白名单比对测试已实装。
- 未验证项：完整 workspace 测试套件、docs 链接检查、screeps-ok 全量基线的正式重跑（按任务分工留给脚本统一执行）。

---

## 追记：修复后复审（2026-09-27，第三轮，直接子代理）

三个独立评审员对 commit 3b35de2 的 38 项修复本体做了聚焦复审（官方源码逐行对照、
deno_core/v8 registry 源码核查、CI 干净环境推演、JS 边界实测）。结论：修复方向全部
正确、无行为回归；但发现 2 条 medium + 若干 low 的"修复不完整/顺带引入"，当场修复：

| 问题 | 处置 |
|---|---|
| 负桶停跑的判定顺序与官方相反（官方回充**前**判定；kk 在 (-limit,0) 区间会跑缩水 tick 甚至 tickLimit=0 的 1ms 秒杀 tick） | **已修**：budget 重写为官方形态——回充并入结算（`bucket += limit − used`，封顶、可为负）、begin_tick 回充前判定、停跑分支带官方 −2×limit 地板；同步单测（含新增深负地板用例） |
| end_tick 缺 cancel：迟到的看门狗 terminate 可打死 Memory 写回、中断整轮验收 | **已修**：disarm 后 cancel + 失败善后恢复堆上限 |
| near-heap 打在 endTick 的 Memory stringify 上时抬高的上限不恢复 | **已修**：end_tick 失败路径接 restore_heap_limit |
| 堆上限恢复无防回归断言 | **已修**：暴露 heap_size_limit_bytes，memory_gate 测试断言恢复到 48MB 附近 |
| 回充并入结算后 after_termination 与 end_tick 双计费 | **已修**：tick_settled 标志二选一结算 |
| utf8Decode：overlong 3/4 字节漏检、截断的 FFFD 计数是 Node 两倍 | **已修**：按官方第二字节受限范围（E0≥A0/ED≤9F/F0≥90/F4≤8F）+ maximal-subpart 重写；全部边界经 embed RPC 对照 Node 实测一致 |
| utf8Encode 孤代理产出 CESU-8 | **已修**：输出 U+FFFD |
| Buffer.from hex/base64 非法输入 throw（Node 宽松：截断/停止/跳过） | **已修**：对齐 Node 宽松行为 |
| Buffer.from(object) 非 array-like 形态与 Node 相反 | **已修**：有 length 按 array-like，否则报错 |
| RoomPosition 系列不解包 `.pos`、分发条件与官方相反 | **已修**：fetchXY 按第二参分发 + .pos 解包（官方 fetchXYArguments 语义） |
| runExpr 错误分类用 new Function 预探（`return 1` 误判 runtime） | **已修**：以 eval 抛出的 `instanceof SyntaxError` 为准 |
| 白名单全大写兜底可放行 `__FOO` 形内部全局 | **已修**：排除 `__` 前缀 |
| embed PlayerSpec 已支持但 adapter.ts 类型未同步 | **已修**：联合类型 |
| fetch-bots.sh wasm 夹具半成品不幂等；setup-screeps-ok.sh 换 pin 被脏树卡住；CI 无 job 超时 | **已修**：分别处理 |

复审确认无问题（要点）：看门狗锁原子化无死锁/丢唤醒（v8 terminate 仅短暂持内部锁）；
budget 桶充足不跳帧与官方 make.js 对照成立；near-heap remove+re-add 的 API 用法正确
（deno_core jsruntime 源码核查，isolate 亲和线程调用）；console 闸门不误杀正常 bot
（tooangel 2000 tick 零截断）；间接 eval/`__bootstrap` 删除/类别名/CJS 失败删缓存/
embed 契约的实现与测试均成立；CI 干净环境推演通过。

**新登记留待后续**：
- console 格式化与官方不同（官方逐参 `toString()`，对象得 `[object Object]`）——影响
  screeps-ok 输出比对类用例，M1 对照 console.js 处理；
- 探测 has-trap 会把否定式能力探测（`!('x' in Game)`）计入缺失清单（仪表噪声方向，
  不影响语义），M1 仪表增强时处理。

---

## 追记：第四轮评审（2026-09-27，外部独立评审——ChatGPT 仓库实读）

外部评审员直接读取仓库并以 release 二进制做探针实证，报告 17 条发现。逐条复核
结论（对照 screeps-ok 契约文档、vanilla 参照实现、官方引擎源码）：

### 已修复（13 条）

| 发现 | 复核结论与处置 |
|---|---|
| [high] end_tick 写回阶段无看门狗（`Memory.toJSON` 可挂死进程，探针实证 3s+ 无响应） | 属实。看门狗保持武装直到写回完成（官方：序列化计入超时窗口与 CPU 计费），结算含写回耗时；`{terminated:true}` 回传并按硬中断善后。测试锁定（挂死→150ms 内被掐断） |
| [high] Memory 写回失败报成 runPlayer 成功 | 属实（adapter-spec:229 归 runtime，vanilla 参照同款）。endTick 结果回传，runPlayer 通道归 runtime；正常路径不受影响 |
| [high] placeStructure 摆放的 spawn 玩家不可见 | 属实（探针实证三处全落空）。placeObject 按 structureType 归一内部类别；测试断言 Game.spawns/FIND_MY_SPAWNS/instanceof |
| [high] Room.memory 只有 getter（tooangel 从 tick 2 起每 tick 抛 setter 异常而被验收漏计） | 属实（官方 rooms.js:551 双向；归档报告实证）。room/creep/flag 三处 memory 全部补 setter；bot-check 计数纳入 player_error；修复后 tooangel 推进到布局阶段 |
| [medium] runLoop 玩家异常被丢弃（Completed 无法证明无异常） | 属实。run_loop 暂存玩家错误，TickReport.player_error 暴露，bot-check 统计区分"执行到入口/玩家抛错" |
| [medium] 运行期抛 SyntaxError 被错分 syntax | **部分成立但不修**：vanilla 参照实现同样按异常类型名分类（errorType==='SyntaxError'）——我们与参照一致；spec 文本与参照的分歧登记上游。行为测试以套件为准 |
| [medium] 可 JSON 化的活对象（RoomPosition）当合法返回值 | 属实。逐条移植 vanilla 的 constructor 检测（function/symbol/非纯对象 → serialization） |
| [medium] 已执行的硬超时被标"未执行"（适配器抛基础设施错误） | 属实（契约：执行后中止归 runtime）。run_eval_tick 区分"负桶停跑未执行"（保持普通 Error）与"执行中被终止"（runtime）；测试锁定 |
| [medium] 结算缺官方逐 tick ceil | 属实（make.js:193 Math.ceil）。settle/settle_terminated 取整 + 亚毫秒单测 |
| [medium] isEqualTo 忽略房间名 | 属实（官方 rooms.js:1579）。修复 + 单测（跨房间同坐标 false） |
| [medium] getDirectionTo 角度算法与官方阈值不同 | 属实（官方 utils.js:64 阈值判定）。逐字移植 + 单测（(7,3)→RIGHT）；跨房间换算登记 M1 |
| [low] 重建测试 1500ms 墙钟阈值 | 属实。放宽为病理性上限 10s，性能基线归 M5 |
| （连带）PathFinder.search 返回 {dx,dy} 步进对象 | 修复 setter 后 bot 推进暴露：官方 path 元素是 RoomPosition 实例（path-finder.js），bot 生态在其上调用方法（含自打的原型补丁）。已改绝对坐标 RoomPosition 数组，tooangel 立即多跑出 8000 次真实寻路 |

### 登记留待后续（4 条）

- [M1] typed placement 精确初态与快照规范化（placeSource 缺省字段、boosts 泄入快照）——
  属快照语义，与 M1 引擎结算同批做；
- [M1] 全局 LIFO 约束仅告警未强制（多 EmbedEngine 实例场景）——需进程级所有者统一
  创建/回收顺序，与多实例架构（M2 正式接入）一起设计；evict_idle 只查栈顶的逐出
  策略同理（非栈顶逐出与 LIFO 冲突，需一起设计）；
- [M1] bot 验收仪表：重复 console 行折叠统计（bot 自捕获异常仅 log 级，逐条计数噪声大）；
  跨房间 getDirectionTo（官方走出口换算）；
- [M2] 旧对象引用失效语义已在 sandbox.md 修正设计（对象仍存活时缓存引用可继续读写，
  对齐 screeps-ok 27.12 官方基准）——M2 快照设计按此区分两种形态。

### 第四轮后的 bot 验收现状

tooangel 推进到**房间布局规划阶段**（`data.positions` 构建）后停在最深处：布局算法
依赖真实地形面（`Room.getTerrain()` / `lookFor(LOOK_TERRAIN)`），桩世界无地形——这是
M0/M1 的真实边界（地形编解码属 kk-core，M1 交付）。宿主稳定性、loop 每 tick 执行、
缺失 API 清单三项 M0 验收口径继续成立；bot 内部降级路径与每层缺口已逐层记录。

验证：`pnpm run check` 全套绿；新增 5 组测试（memory setter 双向、RoomPosition 官方
语义、写回看门狗、契约分类、spawn 归一）全过；wasm-bot 冒烟通过；tooangel 2000 tick
宿主稳定。

### 追记：同类问题专项审计（第五轮，问题驱动的自查）

针对"桩返回值形态/生命周期与官方不一致"这一类问题（PathFinder.search 返回
{dx,dy} 而非 RoomPosition 实例是首个被 bot 踩中的样本），对全部桩表面做了
返回值形态专项对照，确认并修复 3 处：

| 问题 | 官方证据 | 修复 |
|---|---|---|
| RoomVisual 绘制方法返回 undefined | 官方 rooms.js:1157 全部 `return this`（链式调用是文档承诺） | 桩返回 this，链式恢复 |
| `Memory = {...}` 整体赋值被静默吞掉 | 官方 game.js:492-495：首次访问后把自己重定义为普通可写值属性（每 tick 由运行时重装懒 getter） | ensureMemory 首访固化 + beginTick 重装，与官方生命周期一致 |
| getTerrainAt 恒返回 'p' | 官方 map.js:235-258：返回 'wall'/'swamp'/'plain' 完整词，越界或无地形数据 → undefined | 官方语义移植 + 世界 JSON 携带真实地形（bit 语义 TERRAIN_MASK_*，经 createShard/setTerrain 提供时生效） |

同一审计中确认为**桩保真度边界**（属 M1 地形/寻路实装范围，维持登记）：
room.getTerrain()/Terrain 对象与 lookFor(LOOK_TERRAIN)（tooangel 布局阶段的
真正依赖）、serializePath/deserializePath 的真实编解码、store.getCapacity 的
数值语义、findPath 的步进对象内容。测试：stub_value_shapes 新增 8 项断言全绿。

---

## 追记：第六轮评审（外部 GPT 复审 HEAD fb38acc）

7 条发现逐条复核，全部属实（含对我方第四/五轮修复的两个纠偏），全部修复：

| 发现 | 复核与处置 |
|---|---|
| [high] 硬中断后仍执行写回，玩家可经 Memory.toJSON 挂死线程（探针实证 3s+ 无响应） | 属实——第四轮修复只覆盖了"未中断"路径。按官方语义修复：硬超时打断的是整个 run（含序列化），被中断的 tick **跳过写回**（Memory 保持上一 tick）；以专门的 terminated_this_tick 标志判定（修复过程中发现初版误用"已结算"标志会破坏 tick 外写回，已纠正）。GPT 探针复现：即时返回 runtime、进程存活 |
| [high] Memory 固化描述符 writable:true 与官方不符 | 属实——官方 defineProperty 无 writable（默认 false，首访后只读）。我方第五轮想当然写成了可写。已对齐官方（整体赋值静默失败），测试含跨 tick 断言 |
| [high] 返回值检测自身抛错（constructor:null）被误判硬中断并按整份计费 | 属实。检测整体 try/catch 防护 + ctor 空值守卫；病态对象按规范字面归 serialization（constructor ∉ {Object, undefined}），不再触发硬中断结算 |
| [high] placeStructure 无名 spawn 产生 Game.spawns["undefined"] | 属实（上游 StructureSpec 无 name 字段）。按归一化后的记录 type 生成稳定名称（Spawn_N / creep_N）；测试改用无名契约输入 |
| [medium] PathFinder.search 的 path 含起点 | 属实（官方 pf.cc:574 回溯在 origin 停止；tooangel 甚至自行 splice 起点回头部——反证官方语义）。去掉起点，同点→空路径，测试锁定 |
| [medium] Memory.toJSON 返回非字符串被报"写回阶段硬中断"+整份计费 | 属实。writeBackMemory 整体防护：玩家侧写回错误一律 {ok:false}（含 toJSON 非字符串显式分支），绝不抛出 |
| [medium] embed_tests 两个 #[test] 并行违反 LIFO 纪律且告警不失败 | 属实（实测 12 次中 4 次告警仍全绿）。合并为串行套件 + 新增 lifo_violations() 计数并在两套件断言零违规 |

验证：全套 cargo test（21 组）与 pnpm run check 绿；GPT 挂死探针复现通过（即时返回、
进程存活）；tooangel 2000 tick 与 wasm-bot 复验稳定（零中断、缺失清单零）。

---

## 追记：第七轮评审（2026-09-27，全新会话系统性审计 + 修复）

方法与前六轮的区别：不再做增量复审，而是**逐文件通读全部 M0 代码**（prelude.js 1233 行 /
sandbox / watchdog / budget / proto / ops / modules / embed / embed_main / bot_check /
dumb / adapter / rpc / 全部测试），先列不变量清单再逐条验证；对 prelude↔sandbox↔proto↔
embed↔adapter 做五层契约对表；对官方源码做**正向通读**（game.js 全局装配区、runtime.js
run() 末尾、runtime-driver.js evalCode、utils.js 全量函数、rooms.js RoomPosition 区、
map.js getTerrainAt、make.js 桶经济）而非仅按台账已记录行号回查；Node 与本环境双实证。
至此七轮累计：108 条发现、87 项修复、26 项登记。

### 已修复（9 条：5 medium + 4 low，全部有官方源码或 Node 实证）

| # | 级别 | 位置 | 问题与证据 | 修复 |
|---|---|---|---|---|
| 七-1 | medium | prelude.js Buffer.from | array-like 负整数 length 经 `>>>0` 变 4294967295：一次调用分配 4GB TypedArray backing store（V8 堆外内存，near-heap 闸门无效；runExpr 通道无看门狗——并发-4 登记的放大器）。Node 实测 `Buffer.from({length:-1})` 得空 Buffer | 负 length 钳 0，保留非负截断；测试锁定（含 2.7→2 的小数截断） |
| 七-2 | medium | prelude.js Buffer 类 | 缺 `toJSON`：`JSON.stringify(buf)` 得 TypedArray 形态 `{"0":104,...}`；Node/官方（binary 模块链路就是 Node Buffer，runtime-driver.js:17）得 `{"type":"Buffer","data":[...]}` | 原型补 toJSON；测试锁定 |
| 七-3 | medium | prelude.js runLoop | "main 未导出 loop()" 被报成玩家 runtime 错误；官方 game.js:555-563 `if(isObject&&isFunction(loop)){…}` **无 else**，静默跳过——bot-check 的 errored_ticks 口径被失真 | else 分支返回 `{ok:true}`；新增 run_loop_without_loop_silent 测试（Completed + player_error None） |
| 七-4 | medium | prelude.js / ops.rs | require 失败消息 "Cannot find module"（Node 形态）；官方 game.js:587 为 `Unknown module '…'` | prelude 与 op 层消息一并改官方形态；测试锁定 |
| 七-5 | medium | prelude.js | 游戏对象 toString 家族全缺：`String(pos)` 得 '[object Object]'。官方 15 个类各有 prototype.toString（rooms.js:1358 `[room X pos x,y]`、creeps.js:122 `[creep name]`、structures.js:68/893、sources.js:41、minerals.js:42、resources.js:41、deposits.js:40、construction-sites.js:46、tombstones.js:236、ruins.js:68、nukes.js:40、power-creeps.js:102、rooms.js:536 Room、flags.js:53）——bot 生态日志拼接的事实标准 | RoomPosition 类内 + OFFICIAL_TOSTRING 映射挂各类原型（wrapObject 对象经原型链自动获得；未列出的 Structure 子类按官方同构继承 Structure 根格式）；6 项断言锁定 |
| 七-6 | medium | kk-world/dumb.rs matches_find | 缺 FIND_SOURCES_ACTIVE(104)/FIND_MY_STRUCTURES(108)：prelude room.find 有此二分支而 embed findInRoom 无——**同一 FIND 语义两层行为不一致**（玩家代码可见、适配器查询为空） | 补 104（哑世界无 active 语义，与 105 同款）与 108（与 107 同款全部结构）；embed 测试锁定 |
| 七-7 | low | prelude.js Memory getter | 初始懒 getter 安装缺 `enumerable:true`（官方 game.js:470-487 两处 defineProperty 均 true；我们首访固化后才 true） | 补齐描述符 |
| 七-8 | low | prelude.js CREEP_METHODS | `' recycleCreep'` 前导空格（trim 掩盖） | 源头清理（行为不变） |
| 七-9 | low | sandbox.rs begin_tick | world 序列化在 watchdog.arm 之后，失败路径（理论不可达）不 disarm——违反"arm 后所有失败路径必须 disarm"不变量 | 序列化挪到 arm 之前（防御性，行为不变） |

### 登记留待后续（5 组行为差异 → compatibility.md 灰区表已同步）

- [M2] Memory 官方解析语义三细节：解析失败官方 `Memory=null` 且不写回（已登记 JS语义-7
  的补充）；合法 JSON 非对象值（RawMemory="5"）官方保留原值 5、我们重置 {}；解析成功后
  官方顶层 `__proto__=null`（无原型对象）。game.js:470-487。归 M2-2A Memory 正式实现；
- [M2] RoomPosition 参数解包：官方 fetchXYArguments（utils.js:36-63）只认 RoomPosition
  实例/带其实例 pos 的对象，裸 {x,y} 字面量官方得 NaN、我们按同房间计算（宽松超集）；
  官方 inRangeTo 两参形态对无 roomName 目标恒 false（官方自身与 getRangeTo 不一致）；
- [M2] 官方 RoomPosition 有 `__packedPos` own 属性（isEqualTo 快路径 rooms.js:1577），
  我们按坐标比较等价、属性缺失玩家可感知；
- [M2] 模块包装形态：官方 evalCode（runtime-driver.js:52-77）为
  `(function __module(module,exports){…})`，require 是全局变量、非严格模块 this 绑定全局
  对象；我们 Node 形态（require 为参数、this=module.exports）——与循环 require（JS语义-10）
  同批拍板；
- [M2] Game 全局官方为普通可写属性（每 tick 重挂），我们 getter-only——极端边缘。

本轮确认与官方一致、无需动作的高风险区（抽查记录）：FIND_* 常量值 28 项逐项一致
（官方 constants.js:21-48 ↔ assets/constants.json）；budget 桶经济四条规则与
make.js:78-90/194/206-212 逐条一致；Memory 写回条件（`_parsed` truthy 才写回、写回耗时
计入 usedCleanTime）与 runtime.js:249-272 一致；getTerrainAt 与 map.js:235-258 逐字一致；
getDirectionTo 与 utils.js:64-103 逐字一致；serialization 检测口径（constructor 非
Object/Array、数组豁免、ctor undefined 豁免）与 adapter-spec.md 一致；官方 toString 中
generatePixel 在本地官方源码中不存在，桩值 -9 维持现状不臆改。

### 结构健康度评估（M1 建议）

prelude.js 已 1233 行单 IIFE，六类职责（探测仪表/console/Buffer 垫片/CJS 加载器/
RawMemory+Memory/世界投影+宿主通道）共享 8 个闭包状态变量。M0 内风险可控（测试覆盖
到位），但它同时是"验收仪表"与"被验收环境"，继续膨胀会让每轮评审的回归面指数增长。
建议：M1 期间按 sandbox.rs 的注入顺序拆为 3~4 个脚本段（buffer-shim / cjs-loader /
memory+world-projection / host-channel），各段独立 IIFE、只经 globalThis.__kk 与共享对象
交互——零行为变更的结构拆分，可在 M1 任意时点做，不必等 M2 玩家 API 层整体替换。
sandbox.rs / proto.rs / embed.rs 分层清晰，无需拆分。

### "六轮仍有新 bug"的根因分析

1. **桩保真度的形态学盲区**：第五轮对照了"返回值形态"、第六轮对照了"生命周期"，
   但**字符串化形态（toString）与序列化形态（toJSON）**这一族从未被系统性对照——
   本轮 9 条修复中 3 条（七-2/七-5 及错误消息七-4）属于这一族。方法补齐：对每个桩
   对象枚举 ECMAScript 协议钩子（toString/toJSON/Symbol.toPrimitive/hasInstance）。
2. **层间一致从未作为独立审计维度**：prelude room.find 与 dumb matches_find 是同一
   FIND 语义的两个实现，六轮都在单层内找 bug，没有"同一语义两层对表"（七-6）。
3. **官方取证入口单一**：六轮依赖台账已记录的官方行号回查（rooms.js:1588 等），
   被"踩到过"的面才有行号——toString 家族、evalCode 包装、requireFn 错误消息都是
   bot 没踩到的面。本轮改为对官方源码文件正向通读（utils.js 全量、game.js 装配区、
   runtime-driver.js），未踩面才有机会显形。
4. **错误路径的资源量级没被审计**：代码正确性审过（抛不抛、抛什么），但"抛之前
   分配了多少内存"这类量级维度漏了（七-1 的 4GB）。

### 验证记录（第七轮）

- `cargo fmt --all` + `cargo clippy --workspace --all-targets`：通过（零警告）；
- `cargo test --workspace`：全绿（kk-runner 单测 8 + m0_sandbox_suite/m0_thread_model_suite
  含新增 2 个子场景、embed_engine_suite 含新断言、embed_rpc/dumb/budget 全过）；
- `pnpm run check` 全套：cargo 三件套 + renderer/shared/compat JS 测试（适配器端到端
  1 pass）+ 文档链接检查（47 个 Markdown / 135 个本地链接）通过；
- release 冒烟：tooangel 300 tick 完成 300/300（硬中断 0、跳帧 0、缺失 API 0 项）、
  wasm-bot 10 tick 完成 10/10（wasm loaded: true、零中断、缺失 API 0）。

---

## 追记：M0 收官（2026-09-27）：CI 上线 + 基线重验

第七轮后的两个收尾缺口处理完毕，M0 三条验收关卡全部闭环：

### 1. GitHub 远端与在线 CI 建立（"CI 可用"关卡闭环）

- 私有仓库 `lovezhangchuangxin/kkscreeps`，main 分支全量推送；
- 推送前修正 ci.yml：rust job 工具链从 `@stable` 固定为 `1.98.1`（与
  rust-toolchain.toml 同步——@stable 会因 toml 自动切换但 rustfmt/clippy 组件
  只装在 stable 上，`cargo fmt` 会报组件缺失；这是 CI 从未真跑过暴露的问题）；
- **Actions 首跑三 job 全绿**（Linux：rust 2m36s 含 fmt/clippy/test/构建/适配器
  端到端冒烟、js 17s、docs 8s）——Linux 复测随首跑完成（第一轮工具链-1 类平台
  差异在 Linux 无复发）。

### 2. 基线正式重验（229 → 236，入库 JSON 是第一轮修复的迟到产物）

首份基线 JSON 产自 3b35de2（第一轮评审修复），第二~七轮修复合入后**从未重跑**。
收官轮重跑两次全量：**236 passed / 1652 failed / 1026 skipped**，run1 与 run2
逐用例零差异（数字可复现）。229→236 的 15 项翻转全部归因闭环：

- 11 项转绿 = 二~六轮修复的迟到显形（第四轮 placeStructure→spawn 归一、第五轮
  RoomVisual return this、五/六轮 PathFinder 形态与 path 语义等——旧失败消息
  "spawn.recycleCreep is not a function" 即归一缺失的直接症状）；
- 4 项转红（UNDOC-STALERECV-001 系列）= **假绿显形，非回归**：测试期待 stale
  缓存引用调用抛 runtime 错，旧代码方法不存在 → TypeError 恰好满足断言；归一
  修复后方法存在、桩返回 OK → 显形为真失败（stale 引用失效语义属 M2 登记范围）。

机制实证方法：git worktree 在 14668be 构建旧二进制，对同一用例单跑对照
（RECYCLE-CREEP-001 单跑即 pass、STALERECV-001 单跑即 fail，与首份入库基线相反
——差异源于入库 JSON 的代码版本，而非套件顺序依赖，排除 flaky）。

**棘轮登记值更新为 236**（基线报告与 README/roadmap 已同步）；教训入棘轮机制条目：
入库 JSON 必须随修复重跑更新，防止修复效果迟到显形与假绿滞留。

---

## 追记：第八轮评审（2026-09-27，外部 ChatGPT 复核收官 HEAD 28162a5）

6 条发现逐条复核（关键两条以 vanilla 参照实现与官方源码裁决），修复 4 条、登记 2 条：

### 已修复（4 条）

| # | 级别 | 复核结论与处置 |
|---|---|---|
| 八-5 | high（ChatGPT 标"存疑"，复核确认存在） | **end_tick 写回前的 cancel 清除唯一超时信号**：若看门狗恰在 run 成功后投递 terminate（同代只投一次），cancel 清标志后挂死的写回（`Memory.toJSON` 死循环）没有第二次终止机会 → 线程永久挂死。窗口为 µs 级、ChatGPT 12 次探针未复现，但代码路径推演成立。修复：cancel 后**重新武装到本 tick 原 deadline**（由 tick_start 重算，与 begin_tick 零漂移）——迟到标志照清（第三轮"防误伤"语义保留）、写回挂死由新代兜底（第六轮"写回在武装窗口内"语义保留），两个既有语义不再矛盾。附带收紧：deadline 已过时写回被掐（官方"超时打断整个 run 含序列化"的正确方向，旧行为反是漏计费）。确定性测试 writeback_rearmed_after_cancel（手动武装过期 deadline + 等其 fire + 挂死写回必须被掐，旧代码此测试挂死超时） |
| 八-3 | medium | `runPlayer("({toJSON(){return undefined}})")` 被归 runtime（错误消息 '"undefined" is not valid JSON' 显系序列化阶段）。spec 文本：失败在返回值序列化阶段归 serialization；vanilla 参照是意外成功返回 null（其 value 键被 stringify 丢弃）、套件无锁定用例——按 spec 文本与阶段语义修为 serialization（JSON.stringify 产物非字符串的显式分支）。同第六轮"Memory.toJSON 返回非字符串"的返回值对称问题 |
| 八-4 | medium | FIND_SOURCES_ACTIVE 把 energy:0 的 source 也算 active——官方 game.js:23 条件 `energy > 0`，且第七轮的"哑世界无 active 语义"判断错误（place 的初态 energy 就是充分信息）。修复：dumb matches_find 与 prelude room.find 两层同步按初态能量过滤（energy 缺省 3000，与 wrapObject 投影缺省一致）。**基线随之 236→237：ROOM-FIND-005（"FIND_SOURCES_ACTIVE only those with energy > 0"）转绿——套件本就锁定此语义**，无转红 |
| 八-6 | low | sandbox 套件的"全流程 LIFO 零违规"断言位于中段（其后还有 6 个建沙箱场景，后半段违规会漏网）。移至全部场景之后 |

### 登记留待后续（2 条，均有官方/参照对照依据）

- **八-1 [high→M5]** TypedArray backing store 不受 heap 闸门约束（`new Uint8Array(270MB)` 成功且 RSS 288MB，探针复现）：**官方 ivm 的 memoryLimit 同样不覆盖 backing store**（官方玩家同样可分配）——非玩家可见差异，是资源治理缺口，与已登记的 wasm 线性内存同族。M0~M2 的兜底：看门狗掐断持续写循环 + 按组拆进程。每 isolate backing store 计量与限额归 **M5 安全加固**；sandbox.md 安全边界节已扩充登记
- **八-2 [high→登记上游分歧]** runPlayer 嵌套值的静默 JSON 转换（`[undefined,NaN,Infinity]`→`[null,null,null]`、`{p:RoomPosition}`→退化坐标对象）：spec 文本字面要求"recursively composed of number/string/boolean/null"，但 **vanilla 参照实现完全不深检**（Game Object Detection 段明文"top-level check / does not require deep inspection"，嵌套活对象豁免）且我们行为与 vanilla 一致（2897 vanilla 通过率下套件未锁定）。按第四轮先例（spec 文本与参照分歧登记上游、行为以参照为准）不改行为，登记分歧；若上游日后澄清要求深检，M2 玩家 API 层一并实现

### 对"是否影响 M0 完工"的回应

ChatGPT 总体判断称八-1/八-2 使 M0 不宜宣告完成。复核结论：八-1 官方引擎同款行为（非玩家可见差异），sandbox.md/roadmap 的资源隔离与安全加固本就定义在 M5（"自用阶段可单进程"）；八-2 与 vanilla 参照一致且套件未锁定。两条均不构成 M0 验收关卡的缺口；八-5 是真问题且已修复。M0 完工结论维持。

### 验证记录（第八轮）

cargo fmt/clippy 零警告；cargo test --workspace 21 组全绿（含新增 writeback_rearmed_after_cancel 与 104 过滤/toJSON 分类断言）；pnpm run check 全套绿；tooangel 300 tick 与 wasm-bot 10 tick 零中断零缺失；基线重跑 **237**/1651/1026（ROOM-FIND-005 转绿、无转红，入库 JSON 已随行为变化更新——收官教训落实）。

### 追记：八-7（本轮连带发现，修复验证期间实测触发）

八-5 修复后的全量门禁中 `cargo test` 偶发挂死（sandbox_tests 进程 join 永久阻塞，sample 取栈定位）。**根因：watchdog 的 arm/disarm/shutdown 三个 notify_all 都在锁外调用**——锁外 notify 与看门狗线程"持锁检查状态 → cond.wait 原子注册等待"的临界区赛跑：notify 恰落在临界区内时落空，看门狗随后睡死，`Watchdog::shutdown` 的 join 永久挂起（进程无法收尾）。微秒级窗口、极易漏测：并发-1 修复时只原子化了"代际递增与状态变更"，唤醒侧未同步审视；八-5 增加的 end_tick 重新武装加大了 arm/disarm 往返频率，把窗口概率放大到实测可触发。修复：三个 notify 改为**持锁 notify**（Mesa 模式——对方要么已在等待队列、要么拿锁后必见新状态，全部路径闭合）。验证：修复后压测 sandbox 套件 20 连跑全绿（修复前全量门禁可复现挂死）；watchdog 语义（arm/disarm/disarm 后再 arm、shutdown 幂等）不变。八-7 属纯并发正确性修复，无 JS 可见行为变化，基线 237 不受影响。

### 验证记录补充（八-7 后重跑）

cargo fmt/clippy 零警告；cargo test --workspace 21 组全绿；pnpm run check 全套绿；tooangel 300 tick 与 wasm-bot 10 tick 零中断零缺失（重建后的 release 二进制）。
