# 成熟房性能优化源码复查（2026-10-10）

本页是基于 `26a5092` 的研究与实施建议。**2026-10-10 追注：A/B/C 首批已实现并实测**（同窗比率 P50 23.4×→3.68×、CPU 1.58×、RSS 低 20%，证据见 [50x6-after-opt-abc](../../compat/reports/mature-comparison/2026-10-10/50x6-after-opt-abc/summary.json)，实施记录见[接手台账](../plans/mod-next-batches.md)）；下方“目标与已有证据”起的研究正文为优化前历史证据，保留不追改。xxscreeps 对照固定为 `5ff0d1a682170f6b5f9c8bac3e703767625988e0`。实际进度统一维护在[接手台账](../plans/mod-next-batches.md)，开发者复现入口见[基准说明](../../scripts/benchmark/README.md)。

## 2026-10-10 补充：后续架构改造的 Mod 约束

本节是 A/B/C 首批之后的实施约束，以下旧正文仍为优化前研究。用户再次确认：Mod 须具有与固定 xxscreeps 对照范围相当的灵活扩展能力，性能改造不能收缩目标；当前未开放领域不因此变为已完成。统一约束见[总体架构](../architecture/overall.md#性能改造与-mod-扩展能力)，验证要求见[性能验收](../architecture/performance-acceptance.md#架构优化的-mod-正确性前置)。

后续片保留动态对象/字段/资源和规则的通用消费路径；共享块按权限分组并正确失效；候选优化保持 Mod 写入、链拒绝与整轮失败的恢复语义。流水线与并行须核对真实读写域、意图到齐和处理器依赖，无法证明独立的工作走有序路径。声明与可并行优化的资格不能成为删去目标扩展能力的理由，现有权限也不因优化而放宽。

A/B/C 已有默认 50×6 短窗和部分 Mod/双通道回归证据，不能据此宣称全领域 Mod 性能和灵活性已经验收。原生快速路径与动态通用路径的成本分别核验，后续缓存、候选和调度改造按受影响能力补齐公共 SDK 回归，不另增近期性能场景组。流水线有机会减少等待，但耗时取决于依赖、资源竞争与归并，不能直接以 runner/processor 两相的最大值预测优化后 wall。

### 2026-10-10 补遗：首批实测复盘与后续片设计

本小节由 A/B/C 实施方对上节约束的复核与设计补充；【实测】为 release 探针或 sample profiler 采样，【推断】为据此的归因估算。

**与 Mod 约束的关系**：A/B/C 现行共享条件比上节约束更保守——房内存在任何 Mod 对象或扩展字段即整房回退逐查看者编码（见 `SharedRoomBlock` 构建条件），未做权限分组；约束不与已交付实现冲突，是后续片的收紧方向。

【实测】首批后的剩余归因（50×6、并发 4、release）：
- `run` 相从 35ms 升至约 390ms 不是回退——投影不再掩蔽后暴露的是 V8 执行墙钟（约 8ms/玩家），其中 GC 约占 V8 CPU 三分之一（采样），根因是每拍逐玩家重建记录/wrapper/Game 树与 Memory 全量 parse/stringify（官方语义；xxscreeps 以常驻 isolate 有意偏离）。xxs 同窗 CPU/wall≈6.5× 而名义并发为 4，其内部并行度超过 harness 旋钮；kk 线程-per-player 底座支持更高派发窗口，**建议在 E 片前单做一组 concurrency 8（10 核机）对照**，量化纯调度收益后再决定流水线投入。
- `project` 剩余约 540ms 中，KkpvObject 树的构建+释放约占 190ms/tick【推断：sample 主线程 drop glue 采样折算】——根治是类型化直写编码器（跳过树中间形态）；公开地形束仍每代际整份重编码+全量校验（约 800KB，B.6 残留 1，需放宽块头 tick 校验改绑 terrain_epoch）；信封保持全房目（不可裁剪，见下）。
- `settle` 剩余约 245ms 中：economy/structures/movement 路径的 Generic 二次解析（分类已单次化，但各类型 tick、障碍检查、动作记录仍各自 parse）；意图派发交叉积（每房遍历全部玩家全部对象指令，B 片分桶未做）；候选整房克隆与格索引每拍重建。

【设计】共享条件按声明元数据精化（落实上节"按权限分组"）：viewer 无关性可从冻结计划静态判定——`RoomVisible` 可见性、全 `Public` 字段且无 owner_field 的 Mod 对象，其投影记录对所有查看者是同一份字节；仅含 `Public` 扩展字段的 ext 块同理。含 Mod 内容的房间据此仍可共享，只有真正使用 Owner 裁切/OwnerCollection 语义的内容回退逐查看者。缓存已绑 (generation, plan_hash)，切包失效路径不变。

【设计】Generic 表示演进双轨并存（常态而非过渡）：(a) 类型化装配线——rampart 打样（100/房、无 store、字段最简；一次建好结构 helper、夹具边界归一、投影臂与检查点版本 bump，无存量用户策略使版本升级只拒旧不迁移），link/tower/container 跟进（合计约 117/132≈89% 数量）；lab/factory/terminal 字段最重（动态资源键 store+多冷却钟），按打样后的单类型成本再定去留。(b) sidecar——所有现存写点的统一形状是 parse→mutate→`g.data = v.to_string()`，写点顺手把刚序列化的树存回 sidecar 即得零成本失效（sidecar 与其字符串同生同灭，无陈旧窗口）；读侧命中或单次解析回填；`data` 字段收私有+统一写入口把纪律编译期化。powerCreep/deposit/portal/mod 建筑这类本质动态对象长期留 Generic+sidecar。两轨按"新内建热类型走枚举、动态长尾走 Generic"分流。

【澄清】`literal_gate` 与旧正文"不能用字符串 contains 替代 JSON 类型/归属判别"的关系：B 片的 contains 是**过滤器不是判别器**——命中或含 `\u` 转义时仍走完整 JSON parse 后判定。其可靠性依赖 `contains("\\u")` 兜底：合法 JSON 中 ASCII 字母只能以字面量或 `\uXXXX` 转义出现，藏匿目标键/值（如 `"\u0065ffects"`）必含 `\u`（escaped-key 权威性测试锁定）。拷贝该门漏掉 `\u` 兜底会重新打开藏匿通道。

【教训】KKPV 信封不可裁剪不可见无状态房：玩家面把缺失条目当作界外房（closed）而非 normal——`Game.map.getRoomStatus` 的界外语义与注册 normal 房必须可区分（mod_chain portals 测试锁定）。任何"信封瘦身"须先改玩家面消费端语义，不能只砍生产端。

【实测】2026-10-10 类型化直写编码器片（同夹具同窗 conc 4、release、与上表口径一致）：`project` P50 548.28→334.40ms（-39%）、wall P50 1176.15→1018.41ms（-13.4%）、CPU 3237.91→3285.22 core-ms/tick（+1.5%，噪声内）、RSS 峰值 2729→2620MiB（-109MiB）。实现：`KkpvVal<'a>`/`KkpvRow<'a>`/`KkpvBlockRef<'a>` 借用值模型 + `encode_ref` 单一编码核心（owned 入口经 `KkpvValue::as_val` 桥接委托，字节同源）；世界侧 `encode_room_block` 单房直写（Generic/Mod 解析保活后借用），删除 `kkpv_object_of`/`record_to_kkpv` 的第二套构造器；字段秩映射 + 行主序填列取代逐字段 find_map 的 O(字段数²) 重扫；变长载荷直接追加 arena。字节等价锁：`encode_ref_matches_owned_entry`（合成全值域块）、共享装配 vs 全 fresh 逐房编码、mod_chain 491 双通道。JSON 参考通道未动；未改玩家 API 行为，兼容性棘轮未重跑。

【实测】2026-10-10 并发标定（kk 单端、同夹具 5+50 短窗、release）：

| 并发 | wall P50 (ms) | CPU (core-ms/tick) | 平均并行度 (CPU/wall) | run P50 (ms) |
|---:|---:|---:|---:|---:|
| 4 | 1176.15 | 3237.91 | 2.75× | 382.46 |
| 6 | 1198.00 | 3906.12 | 3.26× | 357.23 |
| 8 | 1150.96 | 4036.05 | 3.51× | 309.18 |
| 10 | 1229.00 | 4293.07 | 3.49× | 343.96 |

并发 8 只换约 2% wall 却多烧 25% CPU，并发 10 直接负收益；瓶颈是串行 project 相，加宽派发窗口只能削 run 尾（382→309ms）。xxs 名义 4 实测平均并行度约 6.5×，kk 并发 4 时 2.75×——用户 2026-10-10 定的公平规则（kk 用核不得超过 xxs 实测）当前满足且余量大；但即使纯收益角度提高并发也不值得。E 片（只读投影流水线）的纯调度收益上限即 run 尾约 300ms，排序在串行削减与 V8 侧复用之后。**正式对照新增公平门：kk 实测平均并行度 ≤ xxs 实测平均并行度，且 CPU core-ms/tick 不劣化。**

【实测】类型化编码器后的新采样归因（主线程 7284 样本/约 7.3s 窗）：`kkpv_bundle_for_viewer` 子树 1741（24%）——其中 `encode_room_block` 约 1100（共享块每拍重建：generation 每拍前进，且本夹具 300 房全有 creep 活动、逐房增量缓存在此场景无节省）、信封 350、`visibility_index` 348（每代际重建一次，可接受）；settle 子树 1809（25%）——其中 room_loop/settle_room 308、tick_with_writebacks 非 room_loop 部约 1107（候选准备/working 格重建/全局相）、写回约 394；等待 V8 工作者 3035（42%）即 run 重叠窗。公开地形束重编码+校验（约 800KB）与意图逐房交叉积均不在热点前列——B.6 残留 1（terrain_epoch 缓存）与意图按房分桶按证据降级为"已注册、待新证据"，不再列为下一片。

【推断】剩余 wall 约 1018ms 的下一步排序：run 约 425ms（V8 执行墙钟，GC 约三分之一）是当前最大单相，V8 侧 wrapper/Game 树复用是下一块大石（约束：Memory 全量 parse/stringify 是官方语义，只优化不改变外部行为）；其次是非 room_loop 的 settle 部分与共享块重建（全活跃房场景的固有成本）。束装配 memcpy 约 75MB/tick ≈ 8ms/tick（10GB/s 量级），不值得为此改传输协议。

【设计】E 片入门形态（配合总体架构的有序/并行约束）：Mod JS 钩子（begin/end_settlement、realm/namespace 状态）与跨房/账户/ID/RNG 归并留在有序全局侧，纯引擎房间候选并行；房间级"意图到齐即可结算"所需的 intentRooms 可由视野索引 room_owners 反查派生（A 片索引的第二个消费者）。验收以默认/Mod 串行参考等价为准。

## 目标与已有证据

只使用 50 玩家×6 个 RCL8 满功能成熟房的共同负载，总计 300 房；允许有限玩法结果差异，但必须披露状态与动作差异，并通过初始夹具、完整执行和实际活动校验。成功条件是同机同窗口下更快且进程 RSS 更低，2GiB 只作参考。

【实测】[最终夹具短窗](../../compat/reports/mature-comparison/2026-10-10/50x6-adjacent/summary.json)为 1 轮、预热 5 tick、采集 50 tick、并发 4；两端三个观察时点与动作计数全部一致。macOS、24GiB、release、Node 24.11.0，采集范围为生产沙箱、结算和内存提交，不含 HTTP 与磁盘持久化。

| 指标 | kkscreeps | xxscreeps | 当前含义 |
|---|---:|---:|---|
| wall P50 | 6198.34ms | 264.70ms | kk 耗时约 23.42 倍 |
| wall P95 | 6425.30ms | 399.16ms | 尾部也明显更慢 |
| CPU / tick | 9302.06 core-ms | 1730.57 core-ms | 需要减少实际计算，并发本身不能解决 |
| sampled RSS peak | 3017.17MiB | 4348.03MiB | 本窗口 kk 约低 30.6% |

这是开发诊断，**不证明长窗稳态内存领先或正式验收成功**。正式窗口要求至少 5 轮、500 tick 预热、3000 tick 采集；当前没有这组结果。原始报告、夹具、日志与哈希见[证据清单](../../compat/reports/mature-comparison/2026-10-10/manifest.json)。

【实测】kk 原始报告中，project 总计 277856.36ms，run 总计 1753.55ms，settle 总计 30338.65ms，均覆盖同一 50 拍。相应均值约为 5557.13 / 35.07 / 606.77ms；project 占三相总和约 89.6%。不能相加各相 P50 来计算某个实际 tick。

【源码】[运行协调器](../../crates/kk-runtime/src/embed/execution.rs)把宿主公开束、逐玩家投影和私有束编码计入 project；`run_ns = elapsed - project_ns` 是执行窗口剩余墙钟，并且玩家执行可与后续玩家投影重叠，**run 35ms 不等于全部玩家 JS 的总 CPU**。settle 包括意图合并、写回候选、房间和全局结算、校验与发布。V8 注入/解码发生在工作者，不能据 project 耗时推断 V8 解码占比。

【推断】即使投影完全消失，旧窗口约 607ms 的串行结算仍是明显瓶颈；不能承诺一次视野索引优化就能击败约 265ms 的 xxs P50。改变投影后重叠、GC 与调度都会变化，剩余均值约 642ms 只用于排序优化工作，不是优化后耗时预测。

## 源码揭示的重复工作

### 1. 全服视野和 observer 扫描按玩家重复

[视野判定](../../crates/kk-world/src/projection/visibility.rs)遍历房间对象寻找己方非旗、非工地对象；[对象归属](../../crates/kk-world/src/domain/owner.rs)对 Generic 每次完整解析 `data` JSON。无己方对象的房间必须走到末尾。

[KKPV 信封与私有块](../../crates/kk-world/src/kkpv/mod.rs)分别调用视野判定，并分别调用[observer 查询](../../crates/kk-world/src/world/mod.rs)。observer 查询也遍历全服对象；私有块随后再遍历全部房间的对象，即使房间不可见。

【源码计数推断，非 profiler 计数】共同夹具初始每房 535 个对象（534 个显式对象及控制器），其中 132 个 Generic：100 rampart、6 link、6 tower、10 lab、5 container、terminal/factory/powerSpawn/observer/nuker 各 1；放置变体依据见[归一入口](../../crates/kk-world/src/place/place_structure.rs)。每玩家有 294 个无己方对象的房间：

- 两次视野判定的外国房对象访问：`2 × 50 × 294 × 535 = 15729000`；其中 Generic 归属读取完整 JSON 解析：`2 × 50 × 294 × 132 = 3880800`。
- 两次 observer 查询的全服访问：`2 × 50 × 300 × 535 = 16050000`。
- 私有块生成的全服对象遍历：`50 × 300 × 535 = 8025000`。

上述三处初始布局就约 3980 万次对象访问/轮；对象变化后计数改变，也不能据此断言它们各自占用了多少毫秒。这说明算法重复的规模，不是收益倍率。

建议同一已提交世界代际只构建一次 `owner → 可见房`、observer 限时视野和全局归属集合索引，JSON 与 KKPV 共用；不可见房只枚举己方旗/工地与 Mod OwnerCollection，而不是遍历该房全部对象。第一片保留每注册房一个私有块的协议，先省对象扫描，不顺手改变空块语义。

缓存必须绑定世界实例、generation；依赖 Mod 目录的集合还须绑定有效玩家面/计划身份。创建、删除、移动、改归属、observer 标记清零、管理编辑、候选发布、restore、clone 和包切换都要核对失效路径。不能以“控制器属于谁”替代视野：外矿己方 creep、多方对象以及 observer 都需要保留；Mod 对象目前不授予房间视野，OwnerCollection 可在无视野房导出。

### 2. 房间结算重复遍历全服对象指令

[房间循环](../../crates/kk-engine/src/settle/room_loop.rs)对每一房遍历 `batch.users`，再遍历每名玩家所有对象动作，最后在本房对象表查 ID。300 房下，大量对象指令在另外 299 房查空；复杂度含 `房间数 × 全服对象指令数`。[NPC 合并](../../crates/kk-engine/src/settle/npc.rs)已用 Cow 避免无 NPC 时复制整个 batch，不能把这里误写为每房深拷贝 batch。

建议准备阶段按已提交对象目录把对象动作与 ext 指令分桶为借用的房间视图；房间指令按其目标房分派，市场/power/通知等全局指令仍走原阶段。未知 ID、跨玩家伪造、动作顺序、NPC 显式覆盖、同轮新对象和跨房过渡须保持原行为。公共 `settle_room` 入口及 Mod 消费者要一并核对，不能只改成熟夹具入口。保留玩家名字、对象 ID、动作族的既有排序。

xxscreeps 的对应参考是固定源码 `packages/xxscreeps/engine/processor/model.ts::publishRunnerIntentsForRooms` 的逐房载荷及 `processor/room.ts` 的房间处理。其 `runner/instance.ts::run` 只加载 `visibleRooms` 对应的 room blobs；不能因 kk 使用 Rust 就假定总工作量已经优于这套组织方式。

### 3. Generic 类型判别重复解析

[通用读取](../../crates/kk-world/src/domain/generic.rs)的 `type_name`、`structure_type` 都解析完整 JSON。[对象 tick 分派](../../crates/kk-engine/src/settle/room_loop.rs)依次判 nuke、powerCreep、deposit，最后读取 structureType；普通 Generic 建筑仅为分类就解析四次。在初始布局 39600 个 Generic 上，这一分支约 158400 次完整解析/轮，还不含具体处理器、投影和读写库存。

第一片可以把单次分类改为一次解析后读取多个键，不改变存储格式。后续评估对象 revision 绑定的紧凑类型/归属元数据，以及候选期间一次解析、按需写回的字段视图。不能长期并存全量 JSON 字符串与每对象全量 Value 缓存而不测常驻内存；也不能用字符串 `contains` 替代 JSON 类型/归属判别。所有直接修改 data 的入口都必须参与失效或改为统一写入接口。

### 4. KKPV 仍经过临时 JSON 记录

[KKPV](../../crates/kk-world/src/kkpv/mod.rs)仍走 `object_json → record_to_kkpv → encode_bundle`。Typed 对象先构造 Value，再转换为拥有字符串/集合的 KkpvObject，最后编码；Generic 也先解析再转换。

建议在索引消除全服重复扫描后，测实际可见对象的转换与编码成本，再实现直接写 KKPV 的类型化路径或共用借用记录访问器。JSON 保持参考通道；Mod 动态字段、ext 权限、引用类型、时间派生字段和 controller 归属三态保持一致，不能复制两套逐渐漂移的权限实现。共视野多玩家可共享经过权限分组的记录，但不能直接共享包含 owner 私有字段的房间块。

## 优化顺序与验证出口

以下是实施队列，所有收益均待测量；先减少重复工作和分配，再引入并发。

| 顺序 | 工作 | 收益依据 | 成本与主要风险 | 片尾验证 |
|---|---|---|---|---|
| A | 增投影/结算子阶段诊断；共享代际视野与全局集合索引 | 最大实测阶段；数百万重复 JSON 解析 | 中；视野泄漏、缓存陈旧 | 权限矩阵、JSON/KKPV 同源、编辑/发布/恢复；同组 50×6 短窗 |
| B | 按房分桶指令；单次 Generic 分类；world_span 每轮求一次 | 全服指令逐房查空、分类四次解析；当前 world_span 每房重算 | 中；指令顺序、NPC 合并、未知对象和全局意图 | 旧串行参考与新路径完整候选对照、失败重试及跨房回归 |
| C | 直接 KKPV 编码、紧凑派生元数据和受控候选字段缓存 | 临时 Value/字符串和重复解析仍在 | 中高；动态字段、权限和缓存内存 | 全类型/扩展/Mod 双通道、冷恢复；CPU/分配/RSS 同测 |
| D | 房间候选共享不可变对象，惰性复制被写对象；复用空间索引存储 | 每拍克隆全房对象及 Generic 字符串；反复建立格索引 | 高；原子失败、引用别名、索引顺序和维护成本 | 多房晚期故障全量不变、链拒绝、创建/删除/跨房、检查点等价 |
| E | 只读投影流水线、房间并行候选与有序归并 | 投影主线程串行；房间结算目前串行 | 高；共享账户、ID/RNG、Mod namespace 与宿主线程身份 | 默认/Mod 串行参考等价、故障和确定性；CPU/峰值不退化 |
| F | 静态地形载荷跨 tick 复用、懒解码、受控 V8/分配器调优 | 尚有重新编码与旧 backing store 存活成本 | 中高；tick/generation 身份校验、GC 延迟 | 长窗真实存活/retention/RSS 曲线；跨平台与恢复 |

【源码】[候选准备与提交](../../crates/kk-engine/src/settle/pipeline/engine_tick.rs)每房克隆一次 RoomState，已不是整个 World 每轮 clone；world_span 确实在每房 Plane 装配时遍历房名求值。[Working](../../crates/kk-engine/src/settle/working.rs)重建[格索引](../../crates/kk-world/src/index.rs)，每房初始化 2500 个 Vec 槽。不能把它称为每拍 75 万次独立堆分配：空 Vec 本身不分配元素堆，外层存储与非空格才分配。优先尝试保序的紧凑格存储/复用，验证后再考虑侵入性更强的对象表布局。

【约束】并行房间不是直接给串行循环套线程池。[TickState](../../crates/kk-engine/src/settle/pipeline/txn.rs)为 `Rc<RefCell>`，房间共享玩家/Mod 状态与 ID 游标；JS 处理器存在同步调用和链回退。必须先区分可独立的房间候选与有序全局增量，定义 ID/RNG、跨房与账户冲突归并，保持任一失败不发布。只信 parallelSafe 声明不足以证明安全。300 个成熟房都有 creep 寿命、控制器、资源和冷却活动，房间休眠不是本场景的主要路径，不能省略活动 tick 来制造领先。

## 内存与其他手段的取舍

当前 RSS 较低只是短窗观测。优化工作同时记录 heap 使用、Rust 活对象/分配、共享束存活字节、各采样阶段 RSS 与峰值；这几项口径不能互相替代。采集器和共同夹具驻留也在进程 RSS 内，优化其保留必须双端一致并重新归档。

优先减少解析和临时对象分配；只缓存共享、有限、可失效的数据，避免缓存按玩家复制全服对象。每次并发/缓存改造检查峰值，不能以速度换取比 xxs 更高内存。

【已实现】公开 KKPV 载荷在同代际跨玩家共享，[SharedKkpvBundle](../../crates/kk-runner/src/shared_bundle.rs)接管字节并共享 backing store，不是每玩家复制地形束。[宿主公开束](../../crates/kk-runtime/src/embed/projection.rs)仍因块头 tick/generation 约束每轮重新编码；可研究静态 body 与轮身份分离，但这是协议改造，须考虑安全与版本，不应列作无风险小修。

V8 backing store 的真实存活可能由玩家沙箱 GC 延长；现有 public retention 报告不包含全部 V8 引用，只是存活下界。没有存活/分配证据前不能把 RSS 高或增长直接认定为泄漏，也不能把强制 GC 带来的低 RSS 当作免费收益。

isolate LRU 适合不活跃玩家，但此场景 50 玩家每拍执行，驱逐重建可能增加成本。64MiB 堆上限已经配置，继续压低限额不能替代减少实际分配，也会改变可运行玩家程序范围。寻路、SIMD、PGO/LTO、替换分配器、哈希表/紧凑数组、字符串驻留均保留为后续手段：先有子阶段与分配证据，再选择；改对象表时必须保留确定性遍历语义，不能以 HashMap 的无序迭代改变结算。

真实服务尚有持久化、订阅和网络开销。本次优化先证明相同引擎负载领先；未来服务端 Linux 对照必须另外证明生产进程内存和吞吐，不能从 macOS 内存基准直接宣布上线目标完成。

## 实施与验收方式

1. 首片从 A 开始，诊断默认关闭，记录 visibility/observer/记录转换/编码，以及 prepare/房间复制与索引/派发与对象 tick/全局相/validate/publish。诊断窗口与正式窗口分开；阶段可能嵌套或重叠时注明边界，避免重复相加。
2. A 与 B 各自保存同机旧版/新版 release 产物、源码/夹具哈希和前后报告；重复采集相同 50×6 短窗，交替次序，核对完整执行、活动、字段与动作差异。沿用同一玩家脚本，不减少建筑、角色、寻路或补员。
3. 索引和投影优化验证无主/多主房、外矿 creep、他人旗、无视野己方旗/工地、observer 一拍有效/未注册房、Mod 两类对象可见性、Owner/ServerOnly 字段、删除/改归属/跨房/恢复/同 tick 编辑。行为本应不变，不能用用户允许玩法差异掩盖优化引入的错误。
4. 结算改造保留同输入旧串行参考、完整候选/Memory/CPU/账户/ID/Mod 状态核验，多房及晚期故障不发布、缓存重试不重跑玩家、冷恢复与确定性回放。涉及引擎或玩家行为时执行兼容性棘轮；每提交运行无参数完整门禁。
5. 当短窗速度已接近/超过 xxs 且峰值不退化，再做至少 5×(500+3000) 正式窗口。报告每轮 P50/P95/P99、CPU、RSS 曲线与峰值、差异和范围；P50 与峰值 RSS 两个观测目标都领先才记为成功，尾部/CPU 明显退化须解释，噪声范围内差距应增加重复而不是宣称胜出。

不能给出可信的达标日期：目前只有阶段测量和源码证据，尚无 A/B 优化落点。可确定的下一交付是 A，再由同负载前后结果决定 B/C/D 的投入；若投影降下来后结算仍主导，就继续优化结算，而不是把第一片当成目标完成。

本次复查没有新增引擎行为、SDK 或保存状态域，没有运行新的性能窗口；原始证据保持不变。提交与验证记录见接手台账。
