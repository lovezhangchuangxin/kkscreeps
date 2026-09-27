//! "一个玩家实例绑一个线程"的承载原型（M0）。
//!
//! 动机：`JsRuntime` 非 Send，V8 实例不能跨线程挪动（ADR-008 第 1 条）。
//! 模型：每玩家一个专属线程，线程阻塞在命令通道上即"挂起"（park，常驻开销 KB 级）；
//! V8 实例沿用官方的 3 分钟空闲销毁（官方 user-vm.js 同款），重建实测 <1ms（快照后）。
//!
//! 销毁纪律：V8 要求 isolate 按创建逆序（LIFO）销毁。`spawn_player` 会等待实例
//! 创建完成的回执再返回，使"注册顺序 = 创建顺序"确定化；`shutdown_all` 按逆序
//! 回收全部线程。**不要在 registry 存活期间手动 drop 单个线程**——用 `evict_idle`
//! （它按逆序从栈顶弹出）。

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
    pub console: Vec<LogLine>,
    pub missing_api: Value,
    pub stub_calls: Value,
    pub bucket_after: f64,
}

/// 一次"玩家代码求值 tick"的报告（screeps-ok runPlayer 通道）。
#[derive(Debug)]
pub struct EvalReport {
    /// runExpr 的结果 JSON：`{ok, value?} | {ok:false, kind, error}`。
    pub expr: Value,
    pub console: Vec<LogLine>,
    pub bucket_after: f64,
}

enum Cmd {
    SetModules(ModuleTable, mpsc::Sender<anyhow::Result<()>>),
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
                        Cmd::Tick { world, ack } => {
                            let _ = ack.send(run_one_tick(&mut sandbox, &world));
                        }
                        Cmd::RunExpr { code, ack } => {
                            let _ = ack.send(sandbox.run_expr(&code));
                        }
                        Cmd::RunPlayer { world, code, ack } => {
                            let _ = ack.send(run_eval_tick(&mut sandbox, &world, &code));
                        }
                        Cmd::Shutdown => break,
                    }
                }
                // 通道断开或 Shutdown：sandbox 在此 drop（LIFO 顺序由 shutdown_all/evict 保证）
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

    /// 逐出空闲超过 `idle` 的玩家（逆序从栈顶弹出，保持 LIFO）。
    /// 返回被逐出的玩家 id 列表。官方语义：V8 实例 3 分钟空闲销毁，线程按需重建。
    pub fn evict_idle(&mut self, idle: Duration) -> Vec<u64> {
        let mut evicted = Vec::new();
        while let Some(p) = self.players.last() {
            if p.last_command.elapsed() < idle {
                break;
            }
            let mut p = self.players.pop().expect("checked last");
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
        plan @ TickPlan::SkipPenalty => Ok(TickReport {
            outcome: TickOutcome::Terminated,
            plan,
            used_ms: 0.0,
            console: sandbox.drain_console(),
            missing_api: serde_json::json!({}),
            stub_calls: serde_json::json!({}),
            bucket_after: sandbox.bucket(),
        }),
        plan @ TickPlan::Run { .. } => {
            let outcome = sandbox.run_loop()?;
            let used_ms = match &outcome {
                TickOutcome::Completed { .. } => sandbox.cpu_used_ms(),
                TickOutcome::Terminated => 0.0,
            };
            sandbox.end_tick(used_ms)?;
            Ok(TickReport {
                outcome,
                plan,
                used_ms,
                console: sandbox.drain_console(),
                missing_api: sandbox.drain_missing_api().unwrap_or(serde_json::json!({})),
                stub_calls: sandbox.drain_stub_calls().unwrap_or(serde_json::json!({})),
                bucket_after: sandbox.bucket(),
            })
        }
    }
}

/// runPlayer 通道：世界注入 → 表达式求值（真实沙箱）→ Memory 写回与结算。
/// 跳帧惩罚时表达式不执行，返回硬中断分类（与"代码真的被执行过"契约一致：
/// 由调用方把"未执行"视作适配器错误而非玩家返回值）。
fn run_eval_tick(sandbox: &mut Sandbox, world: &Value, code: &str) -> anyhow::Result<EvalReport> {
    match sandbox.begin_tick(world)? {
        TickPlan::SkipPenalty => Ok(EvalReport {
            expr: serde_json::json!({
                "ok": false, "kind": "terminated", "error": "本 tick 被跳帧惩罚（上一 tick 硬中断）",
            }),
            console: sandbox.drain_console(),
            bucket_after: sandbox.bucket(),
        }),
        TickPlan::Run { .. } => {
            let expr = sandbox.run_expr(code)?;
            sandbox.end_tick(sandbox.cpu_used_ms().min(sandbox.tick_limit()))?;
            Ok(EvalReport {
                expr,
                console: sandbox.drain_console(),
                bucket_after: sandbox.bucket(),
            })
        }
    }
}
