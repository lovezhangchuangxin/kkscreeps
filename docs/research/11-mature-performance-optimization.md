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

【方向】2026-10-10 用户放宽等价标准：不要求内部行为与官方逐项一致，只要求**玩家脚本可观察面无差异**——可判定基准是兼容性棘轮（screeps-ok 2914 例 + tooangel/overmind/wasm 三夹具）与 mod_chain 双通道。这解锁了 V8 侧常驻化路线（xxscreeps 同路）：记录/wrapper/Game 树跨拍复用、Memory 干净拍跳过 parse。已知边界情况须过棘轮验证：(1) 对象同一性——官方每拍重建使 `global.savedRef === Game.creeps.foo` 跨拍为 false，常驻后为 true（依赖此行为的 bot 在官方即坏，实际不存在）；(2) Memory 存非 JSON 值的归一化——每拍末照常 stringify、下一拍字符串未变则跳过 parse 复用常驻对象，归一化语义与官方一致。

【实测】2026-10-10 V8 侧第一双片（等价标准放宽后，用户拍板基准=玩家脚本无感知，见上文【方向】）：

(a) **Memory 常驻 + 洁度走查**：直接照搬 xxscreeps 的 previousJson+crunch 会被自家套件打回——首版实现 5 例转红（UNDOC-MEMJSON-001/003/004/005 + MEMHACK-011：函数/NaN/Infinity 跨拍存活、环引用后运行时不存活、删除 `_parsed` 的丢弃语义泄漏）。按无感知标准自研 `memoryCleanWalk`：一遍 O(n) 无分配深走，就地归一化（数组 undefined 槽转 null、对象 undefined 键删除）并判定对象是否与 `parse(stringify(obj))` 逐值一致——函数/Symbol/bigint、非有限数/-0、非普通原型（Date/Map/类实例）、环、不可枚举自有键、Symbol 键、数组命名键与稀疏洞一律判脏；干净拍建立 (对象, 字符串) 常驻对、下一拍注入串未变即跳过 parse，脏拍或序列化失败或 `_parsed` 缺失（未访问/被删）一律作废常驻走官方 parse。宿主注入不同串（重试/恢复/编辑）失效常驻。棘轮 2905 全绿复验。热路径节省为 8KiB×50 玩家的 parse（数十 ms/tick 量级），非本片主收益。
(b) **api_probe 门控（本批主收益）**：`probe()` 给每个游戏对象包两层 Proxy（pos + 本体）、console 一层，玩家全部字段访问过 trap 且 `in`/Set 查询——内联缓存全灭。穿通对照实测：run 414→307ms（-26%）、CPU -19%。正式实现 `SandboxConfig.api_probe`（默认 true 保 bot-check/适配器诊断），compare-probe 与容量探针显式关闭（与 xxs 生产沙箱同形：诊断不进计时路径）。最终单轮 wall 855ms、run 279ms、CPU 2429 core-ms/tick；两端对照窗（[50x6-after-v8-residency](../../compat/reports/mature-comparison/2026-10-10/50x6-after-v8-residency/summary.json)）kk/xxs P50 835.7/259.5ms——本窗 xxs 比上窗快 24%，跨窗比率不可比，本窗口径 **3.22×**（旅程：23.4×→3.68×→3.17×→3.22×，各为其窗口内数字）；CPU 1.41×、RSS 峰值 0.72×（低 28%）、并行度 2.88× vs 6.57×（公平门保持）。
(c) **教训**：诊断仪表必须可关且不进计时/生产路径；「官方/xxs 怎么做」只是参照——xxs 的 crunch 宽松度在我们的 UNDOC 套件面前不合格，最终形态是按自家验收标准重设计的洁度走查。

【实测】2026-10-10 记录常驻（V8 第二子片）：重采样确认 GC（标记/清除）是 V8 最大采样桶、JSON 仅 99 样本（Memory 片量级判断被证实）。实现：变长字段共享访问器改为**按字段索引**工作（字段元数据含列偏移随块布局逐 tick 变化，改从 DECODE_CTX 按索引取，访问器终身有效）；`decodeBundleResident` 以沙箱生命周期存活的 id→记录 Map 就地更新——布局签名（字段键+列类型有序串，每桶一次）一致才复用，变化即重建；目录外 id 清扫。mod_chain 491 全过。实测 run 279→265ms、CPU 2429→2242（-7.7%）、RSS 峰值一轮 -194MiB（峰值采样噪声大，CPU/run 为稳定信号）。记录对象跨 tick 同一性（已接受类）。**坑**：脚本化替换静默未命中曾使索引制访问器配上缺 `fields` 的旧 ctx（变量字段首读即炸，265 例转红）——大段替换后必须核对锚点真的命中再构建。

【设计】wrapper 常驻（记录常驻已落地，本段为剩余半片）：字段 getter 全部经 `gw.rawById` 按 id 回查、`_name` 真值记忆在实例内——因此 `wrappersById` 常驻：id 存续且类型不变则复用 wrapper（省 RoomPosition/probe/装配/原型链接），`_` 前缀记忆属性按 wrapper **懒失效**（首拍访问时清，未触达的 wrapper 免清扫）；Room/Game 树仍每拍重建（廉价纯对象）。观察面边界：对象同一性跨拍保持（已接受）；主要风险是记忆失效遗漏导致陈旧字段值——由 mod_chain、双通道状态等价与棘轮锁定。probe 门控已消掉最大单项后，此片预期收益需重新采样评估再动工。（2026-10-10 已落地为 v3 并 ABBA 保留，v1/v2 的失败教训见下方补记。）

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

## 2026-10-10 补记：主要优化优先级

本节是复审修复后的诊断与下一步计划，尚未实施以下主要改造；上方按旧窗口提出的 wrapper 优先建议降级。用户要求先做主要瓶颈，避免连续投入没有稳定效果的小项。

固定 50 玩家×6 成熟房、并发 4、预热 5/采集 50 tick、单轮相邻对照，kk/xxs wall P50 为 790.06/259.51ms（3.04×），CPU 为 2057.72/1713.44 core-ms/tick（1.20×），采样 RSS 峰值为 2.33/3.36GiB（0.69×）。状态与动作计数一致；这是诊断窗口，未达到正式验收。[基线摘要与阶段统计](../../compat/reports/mature-comparison/2026-10-10/priority-reassessment/baseline.json)保留复现命令、依赖/源码哈希；完整世界快照仍在报告内标出的本机临时目录，未归档。

同一窗口的 project/run/settle P50 为 314.36/239.06/232.55ms；按三相总耗时计算，project+settle 占约 68.7%。这是协调器计时分账，玩家执行与投影已有部分重叠，不能解释为完全串行的 CPU 占比，更不能直接预测流水线收益。当前差距同时包含实际计算偏多和并行利用不足。

已尝试并撤回的小项：look 空间索引懒装配、进一步懒装配 look 注册表、当前 Room.find 使用本房候选并延后包装查表。三窗 kk P50 分别为 794.12/793.13/819.85ms，没有稳定 wall/CPU 改善，第二项还增加峰值 RSS；不保留实现或专用测试。[look 摘要](../../compat/reports/mature-comparison/2026-10-10/priority-reassessment/lazy-look.json)、[look 注册摘要](../../compat/reports/mature-comparison/2026-10-10/priority-reassessment/lazy-look-register.json)、[find 摘要](../../compat/reports/mature-comparison/2026-10-10/priority-reassessment/room-find.json)用于说明降级依据。短窗存在噪声，结论是没有足够证据继续投入，不是证明所有查询优化永远无效。

| 优先级 | 主要方向 | 选择理由与实施前置 |
|---|---|---|
| P1 | 世界热数据表示与候选复制机制 | 同时影响 project、settle 与内存。现行 Generic 仍以 JSON 字符串保存，多个消费者解析；每房候选深克隆对象与扩展域，Working 重建空间索引。先在实际 tick 内分清编码、解析、复制和索引的成本，再选择主要切片，避免按对象数量推断耗时。类型化热建筑与不可变基底/写入候选分别评估，不一次混改。 |
| P2 | 依赖安全的房间结算并行与执行/结算流水线 | 当前房间结算有序串行，CPU/wall 利用率明显低于对照；有机会缩短关键路径，但共享玩家/ID/RNG、跨房结果与 Mod VM 依赖须先明确。先分离房间局部产物与全局归并，再评估调度，不能直接加线程或将两相取 max 估算。 |
| P3 | 玩家包装树复用与 GC 分配成本 | 既有采样证明它曾是热点，但 probe 门控与记录常驻已消去部分成本；需要修复后实际执行窗口的新采样证明剩余收益，之后才实施。跨拍引用、玩家属性修改与内部记录隔离仍须验证。 |
| 暂缓 | Room 查询细节、公开地形缓存、意图分桶、束 memcpy | 本轮查询试验无稳定收益，其他项在上一轮采样不占主要成本；没有新证据前不投入。 |

下一实施入口是 P1 的成本核验与方案选择。Generic 的 rampart/link/tower/container 类型化可覆盖本夹具大部分 Generic 数量，但数量不是收益证明；不可长期双存全量 JSON 字符串与 Value 树而不测 RSS。候选机制需覆盖所有 Mod 写入域、失败重试与保存恢复。两类改造均保留动态对象/字段/资源和规则的通用路径；调度无法证明独立的 Mod 工作继续走有序路径，不降低扩展能力。

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


## 2026-10-10 补记：Generic 不可变载荷与主要成本复测

【现行实现】按上一节优先级继续核验 P1，实际 tick 采样首先证明 Generic 多消费者重复解析值得处理；本轮采用 [ADR-019](../decisions/ADR-019-generic-payload-metadata.md)，未叠加查询、地形、意图分桶小优化，也未引入房间并行。

### 采样与选择

旧版 release 在同一 50×6 夹具上作 5+120 tick 单端诊断，启动约 16 秒后采样 10 秒，覆盖实际 tick。主线程 tick 栈 7530 个样本中，共享房间编码子树 2197、结算子树 2130、房间克隆 66；投影 Generic/视野解析子树分别 231/237，结算 NPC 分类、observer 配额、对象分派及衰减亦有完整 JSON 解析。以上为嵌套 inclusive 栈，不能相加成独立百分比；完整采样及命令见[采样说明](../../compat/reports/mature-comparison/2026-10-10/50x6-generic-metadata-profile/notes.json)。延长诊断沿用 55 tick 完成标记夹具，不拿该诊断报告宣称双端工作量等价。

据此先减少跨消费者重复解析，而非直接大改候选写时复制。`GenericData` 把原始 JSON 与惰性紧凑身份/衰减元数据放在共享不可变载荷内；候选克隆不复制文本，所有写点整体替换载荷，自然使新读取使用新缓存。元数据只保存少数字段，首次完整解析后释放 Value 树；操作动态字段或投影完整对象仍解析原文。已接入归属、视野/observer、NPC 分类、配额、类型分派、移动类型和衰减读取门。

