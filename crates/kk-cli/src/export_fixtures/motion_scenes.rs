//! 动态场景夹具（R4 运动/战斗、R5 动作事件链）：世界构建器 + 逐 tick 意图脚本。
//!
//! 返回需要意图驱动的对象句柄（名 → id），由 mod.rs 的 render_fixture 逐轮
//! 注入脚本产出的 IntentBatch。

use std::collections::BTreeMap;

use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_schema::room::{ConstructionSiteObj, ControllerObj, RoadObj, RoomObject};
use kk_world::ObjId;
use kk_world::World;
use kk_world::types::BodyKind;

use super::factories::{creep, creep_with_energy};

/// R4 运动与战斗夹具（房间 W5N5 全平原）：移动/朝向插值、受击罩三色、
/// 死亡淡出的实弹取材。布局（互不相扰的两组舞台）：
///   - 走位者×2（p1，move×4）：右/下/左/上循环——方向逐 tick 变化（朝向
///     旋转取材）；两组相位错开。
///   - 近战组：attacker(p1 [attack,move]) 每 tick 咬 victimA(p2 [move]，100
///     hits，ATTACK_POWER=30 → 第 4 tick 死亡——removedIds→死亡淡出)。
///   - 双向组：attacker2 咬 victimB(p2 [tough,tough,move]，300 hits) 且
///     healer2 每 tick 治 victimB → 同 tick attacked+healed=黄罩。
///   - 治疗组：healer(p1 [heal,move]，储能) 每 tick 治 tank(p1 [tough,move])
///     → 纯绿罩（heal.js:34 事件无条件，满血也发）。
///
/// 意图对死亡目标的续发由结算链静默拒（room.rs resolve else-return）。
pub(super) fn build_creeps(world: &mut World) -> BTreeMap<String, ObjId> {
    world
        .register_room("W5N5", kk_core::Terrain::plains())
        .unwrap();
    let mut handles = BTreeMap::new();
    let put =
        |world: &mut World, handles: &mut BTreeMap<String, ObjId>, name: &str, o: RoomObject| {
            let id = world.insert("W5N5", o).unwrap();
            handles.insert(name.to_string(), id);
        };
    put(
        world,
        &mut handles,
        "walker1",
        creep(
            "walker1",
            "p1",
            10,
            10,
            &[
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
            ],
        ),
    );
    put(
        world,
        &mut handles,
        "walker2",
        creep(
            "walker2",
            "p1",
            14,
            10,
            &[
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
            ],
        ),
    );
    put(
        world,
        &mut handles,
        "attacker",
        creep(
            "attacker",
            "p1",
            20,
            20,
            &[BodyKind::Attack, BodyKind::Move],
        ),
    );
    put(
        world,
        &mut handles,
        "victimA",
        creep("victimA", "p2", 20, 21, &[BodyKind::Move]),
    );
    put(
        world,
        &mut handles,
        "attacker2",
        creep(
            "attacker2",
            "p1",
            23,
            20,
            &[BodyKind::Attack, BodyKind::Move],
        ),
    );
    put(
        world,
        &mut handles,
        "victimB",
        creep(
            "victimB",
            "p2",
            22,
            20,
            &[BodyKind::Tough, BodyKind::Tough, BodyKind::Move],
        ),
    );
    put(
        world,
        &mut handles,
        "healer",
        creep_with_energy(
            "healer",
            "p1",
            25,
            25,
            &[BodyKind::Heal, BodyKind::Move],
            1000,
        ),
    );
    put(
        world,
        &mut handles,
        "tank",
        creep("tank", "p1", 25, 26, &[BodyKind::Tough, BodyKind::Move]),
    );
    put(
        world,
        &mut handles,
        "healer2",
        creep_with_energy(
            "healer2",
            "p1",
            22,
            21,
            &[BodyKind::Heal, BodyKind::Move],
            1000,
        ),
    );
    handles
}

