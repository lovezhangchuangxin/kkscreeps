//! 为每个玩家创建专属线程，在该线程内创建、执行和销毁 V8 沙箱。
//!
//! JsRuntime 不能在线程之间移动；命令通过通道串行处理，每条命令有独立回执。
//! tick_all 先派发所有玩家再按注册顺序收集结果，各玩家可以并行执行。
//!
//! V8 实例的逆序销毁约束只作用于同一线程。这里每个玩家线程只有一个实例，
//! 因此可以回收任意空闲玩家；关闭或回收线程时发送 Shutdown 并等待 join。

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::TickPlan;
use crate::modules::ModuleTable;
use crate::ops::LogLine;
use crate::sandbox::{Sandbox, SandboxConfig, TickOutcome};

/// 一次玩家 tick 的完整报告（bot-check 与诊断用）。
#[derive(Debug)]
pub struct TickReport {
    pub epoch: RunEpoch,
    pub outcome: TickOutcome,
    pub plan: TickPlan,
    /// 从世界注入到入口结束的线程 CPU 用量，不含随后写回和指令费用；中断时为 0。
    pub used_ms: f64,
    /// 入口或写回捕获的普通玩家错误；未捕获普通错误时为 None。
    pub player_error: Option<String>,
    pub console: Vec<LogLine>,
    pub missing_api: Value,
    pub stub_calls: Value,
    pub bucket_after: f64,
    /// endTick 带出的本 tick intentsList（Null = 无/硬中断丢弃）——embed
    /// 结算通道消费（IntentBatch::from_json 清洗后进引擎）。
    pub intents: Value,
}

/// 一次"玩家代码求值 tick"的报告（screeps-ok runPlayer 通道）。
#[derive(Debug)]
pub struct EvalReport {
    /// runExpr 的结果 JSON：`{ok, value?} | {ok:false, kind, error}`——注入
    /// 失败（玩家把环境搞坏）为 `{ok:false, kind:"inject_failed"}`（玩家级）。
    pub expr: Value,
    pub console: Vec<LogLine>,
    pub bucket_after: f64,
    /// endTick 带出的本 tick intentsList（Null = 无/硬中断丢弃）。
    pub intents: Value,
    /// 待提交 CPU 费用（毫秒）。RunJob 由宿主结算；旧 RunPlayer 通道使用 settle_bill。
    /// 停跑与主动 halt 为 None，注入失败或硬中断携带完整预算。
    /// bucket_after 是提交前余额，与已经即时结算的 TickReport 不同。
    pub cpu_billed_ms: Option<f64>,
    /// RawMemory 候选（写回成功时的 rawMemoryData 快照；None = 写回失败/未达）。
    pub raw_memory: Option<String>,
    /// InterShardMemory 本地段候选；仅 setLocal 触发的拍为 Some。
    pub ism_local: Option<String>,
    /// 待提交的 segment 内容写入及四类请求；写回失败或未写回时为 None。
    pub segments_out: Option<Value>,
    /// 原样返回命令携带的轮次标识；调用方负责核对是否属于本轮。
    pub epoch: RunEpoch,
}

pub use kk_core::RunEpoch;

/// KKPV 私有束的派发形态。
/// [`KkpvPrivateBundle::Owned`]：逐查看者装配的字节，工作者侧一次性移入
/// backing store（此前的唯一形态）。
/// [`KkpvPrivateBundle::Shared`]：全视野查看者共享束——束字节跨人一致，
/// 每代际装配一次并移入 backing store 后，逐玩家只克隆引用计数（与公开
/// 束同机制）。两种形态对工作者完全同形，挂载与解码路径不分叉。
#[derive(Debug)]
pub enum KkpvPrivateBundle {
    Owned(Vec<u8>),
    Shared(crate::shared_bundle::SharedKkpvBundle),
}