二进制编码与原 String 逐字节相同，缓存不保存；Mod 动态 JSON、未知字段、扩展权限与 SDK 保留原路径。新增缓存边界/字节锁、候选失败撤销/成功提交/旧世界隔离/检查点恢复，以及归属与衰减时钟变更回归。完整无参数 `pnpm run check` 已通过，含 mod_chain 491；最终 release 与兼容性结果见接手台账。

### 同负载短窗结果与限制

全部采用 50 玩家×6 成熟房、并发 4、预热 5/采集 50；前后初始/窗口前/窗口后公开状态及动作计数全部一致。新版本首次两轮交替双端顺序，之后补相邻旧/新各一轮。原始报告与来源哈希已经入库，没有筛掉不利窗口。

| 窗口 | kk wall P50 / P95 / P99（ms） | kk CPU（core-ms/tick） | kk / xxs 峰值 RSS（GiB） | kk / xxs wall P50 倍率 |
|---|---|---|---|---|
| [旧版首轮](../../compat/reports/mature-comparison/2026-10-10/50x6-generic-metadata-before/summary.json) | 808.32 / 987.56 / 1050.34 | 2127.36 | 2.75 / 3.62 | 3.09 |
| [新版两轮聚合](../../compat/reports/mature-comparison/2026-10-10/50x6-generic-metadata-after/summary.json) | 750.84 / 926.33 / 973.84 | 2255.72 | 2.66 / 3.62 | 2.17 |
| [旧版相邻补测](../../compat/reports/mature-comparison/2026-10-10/50x6-generic-metadata-before-repeat/summary.json) | 783.66 / 947.00 / 1001.89 | 2040.13 | 2.35 / 4.01 | 3.10 |
| [新版相邻补测](../../compat/reports/mature-comparison/2026-10-10/50x6-generic-metadata-after-repeat/summary.json) | 789.89 / 1093.66 / 1388.72 | 2236.18 | 2.39 / 3.52 | 2.77 |

结算 P50 从旧版 232.84/231.53ms 降至新版 95.95/142.61/110.35ms，各新版窗口均显著降低；相邻补测约减 52%。但相邻投影 310.99→348.61ms、玩家执行 238.51→320.85ms，抵消结算收益：wall P50 +0.8%、总 CPU +9.6%，尾部变差。xxscreeps 同时 wall +12.7%、CPU +7.7%，提示环境/窗口变化，但不能据此把 kk 的不利结果全部归为噪声。仅可确认结算阶段成本减少，**不能确认端到端或 CPU 的稳定收益**，公平 CPU 门尚未达成。首轮与两轮聚合也不能直接证明速度提升。

新版三轮峰值 RSS 均低于各自同窗 xxscreeps；相邻补测低约 32%，但相对旧版峰值约增 2%、采样中位数约减 8%，短窗峰值波动明显，不能认定本片稳定降低峰值。新版仍慢于 xxscreeps，正式验收与目标达成都保持 false，不启动 5×(500+3000) 正式窗口。

旧/新二进制 SHA-256 分别为 `a8e68dbcbe4425b2223cb5cf508a5b006a1df43d75b3e70aca147d51705d123e` / `45c6961fbd43ceb1f38915680956d9fd43c1b1bdb7bd44ce138073dda0139bcb`。相邻旧版使用精确重建的旧产物；未引用的新 `generic_data.rs` 当时保留在磁盘，来源清单会收录该文件，但基线模块声明已撤回、未编入二进制。恢复改造后使用冻结新版复测，最后正常重建 release 并核对哈希完全相同。

### 下一主要优先级

P1 转向共享房间投影的记录装配、完整 Generic 解析及 KKPV 变长字段编码/临时分配：新版投影仍约 291–359ms，旧版实际编码栈占比高，下一片先在新版实际 tick 采样核对。设计应先减少每 tick 构造/释放的中间树与元数据，保持未知动态字段、权限裁切和时间派生字段同源；不缓存第二份完整 Value 树来赌内存。验收同时关注 project、run/GC、总 CPU、尾部和 RSS，不能只看优化函数自身。

P2 为依赖安全的投影流水线/房间候选并行；全量候选写时复制因当前克隆采样较小暂后排，待新热点证据再推进。wrapper 常驻属于后续待采样方向，查询小优化/地形缓存/意图分桶仍暂缓。本片保留为已验证的结算减负及不可变载荷基础，端到端收益问题继续追踪，不能把它记作性能目标完成。


## 2026-10-10 补记：端到端 ABBA 核验与保留决定

【后续验证】上一节 5+50 短窗只证明结算减负，用户指出不能以局部阶段结果代替整体收益。因此先暂停叠加优化，冻结相同旧/新 release，使用同一 50×6 成熟房夹具、玩家脚本和并发 4，按旧→新→新→旧独立启动，预热 30/采集 100。运行期间没有构建、跑其他测试/基准或改实现。只比较 kkscreeps 两个版本，本实验没有新的 xxscreeps 样本；不是正式窗口。

[完整报告](../../compat/reports/mature-comparison/2026-10-10/50x6-generic-metadata-abba/summary.json)及同目录压缩夹具、四轮原始报告、回执、来源清单、实验脚本和精确实现 patch 已入库。四轮均通过现有成熟房校验；初始/窗口前/窗口后世界字段及全部玩家动作回执逐值一致，没有以省略工作量提速。旧/新二进制哈希与上一节相同，运行前后 Rust 实现 patch 逐字节一致。

| 顺序 | 版本 | 整体 P50 / P95 / P99（ms） | CPU（core-ms/tick） | RSS 中位 / 峰值（GiB） | 结算 P50（ms） |
|---|---|---|---|---|---|
| 1 | 旧 | 789.98 / 976.74 / 1007.75 | 1973.14 | 2.43 / 2.52 | 234.68 |
| 2 | 新 | 653.52 / 902.39 / 1001.91 | 1908.06 | 2.03 / 2.65 | 98.87 |
| 3 | 新 | 771.48 / 970.95 / 1021.08 | 2137.82 | 2.47 / 2.51 | 134.63 |
| 4 | 旧 | 929.82 / 1160.22 / 1211.49 | 2283.92 | 2.69 / 2.74 | 289.89 |

两对相邻样本的方向一致：1→2 新版 wall P50 -17.27%、CPU -3.30%；4 与 3 反向相邻比较，新版 wall P50 -17.03%、CPU -6.40%。按各版本两轮统计量的算术平均（**不是合并 tick 样本重新计算的百分位**），wall P50 859.90→712.50ms（-17.14%），P95/P99 -12.34%/-8.84%，总 CPU 2128.53→2022.94（-4.96%）。project 均值 331.92→320.66ms、run 263.81→270.77ms、settle 262.29→116.75ms；阶段 P50 不可相加当作 wall P50。

RSS 采样中位数均值下降 12.09%，峰值均值仅下降 1.80%；首对新版峰值反增 5.34%，第二对下降 8.37%，没有足够依据认定稳定峰值收益。仍只有每版本两轮短窗，轮间 wall/CPU 明显波动，不能把均值降幅当作正式置信区间，也不据此宣布稳态内存或 xxscreeps 目标达成。先前 5+50 的不利窗口保留；它们与本次差异的原因尚未隔离，不直接归因于热状态、GC 或机器负载。

**决定：保留本片。** 理由是本次交替顺序对照的两对样本均有端到端约 17% 的 wall 收益、CPU 同向下降，而非仅仅结算阶段变快。结论限于同机相同负载的短窗；今后改造继续同时检查整体 wall、CPU、尾部与 RSS，若长窗显示收益消失或内存不利，应重新评估。没有新增实现或 SDK/保存协议变化，不为验证叠加其他优化。

最终 release SHA-256 仍为 `45c6961fbd43ceb1f38915680956d9fd43c1b1bdb7bd44ce138073dda0139bcb`，本轮 Rust 实现与上一轮完整门禁及 2905/9/0 棘轮验证的源码逐字节一致；本轮只新增实验记录和文档检查，不把先前检查冒充本轮复跑。下一主要优先级仍是共享房间投影装配、编码和临时分配，安全流水线/并行其次；保持 Mod 动态路径与候选原子性约束。


## 2026-10-10 补记：投影流水线试验与撤回（懒共享块 + 执行窗口预取）

【已撤回试验】采样证明主线程投影（`kkpv_bundle_for_viewer`，44%）中 ~290ms 是首个玩家触发 `build_shared_room_blocks` 全服一次性构建、阻塞首派发；run 窗口主线程空等 ~37%。据此实施两半：共享块按房懒构建（`RoomBlockCacheEntry` 按房条目，(generation, plan) 失效）+ 派发循环投影预取（槽位满后、等待前预建至并发深度，回执即派发）。mod_chain 491 绿后冻结二进制 ABBA（5+100、并发 4、同夹具）：

两对相邻样本方向均不利——wall P50 +2.3%（878.6→899.1ms 均值）、CPU +2.4%，实现整体撤回。[摘要与四轮原始报告](../../compat/reports/mature-comparison/2026-10-10/50x6-projection-pipeline-withdrawn/summary.json)入库。机理归因：4P+6E 上旧结构的串行投影段独占 P 核与缓存带宽，把主线程编码挪进 4 个 V8 线程的执行窗口制造第 5 个 P 核竞争者，两边同拖慢——重叠在本机不是免费资源。补充测量：预取深度放开到不限（全预取）单窗 870ms 更差（预取连建期间槽位整批空转）；同实现下 conc=6/8 复测 wall 持平/更差且 CPU +17%/+46%、RSS 峰值反超对照，E 核惩罚不因预取消失。结论限于本机拓扑；核数受限的部署机上"总 CPU + 串行段"仍是正确优化面，本试验不改变该判断。

同窗口采样另证：玩家线程 96% 时间在等信号量，忙碌部分 `begin_tick_plan`（世界注入）≈57%、`run_expr`（玩家 JS）≈43%——执行相瓶颈在每玩家的世界树重建，由此启动 wrapper 常驻（下节）。


## 2026-10-10 补记：wrapper 常驻落地（v3）

【现行实现】`registry.js` 模块级 `residentWrappers`/`residentMeta` 表：id 存续且 type/structureType 不变则复用包装实例，省 RoomPosition/装配/原型链接每拍重建；复用点急清全部 `_` 前缀自有键（`Object.keys` 只看自有键，getter 保持官方原形），另刷新自有数据面（pos 仅移动时重建、effects 有则重建无则摘除、flag 定格字段重写）；Mod 对象排除。消亡 id 由 `sweepResidentWrappers` 按 rawById 清扫。设计迭代三次：

