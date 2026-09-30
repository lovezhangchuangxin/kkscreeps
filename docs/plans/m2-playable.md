# M2 单机可玩开发方案：沙箱运行时 + 玩家 API（2A）与存储/流水线（2B）

> 状态：**已确认**（2026-09-28 用户裁决确认）。确认准则：技术方案授权自主决定，两条硬约束为
> 验收前提——①**玩家侧体验与官方一致**（[兼容红线](../designs/compatibility.md)全文有效）；
> ②**安全性对齐官方且玩家不可越狱**（沙箱逃逸为零容忍）。架构不照搬官方老架构，在两条硬约束
> 之上按性能最优自由设计（本方案中所有"对照官方"的取舍均由约束①推导——官方行为即玩家体验
> 的定义，而非架构模仿；内部层——编码/传输/存储/调度——均按性能与确定性自由选择）。
> 架构级结论定型时升格 ADR-012，实现现状回写架构文档。
> 依据：[roadmap M2](../roadmap.md)、三份 M2 目标契约（[tick](../architecture/tick-contract.md) /
> [snapshot](../architecture/snapshot-contract.md) / [performance](../architecture/performance-acceptance.md)）、
> [sandbox.md](../architecture/sandbox.md)、[game-loop.md](../architecture/game-loop.md)、[storage.md](../architecture/storage.md)、
> ADR-004 / ADR-008（第 1/2/4/5/7 条）/ ADR-011、[M0 评审台账](../../compat/reports/m0-code-review.md)各轮追记、
> [兼容灰区表](../designs/compatibility.md)。
> 官方源码引用均为本地 `~/Desktop/projects/sources/screeps/` 的 file:line（engine/ 与 driver/、common/）。
> 方案格式沿用 [m1-engine-skeleton.md](m1-engine-skeleton.md)。

## 0. 勘察结论摘要（方案的实证基础）

本方案动笔前完成了四路并行勘察 + 关键面亲验，以下事实是全部决策的地基：

1. **官方 intent 链路**（file:line 取证）：收集器是**沙箱内**每 tick 新建的闭包对象
   `intents = {list, cpu, set/push/pushByName/remove}`（driver/lib/runtime/runtime.js:60-100）；
   game 层各指令方法"读本 tick 快照做预检 → `intents.set(id, name, data)` → 返回 OK/ERR_*"
   （engine/src/game/creeps.js:126-156 move、:335-402 harvest）；tick 末 run 闭包把
   `outMessage.intentsList = intents.list` **一次性结构化拷出**沙箱（runtime.js:271-296，无 endTick
   之名）；**清洗在宿主进程**（engine/src/utils.js:405-436 storeIntents → common/lib/system.js:4-140
   sanitize 白名单）；计费双轨——沙箱内 push 时 +0.2（供 getUsed 即时显示，say/pull 免费、同名
   覆盖只计一次，runtime.js:60-72），宿主侧按最终清单重算扣桶（make.js:98-111、:193-194）；玩家
   loop 抛软异常时**已 push 的 intents 照收**（runtime.js:241-244 catch 后照常组装 outMessage），
   isolate 级硬故障才整轮丢弃（make.js:279-294）。
2. **官方 game 层结构**：engine/src/game/ 共 23 文件 7119 行，约 337 个方法/属性 getter（≈"约 340
   方法"说法成立）；**整层跑在 isolate 内**，宿主注入面仅 14 项（constants/evalCode/寻路 native/
   runtimeData 快照/intents 对象/RawMemory 工厂/console/getUsedCpu/getHeapStatistics/cpuHalt 等，
   runtime.js:55-231 + game.js:444-515）。`Room.find/lookAt` 是**纯 JS 内存索引**（findCache +
   50×50 网格，game.js:30-48、rooms.js:500-514），零原生需求；唯一必须原生的玩家可见计算是
   `PathFinder.search`（path-finder.js:59-63 经 driver 注入的 C++ native）。
3. **我方通道断点精确定位**：prelude 五段（00_shared/10_buffer/20_cjs/30_memory_world/90_host，
   kk-runner/src/sandbox.rs:33-42）中 `CREEP_METHODS` 等 30 个动作方法全是"计数 + return 0"的桩
   （prelude/30_memory_world.js:428-435）——**断点 A：intent 不出沙箱**；`Engine::tick` 从
   `engine.sandboxes` 收集 intent（kk-engine/src/settle/mod.rs:327-339），而 EmbedEngine 只持有
   kk-runner 的 PlayerThreads、从不注册 trait 实现——**断点 B：两套沙箱体系互不相通**。已就绪
   部件：IntentBatch/Action/IntentData 全套 typed 模型（intent.rs:21-300）、官方压制图与执行序、
   `sanitize_object_intent`（**无生产消费者**）、settle 侧 phase5 完整消费链（8 个动作执行器 +
   createCreep + room 级 2 个）、玩家身份边界硬不变量（settle/mod.rs:329-338）。
4. **screeps-ok 量化基线**（compat/reports/kkscreeps.json 全量解析）：2914 = 277 pass + 1650 fail
   + 987 skip。987 个 skip 全是能力门（lab/factory/nuke/power/terminal/boosts/market 等，M2A 一概
   不覆盖）；1650 个失败分三档——**几乎确定转绿 925**（校验矩阵 + 已有结算执行器的端到端 +
   快照投影修复）、**部分转绿 725**（缺引擎规则/结构动作族/寻路/事件投影）、不受影响若干。
   **两个隐藏依赖**（不修则什么都不亮）：① embed 查询面 `getObject/findInRoom` 投影缺 `kind`
   字段（screeps-ok 契约 common.ts 要求 `kind: 'creep'|'structure'|'site'|'source'|...`；我方
   world_json.rs:99-232 只有 `type`/`structureType`）——275 个失败直接死于 `has kind 'undefined'`；
   ② `spawnBusyCreep` 等 fixture 辅助链依赖 spawnCreep 真 intent + findInRoom 返回 name——约 691
   个"校验矩阵"用例（只断言官方错误码）的共同前置。

---

## 1. 范围与验收

### 1.1 M2A：沙箱运行时 + 玩家 API 核心

交付六件事：

1. **intent 收集通道**（D1）：prelude 动作方法从桩换为官方同形"预检 + 收集"；endTick 一次性带出；
   宿主侧清洗成 IntentBatch 接进结算——打通"真实玩家代码 → 结算 → 下一 tick 可见"闭环；
