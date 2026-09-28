//! 实验 M~Q：deno_core 0.412 的 V8 inspector + deno_inspector_server 0.33 断点调试可行性实测。
//!
//! 关闭 spike-deno_core.md §11 的 "inspector（调试协议）未测" 项。
//! 实验字母续接跨笔记序列（spike-deno_core.md A~E，spike-deno-game-api.md F~L）：
//! - 实验M：CDP 全链路——InspectorServer 起服 + register_inspector + /json/list 发现 +
//!   辅助线程同步 WS 客户端（tungstenite）程序化打断点/暂停/恢复，全程机器可判定；
//! - 实验N：断点暂停 × 看门狗——(a) 暂停期间 arm 的看门狗必掐（反面）；(b) 暂停前 disarm
//!   （debug 档语义）则暂停任意久可恢复、runtime 可复用（正面）；
//! - 实验O：inspector:true vs false 的玩家可见全局面 diff（遮蔽 Deno/__bootstrap 后）；
//! - 实验P：inspector × 启动快照——快照 runtime 可打断点、断点帧内可求值快照全局；
//! - 实验Q：双 isolate 双线程各注册一个 target（模拟两玩家），A 暂停期间 B 照常执行，
//!   LIFO 销毁（B 先于 A）无崩溃。
//!
//! 镜像生产沙箱路径（kk-runner）：tokio current-thread + enter、CreateParams.heap_limits +
//! near-heap 双闸门、带代际守卫的常驻看门狗、遮蔽 Deno/__bootstrap、收集型 console 垫片。
//! 暂停语义依据（deno_core 0.412 inspector.rs）：flags.on_pause 时 poll_sessions 进入
//! PollState::Parked（thread::park），waker 收到 CDP 消息后 unpark——execute_script 阻塞路径可行。

use deno_core::*;
use deno_inspector_server::{InspectPublishUid, InspectorServer};
use std::collections::BTreeSet;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

const HEAP_LIMIT: usize = 256 << 20;

// ---------------------------------------------------------------- 看门狗（kk-runner/src/watchdog.rs 移植精简版）

/// 常驻看门狗线程 + 代际守卫（语义与 kk-runner 完全一致，注释从简）：
/// arm/disarm 递增代际；只有"当前代际"的 deadline 到达才有权 terminate；
/// notify_all 一律持锁（丢失唤醒会让看门狗睡死）。
struct WatchdogState {
  deadline: Option<Instant>,
  armed_generation: u64,
  fired_generation: u64,
}

struct WatchdogShared {
  generation: AtomicU64,
  state: Mutex<WatchdogState>,
  cond: Condvar,
  shutdown: AtomicBool,
}

pub struct Watchdog {
  shared: Arc<WatchdogShared>,
  thread: Option<JoinHandle<()>>,
}

impl Watchdog {
  pub fn spawn(isolate: deno_core::v8::IsolateHandle) -> Self {
    let shared = Arc::new(WatchdogShared {
      generation: AtomicU64::new(0),
      state: Mutex::new(WatchdogState {
        deadline: None,
        armed_generation: 0,
        fired_generation: 0,
      }),
      cond: Condvar::new(),
      shutdown: AtomicBool::new(false),
    });
    let worker = shared.clone();
    let thread = std::thread::Builder::new()
      .name("spike-watchdog".into())
      .spawn(move || {
        let mut st = worker.state.lock().unwrap();
        loop {
          if worker.shutdown.load(Ordering::Acquire) {
            return;
          }
          match st.deadline {
            None => {
              st = worker.cond.wait(st).unwrap();
            }
            Some(deadline) => {
              let now = Instant::now();
              if now >= deadline {
                let armed = st.armed_generation;
                if armed != st.fired_generation
                  && worker.generation.load(Ordering::Acquire) == armed
                {
                  st.fired_generation = armed;
                  isolate.terminate_execution();
                }
                st = worker.cond.wait(st).unwrap();
                continue;
              }
              let (guard, _timed_out) =
                worker.cond.wait_timeout(st, deadline - now).unwrap();
              st = guard;
            }
          }
        }
      })
      .expect("spawn spike-watchdog");
    Self {
      shared,
      thread: Some(thread),
    }
  }

  pub fn arm(&self, deadline: Instant) -> u64 {
    let mut st = self.shared.state.lock().unwrap();
    let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
    st.deadline = Some(deadline);
    st.armed_generation = generation;
    self.shared.cond.notify_all();
    drop(st);
    generation
  }

  pub fn disarm(&self) {
    let mut st = self.shared.state.lock().unwrap();
    self.shared.generation.fetch_add(1, Ordering::SeqCst);
    st.deadline = None;
    self.shared.cond.notify_all();
    drop(st);
  }

  pub fn shutdown(&mut self) {
    if self.shared.shutdown.swap(true, Ordering::AcqRel) {
      return;
    }
    {
      let _st = self.shared.state.lock().unwrap();
      self.shared.cond.notify_all();
    }
    if let Some(t) = self.thread.take() {
      let _ = t.join();
    }
  }
}

impl Drop for Watchdog {
  fn drop(&mut self) {
    self.shutdown();
  }
}

// ---------------------------------------------------------------- 镜像生产路径的 runtime 包装

/// 遮蔽内部函数（kk-runner prelude/90_host.js 同款）+ 收集型 console 垫片（纯 JS）。
const MASKING_PRELUDE: &str = r#"
delete globalThis.Deno;
delete globalThis.__bootstrap;
globalThis.__lines = [];
globalThis.console = { log(...a) { globalThis.__lines.push(a.map(x => String(x)).join(' ')); } };
"#;

/// 字段顺序即 Drop 顺序：watchdog 先于 runtime 销毁（V8 实例纪律，kk-runner 同款）。
struct DebugRuntime {
  watchdog: Watchdog,
  runtime: JsRuntime,
  /// 镜像生产的双闸门诊断标志（本实验负载远不及上限，预期恒 false）。
  #[allow(dead_code)]
  near_heap_fired: Arc<AtomicBool>,
}

