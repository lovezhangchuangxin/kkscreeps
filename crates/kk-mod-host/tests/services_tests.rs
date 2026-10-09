//! 独立后台域：真实包、异步常规文件/受控读、生命周期和资源故障。
use kk_mod_host::services::{BackgroundServices, FileProbe, ProbeReader, ServiceOptions};
use kk_mod_host::{ModHostResult, discover_packages};
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
            "kk-services-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn package(&self, id: &str, dependencies: Value, source: &str) {
        let root = self.0.join(id);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("mod.json"),json!({"id":id,"version":"1.0.0","apiVersion":"^1.0.0","services":"services.js","dependencies":dependencies}).to_string()).unwrap();
        std::fs::write(
            root.join("server.js"),
            "export default function register(api) {}",
        )
        .unwrap();
        std::fs::write(root.join("services.js"), source).unwrap();
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

struct Observe {
    events: Arc<Mutex<Vec<String>>>,
    event: String,
}
impl ProbeReader for Observe {
    fn read(&self) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_millis(1)).await;
            self.events.lock().unwrap().push(self.event.clone());
            Ok(json!(self.event))
        })
    }
}

#[test]
fn services_async_file_read_lifecycle_dependency_order_and_snapshot_are_detached()
-> ModHostResult<()> {
    let root = Root::new();
    root.package("a",json!({}),r#"export default api=>{api.services.define('a:echo',{
        async start(ctx){await ctx.probe('a:start');},
        async request(ctx,args){return {text:await ctx.probe('a:text'),view:ctx.view,args,frozen:Object.isFrozen(ctx.view.nested)}},
        async stop(ctx){await ctx.probe('a:stop')}
    })}"#);
    root.package("b",json!({"a":"^1.0.0"}),r#"export default api=>{api.services.define('b:echo',{
        async start(ctx){await ctx.probe('b:start')}, request(ctx,args){return args},async stop(ctx){await ctx.probe('b:stop')}
    })}"#);
    let text = root.0.join("sample.txt");
    std::fs::write(&text, "hello background").unwrap();
    let trace = Arc::new(Mutex::new(vec![]));
    let mut options = ServiceOptions::default();
    for name in ["a:start", "b:start", "a:stop", "b:stop"] {
        options.probes.insert(
            name.into(),
            Arc::new(Observe {
                events: trace.clone(),
                event: name.into(),
            }),
        );
    }
    options
        .probes
        .insert("a:text".into(), Arc::new(FileProbe::open(&text, 1024)?));
    let original = json!({"tick":7,"nested":{"value":2}});
    runtime().block_on(async {
        let host=BackgroundServices::spawn(discover_packages(&root.0)?,options).await?;
        assert_eq!(host.services(),["a:echo","b:echo"]);
        host.start(original.clone())?.resolve().await?;
        let result=host.request("a:echo",json!({"message":"payload"}),original.clone())?.resolve().await?;
        assert_eq!(result,json!({"text":"hello background","view":original,"args":{"message":"payload"},"frozen":true}));
        host.shutdown(original.clone()).await?;
        Ok::<_, kk_mod_host::ModHostError>(())
    })?;
    assert_eq!(
        *trace.lock().unwrap(),
        ["a:start", "b:start", "b:stop", "a:stop"]
    );
    assert_eq!(original["nested"]["value"], 2);
    Ok(())
}