2. **玩家 API 层地基**（D2/D3）：prelude 内按官方 game/ 文件结构重组的玩家 API 层（薄壳持 id +
   getter 回查 + per-viewer 装配），动作方法族全量的 API 面与已就绪执行器端到端；
3. **查询面契约投影修复**（D5）：embed getObject/findInRoom 对齐 screeps-ok 快照契约（kind 族）；
4. **Memory 正式化**：官方解析三细节、超限信号、cpu 真值回写、模块包装形态拍板；
5. **多实例与生命周期收口**：evict_idle 逐出设计与旧对象引用失效语义（薄壳 getter 化）——
   原"LIFO 进程级所有者"设计前提已于 2026-09-29 证伪并纠偏为每线程纪律
   （[ADR-013](../decisions/ADR-013-architecture-review-coordinator.md) 决策 3），该项整体移交
   B0 批 WP2 执行；
6. **性能观测点**：阶段耗时打点（玩家运行/世界投影构建/结算），小规模负载一次实测记录。

**验收关卡**（roadmap 原文"screeps-ok 通过率过半（内部目标，随 2A 实际基线校准）"）——
现实校准见 D4 与 §8：**2A 出口 ≥40%（约 1170 passed）**，"过半"在 2B API 补全批出口强制达成。
另加 tick 契约验收场景 1（[tick-contract.md](../architecture/tick-contract.md)）：
真实沙箱的 `move`/`harvest` 指令经过清洗、结算，在下一轮玩家视图中可见；假沙箱和真实沙箱走
同一条结算入口。bot 回归：tooangel 2000 tick 重跑（M0 期证据已注明需重跑）+ overmind 2000 tick。

### 1.2 M2B：完整玩家 API + 存储 + 完整世界

1. **API 补全**：结构动作族（tower/link/observer/lab...）执行器与 API、controller 结算族
   （claim/reserve/sign/attackController/safeMode 激活链）、战斗全家（attack/ranged*/heal）、
   repair/dismantle（hits 模型）、mineral/extractor、tombstone/ruin（schema 升级）、eventLog
   投影、kk-pathfinder FFI 接入（moveTo/PathFinder.search 真语义）、market、RawMemory segments；
2. **kk-storage 落地**：redb vs libmdbx 基准后定（ADR-008 第 7 条）、房间快照落盘、检查点/
   manifest/恢复、schema v1 冻结（含 UPGRADE_CHAIN 实链）；
3. **tick 并行化**：D_t 依赖集合、玩家沙箱并发与房间结算重叠、确定性合并（tick 契约场景 2/3/4/5）；
4. **完整世界命令行**：`kkscreeps serve` 单机可玩形态（内存存储起步）+ 机器人对手。

**验收关卡**：screeps-ok ≥95% 且每个未过用例有登记差异说明（参照 xxscreeps 91%）；真实 bot
全功能运行；性能场景 A（空闲世界）+ C（多房多玩家）基线建立（[performance-acceptance.md](
../architecture/performance-acceptance.md)——M2 只锁 A+C）；snapshot 契约"冻结格式前的验收"
五场景。

### 1.3 明确不在 M2

power 全家与 powerCreep（M4，roadmap）、跨世界 shard/InterShardMemory（M4——**官方开源层无此
面**，无对照源码）、官方地形生成算法（M4，ADR-008 第 6 条）、mod 系统（ADR-007）、网络与账号
（M3）、市场成交结算的完整对齐（2B 只做 API 与订单面，成交语义随场景 C 基线细化）、正式性能
对照报告（M5）、backing store 计量（M5 安全加固，灰区已登记）。

---

## 2. 关键决策（需确认的放最前，带推荐）

### D1（最重要，需确认）：intent 收集通道 = 沙箱内闭包收集器 + endTick 一次性传出 + 宿主侧清洗接线

**推荐形态**（五段，每段都有官方对照）：

1. **prelude 收集器**：对照 runtime.js:60-100 在 prelude 闭包内实现 `intents = {list, cpu,
   set(id,name,data), push, pushByName, remove}`，null-prototype `list` 按 **对象 id → intent 名 →
   参数对象** 组织；同对象同名后者覆盖（:71）、`say/pull` 免费（:61）、覆盖不重复计费（:68）、
   cancelOrder 退费（creeps.js:1016-1022 + runtime.js:95）。收集器只存在于闭包内，玩家拿不到
   引用、无法伪造或清空（官方同款防篡改形态）——顺带根治"`__kk.drainMissing` 直调"深层防御
   欠账所属的 `__kk` 重设计。
2. **动作方法**：`mkStubMethod` 全部替换为官方形态——读本 tick 投影做**预检**（ERR_NOT_OWNER/
   ERR_BUSY/ERR_TIRED/ERR_NO_BODYPART/ERR_NOT_IN_RANGE/ERR_INVALID_ARGS/ERR_NOT_ENOUGH_RESOURCES...
   逐方法对照 creeps.js/structures.js/rooms.js 原文），预检过才 `intents.set` 并返回 OK。**预检
   在 game 层完成是官方语义**（creeps.js:126-156 move 读 fatigue/bodypart、:335-402 harvest 读
   目标能量/距离/所有权）——screeps-ok 约 691 个校验矩阵用例正是验收这一层。
3. **传出**：`__kk.endTick()` 返回值从 `{ok}` 扩展为 `{ok, intentsList, intentsCpu}`（对照官方
   run 闭包尾 `outMessage.intentsList` 一次性 ExternalCopy，runtime.js:274-296）。M2A 用 JSON 编码
   （exec_json 既有通道）；ADR-004 决策 5 的**二进制编码推迟到 M2B** 随性能基线评估，登记为
   未兑现目标（每玩家每 tick intent 数量级几十，JSON 在 M2A 负载下非瓶颈）。Memory 写回与
   intents 同批带出（官方同为 outMessage 字段）。
4. **宿主侧清洗接线**：kk-runner `end_tick` 解析 intentsList 暂存；EmbedEngine 把它转成
   IntentBatch 时跑 `sanitize_object_intent`（对照官方：清洗发生在宿主进程 utils.storeIntents →
   system.js，不在沙箱内）——**清偿"sanitize 未接生产"灰区**。房间重组对照 utils.js:405-436
   （对象反查房间；我方 typed world 有全局索引可直查，查不到的丢弃语义对齐 :427-429）。
   转换后的 batch 只归本沙箱注册玩家（复用 settle/mod.rs:329-338 身份边界不变量）。
