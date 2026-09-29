//! "一个玩家实例绑一个线程"的承载原型（M0）。
//!
//! 动机：`JsRuntime` 非 Send，V8 实例不能跨线程挪动（ADR-008 第 1 条）。
//! 模型：每玩家一个专属线程，线程阻塞在命令通道上即"挂起"（park，常驻开销 KB 级）；
//! V8 实例沿用官方的 3 分钟空闲销毁（官方 user-vm.js 同款），重建实测 <1ms（快照后）。
//!
//! 销毁纪律（ADR-013 纠偏：每线程）：rusty_v8 的 isolate Enter/Exit 是线程
//! 局部栈，同线程内多实例须按创建逆序销毁；**跨线程没有全局逆序要求**（探针
//! 实证乱序销毁不崩）。`spawn_player` 会等待实例创建完成的回执再返回，使
//! "注册顺序 = 创建顺序"确定化；`shutdown_all` 默认仍按创建逆序回收全部线程
//! （稳妥）；`evict_idle` 解除栈顶限制、支持回收**任意**空闲实例（一玩家一
//! 线程模型下每线程恰一个 isolate，天然满足线程内约束）。

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
    pub outcome: TickOutcome,
    pub plan: TickPlan,
    pub used_ms: f64,
    /// 玩家 loop 逻辑自身的错误（入口执行≠无异常；None = 正常完成或硬中断）。
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
    /// runExpr 的结果 JSON：`{ok, value?} | {ok:false, kind, error}`。
    pub expr: Value,
    pub console: Vec<LogLine>,
    pub bucket_after: f64,
    /// endTick 带出的本 tick intentsList（Null = 无/硬中断丢弃）。
    pub intents: Value,
}

enum Cmd {
    SetModules(ModuleTable, mpsc::Sender<anyhow::Result<()>>),
    /// 注入 RawMemory 初值（tick 边界——协调层提交点统一驱动的通道面）。
    SetRawMemory(String, mpsc::Sender<anyhow::Result<()>>),
    Tick {
        world: Value,
        ack: mpsc::Sender<anyhow::Result<TickReport>>,
    },
    RunExpr {
        code: String,
        ack: mpsc::Sender<anyhow::Result<Value>>,
    },
    /// 一条原子指令：begin_tick（世界注入）→ run_expr（求值玩家代码）→ end_tick。
    /// screeps-ok 的 runPlayer 语义（"恰好推进 1 tick"）靠它保证。
    RunPlayer {
        world: Value,
        code: String,
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

/// 玩家线程池（M0 原型：无工作窃取、无优先级，只验证承载模型）。
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
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Cmd::SetModules(table, ack) => {
                            let _ = ack.send(sandbox.set_modules(table));
                        }
                        Cmd::SetRawMemory(data, ack) => {
                            let _ = ack.send(sandbox.set_raw_memory(&data));
                        }
                        Cmd::Tick { world, ack } => {
                            let _ = ack.send(run_one_tick(&mut sandbox, &world));
                        }
                        Cmd::RunExpr { code, ack } => {
                            // 看门狗编排（并发-4 清偿）：通道执行任意玩家代码，
                            // 纯 CPU 死循环必须可被掐断，否则线程永久挂死（join 无效）
                            let _ = ack.send(sandbox.run_expr_guarded(&code));
                        }
                        Cmd::RunPlayer { world, code, ack } => {
                            let _ = ack.send(run_eval_tick(&mut sandbox, &world, &code));
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

    /// 注入玩家 RawMemory 初值（tick 边界；Sandbox::set_raw_memory 的私有
    /// 句柄通道——协调层提交点统一驱动的接入面）。
    pub fn set_raw_memory(&mut self, id: u64, data: &str) -> anyhow::Result<()> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::SetRawMemory(data.to_string(), ack_tx));
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 对一个玩家跑完整一 tick（begin→loop→end），等待报告。
    pub fn tick(&mut self, id: u64, world: &Value) -> anyhow::Result<TickReport> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::Tick {
            world: world.clone(),
            ack: ack_tx,
        });
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 对全部玩家按注册顺序跑一 tick（发送有序、回收有序——确定性的雏形）。
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

