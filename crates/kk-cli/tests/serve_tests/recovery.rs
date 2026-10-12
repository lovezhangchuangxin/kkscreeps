//! S1.3 连续运行与冷恢复验收：两玩家/多房/Mod 场景在真实子进程
//! init→run→SIGTERM→再 run 的续存链路上，核对世界、玩家 Memory/CPU、
//! 模块入口与 Mod 状态；JSON/KKPV 双通道等值；撕裂最新份回退到次新
//! 完整版（恢复不越过已确认效果下限）；同步失败不发布新完整检查点。
//!
//! 核对分两层：子进程链路验证 serve 行为（退出码/控制应答/续存），
//! 测试进程内经 `service::checkpoints::select_latest` + 引擎
//! `restore_checkpoint` 复原检查点后用管理快照接口逐项断言——与
//! serve run 的恢复选择同一入口，等值性由同一份载荷保证。
use std::collections::BTreeMap;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use super::{KillOnDrop, read_status_line, serve, wait_for};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// 两玩家/两房/nursery Mod 场景夹具（对齐 compat/fixtures/sim-mod-objects
/// 的形态，模块与 Mod 根改为测试自管路径）。
pub(super) struct Fixture(PathBuf);
impl Fixture {
    pub(super) fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-recovery-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("bots"))?;
        // 每拍计数并把最后执行拍写进 Memory：恢复后 loops 增量即真实
        // 执行拍数，lastTick 证明 Memory 是检查点时刻的落点。
        std::fs::write(
            path.join("bots/main.js"),
            "exports.loop = () => { Memory.loops = (Memory.loops || 0) + 1; Memory.lastTick = Game.time; };",
        )?;
        let mods_root = Self::mods_root()?;
        std::fs::write(
            path.join("scenario.json"),
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "seed": 21,
                "mods": {
                    "root": mods_root.to_string_lossy(),
                    "configs": {"nursery": {"period": 5, "life": 3}},
                },
                "shard": {
                    "players": ["p1", "p2"],
                    "rooms": [
                        {"name": "W1N1", "owner": "p1"},
                        {"name": "W2N2", "owner": "p2"},
                    ],
                },
                "bots": [
                    {"player": "p1", "modules": "bots", "entry": "main", "memory": {"room": "W1N1"}},
                    {"player": "p2", "modules": "bots", "entry": "main", "memory": {"room": "W2N2"}},
                ],
                "objects": [
                    {"alias": "e1", "room": "W1N1", "kind": "mod",
                     "spec": {"typeKey": "nursery:emitter", "fields": {"x": 10, "y": 12}}},
                    {"alias": "e2", "room": "W2N2", "kind": "mod",
                     "spec": {"typeKey": "nursery:emitter", "fields": {"x": 20, "y": 22}}},
                ],
            }))?,
        )?;
        Ok(Self(path))
    }

    /// 官方内容包根（集成测试相对仓库根解析，与 scenario 内绝对 root
    /// 一致，避免夹具目录复制）。
    fn mods_root() -> anyhow::Result<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/official");
        anyhow::ensure!(root.is_dir(), "官方 Mod 包目录不存在: {}", root.display());
        Ok(root)
    }

    /// 写服务配置（channel 等差异经 extra 覆盖）。
    pub(super) fn config(&self, extra: Value) -> anyhow::Result<PathBuf> {
        let path = self.0.join("serve.json");
        let mut config = json!({
            "config_format_version": 1,
            "state_dir": "state",
            "tick_interval_ms": 40,
            "auto_save_ticks": 5,
            "auto_save_wall_secs": 3600,
            "keep_checkpoints": 5,
            "exit_wait_secs": 10,
            // run 恢复需要与检查点 ModIdentity 一致的装载输入；缺省
            // 会被恢复核验拒绝（exit 1）。
            "mods": {
                "root": Self::mods_root()?.to_string_lossy(),
                "configs": {"nursery": {"period": 5, "life": 3}},
            },
        });
        if let (Some(target), Some(source)) = (config.as_object_mut(), extra.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
        Ok(path)
    }

    fn scenario(&self) -> PathBuf {
        self.0.join("scenario.json")
    }

    pub(super) fn state(&self) -> PathBuf {
        self.0.join("state")
    }

    pub(super) fn checkpoints(&self) -> PathBuf {
        self.state().join("checkpoints")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn init_ok(fixture: &Fixture, config: &Path) -> anyhow::Result<Value> {
    let output = serve(&[
        "serve",
        "init",
        "--config",
        &config.to_string_lossy(),
        "--seed-scenario",
        &fixture.scenario().to_string_lossy(),
    ])
    .output()?;
    anyhow::ensure!(
        output.status.success(),
        "serve init 应成功: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// 启动常驻 run 并把 stderr 落文件；返回 (守卫子进程, stderr 路径)。
pub(super) fn spawn_run(
    config: &Path,
    fixture: &Fixture,
    tag: &str,
) -> anyhow::Result<(KillOnDrop, PathBuf)> {
    let stderr_path = fixture.0.join(tag);
    let child = KillOnDrop::spawn(
        serve(&["serve", "run", "--config", &config.to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(&stderr_path)?)),
    )?;
    Ok((child, stderr_path))
}

/// 测试进程内复原最新检查点：与 serve run 同一恢复选择入口，装载同
/// 一 Mod 包与配置（检查点 ModIdentity 核验通过即证明恢复输入一致）。
struct Recovered {
    engine: kk_runtime::EmbedEngine,
    selected: kk_runtime::service::checkpoints::Selected,
}

fn restore_in_process(state: &Path) -> anyhow::Result<Recovered> {
    let dir = kk_runtime::service::ServiceDir::new(state);
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let selected =
        kk_runtime::service::checkpoints::select_latest(&dir.checkpoints_dir(), &identity, None)?;
    let packages = kk_mod_host::discover_packages(&Fixture::mods_root()?)?;
    let options = kk_mod_host::RegisterOptions {
        configs: BTreeMap::from([("nursery".to_string(), json!({"period": 5, "life": 3}))]),
        ..Default::default()
    };
    let mut engine = kk_runtime::EmbedEngine::new();
    engine
        .restore_checkpoint(selected.value.clone(), Some((&packages, &options)))
        .context_proceed()?;
    Ok(Recovered { engine, selected })
}

/// 便于链式 Result 的轻量别名（anyhow::Context 的用法在测试里冗长）。
trait ContextProceed<T> {
    fn context_proceed(self) -> anyhow::Result<T>;
}
impl<T, E: std::fmt::Display> ContextProceed<T> for Result<T, E> {
    fn context_proceed(self) -> anyhow::Result<T> {
        self.map_err(|error| anyhow::anyhow!("进程内恢复失败: {error}"))
    }
}
impl<T> ContextProceed<T> for Option<T> {
    fn context_proceed(self) -> anyhow::Result<T> {
        self.ok_or_else(|| anyhow::anyhow!("进程内恢复失败: 期望的玩家状态缺失"))
    }
}

/// 玩家 Memory 中的 loops 计数。
fn loops_of(engine: &kk_runtime::EmbedEngine, player: &str) -> anyhow::Result<i64> {
    let raw = engine
        .player_state(player)
        .map(|state| state.raw_memory.clone())
        .context_proceed()?;
    Ok(serde_json::from_str::<Value>(&raw)?["loops"]
        .as_i64()
        .unwrap_or(0))
}

/// nursery 命名空间的 born 计数（Mod 状态随结算演化的证据）。
fn nursery_born(engine: &kk_runtime::EmbedEngine) -> i64 {
    engine.engine_mod_state("nursery")["born"]
        .as_i64()
        .unwrap_or(0)
}

/// 核对恢复态：世界时间=检查点 tick、模块入口、两玩家 Memory 落点、
/// Mod 状态与对象、以及世界仍可继续结算一拍（CPU/写回路径活性）。
fn verify_recovered(
    recovered: &mut Recovered,
    min_loops: i64,
    expected_skipped: usize,
) -> anyhow::Result<u64> {
    let tick = recovered.engine.get_game_time();
    anyhow::ensure!(
        tick >= 1 && tick == recovered.selected.meta.tick,
        "恢复后世界时间 {} 应等于检查点 tick {}",
        tick,
        recovered.selected.meta.tick
    );
    anyhow::ensure!(
        recovered.selected.skipped_broken == expected_skipped,
        "恢复选择跳过份数 {} 应等于预期 {expected_skipped}",
        recovered.selected.skipped_broken
    );
    let entries = &recovered.selected.meta.entries;
    anyhow::ensure!(
        entries.get("p1").map(String::as_str) == Some("main")
            && entries.get("p2").map(String::as_str) == Some("main"),
        "模块入口应保留两玩家 main：{entries:?}"
    );
    for player in ["p1", "p2"] {
        let loops = loops_of(&recovered.engine, player)?;
        anyhow::ensure!(
            loops >= min_loops,
            "玩家 {player} 的 Memory.loops={loops} 应 ≥ {min_loops}（Memory 未正确恢复）"
        );
        let raw = recovered
            .engine
            .player_state(player)
            .context_proceed()?
            .raw_memory
            .clone();
        let memory: Value = serde_json::from_str(&raw)?;
        let last_tick = memory["lastTick"].as_u64().unwrap_or(0);
        anyhow::ensure!(
            last_tick >= 1 && last_tick <= tick,
            "玩家 {player} 的 lastTick={last_tick} 应落在 (0, {tick}]"
        );
    }
    anyhow::ensure!(
        nursery_born(&recovered.engine) >= 1,
        "nursery Mod 状态应已随结算演化（born ≥ 1）"
    );
    // CPU 核对面直接读数：两玩家的持久 CPU 桶完整（有限预算/正上限，
    // 与引擎恢复校验同口径），而非只靠后续结算活性间接覆盖。
    for player in ["p1", "p2"] {
        let cpu = &recovered.engine.player_state(player).context_proceed()?.cpu;
        anyhow::ensure!(
            cpu.validate_checkpoint().is_ok() && cpu.limit > 0.0,
            "玩家 {player} 的 CPU 桶应完整有效（limit={}）",
            cpu.limit
        );
    }
    // 多房间核对落在对象分布上：两房各保留至少一个 nursery:emitter。
    let snapshots = recovered.engine.mod_object_snapshots();
    for room in ["W1N1", "W2N2"] {
        anyhow::ensure!(
            snapshots.iter().any(|object| {
                object["typeKey"] == "nursery:emitter"
                    && object["pos"]["roomName"].as_str() == Some(room)
            }),
            "房 {room} 应保留 nursery:emitter 对象"
        );
    }
    // 恢复后世界活性：空请求推进一拍（结算/CPU/写回路径完整走通）。
    recovered.engine.set_player_concurrency(2)?;
    let before = recovered.engine.get_game_time();
    recovered.engine.run_players(&BTreeMap::new())?;
    anyhow::ensure!(
        recovered.engine.get_game_time() == before + 1,
        "恢复后应能继续结算一拍"
    );
    Ok(tick)
}

/// 主链路（KKPV 通道）：init → 常驻跑 → SIGTERM 保存 → 进程内核对 →
/// 同目录再 run 续存 → 再核对（两玩家 Memory 继续增长、Mod 状态单调、
/// tick 不回退）。
#[test]
#[cfg(unix)]
fn two_player_mod_world_recovers_and_continues() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({"channel": "kkpv"}))?;
    let report = init_ok(&fixture, &config)?;
    assert_eq!(
        report["players"],
        json!({"p1": "main", "p2": "main"}),
        "init 报告应含两玩家入口"
    );

    let (mut run1, _) = spawn_run(&config, &fixture, "run1.err")?;
    wait_for(|| checkpoint_count(&fixture) >= 2)?;
    unsafe {
        libc::kill(run1.id() as i32, libc::SIGTERM);
    }
    let status = run1.wait()?;
    anyhow::ensure!(status.code() == Some(0), "run1 应优雅退出 0：{status:?}");

    let mut first = restore_in_process(&fixture.state())?;
    let tick1 = verify_recovered(&mut first, 2, 0)?;
    let (loops1_p1, loops1_p2, born1) = (
        loops_of(&first.engine, "p1")?,
        loops_of(&first.engine, "p2")?,
        nursery_born(&first.engine),
    );

    // 同目录续存：恢复选择非空、live_tick 自检查点继续增长。
    let mut run2 = KillOnDrop::spawn(
        serve(&[
            "serve",
            "run",
            "--config",
            &config.to_string_lossy(),
            "--control-stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()),
    )?;
    let mut stdout = BufReader::new(run2.child_mut().stdout.take().expect("stdout 已接管"));
    // 恢复选择必须指向当前最新完整份（而非旧份），然后才谈续存推进。
    let latest_file = {
        let mut seqs: Vec<u64> = std::fs::read_dir(fixture.checkpoints())?
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_prefix("cp-")?
                    .strip_suffix(".json")?
                    .parse()
                    .ok()
            })
            .collect();
        seqs.sort_unstable();
        format!("cp-{:012}.json", seqs.last().copied().unwrap_or_default())
    };
    let mut first_status = Value::Null;
    wait_for(|| {
        let sent = writeln!(
            run2.child_mut().stdin.as_mut().expect("stdin"),
            r#"{{"command":"status"}}"#
        )
        .is_ok();
        sent && read_status_line(&mut stdout)
            .ok()
            .and_then(|status| {
                first_status = status.clone();
                status["live_tick"].as_u64()
            })
            .is_some_and(|tick| tick > tick1 + 10)
    })?;
    anyhow::ensure!(
        first_status["recovery_selected"].as_str() == Some(latest_file.as_str()),
        "续存应自最新完整份恢复：{first_status}（期望 {latest_file}）"
    );
    writeln!(
        run2.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"stop"}}"#
    )?;
    let final_line = read_status_line(&mut stdout)?;
    assert_eq!(final_line["ok"], true, "stop 应答: {final_line}");
    let status = run2.wait()?;
    anyhow::ensure!(status.code() == Some(0), "run2 应退出 0：{status:?}");

    let mut second = restore_in_process(&fixture.state())?;
    let tick2 = verify_recovered(&mut second, loops1_p1.max(loops1_p2) + 1, 0)?;
    anyhow::ensure!(tick2 > tick1, "续存后 tick {tick2} 应超过 {tick1}");
    anyhow::ensure!(
        loops_of(&second.engine, "p1")? > loops1_p1 && loops_of(&second.engine, "p2")? > loops1_p2,
        "两玩家的 Memory.loops 都应继续增长"
    );
    anyhow::ensure!(
        nursery_born(&second.engine) >= born1,
        "nursery born 计数应单调不减"
    );
    Ok(())
}