5. **两条通道统一**：`run_player`（run_eval_tick：begin→runExpr→end）与 bot 模块化 tick
   （run_one_tick：begin→runLoop→end）都经 end_tick 带 intents；EmbedEngine 驱动序统一为
   "每玩家跑沙箱 → 收 batch → `engine.tick(batch)`"——`Engine::tick` 增加 intents 入参（或
   EmbedEngine 持 pending batch），ScriptedSandbox 通道保留给单测，两条入口汇到同一 settle_room
   （契约场景 1 的"同一结算入口"）。

**安全纵深**（约束②的工程化，玩家不可见面）：① intentsList **上行闸门**——每 tick intents
总条数/总字节上限（合法玩家在 CPU 预算内不可达的量级，如 5 万条/2MB），超限截断并出告知行
（同 M0 console 闸门形态；官方无显式上限、同受超时约束，上限取不可达量级故玩家不可见，登记
灰区）；② 清洗白名单兜底不可信输入（沙箱传来数据一律视为不可信，既有纪律）；③ 收集器闭包
私有 + `__kk` 私有化重设计（玩家拿不到引用即无法伪造/清空/抢跑）；④ endTick 返回值经
exec_json 单向，宿主侧 JSON 解析失败按基础设施错误处理。

**计费定型**（清偿"runExpr 独立通道计费"欠账）：沙箱内 `intents.cpu` 累计并计入 getUsed（对照
runtime.js:202-204）；宿主 settle 按 `0.2 × 最终清单条数`（排除 say/pull）重算并入扣桶（对照
make.js:98-111、:193-194——bucket 结算以宿主重算为准）。纯表达式探针通道（run_expr_guarded，
screeps-ok 探针用）维持不动桶。**硬中断语义**：看门狗掐断的 tick 丢弃本轮全部 intents、按整份
tickLimit 计费——对照官方 isolate 超时 run.apply reject 后 intents 全丢（make.js:279-294；软异常
intents 照收的对照是 runtime.js:241-244，我方玩家抛错路径等价物是 run_loop 捕获后照常 endTick，
天然一致）。锁用例：抛错 tick 的 intents 仍结算；硬中断 tick 的 intents 不结算且计费整份。

**createCreep 空 energyStructures 顺带清偿**：JSON 编码保留字段 presence（undefined 不编码、
`[]` 编码为空数组），IntentData 的 energy_structures 改 `Option<Vec<ObjId>>`——官方空数组走显式
列表模式（灰区表既有登记）。

**备选（不推荐）**：每条 intent 一个 op 上行（`op_kk_push_intent`）。理由：官方不是这个形态
（一次性传出）；op 面进入玩家可探测的全局面，扩大遮蔽与防篡改负担；高频 op 调用的性能代价
与收益不成比例（tick 末才需要数据）。

### D2（需确认）：玩家 API 层放 prelude JS 侧对照移植，Rust op 侧只做计算原语

**推荐**：prelude 内按官方 game/ 文件结构重组（见 §4），**重写而非拷贝**（对照移植 + file:line
证据链注释，沿用 M1 纪律）。理由：

1. **官方自己就是这个形态**：game/ 层整层跑在 isolate 里，宿主只注入 14 项（§0-2）。用 Rust op
   复刻 340 个方法意味着放弃"与官方逐行对照"的验收锚点；
2. **API 面充满 JS 细节语义**：instanceof 原型链与 bot 原型补丁（M0 实测依赖）、lodash 谓词两用
   filter（rooms.js:648-654）、`RoomPosition.__packedPos`（rooms.js:1288-1292）、Store 的 Proxy
   动态资源下标（store.js:87-96）、官方 defineGameObjectProperties 的 eval 惰性 getter
   （engine/src/utils.js:509-545）——这些在 Rust 侧没有可维持对照的等价物；
3. **M0 教训**：层间一致与形态类问题（七-6 同一 FIND 语义两层实现、PathFinder 返回 {dx,dy}
   形态、toString/toJSON 家族）全部源于"桩与官方面形态漂移"——玩家可见查询归 prelude 单源，
   embed 查询面（Rust）只对 screeps-ok 契约形状负责，两层一致性由显式对表测试锁定（既有纪律）。

**Rust op 侧只补**（对照官方注入面的子集）：`op_kk_get_used`（已有）、`op_kk_pathfinder_search`
（kk-pathfinder FFI，排期见 D4）、terrain 共享 buffer（M2B 快照直读时）。`getHeapStatistics/
cpuHalt` 低成本 op 随批补。

**模块包装形态与循环 require 拍板**（灰区表挂账项，本方案推荐**跟官方**）：官方 evalCode 包装
`(function __module(module,exports){…})`、require 是全局变量、非严格模块 this 绑全局、循环 require
抛 "Circular reference to module"（runtime-driver.js:52-77、game.js:578-581 对照；现 M0 为 Node
形态）。跟官方后 kk 更严格不会反向假绿；带循环依赖的 bot 会在 kk 报与官方同款错误。**Game 全
局可写性同批按官方对齐**（普通可写属性、每 tick 重挂，game.js:502）。

### D3（需确认）：数据面分期——M2A 保留 world_json JSON 投影，M2B 切四段布局快照直读

**推荐**：M2A 的玩家 API 层数据源维持"begin_tick 注入 world_json"（现状通道，prelude 装配层
消费），但**消费面单独成层**（§4 的 runtimeData 等价物），使 M2B 换数据源（快照直读 getter +
共享 buffer terrain，snapshot 契约布局）时 API 面零改动。理由：

1. 性能验收文档明文允许："若完整世界 JSON 仍是每玩家每 tick 的必经数据面……须记录为未兑现的
   架构目标"——M2A 负责登记，M2B 负责兑现；
2. API 层重写与数据面重写并行推进会让每步评审的归因面翻倍（M1 经验：单变量提交是棘轮可归因
   的前提）；
3. M2A 负载（screeps-ok 单房小世界、bot 单房）下 world_json 不是瓶颈，性能关卡的实测在 M2B。

**薄壳 getter 化是两期的桥**：M2A 起 wrapObject 从"值拷贝"改为"持 id 的薄壳，getter 回查当前
tick 投影表"（对照官方 `data(id)` 闭包回查 runtimeData，engine/src/utils.js:509-545）——这同时
是**旧对象引用失效语义**的正确实现形态（对象存活则旧引用读到新值、对象消亡按 screeps-ok 27.12
锁定的官方行为；snapshot 契约"包装器保存 id 不保存裸偏移"同款）与 M2B 快照直读的接入点。