    /// 原子"求值 tick"：begin→run_expr→end（screeps-ok runPlayer 通道）。
    pub fn run_player(&mut self, id: u64, world: &Value, code: &str) -> anyhow::Result<EvalReport> {
        let p = self.player_mut(id)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        p.send(Cmd::RunPlayer {
            world: world.clone(),
            code: code.to_string(),
            ack: ack_tx,
        });
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("玩家 {id} 线程无响应"))?
    }

    /// 逐出空闲超过 `idle` 的玩家（全表扫描，任意实例——不限栈顶：LIFO 是
    /// 每线程约束，跨线程销毁顺序无要求，后创建的活跃玩家不得挡住前面空闲
    /// 玩家的回收）。返回被逐出的玩家 id 列表（按注册表序）。官方语义：V8
    /// 实例 3 分钟空闲销毁，线程按需重建。
    ///
    /// 两段式：先按同一时刻快照判定候选集，再逐个回收——逐出者的 join
    ///（V8 teardown 可达数百 ms）不得推进后续候选者的时钟（饥饿下会把
    /// 刚活跃的邻居一并挤出）。
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

fn run_one_tick(sandbox: &mut Sandbox, world: &Value) -> anyhow::Result<TickReport> {
    match sandbox.begin_tick(world)? {
        // 跳帧 tick（桶为负透支，官方 cpuAvailable<0 停跑语义）：不执行玩家代码。
        // TickReport.plan 携带 SkipPenalty 供消费方（bot_check 等）与真实硬中断区分；
        // outcome 有意映射为 Terminated（runPlayer 的 executed=false 契约依赖它）。
        plan @ TickPlan::SkipPenalty => Ok(TickReport {
            outcome: TickOutcome::Terminated,
            plan,
            used_ms: 0.0,
            player_error: None,
            console: sandbox.drain_console(),
            missing_api: sandbox.drain_missing_api().unwrap_or(serde_json::json!({})),
            stub_calls: sandbox.drain_stub_calls().unwrap_or(serde_json::json!({})),
            bucket_after: sandbox.bucket(),
            intents: Value::Null,
        }),
        plan @ TickPlan::Run { .. } => {
            let mut outcome = sandbox.run_loop()?;
            let player_error = sandbox.take_last_loop_error();
            let used_ms = match &outcome {
                TickOutcome::Completed { .. } => sandbox.cpu_used_ms(),
                TickOutcome::Terminated => 0.0,
            };
            // end_tick 自带结算（含写回耗时）；写回阶段被硬中断（如玩家经
            // Memory.toJSON 挂死写回）→ 本 tick 记 Terminated
            let writeback = sandbox.end_tick()?;
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

/// runPlayer 通道：世界注入 → 表达式求值（真实沙箱）→ Memory 写回与结算。
fn run_eval_tick(sandbox: &mut Sandbox, world: &Value, code: &str) -> anyhow::Result<EvalReport> {
    match sandbox.begin_tick(world)? {
        TickPlan::SkipPenalty => Ok(EvalReport {
            // 负桶停跑：代码根本没执行——契约规定这是适配器的普通基础设施错误
            //（"未执行 ≠ undefined"），不是 RunPlayerError
            expr: serde_json::json!({
                "ok": false, "kind": "terminated", "error": "本 tick 停跑（桶透支为负，玩家代码未执行）",
            }),
            console: sandbox.drain_console(),
            bucket_after: sandbox.bucket(),
            intents: Value::Null,
        }),
        TickPlan::Run { .. } => {
            let mut expr = sandbox.run_expr(code)?;
            let writeback = sandbox.end_tick()?;
            let intents = sandbox.take_intents().unwrap_or(Value::Null);
            // 契约（adapter-spec:221-235，vanilla 参照同口径）：按失败阶段分类——
            // 代码已开始执行后被硬中断、或 tick 末 Memory 序列化失败 → runtime；
            // 只有"根本没执行"（上面的停跑分支）才保持 terminated
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
            })
        }
    }
}
