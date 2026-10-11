//! serve init：显式初始化服务状态目录。
//!
//! 顺序不可绕过：目录校验 → 骨架 → 租约 → 清理协议残留 → 生成身份值 →
//! 种子场景装配引擎 → 初始检查点耐久发布 → 最后原子写 identity 文件
//! （完成标记）。identity 文件写入前崩溃的目录是半初始化状态，run 拒绝、
//! init 拒绝复用（处置=确认无价值后删除目录重来）。初始化后运行期不再
//! 读取种子场景。
use crate::sim::mods;
use crate::sim::scene::{self, Scene};
use anyhow::Context;
use kk_runner::{ModuleKind, ModuleTable};
use kk_runtime::service::{self, ServiceDir, ServiceIdentity};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) fn init(config_path: &Path, scenario_path: &Path) -> i32 {
    match init_inner(config_path, scenario_path) {
        Ok(report) => {
            println!("{report}");
            0
        }
        Err(error) => {
            eprintln!("serve init 失败: {error:#}");
            1
        }
    }
}

fn init_inner(config_path: &Path, scenario_path: &Path) -> anyhow::Result<String> {
    let (config, state_dir) = kk_runtime::service::config::ServeConfig::load(config_path)?;
    let service = ServiceDir::new(&state_dir);
    service.validate_for_init().context("服务目录不可初始化")?;
    service.ensure_layout()?;
    // 写权先行：后续一切写入（含临时文件清理）都在租约内。
    let _lease =
        kk_storage::WriterLease::acquire(&service.writer_lock()).context("获取服务目录写者租约")?;
    let mut cleanup_failures = 0usize;
    for dir in [service.root(), &service.checkpoints_dir()] {
        let cleanup = kk_storage::publish::remove_stale_temporaries(dir)?;
        cleanup_failures += cleanup.failed.len();
        for failure in cleanup.failed {
            eprintln!("serve init: 残留临时文件清理失败（不阻塞）: {failure}");
        }
    }
    // 清理后复核：validate_for_init 只放行"仅临时残留"，若临时清理失败
    // （清不干净）则目录仍非空，须拒绝而不是在残留上叠写新世界。
    anyhow::ensure!(
        cleanup_failures == 0
            && std::fs::read_dir(service.checkpoints_dir())?
                .next()
                .is_none(),
        "检查点目录残留清理后仍非空：拒绝初始化（处置=人工核查后删除目录重来）"
    );

    // 种子场景装载（与 sim 共用机制；运行期不再读取）。
    let scenario_path = std::fs::canonicalize(scenario_path).context("读取种子场景路径")?;
    let bytes = std::fs::read(&scenario_path).context("读取种子场景")?;
    let scene: Scene = serde_json::from_slice(&bytes).context("解析场景 JSON")?;
    scene::validate(&scene)?;
    let base = scenario_path.parent().unwrap();
    // 堆上限以配置为权威；场景同时声明且不一致时拒绝（防静默忽略）。
    let heap_limit = match (
        scene.player_heap_limit_bytes,
        config.player_heap_limit_bytes,
    ) {
        (Some(scene_bytes), Some(config_bytes)) => {
            anyhow::ensure!(
                scene_bytes == config_bytes,
                "player_heap_limit_bytes 场景 {scene_bytes} 与配置 {config_bytes} 不一致"
            );
            Some(scene_bytes)
        }
        (scene_bytes, config_bytes) => scene_bytes.or(config_bytes),
    };

    let mut engine = kk_runtime::EmbedEngine::with_world_seed(scene.seed);
    engine.set_player_concurrency(config.player_concurrency)?;
    engine.set_kkpv_channel(matches!(config.channel, service::config::Channel::Kkpv));
    if let Some(bytes) = heap_limit {
        engine.set_player_heap_limit(usize::try_from(bytes).context("堆上限溢出")?)?;
    }
    let shard = serde_json::to_value(&scene.shard)?;
    let loaded = scene
        .mods
        .as_ref()
        .map(|input| mods::load(base, input))
        .transpose()?;
    if let Some(loaded) = &loaded {
        engine.create_shard_with_mods(&shard, &loaded.packages, loaded.options.clone())?;
    } else {
        engine.create_shard(&shard)?;
    }
    let input: Vec<_> = scene
        .objects
        .iter()
        .map(|object| (object.room.as_str(), object.kind.as_str(), &object.spec))
        .collect();
    engine.place_objects(&input).context("放置种子场景对象")?;
    let mut entries = BTreeMap::new();
    for bot in &scene.bots {
        let root = std::fs::canonicalize(base.join(&bot.modules))
            .with_context(|| format!("玩家 {} 的模块目录", bot.player))?;
        let table = ModuleTable::from_directory(&root)
            .with_context(|| format!("加载玩家 {} 的模块", bot.player))?;
        anyhow::ensure!(
            table.kind(&bot.entry) == Some(ModuleKind::Js),
            "玩家 {} 缺少 JS 入口模块 {}",
            bot.player,
            bot.entry
        );
        engine.set_player_modules(&bot.player, table)?;
        engine.set_player_memory(&bot.player, &bot.memory)?;
        entries.insert(bot.player.clone(), bot.entry.clone());
    }

    // 身份值在检查点前一次生成（检查点元数据与 identity 文件两处一致）；
    // identity 文件最后写——写入前的崩溃=半初始化，run/init 都拒绝。
    let identity = ServiceIdentity {
        service_format_version: service::SERVICE_FORMAT_VERSION,
        world_id: service::identity::generate_world_id(),
        world_seed: engine.world_seed(),
        scenario_sha256: crate::support::digest(&bytes),
        initialized: true,
    };
    let dir = service.checkpoints_dir();
    let seq = service::checkpoints::next_publish_seq(&dir)?;
    service::checkpoints::publish(&engine, &dir, seq, &identity, &entries)
        .context("发布初始检查点")?;
    service::identity::write(&service.identity_file(), &identity)
        .context("写入服务身份（初始化完成标记）")?;
    let report = json!({
        "command": "serve init",
        "state_dir": service.root(),
        "world_id": identity.world_id,
        "world_seed": identity.world_seed,
        "initial_checkpoint": {
            "publish_seq": seq,
            "tick": engine.get_game_time(),
            "file": dir.join(service::checkpoints::checkpoint_file_name(seq)),
        },
        "players": entries,
        "mods": loaded.as_ref().map(|loaded| &loaded.metadata),
    });
    Ok(serde_json::to_string_pretty(&report)?)
}
