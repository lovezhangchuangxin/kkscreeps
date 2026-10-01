//! 编译上游寻路算法和本项目的 C ABI、Nan 兼容层。
//! 使用 C++20，并保留异常支持：算法以异常传递回调错误和地形缺失。
//! 上游源码版本见 src/lib.rs 的来源说明，pf.cc/pf.h 保留原文。

fn main() {
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++20")
        .include("native/compat") // 让 pf.h 的 <nan.h> 使用本项目兼容层。
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

    // Apple 链接 libc++；MSVC 工具链自行链接；其他目标使用 libstdc++。
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if !target.contains("msvc") {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
