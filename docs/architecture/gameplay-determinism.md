# 确定性玩法时间与随机协议

本页是 G22.b 交付的玩法 realm（`kk-mod-host`）时间与随机协议的权威契约：
怎么抽随机、怎么读时间、哪些状态可恢复、哪些非确定性被明确排除在玩法
之外。检查点的整体机制见[基础检查点](checkpoints.md)，逐域清单见
[状态域清单](state-inventory.md)，设计承诺原文见
[Mod 系统设计](../designs/mod-system.md)。

## 原则

玩法规则必须是给定输入的确定函数：同一种子、同一已提交状态、同一输入
与执行计划，产出逐位相同的世界。任何 `Date`/`Math.random`/模块全局缓存
都不能被称为可回放状态——因此玩法 realm 的这两个全局是**确定性版本**
（不是"另增一个确定性入口"），墙钟与系统随机不再暴露给规则代码。

## 随机流协议

- **无状态推导**：每次抽取由 `kk_core::rng::gameplay_draw(seed, domain, tick, index)`
  整体混合得出（FNV-1a 域串 + SplitMix64 终结器，与引擎矿物再生
  `MineralRng` 同族），`gameplay_unit` 取高 53 位映射 `[0,1)`。混合常量
  属冻结协议：改变它等于改变所有已定场景的抽值（黄金断言即红）。
- **流身份（domain）**：结算期每抽按 `绑定键#房间#目标ID` 拼装——无关
  绑定、房间与对象互不挤占序列；房间级槽的目标是固定哨兵 ID，等效于
  按房分流。注册期模块顶层走 `{命名空间}#init`，**不含 tick**：冷恢复
  重装与首次装载同值（重装不变式）。
- **序号与重置**：`index` 是本域本轮（结算轮）内的抽取序号，
  `ModRealm::begin_settlement` 每轮清零。失败轮候选被丢弃后重试、或
  丢弃整轮后同 tick 重开，都从 index 0 重抽同值——重试确定性不依赖
  任何持久游标。
- **持久化**：协议的全部持久状态就是世界种子（`engine.world.seed`，
  随世界标量域与房间快照头持久化，恢复时核验）。随机派生不新增持久
  域、不进 `checkpoint_format_version`、不进 PlanHash/执行策略身份
  （宿主行为，非 Mod 声明）。G25 回放只需重放输入，无需记录抽值日志。
- **调用面**：SDK 在每次处理器调用处建立流域作用域（`sdk/chains.js`
  的 `withStreamScope`；wrap 链内层处理器有自己的域，退栈后外层继续
  用自己的域）。`Math.random()` 经 `op_mod_random(domain, tick)` 取值；
  注册期经 `op_mod_random_init()`（不在注册窗口调用按注册状态报错）。
  直呼派发路径（`dispatch`，仅测试与诊断使用）缺 tick 的 ctx 按注册期
  口径取 0、目标为空串。作用域只覆盖处理器函数体：宿主在处理器返回后
  读取结果属性（thenable 探测、异常 message）可能触发用户 getter，此时
  已退回外层或无作用域——取值仍确定（外层域或报错），但抽值不属本处理器
  域。realm 为共同可信宿主，op 不校验域串——伪造只影响伪造者自己的序列。

## 处理器随机注入（测试面）

embed 协议 `tick({count, random})` 可为**结算处理器**的随机消费注入
确定序列（套件 randomInjection 能力）：值域整单前置校验（`[0,1)`，越界
点名拒收且不推进时间），整次调用共享一个序列、逐值供矿物再密化门与
密度选择消费（耗尽报错并回滚该拍候选，此前已成功拍保留），多余值无害。
注入经 `Engine.processor_random`（`ProcessorRandom::Injected`）在调用
期间生效、结束后无论成败复位——玩家沙箱的 `Math.random` 与 Mod 随机
流协议（上节）不在此域，缺省路径仍是确定性派生流。与 vanilla 适配器
（补丁覆写 engine_processor 进程级 Math.random）的两处登记差异：注入
域仅矿物再密化门与密度选择（vanilla 还覆盖掉落能量落点、stronghold
衰减时长、tower 补给掷骰——这些场景未来注入 random 时消费计数将与
vanilla 错位）；耗尽时 kk 回滚整拍并中止后续拍（vanilla 只冻结出错
房间当拍、其余房间照常提交且 count 循环跑完、适配器循环后报错）。注入序列的消费次序
跟随 kk 的房间/对象遍历序（房名排序），与官方 Math.random 的进程级
消费次序（db 自然序）不同属既有登记分歧。signal 逐拍模式下适配器按
tick 响应回传的 `randomConsumed` 切片续传，语义等价于整次调用共享。
失败注入拍之后的重试走派生流（与首次注入值不同）；注入值不进恢复
缓存（PendingRound）也不进输入日志——含注入 tick 的日志段回放按派生
流结算、矿物结果偏离原轮。两者均为登记差异，套件不测注入×重试/回放
组合。

