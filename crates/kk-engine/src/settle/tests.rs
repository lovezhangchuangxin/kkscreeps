use super::*;

#[test]
fn memory_writebacks_publish_with_market_and_retry_without_partial_commit() {
    let stages = [
        TickStage::Prepare,
        TickStage::Settle,
        TickStage::Market,
        TickStage::Validate,
        TickStage::Publish,
    ]
    .map(|stage| (Some(stage), None));
    let steps = [
        WritebackStep::Market,
        WritebackStep::Cpu,
        WritebackStep::RawMemory,
        WritebackStep::Segments,
    ]
    .map(|step| (None, Some(step)));
    for (stage, step) in stages.into_iter().chain(steps) {
        let mut engine = Engine::default();
        engine.players.insert(
            "p1".into(),
            PlayerState {
                money: 10_000_000,
                gcl: 17,
                raw_memory: "old".into(),
                segments: BTreeMap::from([(5, "old segment".into())]),
                ..Default::default()
            },
        );
        let original = engine.players.clone();
        let market = engine.market.clone();
        let generation = engine.world.generation();
        let next_id = engine.next_id;
        let mut batch = IntentBatch::default();
        batch.users.entry("p1".into()).or_default().market.push(
            crate::intent::MarketIntent::CreateOrder {
                order_type: Some("sell".into()),
                resource_type: Some("token".into()),
                price_milli: Some(1000),
                total_amount: Some(100),
                room_name: None,
            },
        );
        let patches = BTreeMap::from([(
            "p1".into(),
            PlayerWriteback {
                cpu: Some(kk_core::cpu::Bucket::new(20.0, 10000.0, 500.0, 13.0)),
                raw_memory: Some("new".into()),
                segments: Some(SegmentWriteback::from_runner(&json!({
                    "writes": [[5, "new segment"]], "active": [5], "public": [5],
                    "defaultPublic": 5, "foreign": {"username": "p2", "id": 7}
                }))),
                ism_local: None,
            },
        )]);
        let input = TickInput {
            now_ms: 1234,
            warmup: false,
        };
        engine.set_fail_stage_for_test(stage);
        engine.set_fail_writeback_for_test(step);
        assert!(
            engine
                .tick_with_writebacks(batch.clone(), input, patches.clone())
                .is_err()
        );
        assert_eq!(engine.players, original, "{stage:?}");
        assert_eq!(engine.market, market, "{stage:?}");
        assert_eq!(engine.world.time(), 0);
        assert_eq!(engine.world.generation(), generation);
        assert_eq!(engine.next_id, next_id);
        engine.set_fail_stage_for_test(None);
        engine.set_fail_writeback_for_test(None);
        engine.tick_with_writebacks(batch, input, patches).unwrap();
        let player = &engine.players["p1"];
        assert_eq!(player.cpu.bucket(), 13.0);
        assert_eq!(player.raw_memory, "new");
        assert_eq!(player.segments[&5], "new segment");
        assert_eq!(player.active_request, vec![5]);
        assert_eq!(player.public_ids, vec![5]);
        assert_eq!(player.default_public, Some(5));
        assert_eq!(player.foreign_request, Some(("p2".into(), Some(7))));
        assert_eq!(player.gcl, 17);
        assert_eq!(player.money, 9_995_000, "内存写回不能覆盖市场扣费");
        assert_eq!(engine.market.orders.len(), 1);
        assert_eq!(engine.world.time(), 1);
    }
}

#[test]
fn unknown_writeback_player_rejects_entire_tick() {
    let mut engine = Engine::default();
    engine.players.entry("p1".into()).or_default();
    let original = engine.players.clone();
    let patches = BTreeMap::from([
        (
            "p1".into(),
            PlayerWriteback {
                raw_memory: Some("new".into()),
                ..Default::default()
            },
        ),
        ("unknown".into(), PlayerWriteback::default()),
    ]);
    assert!(
        engine
            .tick_with_writebacks(
                IntentBatch::default(),
                TickInput {
                    now_ms: 0,
                    warmup: false
                },
                patches
            )
            .is_err()
    );
    assert_eq!(engine.players, original);
    assert_eq!(engine.world.time(), 0);
}

