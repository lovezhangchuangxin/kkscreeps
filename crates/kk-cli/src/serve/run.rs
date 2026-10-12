//! serve run：恢复、常驻调度与停止。
//!
//! 退出码：0=正常停止（收尾保存成功或无需保存）；1=启动/恢复失败（拒绝
//! 运行，不动状态）；2=需操作者关注——停止时存在未处置失败轮、收尾保存
//! 失败，或曾在持久化故障下停止（即使收尾保存随后成功：故障意味着发布
//! 链路异常，应人工核查目录）。调度策略在 kk-runtime::service::host（未
//! 超时锁网格锚+间隔、超时延后一个间隔、不补拍；命令打断不滑动截止点——两点接线见 drive）；本文件只做装配与控制流。
use crate::serve::control::{
    self, ControlCommand, WaitOutcome, install_stop_signals, nap, parse_command, wait_for_control,
    wait_until,
};
use anyhow::Context;
use kk_runtime::service::host::{Fault, HostParameters, ServiceHost, StopOutcome, next_deadline};
use kk_runtime::service::{self, ServiceDir};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(crate) fn run(config_path: &Path, control_stdio: bool) -> i32 {
    let (mut host, runtime, control) = match setup(config_path, control_stdio) {
        Ok(setup) => setup,
        Err(error) => {
            eprintln!("serve run 失败: {error:#}");
            return 1;
        }
    };
    let Runtime {
        tick_interval,
        exit_wait,
        lease: _lease,
    } = runtime;
    install_stop_signals();
    eprintln!(
        "serve run: world {} epoch {} 自检查点恢复（tick {}），间隔 {}ms",
        host.status()["world_id"],
        host.status()["run_epoch"],
        host.status()["live_tick"],
        tick_interval.as_millis()
    );

    // 驱动调度两点状态：anchor=最近已消费截止点（网格锚），pending=
    // 当前等待中截止点（None=下轮重算）。
    let mut anchor: Option<Instant> = None;
    let mut pending: Option<Instant> = None;
    // 距上次耐久保存的墙钟基准：保存成功或进程启动时重置。
    let mut reference = Instant::now();
    let code = drive(
        &mut host,
        tick_interval,
        &control,
        &mut anchor,
        &mut pending,
        &mut reference,
    );
    // 停止收尾：关闭写准入后由 finish_stop 保存/关闭引擎；退出看门狗
    // 保证 native 阻塞不会把“优雅退出”拖成无限等待（超时如实报告并
    // 以需关注码退出，不把强制退出写成成功）。
    host.request_stop();
    let finished = Arc::new(AtomicBool::new(false));
    let watchdog = {
        let finished = Arc::clone(&finished);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(exit_wait));
            // 到点后先二次确认：调度循环退出到此处仍需跨线程可见的收尾
            // 时间，直接强杀会把近在咫尺的成功误报为阻塞。
            if !finished.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(250));
            }
            if !finished.load(Ordering::SeqCst) {
                eprintln!(
                    "serve run: 优雅退出未在 {exit_wait}s（含 250ms 宽限）内完成（native 调用阻塞？），强制退出；最后耐久 tick 见停止前状态"
                );
                std::process::exit(2);
            }
        })
    };
    let outcome = host.finish_stop();
    finished.store(true, Ordering::SeqCst);
    drop(watchdog);
    let (exit_code, summary) = match outcome {
        StopOutcome::Completed { saved } => (
            code,
            json!({"stop": "completed", "saved": saved.map(|(seq, tick)| json!({"publish_seq": seq, "tick": tick}))}),
        ),
        StopOutcome::BlockedByFailedRound { last_durable_tick } => (
            2,
            json!({"stop": "blocked_by_failed_round", "last_durable_tick": last_durable_tick,
                   "note": "存在未处置失败轮：未保存未丢弃，请检查状态后处置"}),
        ),
        StopOutcome::SaveFailed { reason } => (
            2,
            json!({"stop": "save_failed", "error": reason,
                   "note": "收尾保存失败：已提交状态未确认耐久"}),
        ),
    };
    let mut status = host.status();
    status["stop"] = summary;
    println!("{status}");
    exit_code
}