/// JSON 通道与 KKPV 业务状态等值：同一场景在 json 通道走完整链路，
/// 恢复核对面与 KKPV 主链路相同（世界/Memory/入口/Mod 状态/活性）。
#[test]
#[cfg(unix)]
fn json_channel_recovery_preserves_player_and_mod_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({"channel": "json"}))?;
    init_ok(&fixture, &config)?;

    let (mut run1, _) = spawn_run(&config, &fixture, "json-run.err")?;
    wait_for(|| checkpoint_count(&fixture) >= 2)?;
    unsafe {
        libc::kill(run1.id() as i32, libc::SIGTERM);
    }
    let status = run1.wait()?;
    anyhow::ensure!(
        status.code() == Some(0),
        "JSON 通道 run 应优雅退出 0：{status:?}"
    );

    let mut recovered = restore_in_process(&fixture.state())?;
    let tick = verify_recovered(&mut recovered, 2, 0)?;
    anyhow::ensure!(tick >= 5, "JSON 通道链路应已推进足够拍数：{tick}");
    let loops1 = loops_of(&recovered.engine, "p1")?;

    // 跨进程续存（goal 要求 JSON/KKPV 双通道都覆盖连跑与续跑）：恢复后
    // 的玩家执行（JSON 注入路径重建工作者）由 loops 增长直接证明。
    let mut run2 = KillOnDrop::spawn(
        serve(&[
            "serve",
            "run",
            "--config",
            &config.to_string_lossy(),
            "--control-stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()),
    )?;
    let mut stdout = BufReader::new(run2.child_mut().stdout.take().expect("stdout 已接管"));
    wait_for(|| {
        let sent = writeln!(
            run2.child_mut().stdin.as_mut().expect("stdin"),
            r#"{{"command":"status"}}"#
        )
        .is_ok();
        sent && read_status_line(&mut stdout)
            .ok()
            .and_then(|status| status["live_tick"].as_u64())
            .is_some_and(|live| live > tick + 3)
    })?;
    writeln!(
        run2.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"stop"}}"#
    )?;
    let final_line = read_status_line(&mut stdout)?;
    assert_eq!(final_line["ok"], true, "stop 应答: {final_line}");
    let status = run2.wait()?;
    anyhow::ensure!(status.code() == Some(0), "JSON 续存应退出 0：{status:?}");

    let second = restore_in_process(&fixture.state())?;
    anyhow::ensure!(
        loops_of(&second.engine, "p1")? > loops1,
        "JSON 通道续存后玩家应继续真实执行（Memory.loops 增长）"
    );
    Ok(())
}

