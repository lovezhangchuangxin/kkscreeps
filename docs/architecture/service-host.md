# 常驻服务宿主（S1.2 现行）

状态：**现行**（S1.2 交付范围）。`kkscreeps serve init/run` 提供可常驻运行、无审计日志冷恢复的服务宿主；命令接收、代码更新与 outbox 随 S2 切片接入，HTTP/登录随 S3。方案全文见[服务三批方案](../plans/service-playable-batches.md)；文件与目录基座见[基础检查点](checkpoints.md)的[服务目录检查点](checkpoints.md#服务目录检查点s11)节。

## 命令与配置

- `kkscreeps serve init --config <file> --seed-scenario <scene>`：显式一次性初始化。顺序不可绕过：目录校验（拒绝已初始化/半初始化/含未知内容的目录）→ 建骨架 → 获取目录租约 → 清理协议临时残留 → 生成身份值 → 按 sim 同机制装载种子场景并装配引擎（含对象放置、玩家模块/Memory、堆上限、并发/通道）→ 初始检查点耐久发布 → **最后**原子写 `identity.json`（完成标记）。identity 写入前崩溃=半初始化：run 与再次 init 都拒绝，处置=确认无价值后删除目录重来。初始化后运行期不再读取种子场景。
- `kkscreeps serve run --config <file> [--control-stdio]`：恢复并常驻运行。
- 配置为严格版本化 JSON（`config_format_version` 1，未知字段/未来版本拒绝，相对路径相对配置文件解析）：`state_dir`、`tick_interval_ms`（默认 1000）、`player_concurrency`（默认 2）、`channel`（默认 kkpv）、`player_heap_limit_bytes`（对恢复工作者是硬一致性约束）、`auto_save_ticks`/`auto_save_wall_secs`（默认 60/60，先到者触发）、`keep_checkpoints`（默认 3）、`exit_wait_secs`（默认 10）、`mods`（run 恢复时装载并经检查点 ModIdentity 核验）。注意 `mods.root` 的相对基准与 `state_dir` 一致取配置文件所在目录，而 init 的种子场景内模块路径（`bots[].modules`）相对场景文件——两个基准不同，混放时须显式相对化。

## 状态机与调度

生命周期 `Running → Draining → Stopped`；故障不成独立状态而挂在 fault 上并冻结推进：`Paused`（结算失败——重试只重结算复用缓存候选；执行故障——玩家线程/协议层，不自动重派，重试先显式重建工作者；或基础设施故障——如 Mod realm 待恢复，无缓存轮，重试直接重进拍入口）与 `Persistence`（发布耐久性不确定，retry 不可用，只能停止）。错误状态拒绝新拍，保留只读状态（status）与诊断。retry 只在 Running 阶段可用（停止流程中拒绝）。

调度由纯函数 `service::host::next_deadline` 表达并由虚拟时钟测试锁定：网格锚=最近已消费截止点，下一截止点=锚+间隔（快速拍不累积漂移）；拍用时超过间隔则从完成时刻延后一个间隔（过载降低实际 tick 速率，不补拍、不突发追赶停机积压；进程重启无历史锚点）。驱动（serve/run.rs）为两点接线：等待中截止点 pending 在进入等待前锚定，控制命令打断等待后**继续等同一截止点**（不滑动不饿死，命令处理越过截止点才按超时语义延后）；截止点消费执行拍后记入网格锚。单调时钟调度，不用游戏时间计算等待；控制/信号以 25ms 轮询粒度插队响应。

玩家级错误（异常/CPU 中断按现行玩家结局语义结算）只计入统计与 `recent_player_errors`（有界 20 条）并继续运行，**不**沿用 sim 的“玩家错误终结整场景”政策。

## 恢复与周期保存

run 启动：`validate_for_run`（不做任何创建）→ 读 identity（缺失/损坏即拒绝，exit 1）→ 获取目录租约（覆盖整个运行期含收尾保存；第二进程立即被拒，进程退出由 OS 释放）→ 清理协议临时残留 → 无日志恢复选择（撕裂/哈希损坏跳过并计数；I/O/未来格式/身份不一致停止）→ 配置堆上限与检查点工作者一致性核验 → 替代引擎恢复（引擎/Mod 身份核验）→ 宿主装配（runEpoch 每进程新生成）。

周期保存：距上次耐久保存 `auto_save_ticks` 拍或 `auto_save_wall_secs` 秒先到，且确有脏拍才发布（无状态变化不重复写相同版本）；发布序号每次尝试按目录实况分配（不缓存，防失败后环境变化脱节）；保存后执行完整版本保留策略（S1 无保护锚点，`protected=[]`）。发布失败 → `PersistenceFault` 冻结推进，消息附带**目标文件实证**（探测目标名：内容已是新版本=rename 已发生而目录同步未确认的耐久未知形态；不可读=旧版本未受影响），满足“rename 后父目录同步失败不能按旧文件一定还在重试”的判别要求。存在开放/失败轮时引擎拒绝快照（no_open_round 门），跳过保存等下一拍。

## 控制入口与停止

`--control-stdio` 提供逐行 JSON（`{"command":"status"|"retry"|"stop"}`，应答单行 JSON；非法输入回错误行不中断）。SIGTERM/SIGINT 只置原子标志（异步信号安全），宿主在拍间消费。进程持有者是管理主体，本批不开放网络端点。

停止流程：关闭写入准入（Draining，拒绝开始新拍）→ 在途轮次为同步执行、拍间处理命令 → `finish_stop`：存在未处置失败轮时不擅自保存/丢弃（`BlockedByFailedRound`，报告最后耐久 tick，exit 2）；否则有脏拍则收尾保存（失败如实 `SaveFailed`，exit 2）→ 关闭引擎线程。停止码按**当前故障**归类（信号可落在循环任意位置，含暂停等待窗）：持久化故障下的停止恒为 2，即使收尾保存随后成功——故障意味着发布链路异常，应人工核查目录。退出看门狗在 `exit_wait_secs` 后二次确认（250ms 宽限）再强制退出并如实报告（不把强制退出写成优雅退出）；它只兜底收尾阶段，不覆盖在途拍（在途拍由 `exit_wait` 窗本身容纳）。退出码：0=正常停止；1=启动/恢复失败（不动状态）；2=需操作者关注——失败轮未处置、收尾保存失败，或曾在持久化故障下停止（耐久状态完好）。

状态摘要（status 应答与最终输出）包含 `world_id/run_epoch/phase/fault/live_tick/durable_tick/publish_seq/next_publish_seq/dirty_ticks/ticks_total/last_save_ms/last_error/recovery_selected/skipped_broken/players/recent_player_errors`；`live_tick` 在停止收尾时冻结为停止时刻值（teardown 重置引擎后不归零）。RPO 按最新成功保存报告：60 拍/60 秒是健康策略，保存耗时、故障或降级恢复会扩大窗口，不承诺严格一分钟零损失。

## 边界与剩余缺口

- 引擎/V8 由宿主单线程所有；engine 不依赖文件 I/O，存储原语经 kk-storage。宿主不接触网络；信号与 stdio 只表达操作者意图。
- S3 在同一宿主上增加 HTTP/WS 监听与授权投影；命令边界、outbox、恢复下限随 S2。
- 平台：macOS/Linux 本地文件系统已验证（含 flock 租约与 XNU 残锁怪僻的规避，见 kk-storage lease 模块注释）；网络文件系统的锁与耐久保证不默认成立。Linux 侧 flock 未单独实证。
- 本批不包含：同 tick 代码边界事务（S2.2）、命令查询（S2.1/S3）、断线重连（S3）。
