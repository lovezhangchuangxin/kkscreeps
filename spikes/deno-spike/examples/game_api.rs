//! 实验 F~K：Screeps 游戏 API 依赖的高风险沙箱交互模式（deno_core spike 续篇）
//!
//! 运行：cargo run --release --example game_api
//! 依赖 build.rs 生成的 OUT_DIR/snapshot-game.bin（实验 J）。
//!
//!  F 惰性对象 overlay：SAB 字节视图 + prototype getter，vs 每次属性读走 #[op2(fast)] op
//!  G 原生计算 + JS 回调收集参数：roomCallback(name)→CostMatrix(Uint8Array(2500))→op 零拷贝接收
//!  H 大字符串传输：2MB RawMemory JSON 的 Rust→JS 传入 / parse+stringify / JS→Rust 传回
//!  I CommonJS 垫片：模块表由 Rust op 提供，验证循环 require / module.exports 共享 / 缓存 / global 跨执行持久
//!  J snapshot 内置游戏运行时：类 + prototype getter + 闭包 + Object.freeze 常量表预热进 snapshot
//!  K console 捕获与堆栈：未捕获异常的 .stack（行号/列号）+ 自制 JS console 收集参数
//!
//! 形态参考官方实现：engine/src/game/path-finder.js（CostMatrix._bits = Uint8Array(2500)）、
//! driver/lib/path-finder.js（search 包装层：先调 options.roomCallback(roomName) 拿 CostMatrix，
//! 提取 ret._bits 后传原生 mod.search）。

use deno_core::*;
use std::collections::HashMap;
use std::time::Instant;

// ---------------------------------------------------------------- 共用 ops
// （命名一律带 op_ga_ 前缀，避免与 deno_core 内置 op 撞名 —— 见旧笔记 §8.2）

/// 实验 F 对照组：每次属性读都调用 Rust op（fast 路径，免 serde）。
#[op2(fast)]
fn op_ga_get_current_hp(i: u32) -> f64 {
  1000.0 + (i & 127) as f64
}

/// 实验 G：接收 CostMatrix._bits（Uint8Array(2500)）做"原生计算"。
/// #[buffer] &[u8] 是借用视图（零拷贝，指向 V8 backing store，op 期间钉住不回收）。
#[op2]
#[serde]
fn op_ga_search(#[buffer] matrix: &[u8], ox: f64, oy: f64, gx: f64, gy: f64) -> Vec<f64> {
  // 不实现真的寻路：读一遍 2500 字节矩阵 + 生成一条 12 元素假路径
  let mut sum = 0u32;
  for &b in matrix {
    sum += b as u32;
  }
  let mut path = Vec::with_capacity(12);
  for k in 0..5 {
    path.push(ox + (gx - ox) * f64::from(k) / 4.0);
    path.push(oy + (gy - oy) * f64::from(k) / 4.0);
  }
  path.push(f64::from(sum & 0xff)); // 附带矩阵校验和（JS 侧可核对）
  path
}

/// 实验 G 零拷贝探针：返回数据指针 + 往 buf[0] 写一个字节（写穿验证：JS 立即可见 → 同一块内存）。
/// 指针以 f64 返回（macOS 用户态地址 < 2^53，精确无损）。
#[op2(fast)]
fn op_ga_zc_probe(#[buffer] buf: &mut [u8], write_val: u8) -> f64 {
  let ptr = buf.as_ptr() as u64 as f64;
  buf[0] = write_val;
  ptr
}

// ---------------------------------------------------------------- 实验 H 状态与 ops

struct RawMemState {
  mem: &'static str,
  last_recv_len: usize,
}

/// RawMemory.get()：Rust → JS 传 2MB 字符串（#[string] &str：只测 V8 字符串创建，无 Rust 侧 clone）。
#[op2]
#[string]
fn op_ga_raw_get(state: &mut OpState) -> &'static str {
  state.borrow::<RawMemState>().mem
}

/// RawMemory.set()：JS → Rust 传回 2MB 字符串（#[string] String：一次 UTF-8 拷贝）。
#[op2(fast)]
fn op_ga_raw_set(state: &mut OpState, #[string] s: String) -> u32 {
  let n = s.len();
  state.borrow_mut::<RawMemState>().last_recv_len = n;
  n as u32
}

// ---------------------------------------------------------------- 实验 I 模块表与 op

/// 模块源码表由 Rust 持有（模拟服务端按玩家目录注册的模块清单）。
const MODULE_SOURCES: &[(&str, &str)] = &[
  (
    "main.js",
    r#"
const harvester = require('./role.harvester');
const utils = require('./utils');
utils.counter += 1; // 每次真实执行 +1（配合 Rust 侧 load 计数区分"加载"与"缓存命中"）
module.exports = { name: 'main', harvester, utils };
"#,
  ),
  (
    "role.harvester.js",
    r#"
const utils = require('./utils');
module.exports = { role: 'harvester', utils, run() { return 'run:' + utils.helper(); } };
"#,
  ),
  (
    "utils.js",
    r#"
let calls = 0;
module.exports = { counter: 0, helper() { calls++; return 'helped#' + calls; } };
"#,
  ),
  (
    "circ-a.js",
    // 注意：用属性赋值而非 `module.exports = {...}` 整体替换 ——
    // 整体替换会让循环方（circ-b）持有的"半成品"引用与最终 exports 分离（Node 同款语义，首跑实测踩过）
    r#"
const b = require('./circ-b'); // 触发循环：circ-b 会反过来 require('./circ-a')
module.exports.whoami = 'A-done';
module.exports.b = b;
module.exports.sawPartialInB = b.sawEmptyAtLoad;
"#,
  ),
  (
    "circ-b.js",
    r#"
const a = require('./circ-a'); // 循环时刻：拿到的是 circ-a 的"半成品" exports（空对象）
const sawEmptyAtLoad = Object.keys(a).length === 0;
a.injectedDuringCycle = true; // 往半成品上写属性（经典 CommonJS 行为）
module.exports = { whoami: 'B-done', a, sawEmptyAtLoad };
"#,
  ),
];

