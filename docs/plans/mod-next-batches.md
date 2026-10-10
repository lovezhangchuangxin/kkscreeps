# Mod 后续开发任务与接手台账

本页只维护**当前状态、执行队列、未完成范围和最近验证**。长期目标与全量领域范围见[完整玩法计划](mod-complete-gameplay.md)，现行机制见[架构索引](../architecture/README.md)，已完成批次的过程见[历史记录](archive/mod-history-2026-10.md)。G01–G30 是稳定任务编号，不是串行开发顺序。

## 当前接手位置

- 2026-10-10 玩家 V8 堆上限默认 64MiB + 宿主配置面（本批未提交，随批提交）：`kk_runner::DEFAULT_HEAP_LIMIT` 256MB→64MiB（用户拍板；懒分配+近堆回调使上限是过量分配保险丝而非预占，正常玩家实际堆占用远低于此，长存批 bot Memory 峰值 ~26.5KB 量级）；`EmbedEngine::set_player_heap_limit`（16MiB..=4GiB，与检查点工作者身份域同界，创建分片时只补未显式注入配置的玩家——switch/检查点恢复的重建路径按生效配置回注不重复套用）；sim 场景字段 `player_heap_limit_bytes`（报告记录生效值；续跑硬约束：工作者身份随检查点恢复，场景声明值与任一恢复工作者不一致时启动前拒绝，不静默忽略）；架构文档 sandbox.md/local-simulation.md 同步。官方语义限额（Memory 2Mi UTF-16 码元/segments 100KB×100 段 10 活跃/ISM 100KB）保持常量不做配置——玩家可见契约，放开破坏官方兼容。同日用户修订容量目标：**50 玩家×6 房（总 300 房不变）**，ADR-018 与 D 切片条目已加日期注记。测试：kk-runner 默认值钉、scene 边界钉、embed_tests 新 heap_limit_family（101 场景，默认落点/宿主覆盖/显式注入优先/恢复按身份回注/边界）、sim_tests 新 e2e 两例（字段达工作者与报告、续跑漂移拒绝 exit 1）；无参数 `pnpm run check` 通过（首次 Clippy 两处机械错误已修）。

- 2026-10-10 D 切片：满员 isolate 足迹实测 + JSON/KKPV 双通道 A/B（已提交 `a4141f5e`，证据 compat/reports/capacity-isolate-probe/）：mod-player-probe 扩到 1..320 玩家（房间 20 列网格分布防世界越界）+ 里程碑 RSS（建分片前/后、放置后、热身后）。macOS 口径结论：**每玩家边际常驻 ≈8.5MiB**（isolate+线程+世界份额，线性：分段边际 9.0/9.0/8.5MiB；100→300 差分）；**JSON 通道分配 churn 是 O(玩家×房间)**（代码推算每房地形 base64 3336 字符、占视图字节约 97%；驱动 project 时序超线性 α≈1.8-2.1；100 玩家热身后 2807MiB/峰值 4175MiB；RSS 保留受 GC/分配器调节非全程超线性——人均热身增长 11.2/20.1/11.7MiB @ 50/100/300 玩家），根因是 `world_json_impl` 把全服已注册房间地形嵌入每玩家每拍投影；**KKPV 在 100 玩家全面反超**（project -27%/run -45%/RSS 峰值 -36%，共享地形块摊销）——单玩家下 KKPV 慢（长存批实测）与满员反超并存，交叉点在共享块被多消费者摊薄。**对 2GB/100 玩家目标（新容量口径）的判定：当前实现不达标且判定保守**（夹具 1 房/人，目标口径人均 3 房/300 房下 churn 再 ×3 只会更差；结构 884MiB + JSON 热身增长 1.88GiB 已超 2GiB，KKPV 100 玩家峰值 2668MiB（2.60GiB）自身也超）——三项路径（生产默认 KKPV、JSON 地形嵌入改共享/按需、isolate 生命周期管理）方向有 A/B 与归因支撑但**无量化落点（削减需求 0.60-0.74GiB、保守化整 0.8GiB，未验证够用）**，容量验收在 Linux 服务形态复测后定论；300 玩家本机可跑（wall p50 413ms≈2.4 tick/s）。边界如实：绝对 RSS 是 macOS 分配器口径（Linux 容量验收须复测）、窗口 100 拍未见 GC 稳态（长窗复测留容量验收）。后续工程项排入服务线：通道默认值、地形嵌入改造、isolate 生命周期。**（同日注记：容量目标修订为 50 玩家×6 房、总 300 房不变——结构足迹按玩家数减半（≈8.5MiB×50≈425MiB）、JSON 通道 churn 随玩家数下降（50×300）、KKPV 共享块摊销随总房数不变（300）；本条的 100 玩家口径不达标判定不直接平移新口径，Linux 容量验收按新口径复测后定论。）**

- 2026-10-10 输入日志 v2（A 摘要化 + B 流式化 + 目录恢复模型重设计，已提交 `035c1d07`）：**A**——玩家 Memory 出日志改存 `raw_memory_sha256`（实测 30KB/拍→~1KB/拍，Memory 整串占 25KB 且随拍数无界增长；它是玩家输出回显而非结算输入），`LOG_VERSION` 1→2（无迁移，v1 拒读）；回放 `rebuild_writeback` 恒 None（结算端保持检查点值），核对改「直跑终态每玩家 Memory 的 sha256 == 窗口内最后非空摘要」（`verify_logged_memory`），逐位比较剥离玩家 Memory（`strip_checkpoint_memory`）。**B**——`read_input_log` 整读改流式 `scan_input_log`：一行前瞻保撕裂语义（末行三签名收敛/中段报行号/缺换行非法 UTF-8 尾块归因撕裂），增量链校验，按 `Retain`（None/All/FromCursor/FromTick）驻留后缀；三消费方迁移（续接挂载零保留、运行后摘要零保留、回放核对只留后缀），崩溃恢复的整读+`segments_after_cursor` 同步迁移。**恢复模型**——摘要化暴露 v1 恢复的隐藏耦合：后缀重放依赖日志携带 Memory 全值，否则「世界到日志尾、Memory 停在检查点」形成不一致对（auto_checkpoint 实测复现）；改**回滚+游标处原子截断废弃分支（`truncate_after_cursor`）+丢失拍重执行**（官方恢复模型：候选提交原子性保证无半拍状态，重执行与重放在世界推进等价且世界/Memory 恢复到同一时间点），`recovery.reexecuted_ticks` 入报告，成功判据=提交数==请求+重执行。验证：kk-runtime/kk-cli 全测试面绿（embed_tests 100 场景、sim_tests 104 例含迁移后 auto_checkpoint 全家）、无参数 `pnpm run check` 通过。三路后台复审（流式扫描正确性/Memory 摘要语义/恢复模型与调用面）已回收并当批清偿全部发现：①必修「末行撕裂签名后跟空白块误报 BadLine」（前瞻改为只在后继**非空**行刷新，末行口径回归最后一个非空行）+ truncate 三项加固（kept 后置校验堵 TOCTOU 静默不截断、复制点游标段 (seq,sha256) 自证、陈旧 `.truncate-tmp` 孤儿预清理使崩溃窗口后恢复一次成功）；②③共同必修「`recover_latest` 传 `time: cursor.last_tick+1` 使检查点时间-游标绑定恒真」——改传检查点自身世界时间恢复 v1 真实交叉校验（段 tick=结算前时间，正常保存恒 `cursor.last_tick+1==检查点时间`）；测试补齐 truncate 直测/配对错误族流式等值（控制组+跳拍/回退）/verify_logged_memory 负向与双玩家矩阵/摘要形状拒绝/v1 形状夹具拒收/reexecute==0 边界/二次恢复收敛；文档修正 FromCursor 表格「（含）」、行号非空行口径、1 段=1 拍前提、跨目录截断效应、Memory 与 segments/ism_local 不对称理由，并清理 v1 时代陈旧注释四处。清偿使 `input_log.rs` 触及 1032 行（超过 1000 行源码规模硬门禁），按职责拆出读取域 `embed/input_log_scan.rs`（流式扫描/保留策略/游标截断），写入器与协议常量留在 `input_log.rs`，模块导航已同步。日志段 86400 拍体积外推 ~86MB/份（v1 为 2.6GB）；segments_out/ism_local 保持全值（segments 重放兼做清洗规则复验，量小，登记为已知体积贡献者）。生产持久化姿态按用户拍板固化为 ADR-018（周期检查点权威/输入日志为显式审计件/RPO=间隔可配，服务侧接线随 G27.b/G28；用户 2026-10-10 细化：回放/回收/审计类近期历史能力走 Mod 有界窗口支持，不进核心服务）。排期（用户问询后明确）：v2 提交 → 长存批六阶段补跑（v2 验收腿）→ D 满员 isolate 足迹实测（服务线动工前完成，结果决定 2GB 路线；容量目标 2026-10-10 定稿为 9 区块地图/≥100 玩家/人均 3 房/总 300 房，50/100/300 阶梯里 100 为运行点、300 为 3 倍压力点；D 一并做 JSON/KKPV 双通道 A/B——长存批实测单玩家下 KKPV 投影 0.77→1.39ms/玩家执行 2.87→4.11ms（settle 持平），共享块摊销为 1 是结构性慢，「多玩家反超」尚无实测数据，100 玩家下仍输则 KKPV 须重估）。

- 2026-10-10 长期存活批收口（夹具 `0f87ed07`；六阶段已在日志 v2 格式上补跑全绿，证据归档于 compat/reports/gameplay-tooangel-survival/）：夹具/有界观察器/包装器——spawn 落 TooAngel 布局锚点 (5,5)（控制器自动 (1,1)、misplacedSpawn 不触发），观察器全部持久化统计有界（孵化名字表替代 seen 全集、采样数组封顶、按角色前缀归类出生、Memory 字节采样曲线），包装器阈值按 86400 拍全长 debug 探针标定（RCL5@66728、storage 24225、link 67900、8 类角色 636 出生/628 死亡、采集 1.68M/升级 756k/建造 154k、bot Memory 峰值 29.5KB、零错误零跳帧、含逐位回放 749s≈115 拍/秒）。研究结论：kk 为单房夹具播种 GCL2，brain 层扩张链真正卡点是无可占的已侦察房（scout 冻结既知约束），nextroomer/claimer 不会出生、不该断言；creep 名字生日碰撞的 `spawnCreateCreep: -3` 日志走 `Room.prototype.log`（无错误关键字），不触发现察器错误正则。六阶段曾在日志 v1 体积问题下按用户指令暂停（30KB/拍使单段 2.6GB、六阶段约 10GB），日志 v2 交付后在 release 上补跑全绿：JSON/KKPV 各连跑 86400 拍 + 43200 截点冷恢复，每段 --verify-replay 逐位回放，144/144 检查通过；业务指标与 debug 标定一致（RCL 爬升至 5、storagefiller/upgrader 上岗、scout 45 次出生、采集 1.68M/升级 764k/建造 154k、bot Memory 峰值 ~26.5KB、零异常零跳帧）。v2 段体积实测 ~1.5KB/拍，四份日志共 ~450MB；86400 拍段 release 实测 ~12-14 分钟。实测补充：KKPV 通道单玩家下投影 0.77→1.39ms/玩家执行 2.87→4.11ms（settle 持平，共享块摊销为 1 的结构性慢，已登记 D 切片做满员 A/B）。输入日志 v2 方向已获用户认可：A 摘要化（Memory 出日志改存 sha256，~30KB/拍→~1KB/拍，verify-replay 的 Memory 比较改摘要级）+ B 流式读取（逐行解析+增量链校验+游标后缀加载）先行；C 生产不写输入日志（持久化=周期检查点，RPO=间隔，官方/xxscreeps 同级取舍）跟在 embed 服务形态清楚后；D 满员 isolate 足迹实测排期。部署基准（用户拍板，2026-10-10 定稿）：2GB 内存、9 区块大小地图（9×9 房/区块）上 ≥100 玩家、人均 3 房、总 300 房；性能高内存小、不过度设计；官方引擎与 xxscreeps 持久化盘点证据见本条记录。（同日修订：≥50 玩家、人均 6 房、总 300 房不变，现行口径见 ADR-018。）

- 2026-10-10 review 修复（已提交 `d354f9ff`）：房间状态种子保留原值，玩家面与结算消费者按墙钟严格到期求有效状态；普通边缘跨房和 `isRoomAvailable` 拒绝有效 closed 房；powerBank 生成器补有效 closed 过滤（novice/respawn 的官方 db status 仍为 normal，继续参与扫描）。已补状态边界、portal、跨房、核弹、生成器和检查点恢复回归；无参数 `pnpm run check`、release CLI 构建、兼容适配器构建与 release ratchet 均通过，ratchet 为 2905 passed / 9 failed / 0 skipped、无新增转红（既有未命中豁免警告）。本轮继续深度复审差异，后续消费者依执行队列推进。

- 2026-10-10 `e5249ff2^..HEAD` 代码复审（已提交，随 `d354f9ff`）：修复 `tick({random})` 成功拍的输入日志缺失随机值问题，`advance` 段逐拍保存实际消费值、回放注入并校验消费完毕；新增矿物再生的日志回放等值用例。定向 `pnpm run check -- kk-runtime --test embed_tests` 与无参数 `pnpm run check` 已通过（不含 doctest；本次未改 doctest）。复审发现的房间状态三处消费者问题已按上条修复；其中 powerBank 只排除有效 closed，原“非 normal 全部排除”的判断已依据官方 db status 更正。不将本轮标作全领域完成。

- 2026-10-09 review 修复（已提交 `2b68002`）：invaderCore 放置的孵化守军初始 `ageTime=0`，出生后再起算普通寿命；stronghold 部署绑定 `decayTime` 的路径保持原值。新增超过 1500 tick 孵化回归；定向 `pnpm run check -- kk-runtime --test mod_chain_tests --filter invader_core_long_incubation` 与无参数 `pnpm run check` 均通过（不含 doctest；本次未改 doctest）。其余 G02/G03 与 NPC 后续消费者不受此修复影响，下一步按当前执行队列继续。

- 更新日期：2026-10-10；控制器 INVULNERABILITY 消费者已提交 `d48d6ea`；Invader core 生命周期首片已提交 `2b68002`；Stronghold 布防首片已提交 `4f7dd2bd`；raid 调度首片已提交 `e316a07`；powerBank 生成与消费者首片已提交 `e5249ff2`；边缘环绕+核弹簇已提交 `9ff9d2e`；弃用告警已提交 `c39777a`；tick({random}) 注入已提交 `038c1aa`；InterShardMemory 本地段与 CPU shardLimits 已提交 `5daaa88`；动作记录导出与 captureActionLog 已提交 `4cef2ba`（套件最后 21 个跳过例全簇转绿、跳过清零）；TooAngel 防御战斗与冷恢复（敌方脚本坦克 + 真实防御链，520 tick 六阶段）已提交 `c3c2d68`；TooAngel 多房进攻与冷恢复（敌方预留 + attackunreserve 跨房出击，2900 tick 六阶段）随本提交交付。最近完整门禁 Rust 全绿；最近 release 棘轮 2905 passed / 9 failed / 0 skipped（**99% 检查点达成并超出 20 例**）。TooAngel/Overmind 经济六阶段与防御战斗为既有入库证据；本批未重跑经济长测。
- 主线：M2B 默认玩法兼容；M2E lab/boost、M2F factory、M2G nuke、M2H.a power 基座、deposit 采集链、observer 限时视野已交付。**M2H.b 19/19 powers 全部接通**。
- 当前接手：G02.d3 结构/rampart 计划级通行及默认玩家/NPC 寻路已提交 `fd27fbd`；G03 障碍追加后的同源链预演已提交 `3f3d9a7`；障碍摘要的安全模式主人已提交 `41de432`；控制器零升级量修复已提交 `10eb812`；G27.a 独立异步后台宿主已提交 `b25e280`；控制器 INVULNERABILITY 三处消费门已提交 `d48d6ea`；invaderCore 生命周期首片已提交 `2b68002`；stronghold 布防首片已提交 `4f7dd2bd`；raid 调度首片已提交 `e316a07`；powerBank 生成与消费者首片已提交 `e5249ff2`；边缘环绕+核弹簇（portal 传送/衰变/核爆免伤、roomStatus 种子域与 getRoomStatus/launchNuke 门、worldSize 活算与 map/market/terminal 三处环绕计费、检查点 v9）随本提交交付，生产者侧仍缺地图生成、战术 AI（maintainPopulation/refillTowers/focusClosest）与 create-creep 等意图。逐房通行粒度、摘要增强、完整仲裁替换仍未完成。
- 本次完成：JSON/KKPV 各 200 tick 持续战斗、100+100 tick 跨进程冷恢复和完整检查点回放；实际位置/hits 证明移动、受伤与恢复，双通道及直跑/续跑世界与 Memory 等值。TooAngel v6 双通道 2000 tick 连跑及 1000+1000 tick 冷恢复六阶段复验通过。
- 剩余：bot 对战、长期存活与 Overmind 旗标出兵；地图生成与 G21 Mod 世界事件；stronghold 战术 AI/create-creep 意图；walkTo/Keeper lastMove 按实际 stronghold 消费者接线；G02.d3 逐房/摘要后续、G03 完整仲裁替换与 G27.a。NPC 场景不标 M2B 或 G21 完成。
- 兼容率 **2905/2914≈99.7%（99% 检查点 2885 已达成并超出 20 例；跳过清零）**，9 失败=8 既有官方对齐豁免+1 SHAPE-GAME-002 形状互斥豁免。剩余提升面在豁免例（须官方语义仲裁或上游形状通道）。下一队列：输入日志 v2（用户拍板优先）、bot 对战/长期存活补跑、G02.d3 逐房、G03 完整仲裁替换、G27.b outbox、玩家面 actionLog 投影/prelude 与渲染 feed 消费。
- Mod 扩展：移动策略、对象/建筑/资源、库存/耐久/受击、作者 CLI/类型、输入回放和检查点已有基础；未完成范围以任务表为准。默认玩法已有消费者不代表对应 Mod 替换能力已经开放。
- 网络服务、持久 storage 后端与真实游戏客户端仍未实现；renderer R0–R6 已交付，R7 最终矩阵/长测未收口。现有 bot-check 不能代替持续推进世界中的可玩验收。
- M2B 验收标准（2026-10-09 调整）：screeps-ok ≥95% 为中间检查点、≥99% 为后续覆盖目标；最终完成要求约定范围内功能全部通过验收，剩余例外逐项有明确范围、证据和可审查理由，见[路线图](../roadmap.md)。

