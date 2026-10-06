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

**实现（2026-10-07，随本批提交）**：

1. `kk-core/src/rng.rs`：`gameplay_draw(seed, domain, tick, index)`（FNV-1a 域串 + SplitMix64 终结）与 `gameplay_unit`（高 53 位 → [0,1)）；混合常量以固定向量钉死并声明为冻结协议。
2. `kk-mod-host/src/rng.rs`（新）：`RngState`（种子 + 域序号计数）入 OpState；`op_mod_random(domain, tick)`（结算流；tick 走 f64——op 的 u64 参数需 BigInt，2^53 内与 u64 逐位一致）与 `op_mod_random_init()`（注册窗口经 `Registry.active_namespace` 解析 `{ns}#init`，窗外报错）；`ModRealm::begin_settlement` 轮作用域清零（重试/同 tick 重开重抽同值）；`ModRealm::with_world_seed(seed, options)` 构造器（`with_options` 委托种子 0）。
3. `kk-mod-host/src/sdk/determinism.js`（新，SDK 求值期先于任何包代码）：`Math.random` 改走 op；`Date` 替换为确定性包装（结算期 `now/new Date()/Date()` = 作用域 tick×3000，注册期恒 0；显式参数构造与 parse/UTC 继承原生）；删除 `Temporal`。`sdk/chains.js` 在 `dispatch/simple/inner` 三处处理器调用外建立 `withStreamScope`（wrap 链嵌套保存/恢复；域 = `绑定键#房间#目标ID`）。
4. `kk-runtime`：`EmbedEngine::with_world_seed`（`Engine::new(World::new(seed))`）；`teardown`/`create_shard` 世界重置保留种子；`setup.rs` realm 构造注入 `engine.world().seed()`；`ModVm` 持有 `world_seed` 供 recover 重建；**修复两条种子错位路径**——`restore_checkpoint`（`Self::with_world_seed(saved.engine.world.seed)`）与 `switch_mod_packages`（`Self::with_world_seed(self.engine.world().seed())`）：此前 `Self::new()`（种子 0）先装 realm 后收养带种子的世界。
5. `kk-cli sim`：场景 `seed` 字段（缺省 0）、报告顶层 `seed` 与最终观察 `seed`。
6. 夹具：`compat/fixtures/mod-determinism/dice`（init 捕获 + 每 tick 双抽 + Date 终值写 ctx.state）与 `compat/fixtures/sim-mod-determinism`（两种子场景钉黄金抽值；断言只取 tick 1 即存在的指针，分相冷恢复不误红）。
7. 文档：新[确定性玩法时间与随机协议](../../architecture/gameplay-determinism.md)（原则/随机流协议/玩法时间/种子生命周期/边界登记/变更纪律）；同步 state-inventory RNG 行、checkpoints 两处、mod-execution 重建段、local-simulation 场景表与玩家边界句、mod-switching 边界行、architecture/README、docs/README、source-organization。

**验收（实际执行，含复审清偿增量）**：kk-core rng 6 项（含固定向量与桶均匀下限）；kk-mod-host 确定性 8 项（种子敏感、协议推导钉死、轮重置、跨绑定/跨房独立、init 无 tick 重装不变、wrap 链跨层域隔离、跨包不共流、原生 Date 不可取回+注册窗外随机报错）+ lib 全量；kk-runtime `mod_determinism_tests` 5 项（同种子逐位相等/异种子相异/Date 公式、冷恢复续跑等值（并断言恢复种子=载荷种子）、Validate 段失败重试重抽同值、切换重建延续种子流、teardown/重建保种子）；kk-cli sim 确定性 2 项（双场景黄金通过+异种子对照、6 tick 直通 vs 3+检查点+3 双进程逐位等值）。登记计划中「runtime 级跨命名空间独立」实际落在 mod-host realm 级（真实跨包双注册，比 runtime 级更贴协议面）。冷恢复等值断言范围是 Mod 命名空间状态与时间/种子（非零种子下的全状态等值由基线 seed-0 checkpoint 往返族+房间快照头种子核验外推，此处接受）。无兼容面改动（玩家 realm 未动），不需 release 棘轮。

## 复审记录（三路后台只读，2026-10-07）

**R1 协议正确性**：无 P0。P1×2 已清偿——①原生 Date 构造器两条原型链泄漏路径（静态 `Object.getPrototypeOf(Date)`、实例 `Date.prototype.constructor`）：determinism.js 重构为静态面不复挂原生原型（now/parse/UTC 以不可枚举自有属性提供）+ 遮蔽原生 prototype 的 constructor，新增探针测试断言两条路径均不可达且 parse/UTC/instanceof/toJSON（UTC ISO 文本）保真；②wrap 链跨层域隔离与注册窗外报错零覆盖：新增 wrap 处理器（sheller，next 前后各抽）+ RecordingHost 整槽测试钉死「内层抽取不挤占外层序号」，探针测试钉死窗外 `Math.random` 显式报错。P2 清偿：op 内开发期断言拦截垃圾 tick（NaN/负/小数与 tick 0 域串别名）；KkDate 静态面属性特性对齐原生（不可枚举、name/length）；结果属性 getter 在作用域外的边角以协议页「调用面」登记（取值仍确定）。零固定点（seed=0∧空域）属理论备注——绑定键非空，不可达。

**R2 种子集成**：无 P0/P1。P2 清偿：sim 报告新增顶层/观察 `seed` 随纪律升版 6→7（注释与 switch 测试钉值同步）；embed RPC 无选种面在协议页登记（该入口尚不建 Mod 分片，接入时随协议扩展）；冷恢复等值范围与 Date.name 指纹（name 已对齐原生 'Date'）登记。核对确认：候选分片构造仅 restore/switch 两处、realm 构造器全部汇聚 with_world_seed、RngState 单点注入、`Engine::new(World::new(seed))` 与原默认逐位等价、sim 装载检查点时载荷种子经 `*self = replacement` 整体胜出（场景字节 SHA 门+房间快照头种子核验双封）、V8 栈序约束遵守（多分片测试对早分片仅 Rust 侧读）。

**R3 文档一致性**：无 P0。P1×2 已清偿——①local-simulation.md 导语「seed 选项尚未实现」过时句改写；②注册窗外报错零覆盖与「runtime 级独立」口径偏移：前者补测试、后者以真实跨包测试+本节注记清偿。P2 清偿：协议页 `Date()` 改为「本地时区文本」口径；mod-system.md:236 追加 2026-10-07 交付注记（ctx.tick 驱动口径与 TickInput 墙钟的分工、回链协议页）；dispatch 直呼路径缺省与结果 getter 边角入协议页「调用面」。台账 G22 表行/当前任务行的收口更新随交付提交完成。