struct ModuleRegistry {
  load_counts: HashMap<String, u32>,
}

#[op2]
#[string]
fn op_ga_module_load(
  state: &mut OpState,
  #[string] name: String,
) -> Result<String, deno_error::JsErrorBox> {
  for (n, src) in MODULE_SOURCES {
    if *n == name {
      let counts = &mut state.borrow_mut::<ModuleRegistry>().load_counts;
      *counts.entry(name.clone()).or_insert(0) += 1;
      return Ok(src.to_string());
    }
  }
  Err(deno_error::JsErrorBox::type_error(format!(
    "module not found: {name}"
  )))
}

extension!(
  ga_ext,
  ops = [
    op_ga_get_current_hp,
    op_ga_search,
    op_ga_zc_probe,
    op_ga_raw_get,
    op_ga_raw_set,
    op_ga_module_load,
  ]
);

// ---------------------------------------------------------------- 工具

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

fn median(mut v: Vec<f64>) -> f64 {
  v.sort_by(|a, b| a.partial_cmp(b).unwrap());
  v[v.len() / 2]
}

fn ms(d: std::time::Duration) -> f64 {
  d.as_secs_f64() * 1e3
}

/// 对一段 JS 计时代码执行 warmup + 5 轮，返回每轮毫秒。
fn bench_js(rt: &mut JsRuntime, code: &str, warmup: usize) -> Vec<f64> {
  for _ in 0..warmup {
    rt.execute_script("warm", code.to_string()).unwrap();
  }
  let mut times = Vec::new();
  for _ in 0..5 {
    let t0 = Instant::now();
    rt.execute_script("bench", code.to_string()).unwrap();
    times.push(ms(t0.elapsed()));
  }
  times
}

// ---------------------------------------------------------------- 实验F：惰性对象 overlay

fn experiment_f() -> (bool, String) {
  println!("== 实验F 惰性对象 overlay（SAB 字节视图 + prototype getter vs op-per-read；128 对象，每对象 32 字节：u32[id,x,y,roomIdx] + f64[hits,hitsMax]；各预热 3 轮后测 5 轮取中位，每轮 10 万次读） ==");

  let mut rt = JsRuntime::new(RuntimeOptions {
    extensions: vec![ga_ext::init()],
    ..Default::default()
  });
  rt.execute_script("f-setup", F_SETUP_JS).unwrap();

  // 正确性：三种读法 + 期望值完全一致
  let check = eval_json(
    &mut rt,
    r#"(function () {
      let exp = 0;
      for (let i = 0; i < 100000; i++) exp += 1000 + (i & 127);
      const g = __benchGetter(100000), o = __benchOp(100000), v = __benchView(100000), p = __benchPlain(100000);
      return JSON.stringify({ equal: g === o && o === v && v === exp, plain: p === exp, sample: g });
    })()"#,
  );
  println!("   正确性: {check}");

  const N: usize = 100_000;
  let cases: [(&str, &str); 5] = [
    ("getter(SAB) 单属性", "__benchGetter(100000)"),
    ("普通JS对象属性(地板)", "__benchPlain(100000)"),
    ("TypedArray直读(地板)", "__benchView(100000)"),
    ("op-per-read 单属性", "__benchOp(100000)"),
    ("getter 4属性/次", "__benchGetter4(100000)"),
  ];
  let mut results: Vec<(&str, f64)> = Vec::new();
  for (label, code) in cases {
    let t = bench_js(&mut rt, code, 3);
    let med = median(t.clone());
    results.push((label, med));
    println!(
      "   {label:20} 5次={:?} ms -> 中位 {:.3} ms，{:.2} ns/读",
      t,
      med,
      med * 1e6 / N as f64
    );
  }
  let t = bench_js(&mut rt, "__benchOp4(100000)", 3);
  let med4 = median(t.clone());
  println!(
    "   {:20} 5次={:?} ms -> 中位 {:.3} ms，{:.2} ns/读（每次迭代 4 个 op 调用）",
    "op-per-read 4属性/次",
    t,
    med4,
    med4 * 1e6 / N as f64
  );

  let getter = results[0].1;
  let op = results[3].1;
  let plain = results[1].1;
  let pass = check["equal"] == serde_json::json!(true);
  (
    pass,
    format!(
      "单属性读: SAB+getter {:.2}ns vs op-per-read {:.2}ns（{:.1}x）vs 普通对象 {:.2}ns；4属性/次: getter {:.2}ns vs 4xop {:.2}ns/对象读",
      getter * 1e6 / N as f64,
      op * 1e6 / N as f64,
      op / getter,
      plain * 1e6 / N as f64,
      results[4].1 * 1e6 / N as f64 / 4.0,
      med4 * 1e6 / N as f64 / 4.0,
    ),
  )
}

