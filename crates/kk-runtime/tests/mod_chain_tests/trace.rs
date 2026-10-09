//! 运行 trace：槽结果（完成/拒绝/故障）与回退生命周期（整轮重试、
//! realm 重建、自动停用）的事件序、定位字段与默认关闭语义。
use super::*;

/// 按 event 的判别名过滤快照条目（serde 视图不可用，用模式匹配）。
fn kinds(entries: &[kk_runtime::ModTraceEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match &entry.event {
            kk_runtime::ModTraceEvent::SlotCompleted => "slot_completed".into(),
            kk_runtime::ModTraceEvent::SlotInactive => "slot_inactive".into(),
            kk_runtime::ModTraceEvent::SlotDeclined { .. } => "slot_declined".into(),
            kk_runtime::ModTraceEvent::SlotFault { .. } => "slot_fault".into(),
            kk_runtime::ModTraceEvent::SettlementRetry { .. } => "settlement_retry".into(),
            kk_runtime::ModTraceEvent::RealmRebuilt => "realm_rebuilt".into(),
            kk_runtime::ModTraceEvent::AutoDisabled { .. } => "auto_disabled".into(),
        })
        .collect()
}

#[test]
fn trace_is_off_by_default_and_snapshots_only_after_enabling() -> anyhow::Result<()> {
    let mut e = engine(
        &[(
            "healthy",
            package(
                "api.handlers.after('builtin:link.tick',{id:'healthy'},ctx=>{ctx.state={ok:true};});",
            ),
        )],
        false,
    )?;
    link(&mut e, 20, 0, 0)?;
    assert!(
        e.mod_trace_snapshot().is_none(),
        "默认关闭：无 trace 记录点也无快照"
    );
    e.enable_mod_trace();
    e.tick(1)?;
    let (entries, dropped) = e.mod_trace_snapshot().expect("开启后有快照");
    assert_eq!(dropped, 0);
    assert!(!entries.is_empty(), "健康链必须产生完成事件");
    assert!(
        kinds(&entries).iter().all(|k| k == "slot_completed"),
        "健康链只有完成事件：{entries:?}"
    );
    // 槽在提交前的世界时间执行：结算 tick N 记录为 N（提交后游戏时间 N+1）。
    assert!(entries.iter().all(|entry| entry.tick == 1
        && entry.room == "W1N1"
        && entry.slot == "builtin:link.tick"
        && entry.stage == "object_tick"));
    Ok(())
}