impl DebugRuntime {
  fn new(tokio: &tokio::runtime::Runtime, inspector: bool, startup_snapshot: Option<&'static [u8]>) -> Self {
    // JsRuntime 创建需处于 tokio 上下文（deno_core GC 延迟任务）；enter 守卫仅限本作用域，
    // 泵送的 block_on 必须在未 enter 的上下文调用
    let _enter = tokio.enter();
    let create_params = v8::CreateParams::default().heap_limits(0, HEAP_LIMIT);
    let mut runtime = JsRuntime::new(RuntimeOptions {
      inspector,
      startup_snapshot,
      create_params: Some(create_params),
      ..Default::default()
    });

    let watchdog = Watchdog::spawn(runtime.v8_isolate().thread_safe_handle());
    runtime
      .execute_script("kk-mask", MASKING_PRELUDE.to_string())
      .expect("masking prelude");

    // 内存双闸门之一：near-heap 回调内 terminate 并临时放大上限（恢复模式见 kk-runner，
    // 本 spike 脚本远不及上限，不触发）。
    let near_heap_fired = Arc::new(AtomicBool::new(false));
    let handle = runtime.v8_isolate().thread_safe_handle();
    let fired = near_heap_fired.clone();
    runtime.add_near_heap_limit_callback(move |current, _initial| {
      fired.store(true, Ordering::Relaxed);
      handle.terminate_execution();
      current * 2
    });

    Self {
      watchdog,
      runtime,
      near_heap_fired,
    }
  }

  /// enter 守卫下执行脚本（kk-runner exec_script 同款纪律）。
  fn execute(
    &mut self,
    tokio: &tokio::runtime::Runtime,
    name: &'static str,
    src: &str,
  ) -> Result<v8::Global<v8::Value>, Box<deno_core::error::JsError>> {
    let _enter = tokio.enter();
    self.runtime.execute_script(name, src.to_string())
  }

  /// enter 守卫下取值。
  fn eval(&mut self, tokio: &tokio::runtime::Runtime, code: &str) -> serde_json::Value {
    let _enter = tokio.enter();
    eval_json(&mut self.runtime, code)
  }
}

/// 在 runtime 里执行 JS 并把结果反序列化为 JSON（deno-spike 同款）。
fn eval_json(rt: &mut JsRuntime, code: &str) -> serde_json::Value {
  let g = rt
    .execute_script("<anon>", code.to_string())
    .expect("eval failed");
  deno_core::scope!(scope, rt);
  let local = v8::Local::new(scope, g);
  match serde_v8::from_v8(scope, local) {
    Ok(serde_json::Value::String(s)) => {
      serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))
    }
    Ok(v) => v,
    Err(e) => serde_json::json!(format!("<deserialize failed: {e:?}>")),
  }
}

// ---------------------------------------------------------------- CDP 辅助线程（同步 WS 客户端）

/// 一次 CDP 会话的脚本化计划。
struct CdpPlan {
  /// setBreakpointByUrl 的 urlRegex（匹配 execute_script 的脚本名）。
  url_regex: String,
  /// 断点行（1-based）。None = 连上只 enable 然后旁观（实验Q 的 B 目标）。
  bp_line: Option<u32>,
  /// Debugger.paused 后持有暂停的时长（之后自动 resume）。
  hold: Duration,
  /// 暂停帧上执行的 Debugger.evaluateOnCallFrame 求值与期望值。
  evals: Vec<(String, serde_json::Value)>,
  idle_only: bool,
}

#[derive(Debug, Default, Clone)]
struct CdpReport {
  ready: bool,
  bp_id: Option<String>,
  bp_locations: usize,
  paused_line: Option<u32>,
  paused_col: Option<u32>,
  paused_reason: Option<String>,
  hit_breakpoints: usize,
  /// (表达式, 实际值, 与期望一致)
  eval_results: Vec<(String, serde_json::Value, bool)>,
  resumed: bool,
  /// Debugger.scriptParsed 报告的脚本 URL（诊断断点绑定用）。
  scripts: Vec<String>,
  /// Debugger.breakpointResolved 通知（诊断断点绑定用）。
  bp_resolved: Vec<String>,
  /// 收到的消息总数（响应+通知，诊断通知流是否工作）。
  msgs_total: usize,
  /// 前若干条消息的 method（或 resp#id）样本（诊断用）。
  first_methods: Vec<String>,
  /// WS 层原始帧总数与前几条样本（诊断传输 vs 解析）。
  raw_total: usize,
  first_raw: Vec<String>,
  errors: Vec<String>,
}

#[derive(Debug)]
enum CdpEvent {
  Ready,
  Paused { line: u32 },
}

/// "ws://127.0.0.1:port/path" → SocketAddr（inspector server 固定字面量地址）。
fn ws_url_to_addr(ws_url: &str) -> Option<std::net::SocketAddr> {
  use std::str::FromStr;
  let rest = ws_url.strip_prefix("ws://")?;
  let hostport = rest.split('/').next()?;
  std::net::SocketAddr::from_str(hostport).ok()
}

/// 同步 WS 客户端包装。read_msg：Ok(None) = 1s 超时 tick（调用方查退出条件）；
/// Err(()) = 连接关闭/致命错误。
struct CdpClient {
  ws: WebSocket<TcpStream>,
  next_id: u64,
  errors: Vec<String>,
  deadline: Instant,
  closed: bool,
  /// 原始帧计数与前几条样本（诊断传输层 vs 解析层）。
  raw_total: usize,
  first_raw: Vec<String>,
}

impl CdpClient {
  fn read_msg(&mut self) -> Result<Option<serde_json::Value>, ()> {
    if self.closed {
      return Err(());
    }
    match self.ws.read() {
      Ok(Message::Text(t)) => {
        let s = t.as_str().to_string();
        self.raw_total += 1;
        if self.first_raw.len() < 6 {
          self.first_raw.push(s.chars().take(120).collect());
        }
        Ok(serde_json::from_str(&s).ok())
      }
      Ok(Message::Binary(b)) => {
        // CDP 不用二进制帧；出现即按文本尝试
        let s = String::from_utf8_lossy(&b).to_string();
        Ok(serde_json::from_str(&s).ok())
      }
      Ok(Message::Close(_)) => {
        self.closed = true;
        Err(())
      }
      Ok(_) => Ok(None),
      Err(tungstenite::Error::Io(ref e))
        if e.kind() == std::io::ErrorKind::WouldBlock
          || e.kind() == std::io::ErrorKind::TimedOut =>
      {
        Ok(None)
      }
      Err(e) => {
        self.errors.push(format!("ws read: {e}"));
        self.closed = true;
        Err(())
      }
    }
  }