/// 同一条命令携带已提交内存、世界、宿主预算与代码，返回本轮候选。
#[derive(Debug)]
pub struct RunJob {
    /// 可信宿主的不可变寻路快照，不经过玩家输入或持久化编码。
    pub terrain: Option<std::sync::Arc<kk_pathfinder::TerrainSet>>,
    pub world: Value,
    /// KKPV 通道的私有对象束；Some 时 world 是不含对象数组与地形的
    /// 世界信封。进入工作者后以共享句柄挂载，不再复制。
    pub kkpv_bundle: Option<KkpvPrivateBundle>,
    /// KKPV 通道的公开地形束共享句柄；全部玩家同一代际共用同一份内存，
    /// 派发时克隆引用计数，工作者零拷贝挂载。与 `kkpv_bundle` 同 Some
    /// 或同 None。
    pub kkpv_public: Option<crate::shared_bundle::SharedKkpvBundle>,
    pub code: String,
    pub epoch: RunEpoch,
    pub attempt: u64,
    /// 整条命令的绝对期限；None 在派发时绑定 tickLimit 加 1 秒通信余量。
    pub deadline: Option<Instant>,
    pub raw_memory: String,
    pub segments_json: String,
    pub foreign_json: String,
    /// InterShardMemory 本地段注入（JSON 串，"null"=未设置）。
    pub ism_json: String,
    pub budget: kk_core::cpu::RunBudget,
}

/// 回执身份包含世界轮次、执行尝试和工作者实例，重建线程后不可复用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobToken {
    pub epoch: RunEpoch,
    pub attempt: u64,
    pub worker: u64,
}

#[derive(Debug)]
pub struct RunReceipt {
    pub token: JobToken,
    pub report: EvalReport,
    pub finished_at: Instant,
}

/// 已派发命令的独立回执通道；可按任意顺序等待，不混用其他命令的结果。
pub struct PendingJob {
    pub token: JobToken,
    rx: mpsc::Receiver<anyhow::Result<RunReceipt>>,
    pub deadline: Instant,
}

impl PendingJob {
    fn validate(&self, receipt: anyhow::Result<RunReceipt>) -> anyhow::Result<RunReceipt> {
        let receipt = receipt?;
        if receipt.finished_at > self.deadline {
            anyhow::bail!("玩家执行回执超过命令期限");
        }
        Ok(receipt)
    }
    pub fn wait(self) -> anyhow::Result<RunReceipt> {
        let receipt = self
            .rx
            .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
            .map_err(|e| anyhow::anyhow!("玩家工作者 {} 未返回执行回执：{e}", self.token.worker))?;
        self.validate(receipt)
    }
    pub fn try_take(&self) -> anyhow::Result<Option<RunReceipt>> {
        match self.rx.try_recv() {
            Ok(receipt) => self.validate(receipt).map(Some),
            Err(mpsc::TryRecvError::Disconnected) => {
                anyhow::bail!("玩家工作者 {} 未返回执行回执", self.token.worker)
            }
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= self.deadline => {
                anyhow::bail!("玩家执行回执超过命令期限")
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
        }
    }
}

/// 测试时在任务开始前阻塞工作者，观察实际派发上限；释放后才进入计时区间。
pub struct JobGate {
    pub started: mpsc::Receiver<()>,
    pub release: mpsc::Sender<()>,
}

enum Cmd {
    RunJob {
        /// 装箱平抑变体尺寸差（RunJob 携带世界/束/内存等大载荷）。
        job: Box<RunJob>,
        ack: mpsc::Sender<anyhow::Result<RunReceipt>>,
        notify: Option<mpsc::Sender<JobToken>>,
    },
    ExitBeforeJob(mpsc::Sender<()>),
    GateNextJob(mpsc::Receiver<()>, mpsc::Sender<()>, mpsc::Sender<()>),
    SetModules(ModuleTable, mpsc::Sender<anyhow::Result<()>>),
    /// 注入 RawMemory 初值（tick 边界——协调层提交点统一驱动的通道面）。
    SetRawMemory(String, mpsc::Sender<anyhow::Result<()>>),
    /// 注入 RawMemory、活动 segment 窗口及 foreignSegment，供下一次 begin_tick 使用。
    SetMemoryState(
        String,
        String,
        String,
        String,
        mpsc::Sender<anyhow::Result<()>>,
    ),
    /// 提交计费候选（统一提交点的成功臂）。
    SettleBill(mpsc::Sender<anyhow::Result<()>>),
    /// 丢弃计费候选（统一提交点的失败/回退臂）。
    DiscardBill(mpsc::Sender<anyhow::Result<()>>),
    /// 通过命令读取沙箱线程内的当前桶余额。
    Bucket(mpsc::Sender<anyhow::Result<f64>>),
    Tick {
        world: Value,
        /// KKPV 通道的私有对象束；Some 时 world 是不含对象数组与地形的
        /// 世界信封。
        bundle: Option<Vec<u8>>,
        /// KKPV 通道的公开地形束共享句柄；与 `bundle` 同 Some 或同 None。
        public: Option<crate::shared_bundle::SharedKkpvBundle>,
        epoch: RunEpoch,
        ack: mpsc::Sender<anyhow::Result<TickReport>>,
    },
    RunExpr {
        code: String,
        ack: mpsc::Sender<anyhow::Result<Value>>,
    },
    /// 在同一条线程命令中执行 begin_tick → run_expr → end_tick。
    /// 只生成计费与 Memory 候选，世界提交和轮次检查由调用方负责。
    RunPlayer {
        world: Value,
        /// KKPV 通道的私有对象束；Some 时 world 是不含对象数组与地形的
        /// 世界信封。
        bundle: Option<Vec<u8>>,
        /// KKPV 通道的公开地形束共享句柄；与 `bundle` 同 Some 或同 None。
        public: Option<crate::shared_bundle::SharedKkpvBundle>,
        code: String,
        epoch: RunEpoch,
        ack: mpsc::Sender<anyhow::Result<EvalReport>>,
    },
    Shutdown,
}

pub struct PlayerThread {
    pub id: u64,
    tx: mpsc::Sender<Cmd>,
    join: Option<JoinHandle<()>>,
    last_command: Instant,
}

impl PlayerThread {
    fn send(&mut self, cmd: Cmd) {
        self.last_command = Instant::now();
        let _ = self.tx.send(cmd);
    }
}

/// 管理玩家专属线程；同一玩家的命令串行执行，不使用工作窃取。
pub struct PlayerThreads {
    players: Vec<PlayerThread>,
}

impl PlayerThreads {
    pub fn new() -> Self {
        Self {
            players: Vec::new(),
        }
    }