#[test]
fn restored_room_uses_directory_name_during_settlement() {
    let mut world = kk_world::World::new(1);
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    let room = world.room("W1N1").unwrap().clone();
    let restored = RoomState::decode_snapshot(&room.encode_snapshot(0, 1, 0)).unwrap();
    assert!(restored.name.is_empty());
    world.replace_room("W1N1".into(), restored).unwrap();
    let mut engine = Engine::new(world);
    let mut batch = IntentBatch::default();
    batch.add_room_intent(
        "p1",
        crate::intent::RoomIntent::CreateFlag {
            room: "W1N1".into(),
            x: 10,
            y: 10,
            name: "restored".into(),
            color: 1,
            secondary_color: 1,
        },
    );
    engine.tick_with_intents(batch).unwrap();
    let room = engine.world.room("W1N1").unwrap();
    assert_eq!(room.name, "W1N1");
    assert!(
        room.objects
            .values()
            .any(|o| matches!(o, RoomObject::Flag(f) if f.name == "restored"))
    );
}

/// 成功 tick 的全部房间写回只使世界版本递增一次；失败不递增。
#[test]
fn engine_tick_bumps_generation_exactly_once() {
    let mut world = kk_world::World::new(1);
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    let base = world.generation();
    let mut engine = Engine::new(world);
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(engine.world.generation(), base + 1, "一次成功 tick 恰 +1");
    engine.fail_room = Some("W1N1".into());
    let before = engine.world.generation();
    assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
    assert_eq!(engine.world.generation(), before, "失败 tick 不 bump");
}

// 后一个房间失败时，前一个房间的资源和再生时钟也不能写回。
#[test]
fn engine_tick_rolls_back_all_rooms_on_failure() {
    let mut world = kk_world::World::new(1);
    for name in ["W1N1", "W2N2"] {
        world
            .register_room(name, kk_core::Terrain::plains())
            .unwrap();
        world
            .insert(
                name,
                RoomObject::Source(kk_schema::room::SourceObj {
                    id: 0,
                    x: 5,
                    y: 5,
                    energy: 1000,
                    energy_capacity: 3000,
                    next_regeneration_time: None,
                }),
            )
            .unwrap();
    }
    let mut engine = Engine::new(world);
    engine.fail_room = Some("W2N2".into());
    assert!(
        engine.tick_with_intents(IntentBatch::default()).is_err(),
        "注入房 Err ⇒ 整体 Err"
    );
    assert_eq!(engine.world.time(), 0, "time 不推进");
    for name in ["W1N1", "W2N2"] {
        match engine.world.room(name).unwrap().objects.values().next() {
            Some(RoomObject::Source(s)) => {
                assert_eq!(s.energy, 1000, "{name}：能量未被动过");
                assert_eq!(s.next_regeneration_time, None, "{name}：重生钟未挂");
            }
            _ => panic!("{name}：非 source"),
        }
    }
    // 故障解除后正常推进：两房同一 tick 挂钟（time=0 → 300）
    engine.fail_room = None;
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(engine.world.time(), 1);
    for name in ["W1N1", "W2N2"] {
        match engine.world.room(name).unwrap().objects.values().next() {
            Some(RoomObject::Source(s)) => {
                assert_eq!(s.next_regeneration_time, Some(300), "{name}：挂钟")
            }
            _ => panic!("{name}：非 source"),
        }
    }
}

/// 成功后可继续推进；失败后可重试或放弃，世界已修改时两者均被拒绝。
#[test]
fn stage_machine_transitions_recovery_and_mutation_gate() {
    let mut world = kk_world::World::new(1);
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    let mut engine = Engine::new(world);
    assert_eq!(engine.run_state(), &RunState::Ready);
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        engine.run_state(),
        &RunState::Committed { tick: 1 },
        "成功轮落 Committed（可直接开下一轮）"
    );
    // Settle 段失败 → NeedsRecovery；世界未变 ⇒ 重试放行（注入仍在则再失败）
    engine.set_fail_stage_for_test(Some(TickStage::Settle));
    assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
    assert!(matches!(
        engine.run_state(),
        RunState::NeedsRecovery {
            stage: TickStage::Settle,
            tick: 1,
            ..
        }
    ));
    assert!(
        engine.tick_with_intents(IntentBatch::default()).is_err(),
        "注入仍在 ⇒ 重试再失败（世界未变 ⇒ 门放行）"
    );
    engine.set_fail_stage_for_test(None);
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(engine.run_state(), &RunState::Committed { tick: 2 });
    // 放弃恢复：净态拒绝；NeedsRecovery（世界未变）→ Ready 后可正常推进
    assert!(engine.abandon_recovery().is_err(), "净态无需恢复");
    engine.set_fail_stage_for_test(Some(TickStage::Validate));
    assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
    engine.set_fail_stage_for_test(None);
    engine.abandon_recovery().unwrap();
    assert_eq!(engine.run_state(), &RunState::Ready);
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(engine.world.time(), 3);
    // 失败后修改世界，使版本检查拒绝重试和放弃恢复。
    engine.set_fail_stage_for_test(Some(TickStage::Publish));
    assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
    engine.set_fail_stage_for_test(None);
    let g = engine.world.generation();
    engine
        .world
        .register_room("W9N9", kk_core::Terrain::plains())
        .unwrap();
    assert_ne!(
        engine.world.generation(),
        g,
        "夹具前提：register_room bump 代际（否则变异门测试失真）"
    );
    let err = engine
        .tick_with_intents(IntentBatch::default())
        .unwrap_err();
    assert!(err.to_string().contains("外部变异"), "{err}");
    assert!(engine.abandon_recovery().is_err(), "变异后放弃恢复同样被拒");
}

