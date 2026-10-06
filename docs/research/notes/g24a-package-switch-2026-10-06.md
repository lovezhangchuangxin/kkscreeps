# G24.a 交付笔记：无开放轮次的包/API 切换（2026-10-06）

登记提交 `6a22684`，基线 `5fbadbb`（G23.a 收口）。**范围拍板（2026-10-06 会话）**：包集合增减 + 同内容校验；同包内容变更明确拒绝并指路 G23 内容级迁移（维持「开发更新 ≠ 存量世界升级」区分）；sim 同批给最小入口做端到端验收。前置审计三路后台只读（切换先例与引擎状态全集 / 包集合与计划派生面 / sim 暴露与玩家工作者面），关键输入：restore_checkpoint 三道身份门按设计要求新旧同身份——切换走**独立入口**复用其替代分片骨架而非加模式特判；`create_shard_inner` 先回收旧分片失败不回滚，不可用于原位切换；`install_hosted_plan` 具备就地换计划失败保旧语义但仅覆盖引擎七域，EmbedEngine 面（mod_realm/玩家面/工作者）超出其覆盖。

## 交付内容

1. **引擎侧换计划收养**（kk-engine settle/checkpoint.rs）：`restore_checkpoint_replanned` 跳过载荷 plan_hash 等值（计划已由宿主先换装），保留全部内容校验——mod_states 命名空间、Mod 对象类型、扩展字段 schema 对**新计划**执行，天然充当去包残留硬门；与原入口共享抽出的 `adopt_committed_state`。
2. **切换入口**（kk-runtime embed/switch.rs，`switch_mod_packages`）：门禁（`no_open_round` + 同线程 V8 栈序）→ 从**活状态**捕获（引擎提交快照/工作者配置+模块表/停用集/故障三域/last_console/并发与通道/trace）→ 交集包 manifest+文件摘要逐项等值（同包内容变更拒）→ 候选分片全链装配（注册→canary→install_hosted_plan→spawn）→ 孤立 state 分诊（null 占位丢弃、非 null 实数据拒）→ 停用投影重放（按新计划**装配选择集**求交，非发现集）→ 故障过滤 → `retire_previous` + trace 意愿与缓冲转移 + `*self = replacement`。返回切换摘要（前后 plan_hash/执行策略/增删清单）。
3. **工作者重建语义**：预算与模块表（玩家代码）保留；计划派生常量键是封闭集（LINK_CAPACITY/仓库十键/射程四键/RESOURCES_ALL），create 路径确定性覆盖或移除——旧计划常量不残留，玩家可见常量按新计划生效；V8 堆不保留（与冷恢复同口径）。
4. **sim 接线**：场景字段 `mods_switch`（after_tick + 整体替换的 configs/selection，复用 mods.root；切换恰在提交完第 after_tick 轮后，拒绝不使运行失败）；报告 `SIM_REPORT_VERSION` 5→6 + 顶层 `mods_switches`；`--load-checkpoint` 续跑按检查点 `world.time` **与实际生效的切换**（元数据 `applied_switches`，载荷只读一次供推导与恢复复用）选择包集合——被拒切换的检查点按初始集合续跑；续跑不重放已生效切换、不重试已过边界的被拒切换。
5. **故障注入钩子** `fail_next_switch_for_test`：候选全验证后、retire 前失败——补上切换路径自身的注入缺口（既有 fail_next_* 在切换路径全部失效或被静默丢弃）。
6. **同批修复 kk-world 存量隐患**：`World::insert` 用 `dir.generation + 1` 增量比较，`from_checkpoint` 的 u64::MAX 陈旧哨兵会令恢复后的世界插入对象即溢出（基线测试从未覆盖恢复后插入）——改减法比较；切换测试即回归。

## 验收（全部实际执行）

- **runtime**（embed/switch/tests.rs，7 项）：增包（身份换封 + 新包效果仅切换后出现 + 冷恢复新集合过/旧选择集拒）；减包无残留（z-gate 规则回落内建默认 + 世界续跑 + base state 连续）；残留与内容变更双拒（旧分片完整且继续运行，报错指路 G23）；开放轮次拒 + 注入失败保旧（console 历史不被搬进报废候选，放弃后二次切换成功且保留 console）；停用投影（投影后闭包核对）+ 故障三域过滤（streak/last_fault/last_auto_disable 全注入断言）；仍安装包停用跨切换存活（+panel 同时增包）；投影闭包增长拒绝（补装依赖者使闭包超出投影集）。
- **sim**（sim_tests/switch.rs，3 项双通道）：增/减场景 applied 记录与差集正确、LINK_CAPACITY 700→800 玩家常量切换生效、counter/visits 精确值 6；冷恢复三段式（2 轮保存恰含切换、续跑装载新集合、终态与直跑等价、执行策略为切换后身份、不重放切换）+ 边界前保存（1 轮检查点按初始集合续跑、切换在续跑循环内生效）；被拒切换（clockwork 对象在世减包即残留）运行不失败、报告记 rejected 与错误、旧计划继续、该检查点按初始集合续跑。
- `cargo test -p kk-runtime --lib` 46 项全过；完整 `pnpm run check` 见提交记录。

## 设计取舍

