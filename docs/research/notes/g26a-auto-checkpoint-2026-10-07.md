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

（实现完成后回填）
