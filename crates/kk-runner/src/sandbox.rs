//! 持有一个玩家的 V8 实例，装配玩家 API，并生成指令、Memory 和计费候选。
//!
//! 看门狗按墙钟期限请求中断；近堆上限回调临时扩大堆上限以完成当前分配，
//! 同时请求中断，随后恢复配置上限。此限制针对 V8 堆，不覆盖所有堆外或宿主分配。
//!
//! 同一线程内的多个 V8 实例须按创建逆序销毁；本类型只检测并告警，
//! 调用方仍需保证顺序。字段顺序保证先回收看门狗，再销毁 runtime 和 tokio。
//! 每玩家一个线程的 PlayerThreads 可以回收任意玩家，无需跨线程逆序。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use deno_core::v8;
use deno_core::{JsRuntime, RuntimeOptions};

use crate::budget::{BUCKET_CAP, Bucket, DEFAULT_CPU_LIMIT, MAX_PER_TICK, TickPlan};
use crate::modules::ModuleTable;
use crate::ops::{LogLine, Shared, SharedRef, kk_runner};
use crate::watchdog::Watchdog;
use serde_json::Value;

/// 默认 V8 堆上限，单位字节；不等于整个玩家线程的内存上限。
const DEFAULT_HEAP_LIMIT: usize = 256 * 1024 * 1024;

/// Mod 玩家面安装（mods/player-api.js + 全部玩家模块装载与 install 调用）的墙钟
/// 期限：作者代码首次执行的总闸——超时中断使沙箱创建失败，不楔死
/// 分片创建（正常安装是毫秒级）。
const INSTALL_WALL_LIMIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub heap_limit_bytes: usize,
    pub cpu_limit: f64,
    pub bucket_cap: f64,
    pub max_per_tick: f64,
    pub initial_bucket: f64,
    /// 宿主按有效规则覆盖的玩家全局常量；默认为空，保持嵌入资产原值。
    /// 键须是常量表已有的名字，覆盖发生在平铺到 globalThis 之前。
    pub constant_overrides: std::collections::BTreeMap<String, serde_json::Value>,
    /// 是否装配扩展意图的玩家面原语（`__kkEmitExtensionIntent`）。
    /// 只有分片计划含 Mod 意图时置 true；vanilla 分片的全局面不变。
    pub extension_intents: bool,
    /// Mod 玩家面安装清单：存在时装配安装运行时并逐包执行玩家模块
    ///（基础装配后，捕获安装入口并删除私有桥，再执行作者模块）；vanilla 分片为 None。
    pub mod_api: Option<std::sync::Arc<crate::mod_api::ModPlayerApi>>,
    /// Mod 安装期的墙钟期限；None 用默认 10s。测试可缩短以验证挂死
    /// 兜底（作者代码死循环/TLA 使安装失败而不是楔死分片创建）。
    pub install_wall_limit: Option<Duration>,
    /// 计划级墙地形通行声明：JSON/KKPV 通道的寻路地形在物化时按它
    /// 清除墙位（Frozen 通道的快照由宿主构建时已应用）。默认 false
    /// 即官方「墙不可通行」；不影响物理地形查询。
    pub terrain_walls_passable: bool,
    /// 计划级静态对象通行；只改变默认寻路矩阵，生物占格和玩家矩阵保持。
    pub obstacles_passable: bool,
    pub private_ramparts_passable: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            heap_limit_bytes: DEFAULT_HEAP_LIMIT,
            cpu_limit: DEFAULT_CPU_LIMIT,
            bucket_cap: BUCKET_CAP,
            max_per_tick: MAX_PER_TICK,
            initial_bucket: BUCKET_CAP,
            constant_overrides: std::collections::BTreeMap::new(),
            extension_intents: false,
            mod_api: None,
            install_wall_limit: None,
            terrain_walls_passable: false,
            obstacles_passable: false,
            private_ramparts_passable: false,
        }
    }
}