  /// 发送请求并等待同 id 响应；期间到达的通知压入 inbox 由外层处理。
  fn call(
    &mut self,
    method: &str,
    params: serde_json::Value,
    inbox: &mut Vec<serde_json::Value>,
  ) -> Option<serde_json::Value> {
    let id = self.next_id;
    self.next_id += 1;
    let msg = serde_json::json!({ "id": id, "method": method, "params": params });
    if let Err(e) = self.ws.send(Message::Text(msg.to_string().into())) {
      self.errors.push(format!("send {method}: {e}"));
      return None;
    }
    loop {
      if Instant::now() > self.deadline {
        self.errors.push(format!("deadline exceeded in {method}"));
        return None;
      }
      match self.read_msg() {
        Ok(Some(v)) => {
          if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
            if let Some(err) = v.get("error") {
              self.errors.push(format!("{method} error: {err}"));
            }
            return Some(v.get("result").cloned().unwrap_or(serde_json::json!({})));
          }
          inbox.push(v);
        }
        Ok(None) => {}
        Err(()) => return None,
      }
    }
  }
}

/// 辅助线程主体：连接 ws_url，按 plan 脚本化交互，结束后回传 CdpReport。
fn cdp_thread(
  ws_url: String,
  plan: CdpPlan,
  finish: Arc<AtomicBool>,
  ev_tx: mpsc::Sender<CdpEvent>,
  done_tx: mpsc::Sender<CdpReport>,
) {
  let mut rep = CdpReport::default();
  let deadline = Instant::now() + Duration::from_secs(90);

  // 连接（自管 TcpStream 以便设 read timeout，1s tick 供退出检查）
  let addr = match ws_url_to_addr(&ws_url) {
    Some(a) => a,
    None => {
      rep.errors.push(format!("parse url: {ws_url}"));
      let _ = done_tx.send(rep);
      return;
    }
  };
  let stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
    Ok(s) => s,
    Err(e) => {
      rep.errors.push(format!("connect: {e}"));
      let _ = done_tx.send(rep);
      return;
    }
  };
  let _ = stream.set_nodelay(true);
  let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
  let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
  let mut client = match tungstenite::client(&ws_url, stream) {
    Ok((ws, _resp)) => CdpClient {
      ws,
      next_id: 1,
      errors: Vec::new(),
      deadline,
      closed: false,
      raw_total: 0,
      first_raw: Vec::new(),
    },
    Err(e) => {
      rep.errors.push(format!("ws handshake: {e}"));
      let _ = done_tx.send(rep);
      return;
    }
  };

  let mut inbox: Vec<serde_json::Value> = Vec::new();

  // 初始化：Runtime.enable → Debugger.enable →（可选）setBreakpointByUrl
  if client
    .call("Runtime.enable", serde_json::json!({}), &mut inbox)
    .is_none()
  {
    rep.errors = client.errors.clone();
    let _ = done_tx.send(rep);
    return;
  }
  if client
    .call("Debugger.enable", serde_json::json!({}), &mut inbox)
    .is_none()
  {
    rep.errors = client.errors.clone();
    let _ = done_tx.send(rep);
    return;
  }
  if let Some(line) = plan.bp_line {
    let _ = client.call(
      "Debugger.setBreakpointsActive",
      serde_json::json!({ "active": true }),
      &mut inbox,
    );
    if let Some(result) = client.call(
      "Debugger.setBreakpointByUrl",
      serde_json::json!({
        "lineNumber": line - 1,
        "columnNumber": 0,
        "urlRegex": plan.url_regex,
      }),
      &mut inbox,
    ) {
      rep.bp_id = result
        .get("breakpointId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
      rep.bp_locations = result
        .get("locations")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    }
  }
  rep.ready = true;
  let _ = ev_tx.send(CdpEvent::Ready);

  // 主读循环：先清空 inbox 再读新消息；finish 只在超时 tick 上生效，
  // 保证 resume/disable 之后的 trailing 通知（如 Debugger.resumed）不丢。
  loop {
    if Instant::now() > deadline {
      client.errors.push("deadline exceeded in main loop".into());
      break;
    }
    let got = if let Some(v) = inbox.pop() {
      Some(v)
    } else {
      match client.read_msg() {
        Ok(Some(v)) => Some(v),
        Ok(None) => {
          if finish.load(Ordering::Acquire) {
            break;
          }
          continue;
        }
        Err(()) => {
          if plan.idle_only || finish.load(Ordering::Acquire) {
            break;
          }
          // 连接已关但 finish 未置位：避免零睡眠忙转烧 CPU
          std::thread::sleep(Duration::from_millis(50));
          continue;
        }
      }
    };
    let Some(v) = got else { continue };
    rep.msgs_total += 1;
    if rep.first_methods.len() < 12 {
      let m = v
        .get("method")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
          format!("resp#{}", v.get("id").and_then(|i| i.as_u64()).unwrap_or(0))
        });
      rep.first_methods.push(m);
    }
    let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
    if method == "Debugger.paused" {
      let params = v.get("params").cloned().unwrap_or(serde_json::json!({}));
      let loc = params
        .pointer("/callFrames/0/location")
        .cloned()
        .unwrap_or(serde_json::json!({}));
      rep.paused_line = loc
        .get("lineNumber")
        .and_then(|n| n.as_u64())
        .map(|n| n as u32 + 1);
      rep.paused_col = loc
        .get("columnNumber")
        .and_then(|n| n.as_u64())
        .map(|n| n as u32 + 1);
      rep.paused_reason = params
        .get("reason")
        .and_then(|r| r.as_str())
        .map(|s| s.to_string());
      rep.hit_breakpoints = params
        .get("hitBreakpoints")
        .and_then(|h| h.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
      let _ = ev_tx.send(CdpEvent::Paused {
        line: rep.paused_line.unwrap_or(0),
      });

      // 暂停帧内求值（实验P：快照全局在调试求值里可见）
      if let Some(frame_id) = params
        .pointer("/callFrames/0/callFrameId")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
      {
        for (expr, expected) in &plan.evals {
          if let Some(result) = client.call(
            "Debugger.evaluateOnCallFrame",
            serde_json::json!({
              "callFrameId": frame_id,
              "expression": expr,
              "returnByValue": true,
            }),
            &mut inbox,
          ) {
            let actual = result
              .pointer("/result/value")
              .cloned()
              .unwrap_or(serde_json::Value::Null);
            rep.eval_results
              .push((expr.clone(), actual.clone(), actual == *expected));
          }
        }
      }

      // 持有暂停（模拟人在断点前停留），然后恢复
      std::thread::sleep(plan.hold);
      let _ = client.call("Debugger.resume", serde_json::json!({}), &mut inbox);
    } else if method == "Debugger.resumed" {
      rep.resumed = true;
    } else if method == "Debugger.scriptParsed" {
      // 关键发现：deno_core 的 resource_name_to_url 只把"绝对文件路径"的资源名转成
      // file:// URL；非路径名 → 无 URL → setBreakpointByUrl 的 urlRegex 永远匹配不上。
      rep.scripts.push(
        v.pointer("/params/url")
          .and_then(|u| u.as_str())
          .unwrap_or("")
          .to_string(),
      );
    } else if method == "Debugger.breakpointResolved" {
      if let Some(id) = v.pointer("/params/breakpointId").and_then(|u| u.as_str()) {
        rep.bp_resolved.push(id.to_string());
      }
    }
  }

  // 收尾的 Debugger.disable 是 best-effort：runtime 销毁时服务端会先以
  // close_going_away 关闭 WS，此处的发送失败是预期竞态，不计入 errors。
  let pre_error_count = client.errors.len();
  let _ = client.call("Debugger.disable", serde_json::json!({}), &mut inbox);
  client.errors.truncate(pre_error_count);
  let _ = client.ws.close(None);
  rep.errors.extend(client.errors.iter().cloned());
  rep.raw_total = client.raw_total;
  rep.first_raw = client.first_raw.clone();
  let _ = done_tx.send(rep);
}