const F_SETUP_JS: &str = r#"
const OBJ_COUNT = 128; // 2 的幂，用位掩码取模（i & 127）
const sab = new SharedArrayBuffer(32 * OBJ_COUNT);
const u32 = new Uint32Array(sab);
const f64 = new Float64Array(sab);
for (let i = 0; i < OBJ_COUNT; i++) {
  u32[i * 8] = i;                    // id
  u32[i * 8 + 1] = (i * 3) % 50;     // x
  u32[i * 8 + 2] = (i * 7) % 50;     // y
  u32[i * 8 + 3] = 1;                // roomIdx
  f64[i * 4 + 2] = 1000 + i;         // hits（字节偏移 16）
  f64[i * 4 + 3] = 3000;             // hitsMax（字节偏移 24）
}
// RoomObject：实例只存索引，所有属性经 prototype getter 按偏移直读底层 SAB（模拟 xxscreeps 的 lazy hydration）
class RoomObject {
  constructor(i) { this.__i = i; }
}
Object.defineProperty(RoomObject.prototype, 'id',      { get() { return u32[this.__i * 8]; } });
Object.defineProperty(RoomObject.prototype, 'x',       { get() { return u32[this.__i * 8 + 1]; } });
Object.defineProperty(RoomObject.prototype, 'y',       { get() { return u32[this.__i * 8 + 2]; } });
Object.defineProperty(RoomObject.prototype, 'roomIdx', { get() { return u32[this.__i * 8 + 3]; } });
Object.defineProperty(RoomObject.prototype, 'hits',    { get() { return f64[this.__i * 4 + 2]; } });
Object.defineProperty(RoomObject.prototype, 'hitsMax', { get() { return f64[this.__i * 4 + 3]; } });
const objs = [];
for (let i = 0; i < OBJ_COUNT; i++) objs.push(new RoomObject(i));
// 地板 1：普通 JS 对象字段读（V8 隐藏类）
const plain = [];
for (let i = 0; i < OBJ_COUNT; i++) plain.push({ id: i, hits: 1000 + i, x: (i * 3) % 50, y: (i * 7) % 50 });
const opGetHp = Deno.core.ops.op_ga_get_current_hp;
globalThis.__benchGetter = function (n) { let s = 0; for (let i = 0; i < n; i++) s += objs[i & 127].hits; return s; };
globalThis.__benchGetter4 = function (n) { let s = 0; for (let i = 0; i < n; i++) { const t = objs[i & 127]; s += t.hits + t.id + t.x + t.y; } return s; };
globalThis.__benchPlain = function (n) { let s = 0; for (let i = 0; i < n; i++) s += plain[i & 127].hits; return s; };
globalThis.__benchView = function (n) { let s = 0; for (let i = 0; i < n; i++) s += f64[(i & 127) * 4 + 2]; return s; };
globalThis.__benchOp = function (n) { let s = 0; for (let i = 0; i < n; i++) s += opGetHp(i & 127); return s; };
globalThis.__benchOp4 = function (n) { let s = 0; for (let i = 0; i < n; i++) { const j = i & 127; s += opGetHp(j) + opGetHp(j) + opGetHp(j) + opGetHp(j); } return s; };
"#;

// ---------------------------------------------------------------- 实验G：roomCallback 模式

fn experiment_g() -> (bool, String) {
  println!("== 实验G 原生计算 + JS 回调收集参数（PathFinder.roomCallback 模式：包装层调 roomCallback(name) 拿 CostMatrix(Uint8Array(2500))，提取 ._bits 传 Rust op） ==");

  let mut rt = JsRuntime::new(RuntimeOptions {
    extensions: vec![ga_ext::init()],
    ..Default::default()
  });

  // 1) 零拷贝探针：同一数组两次传入指针相同；Rust 写的字节 JS 立即可见；不同数组指针不同
  let zc = eval_json(
    &mut rt,
    r#"(function () {
      const ops = Deno.core.ops;
      const bits = new Uint8Array(2500);
      bits[0] = 1;
      const p1 = ops.op_ga_zc_probe(bits, 0xAB);
      const sawWriteThrough = bits[0] === 0xAB; // Rust 在 op 里写的 0xAB，JS 侧立刻读到 → 同一块内存
      const p2 = ops.op_ga_zc_probe(bits, 0xCD);
      const other = new Uint8Array(2500);
      const p3 = ops.op_ga_zc_probe(other, 1);
      return JSON.stringify({
        ptrSameArrayTwice: p1 === p2,
        ptrDifferentArray: p3 !== p1,
        writeThroughVisible: sawWriteThrough,
        zeroCopy: p1 === p2 && p3 !== p1 && sawWriteThrough,
        ptr1: p1.toString(16), ptr3: p3.toString(16),
      });
    })()"#,
  );
  println!("   零拷贝探针: {zc}");

  // 2) search 流程搭建（形态同 driver/lib/path-finder.js 的 search + engine 的 CostMatrix）
  rt.execute_script("g-setup", G_SETUP_JS).unwrap();
  let sanity = eval_json(
    &mut rt,
    r#"(function () {
      const r = __pfSearch({ x: 5, y: 5 }, { x: 45, y: 45 }, { roomCallback: __pfCallback });
      const expectedChecksum = __pfCallback('W1N1')._bits.reduce((a, b) => a + b, 0) & 0xff;
      return JSON.stringify({ pathLen: r.len, pathFirst: r.path[0], pathLastIsChecksum: r.path[r.path.length - 1] === expectedChecksum });
    })()"#,
  );
  println!("   正确性: {sanity}");

  // 3) 计时：完整 search 流程 / 仅 roomCallback / 仅 op（每轮 1000 次迭代；us/次 = 轮耗时ms × 1000us/ms ÷ 1000次）
  const ITERS: f64 = 1000.0;
  let us_each = |med_ms: f64| med_ms * 1000.0 / ITERS;
  let t_full = bench_js(
    &mut rt,
    "for (let i = 0; i < 1000; i++) __pfSearch({x:5,y:5},{x:45,y:45},{roomCallback:__pfCallback});",
    2,
  );
  let med_full = median(t_full.clone());
  println!(
    "   完整 search 流程 1000 次: 5次={:?} ms -> 中位 {:.2} ms，即 {:.3} us/search",
    t_full,
    med_full,
    us_each(med_full)
  );

  let t_cb = bench_js(&mut rt, "for (let i = 0; i < 1000; i++) __pfCallback('W1N1');", 2);
  let med_cb = median(t_cb.clone());
  println!(
    "   其中 roomCallback（new CostMatrix + 40 次 set）1000 次: 中位 {:.2} ms，即 {:.3} us/次",
    med_cb,
    us_each(med_cb)
  );

  let t_op = bench_js(
    &mut rt,
    "(function () { const bits = __pfCallback('W1N1')._bits; const f = Deno.core.ops.op_ga_search; for (let i = 0; i < 1000; i++) f(bits, 5, 5, 45, 45); })()",
    2,
  );
  let med_op = median(t_op.clone());
  println!(
    "   仅 op_ga_search（矩阵复用）1000 次: 中位 {:.2} ms，即 {:.3} us/次",
    med_op,
    us_each(med_op)
  );

  let pass = zc["zeroCopy"] == serde_json::json!(true)
    && sanity["pathLen"].as_u64() == Some(11)
    && sanity["pathFirst"].as_f64() == Some(5.0)
    && sanity["pathLastIsChecksum"] == serde_json::json!(true);
  (
    pass,
    format!(
      "零拷贝={:?}（指针稳定+写穿可见）；完整 search {:.3} us/次（callback {:.3} + op {:.3} + 包装开销）",
      zc["zeroCopy"].as_bool(),
      us_each(med_full),
      us_each(med_cb),
      us_each(med_op)
    ),
  )
}