- v1（字段级懒戳）：getter 内比较 `_kkMemoTick` 纪元、只清自己字段。共享戳是 wrapper 级的——首访字段推进戳后，后访问字段看到已当前的戳漏清旧值（invaderCore/shield 两例打回，[回归](../../crates/kk-runtime/tests/mod_chain_tests/wrapper_residency.rs)锁定后废弃）。
- v2（getter 内整包 for-in 清）：正确但 ABBA wall +3.0%/CPU +3.5%——每次字段读都过失效检查、for-in 走含原型链的慢路径；bot 每拍读数百万次字段，热路径开销吃掉全部收益，撤回。
- v3（复用点急清，现行）：清空移出 getter；`mod_chain` 493（含两个新回归：多字段记忆失效、跨拍引用同体且读当拍值）、kk-runner 沙箱全绿、棘轮 2905/9/0 零转红。冻结二进制 ABBA（5+100、并发 4）：两对相邻样本方向一致，wall P50 881.6→806.2ms（-8.5%）、P95 -9.8%、CPU 2483.7→2297.0（-7.5%）、run 相 -9.7%、RSS 峰值 +0.7%（噪声级）。[摘要与四轮原始报告](../../compat/reports/mature-comparison/2026-10-10/50x6-wrapper-residency-abba/summary.json)入库；旧二进制与 HEAD release 逐字节一致。

观察面边界（记录常驻已开同类先例）：对象同一性与玩家自挂非 `_` 属性跨拍保持（官方每拍重建即消失）；跨拍持有的旧引用与当拍包装同体且经 rawData 回查读当拍值（回归锁定）；直接读 `_name` 绕过 getter 在复用后首访前得旧值属病态旁路，棘轮未发现依赖。

【已注册后续】wrapper 同一性稳定后，查询面的两项注册启动条件成立但须先重采样归因：(a) `makeRoomQueries` 逐可见房全量扫描改每拍单次分桶共享；(b) look 注册表/空间索引跨拍增量维护（依赖 id→wrapper 稳定）。两项均等 wrapper 常驻后的新热点证据再动工；`Room.find` 同拍记忆化因官方每次返回新数组、缓存引用会被玩家改写污染，暂不做。


## 2026-10-11 补记：共享块编码并行试验撤回（L1a，thread::scope）

【已撤回试验】`build_shared_room_blocks` 用 `std::thread::scope` 按房分块并行编码（先 4 线程、后 2 线程变体，留串行回退），mod_chain 全绿后冻结二进制 ABBA（5+100、并发 4、同 50×6 夹具）。单冷窗极佳：wall 518.1ms、投影段 127.6ms（-59%）、CPU 2161.7 与基线持平——但严格 ABBA 两对相邻样本全部 CPU 违例（公平门"CPU 不劣化"）：

| 变体 | 相邻对 CPU（new vs old） | 均值 |
|---|---|---|
| 4 线程 | 2788.5 vs 2160.1、2759.1 vs 2344.8 | 约 +23% |
| 2 线程 | 2687.4 vs 2342.0、2704.6 vs 2399.4 | 约 +14% |

机理：4P+6E 拓扑上约 330 core-ms 的编码突发集中在 ~85ms×4 线程，P 核功耗/热量密度打满，紧随其后的 V8 run 相实测慢约 40%，每拍总 CPU 反而膨胀——wall 变好但 CPU 变差，是公平门定义拦截的"拿能量换墙钟"。2 线程热机配对有一窗过门（+1.9%），单窗不作依据。实现经 `git checkout` 撤回，未入库；原始报告暂存 `/tmp/kkscreeps-l1a-*.json`、`/tmp/kkscreeps-t2-abba-*.json`（会话产物，未入证据目录）。结论限于本机拓扑；**变体已在 L5 注册重测**：L1b 把投影段砍去约四分之一后突发能量变小，功耗密度耦合可能松动，重测协议为先全机 ABBA、若"CPU 小超 + wall 大赢"再补 2 核亲和窗裁决（用户 2026-10-11 确认的仲裁细则，见部署基准记忆）。


## 2026-10-11 补记：Generic 零拷贝提取落地（L1b，现行实现）

【现行实现】`crates/kk-world/src/kkpv/generic_scan.rs` 受限文法扫描器直接从 Generic 不可变 JSON 原文产出借用 `KkpvVal`，跳过 `object_json` 为每对象建而又弃的 serde_json Value 树（成熟服数百房的 rampart/link/tower 等都以 Generic 存储，共享块构建的解析成本集中于此）。`encode_room_block` 的 Generic 臂优先扫描，受限文法外（转义字符串、病态数字如 `-0`/整型溢出/`1e999`、非对象顶层、嵌套超 serde 递归上限 128、尾随内容）一律回退 serde 参考路径，由 serde 复刻全部行为——含无效 JSON 的空记录分支与非对象顶层的 `json_row` 错误文案。等价由 `generic_scan` 单元对照测试锁定（成熟负载形态×effects 有无×ext 裁切矩阵、数值形态、重复键、递归边界 126-130）。

**键序语义的重要修正（preserve_order 发现）**：首版扫描器按 BTreeMap 排序序实现并在 `cargo test -p kk-world` 单独构建下全绿，但完整门禁 `--workspace` 形态下对照失败——`deno_core`（kk-runner 的 V8 宿主）传递开启 `serde_json/preserve_order`，**所有发布二进制里 serde_json::Map 实为 IndexMap（插入序）**；单独构建 kk-world 时无 deno_core 在图内，静默退回 BTreeMap 排序序，投影键序随构建形态漂移。处置：kk-world 的 serde_json 依赖显式加 `preserve_order`（与 float_roundtrip 并列，Cargo.toml 注明线上事实），扫描器改写为 IndexMap 语义——载荷对象按首现位置保留、后写值覆盖；派生键沿用 `object_json` 插入序列（shard→id→room→x→y→pos→effects），已存在键原地换值、新键追加；`json_row` 的 id/ext 过滤后即行字段。此坑提示：**凡依赖 serde_json::Map 迭代序的代码，必须显式钉住 preserve_order，否则构建形态会改变行为**。

最终二进制 ABBA（5+100、并发 4、O,N,N,O，两对相邻样本方向一致）：

| 指标 | new/old 均值 | 相邻对 1（old1→new2） | 相邻对 2（new3→old4） |
|---|---|---|---|
| wall P50 | **0.890**（-11.0%） | 0.922 | 1.156（旧更慢） |
| 投影段 P50 | **0.725**（-27.5%） | 0.755 | 1.342（旧更慢） |
| CPU core-ms/tick | **0.960**（-4.0%） | 1.007（噪声带） | 1.086（旧更高） |
| RSS 峰值 | 0.990（-1.0%） | | |

公平门通过且 CPU 均值改善；并行度 2001/609≈3.29× 远低于 xxs 同窗约 6.3×。run 段均值 +3.1%（两次 ABBA 均小幅偏高），疑似编码器内存布局变化的次级影响，整拍 CPU 仍下降——登记待 L4 重采样归因。[摘要与四轮原始报告](../../compat/reports/mature-comparison/2026-10-10/50x6-generic-zero-copy/summary.json)入库；旧二进制与 HEAD release 逐字节一致，新二进制 sha `8818a0fa`。验证链：generic_scan 单测 4、kk-world 101（两种构建形态均过）、mod_chain 493、棘轮 2905/floor 零转红、完整无参数 `pnpm run check`。首轮 ABBA（排序序版扫描器 sha `71dbc44d`）方向相同（wall -9.7%/CPU -2.0%），被最终插入序版取代。

【登记后续】投影串行段从 ~292ms 降至 ~220ms（本机口径）；下一片按队列走 L2 结算房并行（Mod 钩子/全局合并保持有序）与 L3 run/settle 重叠。


## 2026-10-11 补记：L2 方向修正与编码器 scratch 落地（L2'，现行实现）

【方向修正】L2 原计划"结算房并行"。`sample` 采样（12 秒双份、~18 拍）证明该方向收益极小：settle 相 ~113ms/拍中，`settle_room` 全部调用点合计仅 ~24ms；大头是 commit/adopt 机制的 BTreeMap 房间表重建与 drop（~22ms）、验证 ~6ms、market/nukes/portals 等全局项与 `Working::new`/索引释放。同批采样另证主线程在 run 相等待 ~295ms（L3 重叠窗口）、共享块每代际全量重建 ~186ms。**并行化一个只占 24ms 的函数没有意义**，L2 按证据改道为投影串行段削减。

【现行实现】`encode_ref` 内部重写（kk-schema，线格式与错误形态逐位不变，33 例字节等价锁全绿）：

- **FNV-1a 符号表**（开放寻址线性探测、调用内生命周期）把类型键与字段键折叠为下标：分桶、字段并集、行内查重、秩映射全部退化为整数比较——取代逐桶 `BTreeSet<&str>` 并集（采样实测 memcmp 15.8ms/拍）、逐行 `seen.contains` 线性串比、逐桶 `HashMap<&str,u32>`（SipHash 11ms/拍）。桶按名称排序、字段并集按名称定秩，均对齐 BTreeMap/BTreeSet 迭代序。
- **行内查重走符号戳**（stamp=行号+1，同拍同 stamp 即重复）：检出点与逐键 BTreeSet 版一致（行序键序遍历），错误形态不变。
- **全调用单一平坦缓冲**：列矩阵（`columns_all`，字段经 `col_base` 区间引用——桶间 clear 会使旧切片失效，故用整数区间而非切片）、变长单元（`cells_all` + `cells_base`）、位图缓冲逐字段复用——取代逐桶逐列 `vec![None; n]`（一拍数百次分配与释放，采样 malloc_zone_realloc+drop glue ~15ms/拍）。
- **目录写入整数查表**（行→桶秩预表 + 逐桶计数器）取代 `BTreeMap<&str,(u32,u32)>` 逐对象字符串查找。
- **嵌套对象查重 scratch**：`encode_nested_val` 增调用方复用的栈式线性扫描参数（base..truncate 为对象作用域），取代逐嵌套对象分配 BTreeSet；检出同一首个重复键。

冻结二进制 ABBA（O,N,N,O、5+100、并发 4，旧=HEAD 38f75e48，新 sha `d644327f`）：两对相邻样本 wall 方向一致（new 快 4.4%/14.9%），均值 wall P50 **-10.4%**、投影段 **-22.2%**、CPU **-5.5%**（公平门通过且均值改善，最差对 +1.9% 在噪声带）、RSS 峰值 -2.0%；run/settle 亦小幅改善（-4.6%/-4.1%，分配压力下降次级效应）。[摘要与四轮原始报告](../../compat/reports/mature-comparison/2026-10-10/50x6-encoder-scratch/summary.json)入库。验证链：kk-schema 33、kk-world 101、mod_chain 493、棘轮 2905/floor 零转红、完整无参数 `pnpm run check`。

【登记后续】投影串行段旅程：~297ms（L1b 前）→ ~215ms（L1b 后）→ ~178-201ms（本片后，本机口径）。剩余：信封/视野每查看者 ~33ms（`ExtensionCatalog::type_for` 逐对象哈希查 4.9ms 可静态预表）、`checked_job_receipt` ~9.7ms、KkpvRow 树 drop ~15ms。settle 侧 commit/adopt 的 BTreeMap 重建与 drop ~22ms 登记 L3'/L4 前采样复核。


## 2026-10-11 补记：目标标尺读数与 xxs 对照环境恢复