**备选（不推荐）**：M2A 就做四段布局原型——会把 schema 冻结、快照验收与 API 层移植搅在同一步；
四段布局的字节常量按契约须"依据官方可导入的最大房间与内存基准确定"，本身依赖 M2B 的基准。

> **追记（2026-09-29，[ADR-013](../decisions/ADR-013-architecture-review-coordinator.md) 决策 2）**：
> D3 的方向不变，时间线细化为 **seam 先行**——M2B API 大扩面（~150→340 方法）**之前**先落地
> 三件：世界注入来源接口化、地形按世界版本缓存、不可变房间版本句柄；M2B 新命名空间的 getter
> 写在 seam 抽象之上（第一后端可仍为 JSON 形态）。四段布局与零拷贝冻结仍按本决策的备选理由
> 留待端到端测量。动机：扩面后再切直读，prelude 改写面积翻倍；seam 先行使"不在旧形态上继续
> 盖楼"与"不过早冻结布局"同时成立。批次定位于 B0 之后的 M2B 前置批。

### D4（需确认）：先 2A 后 2B；2A 关卡现实校准为"出口 ≥40% + 过半落在 2B API 补全批出口"

**推荐顺序**：2A 全部完成并重跑基线后才进 2B。理由：真沙箱对真实代码的兼容性是项目最大
不确定性（roadmap 排序原则：风险先行）；ADR-008 第 7 条的存储选型依赖基准，而基准需要 2A 落地
后的真实负载形态（真玩家代码 × intent 通道 × API 面完整度）——存储基准反而被 2A 阻塞。

**关卡校准**（量化依据见 §8）：2A 六步的现实出口区间 **950–1250 passed（33–43%）**，中位约
1150（≈40%）。"过半"（>1457）需要再吃掉约 260–500 个档 2 用例（mineral harvest 67、eventLog
投影 29、controller 结算族 27、repair/dismantle/近战 hits 模型 ~55、寻路 FFI ~45、tombstone
18...）——这些全是引擎/schema 扩展工作，塞进 2A 会让其范围膨胀约三成并稀释评审带宽。故推荐：

- **2A 出口关卡 = ≥40% 且逐批归因**（roadmap"随实际基线校准"的具体化）；
- **"过半"关卡 = 2B API 补全批出口兜底**；三轨并行（§10）后轨 B 的 B1–B4 恰好覆盖高性价比
  档 2——轨 B 推进快则在 2A 期内即达成过半，无需把引擎扩展硬塞进轨 A；
- 2A 轨 A 批 A4（动作族落地）后设**校准复核点**：用三条轨的实测数据复核出口预测。

**kk-pathfinder FFI 排期**：推荐 **2B 首批**（寻路是独立联编风险：官方 pf.cc C++、macOS/Linux
双端 FFI、CI 工具链；且 pathfinding 用例族约 45 个不足以改变 2A 关卡结论）。M2A 的 moveTo/
moveByPath/findPath 保持直线桩但**登记灰区**（假结果、恒 incomplete:false 的现状照旧，不冒充
真语义）。变体 B 下可提前。

### D5（需确认）：查询面契约投影修复放 Rust 单源

embed `getObject/findInRoom` 的快照形状对齐 screeps-ok 契约（`kind` + 字段族），**在
world_json 的 object_json 出口统一加 `kind`**（creep→creep、spawn/structure→structure、
constructionSite→site、resource→resource、source/mineral/deposit/tombstone/ruin 按 common.ts
契约表），prelude 消费面不受影响（additive）。备选：TS 适配器转换层——不推荐，理由：投影形状
会有第三个真相源（Rust object_json / TS 转换 / prelude wrapObject），违背层间一致纪律。

### D6：bot/验收路径的驱动统一（随 D1 落地，不需单独确认）

`EmbedEngine::tick(n)` 从"只跑环境结算"改为"驱动注册玩家沙箱 → 收 intents → 结算"（bot_check
与 sim 共用）；`runPlayer` 通道同样汇入。`ScriptedSandbox` 保留为单测假沙箱（ADR-008 第 5 条
一真一假不变）。

---

## 3. intent 通道详设（官方对照表）

| 站 | 官方（file:line） | 我方 M2A |
|---|---|---|
| 收集器 | runtime.js:60-100（闭包内、null-proto list、set/push/pushByName/remove） | prelude 闭包内同形收集器（90_host 段持有，不暴露给玩家） |
| 预检 | creeps.js:126-156 等（读快照判 ERR_*，过检才 set） | 薄壳 getter 读当前 tick 投影，逐方法对照移植 |
| 计费（即时） | runtime.js:60-72（0.2、say/pull 免费、覆盖不重复计） | `intents.cpu` 累计计入 getUsed（op_kk_get_used 路径） |
| 传出 | runtime.js:271-296（outMessage.intentsList 一次性 ExternalCopy） | `__kk.endTick()` 返回 `{ok, intentsList, intentsCpu}`（JSON） |
| 清洗 | utils.js:405-436 → system.js:4-140（宿主进程、白名单/parseInt/bodypart/userString） | `sanitize_object_intent` 在 IntentBatch 转换处接线（生产消费） |
| 房间重组 | utils.js:426-429（对象反查房间、查不到丢弃） | typed world 索引直查；不可解析 id 丢弃 |
| 计费（结算） | make.js:98-111、193-211（0.2×count 扣桶、封顶、lastUsedCpu） | budget.settle 并入 `0.2×count`（排除 say/pull） |
| 消费 | processor.js:227-311（users→objects、'room' 特殊键、:248 所有权守卫） | IntentBatch（users→objects+room）→ settle phase5（既有） |
| 软异常 | runtime.js:241-244（intents 照收） | run_loop 捕获后照常 endTick（既有形态天然一致） |
| 硬故障 | make.js:279-294（intents 全丢） | 看门狗掐断 → 丢弃本轮 intents + 整份计费（锁用例） |
| 幂等覆盖 | runtime.js:68-71 | IntentData 同对象同 Action 后写覆盖（既有 add_object_intent） |
| 撤销 | creeps.js:1016-1022 cancelOrder + runtime.js:95 退费 | `intents.remove` + cpu 退费（API 面随批 4） |

差异登记（灰区表新增）：intentsList 编码 JSON vs 官方 ExternalCopy（玩家不可见，M2B 评估二进制）；
`room`/`global`/`notify` 特殊键中，M2A 只实装 room 级（RoomIntent 既有 2 变体），global（market）
与 notify 随 2B 对应能力。