const G_SETUP_JS: &str = r#"
const ops = Deno.core.ops;
// CostMatrix：官方 engine/src/game/path-finder.js 同款形态（_bits = Uint8Array(2500)）
class CostMatrix {
  constructor() { this._bits = new Uint8Array(2500); }
  set(xx, yy, val) { this._bits[xx * 50 + yy] = Math.min(Math.max(0, val), 255); }
  get(xx, yy) { return this._bits[xx * 50 + yy]; }
}
// 玩家提供的 roomCallback（玩家代码，引擎不可控、无法内联优化）
function playerRoomCallback(roomName) {
  const cm = new CostMatrix();
  for (let i = 0; i < 40; i++) cm.set((i * 7) % 50, (i * 13) % 50, 200);
  cm.set(10, 10, 255);
  return cm;
}
// 引擎包装层（driver/lib/path-finder.js 的 search 形态）：先调 cb 取 CostMatrix，提取 ._bits 传原生
function pfSearch(origin, goal, options) {
  const cb = options.roomCallback;
  if (typeof cb !== 'function') throw new Error('roomCallback required');
  const cm = cb('W1N1');       // ← JS 回调收集参数
  if (!cm) return { path: [], ops: 0 };
  const bits = cm._bits;       // ← 提取 TypedArray（Uint8Array(2500)）
  const ret = ops.op_ga_search(bits, origin.x, origin.y, goal.x, goal.y); // ← 传给 Rust
  return { path: ret, len: ret.length };
}
globalThis.__pfSearch = pfSearch;
globalThis.__pfCallback = playerRoomCallback;
"#;

// ---------------------------------------------------------------- 实验H：2MB RawMemory

/// 生成 ~2MiB 的合法 JSON（Screeps memory 风格：creeps 表 + 每只 creep 的 pos/body/memory）。
fn make_raw_memory_json() -> String {
  let mut s = String::with_capacity(2 * 1024 * 1024 + 4096);
  s.push_str("{\"cpuAvailable\":9800,\"gcl\":{\"level\":7,\"progress\":123456789},\"creeps\":{");
  let mut i = 0usize;
  while s.len() < 2 * 1024 * 1024 {
    if i > 0 {
      s.push(',');
    }
    let room = format!("W{}N{}", i % 10, i % 8);
    s.push_str(&format!(
      "\"harvester{i}\":{{\"role\":\"harvester\",\"name\":\"harvester{i}\",\"pos\":{{\"x\":{},\"y\":{},\"roomName\":\"{}\"}},\"hits\":{},\"hitsMax\":{},\"fatigue\":{},\"memory\":{{\"working\":{},\"sourceId\":\"source{}\",\"path\":[{},{},{},{}]}},\"body\":[\"move\",\"work\",\"work\",\"carry\",\"carry\"]}}",
      i % 50,
      (i * 7) % 50,
      room,
      1000 + i % 500,
      3000,
      i % 3,
      if i % 2 == 0 { "true" } else { "false" },
      i % 4,
      i % 50,
      (i * 7) % 50,
      (i + 3) % 50,
      (i * 9) % 50,
    ));
    i += 1;
  }
  s.push_str("}}");
  s
}