【标尺读数】L1b+L2' 后首个同机同窗 xxs 对照（compare.py、50×6、5+100、并发 4、非正式窗，[原始输出](../../compat/reports/mature-comparison/2026-10-11/)暂存 /tmp/kkscreeps-goal-window2）：kk/xxs wall P50 **555.8/237.0ms（2.35×）**、P95 2.28×、CPU **1.30×**（1927 vs 1483 core-ms）、**RSS 峰值 0.59×（2.96 vs 5.03GiB，低 41%）**。并行度 kk 3.47× vs xxs 6.26×（公平门①宽裕；CPU 1.30× 是 ②侧的真实差距）。旅程：23.4×→3.68×→3.17×→3.22×→**2.35×**。达标（0.9×=213ms）需 run 相 ~295ms（V8 聚合 ~660 core-ms/拍，50 线程池并发 4、96.7% 时间等信号量）、投影 ~190ms、结算 ~110ms 三者同时大幅压缩或重叠——L3/L4 是必经之路。

【xxs 对照环境恢复方法（本机 11 天运行后构建产物被清）】git 版 xxscreeps 需要 **node v26**（README："v24 是 npm 版、v26 是 git 版"——node 24 下预编译 pathfinder 报 `HandleScope::DeleteExtensions` 符号缺失是版本错配而非需重编）：`nvm install 26` 后 pathfinder 预编译直接可用；isolated-vm 与 ivm-inspect 需以 node 26 头文件各跑一次 `npx node-gyp rebuild`（isolated-vm 另补 `out/isolated_vm.node` 软链到 compare.py 期望路径）；首次成功启动会在 packages/xxscreeps/dist 下生成 webpack 产物使 tree_digest 中途变化触发 "executable payload changed"——重跑一次即稳定。CMake 构建 pathfinder 的路线（llvm/boost/ninja）不需要，勿再入坑。


## 2026-10-11 补记：L4 注入面归因——GC 是"慢首遍"的谜底，分桶消扫描落地（L4a，现行实现）

【分步分解】以 `op_kk_get_used`（线程 CPU 毫秒、微秒精度）做 beginTick 分步标记（每 50 拍经 KKSTEP 行旁路 stderr 输出均值；热路径禁逐对象 op 调用、只用整数计数器）。50×6 成熟房（每玩家投影 **3210 对象**：298 road + 100 rampart + 60 extension + 14 creep + …，~75% 逐拍完全不变）每玩家每拍：`decodeBundleResident` 5.06ms、refreshGame 7.3ms（主循环 gWrap **4.84**、makeRoomQueries 六次全量扫描 qScan **1.71**（19.2k 迭代中 5/6 是他房跳过 + 3.2k Map.get）、room 回填+清扫 0.67、Room 装配尾段/尾部字面量 <0.1）、rawById 0.53、decodePublic 0.31、信封 parse 0.14、memory/地形 0.12。逐项禁用变体实测：miss 路径（wrapObject 全新包装，~64/拍/玩家）仅 0.17ms、`_` 记忆急清 ~0.07ms、Reflect.apply 换直调省 ~0.25ms——都不是大头。

【阶梯实验→GC 谜底】同体循环一拍内连跑五遍：**3.9 / 1.7 / 0.75 / 0.69 / 0.68ms**——"首遍慢 5.7×"每拍重现（方差极紧、非间歇尖峰）。解码器**之前**的入口遍仅 0.78ms（优化代码跨拍健在）；区间探针（parse 后/解码后/rawById 后/refresh 前）锁定毒在 decodeBundleResident 期间进入、rawById 的轻触遍（每记录一次 `o.id` 读）后"痊愈"。两个形状假设先后证伪：变体 H（MATBITS 物化位标记 + 缺失字段懒删除，未物化字段零接触）与变体 J（值未变跳过 store）都不动 gWrap。最终 `--trace-gc`（经临时 `KK_V8_FLAGS` 管道，已回退；`set_flags_from_command_line` 需要 argv[0] 哑元）实锤：12 拍 × 50 玩家共 **8501 次 GC ≈ 14 次清道夫/拍/玩家 × ~0.31ms ≈ 4.3ms/拍/玩家**，年轻代仅 1-3MB 且存活率 ~50%（解码器 `seen` Set 拽住 3210 个每拍新建的 id 字符串是存活主源之一）。"慢首遍"= 分配密集循环接盘清道夫的落点彩票；旧账面 "refreshGame 384ms/拍" 与 "decodeBundle 256ms/拍" 均含此 GC 分摊，此前按纯工作量的归因口径须打折。

【半空间扫描与裁定（不设默认，逃生口保留）】`--max-semi-space-size` 扫描（探针污染二进制、同二进制 env 对照）：4MB ≈ 无旗标；8MB 在污染二进制全指标 −16%，**干净二进制 + 丢弃窗热协议两轮独立对照确认 wall −2.3~2.4%、CPU −5.6~6.5%、run −7.2%、RSS 峰值 +0.9GB（2.74→3.62GB）**；16MB 反效果（垃圾跨清道夫存活晋升老代）。**裁定不设默认值**：进程级半空间对小堆沙箱失衡——48MB 堆（2GB 部署形态）的年轻代占比 33%，近堆双闸门的恢复不变量两例顶穿（48+16MB 预留使恢复后上限 75-91MB，撞测试界 60/80MB），近堆机制是部署保护面，不为个位数百分比墙钟+RSS +30% 冒险。保留 `KK_V8_FLAGS` 研究逃生口（env 透传，须在首个 isolate 前经 kk-cli main/embed setup 入口生效——**V8 初始化后旗标冻结，迟到调用 `Check failed: !IsFrozen()` 直接崩**，mod realm 先建运行时的 sim 流程踩中过）。数据待 L4b 分配削减降低清道夫压力后复评。

【现行实现：L4a 单次分桶】`buildGame` 主循环一遍产出 `roomBuckets: Map<房名,{os,ws}>` 平行数组（尾部索引赋值不走 push 方法分派，防玩家替换数组原型）；`makeRoomQueries(name, bucket)` 桶直读（扫描 19224→3204 迭代、controller/storage/terminal 与 look 装配的 3.2k 次 objectsById Map.get 归零，桶序=世界序子列与旧全量扫描按房过滤同相对顺序）；`methods.find` 对象类查询同改桶直读（bot 执行期每次 find 免全量扫描+逐对象 Map.get）；room 回填改桶遍历（免 3210 次 Map.get；未建房回填 undefined 与旧"保持主循环重置值"等价）。**坑**：首版漏改 `find()` 闭包里的 `worldObjs` 残引用（sim 44 例 `worldObjs is not defined` 打回），改在 install 内按拍捕获后消。验证链：sim 106 全过、kk-mod-host 链 8、棘轮 2905/9 豁免零转红（两次）、JS 格式、完整无参数 `pnpm run check`。

【ABBA 与冷首窗伪影（重要方法教训）】冻结二进制 old=`21b9a2da`（HEAD 3a15a4c）/new=`e450851a`。前两轮 ABBA 呈"new 慢 10-12%"假象；逐窗绝对值揭露真因：**冷首窗比热稳态快至 30%**（轮间写文档让机器冷却，old1 恒为冷窗）——同二进制四窗对照证实（首窗 707.8 vs 后续 750±20）。修正协议：**先跑 2 个丢弃窗推到热稳态再采 O,N,N,O**。修正后四窗极干净（旧窗 758.1/756.8、新窗 721.1/725.4 几乎逐位复现）：两对相邻样本一致 new 快（wall 4.9%/4.3%），均值 **wall P50 −4.5%、P95 −4.6%、run −9.2%**（分桶机理落点吻合）、project −0.9%/settle −1.7%（未触及、噪声带）、**CPU −5.5%**（公平门②改善）、RSS 峰值 +0.7%（噪声级）、并行度 3.27× vs 旧 3.30×（门①宽裕）——保留。[摘要与四轮报告](../../compat/reports/mature-comparison/2026-10-11/50x6-bucket-queries/summary.json)入库。**本机此后一切 ABBA 必须带丢弃窗预热**；另注意相邻对读数方向：驱动 pair2 是 old÷new，>1 即 new 快（本片曾险些读反）。

【登记后续】(1) 安静机复测：分桶 ABBA + 半空间干净基线；(2) L4b id 复用——目录按 (hi,lo) 升序但块间无序、Map 插入序被新增对象破坏、u32 超 Smi 范围需装箱，块内归并/zip 对齐+变更块回退设计复杂度对 ~2% CPU 收益暂缓；(3) GC 侧更大的杠杆是**削减分配与存活率**：解码器每拍 idFromHalves 新串 + seen 存活、变长字段每拍重物化（store/name 等 bot 每拍首读即新对象，与官方语义同footing）、玩家脚本自身分配；(4) 台阶现象也意味着 run 相内玩家代码同样按 GC 落点付税——L3 重叠设计时须一并考虑分配节奏。


## 2026-10-11 补记：L4b 解码器块内归并落地（现行实现）

【现行实现】GC 归因指出解码器每拍每玩家物化 3210 个新 id 字符串（`idFromHalves` 分配 + Set/Map 哈希）且 `seen` Set 拽住它们存活过整个解码窗口（存活率 ~50% 的主源之一）。落地**块内两指针归并**：`mergeState`（WeakMap，键=resident Map）保存上一拍记录全数组、每块累计末界与目录键并行双列（u32 double 数组）；束块序=投影房间序稳定时逐块归并——键相等即位置复用旧记录（**完全不物化 id 串**，免分配/哈希/Map 探测），小于当键的旧记录即本拍消亡出表；未命中或布局签名不符落回慢路径（idFromHalves + Map 查询）。清扫由 seen Set 改 `SEEN_TICK` 符号戳比对（免 3210 次串哈希）。块数变动（房间集变化）的错位轮由慢路径自愈（错位只退化为重建记录——resident 是缓存，正确性无害），下一拍以真实布局恢复全量对位。

【ABBA】冻结二进制 old=`73c00619`（HEAD 62854ac）/new=`4a2ae71c`，丢弃窗热协议 O,N,N,O（机器外部噪声 load~10 下采集）：两对相邻样本方向一致（new 快 wall 1.0%/3.2%），均值 **wall P50 −2.1%、P95 −1.6%、run −5.9%**（解码器机理落点）、project/settle ±0.1%（未触及）、**CPU −3.4%**（公平门②改善）、RSS 峰值 +1.0%（噪声级）——保留。[摘要与四轮报告](../../compat/reports/mature-comparison/2026-10-11/50x6-decoder-merge/summary.json)入库。验证链：sim 106、kk-mod-host 97、棘轮 2905/9 豁免零转红、JS 格式、完整无参数 `pnpm run check`。

【登记后续】变长字段重物化削减（bot 每拍首读 store/name 即新对象）需要单元格级字节比对设计（快照 cell 区间 + memcmp），收益与复杂度待 GC 复采样后评估；`updateResident` 的急切 store 仍在（变体 J 证其对首遍速度无益，但分配面未触及——float/ref 列读值分配保留）。


## 2026-10-11 补记：热稳态标尺校准——真实比值 ~1.84×

