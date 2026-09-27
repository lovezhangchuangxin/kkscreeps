// 阴性对照：heap_limits(0,64MB) 但【不注册】near_heap_limit 回调
// 预期：V8 FatalProcessOutOfMemory -> 进程 abort（文档声明的行为）
use deno_core::*;

fn main() {
  let tokio_rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
  let _g = tokio_rt.enter();
  let cp = v8::CreateParams::default().heap_limits(0, 64 * 1024 * 1024);
  let mut rt = JsRuntime::new(RuntimeOptions { create_params: Some(cp), ..Default::default() });
  eprintln!("negative-control: no near-heap-limit callback, infinite array growth...");
  let _ = rt.execute_script("m", r#"const a=[]; for(;;){ a.push(new Array(10000).fill(1.5)); }"#);
  eprintln!("UNEXPECTED: script returned");
}
