# 常驻服务宿主（S2.2 现行）

状态：**现行**（S1.2 常驻基座 + S2.1 命令接收/恢复下限 + S2.2 命令消费与代码边界事务）。`kkscreeps serve init/run` 提供可常驻运行、无审计日志冷恢复的服务宿主；v2 目录经 `enable-inbox` 升级后具备耐久命令面（接收、FIFO 消费、code.replace 即时屏障）。玩家表达式命令、outbox 与 HTTP/登录随 S2.3/S2.4/S3 接入。方案全文见[服务三批方案](../plans/service-playable-batches.md)；文件与目录基座见[基础检查点](checkpoints.md)的[服务目录检查点](checkpoints.md#服务目录检查点s11)节。

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

## 耐久命令面（S2.2 现行：接收、消费与代码边界事务）

- `kkscreeps serve enable-inbox --config <file>`：v1→v2 显式一次性启用（可重入续接）。租约内顺序：恢复 v1 最新完整份 → 发布初始 v2 屏障检查点（世界与时间不变，仅元数据升级，屏障身份 `b-<32hex>`）→ 写空 inbox → 写初始 effects 锚点 → 最后原子重写 identity 为 v2（完成边界）。中断后重跑即续接：沿用已发布的 v2 屏障检查点、补齐缺失的 inbox/effects（identity 未切时无接收入口，inbox 必为空或测试注入态且保留不覆盖）、identity 已 v2 则拒绝重复启用。
- `inbox.json` v1（`kk_runtime::service::inbox`）：单一原子发布的有界最新状态（活跃流/nextServerSeq/水位/未淘汰明细）。幂等键 `(principal,streamId,clientSeq)`：同规范载荷返回原接收状态、异载荷冲突、淘汰后统一结果过期不重新执行；clientSeq 跳号拒绝；每主体 ≤4 活跃流；预算 256 条未终态/单主体跨流合计 8 条/16MiB，任一先到即拒；关闭有在途命令的流被拒、退役留墓碑不复用。接收是进程内库 API（宿主未来接线），received 只在文件+父目录同步后成立；文件损坏=停服条件不回退旧值。
- 命令消费（S2.2，`service::host::commands`）：serve run 在运行态、拍间（无开放轮次）调用 `drain_command_prefix`——按接收序（server_seq）全局 FIFO 消费最老未终态前缀，每轮固定批量 4 条防垄断；消费后不回驱动循环顶重入（突发命令不得级联清空整个 inbox 才放行首个 tick）。每笔命令同步应用并立即持久化终态：**completed** 携带结果 JSON（形状与崩溃后幂等补全的重构体一致），**rejected** 携带原因；载荷与授权校验一律验证拒绝并持久化终态（防队首毒消息），但 rejected 不建屏障——屏障只锚定 completed 效果的外部确认，拒绝的消费水位由 inbox 文件自身的 applied_seq 表达。命令面的存储/基础设施错误（inbox 读写、模块切换、屏障发布，含 effects 损坏）统一按持久化故障冻结推进（reason 如实记来源），不与拍推进交错重试；drain 数据面的 Err 均伴随 fault（未分类错误兜底冻结，不形成无故障态活锁；前置守卫断言除外——调用方仅运行态无故障分支可达）。
- `code.replace` 命令（S2.2）：载荷 `{modules: {name: source}, expectedCodeVersion, entry?}`；流主体必须是注册玩家（授权校验拒绝）。codeVersion CAS——expected 不等于当前版本即验证拒绝（过期不静默覆盖，保旧版现役）；校验（非空模块表、字符串源码、总字节 ≤2MiB、入口存在且为 JS）失败全部拒绝不动现役。通过后切换工作者模块表（玩家 Memory/segments/CPU 不动、不推进世界时间），发布**即时屏障**检查点：新屏障身份、codeVersion+1、消费水位；模块顶层在下一正常玩家执行阶段运行。宿主拍的执行入口同步派生（入口变更即切换 require 目标）。
- 崩溃窗口两段确认无死锁：屏障发布后、inbox 终态存档前中断——恢复选择落在新屏障链（meta 水位已含该命令），drain 对"水位已含仍 Received"的条目按可推导结果幂等补全（code.replace 的完成结果=当前 codeVersion，由版本号重构，不重新执行）；effects 未更新则新屏障候选被恢复下限过滤、世界回到旧链重新消费。未知命令 kind 一律 rejected 终态。
- `effects.json` v1（`service::effects`）：恢复下限锚点（屏障检查点 seq/barrier_id/inbox 消费水位）+各 sink 确认水位（S2.4 起填）。不随游戏检查点回滚。
- 检查点 v2 元数据：barrier_id/inbox_applied/code_versions/outbox_cursor 四字段齐备（`is_v2_complete`），普通保存继承恢复版本的屏障与水位；保留策略恒保护锚点序号。恢复选择（`select_latest` 带 `RecoveryFloor`）：v2 目录中 v1 形状旧份跳过（skipped_ineligible 计数），只允许锚点本身或携带同一屏障身份、消费水位不低于下限的后继完整版本；无满足者停服，不回退锚点之前。v2 目录 run 启动要求 inbox/effects 齐备且身份一致，缺一拒启。
- 未交付（后续切片）：玩家表达式与受限 Mod 输入命令（S2.3）、outbox 投递与具名 sink（S2.4）、淘汰接线与 evict 前置（S2.2/S2.3 随消费推进）、模块预算服务配置化（S3）、HTTP 接收入口（S3）。

## 边界与剩余缺口

- 引擎/V8 由宿主单线程所有；engine 不依赖文件 I/O，存储原语经 kk-storage。宿主不接触网络；信号与 stdio 只表达操作者意图。
- S3 在同一宿主上增加 HTTP/WS 监听与授权投影；玩家表达式命令（S2.3）、outbox（S2.4）、淘汰接线与模块预算配置化随后续切片。
- 平台：macOS/Linux 本地文件系统已验证（含 flock 租约与 XNU 残锁怪僻的规避，见 kk-storage lease 模块注释）；网络文件系统的锁与耐久保证不默认成立。Linux 侧 flock 未单独实证。
- 命令面已知形态：命令切换后屏障发布失败冻结时，停止流程的收尾保存可能发布"引擎已持新模块表 + service meta 停留旧版本"的分歧检查点——恢复下限按同屏障同水位放行该候选，重启后 CAS 基准随 meta 回滚、drain 先于任何 tick 重新消费收敛（评审 B 路真实二进制注入实证）；不构成客户端可见损失。