【校准】丢弃窗热协议（先整跑一轮丢弃再采集，两侧引擎同窗同机）重测 L1b+L2'+L4a+L4b 后的 xxs 对照，[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-thermal-yardstick/summary.json)：**热稳态 wall 比值 1.85×/1.83×（两独立窗复现）**、CPU **1.00-1.06×（平价）**、RSS **0.69-0.70×**。晨读 2.35× 系冷机伪影的**反向面**：xxs 热敏感性远高于 kk（冷→热 wall +50% vs +18%），冷窗一直在给 xxs 放水——此后正式验收窗必须两侧丢弃窗预热。旅程（统一热稳态口径）：**~1.84×**（此前各程均冷/混合口径，不可直接比）。等价性分歧签名（structures 517/518、move +0.34%）与晨窗完全一致，系既有登记项非本批引入。

【剩余差距结构】热稳态 kk wall ~515-657ms 三相：run ~283-296、project ~210、settle ~152；xxs 同窗 281-355ms 总量。达标 0.9×（热稳态 xxs×0.9 ≈ 253-319ms）需 run 相再压或 L3 重叠（run 相主线程等待 ~295ms 是最大单块）。


## 2026-10-11 补记：L3 两项结构性试验证伪（并发重标定与就绪队列前瞻）

【主线程归因】macOS sample（12s、丢弃窗后）：主线程 78.6% 在 run_players——其中 44% 纯等待（信号量等回执）、31.6% 投影编码（kkpv_bundle_for_viewer 23.7%）；结算 21%。投影已在主线程与 4 工作线程的 V8 执行重叠（就绪派发流水线），run 相 wall 由 worker 吞吐决定（660 core-ms ÷ 4）。

【并发重标定证伪（L5 关闭）】公平门①留有近一倍余量（kk 3.41× vs xxs 6.26×），但并发 6/8 实测（丢弃窗）：run 相 −26/−57ms 的同时**主线程串行相被功率密度拖慢**（proj 174→218/224、settle 109→146）、**CPU +29%/+35%**，wall 反而 550→595/570——4P+6E 上加并发得不偿失，L5 并发重标定按实证关闭（部署机 2 核形态另论，仲裁协议保留）。

【就绪队列前瞻证伪】假说：run 相 worker 忙碌仅 ~65%，空泡来自"完成事件后主线程先投影 4.2ms 再派发"。实现就绪队列前瞻（等待期预投影、worker 释放即派发，kk-runtime execute_players）：sim 106 + kk-runtime 568 全过，但热平台 interleaved 对照 new 669.7 vs old 660（+1.5% 噪声级）——**无收益撤回**（留证 /tmp/kk-rq-*.json）。空泡主源不是投影延迟，更可能是 V8 GC 停顿与内核调度唤醒；run 相确系 worker 吞吐约束。

【剩余杠杆结构（更新）】热稳态 wall ~515-660ms = proj 174-218（主线程串行）+ run 253-306（worker 吞吐）+ settle 109-148（主线程串行）。xxs 以 6.26× 并行度跑全部相位；kk 受 4P+6E 功率密度约束（L1a/并发扫帚双证）。达标 0.9×（~253-319ms）的可行序：① V8 注入面 row-dirty 跳过（Rust 编码器逐格比对标脏 + 束带行脏位，JS 端免 updateResident 全量——~30-60 core-ms）；② 结算相 109ms 的可并行成分采样归因后分片；③ 相位重叠/并行化的 L1a 仲裁路径（"CPU 小超+wall 大赢"→2 核亲和窗，用户已确认细则）。

【settle 相细分（同批 sample）】1635 样本（~140ms/拍）：tick_with_writebacks ~1100（settle_room 仅 154 ~13ms；BTreeMap 克隆/迭代/drop 胶水 ~350 ~30ms；commit_tick_rooms 124 ~11ms；其余为引擎簿记）+ **publish_tick 299 ~26ms**。可并行成分候选：commit/adopt 的逐房 BTreeMap 重建与 publish_tick 内部结构（下片采样后定）。


## 2026-10-11 补记：row-dirty 行级脏位落地（现行实现）

【现行实现】投影侧每拍重编码 300 房 ×535 对象，而解码端每拍对 ~60% 行做全量 `updateResident`（急读字段重写 + 变长访问器重装）——值未变的行是纯浪费。落地**行级脏位**三层：

1. **编码端逐格比对**（`kk-schema/kkpv/dirty.rs`）：`encode_with_dirty` 与上一拍同源保留块（`PrevEncodedBlock`：块字节 + 结构索引）比对——桶按 type_key 配对、字段按键+列类型配对（含 prev-only 字段"上一拍缺失才干净"补判）、行按目录 ID 双指针对位（行号漂移/桶行数变化不影响）、定宽 8 字节位比对、变长 cells 在两拍 arena 内字节比对。**保守判脏**：任何不确定（无配对/类型变/内容异）判脏，判脏只损失跳过机会；首拍（无基准）产物与 `encode_ref` 逐字节一致。
2. **束格式第五段**：FLAG_DIRTY（头部 144..152 段对 + 152..160 基准拍号，保留区语义更新）；脏位段=逐桶行位图按桶秩序拼接；Rust 视图与 JS 解码端同步扩校验（段/旗一致性、段长与桶行数精确、尾位为零、基准拍号≤本拍）。
3. **解码端干净行跳过**（`kkpv.js`）：块脏位基准与常驻表上一拍成功解码拍号衔接（`mergeState.lastTick`）时，干净行只重指 `DECODE_CTX`（防旧束 backing store 被未物化惰性字段钉住）跳过全字段循环；跳拍/重试/基准过期自动回退全量（安全性不依赖编码端保留新鲜度）。保留挂在 kk-world 共享 full 块（`room_block_dirty`，与共享缓存同 Arc——保留字节即各查看者上一拍所收字节），Fresh 逐查看者路径不参与（内存有界）。

【两个实现期陷阱（入档防错）】① build_index 首版每桶偏移从 buckets_off 重算未累计前序桶字节——第二桶起全错；由"索引偏移 vs 独立走查交叉验证"测试当场抓住。② 世代门初版用 `prev.generation == input.generation` 拦过期保留——但世界每拍变异都换代，门永远不匹配；正确性本就由"保留字节==解码端上一拍所收字节 + 基准拍号校验"保证，世代门既多余又有害，已移除（跨代际保留至多退化为全脏）。

【ABBA 与方法论教训】首版 ABBA 误用 **debug 构建**作 new 对照 **release** 冻结基线，得 3.7× 假回退（project/settle 全膨胀、逐段插桩定位无果后 HEAD 干净对照复现同量级——同码不同构才暴露：冻结基线 72MB=release，误冻 124MB=debug）。**冻结二进制基线固定 release 口径**。正式 ABBA（丢弃窗热协议 O,N,N,O）：两对相邻窗方向一致（0.787/1.149），均值 **wall P50 −17.6%（0.824）、P95 −10.9%、run −36.0%、settle −13.0%、project ±0（比对成本被共享装配吸收）、CPU core-ms −22.6%（公平门②显著改善）、RSS 峰值 +5.0%（每房保留 ~40KB+索引）**。[摘要与四轮报告](../../compat/reports/mature-comparison/2026-10-11/50x6-row-dirty/summary.json)。机理：run 相免 ~60% 行的全字段更新；settle 同步受益于解码端分配/存活垃圾减少（V8 GC 压力外溢，与 L4b GC 归因一致）。

【验证链】kk-schema 40 单测（偏移交叉验证/脏位语义/拒变体/首拍字节等价）+ kk-world 脏位语义锁（第二拍带脏位与全 fresh 编码逐对象逐字段等值、插入行脏/存量行净）+ kk-runner sandbox 62 场景（新增 `kkpv_dirty_resident`：连拍干净行懒字段重指后可读 + 基准错位回退全量）+ sim 106 + kk-mod-host + 棘轮 2905/9 豁免零转红 + 完整无参数 `pnpm run check`。

【剩余杠杆结构（更新）】热稳态 wall 降至 ~500-570ms = proj ~185-227 + run ~170-191 + settle ~117-141；达标 0.9×（热稳态 xxs×0.9 ≈ 253-319ms）已进入射程——run 相尚有变长字段重物化削减（cell 级复用）与 settle 可并行成分（BTreeMap 重建 ~30ms、publish_tick ~26ms、commit_rooms ~11ms）两线；另需安静机热稳态标尺复测确认对 xxs 的实际比值。


## 2026-10-11 补记：row-dirty 后热稳态标尺——1.559×、CPU 首次低于 xxs

丢弃窗热协议同窗复测（[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-rowdirty-yardstick/summary.json)）：**kk wall P50 493.7ms vs xxs 316.6ms = 1.559×**（P95 1.542×、P99 1.279×，旅程 23.4×→…→1.84×→1.559×）；**CPU core-ms 0.775×——kk 首次低于 xxs**（1472 vs 1899）；RSS P50 0.925×/峰值 0.872×（更低，含 row-dirty 保留 +~150MB 后仍低）；公平门①并行度未变（kk ~3.3×）。等价分歧签名与既往完全一致（structures 517/518、move +0.34%），row-dirty 未引入新分歧。剩余 1.559×→0.9×（~285ms）需三相再压：settle 可并行成分（BTreeMap 重建 ~30ms、publish_tick ~26ms、commit_rooms ~11ms）、run 相变长重物化 cell 级复用、以及相位重叠/并行化的 L1a 仲裁路径（2 核亲和窗细则已确认）。


## 2026-10-11 补记：settle 数据搬家切分证伪与 L1a 重叠设计定型

【采样归因（release）】settle ~117ms（row-dirty 后）在 tick_with_writebacks 子树内的自帧分布：settle_room 本体 766（~45%，游戏规则）、BTreeMap 迭代/重建 406+133、publish/commit 331/306、分配器（szone/tiny malloc+free）~600 分散、validate_stage 130、serde 反序列化 123+101（来源待查）。

【A+B 切分与证伪】A：commit_tick_rooms 逐槽替换（values_mut 锁步 zip，免整树重建与 300 次 String 键克隆；上游已验证房集与顺序一致）；B：settle_rooms_stage 房间深克隆 thread::scope 并行（可用并行 1/2、至多 4 线程、小房集退化串行；克隆纯只读、按索引保序、无 ID 确定性问题）。ABBA（丢弃窗 O,N,N,O）：**settle 比值 1.002（±2%）、wall 均值 +4% 且相邻对方向矛盾（0.973/0.897）——判噪声**。结论：深克隆与 BTreeMap 重建/搬家在 release 下合计 <5ms，早前 ~30ms 归因（粗采样）误导了切分选择；B 撤回、A 保留（微改免工作量）。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-settle-split-falsified/summary.json)。

【教训】settle 相的采样归因必须以"改动→ABBA 复验"闭环，子树样本占比≠墙钟可回收量（分配器成本被重复计入多个子树）。