/// 逐个阶段注入错误，检查世界、玩家资金和市场仍是提交前状态。
/// 成功对照会使订单过期并退款，因此后两个阶段的失败能检验市场副本隔离。
#[test]
fn stage_injection_keeps_world_and_market_at_previous_tick() {
    let make = || {
        let mut world = kk_world::World::new(1);
        world
            .register_room("W1N1", kk_core::Terrain::plains())
            .unwrap();
        let mut engine = Engine::new(world);
        engine.players.entry("p1".to_string()).or_default().money = 0;
        let id = engine.market.next_order_id();
        engine.market.insert_order(market::MarketOrder {
            id,
            user: "p1".to_string(),
            active: true,
            order_type: "sell".to_string(),
            resource_type: "energy".to_string(),
            price: 1000,
            amount: 0,
            remaining_amount: 100,
            total_amount: 100,
            room_name: Some("W1N1".to_string()),
            created: 0,
            // 时间戳为 0，在当前墙钟下已超过 30 天，会触发移除和退款。
            created_timestamp: 0,
        });
        engine
    };
    // 先确认无故障时会移除订单并退款，避免对未执行的规则只断言状态不变。
    let mut probe = make();
    probe.tick_with_intents(IntentBatch::default()).unwrap();
    assert!(
        probe.market.orders.is_empty(),
        "成功轮订单过期移除（fixture 判别力前提）"
    );
    assert!(probe.players["p1"].money > 0, "成功轮退款入账");
    // 注入轮：五段逐一——世界（time/代际/事件）、玩家 money、市场订单簿
    // 全部保持上一 tick
    for stage in [
        TickStage::Prepare,
        TickStage::Settle,
        TickStage::Market,
        TickStage::Validate,
        TickStage::Publish,
    ] {
        let mut engine = make();
        let (t0, g0) = (engine.world.time(), engine.world.generation());
        engine.set_fail_stage_for_test(Some(stage));
        let err = engine
            .tick_with_intents(IntentBatch::default())
            .unwrap_err();
        assert!(err.to_string().contains(stage.label()), "{stage:?}: {err}");
        assert_eq!(
            (engine.world.time(), engine.world.generation()),
            (t0, g0),
            "{stage:?}: 世界保持上一 tick（time/代际均不动）"
        );
        assert!(engine.last_events.is_empty(), "{stage:?}: 事件不落账");
        assert_eq!(
            engine.market.orders.len(),
            1,
            "{stage:?}: 订单簿保持（市场真候选——publish 前不可回退写入不存在）"
        );
        assert!(
            engine.market.transactions.is_empty(),
            "{stage:?}: 交易流水保持"
        );
        assert_eq!(engine.players["p1"].money, 0, "{stage:?}: money 保持");
        assert!(
            matches!(engine.run_state(),
                RunState::NeedsRecovery { stage: s, .. } if *s == stage),
            "{stage:?}: NeedsRecovery 记录失败段"
        );
        // 解除注入后重新结算：订单移除且退款生效。此入口会重新读取墙钟。
        engine.set_fail_stage_for_test(None);
        engine.tick_with_intents(IntentBatch::default()).unwrap();
        assert!(
            matches!(engine.run_state(), RunState::Committed { tick: 1 }),
            "{stage:?}: 恢复后 Committed"
        );
        assert!(
            engine.market.orders.is_empty(),
            "{stage:?}: 重试恰一次消费（订单移除不重复）"
        );
        assert!(
            engine.players["p1"].money > 0,
            "{stage:?}: 重试恰一次退款入账"
        );
    }
}