#[test]
fn services_selection_and_sdk_domain_reject_unselected_or_gameplay_capabilities()
-> ModHostResult<()> {
    let root = Root::new();
    root.package(
        "a",
        json!({}),
        "export default api=>{api.services.define('a:echo',{request(ctx,args){return args}})}",
    );
    root.package(
        "b",
        json!({}),
        "throw Error('unselected module must not load'); export default api=>{}",
    );
    let mut options = ServiceOptions::default();
    options.selection.include.insert("a".into());
    runtime().block_on(async {
        let host = BackgroundServices::spawn(discover_packages(&root.0)?, options).await?;
        assert_eq!(host.services(), ["a:echo"]);
        host.start(Value::Null)?.resolve().await?;
        assert!(host.request("b:echo", Value::Null, Value::Null).is_err());
        host.shutdown(Value::Null).await?;
        Ok::<_, kk_mod_host::ModHostError>(())
    })?;
    for source in [
        "import '@kkscreeps/mod'; export default api=>{}",
        "export default async api=>{}",
        "export default api=>{api.services.define('other:echo',{request(){}})}",
        "export default api=>{api.services.define('a:echo',{request(){},unknown(){}})}",
    ] {
        std::fs::write(root.0.join("a/services.js"), source).unwrap();
        let mut options = ServiceOptions::default();
        options.selection.include.insert("a".into());
        assert!(
            runtime()
                .block_on(BackgroundServices::spawn(
                    discover_packages(&root.0)?,
                    options
                ))
                .is_err(),
            "{source}"
        );
    }
    Ok(())
}

struct Wait {
    entered: Arc<AtomicUsize>,
    ms: u64,
}
impl ProbeReader for Wait {
    fn read(&self) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>> {
        Box::pin(async {
            self.entered.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(self.ms)).await;
            Ok(Value::Null)
        })
    }
}
#[test]
fn services_bounded_queue_and_closed_replies_after_resource_fault() -> ModHostResult<()> {
    let root = Root::new();
    root.package("a",json!({}),"export default api=>{api.services.define('a:wait',{async request(ctx){await ctx.probe('a:wait');return 1}})}");
    let entered = Arc::new(AtomicUsize::new(0));
    let mut options = ServiceOptions {
        queue_capacity: 1,
        ..Default::default()
    };
    options.vm_limits.settlement.wall = Duration::from_millis(50);
    options.probes.insert(
        "a:wait".into(),
        Arc::new(Wait {
            entered: entered.clone(),
            ms: 200,
        }),
    );
    runtime().block_on(async {
        let host = BackgroundServices::spawn(discover_packages(&root.0)?, options).await?;
        host.start(Value::Null)?.resolve().await?;
        let first = host.request("a:wait", Value::Null, Value::Null)?;
        while entered.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        let queued = host.request("a:wait", Value::Null, Value::Null)?;
        assert!(
            host.request("a:wait", Value::Null, Value::Null)
                .err()
                .unwrap()
                .to_string()
                .contains("队列已满")
        );
        assert!(first.resolve().await.is_err());
        assert!(queued.resolve().await.is_err());
        assert!(host.request("a:wait", Value::Null, Value::Null).is_err());
        Ok(())
    })
}

#[test]
fn services_probe_capability_fault_cannot_be_hidden_and_unawaited_probe_rejects()
-> ModHostResult<()> {
    for body in [
        "try{await ctx.probe('other:read')}catch(e){} return 1",
        "try{await ctx.probe('a:missing')}catch(e){} return 1",
        "try{await ctx.probe(42)}catch(e){} return 1",
        "ctx.probe('a:wait');return 1",
        "while(true){}",
    ] {
        let root = Root::new();
        root.package("a",json!({}),&format!("export default api=>{{api.services.define('a:bad',{{async request(ctx){{{body}}}}})}}"));
        let mut options = ServiceOptions::default();
        options.vm_limits.settlement.cpu = Duration::from_millis(20);
        options.vm_limits.settlement.wall = Duration::from_millis(200);
        options.probes.insert(
            "a:wait".into(),
            Arc::new(Wait {
                entered: Arc::new(AtomicUsize::new(0)),
                ms: 500,
            }),
        );
        runtime().block_on(async {
            let host = BackgroundServices::spawn(discover_packages(&root.0)?, options).await?;
            host.start(Value::Null)?.resolve().await?;
            assert!(
                host.request("a:bad", Value::Null, Value::Null)?
                    .resolve()
                    .await
                    .is_err(),
                "{body}"
            );
            Ok::<_, kk_mod_host::ModHostError>(())
        })?;
    }
    Ok(())
}

