//! deno_core op 定义：沙箱与宿主之间的唯一通道。
//!
//! 命名纪律：一律 `op_kk_` 前缀，绝不与 deno_core 内置 op 撞名
//! （撞名会触发 V8 CHECK 直接 abort，spike 踩过的坑）。
//! prelude.js 在遮蔽 `Deno` 之前把这些 op 的引用捕获进闭包。

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bytes::BytesMut;
use deno_core::{OpState, extension, op2};
use deno_error::JsErrorBox;

use crate::modules::ModuleTable;

/// 一行玩家 console 输出。
#[derive(Debug, Clone)]
pub struct LogLine {
    pub level: String,
    pub text: String,
}

/// 宿主与沙箱共享的状态（经 OpState 注入）。
#[derive(Debug, Default)]
pub struct Shared {
    /// console 输出收集（op 推入，宿主每 tick drain）。
    pub console: Mutex<Vec<LogLine>>,
    /// 本 tick 起始时刻（`Game.cpu.getUsed()` 的墙钟口径，M0）。
    pub tick_start: Mutex<Option<Instant>>,
    /// 近堆回调是否触发过（诊断用）。
    pub near_heap_fired: AtomicBool,
}

pub type SharedRef = Arc<Shared>;

#[op2]
#[string]
fn op_kk_module_kind(state: &mut OpState, #[string] name: &str) -> String {
    match state.borrow::<ModuleTable>().kind(name) {
        Some(crate::modules::ModuleKind::Js) => "js".into(),
        Some(crate::modules::ModuleKind::Binary) => "binary".into(),
        None => "missing".into(),
    }
}

#[op2]
#[string]
fn op_kk_module_load_js(state: &mut OpState, #[string] name: &str) -> Result<String, JsErrorBox> {
    state
        .borrow::<ModuleTable>()
        .load_js(name)
        .map(str::to_owned)
        .ok_or_else(|| JsErrorBox::generic(format!("Cannot find module '{name}'")))
}

#[op2]
#[buffer]
fn op_kk_module_load_binary(
    state: &mut OpState,
    #[string] name: &str,
) -> Result<BytesMut, JsErrorBox> {
    state
        .borrow::<ModuleTable>()
        .load_binary(name)
        .map(BytesMut::from)
        .ok_or_else(|| JsErrorBox::generic(format!("Cannot find module '{name}'")))
}

#[op2(fast)]
fn op_kk_console(state: &mut OpState, #[string] level: &str, #[string] text: &str) {
    let shared = state.borrow::<SharedRef>().clone();
    shared.console.lock().unwrap().push(LogLine {
        level: level.to_string(),
        text: text.to_string(),
    });
}

#[op2(fast)]
fn op_kk_get_used(state: &mut OpState) -> f64 {
    let shared = state.borrow::<SharedRef>().clone();
    match *shared.tick_start.lock().unwrap() {
        Some(t0) => t0.elapsed().as_secs_f64() * 1000.0,
        None => 0.0,
    }
}

extension!(
    kk_runner,
    ops = [
        op_kk_module_kind,
        op_kk_module_load_js,
        op_kk_module_load_binary,
        op_kk_console,
        op_kk_get_used,
    ]
);
