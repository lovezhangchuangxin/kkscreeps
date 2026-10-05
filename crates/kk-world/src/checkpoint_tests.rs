use super::*;
use kk_schema::room::{EnergyObj, RoomObject};

fn fixture() -> World {
    let mut world = World::new(42);
    for name in ["W1N1", "W2N2"] {
        world.register_room(name, Terrain::plains());
        world
            .insert(
                name,
                RoomObject::Energy(EnergyObj {
                    id: 0,
                    x: 3,
                    y: 4,
                    resource_type: 0,
                    amount: 29,
                    resource: Some("ore:crystal".into()),
                }),
            )
            .unwrap();
    }
    world.room_mut("W1N1").unwrap().extensions.insert(
        ObjId(1),
        BTreeMap::from([("ore:private".into(), serde_json::json!({"value": 3}))]),
    );
    world.bump_next_obj_id(100);
    world.advance_ticks(7);
    world
}

#[test]
fn world_checkpoints_preserve_extensions_deleted_id_cursor_and_cache_versions() {
    let world = fixture();
    let saved = world.checkpoint();
    let bytes = serde_json::to_vec(&saved).unwrap();
    let mut restored = World::from_checkpoint(serde_json::from_slice(&bytes).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(restored.checkpoint()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    assert_eq!(world.world_json(), restored.world_json());
    assert_eq!(world.get(ObjId(1)), restored.get(ObjId(1)));
    let object = restored.get(ObjId(1)).unwrap().1.clone();
    assert_eq!(restored.insert("W1N1", object).unwrap(), ObjId(100));
    assert!(restored.set_terrain("W1N1", Terrain::plains()).is_err());
}

#[test]
fn corrupt_cross_room_ids_headers_coordinates_and_extensions_are_rejected() {
    for kind in ["id", "header", "coordinate", "extension", "room", "version"] {
        let mut saved = fixture().checkpoint();
        match kind {
            "id" | "header" | "coordinate" => {
                let data = &mut saved.rooms.get_mut("W2N2").unwrap().snapshot;
                let mut snapshot = kk_schema::room::decode_snapshot(data).unwrap();
                match kind {
                    "id" => snapshot.objects[0].set_id(1),
                    "header" => snapshot.header.time += 1,
                    "coordinate" => snapshot.objects[0].set_pos(50, 3),
                    _ => unreachable!(),
                }
                *data = kk_schema::room::encode_snapshot(&snapshot);
            }
            "extension" => {
                saved
                    .rooms
                    .get_mut("W1N1")
                    .unwrap()
                    .extensions
                    .insert(99, BTreeMap::new());
            }
            "room" => {
                let room = saved.rooms.remove("W1N1").unwrap();
                saved.rooms.insert("W999N1".into(), room);
            }
            "version" => saved.generation = u64::MAX,
            _ => unreachable!(),
        }
        assert!(World::from_checkpoint(saved).is_err(), "{kind}");
    }
}
