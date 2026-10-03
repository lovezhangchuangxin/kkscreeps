//! 首个故障来源不能被 wrap 传播、伪造的消息或诊断格式化覆盖。
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use kk_mod_api::Key;
use kk_mod_host::{
    ChainContext, ChainHost, ChainTarget, ChainWrite, ModFaultKind, ModPackage, ModRealm,
    RegisterOptions,
};
use serde_json::json;

struct Host(Option<ModFaultKind>);

struct ObservedHost {
    reads: Rc<Cell<usize>>,
    writes: Rc<Cell<usize>>,
    natives: Rc<Cell<usize>>,
}
impl ChainHost for ObservedHost {
    fn read(&mut self, binding: &str) -> Result<ChainContext, String> {
        self.reads.set(self.reads.get() + 1);
        Host(None).read(binding)
    }
    fn write(&mut self, _: &str, _: ChainWrite) -> Result<(), String> {
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
    fn native(&mut self) -> Result<(), String> {
        self.natives.set(self.natives.get() + 1);
        Ok(())
    }
}
impl ChainHost for Host {
    fn read(&mut self, binding: &str) -> Result<ChainContext, String> {
        if self.0 == Some(ModFaultKind::Read) && binding.starts_with("beta:") {
            return Err("领域读取失败".into());
        }
        Ok(ChainContext {
            args: json!({}),
            room: "W1N1".into(),
            tick: 0,
            state: json!({}),
            target: ChainTarget {
                id: "target".into(),
                fields: json!({"cooldown":0}),
            },
        })
    }
    fn write(&mut self, binding: &str, _: ChainWrite) -> Result<(), String> {
        if self.0 == Some(ModFaultKind::Write) && binding.starts_with("beta:") {
            Err("领域写入失败".into())
        } else {
            Ok(())
        }
    }
    fn native(&mut self) -> Result<(), String> {
        if self.0 == Some(ModFaultKind::Native) {
            Err("原生失败".into())
        } else {
            Ok(())
        }
    }
}

fn realm(outer: &str, inner: &str, profile: bool) -> ModRealm {
    let packages = [("alpha", outer), ("beta", inner)]
        .into_iter()
        .map(|(id, source)| {
            (
                id.into(),
                ModPackage {
                    manifest: serde_json::from_value(
                        json!({"id":id,"version":"1.0.0","apiVersion":"^1.0.0"}),
                    )
                    .unwrap(),
                    root: std::env::temp_dir().join("kk-fault-fixture").join(id),
                    files: BTreeMap::from([(
                        "server.js".into(),
                        format!("export default function register(api){{{source}}}").into_bytes(),
                    )]),
                },
            )
        })
        .collect();
    let options = RegisterOptions {
        profiler: profile.then(kk_mod_host::ChainProfiler::default),
        ..Default::default()
    };
    let mut realm = ModRealm::with_options(&options).unwrap();
    realm.register(&packages, options).unwrap();
    realm
}

#[test]
fn inner_fault_retains_binding_and_boundary_through_caught_next() {
    let outer = "api.handlers.wrap('builtin:link.tick',{id:'outer'},(ctx,next)=>{try{next()}catch(e){} throw Error('处理器 alpha:handler.outer: fake');});";
    for profile in [false, true] {
        for (body, host_fault, kind) in [
            (
                "throw Error('处理器 alpha:handler.outer: fake')",
                None,
                ModFaultKind::Handler,
            ),
            (
                "throw {get message(){throw Error('formatter')}}",
                None,
                ModFaultKind::Handler,
            ),
            (
                "throw {get message(){try{Deno.core.ops.op_mod_chain_read('1','ghost:key')}catch(e){}return 'inner';}}",
                None,
                ModFaultKind::Handler,
            ),
            (
                "throw {toString(){try{Deno.core.ops.op_mod_chain_write('1','alpha:handler.outer',{state:Symbol('bad')})}catch(e){}return 'inner';}}",
                None,
                ModFaultKind::Handler,
            ),
            ("return Promise.resolve()", None, ModFaultKind::Handler),
            (
                "ctx.state=Symbol('bad')",
                None,
                ModFaultKind::WriteConversion,
            ),
            ("", Some(ModFaultKind::Read), ModFaultKind::Read),
            ("", Some(ModFaultKind::Write), ModFaultKind::Write),
        ] {
            let inner = format!(
                "api.handlers.replace('builtin:link.tick',{{id:'inner'}},ctx=>{{{body}}});"
            );
            let mut realm = realm(outer, &inner, profile);
            let error = realm
                .invoke_chain("builtin:link.tick", Box::new(Host(host_fault)))
                .unwrap_err();
            let fault = error.fault.unwrap();
            assert_eq!(fault.slot.as_deref(), Some("builtin:link.tick"));
            assert_eq!(
                fault.binding,
                Some(Key::new("beta:handler.inner").unwrap()),
                "{body}"
            );
            assert_eq!(fault.kind, kind, "{body}");
        }
    }
}

#[test]
fn native_and_invalid_capability_faults_do_not_blame_a_wrap() {
    let outer =
        "api.handlers.wrap('builtin:link.tick',{id:'outer'},(ctx,next)=>{try{next()}catch(e){} });";
    let mut vm = realm(outer, "", false);
    let error = vm
        .invoke_chain(
            "builtin:link.tick",
            Box::new(Host(Some(ModFaultKind::Native))),
        )
        .unwrap_err();
    let fault = error.fault.unwrap();
    assert_eq!(fault.kind, ModFaultKind::Native);
    assert_eq!(fault.binding, None);
    let inner = "api.handlers.before('builtin:link.tick',{id:'bad'},ctx=>{try{Deno.core.ops.op_mod_chain_read('1','ghost:key')}catch(e){} });";
    let mut vm = realm(outer, inner, false);
    let error = vm
        .invoke_chain("builtin:link.tick", Box::new(Host(None)))
        .unwrap_err();
    let fault = error.fault.unwrap();
    assert_eq!(fault.kind, ModFaultKind::Capability);
    assert_eq!(fault.binding, None);
}

#[test]
fn repeated_next_is_attributed_to_its_wrap_and_normal_calls_have_no_fault() {
    let mut vm = realm(
        "api.handlers.wrap('builtin:link.tick',{id:'outer'},(ctx,next)=>{next();try{next()}catch(e){} });",
        "",
        false,
    );
    let error = vm
        .invoke_chain("builtin:link.tick", Box::new(Host(None)))
        .unwrap_err();
    let fault = error.fault.unwrap();
    assert_eq!(fault.kind, ModFaultKind::Control);
    assert_eq!(
        fault.binding,
        Some(Key::new("alpha:handler.outer").unwrap())
    );
    let mut vm = realm(
        "api.handlers.wrap('builtin:link.tick',{id:'outer'},(ctx,next)=>next());",
        "",
        false,
    );
    vm.invoke_chain("builtin:link.tick", Box::new(Host(None)))
        .unwrap();
    // 直接诊断的普通错误保持原接口，不伪造整槽来源。
    assert!(
        vm.evaluate_probe("throw Error('probe')")
            .unwrap_err()
            .fault
            .is_none()
    );
}

#[test]
fn fault_formatting_cannot_access_candidates_or_run_twice() {
    for profile in [false, true] {
        for method in ["get message()", "toString()"] {
            let inner = format!(
                r#"
api.handlers.replace('builtin:link.tick',{{id:'inner'}},ctx=>{{
  globalThis.formats=0;
  throw {{{method}{{
    globalThis.formats++;
    const ops=Deno.core.ops;
    try{{ops.op_mod_chain_read('1','beta:handler.inner')}}catch(e){{}}
    try{{ops.op_mod_chain_write('1','beta:handler.inner',{{state:{{dirty:true}}}})}}catch(e){{}}
    try{{ops.op_mod_chain_native('1')}}catch(e){{}}
    return 'inner failure';
  }}}};
}});
"#
            );
            let mut vm = realm(
                "api.handlers.wrap('builtin:link.tick',{id:'outer'},(ctx,next)=>next());",
                &inner,
                profile,
            );
            let reads = Rc::new(Cell::new(0));
            let writes = Rc::new(Cell::new(0));
            let natives = Rc::new(Cell::new(0));
            let error = vm
                .invoke_chain(
                    "builtin:link.tick",
                    Box::new(ObservedHost {
                        reads: reads.clone(),
                        writes: writes.clone(),
                        natives: natives.clone(),
                    }),
                )
                .unwrap_err();
            let fault = error.fault.unwrap();
            assert_eq!(fault.kind, ModFaultKind::Handler);
            assert_eq!(fault.binding, Some(Key::new("beta:handler.inner").unwrap()));
            assert!(error.message.contains("inner failure"));
            assert_eq!(vm.evaluate_probe("globalThis.formats").unwrap(), json!(1));
            assert_eq!(reads.get(), 2);
            assert_eq!(writes.get(), 1, "仅进入 next 前的外层写回");
            assert_eq!(natives.get(), 0);
            assert_eq!(Rc::strong_count(&writes), 1);
        }
    }
}
