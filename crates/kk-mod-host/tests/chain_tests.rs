//! 整槽候选能力的清理、错误留存、冻结链上限与绑定边界。
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_mod_host::{ChainHost, ChainProfiler, ModRealm, RegisterOptions, discover_packages};
use serde_json::{Value, json};

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn realm(body: &str) -> Result<ModRealm, String> {
    realm_with_options(body, RegisterOptions::default())
}

fn realm_with_options(body: &str, options: RegisterOptions) -> Result<ModRealm, String> {
    let root = Root(std::env::temp_dir().join(format!(
        "kk-chain-host-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )));
    let dir = root.0.join("test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mod.json"),
        r#"{"id":"test","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("server.js"),
        format!("export default function register(api) {{ {body} }}"),
    )
    .unwrap();
    let packages = discover_packages(&root.0).unwrap();
    let mut realm = ModRealm::with_options(&options).unwrap();
    realm
        .register(&packages, options)
        .map_err(|e| e.to_string())?;
    Ok(realm)
}

#[test]
fn profile_records_op_errors_without_retaining_candidate_and_resets_window() {
    let profiler = ChainProfiler::default();
    let mut realm = realm_with_options(
        "api.handlers.before('builtin:link.tick',{id:'audit'},ctx=>{});",
        RegisterOptions {
            profiler: Some(profiler.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    profiler.reset();
    let state = Rc::new(RefCell::new(json!({})));
    realm
        .invoke_chain("builtin:link.tick", Box::new(Host(state.clone())))
        .unwrap();
    assert_eq!(Rc::strong_count(&state), 1);
    assert!(
        realm
            .evaluate_probe("Deno.core.ops.op_mod_chain_read('1','test:handler.audit')")
            .is_err()
    );
    let samples = profiler.snapshot();
    assert_eq!(samples["op_read"].calls, 2);
    assert_eq!(samples["op_read"].errors, 1);
    assert_eq!(samples["host_read"].calls, 1);
    assert_eq!(samples["slot"].calls, 1);
    profiler.reset();
    assert!(profiler.snapshot().is_empty());
}

#[test]
fn mismatched_profile_sink_seals_realm_instead_of_silently_losing_metrics() {
    let options = RegisterOptions {
        profiler: Some(ChainProfiler::default()),
        ..Default::default()
    };
    let mut realm = ModRealm::with_options(&options).unwrap();
    let different = RegisterOptions {
        profiler: Some(ChainProfiler::default()),
        ..options.clone()
    };
    let packages = Default::default();
    assert!(
        realm
            .register(&packages, different)
            .unwrap_err()
            .to_string()
            .contains("诊断句柄")
    );
    assert!(realm.register(&packages, options).is_err());
}

struct Host(Rc<RefCell<Value>>);
impl ChainHost for Host {
    fn read(&mut self, _: &str) -> Result<Value, String> {
        Ok(json!({"tick":0,"state":self.0.borrow().clone()}))
    }
    fn write(&mut self, _: &str, context: Value) -> Result<(), String> {
        *self.0.borrow_mut() = context["state"].clone();
        Ok(())
    }
    fn native(&mut self) -> Result<(), String> {
        self.0.borrow_mut()["native"] = json!(1);
        Ok(())
    }
}

#[test]
fn chain_capability_is_dropped_after_success_cancellation_and_failure() {
    let mut realm = realm(
        r#"
api.handlers.wrap('builtin:link.tick',{id:'audit'},(ctx,next)=>{
  if(ctx.state.mode==='call') next();
  if(ctx.state.mode==='fail') {next();throw Error('chain-failure');}
});
"#,
    )
    .unwrap();
    for mode in ["call", "cancel", "fail", "call"] {
        let state = Rc::new(RefCell::new(json!({"mode":mode})));
        let result = realm.invoke_chain("builtin:link.tick", Box::new(Host(state.clone())));
        assert_eq!(result.is_err(), mode == "fail");
        assert_eq!(Rc::strong_count(&state), 1, "候选能力不得残留在 OpState");
        assert_eq!(state.borrow().get("native").is_some(), mode != "cancel");
    }
    let error = realm
        .evaluate_probe("Deno.core.ops.op_mod_chain_read('4','test:handler.audit')")
        .unwrap_err();
    assert!(error.to_string().contains("已经结束"));
}

#[test]
fn caught_unbound_read_or_disallowed_native_call_is_still_a_fault() {
    for body in [
        "try { Deno.core.ops.op_mod_chain_read('1','other:handler.foreign'); } catch(e) {}",
        "try { Deno.core.ops.op_mod_chain_native('1'); } catch(e) {}",
    ] {
        let mut realm = realm(&format!(
            "api.handlers.replace('builtin:link.tick',{{id:'replace'}},ctx=>{{{body}}});
             globalThis.afterCalls=0;
             api.handlers.after('builtin:link.tick',{{id:'after'}},ctx=>{{globalThis.afterCalls++;}});"
        ))
        .unwrap();
        let state = Rc::new(RefCell::new(json!({})));
        let error = realm
            .invoke_chain("builtin:link.tick", Box::new(Host(state.clone())))
            .unwrap_err();
        assert!(!error.to_string().is_empty());
        assert!(state.borrow().get("native").is_none());
        assert_eq!(Rc::strong_count(&state), 1);
        assert_eq!(
            realm.evaluate_probe("globalThis.afterCalls").unwrap(),
            json!(0)
        );
    }
}

#[test]
fn excessive_wrap_depth_is_rejected_during_registration() {
    let body = (0..65)
        .map(|i| {
            format!("api.handlers.wrap('builtin:link.tick',{{id:'w{i:03}'}},(ctx,next)=>next());")
        })
        .collect::<String>();
    assert!(realm(&body).err().unwrap().contains("超过 64"));
}
