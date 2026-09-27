# ADR-010 screeps-ok 适配器桥接：sidecar 子进程 + 嵌入式 API

- **日期**：2026-09-27
- **状态**：已接受（用户在 M0 方案评审中拍板，四项决策一并确认）

## 背景

M0 需要 screeps-ok 适配器"能执行套件并输出通过率报告"（roadmap）。screeps-ok 的适配器
必须是 TS 模块（vitest 经 `SCREEPS_OK_ADAPTER` 动态 import，导出 `createAdapter()`），
而 `runPlayer` 必须走我们的真实沙箱（deno_core，Rust）——TS 与 Rust 之间需要一座桥。
同时 ADR-008 第 8 条要求：适配器调用的引擎接口 = `kkscreeps sim` 的同一接口。

## 决策

四项（M0 方案评审拍板）：

1. **桥接形态 = 子进程 sidecar**：TS 适配器 spawn `kkscreeps embed` 子命令，
   stdin/stdout 上一行一条 JSON-RPC。每个 vitest worker 复用一个子进程
   （createShard 负责完全重置）；teardown 只重置世界与沙箱、不退出进程，EOF 退出；
   适配器侧对子进程 unref + 退出钩子，防孤儿。
2. **验收 bot = tooangel + overmind**：多模块 CJS + 重 lodash 与 TS 单文件 bundle 两形态
   互补。overmind 构建链（2018 年代）在 Node 24 不可用，按预案取 GitHub release 产物；
   wasm 语义用合成夹具覆盖。
3. **M0 桩世界 = 哑世界**：placeXxx 精确存取、tick 只推进时间计数、无任何游戏规则。
   目的是让 00-adapter-contract 一部分真过、M0 基线报告有信息量（实测 217 项通过，
   其中契约区 63 项），并避免向 M1 的引擎工作倒置依赖。
4. **screeps-ok 本体 = pin 脚本克隆**：锁定上游 commit `34a630e`，setup 脚本克隆到
   gitignored 的 `.compat/screeps-ok`；适配器产出的报告入库。不引 submodule、不入库上游代码。

## 嵌入式 API 的地位（本 ADR 的核心架构内容）

`kk-engine::embed::EmbedEngine` 是**引擎作为库的唯一入口**，方法集与 screeps-ok 适配器
契约同构（createShard / placeObject / setTerrain / runPlayer / runPlayers / tick /
getObject / findInRoom / getGameTime / captureConsoleLogs / getControllerPos / teardown）：

- `compat/` 的 TS 适配器经 `kkscreeps embed`（JSON-RPC 传输层）调用它；
- `kkscreeps sim`（M2.5）以库形式直接调用它；
- `kkscreeps bot-check`（bot 环境验收）经 kk-runner 沙箱走同一套世界投影约定。

**规则变更走 ADR**：将来若加 napi 进程内通道或改传输协议，嵌入式 API 的方法集语义
保持稳定，只换传输——这是本决策留给未来的唯一开口。

## runPlayer 的语义锚点（与 screeps-ok 契约逐条对齐）

- 恰好推进 1 tick（原子：世界注入 → 真沙箱求值 → Memory 写回与结算）；
- 错误三分类（syntax / runtime / serialization）经 `Symbol.for('screeps-ok.RunPlayerError')`
  标记传递——上游专门为外部适配器设计了跨副本判别；
- "代码未被执行"（如跳帧惩罚）≠ undefined——适配器抛普通 Error；
- capability 如实声明（M0 全 false），不虚报；parity.json 空 gap（M0 的失败都是真失败）。

## 理由

- 子进程隔离：引擎（V8/线程）崩溃不连累 vitest worker；无 napi 构建链负担；
  符合"同一二进制"原则（sidecar 就是 `kkscreeps` 本体）；
- 哑世界是"能测"与"引擎不存在"之间的最小真实解：适配器接口即引擎库接口，
  M1 换真引擎时适配器零返工；
- pin 脚本 + gitignored 工作副本：上游 4.1 万行不进本仓库历史，换 pin 一行脚本。

## 灰区声明

M0 全部工作发生在开发态基建与桩环境，**不引入任何玩家可见行为**——
`docs/designs/compatibility.md` 灰区表无需新增条目。

## 证据

- M0 基线：全量 2914 用例跑通出报告（compat/reports/kkscreeps.json + m0-screeps-ok-parity.md）
- bot 验收：compat/reports/m0-bot-missing-api.md（两 bot 2000 tick 零崩溃、清单清零）
- 相关实现：crates/kk-engine/src/embed.rs、crates/kk-cli/src/embed_main.rs、compat/src/adapter.ts