// ---------------------------------------------------------------- 事件循环泵送

/// 关键机制：CDP 会话的握手与请求/响应只在 isolate 线程泵事件循环时被处理
/// （deno_core inspector.rs 的 poll_sessions_from_event_loop 挂在 run_event_loop 上）。
/// 暂停期间例外——V8 的 runMessageLoopOnPause 在 poll_sessions 里以 thread::park
/// 独立运转，不依赖事件循环。block_on 必须在未 enter 的上下文调用。
fn pump(tokio: &tokio::runtime::Runtime, rt: &mut JsRuntime, dur: Duration) {
  // 关键踩坑：wait_for_inspector 必须为 false。空闲事件循环 + 挂着
  // wait_for_disconnect 的 WS 会话（deno_inspector_server 建的就是这种）时，
  // true 会让 run_event_loop 走 Deno 的优雅退出分支——inspector.context_destroyed(主上下文)
  // （jsruntime.rs:2641-2647），此后 scriptParsed/断点全部失效。会话握手与消息分发
  // 在事件循环每次 poll 都会执行（poll_sessions_from_event_loop），不依赖该标志。
  let _ = tokio.block_on(async {
    let _ = tokio::time::timeout(
      dur,
      rt.run_event_loop(PollEventLoopOptions {
        wait_for_inspector: false,
      }),
    )
    .await;
  });
}

/// 泵送直到通道里出现 Ready（helper 的 enable/setBreakpoint 往返完成）。
fn wait_ready_with_pump(
  tokio: &tokio::runtime::Runtime,
  rt: &mut JsRuntime,
  ev_rx: &mpsc::Receiver<CdpEvent>,
  overall: Duration,
) -> bool {
  let deadline = Instant::now() + overall;
  while Instant::now() < deadline {
    pump(tokio, rt, Duration::from_millis(50));
    match ev_rx.try_recv() {
      Ok(CdpEvent::Ready) => return true,
      Ok(_) => {}
      Err(mpsc::TryRecvError::Empty) => {}
      Err(mpsc::TryRecvError::Disconnected) => return false,
    }
  }
  false
}

/// 泵送直到 helper 回传报告（其 Debugger.disable 的响应也需要泵送）。
fn collect_report_with_pump(
  tokio: &tokio::runtime::Runtime,
  rt: &mut JsRuntime,
  done_rx: &mpsc::Receiver<CdpReport>,
  overall: Duration,
) -> CdpReport {
  let deadline = Instant::now() + overall;
  loop {
    pump(tokio, rt, Duration::from_millis(50));
    match done_rx.recv_timeout(Duration::from_millis(0)) {
      Ok(r) => return r,
      Err(mpsc::RecvTimeoutError::Disconnected) => return CdpReport::default(),
      Err(mpsc::RecvTimeoutError::Timeout) => {
        if Instant::now() >= deadline {
          return CdpReport::default();
        }
      }
    }
  }
}

// ---------------------------------------------------------------- 通用断点实验驱动（主线程上的 M/N/P）

enum WatchdogAction {
  None,
  /// arm 后不解除（反面：暂停期间看门狗必掐）。
  ArmedLeft(u64),
  /// arm 后立即 disarm（debug 档语义的正面验证）。
  ArmedThenDisarmed(u64),
}

struct BpOutcome {
  exec_ok: bool,
  exec_err: Option<String>,
  elapsed: Duration,
  report: CdpReport,
  trace: Vec<String>,
  result_value: Option<serde_json::Value>,
  reused_ok: bool,
  server_found: bool,
}

