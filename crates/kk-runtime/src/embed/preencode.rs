//! L1a 房间级编码-结算重叠的宿主侧服务（KKPV 通道）。
//!
//! 结算串行窗内，逐房结算回调把候选房快照发往专职编码线程池做共享块
//! 预编码（对候选只读，产物与发布后即时编码逐字节一致）。产物直接写
//! 入共享结果地图，宿主不阻塞等待：下一拍 [`kk_world::World`] 的共享
//! 块构建逐房从地图取用，命中且"源快照==当前世界房"（逐房深比较兜底
//! 跨房移动、市场等晚期变异）即免即时编码——project 相的共享块编码被
//! 隐藏进 settle 相。未命中的房走原内联路径，尾量至多重复编码一次；
//! 失败轮与残留产物由下一 Begin 清空。
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use kk_schema::kkpv::PrevEncodedBlock;
use kk_world::RoomState;
use kk_world::kkpv::{PreencodeMap, preencode_shared_room};

/// 一轮的预测元与脏位保留快照（worker 本地上下文）。
struct RoundCtx {
    tick: u64,
    generation: u64,
    plan_hash: [u8; 32],
    prev: HashMap<String, Arc<PrevEncodedBlock>>,
}

enum Msg {
    /// 开启一轮：预测的提交后 (tick, generation, plan) 与脏位保留快照；
    /// 宿主侧已清空共享地图，worker 保存本轮上下文。
    Begin {
        tick: u64,
        generation: u64,
        plan_hash: [u8; 32],
        prev: HashMap<String, Arc<PrevEncodedBlock>>,
    },
    Job {
        name: String,
        state: Arc<RoomState>,
    },
    Stop,
}

/// 编码线程数。单线程全量编码（50×6 实测约 140ms）放不进结算串行窗
/// （约 110ms）；两路各约一半，尾量由消费点内联兜底。部署在少核
/// 机器上时由 OS 调度自然摊开，不影响正确性。
const WORKERS: usize = 2;

/// 结算回调持有的轻量句柄（服务本体由 EmbedEngine 独占）。
pub(super) struct PreencodeHandle {
    /// 各 worker 的派发端；按原子轮转分发保证负载均衡。
    txs: Vec<mpsc::Sender<Msg>>,
    next: AtomicUsize,
    /// 结算窗标志：仅 Begin 与失败轮复位之间派发任务。
    armed: Arc<AtomicBool>,
}

impl PreencodeHandle {
    pub(super) fn job(&self, name: &str, state: &RoomState) {
        if !self.armed.load(Ordering::Relaxed) {
            return;
        }
        // 候选快照克隆（50×6 实测全批约 7ms，主线程可忽略）；编码线程只读。
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.txs.len();
        let _ = self.txs[idx].send(Msg::Job {
            name: name.to_string(),
            state: Arc::new(state.clone()),
        });
    }
}

pub(super) struct PreencodeService {
    txs: Vec<mpsc::Sender<Msg>>,
    armed: Arc<AtomicBool>,
    workers: Vec<std::thread::JoinHandle<()>>,
    map: Arc<PreencodeMap>,
}

impl PreencodeService {
    pub(super) fn spawn() -> (Self, PreencodeHandle) {
        let map = Arc::new(PreencodeMap::new(HashMap::new()));
        let armed = Arc::new(AtomicBool::new(false));
        let mut txs = Vec::with_capacity(WORKERS);
        let mut workers = Vec::with_capacity(WORKERS);
        for i in 0..WORKERS {
            let (tx, rx) = mpsc::channel::<Msg>();
            let map_worker = map.clone();
            let worker = std::thread::Builder::new()
                .name(format!("kk-preencode-{i}"))
                .spawn(move || {
                    let mut ctx: Option<RoundCtx> = None;
                    for msg in rx {
                        match msg {
                            Msg::Begin {
                                tick,
                                generation,
                                plan_hash,
                                prev,
                            } => {
                                ctx = Some(RoundCtx {
                                    tick,
                                    generation,
                                    plan_hash,
                                    prev,
                                });
                            }
                            Msg::Job { name, state } => {
                                let Some(ctx) = &ctx else {
                                    continue;
                                };
                                if let Some(entry) = preencode_shared_room(
                                    &name,
                                    &state,
                                    ctx.tick,
                                    ctx.generation,
                                    ctx.plan_hash,
                                    ctx.prev.get(&name).map(|arc| arc.as_ref()),
                                ) && let Ok(mut guard) = map_worker.lock()
                                {
                                    guard.insert(name, Arc::new(entry));
                                }
                            }
                            Msg::Stop => break,
                        }
                    }
                })
                .expect("预编码线程可创建");
            txs.push(tx);
            workers.push(worker);
        }
        let handle = PreencodeHandle {
            txs: txs.clone(),
            next: AtomicUsize::new(0),
            armed: armed.clone(),
        };
        (
            Self {
                txs,
                armed,
                workers,
                map,
            },
            handle,
        )
    }

    /// 共享结果地图句柄（挂接到世界）。
    pub(super) fn map(&self) -> Arc<PreencodeMap> {
        self.map.clone()
    }

    /// 结算开始前开启一轮：清空上一轮残留产物，下发预测元与脏位保留
    /// 快照（各 worker 持全量快照，按房取用）。
    pub(super) fn begin_round(
        &self,
        tick: u64,
        generation: u64,
        plan_hash: [u8; 32],
        prev: HashMap<String, Arc<PrevEncodedBlock>>,
    ) {
        self.armed.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.map.lock() {
            guard.clear();
        }
        for tx in &self.txs {
            let _ = tx.send(Msg::Begin {
                tick,
                generation,
                plan_hash,
                prev: prev.clone(),
            });
        }
    }

    /// 失败轮弃用：停止派发即可——残留产物无人消费（下一拍项目走内联
    /// 路径），下一 Begin 清空地图。
    pub(super) fn abort(&self) {
        self.armed.store(false, Ordering::Relaxed);
    }
}

impl Drop for PreencodeService {
    fn drop(&mut self) {
        self.armed.store(false, Ordering::Relaxed);
        for tx in &self.txs {
            let _ = tx.send(Msg::Stop);
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