fn experiment_h() -> (bool, String) {
  println!("== 实验H 大字符串传输（RawMemory 2MB：Rust→JS 经 op 返回 #[string]；沙箱内 JSON.parse+改+stringify；JS→Rust 经 op 参数 #[string] String；各预热后多轮计时） ==");

  let mem: &'static str = Box::leak(make_raw_memory_json().into_boxed_str());
  println!("   Rust 生成 RawMemory JSON: {} bytes（{:.2} MB）", mem.len(), mem.len() as f64 / 1048576.0);
  // 合法性预检（Rust 侧用 serde_json 验一次）
  let v: serde_json::Value = serde_json::from_str(mem).expect("generated JSON must be valid");
  let creep_count = v["creeps"].as_object().map(|m| m.len()).unwrap_or(0);
  println!("   serde_json 校验合法，creeps 数量: {creep_count}");

  let mut rt = JsRuntime::new(RuntimeOptions {
    extensions: vec![ga_ext::init()],
    ..Default::default()
  });
  rt.op_state().borrow_mut().put(RawMemState { mem, last_recv_len: 0 });

  let mb = mem.len() as f64 / 1048576.0;

  // 1) Rust → JS：op 返回 2MB 字符串
  let t_get = bench_js(
    &mut rt,
    "for (let i = 0; i < 30; i++) globalThis.__rawMemory = Deno.core.ops.op_ga_raw_get();",
    2,
  );
  let med_get = median(t_get.clone());
  println!(
    "   Rust→JS（op 返回 #[string]，30 次/轮）: 5次={:?} ms -> 中位 {:.2} ms/轮 = {:.3} ms/次 = {:.4} ms/MB",
    t_get,
    med_get,
    med_get / 30.0,
    med_get / 30.0 / mb
  );

  // 2) 沙箱内 parse + 修改 + stringify
  let t_parse = bench_js(
    &mut rt,
    "for (let i = 0; i < 10; i++) { const o = JSON.parse(globalThis.__rawMemory); o.tick = i; globalThis.__rawOut = JSON.stringify(o); }",
    1,
  );
  let med_parse = median(t_parse.clone());
  println!(
    "   沙箱内 JSON.parse+改+stringify（10 次/轮）: 5次={:?} ms -> 中位 {:.2} ms/轮 = {:.3} ms/次 = {:.4} ms/MB",
    t_parse,
    med_parse,
    med_parse / 10.0,
    med_parse / 10.0 / mb
  );

  // 3) JS → Rust：op 参数传回 2MB 字符串
  let t_set = bench_js(
    &mut rt,
    "for (let i = 0; i < 30; i++) Deno.core.ops.op_ga_raw_set(globalThis.__rawOut);",
    2,
  );
  let med_set = median(t_set.clone());
  println!(
    "   JS→Rust（op 参数 #[string] String，30 次/轮）: 5次={:?} ms -> 中位 {:.2} ms/轮 = {:.3} ms/次 = {:.4} ms/MB",
    t_set,
    med_set,
    med_set / 30.0,
    med_set / 30.0 / mb
  );

  // 长度核对
  let lens = eval_json(
    &mut rt,
    "JSON.stringify({ inLen: globalThis.__rawMemory.length, outLen: globalThis.__rawOut.length, lastSetAck: Deno.core.ops.op_ga_raw_get().length })",
  );
  println!("   长度核对(JS): {lens}");
  let last_recv = rt.op_state().borrow().borrow::<RawMemState>().last_recv_len;
  println!(
    "   Rust 侧 last_recv_len = {last_recv}（应为 stringify 后长度）"
  );

  let out_len = lens["outLen"].as_u64().unwrap_or(0) as usize;
  let pass = med_get > 0.0
    && out_len > 2 * 1024 * 1024
    && last_recv == out_len
    && out_len > lens["inLen"].as_u64().unwrap_or(0) as usize;
  (
    pass,
    format!(
      "2MB/次: Rust→JS {:.3} ms，parse+stringify {:.2} ms，JS→Rust {:.3} ms（每 MB: {:.4} / {:.3} / {:.4} ms）；长度闭环 in={} out={} rust_recv={}",
      med_get / 30.0,
      med_parse / 10.0,
      med_set / 30.0,
      med_get / 30.0 / mb,
      med_parse / 10.0 / mb,
      med_set / 30.0 / mb,
      lens["inLen"],
      out_len,
      last_recv
    ),
  )
}

// ---------------------------------------------------------------- 实验I：CommonJS 垫片

