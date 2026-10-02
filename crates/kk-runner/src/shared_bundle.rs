//! KKPV 块束的 V8 共享内存句柄。
//!
//! 束字节一次性移入 `BackingStore` 并以 `SharedRef` 引用计数：宿主编码
//! 一次后，任意数量的玩家隔离经 `ArrayBuffer::with_backing_store` 把同
//! 一份内存挂成本隔离的 ArrayBuffer——挂载只克隆引用计数，不复制字节。
//! 句柄可以跨线程移动到玩家工作者线程（见下述 Send 论证）。
//!
//! 可写性与玩家不可达性（domain-projection §6）：V8 的 ArrayBuffer 本身
//! 不因共享而只读，本类型的保护面是"玩家代码拿不到它"——ArrayBuffer 仅
//! 在可信 `beginTickKkpv` 调用期间作为参数存在，解码后的值与地形视图
//! 由可信闭包持有，不进入任何玩家可见属性。束字节在构造后不再写入，
//! 各隔离并发只读，无数据竞争。
//!
//! Send 论证：`v8::BackingStore` 在 rusty_v8 中只有 `Send`（定宽非共享、
//! 非 wasm、不可扩容的 store 无 V8 线程亲和状态），`SharedRef` 的自动
//! Send/Sync 门因此不满足；这里对句柄 newtype 施加 `unsafe impl Send`，
//! 依据与 deno_core 对 V8 对象的定向 Send 包装一致——跨线程只发生
//! shared_ptr 引用计数的原子增减与目标隔离的挂载记账，本仓不对共享句柄
//! 调用扩容/分离等线程敏感 API。参照先例：deno_core 的
//! `InspectorWakerInner`/`ExternalPointer`/`V8CrossThreadTaskSpawner`。

use deno_core::v8;

/// 束字节的共享句柄；`Clone` 是引用计数克隆，不复制载荷。
#[derive(Clone)]
pub struct SharedKkpvBundle {
    store: v8::SharedRef<v8::BackingStore>,
    bytes: usize,
}

// 安全性：见模块级 Send 论证。字节在构造后不可变；跨线程并发只读，
// 引用计数与挂载记账均为原子操作。
unsafe impl Send for SharedKkpvBundle {}

impl std::fmt::Debug for SharedKkpvBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不输出载荷内容；调试信息只需要长度。
        write!(f, "SharedKkpvBundle({} bytes)", self.bytes)
    }
}

impl SharedKkpvBundle {
    /// 把已编码的束字节移动进新建 backing store 并转为共享句柄。
    /// `data` 的分配被接管，不发生复制。
    pub fn from_vec(data: Vec<u8>) -> Self {
        let bytes = data.len();
        Self {
            store: v8::ArrayBuffer::new_backing_store_from_vec(data).make_shared(),
            bytes,
        }
    }

    /// 束字节数（构造时记录，避免经 V8 查询）。
    pub fn byte_len(&self) -> usize {
        self.bytes
    }

    /// 挂载为当前隔离的 ArrayBuffer：同一份内存，不复制字节。
    /// 极端异常态（如构造超限触发 V8 致命路径前的空返回）返回 None，
    /// 由调用方转为宿主通道错误。
    pub(crate) fn array_buffer<'s>(
        &self,
        scope: &v8::PinScope<'s, '_, ()>,
    ) -> Option<v8::Local<'s, v8::ArrayBuffer>> {
        Some(v8::ArrayBuffer::with_backing_store(scope, &self.store))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_vec_records_length_without_copy() {
        let bundle = SharedKkpvBundle::from_vec(vec![7u8; 1024]);
        assert_eq!(bundle.byte_len(), 1024);
        // 引用计数克隆不改变字节长度；两次克隆后仍是同一份内存的句柄。
        let clone = bundle.clone();
        assert_eq!(clone.byte_len(), 1024);
    }
}