- **独立入口而非 restore 加模式**：三道等值门语义是「新旧必须同身份」，切换恰好要打破——复用即三处以上特判。
- **换计划收养而非世界重建**：保留 Engine::restore_checkpoint 的全部内容校验作为残留硬门，只跳过不适用的 plan_hash 等值——校验逻辑单一来源。
- **停用投影按装配选择集**：发现集含未选择包，对其求交会把未安装 ID 喂给停用闭包（实测踩中「未知 Mod」——修正为 composition.selection.selected 求交）。
- **孤立 state 的 null/实数据分诊**：链式提交对每个执行过的绑定都落键（实测 mod_states 含 `{"z-gate":null}`），null 占位无信息量随包丢弃；非 null 是 Mod 提交过的数据，静默删除等于丢数据——拒绝。
- **trace 缓冲随切换转移**：切换是继续运行而非冷恢复，诊断连续性不中断（retire 只取走 realm，ModVm 结构仍在，缓冲可转移）。
- **sim 用场景字段而非 CLI 旗标**：场景文件保持唯一身份真相源（scenario_sha256 不变即可冷恢复），续跑按 world.time 推导生效集。

## 已知边界

- 原生分片（无 Mod VM）不支持切换；watch（文件监视自动触发）属 G24 后续；包内容级迁移、Mod 对象/字段转归、RNG 迁移联动属 G23 后续。
- 切换丢弃 perf 样本与 kkpv 公开束（后者必须——块头嵌 plan_hash）；故障注入钩子随旧分片丢弃（测试钩子非意愿态）。
- sim 报告顶层 `mods` 元数据始终记录初始装载输入；切换后实际身份以 `composition`/`execution_strategy` 为准。
- kk-world 代际哨兵在 generation 恰为 `u64::MAX - 1` 的检查点恢复并插入时会误判为当前代际（需 ~2^64 次 bump 才可达，哨兵方案固有边界，先在）。

## 复审记录（三路只读子代理，2026-10-06）

复审对象为工作区未提交改动；发现与清偿如下，全部已在本批内修复：

- **P1（R1）**：`last_console` 在仍有可失败步骤（注入 bail/retire/摘要）之前被 `take`——失败路径把 console 历史搬进随后报废的候选分片静默丢失，违反替代分片自述。**清偿**：console 转移后置到 retire/trace 之后（clone），摘要与增删计算前移到注入点之前；补带 console 输出的注入失败回归（注意 `last_console` 语义是"最近一次完成结算"，断言须避开被无输出轮覆盖）。
- **P1（R3）**：docs/architecture/README.md 目录索引漏登 mod-switching.md。**清偿**：补行。
- **P1（R3）**：登记提交漏同步台账 G24 表行（仍"未开始"）。**清偿**：改为"部分完成｜G24.a 进行中"。
- **P2（R1）**：破坏性 retire 之后仍有可失败读取（策略摘要 `?`）。**清偿**：摘要前移到 retire 之前。
- **P2（R1）**：成功路径静默丢弃 perf 样本与已装测试钩子未文档化。**清偿**：模块文档补"与冷恢复同口径丢弃"段；已知边界登记。
- **P2（R1）**：停用投影只测"移除丢弃"方向；故障过滤只测两域。**清偿**：补"仍安装包停用跨切换存活""投影闭包增长拒绝"两个测试；last_auto_disable 注入断言补齐三域。
- **P2（R1）**：模块文档"仅允许包集合增减"比实际门窄（宿主选项可调）。**清偿**：口径改为"包集合增减与宿主选项调整"，语义表补一行。
- **P2（R2）**：被拒切换的检查点无法续跑——按 `world.time >= after_tick` 推导"切换后身份"对 rejected 不成立。**清偿**：实际生效的切换（`applied_switches`）保存进 sim 检查点元数据（serde default 兼容），推导改为"越过边界且该边界切换曾生效"；补 rejected+save+resume 闭环测试。
- **P2（R2）**：检查点双重读取（探针 + resume）。**清偿**：载荷只读一次，resume 复用同一 Value。
- **P2（R2）**：测试用固定临时路径可被陈旧报告掩蔽。**清偿**：Case（pid+序号私有目录）+ 读失败带 stderr 的兜底。
- **P2（R2）**：边界前保存续跑与 sim 层 rejected 无测试。**清偿**：冷恢复测试补边界前（1 轮）保存分支；新增 rejected 场景（scenario-reject.json）与断言。
- **P2（R2）**：remove 场景 `gte 5` 容忍丢一整轮。**清偿**：精确值 eq 6；Rust 侧补 added==[] 与切换后 composition 断言。
- **P2（R3）**：mod-content-selection.md 边界句过时（运行期改选已交付）且入口表缺 sim 切换面。**清偿**：改口径、补第四入口与交叉引用。
- **P2（R3）**：local-simulation.md 对 mods_switches 记录形状与拒绝原因表述偏松。**清偿**：applied/rejected 分开描述，补装载失败类；补"mods 元数据记录初始装载"口径。
- **P2（R3）**：source-organization.md embed 子模块枚举漏 switch.rs。**清偿**：补。
- **P2（R3）**：笔记"故障三域过滤"验收与实况不符（原只测两域）、"sim 负例已覆盖"不实。**清偿**：随测试补齐后按实况改写验收段。
- **观察（R3）**：续跑报告顶层 mods 元数据为初始集合——已知边界登记并在 local-simulation 补口径说明。
- **P2（R1，先在）**：kk-world 哨兵在 generation `u64::MAX - 1` 的理论退化——已知边界登记，不在本批改。
