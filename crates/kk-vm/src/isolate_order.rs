//! 记录本线程宿主持有的 V8 实例顺序，供冷恢复检查玩家/Mod VM 的共同栈边界。
//! 这是生命周期账本，不替代 V8 Enter/Exit；实例构造成功后登记，V8 销毁后注销。
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    static STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}
static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct IsolateOrder {
    id: u64,
    // V8 的进入栈属于线程；账本令牌不能跨线程移动或共享。
    _thread: PhantomData<Rc<()>>,
}

impl IsolateOrder {
    /// 调用方须在 V8 实例构造成功后登记，并将令牌字段放在实例字段之后。
    pub fn register() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        STACK.with(|stack| stack.borrow_mut().push(id));
        Self {
            id,
            _thread: PhantomData,
        }
    }
    pub fn is_current(&self) -> bool {
        STACK.with(|stack| stack.borrow().last() == Some(&self.id))
    }
    pub fn immediately_above(&self, previous: &Self) -> bool {
        STACK.with(|stack| stack.borrow().ends_with(&[previous.id, self.id]))
    }
}

impl Drop for IsolateOrder {
    fn drop(&mut self) {
        STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            // 冷恢复可暂时退出替代 VM、回收相邻旧 VM 后重新进入；旧令牌在
            // 中间注销。实际 V8 Drop 次序仍由宿主执行，不在此账本伪装退出。
            if let Some(index) = stack.iter().position(|id| *id == self.id) {
                stack.remove(index);
            }
        });
    }
}
