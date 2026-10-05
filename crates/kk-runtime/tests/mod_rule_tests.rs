//! 有效规则读取与计划身份、realm 重建、执行停用的一致性：
//! 故障重试与停用后读取继续反映原冻结选择，不回退注册期默认值。
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_engine::TickStage;
use kk_mod_host::{RegisterOptions, discover_packages};
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

/// gate 声明规则并每轮读取；reads 暴露模块全局，重建后应归零。
const GATE: &str = r#"
let reads = 0;
export default function register(api) {
  api.rules.define('gate:threshold', {
    type: 'i32', min: 1, max: 100, default: 3, consumers: ['gate:handler.read'],
  });
  api.handlers.wrap('builtin:link.tick', { id: 'read' }, (ctx, next) => {
    next();
    reads += 1;
    ctx.state = { threshold: api.rules.effective('gate:threshold'), reads };
  });
}
"#;
const TUNER: &str = r#"
export default function register(api) {
  api.rules.override('gate:threshold', 5);
}
"#;

/// watcher 不定义也不覆盖规则，只读取他人规则：用于验证停用定义方后
/// 规则目录仍然完整（读取继续返回原冻结值）。
const WATCHER: &str = r#"
let watches = 0;
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'watch' }, ctx => {
    watches += 1;
    ctx.state = { threshold: api.rules.effective('gate:threshold'), watches };
  });
}
"#;

fn engine(binary: bool) -> anyhow::Result<(EmbedEngine, PackageRoot, String)> {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-rules-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    for (name, source) in [("gate", GATE), ("tuner", TUNER), ("watcher", WATCHER)] {
        let dir = root.0.join(name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("mod.json"),
            json!({"id":name,"version":"1.0.0","apiVersion":"^1.0.0"}).to_string(),
        )?;
        std::fs::write(dir.join("server.js"), source)?;
    }
    let packages = discover_packages(&root.0)?;
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard_with_mods(&shard(), &packages, RegisterOptions::default())?;
    let id = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[20,20],"structureType":"link","owner":"p1","store":{"energy":0},"cooldown":0}),
    )?;
    Ok((e, root, id))
}

#[test]
fn rule_reads_survive_settlement_faults_rebuild_and_disable() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, _root, _link) = engine(binary)?;
        e.tick(1)?;
        assert_eq!(
            e.engine_mod_state("gate"),
            json!({"threshold":5,"reads":1}),
            "读到的是 tuner 覆盖后的冻结值，不是声明默认 3"
        );
        // 整轮结算失败销毁 realm；重建从同一包快照重装冻结规则。
        e.fail_next_settle_stage_for_test(TickStage::Validate);
        assert!(e.tick(1).is_err());
        assert!(e.needs_mod_recovery());
        e.tick(1)?;
        assert_eq!(
            e.engine_mod_state("gate"),
            json!({"threshold":5,"reads":1}),
            "重建丢弃模块全局，有效值仍为原冻结选择"
        );
        e.tick(1)?;
        assert_eq!(e.engine_mod_state("gate"), json!({"threshold":5,"reads":2}));

        // 停用覆盖方：执行链移除其内容，但不重算冻结规则；读取继续返回 5。
        let added = e.disable_mod("tuner")?;
        assert_eq!(added, BTreeSet::from(["tuner".to_string()]));
        e.tick(1)?;
        assert_eq!(
            e.engine_mod_state("gate"),
            json!({"threshold":5,"reads":1}),
            "停用重建后读取仍反映原冻结选择"
        );

        // 停用定义方：wrap 从链上移除，state 停留在最后提交值；规则目录
        // 保留——未停用的 watcher 仍能读到同一冻结值。
        e.disable_mod("gate")?;
        e.tick(1)?;
        assert_eq!(e.engine_mod_state("gate"), json!({"threshold":5,"reads":1}));
        assert_eq!(
            e.engine_mod_state("watcher"),
            json!({"threshold":5,"watches":1}),
            "定义方停用不卸载规则目录，第三方读取继续返回原冻结值（停用重建归零模块全局）"
        );
    }
    Ok(())
}
