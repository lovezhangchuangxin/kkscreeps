//! Invader raid 管理命令与检查点契约：种子校验、部分更新语义、
//! 预定边界与畸形值拒绝。生成器行为见 invader_raid.rs。
use super::invader_raid::{RaidSetup, setup_raid_room};
use super::*;
use kk_core::constants as c;

const ROOM: &str = "W1N1";
const CORE_ROOM: &str = "W1N2";

/// 评审修复回归：无控制器房间的 null 清除是无操作（不 panic）；
/// ticksToEnd 浮点整数不落缺省；预定对象未知子键拒绝。
#[test]
fn raid_seed_reservation_edge_contract() -> anyhow::Result<()> {
    // controller:false 房间：null 清除无操作、非 null 报错。
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({"players": ["p1"], "rooms": [
        {"name": "W1N1"},
        {"name": "W1N0", "controller": false},
    ]}))?;
    e.set_invader_raid_state("W1N0", &json!({"controllerReservation": null}))?;
    assert!(
        e.set_invader_raid_state("W1N0", &json!({"controllerReservation": {"owner": "p1"}}),)
            .is_err()
    );
    // ticksToEnd 100.0 与 100 等价，不落缺省 5000。
    e.set_invader_raid_state(
        ROOM,
        &json!({"controllerReservation": {"owner": "p1", "ticksToEnd": 100.0}}),
    )?;
    let controllers = e.find_in_room(ROOM, c::FIND_STRUCTURES);
    let ctl = controllers
        .iter()
        .find(|s| s["structureType"] == "controller")
        .expect("必有控制器");
    assert_eq!(ctl["reservation"]["ticksToEnd"], json!(100));
    // 预定对象内未知子键整单拒绝。
    assert!(
        e.set_invader_raid_state(
            ROOM,
            &json!({"controllerReservation": {"owner": "p1", "evil": 1}}),
        )
        .is_err()
    );
    Ok(())
}

/// 检查点注入畸形 raid 值（负阈值/负预算/空房态）在恢复时拒绝。
#[test]
fn raid_checkpoint_rejects_invalid_raid_values() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    setup_raid_room(&mut e, RaidSetup::default())?;
    let saved = e.checkpoint(Value::Null)?;
    for (pointer, value) in [
        ("/engine/world/rooms/W1N1/raid/invader_goal", json!(-1)),
        ("/engine/world/rooms/W1N1/raid/harvested_energy", json!(-5)),
        ("/engine/world/rooms/W1N1/raid/status", json!("")),
    ] {
        let mut bad = saved.clone();
        let target = bad.pointer_mut(pointer).expect("raid 键存在");
        *target = value;
        assert!(
            e.restore_checkpoint(bad, None).is_err(),
            "应拒绝注入 {pointer}"
        );
    }
    assert!(e.restore_checkpoint(saved, None).is_ok(), "原载荷仍可恢复");
    Ok(())
}

/// 种子命令的键校验：未知键/越界值/无源预算/无控制器预定整单拒绝。
#[test]
fn raid_seed_validates_contract() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    setup_raid_room(&mut e, RaidSetup::default())?;
    for bad in [
        json!({"unknown": 1}),
        json!({"harvestedEnergy": -1}),
        json!({"harvestedEnergy": 1.5}),
        json!({"raidGoal": -5}),
        json!({"active": "yes"}),
        json!({"status": ""}),
        json!({"raidGoal": 1.5}),
        json!({"controllerReservation": {"owner": "nobody"}}),
        json!({"controllerReservation": {"owner": "p1", "ticksToEnd": -1}}),
        json!({"controllerReservation": {"owner": "p1", "ticksToEnd": 1.5}}),
        json!({"controllerReservation": {"ticksToEnd": 100}}),
    ] {
        assert!(
            e.set_invader_raid_state(ROOM, &bad).is_err(),
            "应拒绝: {bad}"
        );
    }
    // 无 source 房间不允许正预算；core 房无 source。
    assert!(
        e.set_invader_raid_state(CORE_ROOM, &json!({"harvestedEnergy": 5}))
            .is_err()
    );
    assert!(
        e.set_invader_raid_state(CORE_ROOM, &json!({"harvestedEnergy": 0}))
            .is_ok()
    );
    // 不存在的房间拒绝。
    assert!(e.set_invader_raid_state("E9S9", &json!({})).is_err());
    Ok(())
}
