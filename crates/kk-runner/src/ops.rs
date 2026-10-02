//! 提供模块读取、console 收集、墙钟计时与主动中断的 deno_core op。
//!
//! 使用 op_kk_ 前缀区分项目 op 与 deno_core 内建 op；重复注册名称会触发
//! 运行时检查失败。prelude 在删除 Deno 全局前捕获这些 op 的引用。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use deno_core::{OpState, extension, op2};
use deno_error::JsErrorBox;

use crate::modules::ModuleTable;

/// 每 tick 最多保留的玩家 console 行数；超限提示另外保留一行。
pub const CONSOLE_MAX_LINES: u64 = 1_000;
/// 每 tick 保留文本的 UTF-8 字节上限，不含超限提示和日志级别。
/// op 接收的字符串仍会先传入 Rust；此限制约束累计保留量。
pub const CONSOLE_MAX_BYTES: u64 = 1024 * 1024;

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
    /// 本轮墙钟起点，仅用于看门狗期限；正常计费读取线程 CPU 时钟。
    pub tick_start: Mutex<Option<Instant>>,
    /// 执行线程 CPU 起点；不包含调度等待，getUsed 另加指令费用。
    pub thread_cpu_start: Mutex<Option<Duration>>,
    /// 近堆回调是否触发过（诊断用）。
    pub near_heap_fired: AtomicBool,
    /// 本 tick 是否调用过 Game.cpu.halt；Sandbox 据此选择零计费的中断处理。
    pub halt_requested: AtomicBool,
    /// 本 tick 尝试输出的行数与 UTF-8 字节数，包含被丢弃的输出；begin_tick 重置。
    console_lines: AtomicU64,
    console_bytes: AtomicU64,
    /// 本 tick 是否已输出过超限提示；第一次超限时立即添加，begin_tick 重置。
    console_truncated: AtomicBool,
}

impl Shared {
    /// 保留一行日志；超过行数或文本字节限制时丢弃，并仅提示一次。
    /// 限制 Rust 日志缓冲区的累计保留量，避免玩家高频输出耗尽宿主内存。
    pub fn push_console_line(&self, level: &str, text: &str) {
        let lines = self.console_lines.fetch_add(1, Ordering::Relaxed) + 1;
        let bytes = self
            .console_bytes
            .fetch_add(text.len() as u64, Ordering::Relaxed)
            + text.len() as u64;
        if lines > CONSOLE_MAX_LINES || bytes > CONSOLE_MAX_BYTES {
            if !self.console_truncated.swap(true, Ordering::Relaxed) {
                self.console.lock().unwrap().push(LogLine {
                    level: "warn".into(),
                    text: format!(
                        "[kk] console 输出超过每 tick 上限（{CONSOLE_MAX_LINES} 行 / {} 字节），后续输出已丢弃",
                        CONSOLE_MAX_BYTES
                    ),
                });
            }
            return;
        }
        self.console.lock().unwrap().push(LogLine {
            level: level.to_string(),
            text: text.to_string(),
        });
    }

    /// 重置本 tick 的输出计数和提示标志，不清空已有日志。
    pub fn reset_console_gate(&self) {
        self.console_lines.store(0, Ordering::Relaxed);
        self.console_bytes.store(0, Ordering::Relaxed);
        self.console_truncated.store(false, Ordering::Relaxed);
    }
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
        // 官方消息形态（game.js requireFn：`Unknown module '...'`）
        .ok_or_else(|| JsErrorBox::generic(format!("Unknown module '{name}'")))
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
        .ok_or_else(|| JsErrorBox::generic(format!("Unknown module '{name}'")))
}

#[op2(fast)]
fn op_kk_console(state: &mut OpState, #[string] level: &str, #[string] text: &str) {
    let shared = state.borrow::<SharedRef>().clone();
    shared.push_console_line(level, text);
}

/// 标记玩家主动 halt 并请求 V8 中断；Sandbox 根据此标志选择零计费处理。
#[op2(fast)]
fn op_kk_cpu_halt(state: &mut OpState) {
    let shared = state.borrow::<SharedRef>().clone();
    shared.halt_requested.store(true, Ordering::Release);
    state
        .borrow::<deno_core::v8::IsolateHandle>()
        .terminate_execution();
}

#[op2(fast)]
fn op_kk_get_used(state: &mut OpState) -> Result<f64, deno_error::JsErrorBox> {
    let shared = state.borrow::<SharedRef>().clone();
    let start = *shared.thread_cpu_start.lock().unwrap();
    match start {
        Some(start) => crate::cpu_clock::thread_cpu_time()
            .map(|now| now.saturating_sub(start).as_secs_f64() * 1000.0)
            .map_err(|e| deno_error::JsErrorBox::generic(e.to_string())),
        None => Ok(0.0),
    }
}

extension!(
    kk_runner,
    ops = [
        op_kk_module_kind,
        op_kk_module_load_js,
        op_kk_module_load_binary,
        op_kk_console,
        op_kk_cpu_halt,
        op_kk_get_used,
    ]
);