fn run_breakpoint(
  tokio: &tokio::runtime::Runtime,
  server: &InspectorServer,
  script_name: &'static str,
  script_src: &str,
  plan: CdpPlan,
  action: WatchdogAction,
  startup_snapshot: Option<&'static [u8]>,
) -> BpOutcome {
  let mut rt = DebugRuntime::new(tokio, true, startup_snapshot);
  let url = server.register_inspector(script_name.to_string(), rt.runtime.inspector(), false);
  let ws_url = url.0.clone();
  // 注册即刻（runtime 存活期间）查询发现端点——销毁后 target 自动注销，查不到是预期
  let server_found = json_list_targets(&server.host)
    .map(|t| t.iter().any(|(u, ws)| u == script_name && ws.starts_with("ws://")))
    .unwrap_or(false);
  println!("   target registered: module_url={script_name} ws={ws_url} /json/list可见={server_found}");

  let (ev_tx, ev_rx) = mpsc::channel();
  let (done_tx, done_rx) = mpsc::channel();
  let finish = Arc::new(AtomicBool::new(false));
  let helper = {
    let ws_url = ws_url.clone();
    let finish = finish.clone();
    std::thread::Builder::new()
      .name("spike-cdp".into())
      .spawn(move || cdp_thread(ws_url, plan, finish, ev_tx, done_tx))
      .expect("spawn cdp helper")
  };

  // 泵送等待 helper 完成握手与断点设置（enable/setBreakpoint 往返需事件循环处理）
  if !wait_ready_with_pump(tokio, &mut rt.runtime, &ev_rx, Duration::from_secs(10)) {
    println!("   (!) helper ready 等待超时（泵送 10s）");
  }

  match action {
    WatchdogAction::None => {}
    WatchdogAction::ArmedLeft(ms) => {
      rt.watchdog.arm(Instant::now() + Duration::from_millis(ms));
    }
    WatchdogAction::ArmedThenDisarmed(ms) => {
      rt.watchdog.arm(Instant::now() + Duration::from_millis(ms));
      rt.watchdog.disarm();
    }
  }

  let t0 = Instant::now();
  let exec = rt.execute(tokio, script_name, script_src);
  let elapsed = t0.elapsed();

  finish.store(true, Ordering::Release);
  let report = collect_report_with_pump(tokio, &mut rt.runtime, &done_rx, Duration::from_secs(15));
  let _ = helper.join();

  rt.watchdog.disarm();
  let (exec_ok, exec_err) = match exec {
    Ok(_) => (true, None),
    Err(e) => (false, Some(format!("{e}"))),
  };

  let mut trace = Vec::new();
  let mut result_value = None;
  let mut reused_ok = false;
  // 无论成败都先清终止标志（中断路径的善后；正常路径为无害双保险）
  rt.runtime
    .v8_isolate()
    .cancel_terminate_execution();
  if exec_ok {
    let t = rt.eval(tokio, "JSON.stringify(globalThis.__trace || [])");
    if let Some(arr) = t.as_array() {
      trace = arr
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    }
    result_value = Some(rt.eval(tokio, "globalThis.__r"));
  } else {
    // 中断后的复用：清完终止标志再跑一段新代码
    reused_ok = rt
      .eval(
        tokio,
        "(function(){ globalThis.__reuse = 6 * 7; return globalThis.__reuse; })()",
      )
      .as_i64()
      .map(|n| n == 42)
      .unwrap_or(false);
  }

  BpOutcome {
    exec_ok,
    exec_err,
    elapsed,
    report,
    trace,
    result_value,
    reused_ok,
    server_found,
  }
}

/// GET http://host/json/list，返回 (url, webSocketDebuggerUrl) 列表；
/// 任何失败返回 None——调用方对"空列表"与"查询失败"必须区分（防空对空断言）。
fn json_list_targets(host: &std::net::SocketAddr) -> Option<Vec<(String, String)>> {
  let url = format!("http://{host}/json/list");
  let body = match ureq::get(&url).call() {
    Ok(resp) => match resp.into_string() {
      Ok(body) => body,
      Err(e) => {
        println!("   (!) /json/list body 读取失败: {e}");
        return None;
      }
    },
    Err(e) => {
      println!("   (!) /json/list 请求失败: {e}");
      return None;
    }
  };
  let v: serde_json::Value = match serde_json::from_str(&body) {
    Ok(v) => v,
    Err(_) => {
      println!(
        "   (!) /json/list 返回非数组: {}",
        body.chars().take(200).collect::<String>()
      );
      return None;
    }
  };
  let arr = v.as_array()?;
  Some(
    arr.iter()
      .filter_map(|t| {
        Some((
          t.get("url")?.as_str()?.to_string(),
          t.get("webSocketDebuggerUrl")?.as_str()?.to_string(),
        ))
      })
      .collect(),
  )
}

// ---------------------------------------------------------------- 实验M：CDP 全链路

fn experiment_m(tokio: &tokio::runtime::Runtime, server: &InspectorServer) -> bool {
  println!("== 实验M CDP 全链路（方法：主线程 execute_script 含断点行脚本；辅助线程同步 WS 客户端 Runtime.enable→Debugger.enable→setBreakpointByUrl；暂停依据 deno_core inspector 的 on_pause park 机制，Debugger.resume 唤醒） ==");
  const SCRIPT: &str = r#"
globalThis.__trace = [];
globalThis.__lines = [];
console.log('m-start');
function work() {
  globalThis.__trace.push('before');
  globalThis.__trace.push('after');
  return 42;
}
globalThis.__r = work();
globalThis.__trace.push('end');
console.log('m-end');
"#;
  let targets = json_list_targets(&server.host).unwrap_or_default();
  let found_before = targets
    .iter()
    .any(|(u, ws)| u == "/tmp/kk-spike-m.js" && ws.starts_with("ws://"));
  println!(
    "   /json/list（注册前）: {} 个 target（预期不含 spike-m）found={found_before}",
    targets.len()
  );

  let plan = CdpPlan {
    url_regex: "kk-spike-m\\.js$".to_string(),
    bp_line: Some(6),
    hold: Duration::from_millis(500),
    evals: vec![],
    idle_only: false,
  };
  let out = run_breakpoint(tokio, server, "/tmp/kk-spike-m.js", SCRIPT, plan, WatchdogAction::None, None);
  println!(
    "   execute: ok={} elapsed={:.0}ms err={:?}",
    out.exec_ok,
    out.elapsed.as_secs_f64() * 1e3,
    out.exec_err
  );
  println!(
    "   paused: line={:?} col={:?} reason={:?} hitBreakpoints={} bpId={:?} bpLocations={}",
    out.report.paused_line,
    out.report.paused_col,
    out.report.paused_reason,
    out.report.hit_breakpoints,
    out.report.bp_id,
    out.report.bp_locations
  );
  println!(
    "   scriptParsed urls={:?} breakpointResolved={:?} msgsTotal={} firstMsgs={:?} rawTotal={} firstRaw={:?}",
    out.report.scripts, out.report.bp_resolved, out.report.msgs_total, out.report.first_methods,
    out.report.raw_total, out.report.first_raw
  );
  println!(
    "   trace={:?} __r={:?} resumed={} helperErrors={:?}",
    out.trace, out.result_value, out.report.resumed, out.report.errors
  );
  let pass = out.exec_ok
    && out.server_found
    && out.report.paused_line == Some(6)
    && out.report.hit_breakpoints >= 1
    && out.trace == vec!["before", "after", "end"]
    && out.result_value == Some(serde_json::json!(42))
    && out.elapsed >= Duration::from_millis(500)
    && out.report.errors.is_empty();
  println!(
    "实验M —— {}（断点在 spike-m 第 6 行命中、恢复后脚本完整跑完）",
    verdict(pass)
  );
  pass
}

// ---------------------------------------------------------------- 实验N：暂停 × 看门狗

