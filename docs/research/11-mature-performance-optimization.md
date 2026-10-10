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
