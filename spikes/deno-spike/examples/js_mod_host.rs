//! JS-only mod host feasibility probe; no production engine dependencies.
//! cargo run --manifest-path spikes/deno-spike/Cargo.toml --example js_mod_host --release --locked
//! Also run the debug profile: deno_core checks nested op reentrancy there.
//! Prints a JSON report to stdout. All authoritative values remain Rust-owned.
use deno_core::*;
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Clone)]
struct Txn {
    values: Vec<f64>,
    calls: u64,
}
type SharedTxn = Rc<RefCell<Txn>>;

fn txn(state: &OpState) -> SharedTxn {
    state.borrow::<SharedTxn>().clone()
}
fn checked_index(id: i32, len: usize) -> Result<usize, deno_error::JsErrorBox> {
    let id = usize::try_from(id).map_err(|_| deno_error::JsErrorBox::type_error("negative id"))?;
    if id >= len {
        return Err(deno_error::JsErrorBox::type_error("unknown id"));
    }
    Ok(id)
}

#[op2(fast)]
fn op_mod_get(state: &mut OpState, #[smi] id: i32) -> Result<f64, deno_error::JsErrorBox> {
    let shared = txn(state);
    let mut tx = shared.borrow_mut();
    let i = checked_index(id, tx.values.len())?;
    tx.calls += 1;
    Ok(tx.values[i])
}
#[op2(fast)]
fn op_mod_set(
    state: &mut OpState,
    #[smi] id: i32,
    value: f64,
) -> Result<(), deno_error::JsErrorBox> {
    if !value.is_finite() {
        return Err(deno_error::JsErrorBox::type_error("non-finite field"));
    }
    let shared = txn(state);
    let mut tx = shared.borrow_mut();
    let i = checked_index(id, tx.values.len())?;
    tx.calls += 1;
    tx.values[i] = value;
    Ok(())
}
#[op2]
#[serde]
fn op_mod_read_all(state: &mut OpState) -> Vec<f64> {
    let shared = txn(state);
    let mut tx = shared.borrow_mut();
    tx.calls += 1;
    tx.values.clone()
}
#[op2]
fn op_mod_write_all(
    state: &mut OpState,
    #[serde] values: Vec<f64>,
) -> Result<(), deno_error::JsErrorBox> {
    let shared = txn(state);
    let mut tx = shared.borrow_mut();
    if values.len() != tx.values.len() || values.iter().any(|v| !v.is_finite()) {
        return Err(deno_error::JsErrorBox::type_error("invalid batch"));
    }
    tx.calls += 1;
    tx.values = values;
    Ok(())
}

// Crucial: reentrant op + no OpState/Txn borrow held while invoking JavaScript.
// This tests JS -> Rust next() -> JS lifecycle -> Rust field access.
#[op2(reentrant)]
fn op_mod_native(
    scope: &mut v8::PinScope,
    state: Rc<RefCell<OpState>>,
    #[smi] id: i32,
    #[scoped] callback: v8::Global<v8::Function>,
) -> Result<f64, deno_error::JsErrorBox> {
    let shared = { txn(&state.borrow()) };
    {
        let mut tx = shared.borrow_mut();
        let i = checked_index(id, tx.values.len())?;
        tx.calls += 1;
        tx.values[i] += 3.0;
    }
    v8::tc_scope!(let tc, scope);
    let recv = v8::undefined(tc).into();
    let result = callback.open(tc).call(tc, recv, &[]);
    let Some(result) = result else {
        let msg = tc
            .exception()
            .map(|e| e.to_rust_string_lossy(tc))
            .unwrap_or_else(|| "callback failed".to_string());
        return Err(deno_error::JsErrorBox::generic(msg));
    };
    result
        .number_value(tc)
        .ok_or_else(|| deno_error::JsErrorBox::type_error("callback must return number"))
}

extension!(
    js_mod_probe,
    ops = [
        op_mod_get,
        op_mod_set,
        op_mod_read_all,
        op_mod_write_all,
        op_mod_native
    ]
);

fn runtime(shared: &SharedTxn) -> anyhow::Result<JsRuntime> {
    let mut rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![js_mod_probe::init()],
        ..Default::default()
    });
    rt.op_state().borrow_mut().put(shared.clone());
    rt.execute_script("mod-fixture", include_str!("js_mod_host.js").to_string())?;
    Ok(rt)
}
fn eval(rt: &mut JsRuntime, code: &str) -> anyhow::Result<serde_json::Value> {
    let value = rt.execute_script("probe", code.to_string())?;
    deno_core::scope!(scope, rt);
    let local = v8::Local::new(scope, value);
    Ok(serde_v8::from_v8(scope, local)?)
}