【下一主片：L1a 房间级编码-结算重叠（设计定型）】当前三相串行 wall ≈ project 190（其中共享 full 块编码 ~100-140 + 信封/装配/回退 ~50-90）+ run 175（worker 吞吐约束，主线程 44% 等待）+ settle 117（settle_room 本体为主，切分证伪后不可再压）。重叠方案：staged_body 的 settle_rooms_stage 逐房结算完成即把**候选房**（txn.rooms 已结算元素，不可变）交给空闲工作线程编码共享 full 块——编码对候选只读、产物与发布后编码逐字节一致（含 row-dirty：prev 保留块以 Arc 快照随任务下发，线程内不触共享 RefCell）；主线程继续结算后续房间。收益上界 = min(共享块编码, settle) ≈ 100-140ms 隐藏，wall 预期 −60~100ms；CPU ±0（同样工作量换线程）；确定性无影响（编码纯函数、块序按房名）。实现要点：kk-world 需暴露"按单房编码 full 块"入口（现 build_shared_room_blocks 全服扫）；kk-runtime 的 execute_players 改为消费预编码块（project 相退化为信封+装配+回退房）；候选-发布一致性由"同数据同字节"保证（commit 校验不变）。风险：候选在 publish 前被 inter-room 阶段改动（portal/跨房过渡会改 txn.rooms！）——重叠编码须延后到 inter_room 应用之后，或对受跨房影响的房重编。

## 2026-10-11 补记：L1a 房间级编码-结算重叠落地（现行实现，懒消费 v2）

【交付】engine 暴露 `room_settled_hook`（逐房结算后回调，候选已定、inter_room/market 等晚期阶段未应用）；kk-runtime 的 KKPV 通道惰性起 2 个专职编码线程（`kk-preencode-{0,1}`），hook 内对候选做 `Arc<RoomState>` 快照（实测全批 ~7ms）轮转派发；worker 对候选只读预编码共享 full 块（row-dirty prev 以 Arc 保留快照随 Begin 下发），产物直写共享 `PreencodeMap`（`Mutex<HashMap>`），**宿主无阻塞等待**；下一拍 `build_shared_room_blocks` 逐房从地图取走即删，命中且 `matches(tick,generation,plan)` + `*entry.source == *state`（逐房深比较，兜底跨房/市场/portal 等一切 hook 后变异）才复用，未命中回退内联编码；失败轮 armed 复位、下一 Begin 清空残留；adopt_edit 同步清空。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-preencode-overlap/README.md)。

【结果】冻结 ABBA：wall 0.869×、project 0.381×、settle 1.291×、CPU 1.174×，两相邻对方向一致。进程内交替 A/B（`compare-probe --interleave 25` 新增，判定主证据）：快/慢档 wall −100/−90ms（−24.8%/−17.5%）、CPU +8%/+15%。计数器插桩：hit 300/300、miss/evict/leftover 全零。

【v1 同步 flush 证伪】首版结算末阻塞排空取回：settle 2.05×（flush 等待 42-56ms）、CPU 1.27×、wall 持平——单线程全量编码 140-155ms 放不进 ~110ms settle 窗，超额部分变主线程阻塞。**教训：重叠型优化收益上限 = min(被隐藏相, 窗口)，同步取回点把超额变阻塞；多 worker + 懒消费（消费点校验）才是正解。**

【方法论】窗口级 ABBA 在本机存在 ±80ms 双峰机器档位噪声（曾因用户放视频放大）；交替 A/B 同进程按 tick 块切换开关，机器状态对两模式同权，分辨 ±40ms 级效应必须用它。窗口 ABBA 保留为归档协议（二进制冻结可跨会话复现）。

【公平性】CPU +8~17%（worker 疑落能效核，同量编码占核更久）。本机 4P+6E 并行度 ~3.9/10 未饱和，wall 兑现；2 核 CPU-bound 部署会反噬同幅度（分析结论，darwin 无 taskset 未实测）。后续可试 worker QoS 提升驻留 P 核。

【剩余差距】热稳态 1.559× → 本片预期 ~1.25×（−100ms 级），距 0.9× 仍需 run 相（变长字段重物化 cell 级复用）与 settle 内 serde 来源追查；标尺复测在后续批次执行。

## 2026-10-11 补记：L1a 后标尺复测与测量污染源定性（代理 UI 渲染 + 热积累）

【标尺复测】L1a 后 compare.py（50×6、5+100、并发 4）六窗：受污染四窗（见下）kk 523-536/ratio 1.56-1.65；**静默两窗 w5=1.3971（kk 401.2，proj 66/run 185/settle 143、CPU 1660、CPU 比 0.932）、w6=1.6331（kk 494.1，热浸态）**。w5 与交替 A/B 的 −107ms 相互印证（昨日热窗 494−107≈387≈401）。xxs 同期 287-303；kk 对热态的敏感度约为 xxs 的 5 倍（CPU +20% vs +4%）——**标尺比值本身随 SoC 热态在 ~1.40-1.63 摆动，单窗读数不可作达标判据，正式验收必须多窗取中枢并报告热态**。

【测量污染源定性（本会话最重要的方法论发现）】① 用户放视频：窗口级噪声放大（早前 ON/OFF 对比 +645 core-ms 假差异）。② **代理自身的 UI 渲染进程（本会话 ZCode Helper Renderer 实测 52.9% CPU、WindowServer 30.9%），负载随代理输出 token 量波动**——代理等待时空闲（快速档）、持续输出/轮询时烧核（慢速档），把一切跨进程对比打成双峰；CPU 与 wall 同比例波动（并行度恒 ~4.1）是核速变化的指纹。进程内交替 A/B 因两模式共享机器状态而免疫——这正是它作为判定主证据的原因。③ SoC 热积累：连续满载窗口间 kk 关键路径（串行 project/settle + 2 编码线程）直接拉伸。**新协议：一切基准在后台跑、期间代理保持静默（不轮询不输出）、跑完收单次通知；跨会话/跨日绝对值对比必须带热态与背景负载注记。**

## 2026-10-11 补记：P2 首刀——装配主环与房间查询的干净行增量维护（现行实现）

【机理】R00-lite 裁定 P2 优先（干净行 96%、靶面 bwrap 1.64+brooms 1.08+rawid 0.34+btail 0.32≈3.4ms/玩家）。实现四层：① 解码器（kkpv.js）给变更行打 `CHANGED_TICK` 符号戳（新建/就地更新/签名重建三路径），归并/清扫收集 deadIds——**跨块活行（跨房移动）经 SEEN_TICK 区分不算消亡**（旧房块的归并把换块行当死亡是既有自愈行为，增量装配下必须修正，否则活对象包装被误删）；② 装配主环（game.js）按戳差分：干净行只做 `_` 记忆清空（tickResidentMemo），变更行按常驻元数据（type/owner/name/x/y/room/rec 六元组，registry residentMeta 扩展）分派——同房就地走 lookSpatial 格位迁移（moveCell），新/死/跨房/记录重建/包装换新走房整建（桶原地重填，世界序保序，os/ws 平行）；③ Room 实例+查询索引常驻（queries.js 闭包捕获 let 绑、rebuild 重绑换新、findCache 逐拍重置防位置投毒；room.js ensureRoom/refreshRoom 每拍刷信封字段+eventLog 新 parse+重 track pathfinding 保逐拍 grids 缓存语义）；④ objectsById/分类表/可见房列表持久，分类表拍末浅拷贝保官方"每拍新对象+扫描序"语义。回退轮（跳拍/重试/块数变动）解码全量打戳、装配自动退化全量，无独立门控。

【三个实现陷阱（回归测试全部抓住）】① **时间折算字段**（ticksToDeploy/decayTime=绝对键+Game.time 折算）：行干净但玩家面值每拍变——干净行也必须清 `_` 记忆（mod_chain 6 失败打回）；② **eventLog parsed 数组玩家可原地改**：原串比对跳 parse 会让篡改跨拍存活（room_prototype 打回，官方每拍新 parse）；③ **玩家直挂 `_` 键不经 getter/setter**：MEMO_MARK 条件清空有盲区，棘轮 UNDOC-IDENTITY-005 打回——`_` 前缀是引擎记忆命名空间，保持无条件逐拍清空（首版 ABBA 0.865 与修正版 0.859 同幅，标记跳过本就无收益）。

【ABBA（冻结 release、2 丢弃+O,N,N,O、5+100、并发 4、同 l1b 夹具）】new/old：**wall P50 0.859（−14.2%）、run 0.785（−21.5%）、CPU 0.834（−16.7%）、RSS 峰 0.933（−6.7%）**、project 0.938、settle 0.936；两对相邻窗方向一致（0.834 / 1.131）。绝对：wall ~503→~420ms、run ~222→~174ms。KKDIAG 组成：rawid/brooms/btail 全部 →0（低于整数毫分辨率）、refresh 3.06→2.0（bwrap 1，残留=3210 行 `_` 清空+126 变更行）；dirty/remat 不变（解码侧行为不变）。棘轮修正后 2905/9/0 转红 0。观察面扩展：Room 实例同一性跨拍保持（同 wrapper 先例）。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-incremental-assembly/README.md)。

【对目标的含义】以 w5 静默窗（401.2ms、1.397×）为锚：本片 −14% ≈ −56ms → ~345ms（~1.15-1.2× 热态口径）。0.9×（≈285ms）仍差 ~60ms：下一片按队列——remat 字段级复用（804 次变长重物化/拍/玩家，priv 2ms 主靶）或 P4 依赖调度（settle 143ms 主杠杆）。重叠纪律：本片与 L1a 均作用 run/整体 wall，收益不可与 L1a 的 −90~100ms 相加外推。

## 2026-10-11 补记：P2 首刀后标尺——1.155×

【读数】compare.py 同窗配对（50×6、5+100、并发 4，丢弃轮+测量轮，二进制 670c730d）：**测量窗比值 1.1550（kk 343.1 vs xxs 297.0），自 L1a 后静默窗 1.397×**；丢弃窗（较冷）1.4137（kk 294.9/xxs 208.6）——kk 热敏 +16%、xxs +42%，热窗比值反而更低（与既往"比值随热态 1.40-1.63 摆动"方向相反的窗口对，印证单窗不可判、多窗取中枢）。CPU 比 0.749、RSS 峰值比 0.609（公平门大幅富余）。分相 project 63.4/run 138.0/settle 139.1——run 与 settle 并列最大。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-p2-first-cut-yardstick/README.md)。

【settle 复采样】主线程 sample（300 tick 长窗中段）：settle_world_with_writebacks 内 engine::settle 主导，可辨识叶 settle_room ~15%/publish_tick ~16%/commit_tick_rooms ~6%，其余散布（校验/分配/drop/迭代）——与 settle 搬切证伪时的"成本分散"结论一致，无单一 ≥40ms 杠杆。结构性压缩（相位重叠/房间依赖调度）归 P4/R03 底座按重构方案放行。下一片：remat 字段级复用（run 侧）。

## 2026-10-11 补记：remat 字段级复用——脏行内未变字段物化值跨拍保留（现行实现）

【机理】行脏位让干净行整体跳过，但脏行（120/3210）仍全量重物化全部变长字段（store/body/actionLog/effects 等，中位 6.4 个/行，计 768 次/拍/玩家）。本片把脏粒度下推到字段：编码端（kk-schema dirty.rs DirtyMaps）逐桶逐 kind≥5 字段发位图，新 flags 位 FLAG_FIELD_DIRTY=4（携带前提 FLAG_DIRTY，束内位于行脏位段之后，尾位按桶行数归一）；Rust 解码端 view.rs 同源校验段长与尾位；JS 解码端 updateResident 增加 trusted 路径——脏行内字段位干净的保留已物化值（含惰性访问器 ctx 已指旧值的情形只跳过重装），varIdx 全分支推进防错位。玩家面语义不变：值由编码端逐格比对保证等价，观察面（wrapper/记录/Room 同一性）不动。

