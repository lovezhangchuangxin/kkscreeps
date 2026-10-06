//! 权威状态编辑不能绕过恢复边界，失败不改变已提交版本。
use kk_engine::intent::IntentBatch;
use kk_engine::{Engine, TickInput, TickStage};
use kk_schema::room::{EnergyObj, RoomObject};

fn engine() -> Engine {
    let mut engine = Engine::default();
    engine
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    engine
        .insert_object(
            "W1N1",
            RoomObject::Energy(EnergyObj {
                id: 0,
                x: 20,
                y: 20,
                resource_type: 0,
                amount: 10,
                resource: None,
            }),
        )
        .unwrap();
    engine
}

#[test]
fn failed_world_edit_keeps_checkpoint_versions_and_warm_caches() {
    let mut engine = engine();
    let json = engine.world().world_json();
    let before = serde_json::to_value(engine.checkpoint().unwrap()).unwrap();
    let id = *engine
        .world()
        .room("W1N1")
        .unwrap()
        .objects
        .keys()
        .next()
        .unwrap();
    assert!(
        engine
            .edit_object(id, |object| {
                object.set_id(999);
                Ok(())
            })
            .is_err()
    );
    assert!(
        engine
            .edit_world(|world| {
                world.register_room("W2N2", kk_core::Terrain::plains())?;
                Err::<(), _>("拒绝".into())
            })
            .is_err()
    );
    assert_eq!(engine.world().world_json(), json);
    assert_eq!(
        serde_json::to_value(engine.checkpoint().unwrap()).unwrap(),
        before
    );
}

#[test]
fn every_editor_rejects_recovery_and_account_edits_invalidate_receipts() {
    let mut engine = engine();
    let generation = engine.world().generation();
    engine
        .edit_players(|players| {
            players.entry("p1".into()).or_default().money = 100;
            Ok(())
        })
        .unwrap();
    assert_eq!(engine.world().generation(), generation + 1);
    engine.edit_market(|_| Ok(())).unwrap();
    assert_eq!(engine.world().generation(), generation + 2);
    engine.set_fail_stage_for_test(Some(TickStage::Market));
    assert!(
        engine
            .tick_with_intents_with_input(IntentBatch::default(), TickInput { now_ms: 0 })
            .is_err()
    );
    assert!(engine.edit_world(|_| Ok(())).is_err());
    assert!(engine.edit_players(|_| Ok(())).is_err());
    assert!(engine.edit_market(|_| Ok(())).is_err());
    assert!(engine.edit_mod_states(|_| Ok(())).is_err());
    engine.set_fail_stage_for_test(None);
    engine
        .tick_with_intents_with_input(IntentBatch::default(), TickInput { now_ms: 0 })
        .unwrap();
    assert_eq!(engine.players()["p1"].money, 100);
}

#[test]
fn terrain_edit_replaces_warm_projection_and_exhausted_version_rejects_edit() {
    let mut engine = engine();
    let before = engine.world().world_json();
    let epoch = engine.world().terrain_epoch();
    let mut codes = vec![0; 2500];
    codes[0] = 1;
    engine
        .set_terrain("W1N1", kk_core::Terrain::from_codes(&codes).unwrap())
        .unwrap();
    assert_ne!(engine.world().world_json(), before);
    assert_eq!(engine.world().terrain_epoch(), epoch + 1);
    let mut saved = engine.checkpoint().unwrap();
    saved.world.generation = u64::MAX;
    let mut restored = Engine::default();
    assert!(
        restored
            .restore_checkpoint(saved)
            .unwrap_err()
            .contains("版本")
    );
    assert_eq!(restored.world().generation(), 0);
}
