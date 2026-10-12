//! 接手复审的故障/权限/真实结算回归，复用命令域夹具。
use super::*;

#[test]
fn failed_expression_barrier_cannot_be_bypassed_by_stop_save() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("expr-fence-fault", "exports.loop = () => {};");
    inject_command(
        &host,
        &ident,
        "console.expr",
        json!({"expression": "Memory.count = (Memory.count || 0) + 1"}),
    );
    host.drain_command_prefix(4)?;
    let checkpoints = host.dir.checkpoints_dir();
    let saved = host.dir.root().join("saved-checkpoints");
    std::fs::rename(&checkpoints, &saved)?;
    std::fs::write(&checkpoints, b"block checkpoint publication")?;
    assert!(host.tick().is_err(), "世界提交后屏障发布失败");
    assert!(matches!(host.fault(), Some(Fault::Persistence { .. })));
    std::fs::remove_file(&checkpoints)?;
    std::fs::rename(&saved, &checkpoints)?;
    let before = checkpoints::list_seqs(&checkpoints)?;
    host.request_stop();
    assert!(matches!(
        host.finish_stop(),
        crate::service::host::StopOutcome::SaveFailed { .. }
    ));
    assert_eq!(
        checkpoints::list_seqs(&checkpoints)?,
        before,
        "停止不能保存效果与旧水位"
    );
    assert!(matches!(
        entry_status(&host, &ident, 1),
        inbox::EntryStatus::Received
    ));
    let anchor = effects::load(&host.dir.effects_file(), &ident.world_id)?;
    assert_eq!(anchor.anchor.barrier_id, "b-init");
    Ok(())
}

#[test]
fn expression_hard_interrupt_skips_main_and_next_tick_remains_live() -> anyhow::Result<()> {
    for (tag, source) in [("halt", "Game.cpu.halt()"), ("timeout", "while (true) {};")] {
        let (mut host, ident) = host_v2(
            tag,
            "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };",
        );
        inject_command(&host, &ident, "console.expr", json!({"expression": source}));
        host.drain_command_prefix(4)?;
        let report = host.tick()?.expect("硬中断是玩家结局，世界照常提交");
        assert_eq!(report.player_errors.len(), 1);
        match entry_status(&host, &ident, 1) {
            inbox::EntryStatus::Completed { result } => assert_eq!(result["kind"], "terminated"),
            other => panic!("中断结果应完成: {other:?}"),
        }
        let raw = &host.engine.player_state("p1").unwrap().raw_memory;
        let memory: Value = serde_json::from_str(if raw.is_empty() { "{}" } else { raw })?;
        assert!(memory["n"].is_null(), "中断后不能运行主循环");
        run_one_player(&mut host);
        let memory: Value =
            serde_json::from_str(&host.engine.player_state("p1").unwrap().raw_memory)?;
        assert_eq!(memory["n"], 1, "下一拍主循环恢复运行");
    }
    Ok(())
}

#[test]
fn host_input_retry_pins_effect_and_audit_replays_it() -> anyhow::Result<()> {
    for channel in [false, true] {
        let (mut host, _, id) = host_mod(if channel { "host-kkpv" } else { "host-json" })?;
        let e = &mut host.engine;
        e.set_kkpv_channel(channel);
        let before = e.checkpoint(Value::Null)?;
        let log = host.dir.root().join("audit.jsonl");
        e.enable_input_log(&log, json!({}))?;
        let input = crate::HostIntent {
            player: "p1".into(),
            target: id.clone(),
            intent: "workload:increment".into(),
            args: json!({"amount": 1}),
        };
        e.fail_next_settle_for_test();
        assert!(
            e.run_players_with_inputs(&BTreeMap::new(), &BTreeMap::new(), &[input])
                .is_err()
        );
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 0);
        // 调用方漏传输入，仍须提交失败轮原输入；日志也保留这一真实效果。
        e.run_players(&BTreeMap::new())?;
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 1);
        let data = crate::embed::read_input_log(&log)?;
        assert_eq!(data.segments.len(), 1);
        let live_engine = e.checkpoint(Value::Null)?["engine"].clone();
        let packages = kk_mod_host::discover_packages(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/fixtures/mod-player"),
        )?;
        let mut replay = EmbedEngine::new();
        replay.restore_checkpoint(before, Some((&packages, &Default::default())))?;
        replay.replay_input_rounds(&data.header, &data.segments)?;
        assert_eq!(
            replay.checkpoint(Value::Null)?["engine"],
            live_engine,
            "宿主输入回放须保持完整引擎状态，包括账户和 CPU 桶"
        );
        assert_eq!(replay.get_object(&id).unwrap()["fields"]["value"], 1);
        let replay_state = replay.engine_mod_state("workload");
        drop(replay);
        assert_eq!(replay_state, e.engine_mod_state("workload"));
        // 玩家同槽提交覆盖宿主：同一 tick 只加一次。
        let input = crate::HostIntent {
            player: "p1".into(),
            target: id.clone(),
            intent: "workload:increment".into(),
            args: json!({"amount": 1}),
        };
        e.run_players_with_inputs(
            &BTreeMap::from([(
                "p1".into(),
                format!("__kkEmitExtensionIntent('workload:increment', '{id}', {{amount:1}})"),
            )]),
            &BTreeMap::new(),
            &[input],
        )?;
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 2);
    }
    Ok(())
}

#[test]
fn mod_input_checks_current_plan_schema_receiver_and_owner() -> anyhow::Result<()> {
    let (mut host, ident, id) = host_mod("mod-current-gates")?;
    let foreign = host.engine.place_mod_object(
        "W1N1",
        "workload:counter",
        &json!({"owner":"p2", "x":11, "y":10}),
    )?;
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    inbox::accept::open_stream(&mut state, "workload", "m1").map_err(|e| anyhow::anyhow!(e))?;
    for (index, (intent, target, args)) in [
        ("workload:missing", id.as_str(), json!({"amount":1})),
        ("workload:increment", id.as_str(), json!({"amount":2})),
        ("workload:increment", "kkffffffff", json!({"amount":1})),
        ("workload:increment", foreign.as_str(), json!({"amount":1})),
    ]
    .into_iter()
    .enumerate()
    {
        let envelope = json!({"worldId":ident.world_id, "principal":"workload", "streamId":"m1", "clientSeq":index + 1,
            "kind":"mod.input", "schemaVersion":1, "payload":{"intent":intent, "asPlayer":"p1", "targetId":target, "args":args}});
        assert!(matches!(
            inbox::accept::accept(&mut state, &envelope, &Default::default(), 1),
            inbox::accept::AcceptOutcome::Received { .. }
        ));
    }
    inbox::store(&state, &host.dir.inbox_file())?;
    assert_eq!(host.drain_command_prefix(4)?, 4);
    for seq in 1..=4 {
        assert!(matches!(
            entry_status_on(&host, &ident, "m1", seq),
            inbox::EntryStatus::Rejected { .. }
        ));
    }
    assert!(host.pending_mod_input.is_empty());
    assert_eq!(host.engine.get_object(&id).unwrap()["fields"]["value"], 0);
    Ok(())
}