fn experiment_n(tokio: &tokio::runtime::Runtime, server: &InspectorServer) -> bool {
  println!("== 实验N 暂停×看门狗（方法：同一断点脚本两轮——(a) 暂停前 arm 500ms 看门狗不解除；(b) arm 后 disarm（debug 档语义）再执行；辅助线程持有暂停 1.5s/2s 后自动 resume） ==");
  const SCRIPT: &str = r#"
globalThis.__trace = [];
function work() {
  globalThis.__trace.push('before');
  globalThis.__trace.push('after');
  return 42;
}
globalThis.__r = work();
globalThis.__trace.push('end');
"#;

  // (a) 反面：暂停期间看门狗到期
  let plan_a = CdpPlan {
    url_regex: "kk-spike-na\\.js$".to_string(),
    bp_line: Some(4),
    hold: Duration::from_millis(1500),
    evals: vec![],
    idle_only: false,
  };
  let out_a = run_breakpoint(
    tokio,
    server,
    "/tmp/kk-spike-na.js",
    SCRIPT,
    plan_a,
    WatchdogAction::ArmedLeft(500),
    None,
  );
  println!(
    "   (a) arm(500ms)不解除: ok={} elapsed={:.0}ms err={:?} paused={:?} reused_ok={}",
    out_a.exec_ok,
    out_a.elapsed.as_secs_f64() * 1e3,
    out_a.exec_err,
    out_a.report.paused_line,
    out_a.reused_ok
  );

  // (b) 正面：arm 后 disarm（debug 档），暂停 2s 后恢复
  let plan_b = CdpPlan {
    url_regex: "kk-spike-nb\\.js$".to_string(),
    bp_line: Some(4),
    hold: Duration::from_millis(2000),
    evals: vec![],
    idle_only: false,
  };
  let out_b = run_breakpoint(
    tokio,
    server,
    "/tmp/kk-spike-nb.js",
    SCRIPT,
    plan_b,
    WatchdogAction::ArmedThenDisarmed(10_000),
    None,
  );
  println!(
    "   (b) arm→disarm后执行: ok={} elapsed={:.0}ms trace={:?} __r={:?} paused={:?} resumed={}",
    out_b.exec_ok,
    out_b.elapsed.as_secs_f64() * 1e3,
    out_b.trace,
    out_b.result_value,
    out_b.report.paused_line,
    out_b.report.resumed
  );

  let a_pass = !out_a.exec_ok
    && out_a
      .exec_err
      .as_deref()
      .map(|e| e.contains("terminated"))
      .unwrap_or(false)
    && out_a.report.paused_line == Some(4)
    && out_a.reused_ok;
  let b_pass = out_b.exec_ok
    && out_b.trace == vec!["before", "after", "end"]
    && out_b.result_value == Some(serde_json::json!(42))
    && out_b.elapsed >= Duration::from_millis(2000)
    && out_b.report.resumed;
  println!(
    "实验N —— {}（(a) 开着看门狗打断点必被掐且 cancel 后可复用；(b) disarm 后暂停 2s 可正常恢复跑完）",
    verdict(a_pass && b_pass)
  );
  a_pass && b_pass
}

// ---------------------------------------------------------------- 实验O：全局面 diff

fn experiment_o(tokio: &tokio::runtime::Runtime) -> bool {
  println!("== 实验O 全局面 diff（方法：inspector:true 与 false 各建一个 runtime，同一段遮蔽垫片（delete Deno/__bootstrap）后取 getOwnPropertyNames(globalThis) 排序对比） ==");
  let names = |inspector: bool| -> BTreeSet<String> {
    let mut rt = DebugRuntime::new(tokio, inspector, None);
    let v = rt.eval(
      tokio,
      "JSON.stringify(Object.getOwnPropertyNames(globalThis).sort())",
    );
    // eval_json 对 JSON 文本会再解析一层：字符串→数组，或直接就是数组
    let list: Vec<String> = if let Some(arr) = v.as_array() {
      arr.iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string()))
        .collect()
    } else if let Some(s) = v.as_str() {
      serde_json::from_str(s).unwrap_or_default()
    } else {
      Vec::new()
    };
    list.into_iter().collect()
  };
  let with = names(true);
  let without = names(false);
  let only_with: Vec<_> = with.difference(&without).cloned().collect();
  let only_without: Vec<_> = without.difference(&with).cloned().collect();
  println!(
    "   inspector:true={} 个全局, false={} 个全局, 仅true={:?}, 仅false={:?}",
    with.len(),
    without.len(),
    only_with,
    only_without
  );
  // 空集守卫：两档同时枚举失败时空对空 diff 也为空，必须显式排除
  let pass = !with.is_empty()
    && with.contains("console")
    && only_with.is_empty()
    && only_without.is_empty();
  println!(
    "实验O —— {}（遮蔽后两档玩家可见全局面一致且非空；若不一致须登记进 sandbox.md 遮蔽小节）",
    verdict(pass)
  );
  pass
}

// ---------------------------------------------------------------- 实验P：inspector × 快照

fn experiment_p(tokio: &tokio::runtime::Runtime, server: &InspectorServer) -> bool {
  println!("== 实验P inspector×快照（方法：build.rs 生成的 startup snapshot（含 __snapFib/SNAPSHOT_PRELOADED）+ inspector:true 建 runtime，断点行命中后在暂停帧 evaluateOnCallFrame 求值快照全局） ==");
  static SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/snapshot-inspector.bin"));
  const SCRIPT: &str = r#"
globalThis.__trace = [];
globalThis.__trace.push('call');
globalThis.__snapFib(10);
globalThis.__trace.push('done');
globalThis.__r = SNAPSHOT_PRELOADED;
"#;
  let plan = CdpPlan {
    url_regex: "kk-spike-p\\.js$".to_string(),
    bp_line: Some(3),
    hold: Duration::from_millis(300),
    evals: vec![
      ("__snapFib(10)".to_string(), serde_json::json!(55)),
      ("SNAPSHOT_PRELOADED".to_string(), serde_json::json!(true)),
    ],
    idle_only: false,
  };
  let out = run_breakpoint(tokio, server, "/tmp/kk-spike-p.js", SCRIPT, plan, WatchdogAction::None, Some(SNAPSHOT));
  println!(
    "   paused={:?} evals={:?} trace={:?} __r={:?}",
    out.report.paused_line, out.report.eval_results, out.trace, out.result_value
  );
  let evals_ok = out
    .report
    .eval_results
    .iter()
    .all(|(_, _, ok)| *ok)
    && out.report.eval_results.len() == 2;
  let pass = out.exec_ok
    && out.report.paused_line == Some(3)
    && evals_ok
    && out.trace == vec!["call", "done"]
    && out.result_value == Some(serde_json::json!(true));
  println!(
    "实验P —— {}（快照 runtime 断点命中且暂停帧内可求值快照全局 __snapFib(10)=55）",
    verdict(pass)
  );
  pass
}

