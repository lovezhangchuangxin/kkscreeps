//! 内容选择在引擎装配、玩家面与恢复路径的实际效果：
//! 未选包没有类型、玩家 API、行为与初始数据；选择随分片私有快照恢复。
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_mod_host::{ContentSelection, RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
struct PackageRoot(PathBuf);
impl Drop for PackageRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn shard() -> Value {
    json!({"players":["p1"],"rooms":[{"name":"W1N1","rcl":6,"owner":"p1"}]})
}

/// alpha：规则 alpha:step（默认 1）驱动自有对象增值，带玩家面。
const ALPHA_SERVER: &str = r#"
export default function register(api) {
  api.rules.define('alpha:step', {
    type: 'i32', min: 1, max: 9, default: 1, consumers: ['alpha:node.tick.process'],
  });
  api.objects.define('alpha:node', {
    version: 1, playerClass: 'AlphaNode', visibility: 'room-visible', positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'u32', default: 0 },
    },
    tick(ctx) { ctx.target.fields.value += api.rules.effective('alpha:step'); },
  });
}
"#;

const ALPHA_PLAYER: &str = r#"
export default function install(api) {
  class AlphaNode extends api.builtins.RoomObject {
    get value() { return api.view.field(this, 'value'); }
  }
  api.objects.bind('alpha:node', AlphaNode);
  api.globals.define('AlphaNode', AlphaNode);
}
"#;

/// beta：manifest 依赖 alpha，覆盖其规则，另有自己的类型与玩家面。
const BETA_SERVER: &str = r#"
export default function register(api) {
  api.rules.override('alpha:step', 3);
  api.objects.define('beta:beacon', {
    version: 1, playerClass: 'BetaBeacon', visibility: 'room-visible', positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 },
      marks: { type: 'u32', default: 0 },
    },
    tick(ctx) { ctx.target.fields.marks++; },
  });
}
"#;

const BETA_PLAYER: &str = r#"
export default function install(api) {
  class BetaBeacon extends api.builtins.RoomObject {
    get marks() { return api.view.field(this, 'marks'); }
  }
  api.objects.bind('beta:beacon', BetaBeacon);
  api.globals.define('BetaBeacon', BetaBeacon);
}
"#;

/// gamma：独立包，在 link tick 上挂 before 处理器写入自有 state。
const GAMMA_SERVER: &str = r#"
export default function register(api) {
  api.handlers.before('builtin:link.tick', { id: 'mark' }, ctx => {
    ctx.state = { marked: true };
  });
}
"#;

fn write_pkg(root: &std::path::Path, id: &str, manifest: &str, files: &[(&str, &str)]) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mod.json"), manifest).unwrap();
    for (path, body) in files {
        std::fs::write(dir.join(path), body).unwrap();
    }
}

/// 全部三个包都在包根目录内；选择只经 options 表达，与真实装载一致。
fn content_root() -> PackageRoot {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-selection-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&root.0).unwrap();
    write_pkg(
        &root.0,
        "alpha",
        r#"{"id":"alpha","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
        &[("server.js", ALPHA_SERVER), ("player.js", ALPHA_PLAYER)],
    );
    write_pkg(
        &root.0,
        "beta",
        r#"{"id":"beta","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"alpha":"^1.0.0"},"player":"player.js"}"#,
        &[("server.js", BETA_SERVER), ("player.js", BETA_PLAYER)],
    );
    write_pkg(
        &root.0,
        "gamma",
        r#"{"id":"gamma","version":"1.0.0","apiVersion":"^1.0.0"}"#,
        &[("server.js", GAMMA_SERVER)],
    );
    root
}

fn engine_with(
    root: &std::path::Path,
    binary: bool,
    include: &[&str],
) -> anyhow::Result<EmbedEngine> {
    let packages = discover_packages(root)?;
    let options = RegisterOptions {
        selection: ContentSelection {
            include: include.iter().map(|id| id.to_string()).collect(),
            exclude: BTreeSet::new(),
        },
        ..Default::default()
    };
    let mut engine = EmbedEngine::new();
    engine.set_kkpv_channel(binary);
    engine.set_sandbox_config_for_test(
        "p1",
        kk_runner::SandboxConfig {
            initial_bucket: 100.0,
            ..kk_runner::SandboxConfig::default()
        },
    );
    engine.create_shard_with_mods(&shard(), &packages, options)?;
    Ok(engine)
}

fn place_node(engine: &mut EmbedEngine, room: &str, type_key: &str, x: u64, y: u64) -> String {
    engine
        .place_mod_object(room, type_key, &json!({"x": x, "y": y}))
        .expect("自有对象可放置")
}