---

## 4. 玩家 API 层详设（模块划分 + 官方对照）

### 4.1 prelude 结构重组（30_memory_world 段拆分，对照官方 game/ 文件序）

```
prelude/
├── 00_shared.js / 10_buffer.js / 20_cjs.js        # 不动
├── 30_game/                                        # 玩家 API 层（新，对照 engine/src/game/）
│   ├── _registry.js      # register/findCache/薄壳基类/makeGameObject 装配序（game.js:50-438 对照）
│   ├── _props.js         # defineGameObjectProperties 等价（非 eval 实现，登记灰区）
│   ├── room-position.js  # rooms.js:1288-1662（x/y/roomName getter、__packedPos、15 方法）
│   ├── room.js           # rooms.js:1138-1220（find/look*/findPath(桩)/createFlag/createConstructionSite/
│   │                     #   getTerrain/Terrain/RoomVisual/memory）
│   ├── creep.js          # creeps.js 全量 28 方法 + 13 getter + memory
│   ├── structures.js     # 2A 核心子集：Structure/OwnedStructure/Controller/Spawn+Spawning/Extension/
│   │                     #   Storage/Container/Road/Wall/Rampart/Tower/Terminal(只读)/Portal(只读)/
│   │                     #   Source/Mineral(只读)/Resource + Store（store.js 全量，清偿 JS语义-12）
│   ├── sites-flags.js    # construction-sites.js + flags.js（flag.memory 懒 getter 清偿 JS语义-16）
│   ├── game.js           # game.js 对照：Game 装配（collections/ getObjectById/notify/cpu/map 桩）
│   └── console.js        # 官方 console 形态（既有实现迁移）
└── 90_host.js            # 宿主通道（endTick 扩展 intents 字段；__kk 收敛为 beginTick/runLoop/
                          #   runExpr/endTick/resetModules/仪表排干，全部闭包私有化）
```

### 4.2 per-viewer 装配（清偿正确性-3 与 energyAvailable 灰区）

world_json 增加 `user`（viewer 玩家名，embed run_player 的 handle 名）与 cpu 真值（begin_tick 时
从 budget 现算：limit/tickLimit=min(500,limit+bucket)/bucket——清偿测试-8 两真相源）。prelude 装配层
对照 game.js:273-339：`Game.creeps/spawns/structures/flags` 只收 viewer 自己的（:302-322）、`my` 按
owner 判、`FIND_MY_*` 过滤、energyAvailable/energyCapacityAvailable 按 viewer 的非 off spawn/
extension 累计（:325-328）。**视野过滤（不可见对象不进沙箱）不做**——M2A 数据面全量注入，
per-viewer 语义全部在装配层；登记为 M2B 数据面未兑现项（对照官方 runtimeData.get 按玩家组装，
driver/lib/runtime/data.js:200-261）。

### 4.3 M2A 动作方法范围（API 面全量、执行器分批）

| 批 | API 面（预检+计费+intent push） | 结算执行器 | 状态 |
|---|---|---|---|
| 2A 批 2（通道样例） | move（direction 形态）/harvest | 同左（已有） | 端到端闭环 |
| 2A 批 4 | Creep 全部 28 方法 + spawn 族（spawnCreep/canCreateCreep/renew/recycle API）+ room 级 createConstructionSite/createFlag | transfer/withdraw/drop/pickup/build/upgradeController + createCreep + room 级 2 个 + say/suicide（顺手，结算小）；move 补 id 形态（朝相邻 creep，清偿灰区） | API 全量；attack/heal/ranged\*/repair/dismantle/claim/reserve/sign/attackController/generateSafeMode/pull 的执行器留 2B。**勘正（2026-09-30 双房批复核）**：room 级 createConstructionSite/createFlag 的 intent 通道与结算执行器已交付，但玩家 API 面实为 countStub（room.js:477/489，自 A3a 拆分起即 stub）——本行「API 面」栏对这两项超前宣称，真实现归 M2B API 扩面 |
| 2A 拓展批（可选，D4 变体 B） | — | claim/reserve/sign/attackController/generateSafeMode + repair/dismantle + 近战 attack（hits 模型同根） | 决策点后定 |
| 2B | tower/link/observer/lab 族 | 战斗全家 + pull + 结构动作 + mineral + tombstone | — |

### 4.4 官方文件对照表（M2A 范围内）

| 官方文件（行数） | M2A 处置 | 依据 |
|---|---|---|
| game.js（595） | 全量对照 | 装配器/Memory/require 是一切前提 |
| rooms.js（1662） | 全量（findPath/serializePath 保持桩并登记；寻路真语义 2B） | movement/pathfinding/room-mechanics 族 + 两 bot 核心读路径 |
| creeps.js（1125） | 全量 28 方法 | 动作族是 2A 主交付 |
| structures.js（1491） | 核心子集（§4.1 列表）；Lab/Link/Factory/Observer/PowerSpawn/Nuker 推 2B | combat/spawning/energy/construction 族依赖核心子集 |
| sources/resources/minerals/store/construction-sites/flags/names（≈400） | 全量（mineral 行为面 2B） | 采集-建造-存储主循环 |
| map.js（380）/path-utils（123） | 核心 9 方法；map.visual 推 2B | 21-map 族 + bot 远征 |
| path-finder.js（80） | CostMatrix 全量；search 维持桩（2B FFI） | 直线桩现状延续，登记灰区 |
| console.js（115） | 全量 | bot 输出唯一通道 |
| tombstones/ruins/nukes/deposits/market/power-creeps（≈870） | 推 2B/M4 | 能力门 skip 池 |

---

## 5. M2B 设计要点（决策锁定，细节随批细化）

1. **性能基线先行**：2B 首批埋正式观测点（tick 墙钟分段/复制字节/分配/GC——performance 文档
   统一指标表），跑场景 A（100 房空闲）+ C（50 活跃房/100 玩家）粗基线；**world_json 每玩家每
   tick 全量注入作为未兑现目标记入报告**。场景 fixture 首跑锁定（性能文档要求）。
2. **存储选型**：redb vs libmdbx 基准（写放大/读延迟/检查点停顿，用场景 C 负载）→ 新 ADR；
   kk-storage 按 snapshot 契约四段布局落盘 + manifest 原子切换 + 检查点各写入阶段崩溃注入测试
   （契约"冻结格式前的验收"场景 4）；schema v1 冻结（tombstone/多资源槽/非 energy 掉落等变体
   入库，UPGRADE_CHAIN 实链——冻结日起 bump 纪律生效，替代现"v1 内演化"灰区登记）。