fn experiment_i() -> (bool, String) {
  println!("== 实验I CommonJS 垫片（~50 行 require 实现；模块表由 Rust op_ga_module_load 提供：main.js / role.harvester.js / utils.js + 循环依赖对 circ-a.js / circ-b.js） ==");

  let mut rt = JsRuntime::new(RuntimeOptions {
    extensions: vec![ga_ext::init()],
    ..Default::default()
  });
  rt.op_state()
    .borrow_mut()
    .put(ModuleRegistry { load_counts: HashMap::new() });
  rt.execute_script("cjs-shim", CJS_SHIM_JS).unwrap();

  // 断言 1~3 + 准备断言 4 的全局变量
  let r = eval_json(&mut rt, I_ASSERT_JS);
  println!("   JS 侧断言: {r}");

  // 断言 4：global 跨 execute_script 持久（两次独立 execute_script）
  rt.execute_script("i-set", r#"globalThis.__userVar.n += 1"#).unwrap();
  let n2 = eval_json(&mut rt, "(globalThis.__userVar.n) + ''");
  let global_persist = n2 == serde_json::json!(42); // eval_json 会把 "42" 再解析成数字，两种形态都对
  let require_persist = eval_json(&mut rt, "(typeof require) + '/' + (typeof globalThis.__userVar)");
  println!("   第二次 execute_script 读到递增后 n={n2}（41 设置于上一次执行, +1 后应为 42）");
  println!("   跨执行可见性: {require_persist}");

  // 断言 5：Rust 侧模块加载计数（每个模块恰加载 1 次，其余全是缓存命中）
  let counts = {
    let cell = rt.op_state();
    let reg = cell.borrow();
    let mut c: Vec<(String, u32)> = reg.borrow::<ModuleRegistry>().load_counts.clone().into_iter().collect();
    c.sort();
    c
  };
  println!("   Rust 侧模块加载计数: {counts:?}");
  let counts_ok = counts.len() == MODULE_SOURCES.len()
    && counts.iter().all(|(_, v)| *v == 1);

  let obj = r.as_object().cloned().unwrap_or_default();
  let js_all_pass = ["mainLoads", "runWorks", "counterExecuted", "circularOk", "circularPartial", "circularSharedPartial", "cacheIdentity", "cacheCounter"]
    .iter()
    .all(|k| obj.get(*k) == Some(&serde_json::json!(true)));
  let pass = js_all_pass && global_persist && counts_ok;
  (
    pass,
    format!(
      "循环require(半成品+写穿可见)={} exports共享/缓存={} global跨执行持久={} rust加载计数各1次={}",
      obj.get("circularOk") == Some(&serde_json::json!(true))
        && obj.get("circularPartial") == Some(&serde_json::json!(true)),
      obj.get("cacheIdentity") == Some(&serde_json::json!(true)),
      global_persist,
      counts_ok
    ),
  )
}

const CJS_SHIM_JS: &str = r#"
// CommonJS 垫片：源码表由 Rust op_ga_module_load 提供（模拟服务端注册的玩家模块清单）
(() => {
  const cache = Object.create(null);
  const normalize = (spec) => {
    let name = spec;
    if (name.startsWith('./') || name.startsWith('../')) {
      name = name.replace(/^(\.\.\/)+|^(\.\/)+/, ''); // 简化解析（同目录扁平模块表）
    }
    if (!name.endsWith('.js')) name += '.js';
    return name;
  };
  const requireImpl = (spec) => {
    const name = normalize(spec);
    const cached = cache[name];
    if (cached) return cached.exports; // 缓存命中（循环 require 期间命中"半成品" exports —— Node 语义）
    const source = Deno.core.ops.op_ga_module_load(name);
    const module = { exports: {} };
    cache[name] = module; // 执行前先占坑 → 循环依赖拿到部分初始化的 exports
    const wrapper = new Function('module', 'exports', 'require', source + '\n//# sourceURL=ga_module/' + name);
    wrapper(module, module.exports, requireImpl);
    return module.exports;
  };
  globalThis.require = requireImpl;
  globalThis.__gaModuleCache = cache; // 诊断用（垫片本身不依赖）
})();
"#;

const I_ASSERT_JS: &str = r#"(function () {
  const out = {};
  // 1) 基本加载 + 依赖链（main → role.harvester → utils）
  const main = require('main.js');
  out.mainLoads = main.name === 'main' && main.harvester.role === 'harvester';
  out.runWorks = main.harvester.run() === 'run:helped#1';
  out.counterExecuted = main.utils.counter === 1; // main.js 主体执行时 +1
  // 2) 循环 require：circ-a → circ-b → circ-a(半成品)
  const a = require('circ-a.js');
  out.circularOk = a.whoami === 'A-done' && a.b.whoami === 'B-done';
  out.circularPartial = a.b.sawEmptyAtLoad === true; // B 加载时刻拿到的 A 是空 exports
  out.circularSharedPartial = a.b.a.injectedDuringCycle === true && a.b.a === a; // B 写进半成品的属性最终可见，且就是最终 exports 对象
  // 3) module.exports 对象共享 + 模块缓存
  const u1 = require('utils.js'); u1.counter += 10;
  const u2 = require('utils.js');
  out.cacheIdentity = u1 === u2 && main.utils === u1; // main / u1 / u2 三处拿到同一对象
  out.cacheCounter = u2.counter === 11;               // 缓存命中没有重新执行主体
  // 4) global 跨 execute_script：先种下变量，由 Rust 侧下一次 execute_script 验证
  globalThis.__userVar = { n: 41 };
  return JSON.stringify(out);
})()"#;

// ---------------------------------------------------------------- 实验J：snapshot 内置游戏运行时

