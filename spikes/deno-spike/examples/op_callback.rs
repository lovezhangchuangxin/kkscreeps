//! 实验 G'（op_callback）：op 执行中途回调进沙箱 JS —— 官方 PathFinder roomCallback 模式的决定性验证。
//!
//! 背景：官方 driver 的 PathFinder.search 把 roomCallback 作为参数传给原生模块，
//! C++ 在 A* 搜索展开每个新房间时 Nan::Call 回调进沙箱（pf.cc:36-59）——惰性、只对真正
//! 进入的房间调用、返回 false 拉黑房间、回调抛异常则以 js_error 终止搜索。
//! deno_core 侧对应物 = op 接收 #[scoped] v8::Global<v8::Function>，在 op 内
//! Function::call 回调。本实验验证：
//!   G'1  op 能收到 JS 函数并在计算循环中多次回调（参数传递 + 返回值读取）
//!   G'2  返回值三态：Uint8Array(2500)（零拷贝读 backing store）/ false（拉黑）/ undefined（默认）
//!   G'3  回调抛异常 → 以错误传播到最外层 execute_script（不被吞、不崩进程、runtime 存活）
//!   G'4  回调开销量化（ns/次）——决定 64 房间上限搜索的回调总成本
//!
//! API 模式参照：deno_core webidl.rs:390 的 v8::tc_scope!（TryCatch 作用域内调用并取异常）
//! 与 examples/op2.rs:13 的 #[scoped] v8::Global<v8::Function> 参数。
//!
//! 运行：cargo run --release --example op_callback

use deno_core::*;
use std::time::Instant;

// ---------------------------------------------------------------- ops

/// 模拟官方 search：Rust 侧"搜索"若干房间，每进入一个新房间就回调 roomCallback(name)。
/// 返回 [实际接受的房间数, 全部矩阵校验和]（serde 元组 → JS 数组）。
#[op2]
#[serde]
fn op_oc_search(
  scope: &mut v8::PinScope,
  #[scoped] room_callback: v8::Global<v8::Function>,
  #[smi] total_rooms: i32,
) -> Result<(i32, u32), deno_error::JsErrorBox> {
  let mut entered = 0i32;
  let mut checksum = 0u32;
  for i in 0..total_rooms {
    let name = format!("room{}", i);
    v8::tc_scope!(let tc, scope);
    let name_val = v8::String::new(tc, &name)
      .ok_or_else(|| deno_error::JsErrorBox::generic("alloc string failed"))?;
    let recv: v8::Local<v8::Value> = v8::undefined(tc).into();
    let ret = room_callback.open(tc).call(tc, recv, &[name_val.into()]);
    let ret = match ret {
      Some(r) => r,
      None => {
        // G'3：回调抛了异常 —— tc_scope 模式下异常已被捕获，转换并向上传播
        let msg = if tc.has_caught() {
          let exc = tc.exception().unwrap();
          exc.to_rust_string_lossy(tc)
        } else {
          "callback returned empty".to_string()
        };
        return Err(deno_error::JsErrorBox::generic(format!("js_error: {msg}")));
      }
    };
    // G'2：三态返回值
    if ret.is_false() {
      continue; // false：拉黑该房间，不计入 entered
    }
    if ret.is_undefined() {
      entered += 1; // undefined：用默认地形
      continue;
    }
    if ret.is_uint8_array() {
      // Uint8Array(2500)：零拷贝读 backing store（与实验 G 的 #[buffer] 同源）
      let arr = ret
        .try_cast::<v8::Uint8Array>()
        .map_err(|e| deno_error::JsErrorBox::generic(format!("cast failed: {e:?}")))?;
      let Some(bs) = arr.get_backing_store() else {
        return Err(deno_error::JsErrorBox::generic("no backing store"));
      };
      // BackingStore deref 为 [Cell<u8>]（可共享内存语义），按 Cell 取值零拷贝求和
      checksum = checksum.wrapping_add(bs.iter().map(|c| u32::from(c.get())).sum::<u32>());
      entered += 1;
      continue;
    }
    return Err(deno_error::JsErrorBox::generic(
      "roomCallback must return CostMatrix | false | undefined",
    ));
  }
  Ok((entered, checksum))
}

/// G'4：纯回调往返开销（空回调与带参回调各一轮）。
#[op2]
fn op_oc_callback_bench(
  scope: &mut v8::PinScope,
  #[scoped] cb: v8::Global<v8::Function>,
  #[smi] n: i32,
) -> f64 {
  let t0 = Instant::now();
  for i in 0..n {
    let arg = v8::Integer::new(scope, i);
    let recv: v8::Local<v8::Value> = v8::undefined(scope).into();
    let _ = cb.open(scope).call(scope, recv, &[arg.into()]);
  }
  t0.elapsed().as_secs_f64() * 1e9 / f64::from(n)
}

extension!(op_callback_sample, ops = [op_oc_search, op_oc_callback_bench]);

// ---------------------------------------------------------------- helpers（照抄 game_api.rs 的 eval_json 模式）