3. **tick 并行化**：一玩家一线程派发（既有 PlayerThreads）+ D_t 保守上界推导（tick 契约：按
   已提交世界的对象归属/视野/可执行 API 推导，'room' 级 intents 经可见 Room 触达）+ 确定性合并
   （房间名序/全局 ID 重写）+ 失败重试只消费缓存 RunResult；验收场景 2/3/4/5 逐条配测试。
4. **完整世界**：`kkscreeps serve` 内存存储形态 + 机器人对手账号；bot 全功能回归关卡。

---

## 6. 欠账处置表（M2 输入 → 批次映射）

台账与灰区表全部 M2 登记项的处置（修=2A 某批；2B=随对应能力）：

| # | 登记项（出处） | 处置 |
|---|---|---|
| 1 | intent 收集通道（ADR-011 M2 输入） | **修**（2A 批 2，D1） |
| 2 | sanitize 未接生产（深评登记） | **修**（2A 批 2 接线） |
| 3 | runExpr 独立通道计费口径（步 6 登记） | **修**（2A 批 2，D1 计费定型） |
| 4 | 多实例 LIFO/evict_idle（第四轮，重登记 M2-2A） | **修**（移交 B0 WP2：LIFO 前提已纠偏为每线程——ADR-013 决策 3，非栈顶任意逐出） |
| 5 | 旧对象引用失效语义（第四轮登记 M2） | **修**（2A 批 3：薄壳 getter 化 + 27.12 锁定用例） |
| 6 | move intent 的 id 形态（灰区） | **修**（2A 批 4：move API 双形态对照 creeps.js:132-141） |
| 7 | createCreep 空 energyStructures（灰区） | **修**（2A 批 2：编码 presence） |
| 8 | energyAvailable per-viewer（灰区） | **修**（2A 批 3，§4.2） |
| 9 | 正确性-3 多玩家视角（my 硬编码/Game.collections/FIND_MY_*） | **修**（2A 批 3，§4.2） |
| 10 | Memory 解析三细节（JS语义-7 + 第七轮） | **修**（2A 批 5，对照 game.js:470-487） |
| 11 | Memory >2MB 静默丢弃无信号（安全-9） | **修**（2A 批 5：信号通道随 Memory 正式化） |
| 12 | store 垫片数值（JS语义-12） | **修**（2A 批 4：Store 类对照 store.js 全量） |
| 13 | require.cache 值形态（JS语义-15）+ 循环 require/模块包装拍板（JS语义-10 + 第七轮） | **修**（2A 批 5：跟官方，D2） |
| 14 | flag memory 懒 getter（JS语义-16） | **修**（2A 批 4） |
| 15 | Game.cpu 桩值两真相源（测试-8） | **修**（2A 批 5：budget 真值注入 world_json） |
| 16 | RoomPosition 宽松解包/__packedPos（灰区） | **修**（2A 批 5：fetchXYArguments 逐字对照 utils.js:36-63） |
| 17 | 仪表防篡改深层（安全-6 残余） | **修**（2A 批 2：`__kk` 闭包私有化重设计） |
| 18 | userString UTF-16 截断、to_int 域外（深评登记） | **复核**（2A 批 2 对照点：如 screeps-ok 显形则对齐，否则维持登记） |
| 19 | 无 controller 建造配额（灰区） | **修**（2A 批 4 顺手：按 rcl=0 计） |
| 20 | spawn.spawning 投影 remainingTime 形态（灰区） | **修**（2A 批 4：structures.js:1302-1310 对照） |
| 21 | controller hits undefined 对齐（灰区） | **修**（2A 批 4：投影不发、prelude 不兜） |
| 22 | tombstone 变体/死亡事件（灰区+ADR-011） | **2B**（schema 升级批） |
| 23 | 多资源槽 storeCapacityResource/非 energy 掉落（灰区） | **2B**（schema 扩展批） |
| 24 | 踩工地 safeMode 门/activateSafeMode 族（灰区） | **2B**（controller 结算族批） |
| 25 | build 的同格 swamp/wall 对象系数（灰区） | **2B**（Generic 对象合成路径定型时） |
| 26 | serializePath/findPath 步进对象（重登记 M2-2A） | **2B**（kk-pathfinder FFI 批） |
| 27 | schema v1 内演化→冻结纪律（灰区） | **2B**（存储批冻结 v1） |
| 28 | movement 四键平手决胜序（灰区） | **维持登记**（顺序敏感用例显形时归因） |

---

## 7. 测试计划

- **通道层**（2A 批 2 重点，全部锁值）：
  - 官方收集器语义：同对象同名覆盖只计一次费、say/pull 免费、cancelOrder 退费、`[]` vs
    undefined 的 presence；
  - 清洗接线：sanitize transforms 既有单测保持 + 生产路径贯通断言（伪造字段/未知 intent 名被丢）；
  - 身份边界：冒名键丢弃（既有不变量 + 真沙箱贯通用例）；
  - 软异常 intents 照收 / 硬中断 intents 丢弃且整份计费；
  - tick 契约场景 1：真沙箱 move/harvest 经清洗结算下一轮可见；假沙箱（ScriptedSandbox）与
    真沙箱产出相同世界转移（同一结算入口的等价性断言）；
  - 计费：getUsed 含 intents.cpu（预检后 push 即反映）；settle 重算与沙箱计数一致性。
- **API 层**（批 4/5 重点）：每方法一族官方对照断言（错误码矩阵：NOT_OWNER/BUSY/TIRED/
  NO_BODYPART/NOT_IN_RANGE/INVALID_ARGS/NOT_ENOUGH_RESOURCES/INVALID_TARGET 逐方法逐参形态）；
  per-viewer 装配（双玩家房间的 Game.collections/my/FIND_MY_*/energyAvailable）；薄壳旧引用
  （存活读新值/消亡按 27.12）；Memory 三细节（解析失败 null 不写回/非对象保留/__proto__=null）；
  模块形态（循环 require 抛官方错、require.cache 形态、this 绑定）。
- **查询面**（批 1）：object_json 的 kind 映射表逐类型断言；findInRoom 返回 name（fixture 辅助
  链前置）；embed_tests 形状回归更新。
