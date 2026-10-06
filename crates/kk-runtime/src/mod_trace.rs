//! Mod 运行 trace：可选、有界的槽调用与回退事件记录。
//!
//! 事件在两个汇聚点产生：槽执行器（`embed/mod_chain.rs`，每次整槽调用
//! 的结果）与结算编排（`embed/execution.rs`/`embed.rs`，整轮失败、realm
//! 重建与自动停用）。所有记录点先经 `trace_enabled()` 判空门控——关闭
//! 时既无缓冲也无条目构造。条目只含定位信息（tick/房间/目标/槽键/
//! 阶段）与结果摘要——不记录参数、字段值或玩家 Memory；错误文本可能
//! 引用被拒参数（结算诊断原文），属服务端诊断而非玩家可见面。trace
//! 不进检查点：恢复后的分片从新空缓冲开始（开启意愿经引擎挂起标志
//! 跨恢复保留）。trace 是诊断快照，不是确定性回放日志：复现依赖场景
//! 身份与包目录重跑（报告的 scenario_sha256/plan_hash），缓冲超限丢弃
//! 并计数。

/// 缓冲上限。诊断目标是"最近发生了什么"，超限即截尾并计数——
/// 无界缓冲在长跑场景里会吞掉宿主内存，违背"限制记录量"。
pub(crate) const TRACE_CAP: usize = 1024;
/// 单条文本（错误/拒绝理由）截断长度（字符）。诊断需要定位而不是全文。
const TEXT_LIMIT: usize = 200;

/// 一条 trace 事件；生命周期事件（结算重试/重建/停用）无槽与目标，
/// 对应字段为空串。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ModTraceEntry {
    pub tick: u64,
    pub room: String,
    pub target: String,
    pub slot: String,
    pub stage: &'static str,
    pub event: ModTraceEvent,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub enum ModTraceEvent {
    SlotCompleted,
    /// 停用命名空间的槽：动作被丢弃而非执行（自有槽无 native 回退），
    /// 与 Completed 区分以免"看似执行实则丢弃"误导归因。
    SlotInactive,
    /// 正常拒绝：携带拒绝处理器绑定与 code（候选按局部边界回退）。
    SlotDeclined {
        binding: String,
        code: String,
    },
    /// 处理器/候选故障：携带故障绑定（可空——基础设施故障无绑定）、
    /// 故障种类（fault_kind_label 稳定名）与错误文本（截断）；后续按
    /// 整轮丢弃→重建→缓存重试回退。
    SlotFault {
        binding: Option<String>,
        kind: &'static str,
        error: String,
    },
    /// 结算失败，候选已缓存、等待重试（不重跑玩家代码）。仅记引擎
    /// 实际结算的失败；结算入口前的注入故障/装配失败同样缓存候选，
    /// 但那是宿主测试钩子路径，不产生本事件。
    SettlementRetry {
        error: String,
    },
    /// 故障后的 realm 重建成功（固定启动快照，身份核验通过）。
    RealmRebuilt,
    /// 连击达到阈值的自动停用；下次重试起使用新链，native 回退生效。
    /// mod_id 为停用来源（触发故障绑定的命名空间；基础设施故障不触发）。
    AutoDisabled {
        mod_id: String,
        consecutive: u32,
        disabled: Vec<String>,
    },
}

/// 槽计划阶段 → 稳定字符串（报告消费者按此过滤；不要用 Debug 变体名
/// 直出，改枚举名时这里同步）。
pub(crate) fn stage_label(stage: kk_mod_api::Stage) -> &'static str {
    match stage {
        kk_mod_api::Stage::ObjectAction => "object_action",
        kk_mod_api::Stage::Movement => "movement",
        kk_mod_api::Stage::ObjectTick => "object_tick",
        kk_mod_api::Stage::Market => "market",
    }
}

/// 故障种类 → 稳定字符串；与 `ModFaultKind` 的 serde snake_case 同名，
/// 两个诊断面（mod_trace 与 mod_fault）可按此 join。
pub(crate) fn fault_kind_label(kind: kk_mod_host::ModFaultKind) -> &'static str {
    match kind {
        kk_mod_host::ModFaultKind::Handler => "handler",
        kk_mod_host::ModFaultKind::Control => "control",
        kk_mod_host::ModFaultKind::Context => "context",
        kk_mod_host::ModFaultKind::Read => "read",
        kk_mod_host::ModFaultKind::WriteConversion => "write_conversion",
        kk_mod_host::ModFaultKind::Write => "write",
        kk_mod_host::ModFaultKind::Native => "native",
        kk_mod_host::ModFaultKind::Capability => "capability",
        kk_mod_host::ModFaultKind::Resource => "resource",
        kk_mod_host::ModFaultKind::Chain => "chain",
    }
}

pub(crate) fn truncate(text: &str) -> String {
    text.chars().take(TEXT_LIMIT).collect()
}

#[derive(Default)]
pub(crate) struct TraceLog {
    entries: std::collections::VecDeque<ModTraceEntry>,
    dropped: usize,
}

impl TraceLog {
    pub(crate) fn push(&mut self, entry: ModTraceEntry) {
        if self.entries.len() >= TRACE_CAP {
            self.entries.pop_front();
            self.dropped += 1;
        }
        self.entries.push_back(entry);
    }
    /// 快照（克隆）与累计丢弃数；条目保持发生序。
    pub(crate) fn snapshot(&self) -> (Vec<ModTraceEntry>, usize) {
        (self.entries.iter().cloned().collect(), self.dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_log_is_bounded_and_counts_drops() {
        let mut log = TraceLog::default();
        for tick in 0..(TRACE_CAP + 7) as u64 {
            log.push(ModTraceEntry {
                tick,
                room: "W1N1".into(),
                target: String::new(),
                slot: String::new(),
                stage: "",
                event: ModTraceEvent::RealmRebuilt,
            });
        }
        let (entries, dropped) = log.snapshot();
        assert_eq!(entries.len(), TRACE_CAP);
        assert_eq!(dropped, 7, "超限丢弃必须计数");
        assert_eq!(entries.first().unwrap().tick, 7, "截尾保留最近事件");
        assert_eq!(entries.last().unwrap().tick, TRACE_CAP as u64 + 6);
    }

    #[test]
    fn long_texts_are_truncated_at_char_boundary() {
        let text = "错".repeat(500);
        assert_eq!(truncate(&text).chars().count(), TEXT_LIMIT);
        let mixed = format!("{}结尾", "a".repeat(TEXT_LIMIT));
        assert_eq!(truncate(&mixed), "a".repeat(TEXT_LIMIT));
    }

    #[test]
    fn fault_kind_labels_match_serde_names() {
        // 与 ModFaultKind 的 snake_case 序列化同名：逐变体锁定，新增
        // 变体时编译器强制这里同步。
        for (kind, label) in [
            (kk_mod_host::ModFaultKind::Handler, "handler"),
            (
                kk_mod_host::ModFaultKind::WriteConversion,
                "write_conversion",
            ),
            (kk_mod_host::ModFaultKind::Capability, "capability"),
            (kk_mod_host::ModFaultKind::Chain, "chain"),
        ] {
            assert_eq!(
                fault_kind_label(kind),
                serde_json::to_value(kind).unwrap().as_str().unwrap(),
                "故障种类标签须与 serde 名一致"
            );
            assert_eq!(fault_kind_label(kind), label);
        }
    }
}