/// 恢复下限：最新检查点撕裂（截断）时，恢复选择跳过它回退到次新完整
/// 版——不得把未确认耐久的最新 tick 当有效水位。
#[test]
#[cfg(unix)]
fn torn_latest_checkpoint_falls_back_to_last_durable() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({}))?;
    init_ok(&fixture, &config)?;
    let (mut run1, _) = spawn_run(&config, &fixture, "torn-run.err")?;
    wait_for(|| checkpoint_count(&fixture) >= 3)?;
    unsafe {
        libc::kill(run1.id() as i32, libc::SIGTERM);
    }
    run1.wait()?;

    // 截断最新份（终名文件的前缀截断，详见下方注入说明）。
    let mut seqs: Vec<u64> = std::fs::read_dir(fixture.checkpoints())?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_prefix("cp-")?
                .strip_suffix(".json")?
                .parse()
                .ok()
        })
        .collect();
    seqs.sort_unstable();
    let latest = *seqs.last().expect("至少一份检查点");
    let expected_fallback = seqs[seqs.len() - 2];
    let fallback_file = format!("cp-{expected_fallback:012}.json");
    let latest_path = fixture.checkpoints().join(format!("cp-{latest:012}.json"));
    let bytes = std::fs::read(&latest_path)?;
    anyhow::ensure!(bytes.len() > 64, "检查点应有可截断的内容");
    // 发布协议（tmp 整写+fsync+rename）不会产生终名截断——此形态是
    // 外部损坏/低层撕裂的故障注入，驱动读取侧 Broken 分类跳过路径。
    std::fs::write(&latest_path, &bytes[..bytes.len() / 2])?;

    // 进程内预检：恢复选择必须落在次新完整版（publish_seq 精确断言），
    // 且次新份可完整复原——"不越过已确认效果下限"的读侧证据。
    let mut fallback = restore_in_process(&fixture.state())?;
    anyhow::ensure!(
        fallback.selected.publish_seq == expected_fallback,
        "撕裂最新份后应选择次新完整版 {expected_fallback}，实际 {}",
        fallback.selected.publish_seq
    );
    let fallback_tick = fallback.selected.meta.tick;
    let tick = verify_recovered(&mut fallback, 1, 1)?;
    anyhow::ensure!(tick == fallback_tick);

    // serve run 恢复：同一选择（recovery_selected 文件名）+ 如实计数，
    // live_tick 以次新 tick（同一单位）为基线继续增长。
    let mut run2 = KillOnDrop::spawn(
        serve(&[
            "serve",
            "run",
            "--config",
            &config.to_string_lossy(),
            "--control-stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()),
    )?;
    let mut stdout = BufReader::new(run2.child_mut().stdout.take().expect("stdout 已接管"));
    let mut recovered_status = Value::Null;
    wait_for(|| {
        let sent = writeln!(
            run2.child_mut().stdin.as_mut().expect("stdin"),
            r#"{{"command":"status"}}"#
        )
        .is_ok();
        sent && read_status_line(&mut stdout)
            .ok()
            .and_then(|status| {
                recovered_status = status.clone();
                status["live_tick"].as_u64()
            })
            .is_some_and(|live| live > fallback_tick + 3)
    })?;
    anyhow::ensure!(
        recovered_status["recovery_selected"].as_str() == Some(fallback_file.as_str())
            && recovered_status["skipped_broken"].as_u64() == Some(1),
        "应选择次新完整版并如实报告跳过：{recovered_status}"
    );
    writeln!(
        run2.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"stop"}}"#
    )?;
    let final_line = read_status_line(&mut stdout)?;
    assert_eq!(final_line["ok"], true);
    let status = run2.wait()?;
    anyhow::ensure!(
        status.code() == Some(0),
        "回退恢复后应正常退出 0：{status:?}"
    );
    Ok(())
}