struct Runtime {
    tick_interval: Duration,
    exit_wait: u64,
    /// 目录租约：持有至 run 结束（含收尾保存）后才释放。
    lease: kk_storage::WriterLease,
}

/// 控制通道：未开 stdio 时持有发送端已丢弃的断开通道（try_recv 恒
/// Disconnected，等待循环自然只看截止点与停止信号），stdio 关闭后同理。
enum Control {
    Disconnected(std::sync::mpsc::Receiver<String>),
    Stdio {
        receiver: std::sync::mpsc::Receiver<String>,
    },
}

impl Control {
    fn receiver(&self) -> &std::sync::mpsc::Receiver<String> {
        match self {
            Control::Disconnected(receiver) | Control::Stdio { receiver } => receiver,
        }
    }
}

fn setup(
    config_path: &Path,
    control_stdio: bool,
) -> anyhow::Result<(ServiceHost, Runtime, Control)> {
    let (config, state_dir) = service::config::ServeConfig::load(config_path)?;
    let dir = ServiceDir::new(&state_dir);
    dir.validate_for_run().context("服务目录不可运行")?;
    // 写权覆盖整个运行期：恢复选择、清理与全部发布都在租约内；身份
    // 在租约内读取，防止与 enable-inbox 的身份切换竞态（以过期版本
    // 跑目录会绕过 v2 锚点保护）。
    let lease =
        kk_storage::WriterLease::acquire(&dir.writer_lock()).context("获取服务目录写者租约")?;
    let identity = service::identity::read(&dir.identity_file()).context("读取服务身份失败")?;
    for clean_dir in [dir.root(), &dir.checkpoints_dir()] {
        let cleanup = kk_storage::publish::remove_stale_temporaries(clean_dir)?;
        for failure in cleanup.failed {
            eprintln!("serve run: 残留临时文件清理失败（不阻塞）: {failure}");
        }
    }
    // v2 目录：inbox/effects 必须存在且合法（缺一即拒——不猜测缺失
    // 文件）；恢复下限来自 effects 锚点。v1 目录维持 S1 语义。
    let v2_floor =
        if identity.service_format_version == kk_runtime::service::identity::SERVICE_FORMAT_V2 {
            let effects = service::effects::load(&dir.effects_file(), &identity.world_id)
                .context("v2 目录缺合法恢复锚点（effects.json）；须人工核验，不自动重建")?;
            service::inbox::load(&dir.inbox_file(), &identity.world_id)
                .context("v2 目录缺合法命令收件箱（inbox.json）")?;
            Some(service::checkpoints::RecoveryFloor {
                checkpoint_seq: effects.anchor.checkpoint_seq,
                barrier_id: effects.anchor.barrier_id.clone(),
                inbox_applied: effects.anchor.inbox_applied.clone(),
            })
        } else {
            None
        };
    let selected =
        service::checkpoints::select_latest(&dir.checkpoints_dir(), &identity, v2_floor.as_ref())
            .context("无日志恢复选择失败")?;
    // 配置声明的堆上限对恢复工作者是硬一致性约束（与 sim 同口径）。
    if let Some(expected) = config.player_heap_limit_bytes {
        for (name, worker) in selected.value["workers"].as_object().into_iter().flatten() {
            anyhow::ensure!(
                worker["heap_limit_bytes"].as_u64() == Some(expected),
                "配置 player_heap_limit_bytes={expected} 与检查点工作者 {name} 的堆上限不一致"
            );
        }
    }
    let loaded = config
        .mods
        .as_ref()
        .map(|source| {
            let input = crate::sim::mods::Mods {
                root: source.root.clone(),
                configs: source.configs.clone(),
                selection: source.selection.clone(),
            };
            let base = config_path.parent().unwrap_or(Path::new("."));
            crate::sim::mods::load(base, &input)
        })
        .transpose()
        .context("装载 Mod 包失败")?;
    let mods = loaded
        .as_ref()
        .map(|loaded| (&loaded.packages, &loaded.options));
    let mut engine = kk_runtime::EmbedEngine::new();
    engine
        .restore_checkpoint(selected.value.clone(), mods)
        .context("检查点恢复失败（引擎/Mod 身份核验）")?;
    engine.set_player_concurrency(config.player_concurrency)?;
    engine.set_kkpv_channel(matches!(config.channel, service::config::Channel::Kkpv));

    let anchor_seq = v2_floor
        .as_ref()
        .map(|floor| floor.checkpoint_seq)
        .unwrap_or_default();
    // v2 上下文先取值再移交 entries（避免部分移动后借用）。
    let v2_context = v2_floor.as_ref().map(|floor| service::host::V2SaveContext {
        barrier_id: selected
            .meta
            .barrier_id
            .clone()
            .unwrap_or_else(|| floor.barrier_id.clone()),
        inbox_applied: selected.meta.inbox_applied.clone().unwrap_or_default(),
        code_versions: selected.meta.code_versions.clone().unwrap_or_default(),
        outbox_cursor: selected.meta.outbox_cursor.unwrap_or_default(),
        anchor_seq,
    });
    let mut host = ServiceHost::new(
        engine,
        dir,
        identity,
        selected.meta.entries,
        HostParameters {
            auto_save_ticks: config.auto_save_ticks,
            auto_save_wall_secs: config.auto_save_wall_secs,
            keep_checkpoints: config.keep_checkpoints,
        },
        service::host::RecoverySummary {
            selected: selected
                .file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            publish_seq: selected.publish_seq,
            tick: selected.meta.tick,
            skipped_broken: selected.skipped_broken,
        },
    )
    .context("装配服务宿主")?;
    if let Some(context) = v2_context {
        // v2 保存上下文继承恢复版本的屏障与水位（S2.1 恒为启用初值；
        // 真实推进自 S2.2 起由命令完成事务建立新屏障）。
        host.set_v2_context(context);
    }

    let control = if control_stdio {
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Control::Stdio { receiver }
    } else {
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        drop(sender);
        Control::Disconnected(receiver)
    };
    Ok((
        host,
        Runtime {
            tick_interval: Duration::from_millis(config.tick_interval_ms),
            exit_wait: config.exit_wait_secs,
            lease,
        },
        control,
    ))
}