    /// 启动一个玩家线程并在其上创建沙箱。等待创建回执，保证注册顺序 = 创建顺序。
    pub fn spawn_player(&mut self, id: u64, config: SandboxConfig) -> anyhow::Result<()> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<()>>();
        let join = std::thread::Builder::new()
            .name(format!("kk-player-{id}"))
            .spawn(move || {
                let mut sandbox = match Sandbox::new(config) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                if ready_tx.send(Ok(())).is_err() {
                    return; // registry 已不在，直接退出
                }
                let mut exit_before_job = false;
                let mut gate: Option<(mpsc::Receiver<()>, mpsc::Sender<()>)> = None;
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Cmd::SetModules(table, ack) => {
                            let _ = ack.send(sandbox.set_modules(table));
                        }
                        Cmd::SetRawMemory(data, ack) => {
                            let _ = ack.send(sandbox.set_raw_memory(&data));
                        }
                        Cmd::SetMemoryState(raw, segs, foreign, ism, ack) => {
                            let _ = ack.send(sandbox.set_memory_state(&raw, &segs, &foreign, &ism));
                        }
                        Cmd::SettleBill(ack) => {
                            sandbox.settle_bill();
                            let _ = ack.send(Ok(()));
                        }
                        Cmd::DiscardBill(ack) => {
                            sandbox.discard_bill();
                            let _ = ack.send(Ok(()));
                        }
                        Cmd::Bucket(ack) => {
                            let _ = ack.send(Ok(sandbox.bucket()));
                        }
                        Cmd::Tick {
                            world,
                            bundle,
                            public,
                            epoch,
                            ack,
                        } => {
                            let _ = ack.send(run_one_tick(
                                &mut sandbox,
                                &world,
                                bundle.map(KkpvPrivateBundle::Owned),
                                public,
                                epoch,
                            ));
                        }
                        Cmd::RunExpr { code, ack } => {
                            // 独立表达式也可能死循环，必须使用带看门狗的执行入口，保证线程能继续服务。
                            let _ = ack.send(sandbox.run_expr_guarded(&code));
                        }
                        Cmd::RunPlayer {
                            world,
                            bundle,
                            public,
                            code,
                            epoch,
                            ack,
                        } => {
                            let _ = ack.send(run_eval_tick(
                                &mut sandbox,
                                &world,
                                bundle.map(KkpvPrivateBundle::Owned),
                                public,
                                &code,
                                epoch,
                            ));
                        }
                        Cmd::GateNextJob(release, started, ack) => {
                            gate = Some((release, started));
                            let _ = ack.send(());
                        }
                        Cmd::ExitBeforeJob(ack) => {
                            exit_before_job = true;
                            let _ = ack.send(());
                        }
                        Cmd::RunJob { job, ack, notify } => {
                            let token = JobToken {
                                epoch: job.epoch,
                                attempt: job.attempt,
                                worker: id,
                            };
                            if exit_before_job {
                                drop(ack);
                                if let Some(notify) = notify {
                                    let _ = notify.send(token);
                                }
                                break;
                            }
                            if let Some((release, started)) = gate.take() {
                                let _ = started.send(());
                                let _ = release.recv();
                            }
                            let deadline = job.deadline.expect("派发时绑定命令期限");
                            let result = sandbox.set_command_deadline(deadline).and_then(|()| {
                                sandbox
                                    .set_memory_state(
                                        &job.raw_memory,
                                        &job.segments_json,
                                        &job.foreign_json,
                                        &job.ism_json,
                                    )
                                    .and_then(|()| {
                                        sandbox.set_host_terrain(job.terrain.clone());
                                        run_eval_tick_with_budget(
                                            &mut sandbox,
                                            &job.world,
                                            job.kkpv_bundle,
                                            job.kkpv_public,
                                            &job.code,
                                            job.epoch,
                                            Some(&job.budget),
                                        )
                                    })
                            });
                            sandbox.clear_command_deadline();
                            // 用量已在回执中；宿主发布或丢弃候选均不再通知线程结算。
                            sandbox.discard_bill();
                            let _ = ack.send(result.map(|report| RunReceipt {
                                token,
                                report,
                                finished_at: Instant::now(),
                            }));
                            if let Some(notify) = notify {
                                let _ = notify.send(token);
                            }
                        }
                        Cmd::Shutdown => break,
                    }
                }
                // 通道断开或 Shutdown：sandbox 在此 drop（一玩家一线程恰一个
                // isolate，线程内 LIFO 天然满足；跨线程销毁顺序无要求）
            })?;
        ready_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家线程未能启动"))??;
        self.players.push(PlayerThread {
            id,
            tx,
            join: Some(join),
            last_command: Instant::now(),
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    pub fn set_modules(&mut self, id: u64, table: ModuleTable) -> anyhow::Result<()> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::SetModules(table, ack_tx));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 注入 RawMemory 初值；须在下一次 begin_tick 前调用。
    pub fn set_raw_memory(&mut self, id: u64, data: &str) -> anyhow::Result<()> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::SetRawMemory(data.to_string(), ack_tx));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 在 tick 边界注入 RawMemory、活动 segment 窗口和 foreignSegment。
    pub fn set_memory_state(
        &mut self,
        id: u64,
        raw: &str,
        segments_json: &str,
        foreign_json: &str,
        ism_json: &str,
    ) -> anyhow::Result<()> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::SetMemoryState(
            raw.to_string(),
            segments_json.to_string(),
            foreign_json.to_string(),
            ism_json.to_string(),
            ack_tx,
        ));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 在世界结算成功后提交玩家的计费或停跑回充候选。
    pub fn settle_bill(&mut self, id: u64) -> anyhow::Result<()> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::SettleBill(ack_tx));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 丢弃玩家的计费候选（世界结算失败回退——玩家桶不扣）。
    pub fn discard_bill(&mut self, id: u64) -> anyhow::Result<()> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::DiscardBill(ack_tx));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 读玩家桶当前值（测试断言面——桶只活在沙箱线程内，经通道读出）。
    #[doc(hidden)]
    pub fn bucket(&mut self, id: u64) -> anyhow::Result<f64> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::Bucket(ack_tx));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 对一个玩家跑完整一 tick（begin→loop→end），等待报告。
    pub fn tick(&mut self, id: u64, world: &Value) -> anyhow::Result<TickReport> {
        self.tick_with_bundle(id, world, None, None)
    }

    /// [`PlayerThreads::tick`] 的 KKPV 通道形态：`public` 是跨玩家共享的
    /// 公开地形束句柄，`bundle` 为本玩家私有对象束字节，`world` 为不含
    /// 对象数组与地形的世界信封；epoch 与 JSON 通道同规则从
    /// world.time 推得。
    pub fn tick_kkpv(
        &mut self,
        id: u64,
        world: &Value,
        public: &crate::shared_bundle::SharedKkpvBundle,
        bundle: &[u8],
    ) -> anyhow::Result<TickReport> {
        self.tick_with_bundle(id, world, Some(bundle.to_vec()), Some(public.clone()))
    }

    fn tick_with_bundle(
        &mut self,
        id: u64,
        world: &Value,
        bundle: Option<Vec<u8>>,
        public: Option<crate::shared_bundle::SharedKkpvBundle>,
    ) -> anyhow::Result<TickReport> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::Tick {
            world: world.clone(),
            bundle,
            public,
            epoch: RunEpoch {
                tick: world["time"].as_u64().unwrap_or(0),
                generation: world["generation"].as_u64().unwrap_or(0),
            },
            ack: ack_tx,
        });
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 先按注册顺序向全部玩家派发 tick，再按相同顺序等待报告。
    /// 各玩家在专属线程上可并行执行；结果顺序不代表执行完成顺序。
    pub fn tick_all(
        &mut self,
        world: &Value,
    ) -> anyhow::Result<Vec<(u64, anyhow::Result<TickReport>)>> {
        let mut acks = Vec::with_capacity(self.players.len());
        for p in &mut self.players {
            let (ack_tx, ack_rx) = mpsc::channel();
            let id = p.id;
            p.send(Cmd::Tick {
                world: world.clone(),
                bundle: None,
                public: None,
                epoch: RunEpoch {
                    tick: world["time"].as_u64().unwrap_or(0),
                    generation: world["generation"].as_u64().unwrap_or(0),
                },
                ack: ack_tx,
            });
            acks.push((id, ack_rx));
        }
        let mut out = Vec::with_capacity(acks.len());
        for (id, ack_rx) in acks {
            let r = ack_rx
                .recv()
                .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?;
            out.push((id, r));
        }
        Ok(out)
    }

    pub fn run_expr(&mut self, id: u64, code: &str) -> anyhow::Result<Value> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::RunExpr {
            code: code.to_string(),
            ack: ack_tx,
        });
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 在一条命令中执行世界注入、表达式求值和 Memory 写回，返回候选报告。
    /// epoch 原样带回，由调用方核对；此方法不提交世界或桶余额。
    pub fn run_player(
        &mut self,
        id: u64,
        world: &Value,
        code: &str,
        epoch: RunEpoch,
    ) -> anyhow::Result<EvalReport> {
        self.run_player_with_bundle(id, world, None, None, code, epoch)
    }

    /// [`PlayerThreads::run_player`] 的 KKPV 通道形态：`public` 是跨玩家
    /// 共享的公开地形束句柄，`bundle` 为本玩家私有对象束字节，`world`
    /// 为不含对象数组与地形的世界信封。
    pub fn run_player_kkpv(
        &mut self,
        id: u64,
        world: &Value,
        public: &crate::shared_bundle::SharedKkpvBundle,
        bundle: &[u8],
        code: &str,
        epoch: RunEpoch,
    ) -> anyhow::Result<EvalReport> {
        self.run_player_with_bundle(
            id,
            world,
            Some(bundle.to_vec()),
            Some(public.clone()),
            code,
            epoch,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_player_with_bundle(
        &mut self,
        id: u64,
        world: &Value,
        bundle: Option<Vec<u8>>,
        public: Option<crate::shared_bundle::SharedKkpvBundle>,
        code: &str,
        epoch: RunEpoch,
    ) -> anyhow::Result<EvalReport> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::RunPlayer {
            world: world.clone(),
            bundle,
            public,
            code: code.to_string(),
            epoch,
            ack: ack_tx,
        });
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 合并内存注入与执行；只报告用量，不提交线程内的桶余额。
    pub fn run_job(&mut self, id: u64, job: RunJob) -> anyhow::Result<RunReceipt> {
        self.dispatch_job(id, job)?.wait()
    }

    /// 派发后返回独立回执句柄；调用方在采纳结果前核对完整 token。
    pub fn dispatch_job(&mut self, id: u64, job: RunJob) -> anyhow::Result<PendingJob> {
        self.dispatch_job_notifying(id, job, None)
    }
    pub fn dispatch_job_notifying(
        &mut self,
        id: u64,
        mut job: RunJob,
        notify: Option<mpsc::Sender<JobToken>>,
    ) -> anyhow::Result<PendingJob> {
        let deadline = job.deadline.unwrap_or_else(|| {
            Instant::now()
                + Duration::from_millis(job.budget.bucket().tick_limit().max(1.0) as u64)
                + Duration::from_secs(1)
        });
        job.deadline = Some(deadline);
        let token = JobToken {
            epoch: job.epoch,
            attempt: job.attempt,
            worker: id,
        };
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::RunJob {
            job: Box::new(job),
            ack: ack_tx,
            notify,
        });
        Ok(PendingJob {
            token,
            rx: ack_rx,
            deadline,
        })
    }

    #[doc(hidden)]
    pub fn gate_next_job_for_test(&mut self, id: u64) -> anyhow::Result<JobGate> {
        let (release_tx, release_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let (ack_tx, ack_rx) = mpsc::channel();
        self.player_mut(id)?
            .send(Cmd::GateNextJob(release_rx, started_tx, ack_tx));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家线程已退出"))?;
        Ok(JobGate {
            started: started_rx,
            release: release_tx,
        })
    }

    /// 在下一条 RunJob 执行前退出真实玩家线程，模拟无回执失联。
    #[doc(hidden)]
    pub fn exit_before_next_job_for_test(&mut self, id: u64) -> anyhow::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.player_mut(id)?.send(Cmd::ExitBeforeJob(tx));
        rx.recv().map_err(|_| anyhow::anyhow!("工作者 {id} 已退出"))
    }

    /// 回收距离最后一次命令发送已达到 idle 的玩家，按注册顺序返回 id。
    /// 先用同一个 now 确定候选，再逐个发送 Shutdown 并 join，
    /// 避免较慢的销毁过程使其他玩家在本次扫描中额外达到空闲阈值。
    pub fn evict_idle(&mut self, idle: Duration) -> Vec<u64> {
        let now = Instant::now();
        let candidates: Vec<u64> = self
            .players
            .iter()
            .filter(|p| now.saturating_duration_since(p.last_command) >= idle)
            .map(|p| p.id)
            .collect();
        let mut evicted = Vec::new();
        for id in candidates {
            let Some(pos) = self.players.iter().position(|p| p.id == id) else {
                continue;
            };
            let mut p = self.players.remove(pos);
            let _ = p.tx.send(Cmd::Shutdown);
            if let Some(j) = p.join.take() {
                let _ = j.join();
            }
            evicted.push(p.id);
        }
        evicted
    }

    /// 全部关闭：按创建逆序（LIFO）逐个 shutdown + join。
    pub fn shutdown_all(&mut self) {
        while let Some(mut p) = self.players.pop() {
            let _ = p.tx.send(Cmd::Shutdown);
            if let Some(j) = p.join.take() {
                let _ = j.join();
            }
        }
    }

    fn player_mut(&mut self, id: u64) -> anyhow::Result<&mut PlayerThread> {
        self.players
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("玩家 {id} 不存在"))
    }
}