fn main() -> anyhow::Result<()> {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _entered = executor.enter();
    let shared = Rc::new(RefCell::new(Txn {
        values: vec![0.0; 1000],
        calls: 0,
    }));
    let mut committed = shared.borrow().values.clone();
    let mut rt = runtime(&shared)?;
    rt.execute_script("replace", "runHook('replace', 0);")?;
    assert_eq!(shared.borrow().values[0], 10.0);
    committed.clone_from(&shared.borrow().values);
    rt.execute_script("wrap", "runHook('wrap', 0);")?;
    assert_eq!(shared.borrow().values[0], 20.0); // 10 + before 1 + native 3 + lifecycle 2 + after 4
    committed.clone_from(&shared.borrow().values);
    let failed = rt.execute_script("fail", "runHook('fail', 0);");
    assert!(failed.is_err());
    assert_eq!(shared.borrow().values[0], 999.0); // candidate changed
    assert_eq!(committed[0], 20.0); // authoritative state did not
    shared.borrow_mut().values.clone_from(&committed);
    assert_eq!(eval(&mut rt, "volatileAttempts")?, serde_json::json!(1));
    // Discarding Rust writes cannot undo heap globals. Rebuild for clean retry.
    drop(rt);
    let mut rt = runtime(&shared)?;
    assert_eq!(
        eval(&mut rt, "typeof volatileAttempts")?,
        serde_json::json!("undefined")
    );
    let nested_error = rt.execute_script(
        "nested-error",
        "Deno.core.ops.op_mod_native(0, () => { throw new Error('lifecycle failed'); });",
    );
    assert!(nested_error.is_err());
    assert_eq!(shared.borrow().values[0], 23.0);
    assert_eq!(committed[0], 20.0);
    shared.borrow_mut().values.clone_from(&committed);
    let rejected = rt.execute_script("invalid", "Deno.core.ops.op_mod_set(0, Infinity);");
    assert!(rejected.is_err());
    assert_eq!(shared.borrow().values[0], 20.0);
    assert!(rt
        .execute_script("late", "register('late', () => {});")
        .is_err());

    // Dense, equivalent N-element increment. Both include crossing/conversion;
    // pre-created JS code, no cross-thread IPC. Warm separately, five samples.
    let mut timings = serde_json::Map::new();
    for n in [1, 100, 1000] {
        shared.borrow_mut().values = vec![0.0; n];
        let iterations = 100u32;
        let mut row = serde_json::Map::new();
        for (name, body, calls_per_iteration) in [
            ("field_ops", format!("fieldIncrement({n})"), 2 * n),
            ("serde_batch", "batchIncrement()".to_string(), 2),
        ] {
            let script = format!("for(let j=0;j<{iterations};j++) {{ {body}; }}");
            for _ in 0..5 {
                rt.execute_script("warmup", script.clone())?;
            }
            let mut samples = Vec::new();
            for _ in 0..5 {
                let before = shared.borrow().values[0];
                shared.borrow_mut().calls = 0;
                let start = Instant::now();
                rt.execute_script("measure", script.clone())?;
                samples.push(start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations));
                assert_eq!(
                    shared.borrow().calls,
                    (calls_per_iteration as u64) * u64::from(iterations)
                );
                assert!(shared
                    .borrow()
                    .values
                    .iter()
                    .all(|v| *v == before + f64::from(iterations)));
            }
            samples.sort_by(f64::total_cmp);
            row.insert(
                name.into(),
                serde_json::json!({"median_us_per_pass": samples[2], "samples_us": samples,
        "ops_per_pass": calls_per_iteration, "elements_updated": n}),
            );
        }
        timings.insert(n.to_string(), row.into());
    }
    // Bounded liveness probe: no native blocking operation in this fixture.
    let handle = rt.v8_isolate().thread_safe_handle();
    let watchdog = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        handle.terminate_execution();
    });
    let infinite = rt.execute_script("infinite", "while(true) {}");
    watchdog.join().unwrap();
    assert!(infinite.is_err());
    drop(rt); // hard termination recovery = fresh runtime
    let mut rt = runtime(&shared)?;
    assert_eq!(eval(&mut rt, "6 * 7")?, serde_json::json!(42));
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
          "deno_core": "0.412.0", "profile": if cfg!(debug_assertions) {"debug"} else {"release"},
          "checks": {"replace": true, "native_next_with_nested_js_ops": true, "nested_callback_error": true,
            "candidate_rollback": true, "heap_requires_reset": true, "invalid_value_rejected": true,
            "registration_freeze": true, "js_loop_terminated": true, "runtime_rebuilt": true},
          "limits": "synthetic f64 vector; not engine integration, schemas, storage, quotas, or crash isolation; wall-time timings",
          "timings": timings
        }))?
    );
    Ok(())
}
