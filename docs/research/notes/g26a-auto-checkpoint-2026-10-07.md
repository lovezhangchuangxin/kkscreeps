# G26.a 自动检查点：周期保存与 RPO 恢复选择——登记与交付记录

## 前置审计结论（2026-10-07，两路后台只读 Explore）

**驱动面审计**：

- 保存链现状：`EmbedEngine::checkpoint` 入口 `no_open_round` 门；`write_checkpoint`→`publish`（tmp create_new→write→sync→rename→父目录 sync，崩溃只可能读到完整旧版或新版，中断测试钉死）；sim 只在终态保存且 `failure.is_none()`。单槽文件，无多版本命名/目录索引。
- 挂点结论：配对规则「检查点不得领先日志头」决定周期保存必须在 `log_round` 成功之后——sim 循环 `run_players` Ok 返回后的点位天然满足三条件（已提交/日志段已写/无开放轮次）。放 sim 编排层而非引擎内：周期策略是宿主编排关注点，引擎内做需处理 restore/switch/teardown 生命周期重置。
- 恢复选择：配对原语 `segments_from_checkpoint` 已在（G25.b）；目录扫描/降级选择器全缺属新增；「检查点恰在日志尾+1 且零段待重放」在 verify 语义下报错、在 RPO 恢复语义下应为合法终态——恢复入口特判，不改配对函数。
- 元数据缺口：sim Metadata 需补日志配对游标（保存时最后段 seq/tick）；`applied_switches` 中途保存须传已生效子集；Metadata 是 deny_unknown_fields——新字段带 default；executable_sha256 门保证无跨二进制存量。
- sim 面最短路径=旗标+Options 字段+validate_paths 守卫批次+报告升版+mod_test 调用点同步；embed RPC 无 checkpoint 方法先例，不需要。
- 性能：单次保存=全量快照序列化+fsync×2（无增量机制、ModIdentity 每次重算包 SHA）；storage-bench 量级参考 4.85-5.19 ms/轮（KV 预编码，不可直接搬）；sim 报告不计时检查点阶段——验收补实测样本。
- 风险清单：mods_switch 时序（切换摘除日志→切换后检查点无覆盖）；开放轮次门（只在 Ok 后存）；自动恢复半途失败（replay 失败引擎进恢复态不可原地续放——恢复编排须全有或全无）；撕裂/损坏降级（跳 .tmp、坏文件次新）；G26.b 边界（不预造 provider trait）；teardown/restore 交互（周期状态放编排层天然随进程结束）。

**设计承诺与消费者审计**：

- 台账 G26 行「.a 基于现有检查点……单写者周期完整检查点、RPO 与恢复选择」；正文段「先明确 RPO、单写者发布、崩溃完整性……本地文件即可验收……仅检查点不得承诺零丢失」；G26.b 验收=「native 与 JS provider 等价原子发布/读取」（G26.a 的周期保存即未来 native 参考实现，发布语义须经故障注入验收）；G27.b 依赖 G26.a 的「原子发布基础」，不预留接口。
- 承诺出处：mod-complete-gameplay:116「故障注入包含写数据未完成、manifest 切换前后和重启，报告所选耐久策略/RPO」（唯一出处）；:28 周期调度显式留给 G26.a。ADR-004 决策 4（约 2 分钟周期为设计起点、无默认运行中间隔——间隔按负载定）；ADR-008（后台 I/O 归异步域→G27.a）。roadmap 侧挂点 :66（M3/M4 不含）。
- 测试先例可复用：embed/checkpoint/tests（冷恢复等值模板、开放轮次门）、sim_tests/checkpoint.rs（12+5 拆分双通道等值、**checkpoint_io_failure 保旧版不谎报**——失败语义最直接模板）、input_log.rs 跨进程配对、replay.rs 续跑跳旧段；长跑场景首选 sim-economy（≥500 tick 多阶段）。
- 文档挂点 10 处（checkpoints.md:3/:59/:63+、storage.md:3/:30/:36、roadmap.md:66/:78、state-inventory 导语、local-simulation.md:3/:12、input-log.md:81、两处 README+根 README、台账:34 滞后句、mod-complete-gameplay:28）。

## 范围拍板（登记时定案，详见台账当前任务行）

挂点=sim 编排层 Ok 后；mods_switch 互斥拒绝；目录 `ckpt-{tick:010}.json` 逐份原子发布+keep-K；`--load-checkpoint` 接受目录走「最新可用」选择（坏文件降级、零段待重放合法终态、撕裂日志失败但带 valid_segments 不自动截断）；Metadata 增 `log_cursor`；保存失败=checkpoint_failed 退出 1；报告 v10 `auto_checkpoint`+`recovery` 字段兑现 RPO 承诺；守卫批次互斥。不做：embed RPC/异步线程/provider/增量/切换组合/自动截断。

