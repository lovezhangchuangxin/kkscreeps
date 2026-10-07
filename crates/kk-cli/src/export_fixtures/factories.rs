//! 夹具世界的对象与地形构造工厂：场景构建器共享的统一摆法。
//!
//! creep 部件按满血 100 hits 生成；terrain_with 在平原基底上叠加墙/沼泽
//! 地形码后归一（settle_tests Fixture 同款摆法，见本目录 mod.rs 模块文档）。

use kk_schema::room::{CreepObj, RoomObject, Store};
use kk_world::types::BodyKind;

pub(super) fn body(kinds: &[BodyKind]) -> Vec<kk_schema::room::BodyPart> {
    kinds
        .iter()
        .map(|k| kk_schema::room::BodyPart {
            kind: k.code(),
            hits: 100,
            boost: None,
        })
        .collect()
}

pub(super) fn creep(name: &str, owner: &str, x: u8, y: u8, kinds: &[BodyKind]) -> RoomObject {
    creep_with_energy(name, owner, x, y, kinds, 0)
}

pub(super) fn creep_with_energy(
    name: &str,
    owner: &str,
    x: u8,
    y: u8,
    kinds: &[BodyKind],
    energy: u32,
) -> RoomObject {
    RoomObject::Creep(CreepObj {
        id: 0,
        x,
        y,
        fatigue: 0,
        age_time: 1500,
        hits: kinds.len() as u32 * 100,
        spawning: false,
        action_log: 0,
        name: name.into(),
        owner: owner.into(),
        body: body(kinds),
        store: Store { energy, ghodium: 0 },
        store_extra: Vec::new(),
        notify_when_attacked: None,
        say_message: None,
        say_public: None,
    })
}

pub(super) fn terrain_with(
    walls: &[(usize, usize)],
    swamps: &[(usize, usize)],
) -> kk_core::Terrain {
    let mut t = kk_core::Terrain::plains();
    for &(x, y) in walls {
        t.as_mut_slice()[y * 50 + x] |= kk_core::TERRAIN_MASK_WALL;
    }
    for &(x, y) in swamps {
        t.as_mut_slice()[y * 50 + x] |= kk_core::TERRAIN_MASK_SWAMP;
    }
    t.normalize();
    t
}