#[test]
fn unselected_content_is_absent_across_settlement_and_recovery() -> anyhow::Result<()> {
    for binary in [false, true] {
        let root = content_root();
        let mut engine = engine_with(&root.0, binary, &["alpha"])?;
        let selection = engine.mod_composition().expect("已注册").selection.clone();
        assert_eq!(selection.requested, vec!["alpha"]);
        assert_eq!(selection.selected, vec!["alpha"]);
        assert_eq!(selection.skipped, vec!["beta", "gamma"]);
        assert!(selection.auto_added.is_empty());
        // 未选包的类型完全不在已装配计划中。
        let error = engine
            .place_mod_object("W1N1", "beta:beacon", &json!({"x":1,"y":1}))
            .expect_err("未选类型不可放置");
        assert!(error.to_string().contains("不在已装配计划中"));
        let node = place_node(&mut engine, "W1N1", "alpha:node", 10, 10);
        let link = engine.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[20,20],"structureType":"link","owner":"p1","store":{"energy":0},"cooldown":100}),
        )?;
        // 玩家面：选中包的类可用，未选包的类与查询完全没有安装。
        engine.run_player(
            "p1",
            "Memory.alphaType = typeof AlphaNode; Memory.betaType = typeof BetaBeacon; 'ok'",
        )?;
        let memory: Value = {
            let raw = &engine.player_state("p1").unwrap().raw_memory;
            serde_json::from_str(raw)?
        };
        assert_eq!(memory["alphaType"], "function");
        assert_eq!(memory["betaType"], "undefined");
        // run_player 先结算一轮（value 0→1），tick(3) 再推进三轮。
        engine.tick(3)?;
        assert_eq!(engine.get_object(&node).unwrap()["fields"]["value"], 4);
        assert_eq!(
            engine.engine_mod_state("gamma"),
            Value::Null,
            "未选包无行为"
        );
        assert_eq!(engine.engine_mod_state("beta"), Value::Null);
        // 故障销毁 realm 后，恢复沿分片私有的同一选择快照重建。
        // Validate 阶段失败发生在 Mod 已运行之后，触发真实销毁/重建。
        engine.fail_next_settle_stage_for_test(kk_engine::TickStage::Validate);
        assert!(engine.tick(1).is_err());
        assert!(engine.needs_mod_recovery());
        engine.tick(2)?;
        assert!(!engine.needs_mod_recovery());
        assert_eq!(engine.get_object(&node).unwrap()["fields"]["value"], 6);
        assert_eq!(engine.engine_mod_state("gamma"), Value::Null);
        let after = engine.mod_composition().expect("恢复后有组合").selection;
        assert_eq!(after.selected, vec!["alpha"]);
        assert!(
            engine
                .place_mod_object("W1N1", "beta:beacon", &json!({"x":2,"y":2}))
                .is_err()
        );
        let _ = link;
        drop(engine);
        // 对照组：同一包根目录不做选择，gamma 的处理器正常写 state。
        let mut full = engine_with(&root.0, binary, &[])?;
        let full_node = place_node(&mut full, "W1N1", "alpha:node", 10, 10);
        full.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[20,20],"structureType":"link","owner":"p1","store":{"energy":0},"cooldown":100}),
        )?;
        full.run_player("p1", "0")?;
        full.tick(3)?;
        // 不选择时全部包安装：beta 的覆盖（step=3）与 gamma 的处理器
        // 都生效，与选中 alpha 的 4（默认 step=1）形成对照。
        assert_eq!(full.get_object(&full_node).unwrap()["fields"]["value"], 12);
        assert_eq!(full.engine_mod_state("gamma"), json!({"marked":true}));
    }
    Ok(())
}

#[test]
fn selecting_beta_auto_adds_alpha_with_effective_override() -> anyhow::Result<()> {
    for binary in [false, true] {
        let root = content_root();
        let mut engine = engine_with(&root.0, binary, &["beta"])?;
        let selection = engine.mod_composition().expect("已注册").selection.clone();
        assert_eq!(selection.requested, vec!["beta"]);
        assert_eq!(selection.selected, vec!["alpha", "beta"]);
        assert_eq!(selection.auto_added, vec!["alpha"], "自动依赖可解释");
        assert_eq!(selection.skipped, vec!["gamma"]);
        let node = place_node(&mut engine, "W1N1", "alpha:node", 10, 10);
        let beacon = place_node(&mut engine, "W1N1", "beta:beacon", 12, 12);
        engine.run_player("p1", "0")?;
        engine.tick(2)?;
        // auto 加入的 alpha 使用 beta 的覆盖规则：每轮 +3（三轮共 9），
        // 而非默认 +1。
        assert_eq!(engine.get_object(&node).unwrap()["fields"]["value"], 9);
        assert_eq!(engine.get_object(&beacon).unwrap()["fields"]["marks"], 3);
        assert_eq!(engine.engine_mod_state("gamma"), Value::Null);
    }
    Ok(())
}