## 验收计划

- sim 端到端：every N 多份文件+报告字段+keep 删旧；目录恢复+重放到尾与直跑逐位相等（减 cpu 桶）+--verify-replay 组合；坏最新份降级；零段合法终态；撕裂日志失败携带 valid_segments；mods_switch 互斥；路径互斥守卫；v10 两路同升。
- schema 钉死门不红（不新增载荷域）。
- 文档：checkpoints.md 新节+全部挂点+三处滞后句清偿。

## 交付记录

（2026-10-07 实现完成后回填；提交 ID 见 git log。）

实现落点：

- `kk-cli/src/sim/checkpoint.rs`：`Metadata` 增 `#[serde(default)] log_cursor{last_seq,last_tick}`；`checkpoint_file_name`（`ckpt-{tick:010}.json`）；`recover_latest`（「最新可用」选择：降序逐份信封+SHA 校验、坏文件跳次新计数、检查点时间 ≤ 日志末段+1 硬门、零段合法终态特判、撕裂日志读取处失败并指明人工截断——不自动改写）；`save` 增游标参数（终态保存同带）。
- `kk-cli/src/sim.rs`：`--checkpoint-every/--checkpoint-dir/--checkpoint-keep`（前置校验：every 与目录同进同出、keep 需 every、mods_switch 互斥、目录恢复需 --input-log）；循环内保存点=每轮成功且无失败玩家后（已提交/日志段已写/无开放轮次），保存失败即 `checkpoint_failed` 退出 1；目录恢复流（recover_latest→resume 身份门→`replay_input_rounds` 重放到日志尾→续跑）；报告 v9→**v10** 顶层 `auto_checkpoint`（间隔/份数/保留/最后 tick/保存耗时样本/RPO 声明）与 `recovery`（所选文件/检查点时间/游标/重放段数/跳过坏份数），`auto_checkpoint` 与目录形态 `recovery` 在 setup_failed 路径均非 null（两路同升）。
- `kk-cli/src/sim/auto_checkpoint.rs` 与 `kk-cli/src/sim/replay.rs`（收口前拆出，主文件回到 1000 行门槛内）：前者承载周期保存统计/到点保存/保留策略 `enforce_keep`/v10 字段装配；后者承载 `verify_in_process`（G25.b 进程内回放核对）与 `input_log_summary`。
- 守卫：`validate_paths` 增目录批次——其他输入/输出不得位于目录内、目录不得覆盖它们、目录不进包根。
- 测试 `sim_tests/auto_checkpoint.rs` 十二用例（复审后由七例扩充）：多份落盘+v10 字段+keep 删旧+RPO 数值+`--save-checkpoint` 同跑+未启用时两键 null；目录恢复零段终态（含 log_cursor.last_seq 钉死）+--verify-replay 组合+与直跑 7 轮逐位等值（减 CPU 桶）；从更旧份恢复重放 4 段仍等值（重放确定性）；坏最新份降级（信封翻转→选次新+重放 2 段+非规范命名文件不计坏份+降级终态等值）；撕裂日志拒绝且错误指明人工截断；检查点领先日志（time>末段+1）硬错误不降级；周期保存 I/O 失败=checkpoint_failed 退出 1 且 saved 停在 0；崩溃→恢复→继续周期保存闭环（新目录按世界时间对齐、日志续接 9 行不双录、与直跑 8 轮等值）；every=1 逐轮保存+间隔超总轮数零保存；失败玩家轮不保存（run_failed 而非 checkpoint_failed）；四组旗标互斥守卫+setup_failed 两键非 null+同目录「恢复+续存」被路径互斥拒绝（stderr，不写报告）；另列一用例钉 mods_switch 互斥与目录内输出拒绝。
- 文档：checkpoints.md 新「周期保存与恢复选择」节；storage.md 三处（含「输入日志」滞后句清偿）；roadmap.md:66（含迁移/输入日志滞后句清偿）；state-inventory 导语登记宿主持久面；local-simulation 旗标；input-log.md 写序关系句；三处 README；台账:34 边界滞后句清偿。

实现期修正：目录恢复守卫初版布尔式写反（目录存在即放行）；零段合法终态的判式是「检查点时间==日志末段+1」，世界时间与末段 tick 的差一关系要在测试里先算清（首版用例把「tick 4 检查点+段到 4」误当零段——实际要重放段 4）。

