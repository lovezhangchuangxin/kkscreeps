//! 编译官方 C++ 寻路源码（ADR-004/overall.md：FFI 复用官方源码保证路径逐点一致）。
//!
//! - `native/pf.cc` / `native/pf.h`：官方 `driver/native/src/` 的**原样拷贝**
//!   （sha256 与源仓一致，未做任何改动）；
//! - `native/compat/nan.h`：自有最小 Nan/v8 兼容层，让官方源码零改动编译；
//! - `native/kk_ffi.cc` / `native/kk_ffi.h`：自有 C ABI 包装（Rust 绑定见 src/ffi.rs）。
//!
//! C++ 标准与官方构建配置对齐：binding.gyp 使用 `-std=c++20` 且显式启用异常
//! （`cflags_cc!` 移除 -fno-exceptions），本构建同样需要异常（pf.cc 依赖
//! throw/catch，pf.cc:34-35,70,303,563）。

fn main() {
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++20")
        .include("native/compat") // 官方 pf.h 的 #include <nan.h> 在此解析到兼容层
        .file("native/kk_ffi.cc")
        .file("native/pf.cc")
        .compile("kk_pathfinder_native");

    for path in [
        "native/pf.cc",
        "native/pf.h",
        "native/kk_ffi.cc",
        "native/kk_ffi.h",
        "native/compat/nan.h",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    // C++ 标准库链接：apple 平台是 libc++（-lc++），msvc 由链接器自动处理，
    // 其余（Linux 等）是 libstdc++。CI 合入 Linux 时按此分支生效。
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if !target.contains("msvc") {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