#[test]
fn trace_records_decline_with_binding_and_code() -> anyhow::Result<()> {
    // 与 decline.rs 同手法：wrap 的字段同步断言依赖特定前置链，这里
    // 只关心拒绝事件本身。
    let source = decline_package("return api.decline('full');").replace(
        "if (fields.value!==7) throw Error('next 后字段未同步');",
        "",
    );
    let mut e = engine(&[("a", source)], false)?;
    e.enable_mod_trace();
    let id = e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
    e.run_player(
        "p1",
        &format!(
            "for(const key of ['a:a-accept','a:b-test','a:c-accept'])__kkEmitExtensionIntent(key,'{id}',{{}});'done'"
        ),
    )?;
    let (entries, _) = e.mod_trace_snapshot().expect("开启后有快照");
    let declines: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.event {
            kk_runtime::ModTraceEvent::SlotDeclined { binding, code } => {
                Some((entry, binding.clone(), code.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(declines.len(), 1, "只有 b-test 拒绝：{entries:?}");
    let (entry, binding, code) = &declines[0];
    assert_eq!(
        binding, "a:b-test.process",
        "拒绝绑定指向调用 api.decline 的处理器"
    );
    assert_eq!(code, "full");
    assert_eq!(entry.slot, "a:b-test.slot");
    assert_eq!(entry.stage, "object_action");
    assert_eq!(entry.target, id);
    // 同轮其余动作完成：拒绝只回退本动作候选。
    assert_eq!(
        kinds(&entries)
            .iter()
            .filter(|k| *k == "slot_completed")
            .count(),
        2
    );
    Ok(())
}

#[test]
fn trace_records_fault_retry_rebuild_and_auto_disable_sequence() -> anyhow::Result<()> {
    let options = RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(1),
        ..Default::default()
    };
    let mut e = engine_with_options(
        &[(
            "bad",
            package(
                "api.handlers.replace('builtin:link.tick',{id:'bad'},()=>{throw Error('bad-tick')});",
            ),
        )],
        false,
        &[],
        options,
    )?;
    e.enable_mod_trace();
    let id = link(&mut e, 20, 0, 10)?;
    // 第一轮：槽故障 → 整轮缓存 → 自动停用（阈值 1）。
    assert!(e.tick(1).is_err());
    // 第二轮：重建 realm → 新链（bad 已停用）完成。
    e.tick(1)?;
    let (entries, dropped) = e.mod_trace_snapshot().expect("开启后有快照");
    assert_eq!(dropped, 0);
    // 场景确定性下锁定完整事件序（不止首次出现顺序）。
    assert_eq!(
        kinds(&entries),
        vec![
            "slot_fault",
            "auto_disabled",
            "settlement_retry",
            "realm_rebuilt",
            "slot_completed",
        ],
        "故障→停用→缓存重试→重建→新链完成，恰此五条：{entries:?}"
    );
    let fault = entries
        .iter()
        .find_map(|entry| match &entry.event {
            kk_runtime::ModTraceEvent::SlotFault {
                binding,
                kind,
                error,
            } => Some((entry, binding.clone(), *kind, error.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(fault.0.slot, "builtin:link.tick");
    assert_eq!(fault.0.target, id);
    assert_eq!(fault.1.as_deref(), Some("bad:handler.bad"));
    assert_eq!(fault.2, "handler", "kind 用 serde snake_case 名");
    assert!(
        fault.3.contains("bad-tick"),
        "错误文本保留定位：{}",
        fault.3
    );
    if let kk_runtime::ModTraceEvent::AutoDisabled {
        mod_id,
        consecutive,
        disabled,
    } = &entries
        .iter()
        .find_map(|entry| match &entry.event {
            event @ kk_runtime::ModTraceEvent::AutoDisabled { .. } => Some(event),
            _ => None,
        })
        .unwrap()
    {
        assert_eq!(mod_id, "bad");
        assert_eq!(*consecutive, 1);
        assert_eq!(disabled, &vec!["bad".to_string()]);
    }
    let rebuilt = entries
        .iter()
        .find(|entry| matches!(entry.event, kk_runtime::ModTraceEvent::RealmRebuilt))
        .unwrap();
    assert_eq!(
        rebuilt.tick, 1,
        "重建发生在下一次结算入口（世界时间为热身后的 1），槽/目标为空"
    );
    assert_eq!(rebuilt.slot, "");
    Ok(())
}

#[test]
fn trace_survives_realm_rebuilds_and_keeps_bounded_snapshot() -> anyhow::Result<()> {
    let options = RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(1),
        ..Default::default()
    };
    let mut e = engine_with_options(
        &[(
            "bad",
            package(
                "api.handlers.before('builtin:link.tick',{id:'bad'},()=>{throw Error('bad')});",
            ),
        )],
        false,
        &[],
        options,
    )?;
    e.enable_mod_trace();
    link(&mut e, 20, 0, 10)?;
    // 交替故障/成功：bad 停用后链变健康；trace 跨重建持续记录。
    assert!(e.tick(1).is_err());
    e.tick(1)?;
    let (entries, _) = e.mod_trace_snapshot().expect("开启后有快照");
    assert!(entries.len() >= 3, "重建不丢历史：{entries:?}");
    // 缓冲上限在 mod_trace 单测覆盖；此处断言快照 API 形状（克隆不互相影响）。
    let again = e.mod_trace_snapshot().expect("快照可重复取");
    assert_eq!(again.0.len(), entries.len());
    Ok(())
}

#[test]
fn trace_marks_disabled_namespace_slots_as_inactive_not_completed() -> anyhow::Result<()> {
    let options = RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(1),
        ..Default::default()
    };
    // 自有动作包（decline_package 的 throw 形态）：故障一次即停用；
    // 之后同对象动作的槽被丢弃——记 SlotInactive 而非 Completed。
    let own = decline_package("throw Error('own-fault');");
    let healthy = package(
        "api.handlers.after('builtin:link.tick',{id:'healthy'},ctx=>{ctx.state={ok:true};});",
    );
    let mut e = engine_with_options(&[("a", own), ("healthy", healthy)], false, &[], options)?;
    e.enable_mod_trace();
    let id = e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
    link(&mut e, 20, 0, 0)?;
    assert!(e
        .run_player(
            "p1",
            &format!(
                "for(const key of ['a:a-accept','a:b-test','a:c-accept'])__kkEmitExtensionIntent(key,'{id}',{{}});'first'"
            )
        )
        .is_err());
    // 停用后的重试轮（缓存重放，不重跑玩家代码）：a 的三个自有槽
    // 全部丢弃记 SlotInactive，内建槽正常完成，无新故障。
    let retried = e.run_player("p1", "throw Error('不得重跑')")?;
    assert_eq!(retried.expr["value"], "first", "重试提交缓存轮结果");
    let (entries, _) = e.mod_trace_snapshot().expect("开启后有快照");
    let inactive: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry.event, kk_runtime::ModTraceEvent::SlotInactive))
        .collect();
    assert_eq!(
        inactive.len(),
        3,
        "a 的三个动作槽在停用后全部丢弃：{entries:?}"
    );
    for entry in &inactive {
        assert_eq!(entry.stage, "object_action");
        assert_eq!(entry.target, id);
        assert!(entry.slot.starts_with("a:"), "{}", entry.slot);
    }
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry.event, kk_runtime::ModTraceEvent::SlotFault { .. }))
            .count(),
        1,
        "重试轮不产生新故障：{entries:?}"
    );
    Ok(())
}