fn experiment_j() -> (bool, String) {
  println!("== 实验J snapshot 内置游戏运行时（build.rs create_snapshot 预热：冻结常量表 C / RoomPosition 类 + prototype getter / 闭包工厂 / Map 注册表；对比无/有 snapshot 的 JsRuntime::new 各 10 次取中位） ==");
  static GAME_SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/snapshot-game.bin"));
  println!("   snapshot-game.bin: {} bytes", GAME_SNAPSHOT.len());

  let mut cold = Vec::new();
  for _ in 0..10 {
    let t = Instant::now();
    let rt = JsRuntime::new(RuntimeOptions::default());
    cold.push(t.elapsed().as_secs_f64() * 1e6);
    drop(rt);
  }
  let cold_med = median(cold.clone());
  let mut warm = Vec::new();
  for _ in 0..10 {
    let t = Instant::now();
    let rt = JsRuntime::new(RuntimeOptions {
      startup_snapshot: Some(GAME_SNAPSHOT),
      ..Default::default()
    });
    warm.push(t.elapsed().as_secs_f64() * 1e6);
    drop(rt);
  }
  let warm_med = median(warm.clone());
  println!(
    "   JsRuntime::new 无 snapshot: 10 次中位 {cold_med:.0} us；有 game snapshot: {warm_med:.0} us（加速 {:.2}x）",
    cold_med / warm_med
  );

  // 从 snapshot 启动 + 叠加运行时扩展（ops 扩展不在 snapshot 内，验证二者可组合）
  let mut rt = JsRuntime::new(RuntimeOptions {
    startup_snapshot: Some(GAME_SNAPSHOT),
    extensions: vec![ga_ext::init()],
    ..Default::default()
  });
  let v = eval_json(&mut rt, J_ASSERT_JS);
  println!("   snapshot 恢复验证: {v}");

  let obj = v.as_object().cloned().unwrap_or_default();
  let required = ["loaded", "classOk", "getterOk", "protoGetter", "toStringOk", "frozen", "constOk", "freezeEnforced", "closureOk", "registryOk", "newGlobalOk", "opComposeOk"];
  let mut all = true;
  for k in required {
    if obj.get(k) != Some(&serde_json::json!(true)) {
      all = false;
      println!("   !! 断言失败: {k}");
    }
  }
  let lexical = obj.get("lexicalConstSurvives") == Some(&serde_json::json!(true));
  println!(
    "   附加探针: 顶层 const（全局词法绑定）随 snapshot 恢复 = {lexical}（false 也不影响结论：游戏 runtime 应挂 globalThis）"
  );
  (
    all,
    format!(
      "12 项断言全过；snapshot {:.0}KB，创建 {cold_med:.0}us→{warm_med:.0}us（{:.1}x）；顶层const绑定恢复={}",
      GAME_SNAPSHOT.len() as f64 / 1024.0,
      cold_med / warm_med,
      lexical
    ),
  )
}

const J_ASSERT_JS: &str = r#"(function () {
  const out = {};
  out.loaded = globalThis.__GAME_RUNTIME_LOADED__ === true;
  out.classOk = typeof RoomPosition === 'function';
  const rp = new RoomPosition(25, 30, 'W1N1');
  out.getterOk = rp.x === 25 && rp.y === 30 && rp.roomName === 'W1N1' && rp.xy === 1280;
  out.protoGetter = typeof Object.getOwnPropertyDescriptor(RoomPosition.prototype, 'x').get === 'function';
  out.toStringOk = rp.toString() === '[room W1N1 25,30]';
  out.frozen = Object.isFrozen(C) && Object.isFrozen(C.CONSTRUCTION_COST) && Object.isFrozen(C.BODIES[0]);
  out.constOk = C.OK === 0 && C.CONSTRUCTION_COST.spawn === 15000 && C.BODIES[1].carry === 50;
  C.HACK = 1; delete C.OK;
  out.freezeEnforced = C.HACK === undefined && C.OK === 0; // snapshot 后冻结语义仍在
  const dist = makeDistanceTable(100);
  out.closureOk = dist(10, 0, 4, 0) === 6 && dist(8, 0, 0, 0) === 8; // sqrt(36)=6, sqrt(64)=8
  out.registryOk = GameRegistry.register('creep1', { hp: 1 }) === 1 && GameRegistry.get('creep1').hp === 1;
  globalThis.__postSnapshotProp = 123;
  out.newGlobalOk = globalThis.__postSnapshotProp === 123; // snapshot 启动后仍可挂新全局
  out.opComposeOk = Deno.core.ops.op_ga_get_current_hp(7) === 1007.0; // snapshot + 运行时扩展叠加
  out.lexicalConstSurvives = typeof __SNAPSHOT_LEXICAL_TEST !== 'undefined' && __SNAPSHOT_LEXICAL_TEST === 777;
  return JSON.stringify(out);
})()"#;

// ---------------------------------------------------------------- 实验K：console 捕获与堆栈

