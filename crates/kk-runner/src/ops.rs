//! deno_core op 定义：沙箱与宿主之间的唯一通道。
//!
//! 命名纪律：一律 `op_kk_` 前缀，绝不与 deno_core 内置 op 撞名
//! （撞名会触发 V8 CHECK 直接 abort，spike 踩过的坑）。
//! prelude 各段在遮蔽 `Deno` 之前把这些 op 的引用捕获进闭包。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bytes::BytesMut;
use deno_core::{OpState, extension, op2};
use deno_error::JsErrorBox;

use crate::modules::ModuleTable;

/// 每 tick console 行数上限（官方对 console 输出有频率限制；超限丢行并告知）。
pub const CONSOLE_MAX_LINES: u64 = 1_000;
/// 每 tick console 累计字节上限（防跨边界放大：op 每次调用把字符串完整复制进宿主）。
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
    /// 本 tick 起始时刻（`Game.cpu.getUsed()` 的墙钟口径，M0）。
    pub tick_start: Mutex<Option<Instant>>,
    /// 近堆回调是否触发过（诊断用）。
    pub near_heap_fired: AtomicBool,
    /// 本 tick 已收 console 行数 / 累计字节（闸门计数，begin_tick 重置）。
    console_lines: AtomicU64,
    console_bytes: AtomicU64,
    /// 本 tick console 是否已被截断（drain 时出告知行并复位）。
    console_truncated: AtomicBool,
}

impl Shared {
    /// 推入一行 console 输出；超过每 tick 行数/字节闸门时丢弃（第一次超限出告知行）。
    /// 防御：JS 堆闸门与看门狗都管不住"极短 tick 内高频 op 调用把字符串复制进宿主"的
    /// 跨边界放大（实测 300ms 可注入 >1GB），多玩家共进程时会 OOM 宿主。
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

    /// 每 tick 开始重置 console 闸门计数。
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