/// 逐 tick 意图脚本：走位循环 + 战斗/治疗。方向钟面（constants.rs TOP=1 顺
/// 时针到 TOP_LEFT=8）：3=右、5=下、7=左、1=上（movement.rs offsets）。
pub(super) fn creep_tick_script(tick: u32, handles: &BTreeMap<String, ObjId>) -> IntentBatch {
    let mut batch = IntentBatch::default();
    let id = |name: &str| -> ObjId {
        *handles
            .get(name)
            .unwrap_or_else(|| panic!("意图脚本句柄缺失: {name}"))
    };
    const LOOP1: [u8; 4] = [3, 5, 7, 1];
    const LOOP2: [u8; 4] = [5, 7, 1, 3];
    let phase = (tick as usize) % 4;
    for (name, loop_) in [("walker1", &LOOP1), ("walker2", &LOOP2)] {
        let mut data = IntentData::bare();
        data.direction = Some(loop_[phase]);
        batch.add_object_intent("p1", id(name), Action::Move, data);
    }
    batch.add_object_intent(
        "p1",
        id("attacker"),
        Action::Attack,
        IntentData::target(id("victimA")),
    );
    batch.add_object_intent(
        "p1",
        id("attacker2"),
        Action::Attack,
        IntentData::target(id("victimB")),
    );
    batch.add_object_intent(
        "p1",
        id("healer"),
        Action::Heal,
        IntentData::target(id("tank")),
    );
    batch.add_object_intent(
        "p1",
        id("healer2"),
        Action::Heal,
        IntentData::target(id("victimB")),
    );
    batch
}

/// R5 real event chain. Public intent API only; includes a completed site that
/// disappears from the current index and ranged healing. RangedAttack/MassAttack
/// currently have no settle handlers on this branch; those use a synthetic gallery.
pub(super) fn build_actions(world: &mut World) -> BTreeMap<String, ObjId> {
    const ROOM: &str = "W6N6";
    world
        .register_room(ROOM, kk_core::Terrain::plains())
        .unwrap();
    let mut handles = BTreeMap::new();
    let mut put = |name: &str, object| {
        handles.insert(name.to_string(), world.insert(ROOM, object).unwrap());
    };
    for (name, x, y, kinds, energy) in [
        ("attacker", 9, 10, vec![BodyKind::Attack, BodyKind::Move], 0),
        (
            "ranged-healer",
            8,
            13,
            vec![BodyKind::Heal, BodyKind::Move],
            0,
        ),
        (
            "harvester",
            14,
            10,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            0,
        ),
        (
            "builder",
            13,
            16,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            50,
        ),
        (
            "repairer",
            20,
            15,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            50,
        ),
        (
            "upgrader",
            29,
            24,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            50,
        ),
    ] {
        put(name, creep_with_energy(name, "p1", x, y, &kinds, energy));
    }
    let tank_body = vec![BodyKind::Tough; 20];
    for (name, x, y, owner) in [
        ("ranged-target", 10, 10, "p2"),
        ("heal-target", 10, 13, "p1"),
    ] {
        put(name, creep(name, owner, x, y, &tank_body));
    }
    put(
        "source",
        RoomObject::Source(kk_schema::room::SourceObj {
            id: 0,
            x: 15,
            y: 10,
            energy: 3000,
            energy_capacity: 3000,
            next_regeneration_time: None,
        }),
    );
    put(
        "site",
        RoomObject::ConstructionSite(ConstructionSiteObj {
            id: 0,
            x: 14,
            y: 17,
            structure_type: "extension".into(),
            progress: 2995,
            progress_total: 3000,
            owner: Some("p1".into()),
            name: None,
        }),
    );
    put(
        "road",
        RoomObject::Road(RoadObj {
            id: 0,
            x: 21,
            y: 16,
            hits: 1,
            next_decay_time: 5000,
            notify_when_attacked: None,
        }),
    );
    put(
        "controller",
        RoomObject::Controller(ControllerObj {
            id: 0,
            x: 30,
            y: 24,
            level: 5,
            safe_mode: 0,
            safe_mode_available: 0,
            progress: 1200,
            downgrade_time: None,
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        }),
    );
    handles
}

pub(super) fn action_tick_script(_tick: u32, handles: &BTreeMap<String, ObjId>) -> IntentBatch {
    let mut batch = IntentBatch::default();
    for (source, action, target) in [
        ("attacker", Action::Attack, "ranged-target"),
        ("ranged-healer", Action::RangedHeal, "heal-target"),
        ("harvester", Action::Harvest, "source"),
        ("builder", Action::Build, "site"),
        ("repairer", Action::Repair, "road"),
        ("upgrader", Action::UpgradeController, "controller"),
    ] {
        batch.add_object_intent(
            "p1",
            handles[source],
            action,
            IntentData::target(handles[target]),
        );
    }
    batch
}