/// 主驱动循环：返回停止原因码（0=操作者停止；2=持久化故障下只能停止）。
/// 期间所有状态变化经宿主；控制命令逐行应答。
fn drive(
    host: &mut ServiceHost,
    tick_interval: Duration,
    control: &Control,
    anchor: &mut Option<Instant>,
    pending: &mut Option<Instant>,
    reference: &mut Instant,
) -> i32 {
    let receiver = control.receiver();
    // 故障提示只在原因变化时打印一次，避免常驻循环刷屏。
    let mut announced_persistence: Option<String> = None;
    loop {
        if control::stop_requested() {
            return stop_code(host);
        }
        if let Ok(line) = receiver.try_recv() {
            match handle_command(host, &line, reference) {
                Some(code) => return code,
                None => continue,
            }
        }
        match host.fault() {
            None if host.phase() == kk_runtime::service::Phase::Running => {
                // pending 已锚定则继续等同一截止点（命令打断不滑动、
                // 首拍也不被命令流无限推迟）；无 pending 才按网格锚重算。
                let deadline = match *pending {
                    Some(deadline) => deadline,
                    None => {
                        let deadline = next_deadline(*anchor, Instant::now(), tick_interval);
                        *pending = Some(deadline);
                        deadline
                    }
                };
                match wait_until(deadline, receiver) {
                    WaitOutcome::Stop => return 0,
                    WaitOutcome::Command(line) => {
                        if let Some(code) = handle_command(host, &line, reference) {
                            return code;
                        }
                    }
                    WaitOutcome::Deadline => {
                        // 消费截止点：记录网格锚并清 pending；下一轮
                        // next_deadline 据此锁定网格（快速拍零漂移），
                        // 拍超时才从完成时刻延后一个间隔。
                        *anchor = Some(deadline);
                        *pending = None;
                        run_one_tick(host, reference);
                    }
                }
            }
            Some(Fault::Paused { .. }) => {
                // 暂停：保持 tick，等待显式 retry/stop；不自动重派。
                match wait_for_control(receiver) {
                    WaitOutcome::Stop => return 0,
                    WaitOutcome::Command(line) => {
                        if let Some(code) = handle_command(host, &line, reference) {
                            return code;
                        }
                    }
                    WaitOutcome::Deadline => nap(),
                }
            }
            Some(Fault::Persistence { reason }) => {
                if announced_persistence.as_deref() != Some(reason.as_str()) {
                    eprintln!("serve run: 持久化故障，停止推进（只接受停止）: {reason}");
                    announced_persistence = Some(reason.clone());
                }
                match wait_for_control(receiver) {
                    WaitOutcome::Stop => return 2, // 与 stop_code 同归类
                    WaitOutcome::Command(line) => {
                        if let Some(code) = handle_command(host, &line, reference) {
                            return code;
                        }
                    }
                    WaitOutcome::Deadline => nap(),
                }
            }
            _ => return 0,
        }
    }
}