- **回归纪律**：每批全量 `node compat/scripts/ratchet.mjs`，转红逐用例归因（HEAD~1 基线 diff，
  ratchet-allow 显式豁免）；每批 bot 回归（tooangel 300t 常跑 + 批 6 重跑 2000t、overmind 2000t、
  wasm-bot 10t），bot-check 退出码机器判定。
- **安全验收**（约束②，每批回归）：全局面白名单比对测试维持（deno_core 升级新增残留全局即显形）；
  篡改面用例——收集器不可达性、`__kk` 冻结与私有化、伪造 intentsList 字段被 sanitize 丢弃、
  intents 上行闸门超限截断、runExpr 间接 eval 维持、硬中断丢 intents；玩家不能经任何投影面
  （world_json/快照）获得宿主可写句柄。
- **2B**：tick 契约场景 2/3/4/5（并行等价/失败重试/越权拒绝/重叠收益报告）；snapshot 契约五
  场景（往返/畸形拒载/慢读者/崩溃恢复/字节数报告）；性能场景 A+C（预热 500t、采集 3000t、
  ≥5 次独立运行取中位）。

---

## 8. screeps-ok 增量量化与关卡校准

基线 277/2914（passed/total；1650 fail + 987 skip）。全量解析的分档（§0-4）：

| 档 | 规模 | 内容 |
|---|---|---|
| 1 几乎确定转绿 | 925（31.7%） | 校验矩阵族（6.1-6.3/7.1/repair/dismantle/pull/renew/recycle/gensafemode 的错误码断言 ≈98+84+60）；已有执行器端到端（4.x 199/5.1 37/5.4 69/3.1 41/6.4 48/9.1 46/1.1 28）；查询面投影（00-adapter 52 + 26.0 部分）；RCL 配额/成本/isActive 49；小族合计 ~85 |
| 2 部分转绿 | 725（24.9%） | mineral 67、eventlog 29、controller 结算 27、近战伤害 28、repair/dismantle 28、寻路 45、link/tower 108、tombstone/ruin 25、spawning 细节 18、per-owner 13、RawMemory 段 13、memhack/global 11... |
| 3 不受影响 | 987 skip（33.8%） | 能力门（lab/factory/nuke/powers/terminal/boosts/market/deposit/observer/portal/multiShard...）——2B 能力翻转才显形 |

**M2A 轨 A 六步的批间预期**（累计 passed，含归因折损估计；轨 B/C 的合入增量另计，见 §10）：

| 批 | 内容 | 预期增量 | 累计区间 |
|---|---|---|---|
| 1 | 查询面 kind/字段族 | +50~120 | 330~400 |
| 2 | 通道 + move/harvest 样例 | +30~60 | 360~460 |
| 3 | 薄壳 + per-viewer 装配 | +30~60 | 390~520 |
| 4 | 动作族 API 全量 + 已有执行器端到端 | +350~500 | 740~1020 |
| 5 | Room/RoomPos/Memory/模块形态 | +40~70 | 780~1090 |
| 6 | 结构读取面 + isActive + 收尾 | +30~60 | **810~1150（28%~39%）** |

与独立分档估计（925 档 1 × 85–95% 兑现率）交叉验证：**2A 出口现实区间 950–1250（33–43%）**。
预测方法论复盘（外部复审 ② 采纳，2026-09-28）：A2 预测 +30~60 实际 +218，偏差 = 范围漂移
（批 2 从"move/harvest 样例"扩到九方法，叠加 B1/B1.1 执行器）× 方法缺陷（预测未冻结候选用例
ID、未单列"通道本身"与"并轨交叉解锁"的贡献）——后续批次预测须列候选用例族清单，交叉解锁
另计；A1 预测 +50~120 实际 +97 落在区间内。
与 roadmap"过半"的缺口 260–500 由 D4 校准方案处置。主要不确定性：校验矩阵的错误码次序与快照
字段细节返工；withdraw/drop 的 container（Generic 透传）与非 energy 资源用例可能滑出（执行器
白名单只有 spawn/extension/storage/source）——批 4 实测后归因，必要时 container 判定按
structureType 补（属于既有执行器的小扩展，非新能力）。

---

## 9. 风险与灰区预登记

| 风险 | 对策 |
|---|---|
| 桩变真后的"假绿显形转红"（M0 收官轮 UNDOC-STALERECV 先例：方法不存在→TypeError 恰好满足断言；方法变真后旧失败池形态改变） | 棘轮 HEAD~1 逐用例 diff + 转红归因纪律；预期转红集中在 stale 引用族（27.12）——正是批 3 要锁定的新语义 |
| 预检层读投影的时序一致性（官方预检读 tick 初快照；同 tick 前序对象变更不可见） | 薄壳 getter 绑定 begin_tick 注入的单一投影版本（同 tick 内不刷新），对照官方 runtimeData 不可变性 |
| world_json 全量注入的性能/内存（100 房量级） | M2A 负载无感；正式观测点批 6 埋设，未兑现目标记入 2B 基线报告 |
| prelude 重构规模（30_memory_world 拆 8 文件 + wrapObject 重写） | 批 3 单独成提交、零行为变更拆分先行（M1 步 1 先例）；每文件对照表驱动评审 |
| deno_core 升级引入新残留全局 | 既有全局面白名单比对测试显形（sandbox.md 安全边界） |
| 玩家可影响世界后的防篡改面扩大（伪造 intentsList/篡改收集器） | 收集器闭包私有；endTick 返回值经 exec_json 单向；sanitize 白名单兜底（不可信输入面纪律） |
| 灰区新增（本方案预登记）：intentsList JSON 编码 vs ExternalCopy；moveTo/findPath 直线桩持续；视野过滤缺失（装配层替代）；非 eval 版 defineGameObjectProperties；查询面 kind 投影是验收通道形态非玩家面 | 全部进 compatibility.md 灰区表，随批更新 |
| 2B 存储选型基准被 load 形态偏差污染 | 基准 fixture 锁定 + 与 2A 实测负载形态对账（性能文档"正确性前置"节） |

---

## 10. 实施顺序（三轨并行：subagent 轨道 + worktree 隔离 + 串行合入）

