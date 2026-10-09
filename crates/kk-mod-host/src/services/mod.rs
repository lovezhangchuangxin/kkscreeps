//! G27.a：后台线程/V8 的有界异步生命周期；不持有权威世界或玩家 VM。
pub const SERVICE_PROTOCOL: &str = "kk-services-v1";

mod file_probe;
pub use file_probe::FileProbe;
mod ops;
mod realm;
mod worker;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::thread::JoinHandle;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::{ContentSelection, ModHostError, ModHostResult, ModPackage, ModVmLimits};

/// 显式授予的只读探针。实现须可取消、有界，不能产生待耐久的玩法写入。
pub trait ProbeReader: Send + Sync {
    fn read(&self) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>>;
}

#[derive(Clone)]
pub struct ServiceOptions {
    pub selection: ContentSelection,
    pub configs: BTreeMap<String, Value>,
    /// 键以调用包的命名空间开头；没有登记的探针一律拒绝。
    pub probes: BTreeMap<String, Arc<dyn ProbeReader>>,
    pub vm_limits: ModVmLimits,
    pub queue_capacity: usize,
    pub max_value_bytes: usize,
    pub max_probes_per_call: usize,
}

impl Default for ServiceOptions {
    fn default() -> Self {
        Self {
            selection: ContentSelection::default(),
            configs: BTreeMap::new(),
            probes: BTreeMap::new(),
            vm_limits: ModVmLimits::default(),
            queue_capacity: 16,
            max_value_bytes: 1024 * 1024,
            max_probes_per_call: 8,
        }
    }
}

impl ServiceOptions {
    fn validate(&self) -> ModHostResult<()> {
        self.vm_limits.validate()?;
        if !(1..=1024).contains(&self.queue_capacity)
            || !(1..=8 * 1024 * 1024).contains(&self.max_value_bytes)
            || !(1..=64).contains(&self.max_probes_per_call)
        {
            return Err(error("后台队列/值字节/探针调用限额超出有效范围"));
        }
        Ok(())
    }
}

fn error(message: impl Into<String>) -> ModHostError {
    ModHostError::new("background services", message)
}

fn check_value(value: &Value, limit: usize) -> ModHostResult<()> {
    if serde_json::to_vec(value)
        .map_err(|e| error(e.to_string()))?
        .len()
        > limit
    {
        return Err(error("后台输入/输出超过字节限额"));
    }
    Ok(())
}

pub struct ServiceCall {
    reply: oneshot::Receiver<ModHostResult<Value>>,
}
impl ServiceCall {
    /// 等待后台结果；取消等待不撤销已排队的请求，不承诺崩溃后重放。
    pub async fn resolve(self) -> ModHostResult<Value> {
        self.reply.await.map_err(|_| error("后台域已结束"))?
    }
}

enum Command {
    Start,
    Request(String, Value),
    Stop,
}
struct Job {
    command: Command,
    view: Value,
    reply: oneshot::Sender<ModHostResult<Value>>,
}

/// 可跨线程的队列句柄；V8 自始至终只在 worker 所属线程使用。
pub struct BackgroundServices {
    sender: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
    services: Vec<String>,
    max_value_bytes: usize,
}

impl BackgroundServices {
    /// 包快照移入后台线程；启动 Promise 不阻塞调用者所属的异步执行器。
    pub async fn spawn(
        packages: BTreeMap<String, ModPackage>,
        options: ServiceOptions,
    ) -> ModHostResult<Self> {
        options.validate()?;
        let (sender, receiver) = mpsc::channel(options.queue_capacity);
        let (ready, loaded) = oneshot::channel();
        let max_value_bytes = options.max_value_bytes;
        let thread = std::thread::Builder::new()
            .name("kk-mod-services".into())
            .spawn(move || worker::run(packages, options, receiver, ready))
            .map_err(|e| error(format!("创建后台线程失败：{e}")))?;
        let services = match loaded.await {
            Ok(Ok(services)) => services,
            result => {
                let _ = thread.join();
                return Err(match result {
                    Ok(Err(error)) => error,
                    _ => error("后台装载线程异常退出"),
                });
            }
        };
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            services,
            max_value_bytes,
        })
    }

    pub fn services(&self) -> &[String] {
        &self.services
    }

    fn enqueue(&self, command: Command, view: Value) -> ModHostResult<ServiceCall> {
        check_value(&view, self.max_value_bytes)?;
        if let Command::Request(_, args) = &command {
            check_value(args, self.max_value_bytes)?;
        }
        let (reply, receiver) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or_else(|| error("后台域已关闭"))?
            .try_send(Job {
                command,
                view,
                reply,
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => error("后台请求队列已满"),
                mpsc::error::TrySendError::Closed(_) => error("后台域已关闭"),
            })?;
        Ok(ServiceCall { reply: receiver })
    }

    pub fn start(&self, committed_view: Value) -> ModHostResult<ServiceCall> {
        self.enqueue(Command::Start, committed_view)
    }

    pub fn request(
        &self,
        service: &str,
        args: Value,
        committed_view: Value,
    ) -> ModHostResult<ServiceCall> {
        if !self.services.iter().any(|key| key == service) {
            return Err(error(format!("未知或未选择的后台服务 {service}")));
        }
        self.enqueue(Command::Request(service.to_owned(), args), committed_view)
    }

    /// 显式 stop 完成后回收线程；丢弃句柄时关闭队列，worker 在有限预算下退场。
    pub async fn shutdown(mut self, committed_view: Value) -> ModHostResult<Value> {
        let call = self.enqueue(Command::Stop, committed_view)?;
        let result = call.resolve().await;
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| error("后台线程退场异常"))?;
        }
        result
    }
}

impl Drop for BackgroundServices {
    fn drop(&mut self) {
        // 不在玩法线程等待后台回调；断开发送端后 worker 有限退场并自行回收 V8。
        self.sender.take();
        self.thread.take();
    }
}