/// 同步失败不发布成功水位：检查点目录只读注入 Persistence 故障后，
/// 目录内容集合不得变化（无新完整份、无新名字），且以 2 退出。
#[test]
#[cfg(unix)]
fn persistence_fault_publishes_no_new_complete_checkpoint() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({"auto_save_ticks": 1, "tick_interval_ms": 30}))?;
    init_ok(&fixture, &config)?;
    let checkpoints = fixture.checkpoints();
    let snapshot_before: Vec<String> = dir_entries(&checkpoints)?;

    // 权限恢复守卫：断言早退路径也复原 0o755，保证 Fixture 能清理目录。
    struct RestorePerms(PathBuf);
    impl Drop for RestorePerms {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let restore = RestorePerms(checkpoints.clone());
    std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o500))?;
    let (mut child, stderr_path) = spawn_run(&config, &fixture, "readonly.err")?;
    wait_for(|| {
        std::fs::read_to_string(&stderr_path)
            .map(|text| text.contains("冻结推进"))
            .unwrap_or(false)
    })?;
    let during: Vec<String> = dir_entries(&checkpoints)?;
    anyhow::ensure!(
        during == snapshot_before,
        "故障冻结期间不得出现新检查点名字：{during:?} vs {snapshot_before:?}"
    );
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let status = child.wait()?;
    anyhow::ensure!(
        status.code() == Some(2),
        "持久化故障停止应退出 2：{status:?}"
    );
    let after: Vec<String> = dir_entries(&checkpoints)?;
    anyhow::ensure!(
        after == snapshot_before,
        "故障停止后不得发布新检查点：{after:?} vs {snapshot_before:?}"
    );
    std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o755))?;
    drop(restore);
    Ok(())
}