【ABBA（冻结 release、2 丢弃+O,N,N,O、5+100、并发 4、同 l1b 夹具；旧基线与 P2 提交 2d5a872a 逐字节一致）】new/old：**wall P50 0.944（−5.6%）、run 0.861（−13.9%）、CPU 0.929（−7.1%）**、settle 1.005（持平，符合 decode 侧定位）、project 0.986；相邻对方向一致（0.943 / old4 比 new3 慢 5.8%）。**RSS 峰 +4.4%（~+140MB）记录在案未单独归因**（对 xxs RSS 门 0.609× 余量大）。KKDIAG：**remat 768→60（−92%）**，dirty/rows 口径不变，priv 1.5→2.0ms（字段位图查表，亚毫秒级）。棘轮 2905/9/0 转红 0（束格式变化零玩家可见差异）。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-remat-field-reuse/README.md)。

【测试面】sandbox 计数器测试（field_dirty_reuse_counters）：定宽字段变化拍断言 dirty=1/remat=0、变长字段变化拍 remat=1——粒度语义由计数器直接锁定；kk-schema 42（尾位归一/逐字段跟踪新增）、kk-runner 63、kk-runtime 495、kk-cli sim 106 全绿。mod_chain 侧的字段级同一性断言被证伪删除：`store` getter 每拍 `makeStore` 重建（`_` 记忆无条件清空语义），玩家可见对象身份本就不跨拍，字段复用只在记录层成立。

【对目标的含义】标尺窗（P2 后 1.155×，kk 343.1）锚点再让 ~5.6% ≈ −19ms → ~324ms（~1.09× 热态口径，未复测）。距 0.9×（~267ms）仍差 ~57ms。run 相经 P2+remat 两刀已显著压缩（方向性对照，跨日窗口不可比绝对值：L1a 后 w5 窗 run 185ms → 本片 ABBA 新窗 117-122ms，其中含 P2 首刀 −21.5% 与本片 −13.9% 的同机交错裁定），**下一杠杆在 settle（139ms，成本分散无单点）与 project（63ms）——结构性相位重叠归 P4/R03 重构底座**；队列再评估小片（分类表构建复用、可见房循环窄化）是否够得着剩余差距。

## 2026-10-11 补记：remat 后标尺——1.2213×（kk −77ms，settle 首成最大相）

【读数】compare.py 同窗配对（丢弃+测量，二进制=remat 提交 1e740c5f 重建）：**测量窗 1.2213×（kk 266.0 vs xxs 217.8）、丢弃窗 1.2302×（253.0/205.7）**，两窗一致差 0.9%（本批全程机器清闲，为迄今最稳定标尺对）。CPU 比 0.782、RSS 峰比 0.653，公平/内存门余量大。分相：project 49.5 / run 96.2 / **settle 117.9（首次最大）**；run 旅程 185→138→96（跨窗方向性）。等价签名无新分歧。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-remat-yardstick/README.md)。

【比值反向的机理】kk 绝对 −77ms 但比值从 1.155× 升到 1.222×：xxs 凉窗提速 27%（297→217.8）远超 kk 热敏差——再次印证比值口径依赖 xxs 侧热态、单窗不可判，验收按多窗中枢+热态注记。

【达标路径裁定】0.9×（本窗口径 196.0ms）需 −70ms。单相消除仅 run 可行（project 归零仍差 20ms）；settle 无单一杠杆（成本分散采样已复核）、run 已两刀、project 编码已被 L1a 隐藏。**结构性相位重叠（P4 依赖调度/R03 紧凑房间页）为达标必经**，R00-lite 证据已满足方案放行条件；小片（分类表复用、可见房窄化 ≤10ms）降为伴随优化。

## 2026-10-11 补记：私有束按可见集分组共享（现行实现；用户纠正改型）

【历程】初版"全视野门槛"被 B0 证伪（夹具每房单一主人，无人全视野，ABBA 中性），一度裁定撤回；**用户纠正**——真实负载（打架互驻军/探索/联盟/观战）视野重叠是常态，应推广并构造重叠测试而非丢弃。改型为**按可见集分组共享**：`VisibilityIndex` 增 `owned_rooms` 反转表与 `visible_rooms_of`，分组键=完整可见房集合序列化；世界资格=共享块映射覆盖全部房且空块全部可共享（无归属旗/Mod/扩展/工地——块字节不依赖查看者裁切）；组内首装配者发布 `SharedKkpvBundle` 句柄（RunJob 私有束增 Shared 变体，挂载与公开束同机制），后续同组命中零拷贝复用；信封/market/power 仍逐玩家。同组束仅差无人消费的 audience 位（JS 解码不读）。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-grouped-bundle-sharing/README.md)。

【两个回归抓住的实现陷阱】① **组缓存代际更替必须整体清空**：束字节携带块头 tick，资格跨代际保持为真时只换 epoch 标记不清映射会让陈旧束注入（解码端"块头 tick 与信封不一致"拒绝→表达式 Null），economy_gcl 回归打回；② **引擎夹具的 road 不带归属**——互驻军须用可占领结构（link），无主结构不授予视野（kk-world 手写载荷单测不暴露此点，引擎端到端才暴露）。

【B0 定位】每玩家可见集互异（6 自有+6 观察），分组不生效；终版冻结 ABBA（old 5e9abba5）：**wall 0.991 / project 1.010 / 相邻对 1.000、1.019——中性确认**，微开销（每代际资格探测+每玩家键构建）在噪声带内。棘轮 2905/9/0 转红 0；kk-world 103（新增互驻军/observer 探索同组字节锁、旗与工地资格证伪）、embed 102（新增引擎端到端重叠场景：组缓存恰 1 条+读数一致+旗后回退）、mod_chain 495、sim 106 全绿。**本片对 B0 达标无贡献**（价值主张为真实重叠负载）；B0 的 project 主靶另立——主线程采样：信封构建占逐玩家投影 ~82%（3085/3784 采样）+ 可见性索引重建 ~24% 信封内，逐玩家 Value churn 是下一片（信封逐代际条目模板缓存）。

## 2026-10-11 补记：散页块目录证伪——解码散访代价压倒装配节省（实现后实测）

【历程】分组共享被用户正确指出"活跃玩家可见集不可能完全相同"后，按"不惧工作量的合适方案"方向完整实施散页块目录：每代际一份全房 full+empty 块目录跨查看者共用，沙箱端按信封 visibleRooms 逐房选块（decodeCatalogResident 与逐查看者束共用块游走核心）。实现全绿（布局单测、三玩家混合可见集端到端含**双形态等价锁**——同世界目录/逐查看者装配读数逐玩家一致）。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-scattered-catalog-falsified/README.md)（完整补丁 1336 行存档）。

【实测证伪】B0 短窗两轮：project 66.7/65.4（基线 ~62，无收益）、**run 220.9/232.0（基线 ~120，+100ms）**、wall 443/451；KKDIAG **priv 2→6ms/玩家**（选块正确：rows/dirty/remat 与基线一致）。机理：① B0 每玩家仅见自有 6 房（6×534=3204≈rows 3210，该数字曾两次误导归因），逐玩家装配仅 ~0.1ms（~6ms/拍总量）——**装配从来不是大头**；② 目录缓冲 ~50MB 内散布访问 + 每拍新 backing store 缺页 + 逐槽房名读取 = +4ms/玩家。胜负条件"装配节省>散访代价"在任意可见集形态下都不成立（全互见时装配 memcpy ~0.1ms 仍远低于散访）——**方向被支配，已回退**（HEAD 停在分组共享 348ec122）。

【投影侧归因最终修正】project 62ms = **信封 ~50ms（82%）** + 装配 ~6ms + market/power ~5ms。散页证伪后，投影侧唯一有分量的杠杆是**信封逐代际条目模板缓存/字符串直构**（kkpv_envelope_for_viewer 逐玩家 300 房 Value 构建 + 事件 from_str ×50 玩家）。解码端 kkpv.js 块游走核心与 begin_tick 的 catalog_mode 参数已验证可承载目录形态（补丁可复活），若未来出现"装配份额真实变大"的负载形态再启用。

## 2026-10-11 补记：信封字符串直构——逐代际片段缓存免逐玩家 Value 重建（现行实现）

【机理】散页证伪后归因收敛：project 62ms 中信封 ~50ms（82%），成本在 `kkpv_envelope_for_viewer` 逐玩家 300 房 Value 构建（300 Map × 50 玩家 churn）+ 事件 from_str 往返 + 整树二次序列化。本片把信封拆为**逐代际片段缓存**：`EnvelopeFragments`（generation 键控，一切内容变更随代际失效）预序列化逐房片段——不可见头（键/名称/公共状态）、可见前缀（+`"energyAvailable":`）、可见中段、可见后缀（能量容量后到 events/闭括号，事件段与 Value 路径同源 from_str→to_string 往返保证转义一致）；逐玩家只拼接片段+逐玩家数值（time/user/gcl/visibleRooms/能量两键）。产物与 `to_string(kkpv_envelope_for_viewer(...))` **逐字节一致**（新增字节锁 envelope_string_matches_value_serialization：可见/不可见、状态/无状态、事件含病态条目、观察者能量 0、缓存复用、换代失效）。

【接线】世界源 trait 分离 `kkpv_envelope_string_for_viewer` + `kkpv_bundle_bytes_only_for_viewer`（束装配拆出信封，与合并路径同源同字节；默认 None 供测试替身回退）；execution.rs KKPV 臂直构路径在分组共享块**之前**：信封字符串 + 分组命中即取 Shared 句柄 / 未命中经 bundle_only 装配后发布，market/power 逐玩家序列化后按顶层键序拼接（rooms 之后，与 Value 插入路径 IndexMap 追加序一致）；注入层 `Value::String` 哨兵直用免整树序列化（世界 JSON 恒为对象，字符串值只可能来自本直构路径）。直构不可用时回退合并路径（分组共享语义保持）。

【ABBA（冻结 release、2 丢弃+O,N,N,O、5+100、并发 4、同 l1b 夹具；old=5e9abba5、new=aea12526）】**project P50 0.706（−29.4%）相邻对双确认（0.748/1.487），48.4/60.7→36.2/40.8ms**；CPU 0.941（Value churn 消失）；**wall 0.951 但相邻对方向矛盾——本窗序列单调热漂（old1 255.9→old4 309.4，同二进制 21% 漂移），wall 归因作废**，判定以 project 双对一致+标尺复核为准；RSS 峰 1.022 噪声级。热漂教训再确认：窗序后期二进制系统性偏慢，ABBA wall 必须查同二进制跨窗漂移量。

