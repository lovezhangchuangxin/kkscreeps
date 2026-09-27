//! build script：
//! 1. 从 Cargo.lock 解析精确依赖版本，注入编译期环境变量（供 main.rs 打印）。
//! 2. 用 deno_core::create_snapshot 生成 startup snapshot（预热全局 fib / SNAPSHOT_PRELOADED / 正则），
//!    写入 OUT_DIR/snapshot.bin。
//! 3. 另生成一份"游戏 runtime"snapshot（实验 J：类定义/prototype getter/闭包/冻结常量表），
//!    写入 OUT_DIR/snapshot-game.bin（examples/game_api.rs 使用）。

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

  for (env_name, pkg) in [
    ("SPIKE_VER_DENO_CORE", "deno_core"),
    ("SPIKE_VER_DENO_V8", "v8"), // 源包名 v8（= rusty_v8，crate 目录名带版本号）
    ("SPIKE_VER_SYSINFO", "sysinfo"),
    ("SPIKE_VER_DENO_ERROR", "deno_error"),
    ("SPIKE_VER_TOKIO", "tokio"),
  ] {
    match lockfile_version(pkg) {
      Some(v) => println!("cargo:rustc-env={env_name}={v}"),
      None => println!("cargo:warning=package {pkg} not found in Cargo.lock"),
    }
  }

  let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

  make_snapshot(out_dir.join("snapshot.bin"), SNAPSHOT_WARMUP_JS);
  make_snapshot(out_dir.join("snapshot-game.bin"), GAME_RUNTIME_WARMUP_JS);
}

/// 用 deno_core::snapshot::create_snapshot 生成一份 startup snapshot 并落盘。
fn make_snapshot(snapshot_path: PathBuf, warmup: &'static str) {
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
      rt.execute_script("<snapshot-warmup>", warmup.to_string()).unwrap();
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

const SNAPSHOT_WARMUP_JS: &str = r#"
globalThis.fib = function (n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); };
globalThis.SNAPSHOT_PRELOADED = true;
globalThis.WARM_REGEX = /cr[ae]ep-\d+/g;
"#;

/// 实验 J：模拟"游戏 runtime"进 snapshot（类定义 / prototype getter / 闭包 / 冻结常量表）。
/// 形态参考官方 engine/src/game/*：常量表 C 挂 globals、RoomPosition 类、闭包工厂。
const GAME_RUNTIME_WARMUP_JS: &str = r#"
// ---- 冻结常量表（模拟 driver.constants → globals.C）
globalThis.C = Object.freeze({
  OK: 0, ERR_NOT_OWNER: -1, ERR_NO_PATH: -2, ERR_NOT_FOUND: -5,
  MOVE: 'move', WORK: 'work', CARRY: 'carry', ATTACK: 'attack', HEAL: 'heal',
  COLOR_RED: 1, COLOR_WHITE: 10,
  TERRAIN_MASK_WALL: 1, TERRAIN_MASK_SWAMP: 2,
  CONSTRUCTION_COST: Object.freeze({ spawn: 15000, extension: 3000, road: 300, tower: 5000 }),
  BODIES: Object.freeze([Object.freeze({ move: 50, work: 100 }), Object.freeze({ move: 50, carry: 50 })]),
});

// ---- RoomPosition 类（prototype getter，模拟官方 globals.RoomPosition）
globalThis.RoomPosition = class RoomPosition {
  constructor(x, y, roomName) {
    this.__x = x | 0;
    this.__y = y | 0;
    this.__room = roomName;
  }
  get x() { return this.__x; }
  get y() { return this.__y; }
  get roomName() { return this.__room; }
  get xy() { return this.__x * 50 + this.__y; }
  isNearTo(target, range = 1) {
    return Math.abs(this.__x - target.x) <= range && Math.abs(this.__y - target.y) <= range;
  }
  inRangeTo(target, range) { return this.isNearTo(target, range); }
  toString() { return `[room ${this.__room} ${this.__x},${this.__y}]`; }
};

// ---- 闭包工厂（闭包持有 TypedArray，模拟官方 lodash 类工具/缓存函数）
globalThis.makeDistanceTable = function makeDistanceTable(max) {
  const table = new Float64Array(max + 1);
  for (let i = 0; i <= max; i++) table[i] = Math.sqrt(i);
  return function dist2(ax, ay, bx, by) {
    const dx = ax - bx, dy = ay - by;
    return table[dx * dx + dy * dy];
  };
};

// ---- 一段带正则与 Map 的注册表（模拟 runtime 常驻结构）
const __nameRe = /^[A-Za-z][A-Za-z0-9_-]{0,23}$/;
const __registry = new Map();
globalThis.GameRegistry = {
  register(name, obj) {
    if (!__nameRe.test(name)) throw new Error('invalid name: ' + name);
    __registry.set(name, obj);
    return __registry.size;
  },
  get(name) { return __registry.get(name); },
  get size() { return __registry.size; },
};

// 顶层 const（全局词法环境绑定）——顺带实测：这种绑定能否随 snapshot 恢复
const __SNAPSHOT_LEXICAL_TEST = 777;

globalThis.__GAME_RUNTIME_LOADED__ = true;
"#;