/// 负向约束：Mod 世界的检查点在 serve 配置缺 mods 声明时，run 恢复被
/// ModIdentity 核验拒绝（exit 1），不得静默降级或部分恢复。
#[test]
#[cfg(unix)]
fn mod_checkpoint_run_without_mods_config_is_refused() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({}))?;
    init_ok(&fixture, &config)?;
    // 第二份配置：同 state_dir，仅去掉 mods 装载输入。
    let no_mods_path = fixture.0.join("serve-no-mods.json");
    let mut no_mods: Value = serde_json::from_str(&std::fs::read_to_string(&config)?)?;
    no_mods
        .as_object_mut()
        .expect("配置应为对象")
        .remove("mods");
    std::fs::write(&no_mods_path, serde_json::to_vec_pretty(&no_mods)?)?;
    let output = serve(&["serve", "run", "--config", &no_mods_path.to_string_lossy()]).output()?;
    anyhow::ensure!(
        output.status.code() == Some(1),
        "缺 mods 配置的 run 应拒绝启动：{:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::ensure!(
        stderr.contains("Mod 包缺失"),
        "错误应指向 ModIdentity 核验（Mod 包缺失）：{stderr}"
    );
    Ok(())
}

pub(super) fn checkpoint_count(fixture: &Fixture) -> usize {
    std::fs::read_dir(fixture.checkpoints())
        .map(|entries| entries.filter_map(|entry| entry.ok()).count())
        .unwrap_or(0)
}

fn dir_entries(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().into_string().unwrap_or_default())
        .collect();
    names.sort();
    Ok(names)
}
