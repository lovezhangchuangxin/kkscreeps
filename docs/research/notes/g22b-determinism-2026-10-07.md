# G22.b 确定性玩法时间/RNG——登记与交付记录

- 登记日期：2026-10-07；登记提交见 git log（本文件创建）；实现基线 `626f372`（工作树干净）。
- 任务：[后续任务台账 G22.b](../../plans/mod-next-batches.md)；设计承诺来源 [mod-system 设计](../../designs/mod-system.md)「玩法 realm 提供由 TickInput/RNG 域驱动的时间与随机 API；捕获的 Date/Math.random 也必须一致」。

## 前置审计结论（2026-10-07，两路后台只读 Explore + 本人直读复核）

- **Mod realm（kk-mod-host）无任何时间/随机控制**：`realm.rs` 的 JsRuntime 保留原生 `Date`/`Math.random`/`Temporal`，无对应 op（op 清单仅链与候选域）。这是设计承诺与现状的最大差距。
- **玩家 realm 的 `Date`/`Math.random` 是钉死的合法玩家全局**（`crates/kk-runner/tests/sandbox_tests.rs` 白名单表含 Math/Date/Temporal），且玩家 prelude 自身使用（spawn 命名、签名 datetime 回退）——属玩家输入源非确定边界（game-loop.md/local-simulation.md 已登记），本批不动。
- **引擎结算墙钟**只有 `TickInput.now_ms` 一个入口（市场订单 created_timestamp/过期退费、控制器签名 datetime），ADR-013 记录型外部输入约定已落地（失败轮 `PendingRound.input` 缓存重试不重采样，`embed/execution.rs` 注释与代码一致）。本批不重开墙钟消除。
- **引擎内唯一玩法随机源** `MineralRng`（settle/economy.rs，FNV 房名+ObjId+tick 无状态派生，刻意不用 World.seed）；`World.seed` 已随检查点与房间快照头持久化但零玩法消费者，生产恒 0（`Engine::default → World::new(0)`）。
- **seed 错位缺陷（本批新发现）**：`restore_checkpoint`（embed/checkpoint.rs）与 `switch_mod_packages`（embed/switch.rs）的候选分片都是 `Self::new()`（seed 0）先 `create_shard_with_mods` 装 realm、后由载荷世界收养（seed 来自检查点）——一旦引入 realm 种子，这两条路径的 realm 流会与世界种子静默错位。本批修复：候选分片按当前/载荷种子构造。
- 槽派发结构：整槽一次进 JS（`runChain`），SDK 在 `simple(key)/inner(depth)` 逐绑定调用处握有完整 ctx（tick/room/target.id）——流作用域在 SDK 侧设置最自然；注册期 `Registry.current` 由 Rust 在模块装载+求值窗外设置（realm.rs:739-764），init 流可经 op 解析命名空间。

## 范围拍板（登记时定案）

1. **RNG 协议（无状态派生）**：`kk-core` 新增 `gameplay_draw(seed, domain, tick, index) -> u64` 与 `gameplay_unit(draw) -> f64`（splitmix64 终结器家族，FNV-1a 域串）。结算期每抽 domain=`绑定键#房间#目标ID`、tick=ctx.tick、index=域内本轮序号；`begin_settlement()` 清零计数（轮作用域——失败重试同 tick 重抽同值、丢弃轮后同 tick 新轮重抽同值）。注册期模块顶层 domain=`{ns}#init`、tick 恒 0（重装不变式：冷恢复重装与首次装载同值）。seed 随世界持久化，**不新增持久域、不增 checkpoint_format_version、钉死键树不变**；G25 回放免记抽值日志（推导可重放）。
2. **玩法时间**：SDK 补丁（`sdk/determinism.js`）在 SDK 模块求值期（先于任何包代码）替换 realm 全局 `Date`（`Date.now()/new Date()/Date()` 结算期 = ctx.tick×3000、注册期恒 0；显式参数构造/parse/UTC 原语义继承）并删除 `Temporal`；`Math.random` 改走 `op_mod_random(domain, tick)`（结算期）与 `op_mod_random_init()`（注册期）。玩家 realm 不动。
3. **seed 注入面**：`EmbedEngine::with_world_seed(u64)`（`new()` 保持 seed 0 默认）；`teardown()`/`create_shard` 世界重置保留当前种子（与 `World::reset` 保留 seed 同口径）；restore/switch 候选分片按种子构造（缺陷修复）；`ModVm` 持有 world_seed 供 recover 重建；sim 场景 `seed` 字段（缺省 0）+ 报告输出 `seed`。
4. **边界**：引擎结算墙钟、玩家 realm、CPU 计费口径、`MineralRng`（引擎自有域）均不动；Mod 侧随机协议不进 PlanHash/执行策略身份（宿主行为非 Mod 声明）。

## 验收计划

- kk-core：派生函数性质测试（确定性、seed/domain/tick/index 各自敏感、值域）。
- kk-mod-host：realm 级测试——结算流确定性（同域同序）、轮重置、跨绑定/跨房独立、init 流与重装不变式、Date 映射与 Temporal 删除、注册窗外 Math.random 报错、with_world_seed 异种子相异。
- kk-runtime：确定性族——同种子两次运行 state 相等、异种子相异、检查点冷恢复续跑与直通等值、失败重试同 tick 等值（fail_next_settle 钩子）、switch 后流仍按世界种子、跨命名空间独立。
- sim：独立包夹具（骰子包：init 捕获 + 每抽写 state + Date.now 公式）+ 场景钉死黄金值（含异种子场景）+ 双进程冷恢复等值。

## 交付记录

（实现完成后回填）