/// 市场建单后注入失败，完整市场状态（含私有计数器）与资金均保持不变。
/// 重试只建一张 mkt1 并扣一次费；下一轮建单使用 mkt2。
/// 使用 token 订单免去 terminal 条件，集中检验建单、序号和扣费的提交时点。
#[test]
fn market_candidacy_counter_snapshot_and_retry_id_stability() {
    let mut world = kk_world::World::new(1);
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    let mut engine = Engine::new(world);
    const SEED: i64 = 10_000_000;
    engine.players.entry("p1".to_string()).or_default().money = SEED;
    // fee = ceil(毫price × total × 0.05) = ceil(1000×100×0.05) = 5000
    const FEE: i64 = 5000;
    let mut batch = IntentBatch::default();
    batch
        .users
        .entry("p1".to_string())
        .or_default()
        .market
        .push(crate::intent::MarketIntent::CreateOrder {
            order_type: Some("sell".to_string()),
            resource_type: Some("token".to_string()),
            price_milli: Some(1000),
            total_amount: Some(100),
            room_name: None,
        });
    let before = engine.market.clone();
    // 建单之后、发布之前注入失败，检查候选没有改动已提交状态。
    engine.set_fail_stage_for_test(Some(TickStage::Validate));
    assert!(engine.tick_with_intents(batch.clone()).is_err());
    assert_eq!(
        engine.market, before,
        "失败轮 committed 市场整态保持（含私有计数器——seq 不前进）"
    );
    assert_eq!(engine.players["p1"].money, SEED, "失败轮扣费不落 committed");
    // 重试（同 batch）：恰一次建单、id 稳定为首次分配值、扣费恰一次
    engine.set_fail_stage_for_test(None);
    engine.tick_with_intents(batch).unwrap();
    assert_eq!(engine.market.orders.len(), 1, "重试恰一次建单");
    assert!(
        engine.market.orders.contains_key("mkt1"),
        "订单 id 与首次尝试一致（失败轮候选丢弃 ⇒ 序号未消耗）"
    );
    assert_eq!(
        engine.players["p1"].money,
        SEED - FEE,
        "重试恰一次扣费（双扣形态为 SEED−2×FEE）"
    );
    // 第二次建单继续递增序号，不能重用 mkt1。
    let mut second = IntentBatch::default();
    second
        .users
        .entry("p1".to_string())
        .or_default()
        .market
        .push(crate::intent::MarketIntent::CreateOrder {
            order_type: Some("sell".to_string()),
            resource_type: Some("pixel".to_string()),
            price_milli: Some(1000),
            total_amount: Some(100),
            room_name: None,
        });
    engine.tick_with_intents(second).unwrap();
    assert!(
        engine.market.orders.contains_key("mkt2"),
        "第二张订单 id=mkt2（序号从 committed 续档，非重用）"
    );
}

/// 分别破坏房间覆盖、顺序、ID 游标和玩家集合，验证各项候选校验会拒绝。
#[test]
fn validate_stage_tripwires_discriminate() {
    let mut world = kk_world::World::new(1);
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    world
        .register_room("W2N2", kk_core::Terrain::plains())
        .unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 5,
                y: 5,
                energy: 1000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    let mut engine = Engine::new(world);
    engine.players.entry("p1".to_string()).or_default();
    let mk_cand = |next| {
        let mut txn = engine
            .prepare_tick(
                IntentBatch::default(),
                TickInput {
                    now_ms: 0,
                    warmup: false,
                },
            )
            .unwrap();
        txn.state.inner.borrow_mut().next_id = next;
        txn.rooms = txn
            .room_names
            .iter()
            .map(|name| (name.clone(), engine.world.room(name).unwrap().clone()))
            .collect();
        txn
    };
    engine.validate_stage(&mut mk_cand(engine.next_id)).unwrap();
    let mut short = mk_cand(engine.next_id);
    short.rooms.pop();
    let err = engine.validate_stage(&mut short).unwrap_err();
    assert!(err.to_string().contains("覆盖"), "{err}");
    let mut swapped = mk_cand(engine.next_id);
    swapped.rooms.swap(0, 1);
    let err = engine.validate_stage(&mut swapped).unwrap_err();
    assert!(err.to_string().contains("覆盖"), "{err}");
    let err = engine.validate_stage(&mut mk_cand(0)).unwrap_err();
    assert!(err.to_string().contains("回退"), "{err}");
    let mut cand = mk_cand(engine.next_id);
    cand.state.inner.borrow_mut().players.remove("p1");
    let err = engine.validate_stage(&mut cand).unwrap_err();
    assert!(err.to_string().contains("丢键"), "{err}");
}