// ---------------------------------------------------------------- 实验Q：双 isolate 双 target

enum QCmd {
  Run(String),
  Shutdown,
}

/// 模拟一个玩家线程：一实例一线程（JsRuntime 非 Send，kk-runner 模型），
/// 注册独立 target，等指令执行脚本，Shutdown 后 Drop（watchdog 先于 runtime）。
/// 空闲等待期间泵事件循环——CDP 会话握手/往返只在 isolate 线程泵送时被处理。
fn player_thread(
  server: Arc<InspectorServer>,
  module_url: &'static str,
  cmd_rx: mpsc::Receiver<QCmd>,
  ready_tx: mpsc::Sender<String>,
  report_tx: mpsc::Sender<String>,
) {
  let tokio_rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .expect("tokio rt");

  let mut rt = DebugRuntime::new(&tokio_rt, true, None);
  let url = server.register_inspector(module_url.to_string(), rt.runtime.inspector(), false);
  let _ = ready_tx.send(url.0.clone());

  loop {
    pump(&tokio_rt, &mut rt.runtime, Duration::from_millis(50));
    match cmd_rx.try_recv() {
      Ok(QCmd::Run(src)) => {
        let r = rt.execute(&tokio_rt, module_url, &src);
        let msg = match r {
          Ok(_) => format!("{module_url}:ok"),
          Err(e) => format!("{module_url}:err:{e}"),
        };
        let _ = report_tx.send(msg);
      }
      Ok(QCmd::Shutdown) => break,
      Err(mpsc::TryRecvError::Empty) => continue,
      Err(mpsc::TryRecvError::Disconnected) => break,
    }
  }
  drop(rt);
  let _ = report_tx.send(format!("{module_url}:__destroyed__"));
}

/// 从 report 通道收一条属于 who 的消息，返回消息全文（超时返回 None）。
fn recv_player_report(rx: &mpsc::Receiver<String>, who: &str) -> Option<String> {
  let deadline = Instant::now() + Duration::from_secs(30);
  while Instant::now() < deadline {
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
      Ok(msg) if msg.starts_with(who) => {
        if msg.contains(":err:") {
          println!("   (!) {who} 执行失败: {msg}");
        }
        return Some(msg);
      }
      Ok(other) => {
        println!("   (!) 非预期顺序报告: {other}");
      }
      Err(_) => return None,
    }
  }
  None
}

fn experiment_q(server_arc: Arc<InspectorServer>) -> bool {
  println!("== 实验Q 双isolate双target（方法：两线程各建 runtime 注册独立 target（先A后B）；A 断点暂停期间 B 两次执行计算脚本；结束 B 先于 A 销毁（LIFO）后 /json/list 应清空） ==");
  let (ready_tx, ready_rx) = mpsc::channel();
  let (report_tx, report_rx) = mpsc::channel();
  let (cmd_a_tx, cmd_a_rx) = mpsc::channel();
  let (cmd_b_tx, cmd_b_rx) = mpsc::channel();

  // 先建 A 再建 B（ready 顺序同步保证创建序 = A 先）
  std::thread::Builder::new()
    .name("spike-player-a".into())
    .spawn({
      let server = server_arc.clone();
      let ready_tx = ready_tx.clone();
      let report_tx = report_tx.clone();
      move || player_thread(server, "/tmp/kk-spike-q-a.js", cmd_a_rx, ready_tx, report_tx)
    })
    .expect("spawn A");
  let ready_a = ready_rx
    .recv_timeout(Duration::from_secs(20))
    .expect("A ready");
  std::thread::Builder::new()
    .name("spike-player-b".into())
    .spawn({
      let server = server_arc.clone();
      move || player_thread(server, "/tmp/kk-spike-q-b.js", cmd_b_rx, ready_tx, report_tx)
    })
    .expect("spawn B");
  let ready_b = ready_rx
    .recv_timeout(Duration::from_secs(20))
    .expect("B ready");

  let targets = json_list_targets(&server_arc.host).unwrap_or_default();
  let has_a = targets.iter().any(|(u, _)| u == "/tmp/kk-spike-q-a.js");
  let has_b = targets.iter().any(|(u, _)| u == "/tmp/kk-spike-q-b.js");
  println!(
    "   /json/list: {} 个 target, 含A={has_a} 含B={has_b}（预期两个都在）",
    targets.len()
  );

  // A 的断点 helper（hold 3s）+ B 的旁观 helper
  let plan_a = CdpPlan {
    url_regex: "kk-spike-q-a\\.js$".to_string(),
    bp_line: Some(4),
    hold: Duration::from_millis(3000),
    evals: vec![],
    idle_only: false,
  };
  let plan_b = CdpPlan {
    url_regex: "kk-spike-q-b\\.js$".to_string(),
    bp_line: None,
    hold: Duration::ZERO,
    evals: vec![],
    idle_only: true,
  };
  let (a_ev_tx, a_ev_rx) = mpsc::channel();
  let (a_done_tx, a_done_rx) = mpsc::channel();
  let finish_a = Arc::new(AtomicBool::new(false));
  {
    let ws = ready_a.clone();
    let f = finish_a.clone();
    std::thread::Builder::new()
      .name("spike-cdp-a".into())
      .spawn(move || cdp_thread(ws, plan_a, f, a_ev_tx, a_done_tx))
      .expect("spawn cdp A");
  }
  let (b_ev_tx, b_ev_rx) = mpsc::channel();
  let (b_done_tx, b_done_rx) = mpsc::channel();
  let finish_b = Arc::new(AtomicBool::new(false));
  {
    let ws = ready_b.clone();
    let f = finish_b.clone();
    std::thread::Builder::new()
      .name("spike-cdp-b".into())
      .spawn(move || cdp_thread(ws, plan_b, f, b_ev_tx, b_done_tx))
      .expect("spawn cdp B");
  }
  match a_ev_rx.recv_timeout(Duration::from_secs(10)) {
    Ok(CdpEvent::Ready) => {}
    o => println!("   (!) A helper ready 异常: {o:?}"),
  }
  match b_ev_rx.recv_timeout(Duration::from_secs(10)) {
    Ok(CdpEvent::Ready) => {}
    o => println!("   (!) B helper ready 异常: {o:?}"),
  }

  const SCRIPT_A: &str = r#"
globalThis.__trace = [];
function work() {
  globalThis.__trace.push('before');
  globalThis.__trace.push('after');
  return 42;
}
globalThis.__r = work();
globalThis.__trace.push('end');
"#;
  const SCRIPT_B: &str = r#"
(() => {
  let s = 0;
  for (let i = 0; i < 20000000; i++) { s += i; }
  globalThis.__r = s;
  globalThis.__trace = ['b-done'];
})();
"#;

  // A 开跑（将停在第 4 行断点）
  let _ = cmd_a_tx.send(QCmd::Run(SCRIPT_A.to_string()));
  // 等 A 真的暂停
  let a_paused = matches!(
    a_ev_rx.recv_timeout(Duration::from_secs(10)),
    Ok(CdpEvent::Paused { line: 4 })
  );

  // A 暂停期间 B 跑第一轮
  let _ = cmd_b_tx.send(QCmd::Run(SCRIPT_B.to_string()));
  let b_run1 = recv_player_report(&report_rx, "/tmp/kk-spike-q-b.js")
    .map(|m| m.ends_with(":ok"))
    .unwrap_or(false);
  // A 恢复并完成
  let a_done = recv_player_report(&report_rx, "/tmp/kk-spike-q-a.js")
    .map(|m| m.ends_with(":ok"))
    .unwrap_or(false);
  // A 已恢复，B 再跑一轮
  let _ = cmd_b_tx.send(QCmd::Run(SCRIPT_B.to_string()));
  let b_run2 = recv_player_report(&report_rx, "/tmp/kk-spike-q-b.js")
    .map(|m| m.ends_with(":ok"))
    .unwrap_or(false);

  println!(
    "   A paused_at_4={a_paused} A done ok={a_done} | B run1 ok={b_run1} run2 ok={b_run2}"
  );

  // LIFO：B 先销毁，A 后销毁
  let _ = cmd_b_tx.send(QCmd::Shutdown);
  let b_destroyed = recv_player_report(&report_rx, "/tmp/kk-spike-q-b.js")
    .map(|m| m.contains("__destroyed__"))
    .unwrap_or(false);
  let _ = cmd_a_tx.send(QCmd::Shutdown);
  let a_destroyed = recv_player_report(&report_rx, "/tmp/kk-spike-q-a.js")
    .map(|m| m.contains("__destroyed__"))
    .unwrap_or(false);
  finish_a.store(true, Ordering::Release);
  finish_b.store(true, Ordering::Release);
  let report_a = a_done_rx
    .recv_timeout(Duration::from_secs(30))
    .unwrap_or_default();
  let report_b = b_done_rx
    .recv_timeout(Duration::from_secs(30))
    .unwrap_or_default();

  // 查询失败（None）不得算作"已注销"——空对空通过是断言漏洞
  let targets_after = json_list_targets(&server_arc.host);
  println!(
    "   LIFO销毁: B先={b_destroyed} A后={a_destroyed} | A paused line={:?} | 销毁后 target 数={}（预期 0）",
    report_a.paused_line,
    targets_after.as_ref().map(|t| t.len()).unwrap_or(usize::MAX)
  );
  println!(
    "   helper errors: A={:?} B={:?}",
    report_a.errors, report_b.errors
  );

  let pass = has_a
    && has_b
    && a_paused
    && a_done
    && b_run1
    && b_run2
    && b_destroyed
    && a_destroyed
    && targets_after.as_ref().map(|t| t.is_empty()).unwrap_or(false)
    && report_a.paused_line == Some(4)
    && report_a.errors.is_empty()
    && report_b.errors.is_empty();
  println!(
    "实验Q —— {}（双 target 独立会话、A 暂停不阻塞 B、LIFO 销毁后 target 注销）",
    verdict(pass)
  );
  pass
}