【标尺】compare.py 同窗配对（丢弃+测量）：**测量窗 1.1177×（kk 247.3 vs xxs 221.3）**、丢弃窗 1.1543×（263.3/228.1）；CPU 比 0.708、RSS 峰比 0.650。旅程 1.397×→1.155×→1.2213×→**1.1177×**（kk 绝对 401→343→266→247.3）；分相 project 36.1 / run 97.7 / **settle 111.6（最大相）**；等价签名与既往一致（structures 517/518、move +0.339%）。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-envelope-direct-build/README.md)。

【测试面】kk-world 104（新增字节锁）、kk-runner 全套、kk-runtime embed 102（双通道等价+分组重叠探针保持）、mod_chain 495、sim 106、棘轮 2905/9/0 转红 0。

【对目标的含义】0.9×（本窗口径 199.2ms）还差 −48ms。settle 111.6 为最大相且成本分散（复采样无单点杠杆）——**结构性相位重叠（P4 依赖调度/R03 底座）是剩余主路径**；project 残余 36.1（片段拼接+可见集查表+束装配+market/power）与 run 97.7（V8 玩家代码为主）次之。信封路径已无逐玩家全量重建，投影侧小片空间收窄。

## 2026-10-11 补记：settle 相分段取证——R09 房间并行可行性裁定（KK_SETTLE_DIAG 现行探针）

【背景】信封直构后标尺 1.1177×，距 0.9× 差 −48ms，settle 111.6ms 最大相且成本分散。本片为引擎装 `KK_SETTLE_DIAG`（构造时环境变量定格；逐拍分段 Instant 差分——prepare/clone/settle/hook/interroom/nukes/market/power/writeback/validate/into_prepared/commit，耦合计数随共享候选既有借用累加零额外锁），B0 短窗取子相位构成。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-settle-phase-diag/README.md)。

【读数】settle 126.2ms = 规则执行 48.8 + commit 19.1 + 候选克隆 11.4 + validate 全对象扫 9.3 + 预编码快照克隆 9.0 + prepare 5.6 + nukes/market/power 14.3 + interroom 4.6（分段和 122.65 交叉核对一致，差为房循环脚手架）。**耦合面：id_allocs=0、claim_calls=0、interroom_moves=0、portal_moves=0——B0 稳态顺序敏感耦合为零**；gcl 220.8 次/拍、power 5.7 次/拍均交换律累加；事件按房序拼接且引擎外无消费者；mod namespace 房相零写入；随机源生产路径本就房名+ID+tick 派生。

【裁定】房间循环（clone+settle+hook=69.2ms，55%）四路并行预算 ~50ms——**恰好覆盖至 0.9× 的差距**；通用语义需两处确定性构造：ID 按房预分区（B0 零触发但通用路径必须）、claim 串行窗（同拍他房 gcl 累加后 claim 的读窗）。附加发现：① 预编码 hook 的快照克隆是每拍第二次整树克隆（并行后隐藏、远期 Arc 房消除）；② prepare 内安全模式持有人为全对象扫（160k，可索引化）；③ validate 9.3/commit 19.1 各自成后续小片。下一片：R09-lite 逐房并行结算原型（先 B0 形态：并行 worker 池 + 房序确定性归并 + 交换律累加合并；ID/claim 通用构造随后补）。


## 2026-10-11 补记：R09-lite 逐房并行结算落地（现行实现）——settle 相 −20ms 与三个实现陷阱

【机理】取证片裁定可行后落地：settle_rooms_stage 资格门（工数≥2 ∧ 派生随机 ∧ 无 claim 意图 ∧ 无宿主槽）通过时主线程克隆房候选、连续块分片给 scoped 工作线程逐房结算、房序归并；gcl/power 交换律累加跨线程 Mutex、事件房序归并、错误取最低房序与串行首失败同消息。**串行回退**：并行段触碰 ID 分配/claim 读候选即恢复共享态耦合快照后整拍重做，语义与纯串行完全一致；工数自适应 min(4, 房数/16, 核数−3)，2 核部署机自动串行。类型手术：规则表与贡献链 Rc→Arc+闭包加界、TickState→Arc<Mutex>、宿主执行器移线程本地注册表。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-parallel-room-settle/README.md)。

【三个实现陷阱】① **宿主执行器注册表泄漏**：登记不随引擎销毁退役会让已销毁 V8 realm 保持存活，冷重启后新 realm 崩 HandleScope（automatic_disable 冷重启测试打回）——HostedRegistryGuard 字段守卫随引擎销毁/换装退役，首次 impl Drop for Engine 禁止全部字段移动被编译器打回（改字段级守卫）；② **libc 0.2.190 未公开 pthread_set_qos_class_self_np**（QoS 降 E 核试验），自声明 extern 可用，但 QoS 前提被实测证伪后整体撤回；③ **run 相"膨胀"是当日热态不是并行代价**——QoS 窗 run 未变、ABBA 丢弃对 run 完全一致（106.8/106.8），跨窗 run 波动 ±20ms 全为热序噪声；跨日绝对值对比必须带热态注记。

【ABBA（old=71a37eb）】settle P50 107.5/115.2/160.3 → **85.4/91.4/97.6（相邻对双确认+丢弃对一致，−20~−25ms）**；凉丢弃对 wall 249.1→229.0（run 相一致）；**CPU core-ms +12~18%**（以核换墙钟；对 xxs CPU 比仍 0.69-0.83，公平门保持）。等价：单元字节锁 5 项（并行/串行逐房快照逐字节一致、回退一致、三重资格门）、B0 端到端 300 房终态逐字段全等、耦合面并行前后一致、棘轮 2905/9/0 转红 0。

【标尺与定位】当日标尺多窗：测量 1.1651（kk 249.6/settle 95.2）、丢弃 1.1196、W2 1.1074；末尾双窗热毁（kk 381/413）废弃。settle 相自昨日 111.6 稳定落至 95-97，但当日串行对照同受热态影响、跨日绝对值不可比——**wall 兑现裁定留给凉窗多窗中枢**（验收口径本就要求）。下一片候选：① commit 19.1ms 与 validate 9.3ms 全局段压缩（settle 相内剩余大头）；② run 相（111-117 当日口径）——V8 侧无新杠杆，等价于等待project/run 与 settle 的跨 tick 房间级流水（R09 完整形态）；③ ID 预分区让真实负载（spawn 频繁）也走并行（当前回退门对活跃分片常触发）。

## 2026-10-11 补记：commit 信任路径与旧房异步释放 + 凉窗标尺中枢 1.0993×（现行实现）

【机理】取证发现全对象校验每拍两遍（引擎 validate 段 + commit_tick_rooms 内 validate_room_objects），commit 19.1ms 段的大头是重复扫描。三刀：validate_room_objects 拆对象不变式/域状态两段（管理/恢复路径组合语义不变）；commit_tick_rooms_prevalidated 信任路径（调用方已对同一候选全扫，免重复对象扫，域校验+覆盖/地形/游标自检保留，契约=validate 与提交间候选不变异）；旧 300 房 dealloc 移后台线程与下一拍投影窗重叠。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-commit-compression/README.md)。

【归因】分段：commit 19.1→10.5（同窗其他段热膨胀 40% 下仍 −45%，凉折算 ~7.5ms）。凉态 ABBA（old=9446121f）：settle 六窗全低（79.6/85.5/122.5 vs 72.7/77.0/88.2）相邻对双确认 p50 比 0.720；wall 相邻对受热序污染作废。等价：kk-world 106（新增 prevalidated 自检面）、跨二进制 300 房终态全等、棘轮 2905/9/0 转红 0。

【凉窗标尺（12 分钟冷却，1 丢弃+3 测量）】中枢 **1.0993×（kk 254.0）**、凉丢弃 232.8/1.1147；**settle 旅程 111.6→95-97→78-82，两片 −30ms 精确兑现**（凉丢弃 kk 263.3→232.8）。窗口序列单调热化再现（kk 233→280、xxs 209→265，xxs 热敏更高→热窗比值反而低至 1.0565）——中枢口径的必要性第三次实证。**距 0.9×（凉态 ~188ms）差 ~45ms：run 相（105.7）现为最大相**，settle 已压至 78-82（剩余 commit ~7.5/validate 9.3/全局段 ~29）。下一片候选重排：run 相 V8 侧新杠杆（JS 解码装配已两刀，残余为玩家代码本体）已窄；结构重心转向 ① project 相 42.9（信封后残余=market/power 逐玩家序列化+装配+可见集查表）与 ② R09 跨 tick 房间级流水（project[N+1] 只依赖房 R 的 settle[N]——把逐房投影挪进并行结算工作线程内联，吃掉 project 大部）；③ ID 预分区（真实负载并行资格）。

## 2026-10-11 补记：克隆-结算流水重叠（现行实现）

【机理】并行结算的克隆相位屏障（先全克隆再连续块分片）改单队列流式：主线程克隆一间即交付 `Mutex<Receiver>` 共享队列一间，工作者取房即结算；完成消息经独立通道回传房序索引，合并严格按房序（完成序不进产物），故障注入按房序入合并。该流式通道即 R09 跨拍流水的基建。[证据](../../compat/reports/mature-comparison/2026-10-11/50x6-clone-pipeline/README.md)。

【归因】冻结 ABBA（old=a0200227，12 分钟冷却，2 丢弃+O,N,N,O）：settle p50 64.1/64.6→58.3/58.6（比 0.907，相邻对双确认+丢弃对一致）；wall 中枢 207.7→201.0。**代价面首次清晰**：工作者 settle 段（CPU 和）37.9-38.6→46.8-49.3（+9ms，重叠争用：主线程克隆分配与工作者结算读写竞争分配器/缓存）、clone 段 +1.8ms、CPU core-ms +1~2%、RSS 持平。净 wall −6ms：隐藏克隆 ~10ms、争用回吐 ~4ms——重叠优化的收益公式在实践中要再扣"重叠引入的争用成本"一项，与"收益上限 = min(被隐藏相, 窗口)"并列记入方法论。等价：新增并行注入+重复运行确定性 2 测试、300 房终态跨二进制全等、棘轮转红 0。

【位置】settle 相压至 ~58-59（本机当日凉口径）；流式通道就位，下一片 R09 跨 tick 房间级流水（逐房投影内联进结算工作者）直接复用；其"重叠争用"教训对 R09 同样适用——投影内联会加重结算线程的分配压力，预算时按"毛收益 − 争用"折算。

【片后标尺（状态门放行首跑）】中枢 **1.0732**（m2 225.3/209.9；上锚 1.0993）；窗序列 d/m1/m2/m3 = 1.2503/1.1609/1.0732/1.0671——凉窗最差、热窗最好，xxs 墙钟热敏远高于 kk（181→232，+28%），比值-热态反比第四次实证。CPU 比 0.85-0.96、RSS 峰比 0.62-0.65（公平门与内存门保持）。**距 0.9× 约 −36ms：R09（project 34-43ms）压线，重叠争用扣减后可能不足——ID 预分区与投影残余为后备**。冷却协议升级入档：状态门（pmset 无节流+校准带）替代盲等，基准先行、重活排后，配置级 A/B 用 interleave 免等。