fn eval_json(rt: &mut JsRuntime, code: &str) -> serde_json::Value {
  let g = rt
    .execute_script("<anon>", code.to_string())
    .expect("eval failed");
  deno_core::scope!(scope, rt);
  let local = v8::Local::new(scope, g);
  match serde_v8::from_v8(scope, local) {
    Ok(serde_json::Value::String(s)) => serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s)),
    Ok(v) => v,
    Err(e) => serde_json::json!(format!("<deserialize failed: {e:?}>")),
  }
}

// ---------------------------------------------------------------- runner

fn main() -> anyhow::Result<()> {
  let mut runtime = JsRuntime::new(RuntimeOptions {
    extensions: vec![op_callback_sample::init()],
    ..Default::default()
  });

  // ---- G'1/G'2：三态回调（每 4 个房间：1 个矩阵、1 个 undefined、1 个矩阵、1 个 false）
  let code1 = r#"
    globalThis.__calls = [];
    function roomCallback(name) {
      globalThis.__calls.push(name);
      const n = parseInt(name.slice(4), 10);
      if (n % 4 === 3) return false;          // 拉黑（false 分支）
      if (n % 4 === 1) return undefined;      // 默认地形（undefined 分支）
      const m = new Uint8Array(2500);         // CostMatrix（Uint8Array 分支）
      m.fill(n & 0xff);
      return m;
    }
    globalThis.__result = JSON.stringify(Deno.core.ops.op_oc_search(roomCallback, 10));
  "#;
  let t0 = Instant::now();
  runtime.execute_script("g1", code1.to_string())?;
  println!("[G'1/G'2] execute ok in {:?}", t0.elapsed());

  let result = eval_json(&mut runtime, "globalThis.__result");
  let calls = eval_json(&mut runtime, "globalThis.__calls.length");
  println!("[G'1/G'2] result={} calls={}", result, calls);
  // 10 个房间：false 2 个（3,7）→ entered=8；矩阵 5 个（0,2,4,6,8）校验和=2500*(0+2+4+6+8)=50000
  assert_eq!(calls, serde_json::json!(10), "每个'进入'的房间回调恰好一次");
  match &result {
    serde_json::Value::Array(arr) => {
      assert_eq!(arr[0], serde_json::json!(8), "拉黑 2 个房间后 entered 应为 8：{arr:?}");
      assert_eq!(arr[1], serde_json::json!(50000), "矩阵零拷贝读取的校验和必须精确：{arr:?}");
    }
    other => panic!("result 应为数组：{other:?}"),
  }
  println!("[G'1/G'2] 三态返回值 + 零拷贝矩阵读取全部断言通过");

  // ---- G'3：回调抛异常的传播 + runtime 存活
  let code3 = r#"
    globalThis.__threw = null;
    try {
      Deno.core.ops.op_oc_search(function (name) { throw new Error('boom from ' + name); }, 3);
    } catch (e) {
      globalThis.__threw = String(e);
    }
  "#;
  runtime.execute_script("g3a", code3.to_string())?;
  let threw = eval_json(&mut runtime, "globalThis.__threw");
  let threw_str = threw.as_str().unwrap_or_default().to_string();
  println!("[G'3] exception propagated to JS: {:?}", threw_str);
  assert!(
    threw_str.contains("boom from"),
    "异常必须传播到最外层且保留原消息，实际：{threw_str}"
  );

  // 异常抛出后 runtime 仍可继续执行（等价官方'以 js_error 终止搜索，沙箱存活'）
  runtime.execute_script("g3b", "globalThis.__alive = 6 * 7;".to_string())?;
  let alive = eval_json(&mut runtime, "globalThis.__alive");
  assert_eq!(alive, serde_json::json!(42));
  println!("[G'3] runtime alive after exception: ok");

  // ---- G'4：回调开销（多次取中位）
  let mut ns_noarg = Vec::new();
  let mut ns_arg = Vec::new();
  for _ in 0..5 {
    runtime.execute_script("g4", "globalThis.__ns = Deno.core.ops.op_oc_callback_bench(function () {}, 10000);".to_string())?;
    ns_noarg.push(eval_json(&mut runtime, "globalThis.__ns").as_f64().unwrap_or(f64::NAN));
    runtime.execute_script("g4b", "globalThis.__ns2 = Deno.core.ops.op_oc_callback_bench(function (a) { return a; }, 10000);".to_string())?;
    ns_arg.push(eval_json(&mut runtime, "globalThis.__ns2").as_f64().unwrap_or(f64::NAN));
  }
  ns_noarg.sort_by(|a, b| a.partial_cmp(b).unwrap());
  ns_arg.sort_by(|a, b| a.partial_cmp(b).unwrap());
  println!(
    "[G'4] callback roundtrip ns/op median: no-arg={:.0}ns 1-arg={:.0}ns（64 房间满配搜索回调总成本 < 0.1ms 级）",
    ns_noarg[2],
    ns_arg[2]
  );

  println!(
    "\nRESULT engine=deno_core(op_callback) 三态回调=pass 异常传播=pass runtime存活=pass 回调中位={:.0}ns",
    ns_arg[2]
  );
  println!("CAPABILITY op_callback=pass detail='op 内回调进 JS（官方 roomCallback 模式）：#[scoped] Global<Function> + tc_scope! + Function::call；三态返回值/零拷贝矩阵/异常传播/runtime 存活全部通过'");
  Ok(())
}