/// 停止原因码：持久化故障下的停止恒为 2（需操作者关注），即使收尾
/// 保存随后成功；其余为 0。信号可落在循环任意位置（含 nap 窗口），
/// 停止码必须按当前故障归类而不是按"先看到信号"时的默认值。
fn stop_code(host: &ServiceHost) -> i32 {
    match host.fault() {
        Some(Fault::Persistence { .. }) => 2,
        _ => 0,
    }
}

/// 执行一拍并处理到点保存；tick 结果（含玩家错误）与保存失败如实记录。
fn run_one_tick(host: &mut ServiceHost, reference: &mut Instant) {
    match host.tick() {
        Ok(Some(report)) => {
            for (player, error) in &report.player_errors {
                eprintln!(
                    "serve run: 玩家 {player} 本拍错误（继续运行）: {}",
                    serde_json::to_string(error).unwrap_or_default()
                );
            }
        }
        Ok(None) => {
            if let Some(Fault::Paused { reason, .. }) = host.fault() {
                eprintln!("serve run: 拍暂停（保持 tick，等待显式重试）: {reason}");
            }
        }
        Err(error) => eprintln!("serve run: 拒绝开始新拍: {error:#}"),
    }
    maybe_save(host, reference);
}

fn maybe_save(host: &mut ServiceHost, reference: &mut Instant) {
    // 开放/失败轮存在时引擎拒绝快照（no_open_round 门），跳过等下一拍；
    // 故障期间同样不尝试（保持故障语义单一）。
    if host.fault().is_some() || host.engine().has_pending_round() {
        return;
    }
    if host.should_save(reference.elapsed().as_secs_f64()) {
        match host.save_now() {
            service::host::SaveOutcome::Saved { seq, tick } => {
                *reference = Instant::now();
                eprintln!("serve run: 检查点已耐久（seq {seq}，tick {tick}）");
            }
            service::host::SaveOutcome::Failed { reason } => {
                eprintln!("serve run: 检查点发布失败，冻结推进: {reason}");
            }
        }
    }
}

/// 处理一条控制命令；返回 Some(退出码) 表示收到停止。
fn handle_command(host: &mut ServiceHost, line: &str, reference: &mut Instant) -> Option<i32> {
    let reply = match parse_command(line) {
        Ok(ControlCommand::Status) => host.status(),
        Ok(ControlCommand::Retry) => match host.retry() {
            Ok(report) => {
                maybe_save(host, reference);
                json!({"ok": true, "advanced": report.is_some(),
                       "player_errors": report.map(|r| r.player_errors.len()).unwrap_or(0)})
            }
            Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
        },
        Ok(ControlCommand::Stop) => {
            println!("{}", json!({"ok": true, "stopping": true}));
            return Some(stop_code(host));
        }
        Err(error) => json!({"ok": false, "error": error}),
    };
    println!("{reply}");
    None
}
