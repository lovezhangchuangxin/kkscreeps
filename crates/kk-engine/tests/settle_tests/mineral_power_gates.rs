//! 矿物 power 资格门在绝对时钟边界的引擎验收。
use super::*;

/// REGEN_MINERAL 资格门的时钟真值判（官方 usePower.js 直接读
/// `target.nextRegenerationTime` 真值）：Some(0) 视同未设置，非耗尽
/// 矿物可施放；真实钟（非 0）静默拒绝。embed 通道的放置换算
/// （ctx.time + 剩余值）在热身拍后恒得非 0 钟，该 falsy 边只在此直测。
#[test]
fn regen_mineral_gate_treats_zero_clock_as_unset() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(8, Some("p1"), |c| c.is_power_enabled = true);
    let zero = f.mineral(10, 10, "X", 500, 3, Some(0));
    let clocked = f.mineral(12, 10, "X", 500, 3, Some(50));
    let mut pc = |name: &str, x: u8| {
        f.world
            .insert(
                "W1N1",
                RoomObject::Generic(kk_schema::room::GenericObj {
                    id: 0,
                    x,
                    y: 10,
                    data: format!(
                        r#"{{"type":"powerCreep","owner":"p1","name":"{name}","powers":{{"14":{{"level":1}}}},"store":{{"ops":300}}}}"#
                    ),
                }),
            )
            .unwrap()
    };
    let pc1 = pc("a", 9);
    let pc2 = pc("b", 11);
    let mut b = IntentBatch::default();
    for (creep, target) in [(pc1, zero), (pc2, clocked)] {
        let mut d = IntentData::bare();
        d.power = Some(14);
        d.id = Some(target);
        b.add_object_intent("p1", creep, Action::UsePower, d);
    }
    let events = f.settle(b);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["event"] == kk_core::constants::EVENT_POWER)
            .count(),
        1,
        "仅 0 钟矿物施放成功（真值判：0 ≡ 未设置）"
    );
    assert!(
        f.world
            .edit_room("W1N1", |room| Ok(room.effects.contains_key(&zero)))
            .unwrap(),
        "0 钟目标获得效果"
    );
    assert!(
        !f.world
            .edit_room("W1N1", |room| Ok(room.effects.contains_key(&clocked)))
            .unwrap(),
        "真实钟目标静默拒绝"
    );
}
