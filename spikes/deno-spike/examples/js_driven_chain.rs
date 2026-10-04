//! 实验：贡献链能否完全由 JS 驱动，从而不需要 reentrant op。
//!
//! 命题：宿主只调用一次 JS（跑整条链），wrap 之间的续行是 JS 闭包，
//! Rust native 中心是链内一次普通 op。此形态下 Rust→JS 的回入点不存在，
//! `op_mod_next` / `NextAction` / `invoke_wrap` 这一整套 reentrant 机制
//! 就没有存在的必要。
//!
//! 与 js_mod_host.rs 的方向差异见同名 .js 文件头注释：那里是 Rust 驱动链
//! （next 载荷为 JS 回调），这里是 JS 驱动链。
//!
//! 全部 op 均为普通 op——文件里没有任何 `reentrant` 标记，这是本实验要
//! 证明的核心：嵌套 wrap 与 native 中心共存时不需要它。

use std::cell::RefCell;
use std::rc::Rc;

use deno_core::OpState;
use deno_core::{JsRuntime, RuntimeOptions, extension, op2};

/// 跨边界共享：JS 经 op 写入轨迹，Rust 直接读取断言。
#[derive(Default)]
struct Trace {
    steps: Vec<String>,
    value: f64,
    native_calls: u32,
}

type SharedTrace = Rc<RefCell<Trace>>;

fn trace_of(state: &OpState) -> SharedTrace {
    state.borrow::<SharedTrace>().clone()
}

#[op2(fast)]
fn op_chain_log(state: &mut OpState, #[string] step: String) {
    trace_of(state).borrow_mut().steps.push(step);
}

/// 模拟 Rust native 中心：改候选并记账。它在 JS 嵌套闭包的最深处被调用。
#[op2(fast)]
fn op_chain_native_center(state: &mut OpState, #[smi] id: i32) {
    assert!(id >= 0, "中心收到非法对象 id");
    let shared = trace_of(state);
    let mut trace = shared.borrow_mut();
    trace.steps.push(format!("native(id={id})"));
    trace.value += 3.0;
    trace.native_calls += 1;
}

extension!(
    js_chain_probe,
    ops = [op_chain_log, op_chain_native_center],
);

const FIXTURE: &str = include_str!("js_driven_chain.js");

const HANDLERS: &str = r#"
register('m:before', function () { trace('before'); }, 'before');
register('m:wrapA', function (ctx, next) { trace('A.enter'); next(); trace('A.exit'); }, 'wrap');
register('m:wrapB', function (ctx, next) { trace('B.enter'); next(); trace('B.exit'); }, 'wrap');
register('m:after', function () { trace('after'); }, 'after');
register('m:jsCenter', function () { trace('js-center'); }, 'replace');
register('m:swallow', function () { trace('swallow'); /* 不调 next */ }, 'wrap');
register('m:twice', function (ctx, next) { trace('twice'); next(); next(); }, 'wrap');
register('m:boom', function (ctx, next) { trace('boom.enter'); next(); throw new Error('内层处理器在内层成功后失败'); }, 'wrap');
"#;

/// 链描述就是宿主交给 JS 的普通数据；此处手写字面量以免引入额外依赖。
/// `"center": ["native"]` 表示 Rust native 中心，`["js", key]` 表示 JS 中心。
fn chain_json(wrap_a: &str, wrap_b: &str, center: &str) -> String {
    format!(
        r#"{{"before":["m:before"],"wraps":["{wrap_a}","{wrap_b}"],"center":{center},"after":["m:after"]}}"#
    )
}

fn new_runtime(shared: &SharedTrace) -> JsRuntime {
    let mut rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![js_chain_probe::init()],
        ..Default::default()
    });
    rt.op_state().borrow_mut().put(shared.clone());
    rt.execute_script("chain-fixture", FIXTURE.to_string())
        .expect("JS 夹具装载失败");
    rt.execute_script("chain-handlers", HANDLERS.to_string())
        .expect("处理器注册失败");
    rt
}

fn steps(shared: &SharedTrace) -> Vec<String> {
    shared.borrow().steps.clone()
}

fn reset(shared: &SharedTrace) {
    let mut trace = shared.borrow_mut();
    trace.steps.clear();
    trace.value = 0.0;
    trace.native_calls = 0;
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shared: SharedTrace = Rc::new(RefCell::new(Trace::default()));
    let mut rt = new_runtime(&shared);

    // 1) 两层 JS wrap + Rust native 中心：嵌套续行，中心在最深处置候选。
    let full = chain_json("m:wrapA", "m:wrapB", r#"["native"]"#);
    rt.execute_script("slot", format!("runSlot({full:?}, {{id: 7}});"))?;
    assert_eq!(
        steps(&shared),
        vec!["before", "A.enter", "B.enter", "native(id=7)", "B.exit", "A.exit", "after"]
    );
    assert_eq!(shared.borrow().value, 3.0);
    assert_eq!(shared.borrow().native_calls, 1);
    println!("[1] 两层 wrap + native 中心：顺序与候选写入正确，全程无 reentrant");

    // 2) 中心换成 JS replace 处理器：链驱动权仍在 JS，形状不变。
    reset(&shared);
    let js_center = chain_json("m:wrapA", "m:wrapB", r#"["js","m:jsCenter"]"#);
    rt.execute_script("slot-js-center", format!("runSlot({js_center:?}, {{id: 7}});"))?;
    assert_eq!(
        steps(&shared),
        vec![
            "before",
            "A.enter",
            "B.enter",
            "js-center",
            "B.exit",
            "A.exit",
            "after"
        ]
    );
    assert_eq!(shared.borrow().native_calls, 0);
    println!("[2] JS replace 作中心：无需 native 中心也走通同一条驱动路径");

    // 3) 外层 wrap 吞掉 next：中心与 after 一并不执行（与引擎 after 语义同形）。
    reset(&shared);
    let swallow = chain_json("m:swallow", "m:wrapB", r#"["native"]"#);
    rt.execute_script("slot-swallow", format!("runSlot({swallow:?}, {{id: 7}});"))?;
    assert_eq!(steps(&shared), vec!["before", "swallow"]);
    assert_eq!(shared.borrow().native_calls, 0);
    println!("[3] 吞掉 next：中心与 after 均未执行");

    // 4) 重复 next 响亮失败，错误回到宿主。
    reset(&shared);
    let twice = chain_json("m:twice", "m:wrapB", r#"["native"]"#);
    let err = rt.execute_script("slot-twice", format!("runSlot({twice:?}, {{id: 7}});"));
    assert!(err.is_err(), "重复 next 应当失败");
    println!("[4] 重复 next 失败并回传宿主：{}", err.unwrap_err());

    // 5) 内层处理器抛错：异常穿透整条链回到宿主，中心已产生的候选写入仍在
    //    （与引擎「异常炸候选、整体回滚」的前提一致：回滚由宿主负责）。
    reset(&shared);
    let boom = chain_json("m:wrapA", "m:boom", r#"["native"]"#);
    let err = rt.execute_script("slot-boom", format!("runSlot({boom:?}, {{id: 7}});"));
    assert!(err.is_err(), "内层抛错应当失败");
    assert_eq!(
        steps(&shared),
        vec!["before", "A.enter", "boom.enter", "native(id=7)"]
    );
    println!("[5] 内层抛错穿透到宿主，中心写入仍在候选（回滚归宿主）：{}", err.unwrap_err());

    Ok(())
}
