//! 后台等待 I/O 时，真实玩家世界继续推进；后台只读脱离提交状态的快照。
use kk_mod_host::services::{BackgroundServices, ProbeReader, ServiceOptions};
use kk_mod_host::{RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Root(std::path::PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kk-services-world-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let dir = path.join("audit");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mod.json"),
            json!({"id":"audit","version":"1.0.0","apiVersion":"^1.0.0","services":"services.js"})
                .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("server.js"), "export default api=>{}").unwrap();
        std::fs::write(
            dir.join("services.js"),
            r#"export default api=>{let calls=0;api.services.define('audit:read',{
            async request(ctx){await ctx.probe('audit:wait');return {calls:++calls,view:ctx.view}}
        })}"#,
        )
        .unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Gate {
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: tokio::sync::Semaphore,
}
impl ProbeReader for Gate {
    fn read(&self) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>> {
        Box::pin(async {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered.send(()).map_err(|e| e.to_string())?;
            }
            let permit = self.release.acquire().await.map_err(|e| e.to_string())?;
            permit.forget();
            Ok(Value::Null)
        })
    }
}
#[test]
fn background_wait_does_not_block_real_world_and_restart_discards_transient_js_state()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let root = Root::new();
        let packages = discover_packages(&root.0)?;
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        e.create_shard_with_mods(
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":2}]}),
            &packages,
            RegisterOptions::default(),
        )?;
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"m","owner":"p1","pos":[10,10],"body":["move"],"ticksToLive":100}),
        )?;
        let before = e.world_snapshot();
        let (entered, waiting) = std::sync::mpsc::channel();
        let gate = Arc::new(Gate {
            entered: Mutex::new(Some(entered)),
            release: tokio::sync::Semaphore::new(0),
        });
        let mut options = ServiceOptions::default();
        options.probes.insert("audit:wait".into(), gate.clone());
        options.vm_limits.settlement.wall = Duration::from_secs(2);
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let host =
            executor.block_on(BackgroundServices::spawn(packages.clone(), options.clone()))?;
        executor.block_on(host.start(before.clone())?.resolve())?;
        let pending = host.request("audit:read", Value::Null, before.clone())?;
        waiting.recv_timeout(Duration::from_secs(1))?;
        e.run_player(
            "p1",
            "Memory.calls=(Memory.calls||0)+1;Game.creeps.m.move(RIGHT)",
        )?;
        let after = e.world_snapshot();
        assert_ne!(before, after);
        gate.release.add_permits(1);
        let observed = executor.block_on(pending.resolve())?;
        assert_eq!(observed, json!({"calls":1,"view":before}));
        assert_eq!(e.world_snapshot(), after);
        executor.block_on(host.shutdown(after.clone()))?;
        let restarted = executor.block_on(BackgroundServices::spawn(packages, options))?;
        executor.block_on(restarted.start(after.clone())?.resolve())?;
        gate.release.add_permits(1);
        let observed = executor.block_on(
            restarted
                .request("audit:read", Value::Null, after.clone())?
                .resolve(),
        )?;
        assert_eq!(observed, json!({"calls":1,"view":after}));
        executor.block_on(restarted.shutdown(after.clone()))?;
        assert_eq!(e.world_snapshot(), after);
    }
    Ok(())
}