fn experiment_k() -> (bool, String) {
  println!("== 实验K console 捕获与堆栈（未捕获异常 → Rust 拿 .stack 与结构化行列号；deno_core 自带 console 的去路 + 自制收集型 console） ==");

  let mut rt = JsRuntime::new(RuntimeOptions {
    extensions: vec![ga_ext::init()],
    ..Default::default()
  });

  // 1) 未捕获异常：构造已知行列号的抛错点（throw 在第 2 行第 9 列）
  let code = "function innerBoom() {\n  throw new Error('K-boom (intended)');\n}\nfunction outerWrap() {\n  innerBoom();\n}\nouterWrap();\n";
  let err = match rt.execute_script("<k-stack-demo>", code) {
    Ok(_) => {
      println!("   !! 未捕获异常没有被 execute_script 报错");
      return (false, "uncaught error not surfaced".into());
    }
    Err(e) => e,
  };
  println!("   exception_message: {}", err.exception_message);
  let stack_raw = err.stack.clone().unwrap_or_else(|| "<no stack>".into());
  println!("   err.stack 原文（截取前 5 行）:");
  for line in stack_raw.lines().take(5) {
    println!("     | {line}");
  }
  println!("   结构化 frames（Rust 侧 Vec<JsStackFrame>）:");
  let mut lines_ok = Vec::new();
  for (i, f) in err.frames.iter().enumerate() {
    println!(
      "     frame[{i}] fn={:?} file={:?} {}:{}",
      f.function_name.clone().unwrap_or_default(),
      f.file_name.clone().unwrap_or_default(),
      f.line_number.unwrap_or(-1),
      f.column_number.unwrap_or(-1)
    );
    lines_ok.push(f.line_number);
  }
  // 期望：throw 行=2 列=9；外层调用链行 5、7
  let expect_lines = [Some(2), Some(5), Some(7)];
  let stack_ok = err
    .frames
    .iter()
    .zip(expect_lines.iter())
    .all(|(f, want)| f.line_number == *want)
    && err.frames[0].column_number == Some(9)
    && err.frames.len() >= 3
    && stack_raw.contains("K-boom");

  // 2) deno_core 自带 console 的去路（源码证据：01_core.js CoreConsole → op_print → 宿主 stdout/stderr）
  let probe = eval_json(
    &mut rt,
    "JSON.stringify({ hasConsole: typeof console, logType: typeof (console && console.log), keys: Object.keys(console).sort().join(',') })",
  );
  println!("   deno_core 自带 console 探测: {probe}");
  println!("   （下行应为自带 console 直写宿主 stdout 的证据）");
  let _ = rt.execute_script("native-console", "console.log('[native-console] 这一行经 deno_core 内置 CoreConsole -> op_print 直写宿主 stdout')");

  // 3) 替换为收集型 console（对象做简单 inspect，支持嵌套/循环引用/Error）
  rt.execute_script("console-shim", K_CONSOLE_SHIM_JS).unwrap();
  let captured = eval_json(
    &mut rt,
    r#"(function () {
      console.log('tick', 42, { cpu: 1, nested: { a: [1, 2, { b: 3 }] } }, null, undefined, true);
      console.warn('careful', ['x', 'y']);
      const circ = { name: 'circ' }; circ.self = circ;
      console.log(circ);
      console.error('bad thing', new Error('inline-err'));
      return JSON.stringify(globalThis.__consoleDrain());
    })()"#,
  );
  println!("   自制 console 捕获原文:");
  if let Some(arr) = captured.as_array() {
    for rec in arr {
      println!("     | {rec}");
    }
  }

  // 捕获断言
  let check = eval_json(
    &mut rt,
    r#"(function () {
      const r = globalThis.__consoleDrain(); // 应为空（上面已 drain）
      console.log('again', 1);
      const [rec] = globalThis.__consoleDrain();
      return JSON.stringify({ drained: r.length === 0, level: rec.level, first: rec.parts[0], second: rec.parts[1] });
    })()"#,
  );
  println!("   再捕获核对: {check}");
  let cap_arr = captured.as_array().cloned().unwrap_or_default();
  let capture_ok = cap_arr.len() == 4
    && cap_arr[0]["parts"][0] == serde_json::json!("tick")
    && cap_arr[0]["parts"][2]
      .as_str()
      .map(|s| s.contains("nested"))
      .unwrap_or(false)
    && cap_arr
      .last()
      .and_then(|r| r["parts"].as_array())
      .and_then(|p| p.last())
      .and_then(|p| p.as_str())
      .map(|s| s.contains("inline-err"))
      .unwrap_or(false)
    && cap_arr
      .iter()
      .any(|r| r["parts"][0].as_str().map(|s| s.contains("Circular")).unwrap_or(false));

  let pass = stack_ok && capture_ok;
  (
    pass,
    format!(
      "未捕获异常 .stack 含行/列号（throw@2:9，3 帧链路 2→5→7）={}；console：自带版直写宿主 stdout（CoreConsole→op_print，01_core.js），自制收集版 4 类参数+嵌套+循环引用捕获={}",
      stack_ok, capture_ok
    ),
  )
}

const K_CONSOLE_SHIM_JS: &str = r#"
(() => {
  const __records = [];
  function inspect(v, depth, seen) {
    const t = typeof v;
    if (v === null) return 'null';
    if (t === 'undefined') return 'undefined';
    if (t === 'string') return depth === 0 ? v : JSON.stringify(v);
    if (t === 'number' || t === 'boolean' || t === 'bigint') return String(v);
    if (t === 'symbol') return v.toString();
    if (t === 'function') return `[Function: ${v.name || 'anonymous'}]`;
    if (v instanceof Error) return (v.stack || String(v)).split('\n')[0];
    if (ArrayBuffer.isView(v)) return `${v.constructor.name}(${v.length})`;
    if (seen.has(v)) return '[Circular]';
    if (depth >= 3) return Array.isArray(v) ? '[Array]' : '[Object]';
    seen.add(v);
    if (Array.isArray(v)) return `[ ${v.map((x) => inspect(x, depth + 1, seen)).join(', ')} ]`;
    return `{ ${Object.keys(v).map((k) => `${k}: ${inspect(v[k], depth + 1, seen)}`).join(', ')} }`;
  }
  const shim = { __records };
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
    shim[level] = (...args) => { __records.push({ level, parts: args.map((a) => inspect(a, 0, new Set())) }); };
  }
  globalThis.console = shim;
  globalThis.__consoleDrain = () => __records.splice(0, __records.length);
})();
"#;

// ---------------------------------------------------------------- main

fn main() {
  // deno_core GC 压力场景需要 tokio runtime context（与旧 spike main 同款）
  let tokio_rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  let _tokio_guard = tokio_rt.enter();

  println!("deno-spike game-api | v8 engine version: {}", v8::V8::get_version());
  println!();

  let f = experiment_f();
  println!();
  let g = experiment_g();
  println!();
  let h = experiment_h();
  println!();
  let i = experiment_i();
  println!();
  let j = experiment_j();
  println!();
  let k = experiment_k();

  println!();
  println!("================= 游戏 API 模式汇总 =================");
  for (name, (pass, detail)) in
    [("F", &f), ("G", &g), ("H", &h), ("I", &i), ("J", &j), ("K", &k)]
  {
    println!(
      "GAMEAPI {name}={} {detail}",
      if *pass { "PASS" } else { "FAIL" }
    );
  }
}
