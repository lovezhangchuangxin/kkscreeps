//! 正常拒绝的身份、链控制流、异常优先级与能力寿命。
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

use kk_mod_api::{Decline, Key, SlotOutcome};
use kk_mod_host::{
    ChainContext, ChainHost, ChainTarget, ChainWrite, ModRealm, RegisterOptions, discover_packages,
};
use serde_json::json;

struct Host(Rc<RefCell<usize>>);
impl ChainHost for Host {
    fn read(&mut self, _: &str) -> Result<ChainContext, String> {
        Ok(ChainContext {
            args: json!({}),
            room: "W1N1".into(),
            state: json!({}),
            target: ChainTarget {
                id: "target".into(),
                fields: json!({"value":0}),
            },
            tick: 1,
        })
    }
    fn write(&mut self, _: &str, _: ChainWrite) -> Result<(), String> {
        *self.0.borrow_mut() += 1;
        Ok(())
    }
    fn native(&mut self) -> Result<(), String> {
        panic!("自有动作没有 Rust native")
    }
}

fn realm(body: &str) -> ModRealm {
    realm_with_profile(body, false)
}

fn realm_with_profile(body: &str, profile: bool) -> ModRealm {
    let root = std::env::temp_dir().join(format!(
        "kk-decline-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let dir = root.join("test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mod.json"),
        r#"{"id":"test","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("server.js"),format!(r#"
import {{ decline }} from '@kkscreeps/mod';
export default function register(api) {{
 const target=api.objects.define('test:counter',{{version:1,playerClass:'Counter',visibility:'room-visible',fields:{{value:{{type:'i32',default:0}}}}}});
 api.intents.define('test:increment',{{scope:'object',target,args:{{type:'record',fields:{{}}}},process(ctx){{ {body} }}}});
}}
"#)).unwrap();
    let packages = discover_packages(&root).unwrap();
    let options = RegisterOptions {
        profiler: profile.then(kk_mod_host::ChainProfiler::default),
        ..Default::default()
    };
    let mut realm = ModRealm::with_options(&options).unwrap();
    realm.register(&packages, options).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    realm
}

#[test]
fn explicit_decline_has_binding_code_and_drops_capability_without_poisoning_vm() {
    for profile in [false, true] {
        let mut realm = realm_with_profile("return decline('full');", profile);
        let writes = Rc::new(RefCell::new(0));
        for _ in 0..2 {
            assert_eq!(
                realm
                    .invoke_chain("test:increment.slot", Box::new(Host(writes.clone())))
                    .unwrap(),
                SlotOutcome::Declined(Decline {
                    binding: Key::new("test:increment.process").unwrap(),
                    code: "full".into()
                })
            );
            assert_eq!(Rc::strong_count(&writes), 1);
        }
        assert_eq!(*writes.borrow(), 0);
        assert!(
            realm
                .evaluate_probe(
                    "Deno.core.ops.op_mod_chain_decline('2','test:increment.process','full')"
                )
                .unwrap_err()
                .to_string()
                .contains("已经结束")
        );
        assert!(
            realm
                .invoke("test:increment.process", json!({"state":null}))
                .unwrap_err()
                .to_string()
                .contains("整槽动作入口")
        );
    }
}

#[test]
fn ordinary_values_do_not_become_declines_and_invalid_or_thrown_markers_are_faults() {
    for body in [
        "return {code:'full'};",
        "return {status:'declined',code:'full'};",
        "return -6;",
        "return undefined;",
    ] {
        let mut realm = realm(body);
        let writes = Rc::new(RefCell::new(0));
        assert_eq!(
            realm
                .invoke_chain("test:increment.slot", Box::new(Host(writes.clone())))
                .unwrap(),
            SlotOutcome::Completed
        );
        assert_eq!(*writes.borrow(), 1);
    }
    for body in [
        "throw api.decline('full');",
        "return api.decline('');",
        "return api.decline('x'.repeat(129));",
        "return api.decline(Symbol('bad'));",
        "try{Deno.core.ops.op_mod_chain_decline('1','other:process','full')}catch(e){} return api.decline('full');",
        "try{Deno.core.ops.op_mod_chain_decline('1','test:increment.process','')}catch(e){} return api.decline('full');",
        "return Promise.resolve(api.decline('full'));",
    ] {
        let mut realm = realm(body);
        let writes = Rc::new(RefCell::new(0));
        assert!(
            realm
                .invoke_chain("test:increment.slot", Box::new(Host(writes.clone())))
                .is_err(),
            "{body}"
        );
        assert_eq!(*writes.borrow(), 0, "{body}");
        assert_eq!(Rc::strong_count(&writes), 1);
    }
}
