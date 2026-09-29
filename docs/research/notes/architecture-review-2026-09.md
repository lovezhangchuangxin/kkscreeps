# 2026-09-29 全项目架构复审笔记（六路深查 + 外部结论交叉对比）

> 性质：证据存档。结论与裁决已升格 [ADR-013](../../decisions/ADR-013-architecture-review-coordinator.md)，
> 批次方案见 [b0-runtime-coordinator.md](../../plans/b0-runtime-coordinator.md)。
> 方法：内部六路并行深查（文档决策链 / kk-engine / 沙箱与玩家 API / 数据层四 crate / 兼容测试基建 /
> 工程健康）+ 本人精读四份契约（game-loop / sandbox / tick-contract / snapshot-contract），
> 之后引入 GPT 外部独立复审（基于 d5de6ad，含最小复现实验），对其全部可验证主张逐条复核。

## 一、内部深查结论摘要

- **规模与卫生**：Rust src 19,216 行 + 测试约 12,000+，`#[test]` 271 个；Rust 侧零 TODO/FIXME；
  unsafe 仅 6 处且全部在 kk-pathfinder FFI；3 天 116 提交，工作区干净。
- **架构纪律大体守住**：engine 不依赖具体存储（无 kk-storage/kk-backend 依赖）；typed world
  直接存 schema 类型，二进制为投影（单真源+两投影，双轨无同步问题）；pathfinder 官方 pf.cc
  经 cc crate 原样编译 + 金标校验和锁定，构造性一致。
- **依赖图偏差 3 处**：kk-cli 绕过 kk-backend 直依赖 kk-world；kk-backend 无人依赖（双壳之一）；
  kk-pathfinder 建好但零接线（engine 不依赖，moveTo 仍直线桩）。
- **类型扩面样板**：新增一种对象类型需同步改动约 7 处（schema 手写编解码 match 六臂、
  world_json/snapshot 两投影、matches_find、place 归一化）——16 类对象约 550 行样板，
  M2B 前置批宏化收敛的对象。
- **文档债**：常量数 443（ADR-003 等）vs 423（实际）并存；2A 关卡"过半"（roadmap 总表）与
  ≥40%（m2-playable D4）不一致且无修订记录；ADR-004 intent"二进制编码"无延期标注；
  ADR-011 棘轮描述落后于实际四重门禁；灰区表约 125 条无状态列、尾部追加成惯例。

## 二、外部（GPT）增量发现与复核结果（全部验证属实）

1. **部分提交 bug（P0）**：`EmbedEngine::run_players` 循环内 `?` 中途冒泡（kk-engine/src/embed.rs），
   而每个玩家的 `end_tick` 已即时 `budget.settle` + Memory 写回（kk-runner/src/sandbox.rs），
   无暂存——世界结算失败回退时，先执行玩家的持久状态已泄漏。GPT 复现序列：p2 把 `Game`
   设为不可重定义 → p2 世界注入失败 → 整轮 Err、世界时间不动，但 p1 的 `Memory.n` 已从 0 变 1。
   tick-contract.md"宿主持久值必须暂存到提交点"目标未落实。**复核：代码路径逐行确认。**
2. **全局 LIFO 销毁约束是错误假设（P1）**：spike 证据（spike-deno_core.md:174，"正序 drop 直接
   abort"）出自**单线程叠多 runtime**——rusty_v8 的 Enter/Exit 是线程局部栈；被过度泛化为全局
   约束并写入 sandbox.md:68（"V8 的硬性约束"）。实际成本：`evict_idle` 只能从栈顶逐出
   （proto.rs:230），后创建的活跃玩家挡住前面空闲玩家的回收。**复核：本人重跑 GPT 探针
   （两线程各持实例、乱序销毁）：V8 不崩、存活实例正常执行 `1+1→2`、进程干净退出（exit 0），
   仅项目自家告警触发。** sandbox.md:68 表述已随本批判正。
3. **宿主控制面玩家可达（P1，已登记欠账的实锤）**：`globalThis.__kk` 挂 beginTick/endTick/
   runLoop/诊断清理，冻结但可调用；GPT 复现玩家可清自己刚发的指令、把 `Game.time` 换成 999
   （自害面为主，结算侧身份复核仍权威，但属规则破坏面且官方无此通道）。
4. **`Working` 可变持有全局 ID 分配器与玩家表**（settle/mod.rs：`next_id: &mut u64`、
   `players: &mut BTreeMap`）：房间并行化前必须先改结果表达，不能简单 `par_iter()`。
5. **数据路径量化**：每玩家每 tick 全量 `world_json_for_viewer`（含每房 2500 格地形）+ JSON
   字符串注入 + JS 侧全量重建 + 每对象 Proxy 探针 + `room.find` 线扫；100 玩家×100 房仅地形
   即每 tick 2500 万格值物化。"共享内存直读"契约（snapshot-contract.md）零代码落地。
6. signController 墙钟（creep.rs:1482 `SystemTime::now()` 落盘）与内部发现一致；GPT 两次执行
   得到不同时间戳。另：每沙箱常驻看门狗线程（watchdog.rs:4）——"一玩家一线程"实际≈2 线程。

## 三、双方收敛点（独立到达，互为印证）

保留 Rust/V8/房间模型/模块化单体；否决 ECS 重写 / 照搬 xxscreeps / 进程外沙箱（官方 5 进程
老路）/ 微服务化 / 完整事件溯源；兼容工程（棘轮+灰区+官方对照）是最大资产；官方 C++ pathfinder
复用正确；embed/V8 拖进 engine 且 Sandbox trait 被真路径架空；性能第二形态未兑现是最大风险。

## 四、分歧与折中（唯一实质分歧）

直读时机：GPT 主张全部优化等端到端测量（五步渐进）；内部主张 M2B API 大扩面（~150→340 方法，
全部按 JS 对象世界书写）前是直读迁移最后便宜窗口。**折中裁决（用户拍板）：seam 三件套
（世界注入来源接口化 + 地形按世界版本缓存 + 不可变房间版本句柄）在 M2B 扩面前落地；
列式布局与零拷贝冻结仍由测量决定（snapshot-contract"冻结格式前的验收"条款不变）。**

## 五、用户裁决（2026-09-29）

① 先关 2A（A6 原范围收口）再开 B0 清偿批；② 部分提交修复取**正式协调层**形态（embed 编排
上移 + RunResult 统一类型 + 真/假沙箱同一执行接口，一步到位）；③ 直读节奏取 **seam 先行**折中；
④ `__kk` 宿主面 **B0 批根治**（全收进 Rust 私有 V8 句柄）。

## 六、原则符合性确认（复审追问）

三原则（玩家体验一致 / 不可越狱 / Rust 最优不照搬官方）下逐项校验：B0 五项无一以玩家可见行为
换性能——seam 是纯传输层、协调层为确定性（官方根本没有事务性协调层，此为比官方更严格的自有
组件）、LIFO 纠偏纯资源管理（sandbox.md"玩家不可感知"）、TickInput 保持玩家可见的墙钟值仅重放
用记录值、宿主面收口反而**移除**官方没有的玩家可达通道。原型挂载面（`Creep.prototype.*` 等
bot 生态唯一官方扩展形）已官方同形支持：类全局非可写安装跨 tick 持久（_registry.js，对照
creeps.js:1123）、继承链复刻、对象壳 setPrototypeOf 接持久类、tooangel 原型补丁文件族在
bot 验收实跑。整 tick 原子回退 vs 官方"出错跳过继续"的刻意分歧已随本批评审批登记灰区表
（B0 落实时销账）。
