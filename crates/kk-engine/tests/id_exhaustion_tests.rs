//! ID 耗尽属于整轮宿主故障，不能发布局部成功或重复对象。
use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_engine::{Engine, RunState, TickInput};
use kk_schema::room::{BodyPart, CreepObj, RoomObject, Store};

#[test]
fn restored_near_exhausted_allocator_discards_all_rooms_and_retries_without_publishing() {
    let mut e = Engine::default();
    let mut batch = IntentBatch::default();
    for (n, room) in ["W1N1", "W2N2", "W3N3"].iter().enumerate() {
        e.register_room(*room, kk_core::Terrain::plains()).unwrap();
        let id = e
            .insert_object(
                room,
                RoomObject::Creep(CreepObj {
                    id: 0,
                    x: 20,
                    y: 20,
                    fatigue: 0,
                    age_time: 10000,
                    hits: 100,
                    spawning: false,
                    action_log: 0,
                    name: format!("c{n}"),
                    owner: "p1".into(),
                    body: vec![BodyPart {
                        kind: 1,
                        hits: 100,
                        boost: None,
                    }],
                    store: Store {
                        energy: 20,
                        ghodium: 0,
                    },
                    store_extra: vec![],
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
                }),
            )
            .unwrap();
        let mut d = IntentData::bare();
        d.amount = Some(10);
        d.resource_type = Some("energy".into());
        batch.add_object_intent("p1", id, Action::Drop, d);
    }
    e.bump_next_obj_id_for_test(u64::MAX - 1).unwrap();

    let saved = e.checkpoint().unwrap();
    let before = serde_json::to_value(&saved).unwrap();
    let mut restored = Engine::default();
    restored.restore_checkpoint(saved).unwrap();
    for _ in 0..2 {
        let error = restored
            .tick_with_intents_with_input(batch.clone(), TickInput { now_ms: 0 })
            .unwrap_err();
        assert!(error.0.contains("ID"), "{error}");
        assert!(matches!(
            restored.run_state(),
            RunState::NeedsRecovery { .. }
        ));
        for room in restored.world().rooms().values() {
            assert!(
                !room
                    .objects
                    .values()
                    .any(|object| matches!(object, RoomObject::Energy(_)))
            );
            for object in room.objects.values() {
                if let RoomObject::Creep(creep) = object {
                    assert_eq!(creep.store.energy, 20);
                }
            }
        }
        restored.abandon_recovery().unwrap();
        assert_eq!(
            serde_json::to_value(restored.checkpoint().unwrap()).unwrap(),
            before
        );
    }
}
