//! 从 V8 接收拥有值的统一转换边界；调用方负责故障留存与预算。

use deno_core::v8;
use serde::de::DeserializeOwned;

pub(crate) fn from_v8<'s, 'i, T: DeserializeOwned>(
    scope: &mut v8::PinScope<'s, 'i>,
    value: v8::Local<'s, v8::Value>,
) -> Result<T, deno_core::serde_v8::Error> {
    // 固定版本 serde_v8 对 Symbol 等未知 ValueType 会 panic，而非返回 Err。
    // 这里只围住同步转换，期间没有候选/OpState 借用；退出后由调用方按入口语义
    // 处理错误。不格式化 panic 载荷，不把 unwind 带过 V8 op 边界。
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        deno_core::serde_v8::from_v8(scope, value)
    }))
    .unwrap_or(Err(deno_core::serde_v8::Error::UnsupportedType))
}