## 玩法时间

- 结算期（含建筑 tick、全部槽处理器）：`Date.now()`、`new Date()` 都等于
  **当前结算 tick×3000**；可调用形态 `Date()` 返回 UTC 文本。当前结算 tick 是本轮玩家所见
  的 `Game.time`（发布后世界时间才推进）。
- 注册期（模块装载）：`Date` 恒为 0——装载不在玩法时间轴上，与装载
  时刻的世界 tick 无关（重装不变式）。`new Date(ms)` 保留毫秒语义；多参数构造、未带 UTC 的 getters/setters、`getYear/setYear` 均采用 UTC，`getTimezoneOffset()` 为 0（非法日期为 NaN）。`Date.UTC` 保持原生数值归一规则。
- `Date.parse` 和字符串构造仅接受 ISO 日期/时间及带 GMT 的 `toUTCString()` 文本；ISO 时间缺时区时按 UTC 解析，其他宽松文本返回 NaN。`Date()/toString()` 使用 `toUTCString()`，日期/时间字符串从该文本提取；locale 方法固定 UTC，未指定 locale 时用 `en-US`。这是一项明确的 SDK 语义变更，不完全等同原生 Date 的本地时区 API。
- `Temporal` 从玩法 realm 删除（`Temporal.Now` 是墙钟面）；其确定性
  部分待出现真实消费者再评估。玩家 realm 不受影响（见下）。
- Mod 的调度用 `ctx.tick`（槽 ctx 一直携带）；`Date` 系是兼容 JS 生态
  的确定性投影，不是新的时间单位。

## 种子生命周期

- 新世界经 `EmbedEngine::with_world_seed(seed)` 建种（`new()` 为 0）；
  sim 场景 `seed` 字段（缺省 0）与报告 `seed` 输出。embed RPC 协议
  （`embed_main`）暂未开放选种——该入口尚不建 Mod 分片，接入时随协议
  扩展。种子随检查点持久化，恢复分片按载荷种子构造（realm 装配期注入，
  与收养世界同根）。
- `teardown` 与重建分片保留种子（与 `World::reset` 同口径）；包切换
  （[包切换](mod-switching.md)）的候选分片按当前世界种子构造，切换后
  随机流延续同一根。
- 相同种子与相同输入得到相同抽值；不同种子必然分叉——种子是场景可
  复现身份的一部分（夹具黄金断言钉死，见
  [sim-mod-determinism](../../compat/fixtures/sim-mod-determinism/README.md)）。

## 边界与非确定性登记

- **玩家 realm 不收敛**：玩家代码的 `Date`/`Math.random`/CPU 查询保持
  生产行为（白名单测试钉死）。玩家是输入源，其已执行结果按日志记录
  （G25），不参与玩法结算回放的重放；玩家面模块（Mod 的 player.js）
  运行在玩家沙箱，同样不在本协议内。
- **引擎结算墙钟**：`TickInput.now_ms`（市场订单时间、控制器签名
  datetime）按 ADR-013 记录型外部输入处理——进程内失败重试复用缓存
  值，跨进程回放属 G25 输入日志，不在本协议内消除。
- **引擎自有随机**：矿物再生等原生结算随机维持"稳定标识 + tick 无状态
  派生"（`MineralRng`，不用 World.seed）；未来开放给 Mod 的引擎侧随机
  事件（G21）按本协议的流域划分扩展。
- **CPU 计费与看门狗**：线程 CPU 毫秒进入持久玩家桶、超限可停用 Mod
  ——过程观测口径的非确定性，已在[状态域清单](state-inventory.md)
  登记，与随机协议无关。
- Date 相关文本在同一宿主协议内采用 UTC；国际化数据库、显式 locale 和可信 Mod 自行引入的外部输入仍须由部署者固定。全局 `Intl` 没有全面虚拟化，本协议只承诺上述 Date/RNG 入口；不要通过其他宿主能力绕过回放输入边界。

## 变更纪律

改变混合常量、域拼装规则、轮重置时机或 Date 映射，都是协议变更：先升
级本页与黄金断言的预期值，明确说明旧检查点在新协议下的抽值分叉（种子
不变但序列变），再合入——不作为实现细节静默滑过。

## 宿主协议身份与旧历史

当前 `kk_mod_host::GAMEPLAY_PROTOCOL` 为 `kk-gameplay-v2-utc-date`，同时写入检查点 `host_protocol` 与输入日志头段 identity，恢复、回放、续接均核验。检查点载荷升为 v3：v2→v3 只把缺身份载荷标记为 `kk-gameplay-v1-local-date`，不会伪造当前身份。旧时区历史没有自动内容迁移，恢复明确拒绝并保留旧分片；JSONL 线格式仍为 v1，但旧头段缺协议身份时不能续接/回放。今后改变 Date、RNG 或 SDK 候选语义时必须升协议身份并说明迁移政策。
