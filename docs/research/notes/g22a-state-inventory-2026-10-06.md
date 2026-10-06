# G22.a 状态清单与检查点 schema 门禁交付笔记（2026-10-06）

基线 `a9b4dfd`（登记提交 `eeaf5f8`）。目标：给 `EmbedEngine::checkpoint()` 载荷建立逐域字段清单文档与机器可执行的 schema 键树门禁，作为"新增持久化字段必须当批接线保存/恢复/版本策略"纪律的第一道自动防线（R08 类事故的回归闸）。

## 交付内容

1. `docs/architecture/state-inventory.md`：编排层 8 域、引擎层 9 域的 归属模块/字段路径/版本策略/恢复消费者/迁移状态 表格；不持久化域 6 项及理由；夹具覆盖边界与身份门现状（engine_version+room_schema_version 双硬门、plan_hash 等值核对、ModIdentity manifest+files_sha256、sim 附加身份）。迁移状态全部如实标注"拒绝不迁移，G23 负责"。
2. `crates/kk-runtime/src/embed/checkpoint/tests.rs` 新增 `checkpoint_schema_keys_are_pinned`：提取三个场景真实检查点的键树并集，与 `PINNED_CHECKPOINT_KEYS`（99 键）逐项双向 diff——官方包全量场景（双房/双玩家/JS+二进制模块/Mod 对象/市场订单与交易/通知与 segments/停用包，双投影通道各采一份，p1 注入跨 shard 资源）∪ 故障自动停用场景（mounts 声明包处理器连续故障跨阈值 2，`fault_streak`/`last_fault`/`last_auto_disable` 取非空值并携带已声明扩展字段）。
3. 夹具修正：`MarketOrder.created_timestamp` 从 0 改为当前墙钟毫秒。`order_lifecycle`（`crates/kk-engine/src/settle/market/orders.rs:293`）按 `now_ms - created_timestamp > MARKET_ORDER_LIFE_TIME`（30 天）墙钟判过期，置 0 的订单在首轮结算即被清退，订单表 12 字段的 schema 根本进不了键树；修正后订单存活，门禁覆盖 +12 键。

## 键树折叠规则

- 对象展开全部键；数组折叠为 `路径[]` 并只下钻首元素；数据键映射（SCHEMA_DYNAMIC_MAPS：players/rooms/segments/resources/mod_states/orders/workers/modules/constant_overrides/mods.packages/last_console）折叠为 `.*` 仍下钻内部 schema；不透明叶子（metadata、mods.options、manifest、files_sha256、扩展字段载荷）不再下钻。
- 双向断言：新增键（schema 长出）与缺失键（schema 缩水）都判失败，错误信息直接打印两组 diff，指引同步 state-inventory 文档与钉死清单。

## 证据

- 门禁测试绿：`cargo test -p kk-runtime --lib checkpoint` → 14 passed; 0 failed（含共享夹具的全部既有检查点测试，墙钟夹具改动无破坏）。
- 变异验证一（拦截能力）：钉死清单临时去掉 `engine.plan_hash` → 测试红（实际键树多出该键被判"新增"）；恢复后绿。
- 变异验证二（夹具修正被门禁发现）：created_timestamp 改墙钟后首跑 → 门禁红并列出 12 个 `engine.market.orders.*` 新增键；登记后绿。这同时证明门禁对"夹具覆盖变化"也敏感。
- `pnpm run check:fast -- kk-runtime` 全绿（fmt/Clippy/测试/链接检查 308 篇/源码规模）。
- 复审清偿后键树从 76 键扩到 99 键：故障三域展开（2+8+11 键，另加健康场景的三个空值裸叶）、`players.*.resources.*`、`rooms.*.extensions.*`；`engine.mod_states` 移入不透明叶子（namespace state 是 Mod 自管 JSON，与 `extensions.*` 载荷同理，避免宿主门禁与官方 Mod 内部 state 形状耦合）。
- 覆盖边界（如实登记，不声称全覆盖）：残余盲区为"数组只下钻首元素"（`last_events[]` 自由 Value 按当时首位形状钉死）与不透明叶子内部形状；`fault_streak`/`last_fault`/`last_auto_disable` 是持久化链上没有 `deny_unknown_fields` 的少数结构，其 schema 只能靠键树发现；键树只约束 schema 形状，取值等价仍由冷恢复往返相等测试与 sim 双通道验收负责。

## 三路复审结论

三路后台只读复审（测试正确性 / 文档事实核对 / 台账范围），无 P0。已清偿：

- **P1（测试正确性）**①门禁对 R08 原形态（给自动停用/故障记录增删字段）不设防且文档兜底表述失实——三域夹具中为 None，且恰是持久化链上无 `deny_unknown_fields` 的结构；②空映射连字段存在性都未钉死（`resources`/`extensions` 整字段删除不红）。修复：新增故障自动停用场景（含 mounts 扩展）使三域与两映射非空，99 键并集钉死；`state-inventory.md` 覆盖边界改为逐结构如实的兜底表述。
- **P2（清偿）**：`engine.mod_states` 移入不透明叶（宿主门禁不耦合官方 Mod 内部 state）；测试注释去批次标签、绝对断言收窄并指向文档边界；夹具墙钟注释补"随运行变化，勿引入字节级比较"；文档补测试故障钩子行、Mod 配置身份枚举补逐包配置与 profiler、"账户全字段"措辞修正、G25 点名、metadata 迁移例外说明；笔记引擎层域数 10→9。
- **P1（台账）**：台账收口随本批提交完成（G22.a 段落与任务表更新，含验证记录）。
- 三路复审实测佐证：订单墙钟过期路径（orders.rs:293、MARKET_ORDER_LIFE_TIME=30 天、placeMarketOrder 生产默认同墙钟）、共享夹具的 7 个测试全过、文档字段数（订单 12/通知 3/CPU 桶 5）与代码逐一对上、链接检查 309 篇通过。