impl Default for PlayerThreads {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PlayerThreads {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

/// 线程命令的注入通道输入。KKPV 臂的两个束都以共享句柄进入工作者：
/// 公开束与全视野共享私有束是引用计数克隆，逐查看者私有束把命令携带的
/// 字节一次性移入 backing store——此后挂载都不再复制字节。
enum ChannelInput {
    Json,
    Kkpv {
        public: crate::shared_bundle::SharedKkpvBundle,
        private: crate::shared_bundle::SharedKkpvBundle,
    },
}

fn channel_input(
    bundle: Option<KkpvPrivateBundle>,
    public: Option<crate::shared_bundle::SharedKkpvBundle>,
) -> anyhow::Result<ChannelInput> {
    match (bundle, public) {
        (Some(private), Some(public)) => {
            let private = match private {
                KkpvPrivateBundle::Owned(bytes) => {
                    crate::shared_bundle::SharedKkpvBundle::from_vec(bytes)
                }
                KkpvPrivateBundle::Shared(handle) => handle,
            };
            Ok(ChannelInput::Kkpv { public, private })
        }
        (None, None) => Ok(ChannelInput::Json),
        // 协调器必须成对派发两个束；单侧出现说明装配缺陷，按基础设施
        // 故障上抛而不是静默降级到 JSON 通道。
        (Some(_), None) | (None, Some(_)) => {
            anyhow::bail!("KKPV 通道的公开束与私有束须成对派发（收到单侧 Some）")
        }
    }
}

fn run_one_tick(
    sandbox: &mut Sandbox,
    world: &Value,
    bundle: Option<KkpvPrivateBundle>,
    public: Option<crate::shared_bundle::SharedKkpvBundle>,
    epoch: RunEpoch,
) -> anyhow::Result<TickReport> {
    // 此 tick 通道即时结算；注入失败也提交中断处理产生的完整预算费用。
    let begin = match channel_input(bundle, public)? {
        ChannelInput::Kkpv { public, private } => sandbox.begin_tick_kkpv(world, &public, &private),
        ChannelInput::Json => sandbox.begin_tick(world),
    };
    let plan = match begin {
        Ok(plan) => plan,
        Err(e) => {
            sandbox.settle_bill();
            return Err(e);
        }
    };
    match plan {
        // 负余额时不执行代码，但仍提交回充。plan 用于区分停跑与执行后中断；
        // outcome 在两种情况下都使用 Terminated。
        plan @ TickPlan::SkipPenalty => {
            sandbox.settle_bill();
            Ok(TickReport {
                epoch,
                outcome: TickOutcome::Terminated,
                plan,
                used_ms: 0.0,
                player_error: None,
                console: sandbox.drain_console(),
                missing_api: sandbox.drain_missing_api().unwrap_or(serde_json::json!({})),
                stub_calls: sandbox.drain_stub_calls().unwrap_or(serde_json::json!({})),
                bucket_after: sandbox.bucket(),
                intents: Value::Null,
            })
        }
        plan @ TickPlan::Run { .. } => {
            let mut outcome = sandbox.run_loop()?;
            let player_error = sandbox.take_last_loop_error();
            let used_ms = match &outcome {
                TickOutcome::Completed { .. } => sandbox.cpu_used_ms()?,
                TickOutcome::Terminated => 0.0,
            };
            // 此通道立即提交 end_tick 的费用，包含写回阶段耗时。
            // 若写回被中断，报告的 outcome 同步改为 Terminated。
            let writeback = sandbox.end_tick()?;
            sandbox.settle_bill();
            let intents = sandbox.take_intents().unwrap_or(Value::Null);
            if writeback.get("terminated").and_then(Value::as_bool) == Some(true) {
                outcome = TickOutcome::Terminated;
            }
            let player_error = match player_error {
                Some(e) => Some(e),
                None => match writeback.get("ok").and_then(Value::as_bool) {
                    Some(false) => writeback
                        .get("error")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    _ => None,
                },
            };
            Ok(TickReport {
                epoch,
                outcome,
                plan,
                used_ms,
                player_error,
                console: sandbox.drain_console(),
                missing_api: sandbox.drain_missing_api().unwrap_or(serde_json::json!({})),
                stub_calls: sandbox.drain_stub_calls().unwrap_or(serde_json::json!({})),
                bucket_after: sandbox.bucket(),
                intents,
            })
        }
    }
}

/// 执行注入、表达式与写回，返回等待协调器提交的结果；原样回带 epoch。
fn run_eval_tick(
    sandbox: &mut Sandbox,
    world: &Value,
    bundle: Option<KkpvPrivateBundle>,
    public: Option<crate::shared_bundle::SharedKkpvBundle>,
    code: &str,
    epoch: RunEpoch,
) -> anyhow::Result<EvalReport> {
    run_eval_tick_with_budget(sandbox, world, bundle, public, code, epoch, None)
}

fn run_eval_tick_with_budget(
    sandbox: &mut Sandbox,
    world: &Value,
    bundle: Option<KkpvPrivateBundle>,
    public: Option<crate::shared_bundle::SharedKkpvBundle>,
    code: &str,
    epoch: RunEpoch,
    budget: Option<&kk_core::cpu::RunBudget>,
) -> anyhow::Result<EvalReport> {
    // 世界注入失败作为该玩家的 inject_failed 结果返回，让协调器继续处理其他玩家。
    // 通道断开等基础设施故障仍返回 Err。
    let begin = match (channel_input(bundle, public)?, budget) {
        (ChannelInput::Kkpv { public, private }, Some(budget)) => {
            sandbox.begin_tick_kkpv_with_budget(world, &public, &private, budget)
        }
        (ChannelInput::Kkpv { public, private }, None) => {
            sandbox.begin_tick_kkpv(world, &public, &private)
        }
        (ChannelInput::Json, Some(budget)) => sandbox.begin_tick_with_budget(world, budget),
        (ChannelInput::Json, None) => sandbox.begin_tick(world),
    };
    let plan = match begin {
        Ok(plan) => plan,
        Err(e) => {
            // 注入失败的费用候选按完整预算计算，报告携带同一候选；此处尚不修改余额。
            let bill = sandbox.pending_bill();
            return Ok(EvalReport {
                expr: serde_json::json!({
                    "ok": false, "kind": "inject_failed",
                    "error": format!("世界注入失败: {e}"),
                }),
                console: sandbox.drain_console(),
                bucket_after: sandbox.bucket(),
                intents: Value::Null,
                cpu_billed_ms: bill,
                raw_memory: None,
                ism_local: None,
                segments_out: None,
                epoch,
            });
        }
    };
    match plan {
        TickPlan::SkipPenalty => Ok(EvalReport {
            // 代码未执行时使用 terminated，区别于成功求值得到 undefined。
            // 停跑回充留给协调器提交，bucket_after 仍是提交前余额。
            expr: serde_json::json!({
                "ok": false, "kind": "terminated", "error": "本 tick 停跑（桶透支为负，玩家代码未执行）",
            }),
            console: sandbox.drain_console(),
            bucket_after: sandbox.bucket(),
            intents: Value::Null,
            cpu_billed_ms: None,
            raw_memory: None,
            ism_local: None,
            segments_out: None,
            epoch,
        }),
        TickPlan::Run { .. } => {
            let mut expr = sandbox.run_expr(code)?;
            let writeback = sandbox.end_tick()?;
            let intents = sandbox.take_intents().unwrap_or(Value::Null);
            // 代码开始执行后被中断，或写回失败，按适配器契约归为 runtime。
            // 只有上面的负余额停跑分支使用 terminated 表示尚未执行。
            if expr.get("ok").and_then(Value::as_bool) == Some(false)
                && expr.get("kind").and_then(Value::as_str) == Some("terminated")
            {
                expr = serde_json::json!({
                    "ok": false, "kind": "runtime",
                    "error": "硬中断（超时或内存闸门，代码已开始执行）",
                });
            } else if expr.get("ok").and_then(Value::as_bool) == Some(true)
                && writeback.get("ok").and_then(Value::as_bool) == Some(false)
            {
                expr = serde_json::json!({
                    "ok": false, "kind": "runtime",
                    "error": writeback.get("error").cloned().unwrap_or_else(|| "Memory 写回失败".into()),
                });
            }
            Ok(EvalReport {
                expr,
                console: sandbox.drain_console(),
                bucket_after: sandbox.bucket(),
                intents,
                // 计费与 RawMemory 均作为候选返回；协调器在世界结算成功后提交。
                cpu_billed_ms: sandbox.pending_bill(),
                segments_out: writeback.get("segmentsOut").cloned(),
                raw_memory: writeback
                    .get("rawMemory")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                ism_local: writeback
                    .get("ismLocal")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                epoch,
            })
        }
    }
}