/// 一次玩家执行的结局。
#[derive(Debug)]
pub enum TickOutcome {
    /// 正常执行完毕（玩家逻辑自身的错误由 runLoop 结果 JSON 携带，不算这里）。
    Completed { used_ms: f64 },
    /// 执行被中断，包括超时、近堆上限和主动 halt。
    /// 此结果本身不提交费用；超时和近堆上限生成完整预算费用，halt 不计费。
    Terminated,
}

// ---------------------------------------------------------------- LIFO 纪律（每线程）

static SEQ: AtomicU64 = AtomicU64::new(0);
static LIVE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static LIFO_VIOLATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
// 本线程存活实例的创建顺序，用于检测逆序销毁要求。
// 其他线程的实例不属于这个栈；跨线程销毁顺序不影响本线程的 Enter/Exit。
thread_local! {
    static THREAD_ORDER: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// 当前进程存活的 Sandbox 数（测试用：全部销毁后应归零）。
pub fn live_sandbox_count() -> usize {
    LIVE_COUNT.load(Ordering::SeqCst)
}

/// 线程内 LIFO 销毁纪律的违规次数（测试断言为零——告警不该被无视）。
pub fn lifo_violations() -> usize {
    LIFO_VIOLATIONS.load(Ordering::SeqCst)
}

pub struct Sandbox {
    // 字段声明顺序 = Drop 顺序：先 join 看门狗线程，再销毁 JsRuntime，最后拆
    // tokio。host（v8::Global 句柄）在 isolate 销毁后才 drop——v8 150 对已销毁
    // isolate 的 Global drop 是显式安全 no-op（handle.rs "Globals may be dropped
    // after their host isolate is disposed"）。
    watchdog: Watchdog,
    runtime: JsRuntime,
    tokio: tokio::runtime::Runtime,
    /// Rust 捕获的宿主函数句柄，不通过玩家全局名称查找生命周期入口。
    host: HostFns,
    seq: u64,
    budget: Bucket,
    command_deadline: Option<Instant>,
    shared: SharedRef,
    /// 配置的堆硬上限（near-heap 触发放大后据此恢复）。
    heap_limit: usize,
    /// 本 tick 是否已中断；end_tick 据此跳过写回。
    /// 正常 end_tick 不设置此标志，费用提交与此标志分别管理。
    terminated_this_tick: bool,
    /// 上一次 runLoop 捕获的玩家逻辑错误（run_loop 写入，take_last_loop_error 取走）。
    last_loop_error: Option<String>,
    /// 等待 take_intents 取走的本 tick 指令；中断或写回失败时丢弃。
    /// 协调器将其转换为 IntentBatch，再交给世界结算。
    pending_intents: Option<Value>,
    /// 本 tick 待提交的费用，单位毫秒：正常结束为线程 CPU 用量加 0.2×指令数，
    /// 超时或近堆上限中断为完整预算。RunJob 导出给宿主；独立诊断通道本地提交或丢弃。
    /// 新的 begin_tick 会清除尚未处理的候选，调用方须先处理上一轮结果。
    pending_bill: Option<f64>,
    /// 负余额停跑时暂存的回充增量，与计费候选互斥。
    /// settle_bill 应用回充，discard_bill 丢弃；新的 begin_tick 清除旧候选。
    pending_recharge: Option<f64>,
    /// 原始 JSON 寻路地形按内容核对；可信宿主另传世界独立快照。
    /// 任一房换地形全体失效）；裸数组夹具形态无 rev，不缓存逐 tick 重建。
    terrain_pack_cache: Option<std::sync::Arc<crate::pathfinder::TerrainPack>>,
    host_terrain: Option<std::sync::Arc<kk_pathfinder::TerrainSet>>,
    /// 计划级墙地形通行声明；见 [`SandboxConfig::terrain_walls_passable`]。
    terrain_walls_passable: bool,
    /// 与 Mod realm 共用的线程栈账本；须在 runtime 销毁后注销。
    _isolate_order: kk_vm::isolate_order::IsolateOrder,
}

impl Sandbox {
    pub fn new(config: SandboxConfig) -> anyhow::Result<Self> {
        crate::cpu_clock::thread_cpu_time()?;
        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let _guard = tokio.enter();

        let create_params = v8::CreateParams::default().heap_limits(0, config.heap_limit_bytes);
        // 玩家模块装载器只在 Mod 分片装配（vanilla 分片的运行时选项不变）。
        let player_loader = config.mod_api.as_ref().map(|api| {
            std::rc::Rc::new(crate::mod_api::PlayerModuleLoader {
                modules: api.modules.clone(),
            })
        });
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![kk_runner::init()],
            create_params: Some(create_params),
            module_loader: player_loader
                .map(|loader| loader as std::rc::Rc<dyn deno_core::ModuleLoader>),
            ..Default::default()
        });

        let shared: SharedRef = Arc::new(Shared::default());
        let mut install_watchdog = None;
        let host = {
            let _guard = tokio.enter();
            runtime.op_state().borrow_mut().put(ModuleTable::new());
            runtime.op_state().borrow_mut().put(shared.clone());
            // Game.cpu.halt 在玩家线程内设置 V8 终止标志；后续根据 halt_requested 选择计费方式。
            runtime
                .op_state()
                .borrow_mut()
                .put(runtime.v8_isolate().thread_safe_handle());

            prelude::install_base(&mut runtime, &config)?;
            // Mod 玩家面安装（mods/player-api.js + 模块装载）先于 __kkp 删除与玩家可
            // 见化：mods/player-api.js 在 __kkp 上定义安装运行时并把 createInstallApi/
            // finalize 捕获为 Rust 持有的 v8 句柄，随后先捕获宿主句柄、删
            // 除 __kkp，再经捕获句柄逐包装载玩家模块——Mod 代码执行时
            // __kkp 已删除，与玩家同权限面。任何一步失败都使沙箱创建失败
            // （fail-loud），不产出半装好的玩家 API。
            //
            // Mod 代码是首次执行的作者代码：看门狗带安装墙钟期限、近堆
            // 回调先于安装挂好——死循环挂死与无界分配进程中止都有兜底，
            // 中断使安装失败而不是楔死分片创建。
            // 宿主句柄在 __kkp 删除前捕获（host.js 注册于其上）。
            let host = HostFns::capture(&mut runtime)?;
            let mod_install = match &config.mod_api {
                Some(api) => {
                    let watchdog = Watchdog::spawn(runtime.v8_isolate().thread_safe_handle());
                    install_near_heap_callback(&mut runtime, shared.clone());
                    watchdog.arm(
                        Instant::now() + config.install_wall_limit.unwrap_or(INSTALL_WALL_LIMIT),
                    );
                    let result = prelude::install_mod_player_api(&mut runtime, &tokio, api);
                    watchdog.disarm();
                    runtime.v8_isolate().cancel_terminate_execution();
                    match result {
                        Ok(()) => {
                            install_watchdog = Some(watchdog);
                            Ok(())
                        }
                        Err(error) => Err(anyhow::anyhow!("Mod 玩家面安装失败：{error}")),
                    }
                }
                None => Ok(()),
            };
            mod_install?;
            // Mod 路径在安装内部已删除 __kkp；这里再删一次幂等（缺键 delete
            // 为 no-op），两路径共用同一收尾。
            prelude::unlink(&mut runtime)?;
            host
        };

        let watchdog = install_watchdog
            .unwrap_or_else(|| Watchdog::spawn(runtime.v8_isolate().thread_safe_handle()));

        let seq = SEQ.fetch_add(1, Ordering::SeqCst);
        let mut sb = Self {
            watchdog,
            runtime,
            tokio,
            host,
            seq,
            command_deadline: None,
            budget: Bucket::new(
                config.cpu_limit,
                config.bucket_cap,
                config.max_per_tick,
                config.initial_bucket,
            ),
            shared,
            heap_limit: config.heap_limit_bytes,
            terminated_this_tick: false,
            last_loop_error: None,
            pending_intents: None,
            pending_bill: None,
            pending_recharge: None,
            terrain_pack_cache: None,
            host_terrain: None,
            terrain_walls_passable: config.terrain_walls_passable,
            _isolate_order: kk_vm::isolate_order::IsolateOrder::register(),
        };
        sb.install_near_heap_callback();
        // 所有可能失败的初始化都完成后再登记实例，避免 ? 早退留下没有实例的计数和栈记录。
        THREAD_ORDER.with(|s| s.borrow_mut().push(seq));
        LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(sb)
    }

    fn install_near_heap_callback(&mut self) {
        install_near_heap_callback(&mut self.runtime, self.shared.clone());
    }

    /// 近堆回调触发后，以配置值请求恢复上限并重新注册回调。
    /// V8 生效上限还受当前堆状态影响，不保证恰好等于配置值。
    /// 诊断标志保留至下一次 begin_tick，便于判断本轮是否触发过。
    fn restore_heap_limit(&mut self) {
        if self.shared.near_heap_fired.load(Ordering::Relaxed) {
            self.runtime
                .remove_near_heap_limit_callback(self.heap_limit);
            self.install_near_heap_callback();
        }
    }

    /// 替换 Rust 模块表，并重建 JavaScript require 缓存。
    /// 缓存重置失败也已替换模块表，此操作不提供失败回滚。
    pub fn set_modules(&mut self, table: ModuleTable) -> anyhow::Result<()> {
        {
            let state = self.runtime.op_state();
            state.borrow_mut().put(table);
        }
        // 失败时清除 V8 终止标志并恢复堆上限；此调用不生成计费候选。
        call_host_unit(
            &self.tokio,
            &mut self.runtime,
            &self.host.reset_modules,
            &[],
        )
        .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    /// 设置下一 tick 的 RawMemory 初值，通过捕获的宿主函数写入闭包状态。
    ///
    /// 调用方须在 begin_tick 前注入；此处不检查时序。tick 中途注入不会清除
    /// 已解析的 RawMemory._parsed，后续写回可能用旧解析值覆盖新数据。
    pub fn set_raw_memory(&mut self, data: &str) -> anyhow::Result<()> {
        call_host_unit(
            &self.tokio,
            &mut self.runtime,
            &self.host.set_raw_memory,
            &[data.to_string()],
        )
        .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    /// 注入 RawMemory、活动 segment 窗口（`[[id,str],...]` JSON）和 foreignSegment。
    /// foreignSegment 的 null 字符串表示无数据；须在 begin_tick 前调用。
    pub fn set_memory_state(
        &mut self,
        raw: &str,
        segments_json: &str,
        foreign_json: &str,
    ) -> anyhow::Result<()> {
        call_host_unit(
            &self.tokio,
            &mut self.runtime,
            &self.host.set_memory_state,
            &[
                raw.to_string(),
                segments_json.to_string(),
                foreign_json.to_string(),
            ],
        )
        .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    pub fn set_command_deadline(&mut self, deadline: Instant) -> anyhow::Result<()> {
        if deadline <= Instant::now() {
            anyhow::bail!("玩家命令在执行前已过期");
        }
        // 先封住旧代次的投递，再清掉已投递的标志；内存注入早于 begin_tick。
        self.watchdog.disarm();
        self.clear_terminate_flag();
        self.command_deadline = Some(deadline);
        self.watchdog.arm(deadline);
        Ok(())
    }
    pub fn clear_command_deadline(&mut self) {
        self.watchdog.disarm();
        self.clear_terminate_flag();
        self.command_deadline = None;
        *self.shared.tick_start.lock().unwrap() = None;
        *self.shared.thread_cpu_start.lock().unwrap() = None;
    }

    pub fn bucket(&self) -> f64 {
        self.budget.bucket()
    }

    pub fn tick_limit(&self) -> f64 {
        self.budget.tick_limit()
    }

    /// 执行玩家入口 `require('main').loop()`。入口执行完毕即 Completed——
    /// "执行到入口"≠"玩家代码无异常"：玩家逻辑错误由 runLoop 内部捕获，
    /// 经 `take_last_loop_error` 暴露（报告/验收统计据此区分）。
    pub fn run_loop(&mut self) -> anyhow::Result<TickOutcome> {
        let result = call_host_json(&self.tokio, &mut self.runtime, &self.host.run_loop, &[]);
        match result {
            Ok(v) => {
                self.last_loop_error = match v.get("ok").and_then(serde_json::Value::as_bool) {
                    Some(false) => v
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    _ => None,
                };
                Ok(TickOutcome::Completed { used_ms: 0.0 })
            }
            Err(_) => {
                // runLoop 内部捕获普通玩家异常；逃出宿主调用的错误按中断处理。
                // halt_requested 区分主动 halt，此情况不生成费用。
                if self.shared.halt_requested.load(Ordering::Acquire) {
                    self.after_halt();
                } else {
                    self.after_termination();
                }
                Ok(TickOutcome::Terminated)
            }
        }
    }

    /// 处理主动 halt：解除期限、清除终止标志、恢复堆上限并跳过本轮写回。
    /// 不生成费用，不销毁长期 V8 实例；已收集的日志保留，并追加 CPU halted。
    /// 来源：上游 driver/lib/runtime/make.js 的 didHaltByUserRequest 分支，
    /// 保留沙箱和已收日志的差异见 docs/designs/compatibility.md。
    fn after_halt(&mut self) {
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        if !self.terminated_this_tick {
            self.pending_bill = None;
            self.terminated_this_tick = true;
            self.shared.push_console_line("error", "CPU halted");
        }
        self.restore_heap_limit();
        *self.shared.tick_start.lock().unwrap() = None;
        *self.shared.thread_cpu_start.lock().unwrap() = None;
    }

    /// 取走最近一次 runLoop 捕获的玩家逻辑错误；取走后清空。
    /// 中断分支不更新该字段，调用方应每轮消费，避免残留上一轮错误。
    pub fn take_last_loop_error(&mut self) -> Option<String> {
        self.last_loop_error.take()
    }

    /// 执行 Memory 写回并生成指令与计费候选，不提交桶余额。
    ///
    /// 写回可能调用玩家 Memory.toJSON，必须继续受看门狗保护。返回 JSON
    /// 表示写回成功、普通错误或中断；中断费用由处理函数暂存，调用方随后决定是否提交。
    pub fn end_tick(&mut self) -> anyhow::Result<serde_json::Value> {
        // 硬中断后的 Memory 保持上一 tick 的存档。
        if self.terminated_this_tick {
            // 中断后不调用 JS 写回函数，也不保留本轮指令；费用候选已由中断处理生成。
            self.pending_intents = None;
            return Ok(serde_json::json!({
                "ok": false, "terminated": true,
                "error": "本 tick 已硬中断，按官方语义跳过 Memory 写回",
            }));
        }
        // 清除执行入口结束后可能迟到的 V8 终止标志，再为写回登记新代次。
        // 期限根据保存的 tick_start 加预算重算，不重新给写回一份完整预算。
        // 若不重新登记，旧代次可能已经触发，Memory.toJSON 死循环就无法再次中断。
        self.runtime.v8_isolate().cancel_terminate_execution();
        if let Some(t0) = *self.shared.tick_start.lock().unwrap() {
            let deadline = t0 + Duration::from_millis(self.budget.tick_limit().max(1.0) as u64);
            let deadline = self
                .command_deadline
                .map_or(deadline, |command| command.min(deadline));
            self.watchdog.arm(deadline);
        }
        let exec = call_host_json(&self.tokio, &mut self.runtime, &self.host.end_tick, &[]);
        match exec {
            Ok(v) => {
                self.watchdog.disarm();
                // intentsList 带出与宿主计费（官方双轨：沙箱内 intents.cpu 供
                // getUsed 即时显示 runtime.js:202-204；宿主按最终清单重算
                // 0.2×count 并入扣桶 make.js:193-194——排除 say/pull）。
                // 硬中断 tick 不进此分支（Err 路径丢弃 intents 并按整份结算，
                // 官方 isolate 级故障同款 make.js:279-294）
                let (intents, billed) = self.take_endtick_intents(&v);
                // 普通写回错误也丢弃指令，避免只提交玩家 tick 的部分结果。
                // 仍按当前已用时间和收集到的指令数生成费用候选。
                let intents = if v.get("ok").and_then(Value::as_bool) == Some(false) {
                    None
                } else {
                    intents
                };
                self.pending_intents = intents;
                // 线程 CPU 用量包含写回耗时；此处只暂存，世界结算成功后由调用方提交。
                self.pending_bill = Some(self.cpu_used_ms()? + 0.2 * billed as f64);
                // 即使宿主调用成功返回，也恢复可能被近堆回调扩大的上限。
                self.restore_heap_limit();
                *self.shared.tick_start.lock().unwrap() = None;
                *self.shared.thread_cpu_start.lock().unwrap() = None;
                Ok(v)
            }
            Err(_) => {
                // 写回被中断时丢弃指令并清理执行状态；主动 halt 无费用，其他中断计完整预算。
                self.pending_intents = None;
                if self.shared.halt_requested.load(Ordering::Acquire) {
                    self.after_halt();
                } else {
                    self.after_termination();
                }
                Ok(
                    serde_json::json!({ "ok": false, "terminated": true, "error": "写回阶段硬中断" }),
                )
            }
        }
    }

    /// 截取 endTick 的 intentsList，并计算保留清单的计费条数。
    /// 最多保留 50,000 个顶层键，包括对象 id、room/global 和 notify 等通道。
    /// 该限制不约束嵌套数组长度或 JSON 字节数；超限截断并添加日志提示。
    fn take_endtick_intents(&mut self, v: &Value) -> (Option<Value>, usize) {
        const MAX_INTENT_OBJECTS: usize = 50_000;
        let Some(list) = v.get("intentsList") else {
            return (None, 0);
        };
        let free: &[&str] = &["say", "pull"];
        // 官方 intentsCount（make.js:100-106）：对象级按动作名、数组通道
        //（room/global）按元素数计；排除 say/pull
        let count_billed = |map: &serde_json::Map<String, Value>| {
            map.values()
                .map(|a| match a {
                    // 顶层数组通道（notify——push 全局形态）：官方 make.js:98-107
                    // intentsCount 对数组 for-in 逐元素 +1（索引串不中免费表）
                    Value::Array(items) => items.len(),
                    a => a
                        .as_object()
                        .map(|inner| {
                            inner
                                .iter()
                                .filter(|(k, _)| !free.contains(&k.as_str()))
                                .map(|(k, v)| {
                                    // 扩展意图（冒号键）按"每键一次"计：载荷可为
                                    // 数组（nullable schema 下的合法形态），不能沿用
                                    // room/global 数组通道的逐元素计数。
                                    if k.contains(':') {
                                        1
                                    } else {
                                        v.as_array().map_or(1, Vec::len)
                                    }
                                })
                                .sum::<usize>()
                        })
                        .unwrap_or(0),
                })
                .sum::<usize>()
        };
        let objects = list.as_object().map(|m| m.len()).unwrap_or(0);
        if objects > MAX_INTENT_OBJECTS {
            let mut truncated = serde_json::Map::new();
            if let Some(map) = list.as_object() {
                for (k, val) in map.iter().take(MAX_INTENT_OBJECTS) {
                    truncated.insert(k.clone(), val.clone());
                }
            }
            self.shared.push_console_line(
                "error",
                &format!("intent 上行闸门：对象数 {objects} 超上限 {MAX_INTENT_OBJECTS}，已截断"),
            );
            let billed = count_billed(&truncated);
            return (Some(Value::Object(truncated)), billed);
        }
        let billed = list.as_object().map(count_billed).unwrap_or(0);
        (Some(list.clone()), billed)
    }

    /// 取走本 tick 的 intentsList（embed 结算通道消费；None = 无或已取）。
    pub fn take_intents(&mut self) -> Option<Value> {
        self.pending_intents.take()
    }

    /// 当前计费候选的只读视图（EvalReport 带出给协调器/测试）。
    pub fn pending_bill(&self) -> Option<f64> {
        self.pending_bill
    }

    /// 读取尚未提交的停跑回充增量。
    pub fn pending_recharge(&self) -> Option<f64> {
        self.pending_recharge
    }

    /// 提交本轮费用或停跑回充，随后消费候选；没有候选时无操作。
    /// 调用方应在世界结算成功后调用。停跑回充提交时按当前桶余额重新计算。
    pub fn settle_bill(&mut self) {
        // Run 产生费用，SkipPenalty 产生回充；同轮只能有一个，否则会重复增加 limit。
        debug_assert!(
            self.pending_bill.is_none() || self.pending_recharge.is_none(),
            "计费候选与停跑回充候选不得同轮共存（Skip/Run 分支互斥）"
        );
        if let Some(bill) = self.pending_bill.take() {
            self.budget.settle(bill);
        }
        if self.pending_recharge.take().is_some() {
            self.budget.apply_recharge();
        }
    }

    /// 丢弃费用与回充候选，不修改桶余额。
    /// 此操作不恢复 V8 全局或闭包状态；下一轮应重新注入已提交的 Memory。
    pub fn discard_bill(&mut self) {
        self.pending_bill = None;
        self.pending_recharge = None;
    }

    /// 求值一段玩家表达式（screeps-ok 适配器的 runPlayer 通道），恰好占用一个 tick 的
    /// 执行语义由调用方（engine/embed）负责编排。
    pub fn run_expr(&mut self, code: &str) -> anyhow::Result<serde_json::Value> {
        match call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.run_expr,
            &[code.to_string()],
        ) {
            Ok(v) => Ok(v),
            Err(_) => {
                // 表达式中的主动 halt 也使用零计费的中断处理。
                if self.shared.halt_requested.load(Ordering::Acquire) {
                    self.after_halt();
                } else {
                    self.after_termination();
                }
                Ok(serde_json::json!({ "ok": false, "kind": "terminated", "error": "硬中断" }))
            }
        }
    }

    /// 在独立看门狗窗口中求值，不注入世界、不执行 tick 写回、不修改预算桶。
    /// 窗口按当前余额计算，至少 1 毫秒；成功或失败后均解除期限、清标志并恢复堆上限。
    /// 需要完整 tick 语义时，由调用方使用 begin_tick、run_expr 和 end_tick。
    pub fn run_expr_guarded(&mut self, code: &str) -> anyhow::Result<serde_json::Value> {
        // arm 前完成可失败步骤（句柄通道参数为值传递，无脚本构造步骤）
        self.clear_terminate_flag();
        let window = self.budget.next_tick_limit().max(1.0);
        self.watchdog
            .arm(Instant::now() + Duration::from_millis(window as u64));
        let out = call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.run_expr,
            &[code.to_string()],
        );
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        match out {
            Ok(v) => {
                // 独立表达式也可能触发近堆回调，成功返回后仍需恢复上限。
                self.restore_heap_limit();
                Ok(v)
            }
            Err(_) => {
                // 独立通道被掐断：恢复堆上限即可，不动桶、不碰 tick 生命周期状态
                self.restore_heap_limit();
                Ok(serde_json::json!({ "ok": false, "kind": "terminated", "error": "硬中断" }))
            }
        }
    }

    /// 解除期限、清终止标志、恢复堆上限，并暂存完整预算费用、清除计时起点。
    /// 同一 tick 重复调用不会再次生成费用；真正扣桶由 settle_bill 完成。
    fn after_termination(&mut self) {
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        if !self.terminated_this_tick {
            // 费用只暂存，提交点再修改桶余额。
            self.pending_bill = Some(self.budget.tick_limit().ceil());
            self.terminated_this_tick = true;
        }
        self.restore_heap_limit();
        *self.shared.tick_start.lock().unwrap() = None;
        *self.shared.thread_cpu_start.lock().unwrap() = None;
    }

    /// 清除 tick 外调用可能遗留的终止标志，并恢复近堆回调扩大的上限。
    /// 不修改计费候选，也不解除看门狗期限。
    fn clear_terminate_flag(&mut self) {
        self.runtime.v8_isolate().cancel_terminate_execution();
        self.restore_heap_limit();
    }

    /// 看门狗句柄（测试驱动真实看门狗线程的代际守卫用）。
    pub fn watchdog(&self) -> &Watchdog {
        &self.watchdog
    }

    /// 近堆回调是否在本 tick 触发过（诊断：区分内存闸门与看门狗兜底路径）。
    pub fn near_heap_fired(&self) -> bool {
        self.shared.near_heap_fired.load(Ordering::Relaxed)
    }

    /// V8 当前生效的堆上限。近堆回调的返回值会把它持久抬高（remove+re-add 恢复），
    /// 测试据此断言"触发后恢复到配置值"而非仅凭实例复用。
    pub fn heap_size_limit_bytes(&mut self) -> usize {
        self.runtime
            .v8_isolate()
            .get_heap_statistics()
            .heap_size_limit()
    }

    /// 本 tick 在执行线程上消耗的 CPU 毫秒数，不含指令费用。
    pub fn cpu_used_ms(&self) -> anyhow::Result<f64> {
        let start = *self.shared.thread_cpu_start.lock().unwrap();
        match start {
            Some(start) => Ok(crate::cpu_clock::thread_cpu_time()?
                .saturating_sub(start)
                .as_secs_f64()
                * 1000.0),
            None => Ok(0.0),
        }
    }

    pub fn drain_console(&mut self) -> Vec<LogLine> {
        std::mem::take(&mut *self.shared.console.lock().unwrap())
    }

    pub fn drain_missing_api(&mut self) -> anyhow::Result<serde_json::Value> {
        let out = call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.drain_missing,
            &[],
        );
        // 诊断导出没有看门狗窗口；成功或失败后都恢复近堆回调可能扩大的上限。
        // missing 的属性名由玩家访问产生，输出量可能较大。
        self.restore_heap_limit();
        out.inspect_err(|_| self.clear_terminate_flag())
    }

    pub fn drain_stub_calls(&mut self) -> anyhow::Result<serde_json::Value> {
        let out = call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.drain_stub_calls,
            &[],
        );
        self.restore_heap_limit();
        out.inspect_err(|_| self.clear_terminate_flag())
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
        THREAD_ORDER.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.last() != Some(&self.seq) {
                // 销毁顺序不满足本线程创建栈时记录并告警；告警本身不能修复 V8 的 Enter/Exit 顺序。
                eprintln!(
                    "kk-runner: 线程内销毁顺序违反 LIFO（seq={}，栈顶={:?}）。V8 的 isolate Enter/Exit 栈是线程局部的，同线程多实例必须按创建逆序销毁！",
                    self.seq,
                    stack.last()
                );
                LIFO_VIOLATIONS.fetch_add(1, Ordering::SeqCst);
                stack.retain(|s| *s != self.seq);
            } else {
                stack.pop();
            }
        });
    }
}

mod host;
mod inject;
mod prelude;
use host::*;

#[cfg(test)]
mod command_deadline_tests;
