# 架构再思考复核笔记（2026-10-01）

本笔记是 [ADR-014](../../decisions/ADR-014-contribution-plane.md) 的证据存档：记录同日架构再思考会话对 [05](../05-mod-extensibility-review.md)/[06](../06-architecture-and-js-mod-review.md) 两份调研的独立核实、对 xxscreeps 机制的对照核实、以及评审门禁清偿中发现的数字勘误。性质：复核摘要，非新增实验（当日探针见同目录三份 2026-10-01 工件）。

## 1. 核实方法

两路并行独立调查（各一路 subagent，互不见对方结论）：（a）本仓源码全链复核——crate 拓扑、tick 全链路、数据模型、intent 清洗、玩家沙箱、结算器、测试/性能/存储现状、耦合度；（b）xxscreeps 本地 checkout（4795a33）机制核实——mod 贡献体系、schema 扩展、tick 执行模型、intent 系统、常量可变性、性能特征。之后按工作流门禁对定稿文档做三路评审（事实核对/决策一致性/可实施性），发现的问题已全部清偿。

## 2. 对 05/06 关键断言的复核结论

**全部属实，未发现与代码相悖的实质断言**。要点锚点：

- 封闭分派面：`Action` enum（kk-engine/src/intent.rs:23，**33 变体**）、`RoomObject`（kk-schema/src/room.rs:402，16 变体）、intent 清洗白名单（intent.rs:495 起）、creep 动作 match（settle/creep/mod.rs:51）、结构分派（settle/structures.rs）、阶段 8 逐对象 match（settle/mod.rs:579-608）、市场分节 match（settle/market.rs:330）、schema 宏展开（kk-schema/src/lib.rs）——**多处（≥5 处）封闭 enum/match**，新增玩法必改 Rust。
- `Engine::tick_with_intents_with_input`（settle/mod.rs:723）；`Working<'w>` 借用 room/players/next_id（settle/mod.rs:160 起）；`settle_market` 以已结算房数组为入参、直接改 `self.market`、不可失败（settle/market.rs:219/330，mod.rs:788）。
- 玩家执行逐个串行等回执（kk-runtime/src/embed.rs:642-704）；`tick_all` 批量接口仅测试用。
- 投影为每玩家每 tick 全量 JSON 重建（kk-runner/src/sandbox.rs:410，begin_tick 序列化整个 world JSON）。
- 桶账：`cpu_billed_ms` 候选回传协调器但落桶仍靠 `settle_bill` 回执，线程失联时 `let _ =` 忽略（embed.rs:823 附近）。
- 计费为墙钟毫秒（kk-runner/src/budget.rs:22 登记 TODO）。
- kk-storage/kk-backend 为 3 行注释占位；apps/packages 前端占位。
- 棘轮地板 1852/2914=63.55%（compat 报告 2026-10-01）。
- js_mod_host 实验（deno_core 0.412.0，5 release + 1 debug 进程）验证 JS↔Rust 再入/候选回滚/冻结后注册拒绝/V8 terminate。

## 3. xxscreeps 机制核实要点

- mod 体系：Manifest（dependencies/provides/types）+ 11 种提供面（backend/config/constants/driver/game/main/processor/schema/storage/terrain/test）经 Node module loader hook 聚合为虚拟模块；各服务按需装配子集。
- 三处冻结时机：schema（structForPath/variantForPath 关闭后注册即抛错）、intent（worker 启动时 `initializeIntentConstraints` 拓扑排序 + bitmask）、hook（key 被 took 后注册抛错）——**加载期封闭世界**。
- schema 扩展：`registerStruct('Room', {...})` 给已有对象加字段、`registerVariant('Room.objects', ...)` 加对象类型、`registerEnumerated` 扩枚举；持久化为自研定长二进制 blob（墙 44B、满身 creep 274B、多数房间 250B~60KB）+ 双缓冲键（`room{time%2}`）+ 版本化归档升级 + overlay getter 惰性物化（未读字段不进 V8 堆）。
- tick 模型：main/processor/runner/backend 四服务 + pubsub；**runner 与 processor 并发、房间级流水线重叠**（某房间玩家 intent 计数归零即处理）；房间 worker 亲和 + 跨 tick Room 缓存；房间睡眠/唤醒；tickSpeed 默认 250ms；intentAbandonTimeout 5s。
- 玩家执行：isolated-vm 每玩家一个 isolate；**每玩家每 tick 全量读全部可见房间 blob**（无增量）；CPU 计费用 `isolate.cpuTime`（线程 CPU）；常量经虚拟模块命名空间合并，玩家与宿主同源（mod 加载期变异常量即改宿主结算）。
- intent：registerIntentProcessor 的 before/after/type(bitmask)/internal 约束；check 函数在 runner 与 processor 双跑（UX 非安全边界）；intent JSON 序列化经 scratch 存储中转。
- 规模：玩法 mods 约 3.65 万行 ≈ 引擎核心（engine+game+schema+driver ≈ 1.62 万行）的 2.2 倍；classic 本身只是 mod 依赖包（其 dependencies 为一组子 mod，具体计数两轮调查分别为 28/30，ADR-014 行文回避精确数）。
- 对官方的取舍证据：README/docs 自述 blob 免序列化（对照官方 runner 多次序列化）、run/process 重叠（对照官方 processor 干等全部玩家脚本）；"功能上还有残缺，但地基扎实"。

## 4. 定型结论（详见 ADR-014）

六原则（贡献平面启动冻结/vanilla 同契约/冷热双轨 slot/三平面+投影一步设计/全域 TickTxn/零差异+显式发布）+ 四项用户拍板（vanilla 同契约=是；投影一步设计分步实施；首版共同可信玩法 VM；三方对照 benchmark 与 A 批并行）+ 封闭分派评审门禁 + 批次 A–G 接受并调整（市场入候选进 A、RunJob 并发并行轨、B 合并投影协议设计、vanilla 扩面继续但受门禁）。

## 5. 评审门禁清偿中的数字勘误（对 05/06 及初稿）

| 初稿表述 | 勘误后 | 依据 |
|---|---|---|
| Action 31 变体 | **33 变体** | intent.rs:23-83 逐个数 |
| 投影占 wall 约 45% | **投影约 24%、玩家执行约 45%**（8 玩家档 p50：0.541/2.265、1.027/2.265） | architecture-perf-probe-2026-10-01.json |
| wall p50 2.27ms | **2.265ms** | 同上（三次运行中位数） |
| settle_tests 约 230 | **210（kk-engine 全部测试约 230）** | tests/ #[test] 计数 |
| bot 三夹具 2000 tick | **tooangel/overmind 2000t + wasm 10t（既有口径）** | compat/reports/bot-wasm.json |
| classic 30 个 mod 依赖 | 回避精确数（28/30 两轮调查不一致） | classic/index.ts dependencies |

另三处结构性修正：A4 桶账不留「保留现状」选项（06 §2.1 已有方向：协调器候选扣桶）；A1「迟到报告拒收」联调验收移 P1（当前同步 ack 模型下不存在迟到报告）；P2「同 seed」改「同地形数据导入」（跨引擎无共同 seed）。