fn verdict(pass: bool) -> &'static str {
  if pass {
    "PASS"
  } else {
    "FAIL"
  }
}

// ---------------------------------------------------------------- main

fn main() {
  println!(
    "inspector-spike | deno_core={} v8(rusty)={} deno_inspector_server={} tungstenite={} ureq={} serde_json={}",
    env!("SPIKE_VER_DENO_CORE"),
    env!("SPIKE_VER_DENO_V8"),
    env!("SPIKE_VER_DENO_INSPECTOR_SERVER"),
    env!("SPIKE_VER_TUNGSTENITE"),
    env!("SPIKE_VER_UREQ"),
    env!("SPIKE_VER_SERDE_JSON"),
  );
  println!("rustc=1.98.1 platform={}", std::env::consts::OS);

  // 全局兜底死锁保护（macOS 无 timeout 命令，Rust 内部 deadline）
  std::thread::spawn(|| {
    std::thread::sleep(Duration::from_secs(300));
    eprintln!("FATAL: 全局 300s 死锁保护触发，强制退出（exit 2）");
    std::process::exit(2);
  });

  // 主线程 tokio（deno_core GC 延迟任务需要；enter 守卫在 DebugRuntime 方法内按作用域持有，
  // 泵送的 block_on 要求未 enter 的上下文，故这里不挂全局守卫——deno-spike 之后的踩坑）
  let tokio_rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .expect("tokio rt");

  // 一个 InspectorServer 贯穿全部实验（顺带验证注册/注销复用）
  let server = Arc::new(
    InspectorServer::new(
      "127.0.0.1:0".parse().unwrap(),
      "inspector-spike",
      InspectPublishUid::default(),
    )
    .expect("inspector server"),
  );
  println!("inspector server listening: {}", server.host);

  let m = experiment_m(&tokio_rt, &server);
  let n = experiment_n(&tokio_rt, &server);
  let o = experiment_o(&tokio_rt);
  let p = experiment_p(&tokio_rt, &server);
  let q = experiment_q(server.clone());

  println!();
  println!(
    "RESULT inspector-spike: M={} N={} O={} P={} Q={}",
    verdict(m),
    verdict(n),
    verdict(o),
    verdict(p),
    verdict(q)
  );
  println!(
    "CAPABILITY deno_core inspector + deno_inspector_server: CDP断点全链路机器可判定(M) 暂停期看门狗必掐/豁免=disarm(N) 全局面无泄漏(O) 快照可调试(P) 每isolate独立target+LIFO(Q)"
  );
  println!("NOTE chrome-devtools-manual=人工附加验证见笔记（复跑不依赖）");

  if m && n && o && p && q {
    println!("exit 0");
  } else {
    println!("exit 1（存在 FAIL）");
    std::process::exit(1);
  }
}