验收：完整 `pnpm run check` 见收口提交。

## 复审记录（2026-10-07，三路后台只读复审，全部清偿）

复审分工：协议与语义正确性 / 测试充分性与边界 / 文档与台账一致性。结论：0 P0；4 P1（3 测试缺口 + 1 台账父行滞后）全部修复；P2 逐条处置如下。

**P1 清偿**：

1. 「检查点时间 > 日志末段+1 硬门」零直接测试 → 新增 `checkpoint_beyond_log_tail_is_hard_error`（长日志目录 + 短日志恢复 → setup_failed，错误含「超出日志末段/摘除」）。
2. 周期保存 I/O 失败路径零测试 → 新增 `periodic_save_io_failure_fails_the_run`（预置同名目录使 rename 失败，钉 status/failure.stage/failure.tick/saved=0/committed=2）。
3. 「恢复后继续周期保存」组合零测试、同目录恢复被守卫阻断未钉 → 新增 `recovered_run_continues_periodic_checkpointing`（新目录按世界时间对齐落 ckpt-8、日志续接 9 行、与直跑 8 轮等值）+ guards 用例补同目录拒绝钉（stderr 断言 + 不写报告）。
4. 台账 G26 父行「未开始」与边界句矛盾 → 改「部分完成（.a 已交付，.b 待 G27.a）」。

**P2 清偿（修复类）**：

- recover_latest/enforce_keep 对非规范命名文件（如手工 `ckpt-9.json`）误计坏份/不可见 → 统一 `list_checkpoint_ticks` 只认规范命名，外部文件被忽略（测试钉死不计坏份）。
- `enforce_keep` 的 `read_dir` 失败经 `?` 逸出会把运行中段 I/O 失败伪装成 setup_failed（已保存统计全丢）→ 改为不致命记入 `delete_errors`。
- log_cursor 手工解析 `unwrap_or(0)` 宽松 → 改 `serde_json::from_value::<LogCursor>` 严格解码，畸形游标早失败并指名。
- 日志不可读错误一律附「人工截断」提示（权限/不存在等也误导）→ 按错误文本含「撕裂」门控，仅撕裂尾附修复提示。
- setup_failed 报告缺 `recovery` 键（与 v8/v9/v10 「启用时非 null」契约不对称）→ 目录形态启用时补 `{"status":"setup_failed"}`（guards 用例钉死）。
- `--checkpoint-every` help「每 N 轮提交后」与按世界时间整倍数对齐的实际判式不符 → help 与 local-simulation.md 均改为「提交后世界时间为 N 整倍数的轮次」。
- 台账:10（⑥）登记措辞「恢复到 tick/跳过坏文件数」与实际字段不符 → 改 checkpoint_time/log_cursor 字段名；台账:23 队列指针滞后（G26.a 仍列首选）→ 前移至 G07.b；note「五组互斥守卫」计数错误 → 改四组+另列一用例；state-inventory「log_cursor 不进 checkpoint() 载荷」措辞不精确 → 改「不新增引擎载荷域、不进键树门禁（不透明叶子）」；local-simulation.md:3 导语补周期保存提法；checkpoints.md 补差一说明（auto_checkpoint.last_tick vs recovery.log_cursor.last_tick）、非规范文件忽略、keep 清点失败不致命。
- 既有测试补钉：RPO 数值段、delete_errors==[]、plain 运行两键 null、log_cursor.last_seq、坏份降级终态等值、switch 用例 stderr 断言简化（去掉 unwrap_or 回退）。

**P2 处置（记录不改，附理由）**：

- 信封/SHA 合法但载荷缺 `engine.world.time` 的文件走硬错误而非跳过计数：自产文件必含该键，缺失=构造性畸形，fail-loud 优于误计坏份（与「关系问题不降级」同口径）。
- 同目录「恢复+继续保存」当前被路径互斥守卫拒绝：属有意取舍（目录由自动策略管理），放宽为「load 目录==保存目录允许」列为后续切片；测试已钉死现行为，放宽时翻红提示。
- rpo 字符串「最多丢 every tick」在跨目录手动换存目录场景可被超过：文案已有「不承诺零丢失」兜底，且单目录闭环（含恢复续存）不超窗；跨目录工作流待同目录放宽时一并复审。
- keep 删除失败注入（delete_errors 非空路径）无测试钩子：删除失败已不致命且记入报告，成功路径断言空数组；注入钩子不值当。
- `Case` 与 sim_tests.rs `Fixture` 功能近似：本文件用例需要自定义 bot 代码与目录形态，保留独立夹具但加原子计数消除 tag 撞名风险。