## 当前执行队列

| 顺位 | 任务 | 首片与完成条件 |
|---|---|---|
| 1 | 输入日志 v2（用户拍板优先） | A：玩家 Memory 出日志改存 sha256 摘要（~30KB/拍→~1KB/拍），verify-replay 的 Memory 比较改摘要级；B：read_input_log 流式化（逐行+增量链校验+游标后缀加载，撕裂语义不变）。完成条件：embed/replay 全部消费方与测试迁移到 v2，长期存活批六阶段在 v2 格式上全绿 |
| 2 | 零散红逐例归因与修复 | 8 条既有官方对齐豁免保留证据；Invader 自动远程攻击已交付；零步相邻近战已交付；普通 Invader 路径建筑改向/追击/治疗已贯通；NPC 持续战斗已验（Source Keeper 同房巡守/战斗、stronghold 布防、raid 生成已交付），剩余地图生成/powerBank 生成另拆；每例有官方源或套件仲裁证据 |
| 3 | 真实 bot 验收与 G02.d3 后续 | TooAngel 单房经济/死亡后补员/冷恢复已验；Overmind 单房经济/寿命/冷恢复已验；TooAngel 防御战斗（塔/防御者/安全模式）已验；TooAngel 多房进攻（attackunreserve 跨房出击击杀 reserver）已验；剩余 bot 对战、长期存活（夹具已交付待六阶段）与 Overmind 旗标出兵 |
| 4 | 玩家资源配额配置面（2026-10-10 用户问询提出） | 首片已随堆上限批交付：默认 64MiB + `EmbedEngine::set_player_heap_limit` + 场景字段 `player_heap_limit_bytes`（含续跑身份硬约束与 e2e）。剩余：服务级配置入口随 G27.b/G28 服务形态接线；同族 cpu_limit/bucket 是否一并暴露随服务配置面设计定。官方语义限额（Memory 2Mi UTF-16 码元/segments 100KB×100 段 10 活跃/ISM 100KB）保持常量不做配置——玩家可见契约，放开破坏官方兼容 |
| 独立候选 | G02.d3 后续、G27.a | 前者继续逐房粒度与摘要增强（结构/rampart 已贯通）；后者提供异步后台生命周期，随后才接 storage/outbox/网络 |

维护优化已完成的轮次不作为新功能前置。任务首次实施前按业务闭环拆片；G30.a 覆盖清单与 G30.d 同功能参考随领域滚动，不等最终收口才补证据。

## M2H.b power 进度（19/19 收口）

