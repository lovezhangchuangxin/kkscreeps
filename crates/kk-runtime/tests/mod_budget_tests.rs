//! 资源故障与分片候选/恢复接线：玩家只执行一次，VM 超限不提前发布。
use kk_mod_host::{ModVmLimits, RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn engine(source: &str, binary: bool, limits: ModVmLimits) -> anyhow::Result<EmbedEngine> {
    let root = Root(std::env::temp_dir().join(format!(
        "kk-runtime-mod-budget-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )));
    let dir = root.0.join("budget");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("mod.json"),
        r#"{"id":"budget","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )?;
    std::fs::write(dir.join("server.js"), source)?;
    let packages = discover_packages(&root.0)?;
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard_with_mods(
        &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6}]}),
        &packages,
        RegisterOptions {
            vm_limits: limits,
            ..Default::default()
        },
    )?;
    Ok(e)
}

fn link(e: &mut EmbedEngine, x: u8, energy: u32) -> anyhow::Result<String> {
    e.place_object("W1N1","structure",&json!({"pos":[x,20],"structureType":"link","owner":"p1","store":{"energy":energy},"cooldown":0}))
}

#[test]
fn cpu_timeout_keeps_player_candidates_and_reuses_original_limits_on_retry() -> anyhow::Result<()> {
    let source = r#"export default function register(api){
      api.handlers.wrap('builtin:link.transfer',{id:'guard'},(ctx,next)=>{
        next();ctx.state={amount:ctx.args.amount};
        if(ctx.args.amount===1){while(true){}}
      });
    }"#;
    for binary in [false, true] {
        let mut limits = ModVmLimits::default();
        limits.settlement.cpu = Duration::from_millis(10);
        limits.settlement.wall = Duration::from_secs(2);
        let mut e = engine(source, binary, limits)?;
        let a = link(&mut e, 20, 100)?;
        let b = link(&mut e, 21, 0)?;
        let generation = e.engine_generation();
        let player = e.player_state_for_test("p1").unwrap().clone();
        let code = |amount| {
            format!(
                "globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;Game.getObjectById('{a}').transferEnergy(Game.getObjectById('{b}'),{amount});globalThis.n"
            )
        };
        for amount in [1, 2] {
            assert!(
                e.run_player("p1", &code(amount))
                    .unwrap_err()
                    .to_string()
                    .contains("CPU 预算")
            );
            assert!(e.has_pending_round());
            assert!(e.needs_mod_recovery());
            assert_eq!(e.get_game_time(), 0);
            assert_eq!(e.engine_generation(), generation);
            assert_eq!(e.player_state_for_test("p1"), Some(&player));
            assert_eq!(e.engine_mod_state("budget"), Value::Null);
            assert_eq!(e.get_object(&a).unwrap()["store"]["energy"], 100);
        }
        e.discard_pending_round();
        let run = e.run_player("p1", &code(2))?;
        assert_eq!(run.expr["value"], 2, "失败重试未运行新玩家代码");
        assert_eq!(e.get_game_time(), 1);
        assert_eq!(e.get_object(&a).unwrap()["store"]["energy"], 98);
        assert_eq!(e.engine_mod_state("budget"), json!({"amount":2}));
    }
    Ok(())
}

#[test]
fn external_memory_limit_discards_native_and_mod_writes() -> anyhow::Result<()> {
    let source = r#"export default function register(api){
      api.handlers.wrap('builtin:link.tick',{id:'alloc'},(ctx,next)=>{
        next();ctx.target.fields.cooldown=9;ctx.state={dirty:true};globalThis.a=new ArrayBuffer(48*1024*1024);
      });
    }"#;
    for binary in [false, true] {
        let limits = ModVmLimits {
            memory_bytes: 16 * 1024 * 1024,
            ..Default::default()
        };
        let mut e = engine(source, binary, limits)?;
        let id = link(&mut e, 20, 0)?;
        let before = e.get_object(&id);
        let generation = e.engine_generation();
        for _ in 0..2 {
            assert!(e.tick(1).unwrap_err().to_string().contains("内存预算"));
            assert!(e.needs_mod_recovery());
            assert_eq!(e.get_game_time(), 0);
            assert_eq!(e.engine_generation(), generation);
            assert_eq!(e.get_object(&id), before);
            assert_eq!(e.engine_mod_state("budget"), Value::Null);
        }
    }
    Ok(())
}

#[test]
fn many_short_object_callbacks_share_one_tick_budget() -> anyhow::Result<()> {
    let source = r#"export default function register(api){
      api.handlers.after('builtin:link.tick',{id:'count'},ctx=>{
        let n=0;for(let i=0;i<4000;i++)n+=Math.sqrt(i);globalThis.n=n;
        ctx.target.fields.cooldown=9;ctx.state={count:(ctx.state?.count||0)+1};
      });
    }"#;
    let mut limits = ModVmLimits::default();
    limits.settlement.cpu = Duration::from_millis(15);
    limits.settlement.wall = Duration::from_secs(2);
    let mut e = engine(source, false, limits)?;
    let mut ids = Vec::new();
    for i in 0..400 {
        ids.push(e.place_object("W1N1","structure",&json!({"pos":[i%50,i/50],"structureType":"link","owner":"p1","store":{"energy":0},"cooldown":0}))?);
    }
    let generation = e.engine_generation();
    assert!(e.tick(1).unwrap_err().to_string().contains("CPU 预算"));
    assert!(e.needs_mod_recovery());
    assert_eq!(e.get_game_time(), 0);
    assert_eq!(e.engine_generation(), generation);
    assert_eq!(e.engine_mod_state("budget"), Value::Null);
    for id in ids {
        assert_eq!(e.get_object(&id).unwrap()["cooldown"], 0);
    }
    Ok(())
}
