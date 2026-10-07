//! 静态玩法障碍与动态占格分离；登记阶段与仲裁阶段共用结构判定。
use kk_core::TERRAIN_MASK_WALL;
use kk_schema::room::RoomObject;
use kk_world::types::is_obstacle_type;
use kk_world::{RoomIndex, RoomState};

use super::{has_road, type_name_of};

pub(crate) fn registration_blocked(
    room: &RoomState,
    index: &RoomIndex,
    buildings: &kk_mod_api::buildings::BuildingCatalog,
    at: (u8, u8),
    owner: &str,
) -> bool {
    index
        .at(at.0 as usize, at.1 as usize)
        .iter()
        .any(|id| match room.objects.get(id) {
            Some(RoomObject::Creep(_) | RoomObject::ConstructionSite(_)) | None => false,
            Some(RoomObject::Generic(object)) => {
                let data: serde_json::Value =
                    serde_json::from_str(&object.data).unwrap_or_default();
                let kind = data["structureType"].as_str().unwrap_or("");
                if kind == "rampart" {
                    !data["isPublic"].as_bool().unwrap_or(false)
                        && data["owner"].as_str().unwrap_or("") != owner
                } else {
                    is_obstacle_type(kind) || buildings.is_obstacle(kind)
                }
            }
            Some(other) => {
                let kind = type_name_of(other);
                is_obstacle_type(&kind) || buildings.is_obstacle(&kind)
            }
        })
}

pub(crate) fn arbitration_blocked(
    room: &RoomState,
    index: &RoomIndex,
    buildings: &kk_mod_api::buildings::BuildingCatalog,
    at: (u8, u8),
    owner: &str,
) -> bool {
    registration_blocked(room, index, buildings, at, owner)
        || index.at(at.0 as usize, at.1 as usize).iter().any(|id| {
            matches!(room.objects.get(id), Some(RoomObject::ConstructionSite(site))
                if site.owner.as_deref() == Some(owner)
                    && (is_obstacle_type(&site.structure_type) || buildings.is_obstacle(&site.structure_type)))
        })
        || (room.terrain().check_mask(at.0 as usize, at.1 as usize, TERRAIN_MASK_WALL)
            && !has_road(room, index, at))
}