公共效果表、施放共用门、JSON/KKPV 投影、当前格式检查点和候选回滚已实现，机制见[游戏循环](../architecture/game-loop.md#power-效果链当前首片)与[检查点](../architecture/checkpoints.md)。

| 状态 | Power / 消费者 |
|---|---|
| 已完成 19 项 | GENERATE_OPS；OPERATE_TOWER / DISRUPT_TOWER；OPERATE_SPAWN / DISRUPT_SPAWN；OPERATE_LAB；OPERATE_POWER；OPERATE_FACTORY；REGEN_SOURCE / DISRUPT_SOURCE；REGEN_MINERAL；OPERATE_STORAGE；OPERATE_EXTENSION；OPERATE_TERMINAL / DISRUPT_TERMINAL；OPERATE_CONTROLLER；FORTIFY / SHIELD；OPERATE_OBSERVER |
| 待实现 | 无 |

剩余任务的具体门序和量值须核对锁定的官方源码与 POWER_INFO：

- 资源/容量：storage 容量与库存消费者同源；extension 为即时灌能，按目标房主、距离顺序和整批预算结算。
- observer/OPERATE_OBSERVER 已交付（本批）：效果即无限射程、标记生命周期一拍、双通道可见集并入。
- 战斗：FORTIFY/SHIELD 联动伤害、拆毁、临时 rampart 耐久与衰减；EFFECT_INVULNERABILITY 的伤害/dismantle/attackController/nuke/rangedMassAttack 等消费门同片盘点，其生产者属于后续 invader core/NPC 域，不计入 19 个 power。

## 任务台账

“部分完成”必须保留父项缺口；“未开始”表示 Mod 能力尚未交付，不否定该领域的默认玩法实现。依赖须在开始任务时核验。

| 编号 | 任务 | 状态 | 已完成与剩余范围 / 入口 |
|---|---|---|---|
| G01 | 移动竞争优先级 | 已完成 | [竞争排序](../architecture/movement.md#竞争排序)；排序能力不等于完整仲裁 |
| G02 | 移动准入 | 部分完成 | .a 准入、.b 障碍追加、.c 请求预检、.d1 静态放行、.d2 双端玩家门、.d3 墙与静态对象/rampart 通行声明已贯通；安全模式主人摘要已接通；剩余逐房粒度及其余摘要增强；[移动契约](../architecture/movement.md) |
| G03 | 完整房间仲裁 | 部分完成 | .a 请求视野/链预演与 pull 登记已开放；障碍槽第二预演已开放；剩余完整仲裁替换；[移动契约](../architecture/movement.md) |
| G04 | 移动代价与效果 | 部分完成 | .a 代价槽已交付；剩余疲劳恢复、重量评分要素；[移动契约](../architecture/movement.md) |
| G05 | 作者 CLI | 已完成 | mod init/test；[本地模拟](../architecture/local-simulation.md#作者包模板)，增强按实际痛点另拆 |
| G06 | trace 与故障复现 | 部分完成 | .a 槽 trace / --mod-trace 已交付；剩余过滤、停用阈值、一键复现；[trace](../architecture/mod-trace.md) |
| G07 | 玩家预检策略 | 部分完成 | .a 战斗射程、.b ext 压制声明已交付；剩余 EXECUTION_ORDER 策略化和参数策略；[预检规则](../architecture/player-precheck-rules.md) |
| G08 | 建筑字段与生命周期 | 部分完成 | .a 字段/tick 已交付；剩余 Mod 建筑 RCL、逐资源容量、专属 FIND、生产事件；[建筑契约](../architecture/mod-buildings.md) |
| G09 | 自有对象能力 | 部分完成 | .a 库存、.b 耐久/死亡、.c 结构受击、.d 声明反击已交付；实际伤害联动反击、owner、维修/掉落等增强按需拆片；[库存](../architecture/object-store.md)、[耐久](../architecture/object-durability.md) |
| G10 | 可编程生产与动作 | 未开始 | 依赖 G08 与资源/配方；具名动作、多配方、进度、冷却和原子支付/产出 |
| G11 | 配额与容量策略 | 部分完成 | .a 内建逐型逐级配额已交付；剩余资源容量、逐房/动态配额；[配额](../architecture/structure-quota.md) |
| G12 | 采集与再生策略 | 未开始 | 依赖实际采集/再生消费者；产量、消耗、耗尽、周期恢复和竞争 |
| G13 | 孵化、寿命与死亡 | 未开始 | 分拆成本/耗时、出生/出口、寿命、死亡产物；含孵化门槛 |
| G14 | 塔动作策略 | 未开始 | 依赖伤害/治疗/受击边界；公式、消耗、距离、改向与事件 |
| G15 | 控制器与成长 | 未开始 | 对象/账户候选边界；升级、占领/预定、控制器周期与 GCL |
| G16 | lab/factory 策略 | 未开始 | 依赖 G10/G11 相关能力；分拆化学/工厂，贯通配方、预检、库存、冷却和事件 |
| G17 | 跨房 Mod 资源 | 未开始 | 依赖 terminal 全局结算与资源目录；费用、两端库存、目标房和跨房原子性 |
| G18 | 市场扩展 | 未开始 | 依赖 G17 与订单/账户边界；余额、余量、费用、记录和游标 |
| G19 | 房间生命周期 | 未开始 | 分拆对象 preTick/房间阶段顺序与资格、受控房间操作；双房失败不发布 |
| G20 | 玩家/世界生命周期 | 未开始 | 依赖对应房间阶段与串行屏障；玩家/世界阶段、服务端命令、账户和跨域访问 |
| G21 | 地图与世界事件 | 未开始 | 依赖 G19/G20、G22.b；初始化/NPC/周期事件，地形/寻路缓存与恢复同源 |
| G22 | 执行身份/时间/RNG | 已完成 | .a 状态清单、.b 确定性时间/RNG、.c 执行身份；[状态清单](../architecture/state-inventory.md)与[检查点](../architecture/checkpoints.md) |
| G23 | 包/schema/state 迁移 | 部分完成 | .a 载荷版本/显式迁移已交付；剩余内容级迁移与 RNG 联动；[检查点](../architecture/checkpoints.md) |
| G24 | tick 边界更新/watch | 部分完成 | .a 无开放轮次的包/API 切换已交付；watch 未开始；[检查点](../architecture/checkpoints.md) |
| G25 | 输入日志与结算回放 | 已完成 | .a 日志协议/保存、.b 回放/完整状态核对；[输入日志](../architecture/input-log.md) |
| G26 | 自动检查点/storage | 部分完成 | .a 周期保存/keep/恢复选择已交付；.b provider 待 G27.a；[检查点](../architecture/checkpoints.md) |
| G27 | 后台服务/outbox | 部分完成 | .a 独立异步宿主与有限只读探针已交付，见[后台服务](../architecture/background-services.md)；.b 耐久输入/outbox 未实现 |
| G28 | Backend 贡献 | 未开始 | .a M3 最小真实服务；.b 路由/订阅/provider，依赖后台与输入边界 |
| G29 | 玩家类型与客户端 | 部分完成 | .a mod types / --check 已交付；.b 真实客户端、.c 资产/渲染/面板未开始；[类型生成](../architecture/mod-types.md) |
| G30 | 预设/覆盖/性能验收 | 未开始 | .a 覆盖清单、.d 同功能参考先滚动；.b 官方预设、.c 内容所有权迁移、.e 最终收口 |

## 子片依赖与实施边界

下表只列关键子片；已完成的前置仍须按实际源码与契约核验。

| 子片 | 硬依赖 | 交付边界 |
|---|---|---|
| G23 内容迁移 | G22.a/c；涉及 RNG 时加 .b | 替代候选迁移，失败保留原世界/计划；缺包与只读导出明确区分 |
| G24 watch | G23 对应更新能力 | 无开放轮次或待恢复玩家缓存；API/计划/工作者一起切换，失败保留原版 |
| G26.b storage provider | G26.a、G27.a | native / JS provider 等价原子发布与读取；异步 storage 属后台域 |
| G27.a 后台宿主 | 现有 VM/预算/已提交读面 | 受控异步生命周期与有限探针 I/O；不依赖 outbox，不直接改玩法 |
| G27.b 耐久输入/outbox | G27.a、G25.a、G26.a | 世界、输入消费游标、待输出记录原子耐久；确认后投递 |
| G28.a 最小真实服务 | G27.b | M3 认证/授权、代码上传、订阅、控制台；玩法写入走受控输入 |
| G28.b 业务 provider | G28.a | 可替换路由/订阅/provider；返回结果仍经过宿主授权 |
| G29.b 真实客户端 | G28.a | 实际 API/订阅消费者；JSON 投影不算图形客户端交付 |
| G29.c 客户端贡献 | G29.b | 可替换资源资产、渲染与面板，包选择和类型声明一致 |
| G30.b/c 内容预设/提取 | 对应能力已开放；存量变化依赖 G23 | 公开 SDK 内容；未选包不被 native fallback 恢复 |

G28 的协议设计和只读原型可提前；玩法写入必须先有输入/提交边界。G27.a → G26.b 与 G27.a → G27.b 是两条后续线，outbox 可先用 native 后端，不等待 JS provider。

### 移动与作者工具

- G02.d3 后续须同时接通 PathFinder、Room.findPath/moveTo、请求预检、仲裁默认与链预演；物理地形查询保持原值，不放开未注册房墙环或动态占格。tick 级 allowStatic/服务器 JS 回调不能同步进入玩家寻路。
- G03 完整替换须覆盖交换、循环、拉拽、阻塞传播、整房候选与拒绝回退。默认路径先选胜者再取消，不自动递补；不得静默截断请求或在排序比较器内执行有副作用 JS。
- G04 覆盖受伤后身体效能、疲劳生成/恢复、道路磨损、工地踩踏与跨房顺序；任意传送另设计能力。
- G06 trace 必须有界，注明记录量/成本/私有数据范围；诊断快照不冒充可回放日志。G07 当领域首片即接必要预检，后续再通用化，不形成循环等待。

### 对象、经济与世界

- G08/G09 贯通声明、建造/创建、字段、预检、投影、事件、索引与恢复；第二种对象不新增专用 Rust 变体。新的 body/effect/能力表示由 G30.a 单列，不假定现有表示永久封闭。
- G09 实际伤害反击先定义减伤/治疗前后基数、来源归因、非递归与同轮追加顺序；不能向已完成 tick 的对象挂队列后不消费。
- G10–G18 逐域确认消费者，覆盖满仓/耗尽、完整资源支付、多人竞争、冷却、全局账户及跨房原子性。孵化用真实 spawnCreep 验收，不伪造 spawning；默认 lab/factory/tower 已实现不等于策略开放。
- G19/G20 明确对象动作、仲裁、tick、房间与世界阶段的相对顺序、访问域和串行屏障；生命周期不默认继承动作式局部拒绝。玩家连接器保持沙箱隔离，不只相信 parallelSafe 声明。
- G21 的初始化与运行期更新分开；地形/投影/查询/寻路缓存 epoch/检查点同源，时间和随机按 G22 协议输入。

### 耐久、服务与收口

- G26 明确 RPO、单写者、发布与恢复选择；检查点不能承诺零丢失。G27.b 覆盖发布前、耐久确认后发送前、发送后确认前崩溃，先用模拟接收端验收幂等/去重。
- 后台响应进入后续 tick；不能直接写正在结算的权威世界。玩家上传代码不能按管理员可信 Mod 装载。
- G29 类型签名须有显式描述，不用全 any 替代；未选包不残留声明，生成物新鲜度进门禁。
- G30.a 展开 config/constants/schema/processor/game/driver/main/terrain/storage/backend/test 的真实消费者，以及 power/effects、observer、NPC、portal/跨 shard、intershardResource、flag/visual/messages 等未显式列出的子片。缺口可新增 G31+，不以父编号齐全宣布全量覆盖。
- G30.d 在重大重构前保存同功能参考，测相同规模的原生/JS/组合成本；不能删参考后才补基准。官方内容提取须明确常量/目录所有权、冻结冲突与默认兼容，native 加速保持公开语义等价。

## 已知限制与复审入口

- G09 的 owner、massAttack/dismantle/维修/死亡产物，以及私有 rampart 的预检/结算差异，仍按[库存](../architecture/object-store.md)和[耐久](../architecture/object-durability.md)的限制选型；不把四个首片交付标成全领域完成。
- G02.d3 历史登记的 worker 配置传递、跨房墙环端到端、allowStatic 与声明叠加等增强项，在相关切片开始时复核；安全模式主人摘要为可解释性增强，孵化门槛归 G13。按[移动契约](../architecture/movement.md)与实际测试核验。
- 控制器 INVULNERABILITY 的玩家/原生攻击与核弹消费门本批补齐；此前“结算全部备好”的登记不覆盖这两条漏门。生产者仍属后续 invader core/NPC 域，G21 未交付。
- upgradeController 零量动作/事件分歧已修复；RCL8 同轮满额仍拒绝，回归见控制器升级边界测试。
- 历史登记过 sim 并行冷恢复子进程偶发失败，隔离复跑通过且后续完整门禁通过；若再现须保留失败报告归因，不能把重跑成功当作已修根因。
- Overmind bot-check 保留基线已有的 1 个 console-error tick；不宣称零错误。keeper/NPC、多 shard 和其余能力门控缺口以最新兼容报告核对，旧批次数字不参与当前决策。
- 本地检查点的文件大小、单写者及耐久适用范围见[检查点契约](../architecture/checkpoints.md)；存储和真实网络服务不能从 embed 能力直接推导为已完成。

## 最近交付与实际验证

最近功能批次：observer/OPERATE_OBSERVER（`2e81e90`，交付记录见该提交）与零散红 G1/G4/G8 批（本提交）：27 例失败逐例归因（vanilla/xxscreeps 双基线交叉：19 实现缺口 + 8 官方对齐豁免，MOVE-COLLISION-007 补登记 ratchet-allow note 归因）。本批修复六例：放置边界校验（旗名 |~ 分隔符拒绝、spawn 缺 owner 拒绝、object-only 类型经 structure 通道拒绝并提示 placeObject、createShard 未知玩家错误文案加 Unknown player 英文 token）、SHIELD 效果条目无 effect 键（官方专用分支 {power,level,endTime}——EffectEntry.effect 可选化，投影/快照/检查点随选通、管理放置 ingress 默认 effect=power 不变）、attackController 事件省略 data 键（push_event 空 payload 整形）。observer：observeRoom 预检门序 NOT_OWNER→房名 regex→RCL→射程（逐轴绝对差 ≤10；效果活跃完全豁免——官方无 10+N 公式）；结算侧复验同门防直注后写 data.observeRoom 标记、预意图阶段清零（官方 processor.js 同形，观察视野恰一拍）；世界级预扫 `observed_rooms_of` 把标记房并入 JSON/KKPV 双通道可见集（被观察房的能量/事件/对象导出与己方房同口径，未注册房查询落空同官方）；OPERATE_OBSERVER 挂 observer（duration 数组按级、无 effect 数组，消费只看存在性）、无效目标静默丢弃不扣费；标记随 Generic data 走既有快照/检查点。附带：intent.rs 触 1000 行门禁拆出 from_json.rs（纯移动+pub(super) 可见性，零行为变化）；放置层对无公开 cooldown 的结构类型拒绝 cooldown spec（适配器契约，套件 cooldown 拒绝用例）。**capabilities.observer 已翻转**：套件 observer 组（13.4 十六例、EFFECT-HOST observer 行、adapter cooldown 拒绝例）与 POWER-OPERATE-003/005 两失败全部转绿。deposit：harvest 的 Deposit 分支（预检 isNearTo/ERR_TIRED；结算量 WORK×HARVEST_DEPOSIT_POWER 含 harvest boost，官方分支无 amount 门——WORK 全熄的 amount=0 仍落动作日志、按当前累计重算冷却并刷新寿命）、超容掉落（资源堆落成当拍按 ceil(amount/ENERGY_DECAY) 衰减）、harvested 累计含 boost、decayTime 刷新 time+50000、冷却 ceil(0.001×harvested^1.2)（>1 才挂）经 pending 表在对象 tick 相落成（官方 _cooldown 临时字段同构，同 tick 后续采集读旧值仍成功）、寿命门 `time >= decayTime-1`（processor.js 移除相为实际边界）；放置归一把相对 cooldown/ticksToDecay 换算为绝对键（与 nuke 同形），查询快照面派生 lastCooldown 并折算 ticksToDecay/剩余 cooldown（rawData 投影通道仍透传绝对键由类 getter 折算，双通道口径自洽）。**capabilities.deposit 已翻转**：套件 deposit 组 44 例全绿（含修复 DEPOSIT-HARVEST-003/006 的放置相对冷却换算、DEPOSIT-006 寿命门、快照往返派生字段）。三路复审（结算保真/测试边界/全局影响）发现项当批清偿：amount=0 无门（去提前返回+引擎意图直注测试锁定结算侧静默门与零量流程）、EVENT_HARVEST 断言移回采集轮（事件 1-tick 生命期）、retry 注释改缓存重放语义、>1 冷却边界用同 tick 双 creep 锁定、到期拍 T=4/T=5 钉边、ERR_NOT_IN_RANGE 距离腿、lastCooldown=0/缺 decayTime→undefined、满仓全量掉落极值、creep.js 陈旧注释、bot 基线 find:114 归因纠正（FIND_MY_CONSTRUCTION_SITES 每轮随机漂移 381↔386，非 deposit 因果——bot 世界无 deposit）。FORTIFY 挂 constructedWall/rampart：非核弹伤害完全免疫（_damage 严格 >，免疫含扣血/摧毁/反击/事件）、dismantle 能量回收归零（SHIELD/FORTIFY/INVULN，>= 边界，伤害照常）、rangedMassAttack 跳过目标（>=）、attack/rangedAttack/dismantle 预检即拒 ERR_INVALID_TARGET（SHIELD 不在预检门）。SHIELD 无 ops 无目标：删自身位工地、过官方 checkConstructionSite 的 rampart 放置子集（同类不重叠/本格非墙/贴边三块边界格全墙）后插临时 rampart（hits=effect、hitsMax=0、nextDecayTime=endTime）；护盾可受伤、dismantle 无回收、衰减 tick 跳过同步并到期删除；POWER_INFO.energy 官方无消费者不扣。EFFECT_INVULNERABILITY 与 FORTIFY 共享全部消费门（生产者属后续 NPC 域）。三路复审（结算保真/测试边界/全局影响）发现项当批清偿：rangedAttack 预检效果门移到射程检查后（官方顺序，超射程且无敌返回 -9）、SHIELD 边界检查移到工地删除后（越界 pc 的工地同样先删）、贴边四角选边改顺序赋值（y 覆盖 x，官方与 room.rs 既有决策同形）、插入不带 notifyWhenAttacked、免伤正路径补塔攻击例（无预检门的唯一可达通道）、到期拍用绝对时间钉住（此前 tick 计数失真未落边界）、dismantle 回收加 carry 部件使 0/1 可区分、massAttack 用有主 rampart 钉 >= 边界、rampart 腿预检与 SHIELD 拒绝路径工地删除副作用补断言、attackController 的 INVULN 预检缺口登记。行为变化：无效果路径逐位不变（免伤/回收/跳过门仅活跃效果命中介入）。damage.rs 现 762 行超 700 复审线，下一批涉及时先拆分。

- 完整无参数 `pnpm run check`：Rust workspace 1428 passed / 0 failed（放置三门的 kk-world 单测、attackController 无 data 键钉扎进 controller.rs）；JS workspace、工具、文档链接与源码规模通过；无 doctest 变更。
- release CLI 与适配器重建，固定 screeps-ok HEAD `34a630e`；全量 2914 例：2789 passed / 21 failed / 104 skipped（≈95.7%），相对 `2e81e90` 基线 +6 转绿 / 0 转红，地板 2783→2789。
- 三 bot 完成 2000/2000/10 tick，无中断/惩罚跳过/missing_api，行为键与基线全等；Overmind 如上保留既有报错。
- 持久证据：[兼容机器报告](../../compat/reports/kkscreeps.json)、[棘轮配置](../../compat/reports/ratchet.json)、[TooAngel](../../compat/reports/bot-tooangel.json)、[Overmind](../../compat/reports/bot-overmind.json)、[WASM](../../compat/reports/bot-wasm.json)。原临时日志路径仅在历史过程保留，不作为长期证据入口。

## 台账维护与每批验收

- 开始/调整范围/结束会话时更新接手位置、任务状态、剩余消费者和实际验证；只记录本批必要摘要。已完成的详细过程退出当前入口，普通修复使用提交说明和测试，不逐批新建交付文档。
- 当前页维护进度和依赖；现行机制回写 architecture，公共 SDK 行为变更更新用法/样例，里程碑范围变化更新 roadmap/完整玩法计划。历史结论不追改。
- 新增写入域当片更新[状态清单](../architecture/state-inventory.md)和检查点/版本策略，验证保存恢复与失败边界。预检、意图、结算与消费者同步接线，不等远期通用任务。
- 可替换玩法至少两种独立 JS 实现、原生对照与组合；工具用多包/保护失败案例，迁移/日志/storage 用版本与崩溃矩阵；按相关性覆盖拒绝、异常/预算、停用回退、晚期失败、缓存重试与冷恢复，JSON/KKPV 业务状态一致。
- 公式/准入属于玩法，ID/合法表示/索引/编码一致性/原子发布属于宿主约束；engine 不依赖 V8，mod-api 保持纯数据，同步玩法 VM 入口前释放候选借用。
- 首片明确输入规模、单位/数值域、权限、写面及 before/native/replace/拒绝关系；源码按职责组织，源码/测试不超过 1000 行。
- 验证与提交按 [AGENTS.md](../../AGENTS.md) 和 [compat/README.md](../../compat/README.md)；Rust 改动重建测试 CLI，提交前运行无参数完整门禁。兼容改动重建 release/适配器并运行 ratchet，另与实际开发基线逐例比较，不只看总通过数。

### 2026-10-09 热身拍接手实施记录（已完成，随本提交交付）

- 接手时 HEAD 为 `8d5d63a`，工作区已有 101 个文件的热身拍实现/时间断言/夹具及报告；本次复审后收口，不以原报告替代实际门禁。
- 补齐 TickInput 新字段的漏改调用、kk-world 发布单测参数与 rustfmt；矿物 power 资格门测试移入独立 mineral_power_gates 模块，消除 1001 行门禁失败。
- 修复复审发现的场景切换回归：after_tick 使用显式推进轮数 player_ticks，热身拍不提前切换、after_tick=1 不漏触发，恢复按同一计数与 applied_switches 选择包集合。双通道首轮切换/冷恢复新增用例通过；原切换夹具边界保持不变。新增世界发布测试验证装配后 setTerrain 仍开放、首个显式轮后关闭。
- 现行 game-loop/local-simulation/mod-switching/state-inventory 已同步；没有新增保存域或检查点格式键，time/player_ticks 沿已有载荷保存，恢复不重复热身。装配失败仍要求重新创建分片；Mod 生命周期参与热身，场景对象在其后放置。
- 最终无参数 `pnpm run check` 通过：Rust 1431 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；`pnpm run fmt:js:check` 通过，无 doctest 变更。release CLI 与适配器重建；固定 screeps-ok `34a630e` 全量 2791 passed / 19 failed / 104 skipped，棘轮通过；另相对实际 HEAD 报告核对 +2 转绿 / 0 转红。
- 三 bot 复验完成 2000/2000/10 tick，无中断/惩罚跳过/missing_api；Overmind 保留既有初始化报错。查询键集合不变、部分计数有少量变化，不宣称逐位等价。持久报告已更新，bot-check 仍不代表实际推进世界验收。
- 剩余 19 例（11 实现缺口 + 8 官方对齐豁免）继续按执行队列推进；keeper/NPC 应独立拆片，模块上下文三例与 Room 原型/pull 等散例另行逐例核对。G02.d3 结构/rampart 寻路与 G27.a 后台宿主仍未开始，本片不扩大完成范围。

### 2026-10-09 Review 修复（已完成，基线 `1b452c6`，随本提交交付）

- 修复 `f1607b8..1b452c6` 复审发现的 storage 截止拍容量提前回落、terminal 边界测试跳过等号拍两个问题。
- storage 新增权威 `store_capacity`，对象 tick 在意图相之后更新；无控制器时保留原值。creep/powerCreep 独立 transfer 处理器、Mod 资源读面、JSON/KKPV 玩家投影与管理查询全部消费保存值，截止拍仍允许按上一轮加成容量转入，结算后才回落，不裁剪超存库存。
- 容量纳入对象候选与房间二进制快照，schema 升至 v2；v1 缺少权威容量，无法无损恢复截止拍/归属窗口，按现有开发期策略显式拒绝，不补造状态。现行 game-loop/checkpoints/state-inventory 已同步。
- 新增双通道精确截止拍实际转账、晚期写回失败/缓存重试、截止拍检查点冷恢复、无控制器保留容量与 v1 拒绝用例；terminal 按真实施放时间覆盖截止前/等号/后一拍的费用与冷却，DISRUPT_TERMINAL 验证等号拍即恢复提取。
- 最终无参数 `pnpm run check` 通过：Rust 1434 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过，无 doctest 变更。release CLI 与适配器重建，bot 夹具已准备；固定 screeps-ok `34a630e` 全量 2791 passed / 19 failed / 104 skipped，棘轮通过。与实际接手基线 `1b452c6` 逐例比对 0 转红 / 0 转绿；本次未重跑三 bot，不沿用旧结果宣称本批通过。
- 两条复审问题已解决，本批没有未提交开发残项；剩余兼容红例与后续消费者范围不变，继续按当前执行队列推进。

### 2026-10-09 实现质量复审修复（已收口，基线 `f377b86`）

- deposit 的相对倒计时、绝对时钟和 harvested 共用安全整数校验，放置换算使用检查加法；插入、管理编辑、候选发布与冷恢复共用保存状态校验。新增非法放置原子性、截止时间超限及损坏检查点拒绝测试；debug/release embed 复现输入均明确报错、继续响应且保持时钟不变。
- storage 容量公式归入 engine，复用控制器配额与 power 查表；SHIELD 放置和 extension 灌能各自独立模块，公共施放门、扣费及成功提交边界保持。房间快照只接受当前 v2，删除无消费者的升级链；纠正 19 个 powers、测试相对拍与容量时序说明，并同步现行架构。
- 最终无参数 `pnpm run check` 通过：Rust 1436 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；无 doctest 变更，收尾注释/断言说明调整后另跑 Rust 格式检查通过。release CLI、适配器已重建，固定 bot 夹具已准备；screeps-ok `34a630e` 全量 2791 passed / 19 failed / 104 skipped，棘轮通过，与实际接手基线 `f377b86` 逐例比较 0 转红 / 0 转绿。本批未重跑三 bot。
- 复审问题及模块/版本策略建议均已处理；本批随提交交付，无未提交开发残项。后续兼容红例与 Mod 消费者范围保持台账所列缺口。

### 2026-10-09 keeper lair 生命周期（已收口，基线 `fcb7265`）

- 选取零散红中的 keeper lair 六例：管理放置、倒计时、Source Keeper 生成/替换及玩家归属/形状；同片验证候选失败重试与冷恢复。NPC 主动行为、Invader/stronghold 与 NPC 自杀仍为后续缺口，不扩大 G21 完成范围。
- 定向验收：新增 4 个双通道运行时测试及 1 个世界时钟测试通过；涉及 keeper 的三文件兼容子集 68 passed / 14 skipped，随后全量 2797 passed / 13 failed / 104 skipped（六例转绿）。复查补齐 lair 的敌方结构筛选与 Game.structures 排除，新增断言在 JSON/KKPV 两通道通过。
- 最终无参数 `pnpm run check` 通过：Rust 1441 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；`pnpm run fmt:js:check` 和收尾 Rust 格式检查通过，无 doctest 示例变更。release CLI、适配器重建且固定 bot 夹具已准备；最新构建的固定 screeps-ok `34a630e` 全量 2797 passed / 13 failed / 104 skipped，地板升至 2797 后棘轮通过，与实际接手基线 `fcb7265` 逐例比较 6 转绿 / 0 转红。本批未重跑三 bot。
- lair 时钟沿用 Generic data、keeper 沿用 CreepObj（下一轮设置 age_time），已验证计时中和出生后冷恢复、损坏时钟拒绝及原世界保留；无新二进制字段、无快照升版或旧数据迁移。本批随提交交付，无未提交开发残项；G21 地图初始化/NPC 主动行为及其他列明消费者仍未完成。

### 2026-10-09 玩家表达式模块上下文（已交付，基线 `b736482`）

- 核对锁定官方 runtime-driver.js 的控制台 eval 包装、game.js 的入口缓存检查与 screeps-ok vanilla 创建流程；实现 embed 默认真实空 main 模块及独立控制台 module/exports 作用域。保留模块替换、缓存失效、玩家隔离与候选提交边界。新增三条双通道测试通过；默认 JS main 暴露既有 binary=null 分支，键树补登记 null/数组两分支（117 键），不改变载荷布局，无快照升版或旧数据迁移。
- 默认模块仅在 Embed 创建时安装，显式上传（包括空表）整体替换；表达式不自动运行上传 loop。控制台局部变量与 exports 每请求独立，显式全局写入按既有 V8 生命周期保留；求值器不捕获 host 私有状态。已验证缓存删建、玩家隔离、错误分类、候选失败重试仅执行一次，以及默认/上传模块资产冷恢复。
- 最终无参数 `pnpm run check` 通过：Rust 1444 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；`pnpm run fmt:js:check` 通过，无 doctest 示例变更。release CLI、适配器重新构建且固定 bot 夹具已准备；固定 screeps-ok `34a630e` 全量 2800 passed / 10 failed / 104 skipped，棘轮通过，按实测结果将地板升至 2800。相对实际 HEAD `b736482` 逐例比较 3 转绿 / 0 转红。
- 最新 release 三 bot 验证：TooAngel 2000/2000、Overmind 2000/2000、合成 WASM 10/10；全部 0 terminated / 0 skipped / missing_api 空。TooAngel/WASM 无 console-error；Overmind 保留基线已有 1 个 console-error tick，异常类型与调用栈相同，只有随机 harvest 标识变化。此检查验证模块运行，不替代持续推进世界中的经济闭环与 NPC AI 验收。
- 现行契约已同步 sandbox 与状态登记；本批随提交交付，无未提交开发残项。下一片核对 Room 原型剩余红例，NPC 主动行为、Invader/stronghold、G02.d3 后续及 G27.a 仍未完成。

### 2026-10-09 Room 原型兼容（已交付，基线 `72873a1`）

- 核对 UNDOC-PROTO-006 的实例自有方法与原型继承契约，并盘点 Room 对象构造及每拍投影更新；默认玩法修复，不扩大 NPC/Mod 开放范围。
- 已将公共方法安装到共享 Room.prototype，单房方法实现留在私有 WeakMap，探测代理与原对象登记同一上下文；queries 只装配私有方法表。新增两条 JSON/KKPV 双通道测试通过，覆盖原型包装跨 tick、call/apply 借用方法的接收者、两房查询/Memory 隔离、真实创建结算、事件缓冲、寻路及冷恢复。release CLI、适配器重建。
- 最终无参数 `pnpm run check` 通过：Rust 1446 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；`pnpm run fmt:js:check` 通过，无 doctest 示例变更。固定 screeps-ok `34a630e` 全量 2801 passed / 9 failed / 104 skipped，棘轮通过，按全量报告核对并上调地板至 2801；相对实际接手 HEAD `72873a1` 逐例比较 1 转绿 / 0 转红。
- 固定 bot 夹具重新准备，最新 release 三 bot 完成 TooAngel 2000/2000、Overmind 2000/2000、合成 WASM 10/10，全部 0 terminated / 0 skipped / missing_api 空。完成数与错误摘要均与入库基线一致（Overmind 保留 1 个 console-error tick，仅随机 harvest 标识变化）；不据此宣称经济闭环或 NPC AI 已完成。
- 已复核原型方法装配只执行一次、私有上下文不进入玩家属性、两房指令按接收者结算；现行 sandbox/源码组织契约同步。无新增持久化字段、快照升版或旧数据迁移；本批随提交交付，无未提交开发残项。下一片核对 NPC 主动行为与死亡闭环，Invader/stronghold、G02.d3 后续及 G27.a 仍未完成。

### 2026-10-09 Invader 无目标退场（已交付，基线 `d1d0c7b`）

- 核对官方 invaders/pretick.js、findAttack.js 与 suicide.js：首片限已占领、无敌方单位且无 spawn 的房间，非治疗型 Invader 发射自杀动作，经既有结算建立空墓碑与事件。适配器 NPC 别名映射到保留用户名；追击/治疗/寻路、Source Keeper 巡守及 raid/stronghold 留后续，不以一条转绿宣布全领域完成。
- 实现已接通：核心统一 Invader/Source Keeper 精确保留归属；适配器只在放置边界转换别名。自动退场与玩家动作共用派发/压制/候选发布，Invader 自杀使用零回收率，TTL/战斗死亡保持普通回收。四条 JSON/KKPV 双通道测试通过，覆盖空墓碑/容器/事件、非目标分支守卫、失败重试及死亡前后冷恢复、短玩家名与 NPC 跨房门；复核补齐受伤无有效 ATTACK 且有 healer 时保留的门。最终门禁与重建后的全量兼容验证已通过。
- 完整无参数 `pnpm run check` 通过：Rust 1450 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；`pnpm run fmt:js:check` 通过，无 doctest 示例变更。release CLI、适配器已重建，固定 bot 夹具重新准备；NPC 死亡/适配器契约子集 85 passed，固定 screeps-ok `34a630e` 全量 2802 passed / 8 failed / 104 skipped，棘轮通过；相对实际接手 HEAD `d1d0c7b` 逐例比较 1 转绿 / 0 转红，按全量报告核对并上调地板至 2802。本批未重跑三 bot，不沿用旧结果宣称本批通过。
- 复审补齐管理快照 owner 回套件 handle 的反向映射，玩家 VM 的 owner.username 保留公开用户名；发现的 srcKeeper 契约回归已修复并经子集/全量验证。退场算法仅在可证明无目标的子域发射动作，缺少路径判定的分支保留对象，治疗/孵化守卫均有实际断言。
- 已同步现行游戏循环、源码组织与兼容说明；NPC owner、creep、墓碑沿既有保存域，自动预意图不引入保存字段，无快照升版或旧数据迁移。本批随提交交付，无未提交开发残项。下一片推进 NPC 主动战斗/巡守/追击/治疗；G21 的地图初始化、raid/stronghold、Mod 贡献面，以及 G02.d3 后续和 G27.a 仍未完成。

### 2026-10-09 Source Keeper 巡守与战斗（已收口，基线 `a8f579f`）

- 三路复审（结算保真/测试边界/全局影响）发现项当批清偿：`overall.md` 实际依赖表与 kk-pathfinder 双消费面（玩家 op 通道 + 引擎 NPC Rust 直调）同步；`checkpoint_format_version` 升 v5 并注册 v4_to_v5 拒绝型迁移（对齐 effects 域先例，缺 keeper_memory 的 v4 房间显式报错，不再撞裸 serde 错误），相关版本断言与迁移测试更新；inter_room 跨房迁移与 effects/extensions 对称随迁 keeper_memory（当前 NPC 不跨房，防御性接线并有单测）；powerCreep 缺失 hits（手工检查点可达）改为入桶不读 hits——仍参与 mass 估计、最低 hits 选择跳过（官方 `_.min` 跳过 undefined，全缺时官方发废意图由结算丢弃、kk 不发，可观察等价）；测试修复：NPC 同伴排除断言把 Invader friend 先放使其 ID 最小（原平手不可区分）、死亡回滚测试经 discard_pending_round 后断言记忆保留（原断言消息引用不含 keeper_memory 的玩家投影面）、投影泄漏断言改真实字段名（source_id/move_cache）、补记忆失效重选与超距（>5）负例、失败轮玩家 Memory 与直跑等值断言、time=0 合法载荷触发缓存重算哨兵的对照用例、缺 hits powerCreep 的 mass 临界（15>13）与全缺不发攻击用例；fatigued 与字段剥除统一经真实检查点 decode/encode 通道（edit_object helper）。
- 复审登记的遗留（不修，见 game-loop 已知对齐边界与执行队列）：官方 moveTo 的 `memory_move.lastMove`（唯一消费者为 Invader walkTo 推挤判定，实现 Invader 寻路片时必须补）；NPC 意图派发先于玩家（官方玩家在前，仲裁按对象 ID 重排洗平，属既有派发架构）；官方 roomObjects 取回序按 _id 升序为外部行为假设；RemovedObject.keeper_memory 在 Mod 槽链内拒绝的轮内恢复路径无测试触达（本片测试覆盖整轮不发布路径）。

- 核对官方 keepers/pretick.js、processor.js 收集条件与 common/fake-runtime.js：同房 source/mineral 目标记忆（对象表顺序=ID 升序即插入序）、50 tick 路径缓存、近战/远程最低 hits 选择及 massDamage `[10,10,4,1]` 严格 >13 分支；敌对分类含孵化雏形（与 Invader 退场不同）、NPC 归属排除（官方 user 2/3）。寻路复用原生 kk-pathfinder（engine 不依赖 V8），代价矩阵按官方 defaultCostMatrix：障碍+portal、非公有他人 rampart、自家障碍工地阻挡，道路无条件覆盖先前写入（官方 Uint8 钳 255 仍小于 Infinity）；不在路径且与首格切比雪夫 >1 或已达路径尾时清缓存（官方 nextDirectionByPath 返回 0 同形）。空近战桶官方 `_.min` 返回 Infinity 仍发一条 id=undefined 废弃意图由结算丢弃，kk 直接不发（可观察等价）。合并按官方 `assign(ii, existing)`：显式同名动作优先。机制见[游戏循环](../architecture/game-loop.md#source-keeper-巡守与战斗当前首片)。
- NPC 私有记忆为房间级新保存域 `keeper_memory`（source_id 与可空 move_cache），补齐候选回滚（RemovedObject 随对象恢复）、对象删除清理、完整检查点、值域校验（执行者归属/目标 ID 范围/坐标 0..49/时钟安全整数/路径 ≤2500 逐步相邻）与状态清单登记；不进玩家投影；缺整个域的旧检查点由 serde 拒绝。
- 接手时上一片实现 4 例测试失败，逐例归因为测试侧问题并修复（实现与官方源逐行复核保真）：source 放置须走 structure 通道成为原生 Source 变体（kind="source" 落入 Generic，patrol/path_reuse/corrupt 及 schema 夹具同改）；`IntentBatch::from_json` 线格式是对象 ID 直接做键，套 `{"objects":…}` 外壳被静默丢弃；`builtin:creep.suicide` 处理器入口不存在（suicide 无开放拒绝槽），对象保留场景改为真实机制：TTL 死亡同轮玩家写回失败→整轮候选回滚→对象与记忆一同恢复、重试轮重放后死亡链落盘；p1 无房内对象时整房不可见，投影不泄漏断言前先放远处工人建立视野。无包分片 checkpoint 顶层 `mods: null` 为真实可空分支，钉死键 123→124（与 binary null/数组分支同理）。
- 定向验收：keeper_ai 六条 JSON/KKPV 双通道测试通过（巡守记忆与贴邻停止、攻击最低 hits 与 mass 严格阈值、失败重试与冷恢复、50 tick 路径复用、损坏记忆七分支原子拒绝、死亡候选回滚恢复记忆随对象）；checkpoint 键树 124 键钉死。
- 最终无参数 `pnpm run check` 通过：Rust 1456 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；无 doctest 变更。release CLI 已重建，适配器经 ratchet 重建；固定 screeps-ok `34a630e` 全量 2802 passed / 8 failed / 104 skipped，棘轮通过（地板 2802 持平、0 转红，8 例均为既有官方对齐豁免），工作区机器报告已更新。本批未重跑三 bot，不沿用旧结果宣称本批通过。
- 现行 game-loop 新增巡守小节、state-inventory 登记 keeper_memory 域、compat README 当前验证句同步。本批随提交交付，无未提交开发残项；剩余消费者：Invader 战斗/追击/治疗、keeper 跨房移动、raid/stronghold、keeper 地图生成（G21），以及 G02.d3 后续与 G27.a。

### 2026-10-09 开发进展调研（只读核验实现与入库证据）

- 核验基线：`main` / `5315dca`，接手工作区干净。本次未开发功能、未重跑构建/测试/棘轮/bot；下列结果来自现有提交、机器报告和验收记录，不能称为本次实测通过。
- 最新兼容机器报告时间为北京时间 2026-10-09 15:29:51：2802 passed / 8 failed / 104 skipped，共 2914 例，通过率约 96.2%，地板 2802。8 个红例已登记为官方对齐差异；104 个跳过项还涉及 NPC/raid/stronghold、portal、actionLog、跨 shard 等，跳过本身不证明已实现或未实现，须逐例核对能力门与范围。99% 目标至少 2885 个通过，较当前需增加 83 个，不能只消除 8 个失败便宣布达标。
- 实现抽查：NPC 预意图已接 Source Keeper 同房巡守/战斗与 Invader 无目标退场；storage/backend 源码仍为预留，apps/game 仍占位；renderer/shared 已有实现，renderer R7 最终矩阵与长测仍未收口。Mod 父项台账为 4 项已完成、12 项部分完成、14 项未开始，不可换算为工作量完成率。
- 入库三 bot 完成数为 TooAngel 2000/2000、Overmind 2000/2000、WASM 10/10，missing_api 为空；Overmind 保留 1 个 console-error tick。这些是先前固定世界运行证据，Keeper 最新批未重跑，也不能代替真实世界经济/战斗/存活/重启验收。
- 文档复审问题：overall.md 的“前端 apps/game 和 packages/* 目前是占位包”与 frontend.md、renderer/shared 源码不一致，README 的目录占位描述亦应按包区分；compat/README.md 的约 96.1% 与实际比值约 96.2% 有轻微差异。本次仅登记，不改现行实现契约或历史记录。
- 下一步仍按当前执行队列选择 Invader 主动行为、真实 bot 玩法验收或 G02.d3 结构/rampart 寻路贯通；服务线按 G27.a → G27.b → G28.a/G29.b 推进，G26.b 是 G27.a 的另一后续线。本次仅更新本台账（未提交），没有未提交实现残项。

### 2026-10-09 真实 bot 持续世界验收首片（已完成，随本提交交付）

- 实施基线 `5315dca`；工作区仅有上一轮调研台账改动，保留。本片先以锁定版本 TooAngel 未修改的 `src/` 模块运行真实 sim 世界，核对出生、采集/搬运、建造/升级与存活；贯通 JSON/KKPV 和检查点冷恢复。Overmind 与完整战斗/多房验收仍为剩余消费者，不把首片记作 M2B 完成。
- 先核对实际行为与失败证据，再补可复跑场景和验收入口；玩家自行捕获的异常不能仅靠 CLI 返回成功销账。本片必要的实现修复须补回归与完整门禁，兼容变更重建 release 并跑棘轮。新增验收与本批实际结果见下文，不能沿用上一片结果宣称通过。

- 已实现 `compat/scripts/bot-gameplay.mjs` 与 `sim-tooangel` 场景/公开观察入口：逐文件核验锁定提交的 89 个原版模块，不修改 bot 配置或发额外意图；统计上一拍正量结算事件、真实出生/死亡后出生，完整窗口捕获锁定版本已知的 bot 自吞异常日志；输出目录全新且禁止经符号链接写入源码。六个反例测试覆盖空玩法、假恢复/漏日志/空回放、实际出生、超过日志头部窗口的自吞异常、源码路径与旧报告保护。
- release CLI 已重新构建。JSON/KKPV 各完成 2000 tick 连跑、1000 tick 前段保存、1000 tick 新进程冷恢复续跑，共六个阶段（8000 个实跑 tick）；每阶段输入回放均与完整检查点逐位相等。观察事件截至最终玩家执行的上一拍（终态窗口为 tick 1999），不计未读末拍；独立直跑/冷恢复按业务条件验收，不承诺玩家随机数与 CPU 的跨运行等值。
- 四份 2000 tick 终态：RCL2、3 个建成 extension、9–10 只存活 creep；出生 14 次、死亡 4–5 次、死亡后出生 2–5 次；实际采集 12294–12962、转账 11008–12097、建造进度 6482–6885、升级及 GCL 增量 200–201；均无已登记捕获异常。持久证据：[验收汇总及六份原始 sim 报告](../../compat/reports/gameplay-tooangel/summary.json)。临时模块/检查点/输入日志不入库，可按固定源码与公开命令重建；失败试跑保留在临时目录，曾因工具误把恢复日志累计段数当本段数判失败，已修正并有反例测试，无引擎修复。
- 完整无参数 `pnpm run check` 通过：Rust 1457 passed / 0 failed，JS workspace、维护工具、文档链接与源码规模通过；收尾新增旧报告保护反例后工具 6/6 单独复验通过，`pnpm run fmt:js:check` 通过。没有 Rust 实现或 doctest 改动；完整门禁执行前已重建 release，本批不是引擎规则修复。
- 已同步本地模拟契约、compat 使用说明和 roadmap 的首片范围；剩余 Overmind、战斗、多房、长期存活与 M2B 最终验收不销账。提交状态：本批改动及上一轮调研台账随本提交交付；下一步按队列选择 Invader 主动行为或其他真实 bot 首片，G02.d3 与 G27.a 仍未完成。

- 最终兼容验收：固定 bot 夹具重新准备、适配器重建；显式指定本次 release 的 `node compat/scripts/ratchet.mjs` 通过，2802 passed / 8 failed / 104 skipped，转红 0 / 转绿 0，地板维持 2802。未重跑固定世界三 bot-check，不用历史报告宣称本批三 bot 验收；本批真实玩法验收仅为上述 TooAngel 首片。收尾文档链接与 diff whitespace 检查通过。

## 2026-10-09 Invader 自动远程攻击首片（已完成，随本提交交付）

- 基线：TooAngel 验收已提交 `a3c0602`，起始工作区干净。按当前队列选择完整 `shootAtWill` 消费者，未扩为完整 Invader AI。
- 实现：`npc/invader.rs` 持有原退场逻辑与自动远程目标选择；有效 RANGED_ATTACK、三格内最低 hits、稳定平手顺序、精确 NPC 归属排除、孵化 creep 排除、powerCreep 分类和缺 hits 边界按锁定官方 4.3.0 源码。治疗型仍开火，不切换 mass。显式同名动作优先，伤害走现有战斗槽链。
- 保存与职责复审：无新增状态写入域或保存版本，目标每轮从对象/身体/位置重建；预意图模块仅生成/合并动作，伤害、boost、rampart、事件、失败回滚沿原消费者。退场逻辑原样迁入 Invader 模块，NPC 合并入口保留共用边界。
- 已执行定向门禁：`pnpm run check -- kk-runtime --test mod_chain_tests --filter invader_ranged`，8 passed / 0 failed（每例 JSON/KKPV），覆盖最低 hits/平手/距离、NPC/孵化目标过滤、部件损毁/孵化执行者、powerCreep/缺 hits、boost/rampart、显式覆盖、Mod 取消、晚期失败重试及冷恢复续跑。初次测试发现 Keeper 零伤害预意图也产事件，已把 Keeper 夹具放在玩家射程外保留 NPC 排除断言，最终定向通过。
- 已执行完整无参数 `pnpm run check`：Rust 1465 passed / 0 failed，Clippy、debug CLI 构建、JavaScript workspace/维护工具、303 个 Markdown / 1927 个本地链接及源码规模检查通过；release CLI、固定 bot 准备与适配器构建通过。显式指定新 release CLI 执行 `node compat/scripts/ratchet.mjs`：2802 passed / 8 failed / 104 skipped，floor 2802，相对 HEAD~1 转红 0 / 转绿 0，8 条仍为既有官方对齐豁免；最新机器报告随本提交更新。本批不重跑三 bot/TooAngel 2000 tick（首片不含 Invader，既有入库证据保留）。
- 剩余消费者：Invader 近战、追击/逃离、治疗/跟随、多轮寻路与 walkTo 推挤记忆，随后 raid/stronghold；G21 的 Mod 世界事件能力仍未开放。本批随提交交付，台账、现行游戏循环、源码导航和兼容说明已同步；未推送远端。

## 2026-10-09 Invader 相邻近战首片（已完成，随本提交交付）

- 基线 `5ed3a03`，起始工作区干净。核对锁定官方 4.3.0 `findAttack.js` 后发现首个相邻 attack 会被最终零步 `findClosestByPath` 覆盖；按实际最终消费者实现，同格首个优先、否则最后相邻目标，不按最低 hits。含损毁 HEAL 仍属 healer 分支，不生成自动近战；与 shootAtWill 远程独立。
- 共用敌方分类但保留 hits 可空：零步近战入桶不读取 hits，远程最低值选择才跳过缺 hits。无新增保存域/版本，预意图从保存的房间对象、身体与位置重建，伤害/事件/死亡由既有候选战斗链持有；未新增公共 SDK 行为或样例。
- 定向 `pnpm run check -- kk-runtime --test mod_chain_tests --filter invader_` 已通过：14 passed / 0 failed（含既有远程 8 例、新增近战 6 例，每例 JSON/KKPV）。覆盖最终目标覆盖顺序、同格 powerCreep/缺 hits、NPC/孵化分类、有效部件/含 HEAL 分支、安全模式、boost/rampart、显式覆盖、Mod 取消不压制独立远程、晚期失败重试与冷恢复后死亡一次。
- 完整无参数 `pnpm run check` 通过：Rust 1471 passed / 0 failed，Clippy、debug CLI 构建、JavaScript workspace/维护工具、303 个 Markdown / 1927 个本地链接与源码规模检查通过。release CLI 重建、固定 bot 准备与适配器构建通过；显式指定新 release CLI 执行兼容棘轮，2802 passed / 8 failed / 104 skipped、floor 2802，相对 HEAD~1 转红 0 / 转绿 0（8 条仍为既有官方对齐豁免）。报告更新入库，本批不重跑三 bot/TooAngel 首片长测。
- 复审与剩余：本片没有 moveTo；官方路径首格建筑改向攻击/dismantle、复杂阻塞、非相邻追击/逃离、治疗/跟随、NPC walkTo 推挤与记忆仍未实现，须后续同片补齐路径保存恢复。只交付零步选敌与战斗消费，不标完整 findAttack/NPC 或 G21 完成。实现与现行架构/源码导航/兼容说明同步，随本提交交付；未推送远端。

## 2026-10-09 普通 Invader 行为贯通（已完成，随本提交交付）

- 基线 `13424c7`，起始工作区干净；持续开发授权下贯通 findAttack 的三轮寻路、路径建筑 attack/dismantle、逃离、healer 跟随/治疗与退场，未停在单个相邻动作。来源为锁定官方 4.3.0 invaders 与 common/fake-runtime.js。
- 职责复审：Invader 入口只持有分类/动作合并；attack/healer 按独立变化原因拆开；invader_path 持有最近目标/5 tick moveTo/flee，NPC path 共用原生矩阵与搜索，Keeper 仍用 50 tick 缓存。
- 保存：新增 invader_memory（move_cache 与 last_move）已接全世界检查点、编辑/发布校验、对象删除、RemovedObject 链拒绝恢复和迁移搬运；不进玩家投影。载荷 v6，v5_to_v6 缺域显式拒绝，可空键必须存在；schema 键树 130 键。Keeper 记忆未改形态。推挤依赖调整有源码证据：walkTo 唯一上游消费者为 stronghold/creeps.js，普通 Invader 使用 moveTo；Keeper lastMove/walkTo 随 stronghold 接线。
- 定向 Rust 测试：Invader 24 passed / 0 failed（含更新后的退场守卫），checkpoint 首次 19 passed / 0 failed / 1 ignored，随后追加独立 v5→v6 迁移用例已随完整门禁通过。覆盖追击/缓存时钟、逃离与远程独立、缺血量/稳定平手、治疗与跟随目标不同、严重受伤分支、墙环 attack/WORK dismantle、路径晚期失败丢弃/重试、冷恢复、非法记忆和缺域原子拒绝。
- 原首片守卫变化：可达 spawn 不再保留无目标 Invader；满血孤立 healer 无目标自杀。无活跃 ATTACK 仍可能按官方最终 attack 产生零伤害事件，旧“零攻击事件”断言按真实官方消费者收紧为零伤害，而非隐藏新行为。
- 完整无参数 `pnpm run check` 通过：Rust 1481 passed / 0 failed，Clippy、debug CLI、JavaScript workspace/维护工具、303 个 Markdown / 1929 个本地链接、912 个源码/测试文件均 ≤1000 行。release CLI、固定 bot 准备与适配器构建通过；显式 release 棘轮 2802 passed / 8 failed / 104 skipped（floor 2802，0 转红/0 转绿，8 条既有豁免）。代码/契约/报告随本提交交付，未推送。随后直接推进持续世界 NPC 战斗与输入日志/冷恢复验收。
- 父领域未完成：地图 NPC 生成/raid 调度、userSummoned/strongholdId 特殊状态、stronghold 战术/walkTo 推挤与 Keeper lastMove，G21 Mod 世界事件能力仍未开放。

## 2026-10-09 持续世界 NPC 战斗验收（已完成，随本提交交付）

- 基线 `5c544cf`，工作区干净。普通 Invader 已交付后直接继续验收，不等待逐片确认。
- 当前场景：护卫与近战/远程/治疗三名 Invader 在真实世界持续交战，使用公开事件与 hits/位置变化记录 NPC 移动、近战/远程伤害、治疗事件、NPC 实际恢复及护卫实际受伤；不只累计已发出的治疗意图。
- 计划验证：JSON/KKPV 各 200 tick 连跑、100+100 tick 跨进程冷恢复，各阶段输入日志与完整状态 verify-replay；补 CLI 场景回归及说明，保存可复现命令/报告证据。新载荷 v6 同时重跑既有 TooAngel 双通道首片确认输入回放/冷恢复未受空 NPC 域影响。
- 最终六阶段矩阵通过；四份终态指标一致：NPC 移动 229 次、近战伤害 840、远程伤害 4780、NPC 治疗事件正量 4600、NPC 实际恢复 3808、护卫实际受伤 252，事件观察截至 tick 199。各阶段完整输入回放通过，同通道连跑/续跑与跨通道的最终世界、玩家 Memory 等值。报告见 [NPC 战斗汇总](../../compat/reports/gameplay-invader/summary.json)。
- TooAngel 六阶段重新实跑通过：各通道 2000 tick 连跑、1000+1000 tick 冷恢复，出生/死亡后补员、采集/搬运/建造/升级与 GCL 条件及完整检查点回放通过，最新 v6 报告覆盖原位置；历史证据保留于 Git。
- 完整无参数 `pnpm run check` 通过：Rust 1482 passed / 0 failed，Clippy、debug CLI、JS workspace/维护工具、303 个 Markdown / 1932 个本地链接、914 个源码/测试文件规模通过；release CLI 重建、JS 格式检查通过。无生产实现/适配器或 doctest 变化，本片不重复棘轮，最近 `5c544cf` 的 release 棘轮 2802/8/104、零转红仍是对应实现证据。
- 场景/CLI 回归、机器报告和现行模拟契约随本提交交付，无实现残项；下一步 Overmind 单房经济验收。场景不替代真实 bot 战斗/多房验收，不标 M2B 或 G21 完成。

## 2026-10-09 Overmind 持续世界验收（已收口，随本提交交付）

- 基线 `4a3016e`，起始工作区干净；按持续开发授权直接推进执行队列第二项。使用未修改 v0.5.2.1 原版模块，初始 Memory 显式包含 creeps/flags/rooms；100 tick 探针成功不能代替经济闭环与完整日志检查。
- 探针发现 Overmind 的 RoomObject.prototype.ref 未被 Flag 继承，overlord 引用成为 undefined>mine。锁定官方 engine 4.3.0 flags.js:28 明确继承 RoomObject；当前 registry 类层级漏 Flag。修复共用继承后补双通道即时创建、跨拍投影与检查点冷恢复回归，再继续长期经济/捕获异常验收。无新保存域。

- 原版发布资产已从官方 v0.5.2.1 独立下载核验，SHA-256 为 e78ad3c8…b5a4b8；工具按完整固定 SHA-256 与目录清单验证后复制，不改 bot。共用 bot-gameplay 六阶段矩阵，新增 --bot overmind；仍支持原 TooAngel 默认入口。
- 观察器复审发现运行前写 Memory 的计数会被原版 Mem.load() 替换丢弃，已改为原版 loop 后写当前 Memory；完整窗口拦截 logger 的 FATAL/ERROR/异常，补 Memory 替换、冷恢复不重复出生、日志头部之外捕获及源身份失败不写输出反例（工具 8/8 通过）。
- 六阶段矩阵实际通过：JSON/KKPV 各 2000 tick 连跑、1000+1000 tick 跨进程冷恢复，每阶段完整检查点回放相等；四份终态实际出生 21–24、死亡 5、死亡后出生 16–19，采集 25728–26704、搬运 3270–4594、建造 6500–9870、升级/GCL 增长 350–500，均 RCL2、建成 extension、无捕获异常。报告见 [Overmind 汇总](../../compat/reports/gameplay-overmind/summary.json)。独立运行按业务条件验收，不宣称随机名称/CPU/最终世界跨运行逐位等值。
- Flag 修复定向门禁通过；release CLI 重建、固定 bot 准备、适配器重建后 release 棘轮 2802 passed / 8 failed / 104 skipped、0 转红/0 转绿（既有 8 豁免）。最终完整无参数 `pnpm run check` 通过：Rust 1483 passed / 0 failed，Clippy、debug CLI、JS workspace/维护工具、文档链接与 916 个源码/测试文件规模通过；JS 格式检查通过。无 doctest 或新增保存域。
- 本片代码/场景/报告、沙箱及模拟契约、使用说明与 roadmap 随提交交付；下一项 G02.d3 后续。真实 bot 战斗、多房和长期验收仍未完成，不标 M2B 完成。

## 2026-10-09 G02.d3 静态对象通行后续（已完成，随本提交交付）

- 基线 `d4f4403`，起始工作区干净；按执行队列继续计划级通行，单值规则分别控制静态障碍对象（内建 OBSTACLE_OBJECT_TYPES 中非生物项及 Mod obstacle 建筑/己方障碍工地）和私有他人 rampart。默认保持原生；动态 creep/powerCreep 占格、portal 默认寻路避让、建造/孵化放置不随规则改变。
- 两个布尔规则实现为 builtin:movement.obstaclesPassable 与 builtin:movement.privateRampartsPassable；冻结快照同源驱动登记/仲裁/预检摘要、默认玩家矩阵和 NPC 矩阵。自定义 CostMatrix 仍由玩家控制，不抹除其显式 255。既有墙规则同时核对 NPC 寻路消费。
- 无新增持久域；有效规则/计划身份沿现有检查点保存，catalog revision 更新并沿身份门拒绝旧计划。补双通道原生/两独立 JS 包/组合、真实移动、失败重试与冷恢复；逐房粒度及摘要增强仍未完成。

- 实现：StaticPassability 三轴同源快照替代旧墙旗标，单独模块持有目录与值域解析；Engine 安装候选、Working/Movement 与链预演携带快照，两个 creep 请求入口及 powerCreep 登记同步；玩家沙箱私有装配同源值，不新增公开全局。NPC 寻路同步墙与对象声明，并补 Mod 建筑 obstacle/road 的目录消费，保留原版搜索选项与缓存时效。
- 定向 7 条 Rust 回归通过：JSON/KKPV，静态障碍/私有、自有、公开 rampart/两轴组合、己方及他人工地、玩家显式 255、Mod 建筑完工/创建拒绝、powerCreep、动态生物占格/两个否决槽、两独立包、同规则冲突/非法值、晚期写回失败缓存重试及带真实包身份冷恢复、Keeper 对象/墙搜索。首轮失败为测试夹具：跨房 costCallback 覆盖采样、原生局部路径向后绕行并非原地不动、mineral 必需字段、恢复须提供相同源码身份的 Mod 包；已按实测消费修复，未放松越障/执行等值要求。
- 公共三个声明包与使用说明、现行移动/源码组织契约和完整玩法计划已更新；builtin revision v20、规则总数 157，沿既有规则/计划保存，无新增写入域。完整无参数 `pnpm run check` 通过：Rust 1490 passed / 0 failed，Clippy、debug CLI、JS workspace/维护工具、304 个 Markdown / 1955 个本地链接与 921 个源码/测试文件规模通过。release CLI 重建、固定 bot 准备与适配器构建通过；显式 release 棘轮 2802 passed / 8 failed / 104 skipped、floor 2802、0 转红/0 转绿（8 例既有豁免）。JS 格式检查通过，无 doctest 改动；本片未重跑 bot 长测，不以之前报告宣称本片长测。

- 本批实现/测试/示例/契约及兼容报告随提交交付，无实现残项；下一片 G03 障碍槽第二预演。G02/G03 父项仍部分完成，不标完整移动替换。

## 2026-10-09 G03 障碍追加后链预演（完成）

- 基线 `fd27fbd`，起始工作区干净；按移动领域实际消费者继续，不等待小任务确认。新增 ctx.obstacle.evaluate()，按调用时当前追加阻挡和已经提交的准入终值运行与正式取消链同一代码，返回幸存胜者 ID 升序集合；不写世界、不改变候选，不提供解除阻挡或重选落选者。
- 验收包括多次调用/继续追加、交换/循环/跟随链/拉拽、静态放行与动态生物占格、准入覆盖、双通道、能力所属槽/过期及 catch+decline 故障、正常拒绝回退、晚期写回失败缓存重试及冷恢复。瞬态候选无新保存域，目录 revision 同步能力变化；完整仲裁替换和逐房通行粒度仍未完成。

- 实现已接 ctx.obstacle.evaluate 的 SDK/读 op/能力门与预算归属，经当前追加集脱离快照委托 Working；准入与障碍预演共用 simulate_with_blocks/cancel_blocked，后者锁定已经发布的准入终值，前者仍为障碍前上界。调用不消费阻挡或改变正式赢家。
- 前五组定向回归通过；追加两包组合与安全模式后，组合通过，安全模式夹具首次因未声明 safeModeAvailable 导致原版返回 ERR_NOT_ENOUGH_RESOURCES，已显式提供可用次数并重跑。第一次完整门禁仍使用修复前编译测试而失败，不登记通过；最终七组定向回归与无参数完整门禁已通过：Rust 1497 passed / 0 failed，Clippy、JS workspace/维护工具、源码规模 922 个文件均通过。
- 公共使用说明/既有障碍样例 README 与移动契约已更新，catalog revision v21。无新增保存域/版本；准入/追加候选仍为瞬态，冷恢复只保存真实世界与已提交 Mod state/Memory。G03 完整仲裁替换仍未完成。

- release CLI 重建、固定 bot 准备、适配器构建与显式 release 棘轮通过：2802 passed / 8 failed / 104 skipped，floor 2802，0 转红/0 转绿（8 例既有豁免）；JS 格式检查通过，无 doctest 改动。去除误重复的静态通行正文后，文档链接复验通过：304 个 Markdown / 1951 个本地链接。未重跑 bot 长测。实现/测试/使用说明/兼容报告随本批提交；下一项安全模式主人摘要，G02/G03 父项仍部分完成。

## 2026-10-09 G02 障碍摘要安全模式主人（完成）

- 基线 `3f3d9a7`，起始工作区干净。新增只读 ctx.args.safeModeOwner，复用正式取消链的有效主人计算；无主/无安全模式/到期为 null，不改变物理对象摘要或通行规则。验证双通道、生物归属、到期边界、候选重试和冷恢复。公共输入变化同步 catalog 身份，无新增保存域。逐房通行与完整仲裁仍待后续。

- 实现复用 Working.active_safe_mode_user，输入所有者与 native/预演/取消同源，补充 creep/powerCreep yielding 注释。双通道覆盖主人/非主人执行者 × 自有/外来占据者 × 两种生物 × 激活/未激活；一拍到期边界、晚期失败缓存重试、活跃时保存后到期冷恢复均通过。无参数完整门禁通过：Rust 1499 passed / 0 failed，Clippy、JS workspace/维护工具与 923 个源码/测试规模通过；JS 格式通过，无 doctest 改动。release CLI 已重建，固定 bot 准备与适配器构建通过；显式 release 兼容棘轮通过：2802 passed / 8 failed / 104 skipped、floor 2802、0 转红/0 转绿（8 例既有豁免）。文档链接 304 个 Markdown / 1952 个本地链接通过；不声明 bot 长测。

## 2026-10-09 控制器零升级量兼容修复（完成）

- 基线 `41de432`，起始工作区干净。核对固定官方 engine 4.3.0 的 upgradeController.js：有能量且其他门通过时，即便 WORK 效果为 0，仍写 actionLog 与 amount=0/energySpent=0 事件；RCL8 满额仍提前拒绝。补原生结算回归，验证不付费/不涨 GCL、限额与其他拒绝门保留。不扩大 G15 Mod 策略范围，无新增保存域。

- 删除零量提前返回，保留原生所有拒绝门；新增无 WORK/损毁 WORK × RCL1/8、能量/归属/距离/等级/封锁/孵化拒绝和 RCL8 同轮先耗尽限额回归，三组定向测试通过。首版测试夹具编译失误已修正；红测真实复现原来的零量事件缺失，孵化孤儿清理导致的夹具查询也已按可选对象修正。无参数完整门禁通过：Rust 1502 passed / 0 failed，Clippy、JS workspace/维护工具与 924 个源码/测试规模通过。release CLI 已重建；固定 bot 准备、适配器构建与显式 release 兼容棘轮通过：2802 passed / 8 failed / 104 skipped、floor 2802、0 转红/0 转绿（8 例既有豁免）。文档链接 304 个 Markdown / 1953 个本地链接通过，无 doctest 改动。无新 SDK 示例需求，不新建交付报告；本片没有运行 bot 长测。

## 2026-10-09 G27.a 独立异步后台宿主（完成）

- 基线 `10eb812`，起始工作区干净。首片使用独立线程/V8 与有界请求队列，包内 services 入口同步注册、start/request/stop 可 await；只接脱离世界的已提交快照与显式命名的有限只读探针。复用 VM CPU/墙钟/内存预算，失败销毁后台域，不直接访问 Working/玩家权限。后台进程态是瞬态，冷启动重新 start；不承诺耐久任务，G27.b outbox/输入游标与 G26.b storage provider 仍未实现。验收须含真实包、异步 I/O、生命周期、队列满、资源故障、未选包隔离与持续世界并行。

- 独立 worker/realm、原生登记能力门、同步注册/异步生命周期、队列快拒绝、过期/跨包/未授予/调用和字节限额及共享 VM 预算已接线；FileProbe 使用常规文件的有限只读异步任务。实际发现异步 op 会在 V8 调用时提前轮询，首轮测试因没有 Tokio 上下文而中止，已将预算调用统一进入 worker 执行器上下文；随后宿主六组、CLI 三组与双通道持续世界一组定向回归通过。CLI 冷检查点身份读取/输入守卫、后台等待读期间世界继续推进及重启 calls 归零均实测。新增 ADR-017、现行后台契约/源码导航/状态域清单与公共示例，无新玩法持久域或载荷版本；可选 services 清单/代码沿既有 Mod 身份与包摘要保存，无服务旧清单保持原序列化形态。首轮完整门禁通过，最后的故障来源诊断和协议常量收口后须再次完整复验。

- 最终无参数完整门禁通过：Rust 1512 passed / 0 failed，Clippy、JS workspace/维护工具、307 个 Markdown / 1971 个本地链接与 936 个源码/测试规模通过。release CLI 重建通过，公共 services 包由 release 实际运行，start/request/stop 与 world_unchanged=true 验收通过；JS 格式检查通过，无 doctest 改动。固定 bot 准备、适配器构建和显式 release 兼容棘轮通过：2802 passed / 8 failed / 104 skipped、floor 2802、0 转红/0 转绿（8 例既有豁免）。本片未运行 bot 长测，不标网络/耐久任务完成；实现/测试/公共示例/契约及报告随本批提交。

## 2026-10-10 TooAngel 多房进攻与冷恢复（已完成，随本提交交付）

- 基线 `c3c2d68`（防御战斗批），起始工作区干净。真实 bot 多房进攻首片：锁定 TooAngel 在 W1N1（RCL5）对邻房 W2N1 的敌方脚本 reserver 发动 attackunreserve——`handleHostileReservedRoom` 判定敌方预留（首见 1600 拍 `noReservedRoomInterval` 门 + RCL≥5 + isHealthy（存储 ≥100k）+ spawnIdle EMA 门）→ 排队（优先级缺省 20+距离+年龄）→ 跨房行军 → 按 CLAIM 部件优先锁定击杀。夹具 `compat/fixtures/sim-tooangel-offense/`（场景/敌方脚本/观察器）+ wrapper `compat/scripts/bot-offense.mjs`（默认 2900/2400 即验证窗口，观察器以 kk_offense 模块身份部署，二进制/观察器/敌方哈希逐段断言）。入库证据 `compat/reports/gameplay-tooangel-offense/`（六报告+summary 含溯源补注；终版实测击杀 1888-2048 拍（标定期十二段观测 1861-2135）、attackEvents 10/reserverDamage 360、可见拍数 2900/2900）。
- 夹具四个非显然约束（研究期逐个排除）：(1) **结构必须按 bot 自算布局位播种**—— TooAngel 布局从控制器邻位推导（存储=upgrader 位最佳邻位、spawn 位沿路径），乱放触发 `checkForMisplacedSpawn` 置 misplacedSpawn → isRampingUp 恒真 → isHealthy 永假 → 进攻门不开且能量死亡螺旋（错位 extension 堵塞 filler 区使 universal 无路由目标）；按战斗批 Memory 里已算好的布局位（spawn(5,5)/存储(3,3)/extension 前 20 位）重播后经济链即通。(2) **可见性必须播种**——bot 自产 scout 最早 1497 拍（`handleScout` 相位门），brain 层 nextroomer 排队（含 scout）被 `resourceStats` EMA（τ=10000，cpuUsed 初值=limit、cpuIdle=0）在 sim 时限内永假（u=0 理想情况也需 ≥6931 拍，冷恢复重置 EMA）；播种 bot 方 scout（出生即在 W2N1 + Memory.creeps 预置角色记忆，creep memory 即 Memory.creeps[name]）提供首拍可见性。(3) **世界边缘滞留是官方同构行为（四环机制）**——未注册邻房在寻路地形包中为全墙占位（kk-runner/pathfinder.rs ring_rooms+ALL_WALL），PathFinder.search 只返回 incomplete 的房内部分路径不跨界；scout 的 memory.path 经 Memory 序列化丢失路径形态，每拍 moveByPath NOT_FOUND→重搜→新路径首步 OK→incompleteCount 清零，25 拍救援永不触发（kk moveByPath 末端语义与官方一致）；停在边缘格时结算期跨房过渡放回原格（settle/inter_room.rs 未注册目标房）；getNextRoom 的 lastSeen 启发式使未注册邻房恒被优先、唯一已注册邻房 W1N1 永不被选中——四环使播种 scout 永久滞留 W2N1 维持全程可见。(4) **外房必须播种 mineral**——`updateBasicData` 对有控制器无矿房读 `minerals[0].mineralType` 每拍抛错。
- 冷恢复语义：截点 2400 在击杀（最晚 2122）之后，前缀段 `/objects/reserver missing` 断言成立；reserver 死后预留持续至绝对期限 end_time（种子 5000、结算按绝对期限制不逐拍递减；玩家面 ticksToEnd 读数随之递减），W2N1 全窗口保持 HostileReserved 不触发 bot 自预留/扩张；恢复后 heap（global.data.rooms/scout data.nextRoom）丢失无碍——scout 冻结态由 Memory.path 维持、攻击已在前缀完成、Memory 计数器跨检查点接续。结构计数四段全窗口一致（offenseBirths/attackEventsOnReserver/reserverDamage/可见拍数），时序类指标随墙钟抖动、阈值留裕度。
- 登记：bot 对战（双方真实 bot 互攻）、长期存活（≥72h 等价长窗）、Overmind 指令旗标出兵（Overmind 侧战争系统更重，旗标驱动 squad）为独立切片；TooAngel `handleHostileOccupiedRoom`（RCL6 门 + storage 能量分档 squad）未在本片范围；scout 边缘冻结若未来引擎引入"世界边界回退"语义需复审此夹具的可见性假设。
- 三路复审（夹具断言/官方语义时序/验收纪律）发现当批清偿：判定强化两处——"进攻发起于预留观察之后"只排除非触发链出生、判别力近恒真，改为 `offenseFirstBirthTick >= w2HostileReservationTick + 1600` 使 1600 拍门合规进入自动判定；对 reserver 的攻击事件补攻击者归属核验（`Game.getObjectById(objectId).my`，对齐战斗批塔归因纪律）。文档口径——README 击杀范围注明标定期/终版两口径、roadmap 未收口清单补回 Overmind 旗标出兵。其余全通过（事件字段/objectId 归因/命名前缀唯一性/触发门参数/冷恢复接线/SHA 溯源链闭合均经写入点核实）。
- 验证：debug 与 release 二进制六阶段全绿（强化断言后 release 终版证据 26/16 项检查零失败）；无参数 `pnpm run check` 通过；本片不关闭 M2B（bot 对战/长存仍开）。

## 2026-10-10 TooAngel 防御战斗与冷恢复（已完成，随本提交交付）

- 基线 `4cef2ba`（动作记录导出批），起始工作区干净。真实 bot 战斗首片：锁定 TooAngel 在 RCL4 自家房对抗敌方脚本坦克（敌方为脚本玩家而非引擎 NPC，共同点仅是"非真实第三方 bot"），验证 bot 真实防御链——findEnemies 判敌（Memory 预置 reputation=-100，TooAngel diplomacy 未知玩家默认友好、checkPlayers `reputation || 0` 保留负值，终态 -620）、塔 attack 自 2 拍（90 发耗尽 900 能量后断能）、defendranged 250 相位门 385 拍开孵 394 拍出生、安全模式 398 拍激活 399 拍可见、经济共存（采集 2080）。
- 夹具 `compat/fixtures/sim-tooangel-battle/`（场景/敌方脚本/观察器）+ wrapper `compat/scripts/bot-battle.mjs`（默认 520/260 即验证窗口；观察器经 prepareModules 以 kk_battle 模块身份部署，不混入经济夹具模块；JSON/KKPV × 连跑/前缀+跨进程冷恢复六阶段，前缀段只验通用与 harness 检查（战斗域跳过），每段 --verify-replay 逐位重放比较完整检查点，二进制/观察器/敌方脚本哈希逐段断言）。入库证据 `compat/reports/gameplay-tooangel-battle/`（六报告+summary 含溯源补注：attackEvents 181=塔 90+defendranged 91、塔伤 13500、敌方承伤 14410/自疗 13584、combatBirths 2）。
- 迭代中两个实质发现：(1) **safe-mode 敌方 creep 穿行堆叠是官方语义**——kk `check_obstacle_at_xy` 的房主安全模式分支与官方 movement.js `checkObstacleAtXY` 真值表逐条同形（获准移动豁免先于安全模式分支，两侧一致），非 kk bug；(2) **TooAngel defendmelee 潜在自伤崩溃**——与敌同格后 `PathFinder.search(自,自)` 返回空 path（官方 pf.cc:506 同形），bot role_defendmelee.js:34 的 `search.path[0].isBorder` 崩溃，官方服同样触发；defendmelee 498 拍入队 516 拍出生，约 556 拍抵达坦克同格。验收窗口 520 拍取防御三腿与堆叠点之间，不触发该崩溃。
- 三路复审（夹具断言/官方语义时序/验收纪律）发现全部当批清偿：语义证伪两处——安全模式 399 的真实触发路径是 handleReviveRoom 的 400 拍相位门（控制器 (1,1)→T≡398 mod 400，isStruggling+findHostileAttackingCreeps 把含 heal 部件坦克算作敌），checkForSafeMode 的 attackTimer>100 路径是死代码（enemies>0 数组比较恒 false，登记为 bot 潜在缺陷家族）；defendmelee"498 出生"口径错（498 入队开孵、516 出生）。纪律——脚本默认窗口曾与 README 不符（1200/600，裸命令会跑进崩溃窗）改为 520/260、README 节位曾错拆 Invader 节、"通用 12 项"计数失准、summary 补先例溯源字段（evidence_note/implementation_commit/validated_harness_sha256）。夹具——EVENT_ATTACK 伤害字段 damage 归因、safeMode 玩家面是剩余拍数（观察器与终态断言从绝对拍比较改为剩余拍语义）、塔攻击按攻击者 objectId 单独归因（原"塔攻击"检查实为全房间事件计数，现 towerAttackEvents≥50 独立断言）、敌方坦克注释机理勘误（塔断能时序+安全模式禁疗而非免伤保护）。
- 新发现（CPU 墙钟语义）：kk-vm cpu_clock 用 Instant/thread_cpu_time 计量玩家 CPU（官方同形）——验收重跑时与评审代理并发的 json-continuous 段出现防御时序平移（防御者 394→420、attackEvents 181→155、采集 2080→1632，其余三段不变），空闲机器重跑四段全窗口逐位恢复一致；跨段指标等值是空闲机器观察结果而非引擎不变量，引擎结算确定性由每段 verify_replay 保证，指标阈值均为留裕度下限（重负载下仍全过）。
- 登记后续：多房进攻切片（TooAngel attackunreserve 需 RCL5+storage+侦察+1600 拍间隔，或 Overmind 预置指令旗标驱动出兵——两者都不改 bot 源码）、bot 对战、长期存活；TooAngel `global.data.rooms` 侦察数据是 heap 态（冷恢复丢失会重置扩张/攻击计时——跨恢复断言只看最终态与事件累计）；经验：EVENT_ATTACK 伤害字段是 damage 非 amount（观察器已按此实现）。
- 验证：debug 与 release 二进制六阶段全绿；终版 release 证据（三重身份断言，26/16 项检查零失败，四段全窗口指标逐位一致）；无参数 `pnpm run check` 通过（JS 格式/Clippy/Rust 测试/文档链接/源码规模）；本片不关闭 M2B 战斗验收（多房/对战/长存仍开）。

## 2026-10-10 动作记录导出与 captureActionLog（已完成，已提交 `4cef2ba`，99% 检查点达成）

- 基线 `5daaa88`（ISM+shardLimits 批），起始工作区干净。套件最后 21 个跳过例（27.10-actionlog，actionLogCapture 能力门控）整簇交付；语义源=官方 4.3.0 processor 各 intent 写入点+vanilla 适配器读 history[gameTime-1] 报 tick=gameTime 的口径。
- 交付：`settle::action_log` 旁路通道——`Working.action_log` 每拍新建（官方每拍重置模板同形）/同键后写覆盖（DEDUP）→ 结算尾按**最终对象集**装配（同拍死亡对象丢弃，官方 saveRoomHistory 同形）→ `SettledRoom` → `OwnedTickTxn` → `Engine.last_action_log`（与 last_events 同为检查点载荷，钉死键 137→145，冷恢复保留上一拍）；embed RPC `captureActionLog(room)` 返回 `{room, tick, objects}`（tick=当前世界时间、产物属上一拍；非公开 say 裁剪——官方历史快照删除非公开 say）；适配器透传与 actionLogCapture 翻转。写入点覆盖：creep 13 键模板全量（rangedHeal 独立键、attackController/dismantle 复用 attack、rangedMassAttack 空对象、rangedAttack 与塔远程分键）、目标侧 attacked/healed（坐标=来源；含持有模板的五类结构目标 tower/link/lab/invaderCore/factory）、结构源侧 tower 3 键/link/lab 4 坐标/invaderCore reserve、pc 侧 power `{id,x,y}`（新枚举变体）/enableRoom 复用 attack/renew healed 自身坐标官方怪癖/say 公开、spawn renew 与 lab boost 的 healed、Mod 声明反击配对。
- 三路复审（官方语义/对抗安全/测试边界）发现全部当批清偿：语义——结构目标 attacked 丢失（官方 _damage.js:73 对模板结构同写）、Mod 反击与 pc 四处单通道漂移（违反"位图与旁路同点"自定约定）；安全——untagged `Mass {}` 兜底过宽改 deny_unknown_fields 严格空载体（杂键/越界坐标/残缺载荷整体拒绝，不再静默降级空载荷）；测试——补 pc 条目（usePower/enableRoom）、反伤 attacked+同拍死亡丢弃、rangedAttack/rangedMassAttack 键、say 10 UTF-16 截断、结算失败轮保留、objects 序显性化（ObjId 定宽零填充=localeCompare 等价）。
- 登记：factory produce（载荷含 resourceType，枚举扩展随 factory 簇）；invaderCore attackController（官方实际写 **reserveController** 键而非模板键，processor.js:78 模板键从未被写过——将来接线勿照模板）/upgradeController/transferEnergy（intents 未接线，见既有台账）；pc spawned（power 全局阶段时机，旁路接入需另设计）；tower heal→pc 的玩家面 data.actionLog healed 位图缺口（既有，随 pc 簇回补）；条目附加 structureType 装饰字段（契约内 optional，vanilla 结构条目实际无此字段）；跨房/portal 对象条目留来源房=移动前快照（官方 per-room save 先于全局跨房应用，同形）；官方 saveRoomHistory 仅 activateRoom 房保存（纯 NPC/keeper 房 vanilla 无历史、kk 恒记录，正常场景不可分辨）；恢复面无规模校验（与 last_events 同政策）；pc say 按 Unicode 标量截断（与 creep UTF-16 口径的既有差异）。
- 验证：无参数 `pnpm run check` 通过；Rust 集成 7 例（mod_chain_tests/action_log.rs，双通道，含冷恢复/失败保留/同拍死亡）；release 棘轮（全引擎修复后的二进制）**2905 passed / 9 failed / 0 skipped**（floor 2884→2905，+21 转绿：ACTIONLOG 全簇，**跳过清零、99% 检查点 2885 达成并超出 20 例**）；9 失败=8 既有豁免+1 SHAPE-GAME-002 形状互斥豁免。

## 2026-10-10 InterShardMemory 本地段与 CPU shardLimits（已完成，已提交 `5daaa88`）

- 基线 `038c1aa`（random 注入批），起始工作区干净。官方 OSS 引擎无这两个 API（后端专有）——语义源为套件契约（29-multi-shard 测试全文 + adapter.ts 能力注释）。套件 7 例转绿：ISM-001..004、CPU-SHARD-001..003。
- 交付内容：InterShardMemory 本地段六层写回链——prelude `globalThis.InterShardMemory`（getLocal/setLocal；非字符串与超 100×1024 UTF-16 码元抛错不留痕、同拍 round-trip 读闭包现行值）→ host endTick 仅 setLocal 触发拍带出 `ismLocal`（与 RawMemory 同门：写回失败轮整轮丢弃）→ proto `EvalReport.ism_local`（`Cmd::SetMemoryState` 扩第四参注入 JSON 串）→ `RunResult.ism_local` → `PlayerWriteback.ism_local`（apply 挂 RawMemory 步同门回滚）→ `PlayerState.ism_local`（serde(default)，旧检查点回填 null；钉死键 136→137、state-inventory 补行）→ 输入日志 `ism_local` 键 + 回放缺键按 None。`Game.cpu.shardLimits` 读面（单 shard 恒 {shard0: limit}，getter 每次新对象）与 `setShardLimits` 真校验（非对象/未知 shard/非整数/负值/缺键/和≠limit→ERR_INVALID_ARGS；唯一合法分配即现状、成功 no-op 返回 OK——分配持久化与 12h 冷却随 multiShard 簇登记缓接）。capabilities.interShardMemory/cpuShardLimits 翻转。
- Rust 集成 5 例（mod_chain_tests/intershard_cpu.rs，双通道）：null 起点同拍 round-trip+**跨拍持久+冷恢复**（超套件）、非字符串/超限六型拒绝不留痕、双玩家隔离（单轮双写双读各得各值）、读面键值/和==limit、四错误分支+合法分配 OK 腿。套件 7/7 定向全绿。
- 三路复审（官方语义/对抗安全/测试边界）发现当批清偿：测试边界 G2 写回失败轮（Memory.big=1n）ISM 候选随整轮丢弃不泄漏组合测试；G3 四项补齐——恰 102400 码元成功腿（严格大于门）、shardLimits getter 新对象语义（改返回值不污染后续读）、回放 Some 分支（three_sources 写串并入 setLocal）、双玩家隔离（run_players 单轮双写双读各得各值）。安全 P2 检查点装载 ism_local 100KiB 限额校验（与 raw_memory/segments 同形）、InterShardMemory 全局随 resetRawMemoryHost 每 tick 重装（与 RawMemory/Memory 防篡改纪律对齐，闭包状态不重置）、陈旧注释清理。语义 P2 setShardLimits 捕获 Array.isArray/Number.isInteger（捕获纪律）。登记后续批：lone surrogate 使 endTick JSON 不可解析误判硬中断（ISM/RawMemory.set/segments 三通道同形的既有缺陷家族，自伤面，随写回出口统一 lossy 清洗修）；cpu.limit 非整数配置下 setShardLimits 合法分支不可达（当前恒 20.0 整数）；`RawMemory.interShardSegment` 死面与新闭包面并存（multiShard 簇接线时统一）；分配持久化与 12h 冷却（multiShard 簇）。守卫测试随批更新：沙箱全局白名单 +InterShardMemory、shardLimits 缺席钉改在场形状钉。
- 验证：无参数 `pnpm run check` 通过；release 棘轮 2884 passed / 9 failed / 21 skipped（floor 2878→2884，转绿 7、豁免转红 1——SHAPE-GAME-002 与 cpuShardLimits 套件契约互斥：dataProps 计入一切非函数属性且无 gameCpu divergence 通道，同一适配器无法同时通过形状例与 CPU-SHARD 读面例；ratchet-allow/parity.json 双登记归因，撤销暴露即转绿）。**99% 检查点（2885）恰差 1**——被互斥消耗；2884/2914≈99.0%，检查点达成移至 actionLog 21 例簇（唯一剩余跳过簇）。

## 2026-10-10 tick({random}) 处理器随机注入（已完成，已提交 `038c1aa`）

- 基线 `c39777a`（弃用告警批），起始工作区干净。官方/契约参考：engine processor/intents/minerals/tick.js:8-38（门 `Math.random()<MINERAL_DENSITY_CHANGE` 仅非 LOW/ULTRA 求值（|| 短路不消耗门值）、选择 do-while 排除旧密度、概率表 for-in 键序 1..4 `random<=概率` 命中即停、补量按旧 density 容量）；套件 src/adapter.ts TickOptions 契约（random 跨整次调用共享、signal 逐拍前检查、"once exhausted, adapters must throw rather than falling back"）与 vanilla 适配器（补丁覆写 engine_processor 进程级 Math.random、耗尽 sentinel 报错）。
- 交付内容：`ProcessorRandom`（Derived/Injected(Rc<RefCell<VecDeque<f64>>>)）挂 Engine（`set_processor_random`，调用期间设置成败均复位——run_player 路径零扰动），prepare_tick 带入 TickState、mineral_tick 的门与选择循环逐值消费（耗尽 SettleError 整拍候选回滚；Derived 派生流逐行等值保持 MineralRng 单实例语义）；embed RPC `tick` 新增 random 数组（整单前置值域校验 [0,1) 点名 `random[i]` 拒收不推进时间；响应回传 `randomConsumed`）与适配器透传（非 signal 单发整序列；signal 逐拍按消耗计数切片续传，耗尽后仍发 `random: []` 保留报错语义不静默回落）；capabilities.randomInjection 翻转。gameplay-determinism.md 新增"处理器随机注入（测试面）"节。
- 套件 7 例转绿：adapter-contract tick options.random 三例（越界/耗尽/多余值）+ MINERAL-REGEN-009 四例（MODERATE/HIGH × 门过/门不过）。套件 fixture 恒注入 signal → 7 例全部实测适配器逐拍切片路径。
- Rust 集成 5 例（mod_chain_tests/random_injection.rs，双通道）：越界双点位（random[0]=1.0/random[1]=-0.1）不推进时间、耗尽报错含 exhausted 且**已成功拍保留**（再生拍=time+3 门、第 3 拍耗尽回滚前 2 拍提交，终态 +2）、多余值无害、密度门矩阵（2/3 × 过→LOW+按旧密度补量 35000/70000/不过→原密度）、LOW/ULTRA 短路不消耗门值（单值 [0.5]/[0.05] 可区分）+ do-while 排除旧密度重掷分支（[0.04,0.7,0.95]→density 4）。
- 三路复审（官方语义/对抗安全/测试边界）发现当批清偿：语义+边界+安全三方独立同发现适配器 signal 模式空切片静默回落 Derived（违背上游"耗尽必抛"契约、与非 signal 路径自相矛盾）——修为耗尽后仍发 `random: []`；语义 P2 两处 settle/mod.rs 文档归属错位（ProcessorRandom doc 并吞 TickInput 原 doc、set_fail_room_for_test 的 doc/#[doc(hidden)] 错挂）归还。登记差异四处：注入域仅矿物再密化（vanilla 还覆盖掉落落点/stronghold 衰减/tower 补给掷骰）；耗尽时 kk 回滚整拍中止后续拍（vanilla 只冻结出错房间当拍、其余照常提交循环跑完）；注入值不进恢复缓存（PendingRound）也不进输入日志——注入拍失败后的重试与日志段回放按派生流结算；serde_json 上界进位（距 1.0 一 ULP 内合法值如 0.9999999999999999 被解析为 1.0 误拒——fail-closed 方向，无反向绕过）。embed random 数组无长度上限属既有模式（本地 stdin 可信面），接网络面前须补请求体上限。
- 验证：无参数 `pnpm run check` 通过；release 棘轮 2878 passed / 8 failed / 28 skipped（floor 2871→2878，0 转红、7 转绿，8 例既有官方对齐豁免）。

## 2026-10-10 弃用告警（已完成，已提交 `c39777a`）

- 基线 `9ff9d2e`（边缘环绕批），起始工作区干净。官方参考：engine game.js:113-120（register.deprecated 按消息文本 tick 内去重 + console.log）、map.js:199-207/235-258（告警先于房名正则与 pos 解构）、path-finder.js:68-74（仅 !isActive 告警 + 写 _useNewPathFinder）、rooms.js:231-238/:325-336（findPath 告警并置 undefined / findClosestByPath 仅告警）、structures.js:1257-1264（renewCreep 强化体告警在全部拒绝门后）。机制底座（deprecation.js 去重闭包 + perTickReset 挂钩、收集型 console + op 上限封顶、last_console 每玩家最近一次运行 + captureConsoleLogs RPC + 检查点保存）与 avoid/ignore/renewCreep 三处告警均为既有实现；本批只补三个缺失触发点：map.isRoomAvailable/map.getTerrainAt（官方逐字消息、先于参数校验——非法名也告警）、PathFinder.use 仅 false 触发（原 countStub 桩移除，无遗留消费方），翻转 capabilities.deprecationNotices。sandbox.md 新增"控制台输出与弃用告警"契约节。
- 套件 7 例转绿：DEPRECATED-MAP-001/002、PATH-001/002/003、SPAWN-001、DEDUP-001（2864→2871）。翻转外溢面经全量 grep：三 API 在 28 之外仅 LEGACY-PATH-003（use 三连调、无 console 断言）与 RENEW-CREEP-004（强化体续命新增一条告警行、无 console 断言），均无害。
- Rust 集成 4 例（mod_chain_tests/deprecation.rs，双通道）：map 双告警+三拍去重（同拍两调合一/新拍重发/非法名先告警后 false）、use(false) 告警+use(true) 静默（新拍独立腿防同拍去重遮蔽）+消息正文逐字、avoid/ignore 六表面（findPath/findPathTo/findClosestByPath 各 avoid+ignore，选项丢弃 10 步对角、filter 子串集与套件同形、不同文本互不去重）、renewCreep 强化体告警+无强化对照（capture 替换语义钉住）。
- 三路复审（官方语义/对抗安全/测试边界）发现当批清偿：语义 P1 登记分歧两处——官方 use(false) 会切旧寻路器（同 tick 后续 findPath 家族走旧分支：avoid/ignore 真实生效且不告警），kk 仅实现新寻路不建模开关（path-finder.js 注释+sandbox.md 明示）；官方告警经调用时全局 console 解析（玩家重绑会改道），kk 装配期捕获收集通道属有意加固。语义 P2 两处注释/文档表述修正（findClosestByPath 不置选项）。测试边界 G2-1 ignore 腿三表面补全+filter 补第三子串、G2-2 use(true) 判别性腿、G2-3 非法名告警腿；G3-1 评审稿的 SPAWN_RENEW_RATIO 按 0.6 推导与常量 1.2 不符（夹具两种读法均过、rc 断言可检）。安全无 P0/P1：dedup 劫持面（共享 lodash 活查找/Array.push 覆写）为官方同构自伤面（消息恒 6 条常量串、每 tick 重置、console 通道 1000 行/1MiB 先计数后封顶）。
- 验证：无参数 `pnpm run check` 通过（含 fmt 修正复跑）；release 棘轮 2871 passed / 8 failed / 35 skipped（floor 2864→2871，0 转红、7 转绿，8 例既有官方对齐豁免）。

## 2026-10-10 边缘环绕+核弹簇（已完成，已提交 `9ff9d2e`）

- 基线 `e5249ff2`（powerBank 批），起始工作区干净。官方参考经两轮后台研究代理逐文件取证：engine `processor/intents/portals/tick.js`（衰变门 `time > decayTime` 严格大于 + 连带移除 (x+1,y+1) constructedWall、无 ruin）、`creeps/tick.js:50,80-91`（传送标记 + `if(object.ageTime)` 真值门 + 不发 EVENT_EXIT）、`power-creeps/tick.js:44-47`（仅同 shard）、`processor/global.js:33-43`（同 tick 精确落格、无占位检查、目的房不可达保留标记）、`game/map.js:209-233`（getRoomStatus closed>novice>respawn）、`common/index.js:183-193`（calcWorldSize 包含跨度、maxX/maxY 零锚点）、`utils.js:644-659`（calcRoomsDistance 单 worldSize 双轴环绕 min(size-d,d)）、`structures.js:1356-1389`（launchNuke 状态门在 INVALID_ARGS 与 TIRED 之间、源+目标 novice/respawn→ERR_INVALID_TARGET）、`launch-nuke.js:22-24`（处理器只查源房）、`nukes/tick.js:10-14`（保护区飞行核弹移除）、driver `index.js:498-524`（novice/respawnArea/openTime 三墙钟折叠）。套件 27 例转绿：PORTAL-001..006、MAP-ROOM-003/004:adminClosed/novice/respawn/005、TERMINAL-SEND-015、NUKE-LAUNCH-014..017、NUKE-IMPACT-008:portal、SHAPE-NPC-004、hard-prerequisites/inspection portal 各一及关联行。
- 交付内容：portal 放置归一臂（destination 两形态互斥校验、ticksToDecay→decayTime、缺省永久门、store/owner 拒绝）+ 域级 validate 挂三口；玩家面 StructurePortal getter 沿用既有实现（同 shard RoomPosition/跨 shard POJO），查询快照折叠 destination room→roomName 与 ticksToDecay；nuker 快照缺省 cooldown:0（官方恒带，适配器 expectStructure 契约）；传送结算 `settle/portals.rs`（全房结算后、边缘过渡应用前单遍扫描——portal 标记覆盖 edge 标记故传送优先且被接管对象从边缘移动集剔除，creep 双条件门 age_time!=0 且本拍未从 0 初始化（fresh 集合随房间结算产物带出=官方门在寿命补设前求值，引擎孵化首拍豁免、放置即带寿命首拍传送）、powerCreep 无门仅同 shard、同格多 portal 按对象 ID 序只取首个=官方 `_.find`、未注册或 closed 目的房原地不动逐拍重试=官方不可达保留标记的每拍重扫等价）；衰变移除官方 `decayTime &&` 真值门加严格 `time > decayTime`（decayTime=0 与缺省同义即永久门；玩家可读 0 与 -1）+ 配墙连带；房间状态种子域 `room_status`（createShard status 四态、时间戳=未来 1 小时墙钟毫秒=vanilla 适配器同款），JSON/KKPV 两通道随房导出 status/statusTimestamp（公共地图数据不裁切视野），玩家面 getRoomStatus 四态；launchNuke 预检源+目标门（门序官方同位）+ 结算侧房间相/全局相源房门 + 核弹 tick 保护区移除；worldSize 活算（calcWorldSize 零锚点同式）驱动 getWorldSize/getRoomLinearDistance continuous/`calcTransactionCost`/terminal send 费用四处同源环绕（Rust `market::world_span` + `Plane.world_span` 注入意图相）；检查点格式 v9（v8_to_v9 拒缺迁移、固定键 136）。capabilities.portals/roomStatus/liveWorldSize 翻转。
- 登记（后续片回补）：跨 shard 传送官方 OSS 无实现（专有后端）kk 跳过；claimController novice 引导门（已辖房数 ≥GCL_NOVICE=3 且房在 novice 区→ERR_FULL，creeps.js:853-856）、novice 区 safeModeCooldown 抑制、墙钟到期的状态自动轮转。复审证伪修正两项原登记：describeExits/findRoute 经复核官方 OSS 本就不读 accessibleRooms（无状态过滤），kk 四向出口恒开与官方一致、无需登记；powerCreep 踩跨 shard portal 的 move 意图官方 OSS 亦无拒绝（仅传送跳过），kk 一致、原登记撤销。
- Rust 新增测试 24 例（portals 集成 15：传送两形态/踩格被动/衰变读数 [2,1,0,-1,null]+永久门快照 null/跨 shard 暴露/核爆存活/发射门四态/状态面+环绕距离/terminal 环绕计费/冷恢复+拒缺/放置拒绝/未注册目的房/疲劳保持无 EXIT 事件/边缘格 portal 胜过环绕过渡/closed 目的房阻断；settle 单测 8：同格传送集合/同格多 portal 首个/跨 shard 跳过/衰变门与配墙/49 边界/fresh 首拍豁免/closed 目的房/decayTime=0 永久门，另 nuke_actions 1 例发射门结算侧三腿；kk-world 域单测：portal normalize+validate、room_status 枚举/值域/序列化形状；v8_to_v9 迁移两腿）；terminal 既有费用断言随环绕更新（W1N1+W5N1 世界 span=7，直线 dx=4→环绕 3，注释锚定）。
- 三路复审（官方语义/对抗安全/测试边界）发现项当批清偿。官方语义：P1 跨 shard destination 快照折叠误改 roomName（补 `!shard` 守卫，玩家面/快照面同修）；P1 ageTime 首拍豁免在引擎孵化路径差一拍（官方门 creeps/tick.js:80 先于寿命补设 :88-91 求值——fresh_age 集合经 Working→settle_room 四元组→engine_tick 传入收集点，settle_room 签名变更同步 6 处测试调用点）；P2 边缘格 portal 与环绕过渡优先级（portal 标记覆盖 edge 标记——收集移至边缘应用前并按 portal 命中剔除边缘移动）；P2 closed 目的房未拦（补 accessibleRooms 语义：closed 跳过、novice/respawn 放行）；P2 decayTime=0 非永久（补官方 `decayTime &&` 真值门）；P2 永久门管理快照缺 ticksToDecay 键（恒插 null——适配器 PortalSnapshot 契约键非可选，玩家面官方 getter 仍 undefined）；登记清单三处证伪（describeExits/findRoute 官方无过滤、powerCreep move 官方无拒绝、claimController 系 GCL_NOVICE=3 引导门返回 ERR_FULL）；nukes.rs 过时头注与 market.rs 退化域注释（dx>span 时 JS 负距离 vs Rust 钳 0，多收不少收、全局相拒未注册房）随改。对抗安全：P2 validate 拒 store/owner/hits/hitsMax 走私键（portal 官方无 hits 面，防走私 hits 变可打）；P2 跨 shard 快照折叠同语义修复。测试边界：1a-1h/2/3/5/6 全清——launchNuke 补结算侧直注意图+stow 标记+closed 反向三腿、run_player 时序模型纠偏（执行+结算+推进一拍）、传送疲劳保持改即时读（避免闲置拍恢复归零）、堆叠 portal 单搬去重与 49 边界补测、双通道覆盖复核。
- 验证：无参数 `pnpm run check` 通过（Rust workspace 全绿、Clippy、JS、文档链接 307/1979、源码规模）；release 棘轮 2864 passed / 8 failed / 42 skipped（floor 2844→2864，0 转红、27 转绿，8 例既有官方对齐豁免）。

## 2026-10-10 powerBank 生成与消费者首片（已完成，已提交 `e5249ff2`）

- 基线 `e316a07`（raid 批），起始工作区干净。官方参考：backend-npm/lib/cronjobs.js 的 `genPowerBanks`（L194-243）、engine 的 _damage.js:35-41（反射）、processor.js:421-426（衰变移除）、_destroy.js + ruins/tick.js（战毁废墟与泼洒）、structures.js:589（owner='Power Bank'）、withdraw.js:32（拒提取）。套件 7 例（POWER-BANK-001/002/004、RUIN-002:powerBank、inspection、NPC-OWNERSHIP、SHAPE-NPC-003）由 capabilities.powerBank 翻转全绿。
- 交付内容：放置归一臂（power→store、ticksToDecay→decayTime 且缺省默认 POWER_BANK_DECAY——适配器契约明文、hits 覆盖须 ≥1、hitsMax 恒 2M、spec 键 store/owner 拒绝）；快照折叠 power/ticksToDecay；引擎反射分支（任意攻击类型 ×POWER_BANK_HIT_BACK 毫精度取整、safemode 抑制、HIT_BACK 事件 objectId=银行；显式反击策略整体替换）、static_hits_max 表项、power_bank_tick 衰变移除（>=decayTime-1 无废墟无泼洒）；战毁经通用 _destroy 产废墟（store.power 转移、RUIN_DECAY_STRUCTURES=10）由既有 ruin 通道泼洒；生成器 run_spawner（高速路房=编号任一轴 ≡0 mod 10、respawn 先掷、位置 do/while 墙体+出口且重掷上限 10_000 防病态地形挂死、容量+暴击、种子流带 power-bank 域标签）；RoomState.power_bank_time + 检查点 v8（拒缺迁移）+ 固定键 135；embed 命令 runPowerBankSpawner/setPowerBankState（0 归一 null，官方 falsy 同义）；powerBank 域级 validate 挂插入/编辑/恢复三口；deposit 记录拒带 structureType（串扰防护）。
- Rust 测试 30 例新增（settle 7：反射事件序/击杀拍反射/奇数不足 1 不反射/safemode/普通远程/反射致死墓碑/衰变/废墟；mod_chain 15：套件同形消费者 + 生成器调度/重掷/暴击/护栏/多房原子回滚/冷恢复双通道/命令契约；kk-world 8：放置归一/拒绝/溢出/域校验/高速路 S 轴；迁移与固定键更新）。三路后台评审（官方语义/对抗安全/测试边界）全部发现当批清偿：语义 P1 放置缺省默认值对齐适配器契约、P2 魔数常量化/0 归一/版本文档；安全 P0 单测期望值笔误、P1 病态地形挂死护栏与域级 validate 缺失、P2 边界复验/键纪律；边界 P1 击杀拍反射与反射致死缺测/种子路径与双通道缺测/多房次序、P2 锚定与注释措辞。登记分歧：反射 0.5 小数跨击累积不可复刻（整数伤害模型）、多房随机流交织序（房名序 vs db 序）、墙钟 cron 递延、status:'normal' 门随 roomStatus 簇。
- 验证：无参数 `pnpm run check` 通过；release 棘轮 2844 passed / 8 failed / 62 skipped（floor 2837→2844，0 转红、7 转绿）。

## 2026-10-09 raid 调度首片（已完成，随本提交交付）

- 基线 `4f7dd2bd`，起始工作区干净。官方参考源经确认本地在库：backend-npm/lib/cronjobs.js 的 `genInvaders`（L245-441，约 200 行）与 lib/utils.js 的 `isCenter`/`roomNameToXY`——与 stronghold 批同为"官方源逐字对照"。套件 14.6 共 21 例（INVADER-RAID-001..010）经 `runInvaderRaidSpawner({random})` 注入确定性随机序列驱动，随机流消费次序（方向→升级 R1/R2→count→每 creep 子型条件掷/落格/boost/名字→成功后阈值两次掷骰）已逐例手工推演并对齐矩阵 7 例组合金标。
- 交付内容：kk-world 域 `invader_raid`（RaidRoomState 四键、官方六 body 表逐字、boost 映射、sector_key/is_center_room——kk RoomName W/N 取负减一约定下 W 轴分组按房间号 |xx|-1 再除 10、中心带 n%10∈[4,6]）；引擎 `settle/invader_raid.rs` 生成器（资格门序：normal 且非活跃→无 Invader creep→预算≥阈值（goal==1 旁路）→扇区 level>0 核→出口扫描 0..49 恰排四角→邻房 owner/reservation 封锁方向；createRaid/createCreep 逐字复刻含 count 掷骰在内层 if 之后的官方缩进层级）；harvest 结算逐次累计预算（官方 harvest.js:45-48 同源，与超容掉落无关）；三个 embed 管理命令 `setInvaderRaidState`（逐键部分更新、harvestedEnergy>0 需有源、controllerReservation 绝对 endTime 缺省 5000、owner 须本分片非 NPC 玩家——vanilla 同形契约）/`runInvaderRaidSpawner`（注入序列逐值校验 [0,1)、耗尽报错保留原世界；缺省世界种子派生流）/`clearInvaderRaidCreeps`（无墓碑清理）；适配器三方法实装 + capabilities.invaderRaidSpawner 翻转。
- **官方怪癖逐字保留三处**：阈值重掷乘数 `Math.floor(rand>0.5?2:0.5)` 半量分支 floor(0.5)=0 把阈值**写成 0**，读回侧 `goal || C` 的 JS 真值回退把存储 0 视同默认阈值（本意减半实际重置为默认——首版实现误把 0 当免门槛，语义评审以 cronjobs.js:388 证伪后修正并有金标区分 0/×2 两分支）；出口方向选择数组序是各方向首个开口格的发现序而非固定方位序（同评审发现，排序按发现 i 升序实现）；RCL0 有主房在 controllerLevel 真值链中等同无主。出口方向算术按官方 W/N 负坐标（top=yy-1）。
- **新持久域 `raid`（每房）**：invader_goal/harvested_energy/active/status，官方分放 rooms 集合与 source.invaderHarvested，kk 聚合为一（生成器只读总和）；任意玩家/管理快照不暴露。检查点格式 v6→v7：v6_to_v7 缺域拒绝（不补造——活跃位丢失会静默放行生成）；schema 钉死键 130→134；harvest 累计随房间候选提交并经真实采集用例验证（WORK×1 每拍 2 能量，阈值 10 于第 5 拍达标）。
- 集成测试 14 例（mod_chain_tests/invader_raid.rs 生成器行为 11 例 + invader_raid_admin.rs 管理命令/检查点契约 3 例，源码规模门禁触发 1002 行后按职责拆分；关键面双通道）：抑制门矩阵（9 例对照）、阈值旁路与单出口精确落格、官方组合矩阵 7 例逐例 + 自派生 RCL8 例（body/boost 档/落位/随机流/count 收敛/0.4 强化阶梯）、成功后预算清零+活跃置位+再扫描拦截、乘数怪癖 0/×2 双分支（真值回退语义）、真实 harvest 预算累计、冷恢复等值（恢复的预定继续封锁 + 恢复的预算驱动生成 + raid 子树逐字等值——防 raid 域恢复丢失）、方向发现序与首格映射、部分更新语义、序列耗尽/非法值原子拒绝、种子命令契约校验（含无控制器房 null 清除无操作、ticksToEnd 浮点等价、检查点畸形值拒绝）。实测量：夹具同扇区核心放邻房时，玩家 tick 触发核心 reserve 意图会封锁出口（官方行为联动，测试改用非邻房同扇区核心）。
- 三路复审（官方语义对照/对抗安全/测试边界）发现项当批清偿：语义 P0（invaderGoal 真值回退——首版注释还把错误理解写成官方怪癖，修正实现+金标）、语义 P1（方向组序是发现序非固定方位序，修正+案例）、安全 P0（无控制器房 + controllerReservation null 的 expect panic 击穿 embed 服务，改无操作同 vanilla）、安全 P1（ticksToEnd 浮点整数静默落缺省 5000、检查点恢复 raid 值域不设防——as_f64 取整 + 域校验）、测试边界 G1/G2/G3/G4/G6/G7/G8（冷恢复对 raid 域失明、乘数分支零演练、active 置位无断言、方向序/首格映射、部分更新、残余校验）全部补测钉住；切比雪夫 vs 曼哈顿度量经复核在同边出口组内 dy 恒同、数学不可区分，登记为不可观察。安全 P2 有意分歧登记：kk 校验严于 vanilla（拒绝小数预算/未知键/负阈值、清理命令对不存在房报错、random 数组前置整单校验）；生成器失败时 kk 整轮候选回滚（vanilla 部分落库保留）。
- 登记（后续片回补）：墙钟 cron 调度（官方 5 分钟）不在本片；raid creep 战术沿用普通 Invader 行为；powerBank 生成（官方同属 invader cron 家族，套件另有 powerBank 消费者簇约 9 例）；遍历序 kk 房名排序 vs 官方 db 自然序（多候选房随机流交织次序不同）；未注入 random 的种子派生流 vs 官方真随机（同拍重复扫描同值）。
- 定向与门禁：kk-world 域单测 4 例、引擎随机源单测 1 例、运行时集成 8 例通过；无参数完整 `pnpm run check` 通过（Rust 全绿、Clippy、JS workspace、维护工具、文档链接与源码规模）；`pnpm run fmt:js` 通过，无 doctest 变更。release CLI 重建，固定 screeps-ok 全量 2914 例 **2837 passed / 8 failed / 69 skipped**，棘轮通过（地板 2816→2837、0 转红/21 转绿，8 例既有官方对齐豁免）。本批未跑 bot 长测，不以历史报告宣称本批 bot 验收。

## 2026-10-09 Stronghold 布防首片（已完成，随本提交交付）

- 基线 `2b68002`，起始工作区干净。按队列拆片交付 deployStronghold 布防：核心布防键 templateName（域校验限官方 bunker1..5）/strongholdId（字符串校验）；部署在 invader_core::tick 坍塌检查后接线（官方 pretick 先行——kk 晚于意图与移动仲裁，部署当拍移出布防格的 creep 逃逸，窗口限部署拍与模板脚印，登记差异）；部署执行逐字对照官方 stronghold.js：随机衰减时长（STRONGHOLD_DECAY_TICKS×(0.9+r×0.2)，DeployRng 房名+核心 ID+tick 派生，重试/回放/冷恢复同值）写核心 decayTime+满血+坍塌条目（同 power 改写不重复），先整体移除 Invader rampart（无废墟），逐格清格（击杀已出生 creep 留墓碑、工地 progress>1 折半掉能量、有建造成本的玩家结构摧毁留废墟且不发事件）后按模板插结构：rampart（hits=奖励档/hitsMax=8 级/hitsTarget=核心级/nextDecayTime=decayTime/私有）、tower（满能量+双容量键）、container（显式 storeCapacity 0+奖励）、road（专用变体）；全部结构复制 strongholdId 并登记同截止坍塌条目，到期与核心一起被通用坍塌门移除。清格豁免谓词 is_bunker_member：Generic 数据 strongholdId 或效果表已有坍塌条目（同拍先铺 road 变体靠后者豁免——修复了后铺 rampart 清掉先铺 road 的自伤 bug）。calcReward 逐字复刻含官方怪癖（末项除数按位置下标取密度表第三项 5，末位密度非 5 时总量超发，金标测试钉住）。放置路径孵化守军寿命部署时绑定核心 decayTime（官方 create-creep ageTime 同源，回补上批登记缺口）。带坍塌效果的结构战毁时废墟衰减与坍塌截止取 max（官方 _destroy.js 同款，饱和减防管理通道过期条目下溢）。capabilities.strongholdDeploy 翻转。
- 三路复审（官方语义对照/对抗安全/测试边界）发现项当批清偿：calcReward 末项除数官方怪癖（P1，逐字复刻+金标）、damage.rs end_time 下溢饱和减（P2）、衰减窗口上界收紧 67500..=82500、坐标裁剪分支测试（bunker5 靠边）、ticksToCollapse+模板组合（部署改写截止）、rampart/container decayTime 与核心交叉断言、progress=1 无掉落边界、核心格击杀、模板 rampart 不受旧 rampart 清除波及、坍塌扩面（非 controller 全清+控制器清空）、域坏值循环补 ticksToCollapse、奖励金标钉官方超发形态（liquid 密度 30÷5=28）。安全侧核对通过：玩家指令不可写布防数据键、豁免谓词不可伪造（1002 效果仅布防链路可写）、随机流不掺候选态、发布校验不拒部署产物。
- 登记（后续片回补）：raid 调度、maintainPopulation/refillTowers/focusClosest 战术、create-creep/transfer/attack/upgrade 意图处理器；tower actionLog 键（全仓无 actionLog 既有决策）、creep 的 strongholdId（kk CreepObj 无数据袋）、road 变体不携带 strongholdId、powerCreep 清格击杀（官方杀、kk 存活，罕见边角）、rampart hits/hitsTarget 查表互换不可区分（全模板 reward_level==core_level）、container 0 档空奖励分支经模板不可达、tower 平铺 storeCapacity 为 kk 建造同款双键约定。
- 定向与门禁：运行时双通道 7 例（布局/数值奖励/清格/确定性重试冷恢复/寿命绑定坍塌协同/边缘裁剪/非法放置）+域单测 4 例通过；无参数完整 `pnpm run check` 通过（源码规模：模板数据按 bunker 逐文件拆分 stronghold_templates/），`pnpm run fmt:js:check` 通过，无 doctest 变更。release CLI 重建，固定 screeps-ok 全量 2914 例 **2816 passed / 8 failed / 90 skipped**，棘轮通过（地板 2811→2816、0 转红/14 转绿，8 例既有官方对齐豁免；5 例 STRONGHOLD-LAYOUT-001 转绿）。本批未跑 bot 长测。

## 2026-10-09 Invader core 生命周期首片（已完成，随本提交交付）

- 基线 `1a26bfc`，起始工作区干净。按队列零散红的地图/raid/stronghold 后续拆片交付 invaderCore 对象生命周期：放置归一（level/ticksToDeploy→deployTime、spawning 剥 body 换算绝对 spawnTime、ticksToCollapse→效果表条目、owner 强制 Invader、hits/hitsMax 默认满血、效果空槽恒入表投影空数组属性）、坍塌到期按官方清单清控制器（upgradeBlocked/sign/reservation 保持）+通用效果门静默移除无 ruin、孵化到期经通用出生落格（与 spawn 共用 `_born-creep` 8 方向/踩踏语义，被围顺延）、reserveController 意图（无邻接/部件门、time+1 起步、常量加成、RESERVE_MAX 封顶、事件）与 default 行为 handleController 预意图（仅 reserve 分支发射）、孤儿宿主判定扩展为官方 `spawn||invaderCore` 同格同形。capabilities.invaderCore 翻转。对象走 Generic + 既有 effects 表与 CreepObj，无新增保存域；效果沿 v6 检查点。
- 集成缝修复两处：孵化 creep 原被孤儿判定（宿主表缺 invaderCore）一拍移除；驻防 creep 无 strongholdId 按官方语义落入普通 Invader 退场（官方 processor.js 收集条件同形，测试出生当拍捕获）。
- 三路复审（官方语义对照/对抗安全/测试边界）零语义分歧，发现项当批清偿：rustfmt、坍塌断言补强（ticksToDowngrade null/效果条目随清/sign 等三项经代码复审锁定——结构化字段在二进制快照不可注入）、坍塌冷恢复两腿用例（到期照常清+未到期不提前）、通用门注释精化（同拍新对象与官方 bulk.insert 同形）、effects 空槽+hits 默认补齐（SHAPE-NPC-002 形状例转绿，评审预告外发现：26.0-discovery 也挂 invaderCore 能力键）。安全评审的「宿主遮蔽分歧」经官方原文复核证伪——creeps/tick.js 谓词仅位置+类型无名字匹配，kk 与官方逐字同形，未改。
- 登记（后续片回补）：deployStronghold 布防/模板（含核心 decayTime 键与孵化寿命绑定 core.decayTime——当前放置默认 1500）、raid 调度、maintainPopulation/refillTowers/focusClosest 战术、create-creep/transfer/attack/upgrade 意图处理器（handleController 的 own→upgrade、他人预定→attack 分支因此不发射）、结构 actionLog/roomInfo.active（全仓结构无 actionLog 既有决策，无消费者）、出生 chick 查找多 spawning 过滤（官方无、边角更保守）、多核心同房逐核心发射（官方单变量，仅夹具差异）、效果条目 level:1 投影差异（效果表既有约定）。
- 定向与门禁：运行时双通道 7 例（含坍塌冷恢复）+域单测 2 例通过；无参数完整 `pnpm run check` 通过：Rust 1525 passed / 0 failed，JS workspace、维护工具、307 个 Markdown / 1976 个本地链接与源码规模通过；`pnpm run fmt:js:check` 通过，无 doctest 变更。release CLI 重建，固定 screeps-ok 全量 2914 例 **2811 passed / 8 failed / 95 skipped**，棘轮通过（地板 2802→2811、0 转红/9 转绿，8 例既有官方对齐豁免）。本批未跑 bot 长测，不以历史报告宣称本批 bot 验收。

## 2026-10-09 控制器 INVULNERABILITY 消费者（已完成，随本提交交付）

- 基线 `b25e280`，起始工作区干净。核对固定官方 engine 4.3.0：creeps.js attackController 预检、creeps/attackController.js 原生免疫和 nukes/tick.js 控制器封锁免疫均有实际消费者，kk 三处缺门。核弹 safeMode 结束独立于免疫；已有过期封锁刷新分支因官方 OR 优先级也独立于免疫，不能扩大免疫范围。补双通道、严格到期、effect 与 power 不混淆、绕过玩家摘要的原生防线与保存恢复验证。生产者/raid/stronghold/G21 仍未实现，无新增保存域。

- 原生攻击红测复现缺免疫门；补严格 active_effect 查询并接两条结算消费者，玩家预检保持门序。原生攻击/核弹两组回归通过（核弹夹具按实际 landTime-1 改为 ttl=1）；端到端夹具按实际 controller structureType/坐标与检查点数字对象键修正，验证效果标识、到期、摘要改写后的结算防线、晚期失败缓存重试与冷恢复。无新保存域，效果沿既有表/v6 保存；本片不添加生产者。

- 三路复审（官方语义对照/对抗安全/测试边界）零阻塞，发现项当批清偿：免疫路径补动作日志不落断言（事件/封锁/降级之外的第四面）、核弹期望值改按封锁象限写死不再镜像实现公式、预检改用装载期捕获的 C.EFFECT_INVULNERABILITY（发现 `__creepC` 为逐键白名单表，补登该键）防玩家改写全局常量劣化错误码；`ticksRemaining>0` 冗余判据与官方同形保留；Some(0) 封锁值引擎写点不可达、`unwrap_or(0)==0` 为官方 falsy 语义保真度修正，均经评审取证。
- 定向与门禁：引擎 settle_tests 2 例、运行时 controller_invulnerability_tests 2 例（JSON/KKPV 双通道）通过；无参数完整 `pnpm run check` 通过：Rust 1516 passed / 0 failed，JS workspace、维护工具、307 个 Markdown / 1976 个本地链接与源码规模通过；`pnpm run fmt:js:check` 通过，无 doctest 变更。release CLI 重建，固定 screeps-ok 全量 2914 例 2802 passed / 8 failed / 104 skipped，棘轮通过（floor 2802、0 转红/0 转绿，8 例既有官方对齐豁免）。本批未重跑 bot 长测，不以历史报告宣称本批 bot 验收。
