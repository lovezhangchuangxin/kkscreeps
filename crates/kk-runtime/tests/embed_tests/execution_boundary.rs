//! 两种投影通道都必须由运行时统一提交世界与玩家候选。
use super::*;
use std::collections::BTreeMap;

pub(super) fn multi_player_writeback_retry_impl() -> anyhow::Result<()> {
    for kkpv in [false, true] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(kkpv);
        for player in ["p1", "p2"] {
            e.set_sandbox_config_for_test(
                player,
                kk_runner::SandboxConfig {
                    initial_bucket: 1000.0,
                    ..Default::default()
                },
            );
        }
        e.create_shard(&json!({
            "players": ["p1", "p2"],
            "rooms": [
                {"name": "W1N1", "owner": "p1", "rcl": 3},
                {"name": "W2N1", "owner": "p2", "rcl": 3}
            ]
        }))?;
        e.set_player_concurrency(2)?;
        let mut ids = Vec::new();
        for (player, room) in [("p1", "W1N1"), ("p2", "W2N1")] {
            ids.push(e.place_object(
                room,
                "creep",
                &json!({
                    "pos": [10, 10], "owner": player, "name": player, "body": ["move"]
                }),
            )?);
        }
        let before: Vec<_> = ["p1", "p2"]
            .into_iter()
            .map(|p| e.player_state_for_test(p).unwrap().clone())
            .collect();
        let objects: Vec<_> = ids.iter().map(|id| e.get_object(id).unwrap()).collect();
        let code = r#"
            globalThis.n = (globalThis.n || 0) + 1;
            Memory.n = globalThis.n;
            RawMemory.segments[5] = 'candidate';
            RawMemory.setActiveSegments([5]);
            for (const c of Object.values(Game.creeps)) c.move(3);
            globalThis.n
        "#;
        let codes = BTreeMap::from([("p1".into(), code.into()), ("p2".into(), code.into())]);
        // segments 位于 CPU 和 RawMemory 候选之后，验证整个提交仍保持原子性。
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::Segments);
        assert!(e.run_players(&codes).is_err());
        assert!(e.has_pending_round());
        assert_eq!(e.get_game_time(), 0);
        for (index, player) in ["p1", "p2"].into_iter().enumerate() {
            assert_eq!(e.player_state_for_test(player).unwrap(), &before[index]);
            assert_eq!(e.get_object(&ids[index]).unwrap(), objects[index]);
        }
        let replacement = BTreeMap::from([
            ("p1".into(), "throw new Error('must not run')".into()),
            ("p2".into(), "throw new Error('must not run')".into()),
        ]);
        let runs = e.run_players(&replacement)?;
        assert!(!e.has_pending_round());
        assert_eq!(e.get_game_time(), 1);
        for (index, player) in ["p1", "p2"].into_iter().enumerate() {
            assert_eq!(runs[player].expr["value"], 1);
            let state = e.player_state_for_test(player).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&state.raw_memory)?["n"],
                1
            );
            assert_eq!(state.segments[&5], "candidate");
            assert_eq!(state.active_request, vec![5]);
            assert_ne!(state.cpu, before[index].cpu);
            assert_eq!(e.get_object(&ids[index]).unwrap()["pos"]["x"], 11);
        }
        let read = BTreeMap::from([
            (
                "p1".into(),
                "[globalThis.n,Memory.n,RawMemory.segments[5]]".into(),
            ),
            (
                "p2".into(),
                "[globalThis.n,Memory.n,RawMemory.segments[5]]".into(),
            ),
        ]);
        for run in e.run_players(&read)?.values() {
            assert_eq!(run.expr["value"], json!([1, 1, "candidate"]));
        }
    }
    Ok(())
}