#[test]
fn services_expired_probe_and_call_output_limits_fail_the_domain() -> ModHostResult<()> {
    for (source, limits) in [
        (
            "let saved;export default api=>api.services.define('a:test',{start(ctx){saved=ctx},async request(ctx){try{await saved.probe('a:wait')}catch(e){}return 1}})",
            0,
        ),
        (
            "export default api=>api.services.define('a:test',{async request(ctx){try{await ctx.probe('a:wait');await ctx.probe('a:wait')}catch(e){}return 1}})",
            1,
        ),
        (
            "export default api=>api.services.define('a:test',{request(){return 'x'.repeat(1000)}})",
            2,
        ),
        (
            "export default api=>api.services.define('a:test',{request(){globalThis.large=new Uint8Array(32*1024*1024);return 1}})",
            3,
        ),
        (
            "export default api=>{try{api.services.define('a:test',{request(){},invalid(){}})}catch(e){};api.services.define('a:test',{request(){return 1}})}",
            4,
        ),
    ] {
        let root = Root::new();
        root.package("a", json!({}), source);
        let mut options = ServiceOptions::default();
        options.probes.insert(
            "a:wait".into(),
            Arc::new(Wait {
                entered: Arc::new(AtomicUsize::new(0)),
                ms: 1,
            }),
        );
        if limits == 1 {
            options.max_probes_per_call = 1;
        }
        if limits == 2 {
            options.max_value_bytes = 128;
        }
        if limits == 3 {
            options.vm_limits.memory_bytes = 16 * 1024 * 1024;
        }
        runtime().block_on(async {
            let loaded = BackgroundServices::spawn(discover_packages(&root.0)?, options).await;
            if limits == 4 {
                assert!(loaded.is_err());
                return Ok(());
            }
            let host = loaded?;
            host.start(Value::Null)?.resolve().await?;
            let result = host
                .request("a:test", Value::Null, Value::Null)?
                .resolve()
                .await;
            assert!(result.is_err(), "{limits}");
            if limits == 3 {
                assert!(result.err().unwrap().to_string().contains("内存"));
            }
            Ok::<_, kk_mod_host::ModHostError>(())
        })?;
    }
    Ok(())
}

#[test]
fn services_manifest_paths_snapshot_digest_and_input_limits_are_validated() -> ModHostResult<()> {
    let root = Root::new();
    root.package(
        "a",
        json!({}),
        "export default api=>api.services.define('a:echo',{request(ctx,args){return args}})",
    );
    let packages = discover_packages(&root.0)?;
    let digest = packages["a"].code_digest();
    std::fs::write(
        root.0.join("a/services.js"),
        "export default api=>api.services.define('a:other',{request(ctx,args){return args}})",
    )
    .unwrap();
    assert_ne!(discover_packages(&root.0)?["a"].code_digest(), digest);
    // 装载只用给定快照，后续磁盘改写不会替换实际执行字节。
    runtime().block_on(async {
        let host = BackgroundServices::spawn(
            packages,
            ServiceOptions {
                max_value_bytes: 128,
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(host.services(), ["a:echo"]);
        host.start(Value::Null)?.resolve().await?;
        assert!(
            host.request("a:echo", json!("x".repeat(200)), Value::Null)
                .is_err()
        );
        assert!(
            host.request("a:echo", Value::Null, json!("x".repeat(200)))
                .is_err()
        );
        assert_eq!(
            host.request("a:echo", json!(1), Value::Null)?
                .resolve()
                .await?,
            1
        );
        host.shutdown(Value::Null).await?;
        Ok::<_, kk_mod_host::ModHostError>(())
    })?;
    for path in [
        "../outside.js",
        "server.js",
        "missing.js",
        "/tmp/outside.js",
    ] {
        std::fs::write(
            root.0.join("a/mod.json"),
            json!({"id":"a","version":"1.0.0","apiVersion":"^1.0.0","services":path}).to_string(),
        )
        .unwrap();
        assert!(discover_packages(&root.0).is_err(), "{path}");
    }
    Ok(())
}