**机制**（2026-09-28 确认）：并发边界是轨道不是任务（轨内文件耦合必须串行）。主会话前台
推进轨 A 并掌握全局进度；轨 B/C 为后台 subagent，各自在 `.worktrees/track-b|c`（独立分支
`m2/track-b`、`m2/track-c`）开发、自验 `cargo test -p <crate>`、提交到自己的分支。**开发并行、
合入串行**：一次只合一条轨一批 → 分支 diff 上 3 路评审 subagent（轨 B 视角：官方结算语义对照 /
Rust 正确性 / 测试锁值充分性；轨 C 视角：官方对照 / unsafe 边界 / 测试与基准）→ 合入 main（
合入轨提交由 pre-commit、merge 由 pre-merge-commit hook 拦 rustfmt，见 README「本地开发与提交检查」）→
`cargo test --workspace` → 全量棘轮 → bot 回归 → 地板更新 → 单一归因提交。合入由主会话执行。

**文件所有权**（写进每批规格，越界即评审打回）：

| 轨 | 所有权 | 冲突面处理 |
|---|---|---|
| A（主会话前台） | `crates/kk-runner/**`、`kk-engine/src/{embed,sandbox}.rs`、`kk-world/src/world_json.rs`、prelude | `lib.rs`/`Cargo.toml`/`Cargo.lock` 小冲突合并时手工处理 |
| B | `kk-engine/src/settle/**`、`kk-engine/src/intent.rs`、`kk-engine/tests/**`、`kk-schema/src/room.rs`、`kk-world/src/{types,place}.rs` | 同上；**投影接线**（world_json/prelude 侧消费新字段）在合入后由轨 A 批次接 |
| C | `crates/kk-pathfinder/**`、`kk-storage` 骨架 | 同上 |

**轨 A（沙箱与玩家 API 面，六步；批 3 拆三提交降低单步回归面）**：

- **A1 查询面契约投影**：object_json 加 kind 映射 + getter 形态值（spawning remainingTime、
  safeMode/ticksToDowngrade 剩余值、ticksToRegeneration null 形态、store/storeCapacity）+
  embed_tests 形状回归（预期 +50~120，立即验证隐藏依赖判断）；
- **A2 intent 通道**：prelude 收集器 + 动作样例（move/harvest）+ endTick 扩展 + kk-runner 通道
  （EvalReport/TickReport 带 intents）+ sanitize 生产接线 + EmbedEngine→Engine 接线 + 计费双轨 +
  intents 上行闸门 + energyStructures presence + tick 契约场景 1 测试；
- **A3a prelude 30_game/ 零行为拆分**（八文件对照官方 game/ 结构，纯移动不改语义）；
- **A3b 薄壳 getter 化**（wrapObject 改持 id 回查当前 tick 投影；旧引用语义用例 27.12）；
- **A3c per-viewer 装配**（world_json 增 user/cpu 真值；Game.collections/my/FIND_MY_*/
  energyAvailable 按 viewer——清偿正确性-3 与 energyAvailable 灰区）；
- **A4 动作族 API 全量**：creep 28 方法 + spawn 族 + room 级指令 + Store 类 + move id 形态 +
  say/suicide 执行器 + spawning 投影形态 + controller hits + 无 controller 配额 + flag 懒 memory；
- **A5 Room/RoomPosition/Memory/模块形态**：find/look*/getTerrain/packedPos/fetchXY 对照 +
  Memory 三细节 + 超限信号 + 循环 require/模块包装/Game 可写性跟官方 + require.cache 形态；
- **A6 收尾**：结构读取面（isActive/hits 族）+ 多实例 LIFO/evict_idle 设计（已移交 B0 WP2，前提纠偏见 ADR-013）+ tooangel 2000t
  重跑 + 性能观测点埋设 + 小规模负载实测记录 + ADR-012 + 棘轮地板上调 + 灰区表/架构文档回写。

**轨 B（引擎结算面，ScriptedSandbox 单测验证，不依赖真沙箱）**：

- **B1 hits 模型 + attack/repair/dismantle 执行器**（伤害公式/部件损伤分布/死亡合流）；
- **B2 controller 结算族**（claim/reserve/sign/attackController/generateSafeMode + safeMode 激活
  链 + 踩工地 safeMode 门清偿 + schema 补 reservation/sign 字段）；
- **B3 tombstone/ruin 变体 + 死亡事件 + TTL 死亡产墓碑**（schema v1 内演化窗口）；
- **B4 mineral/extractor + 多资源槽**（schema 扩展）。
每批合入后由轨 A 侧接投影，点亮对应 screeps-ok 族——**过半关卡在 B1–B4 与 A4 合入后复核**
（D4 校准的并发化：轨 B 快则在 2A 期内达标）。

**轨 C（独立 crate）**：

- **C1 kk-pathfinder FFI**：官方 pf.cc 拷入 + cc 编译（macOS arm64 本机先过）+ search 安全封装 +
  路径逐点用例；
- **C2 CostMatrix 编解码 + roomCallback 通道设计 + 基准**；
- **C3 存储选型基准骨架**（redb vs libmdbx 微基准 + 场景 fixture 备料；正式选型仍待 2A 落地后
  真实负载，ADR-008 第 7 条）。

**2B（2A 收口后出细化批次清单，按 §5 决策锁定推进）**：

7. 性能观测与场景 A+C 粗基线（未兑现目标登记）；
8. kk-pathfinder 全量接入（moveTo/findPath/serializePath 真语义，吃 C1/C2 成果）；
9. API 补全批组（结构动作族/战斗全家/mineral/eventLog 投影接线/tombstone 投影/market/
   RawMemory segments——**过半关卡在本批组出口兜底**）；
10. 存储选型基准 + 新 ADR + kk-storage 落盘 + manifest/检查点 + schema v1 冻结；
11. tick 并行化（D_t/合并/重试，契约场景 2-5）；
12. `kkscreeps serve` 完整世界 + 机器人对手 + bot 全功能回归 + 性能基线正式报告（A+C）+
    2B 收口（≥95% 关卡核验 + 缺口登记）。

---

## 11. 纪律（沿用仓库既有约定）

- 行为主张全部 file:line 取证；官方没有的语义不臆断（本方案 §0 的取证在实施批逐条复核行号）；
- 玩家 API 层为**重写而非拷贝**官方源码，证据链式注释（官方 file:line + 机制知识留在代码旁，
  先结论后出处、一条一个要点）；
- 灰区差异登记 compatibility.md，不写进代码注释凑数；
- 每步提交前 ≥3 路 subagent 并行评审（官方语义/契约投影/Rust 正确性与并发三视角）；外部 AI
  报告逐条复核再修，修复后可请外部复审 HEAD；
- 行为变化收尾全量重跑棘轮并按需上调地板（ratchet.json 唯一真相源）；转红逐用例归因 +
  ratchet-allow 显式豁免；文档数字同步。
