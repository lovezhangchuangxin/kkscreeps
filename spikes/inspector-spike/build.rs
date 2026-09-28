//! build script：
//! 1. 从 Cargo.lock 解析精确依赖版本，注入编译期环境变量（供 main.rs 打印）。
//! 2. 用 deno_core::snapshot::create_snapshot 生成 startup snapshot
//!    （预热全局 __snapFib / SNAPSHOT_PRELOADED，实验 P 用），
//!    写入 OUT_DIR/snapshot-inspector.bin。

use std::path::PathBuf;

fn lockfile_version(name: &str) -> Option<String> {
  let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
  let lock_path = manifest_dir.join("Cargo.lock");
  let text = std::fs::read_to_string(&lock_path).ok()?;
  let mut in_package = false;
  for line in text.lines() {
    let line = line.trim();
    if line.starts_with("[[package]]") {
      in_package = false;
    } else if let Some(rest) = line.strip_prefix("name = ") {
      let rest = rest.trim_matches('"');
      in_package = rest == name;
    } else if in_package {
      if let Some(v) = line.strip_prefix("version = ") {
        return Some(v.trim_matches('"').to_string());
      }
    }
  }
  None
}

fn main() {
  println!("cargo:rerun-if-changed=build.rs");
  println!("cargo:rerun-if-changed=src/main.rs");
  // 版本号来自 Cargo.lock 解析，lock 变更须触发重编（否则打印的版本串可过期）
  println!("cargo:rerun-if-changed=Cargo.lock");

  for (env_name, pkg) in [
    ("SPIKE_VER_DENO_CORE", "deno_core"),
    ("SPIKE_VER_DENO_V8", "v8"),
    ("SPIKE_VER_DENO_INSPECTOR_SERVER", "deno_inspector_server"),
    ("SPIKE_VER_TUNGSTENITE", "tungstenite"),
    ("SPIKE_VER_UREQ", "ureq"),
    ("SPIKE_VER_SERDE_JSON", "serde_json"),
  ] {
    match lockfile_version(pkg) {
      Some(v) => println!("cargo:rustc-env={env_name}={v}"),
      None => println!("cargo:warning=package {pkg} not found in Cargo.lock"),
    }
  }

  let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
  make_snapshot(out_dir.join("snapshot-inspector.bin"));
}

/// 生成 startup snapshot 并落盘（deno-spike build.rs 同款模式）。
fn make_snapshot(snapshot_path: PathBuf) {
  let options = deno_core::snapshot::CreateSnapshotOptions {
    cargo_manifest_dir: Box::leak(
      std::env::var("CARGO_MANIFEST_DIR")
        .unwrap()
        .into_boxed_str(),
    ) as &'static str,
    startup_snapshot: None,
    skip_op_registration: false,
    extensions: vec![],
    extension_transpiler: None,
    with_runtime_cb: Some(Box::new(move |rt: &mut deno_core::JsRuntimeForSnapshot| {
      rt.execute_script("<snapshot-warmup>", SNAPSHOT_WARMUP_JS.to_string())
        .unwrap();
    })),
  };

  let output = deno_core::snapshot::create_snapshot(options, None).expect("create_snapshot failed");
  std::fs::write(&snapshot_path, &output.output).expect("write snapshot failed");
  println!(
    "cargo:warning=snapshot written: {} -> {} bytes",
    snapshot_path.display(),
    output.output.len()
  );
}

/// 实验 P 的快照预热代码：一个会被断点脚本调用的全局函数 + 常驻结构 + 标志位。
const SNAPSHOT_WARMUP_JS: &str = r#"
globalThis.__snapFib = function (n) { return n < 2 ? n : __snapFib(n - 1) + __snapFib(n - 2); };
globalThis.SNAPSHOT_PRELOADED = true;
globalThis.WARM_MAP = new Map([['a', 1], ['b', 2]]);
"#;
