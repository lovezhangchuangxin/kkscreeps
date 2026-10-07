//! 原子批量管理放置：共享顺序归一化、计划校验与候选提交边界。
use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ModPlacement {
    type_key: String,
    fields: Value,
}

impl EmbedEngine {
    /// 按适配器的对象类别和初始字段摆放对象，返回对象 ID。
    /// 字段归一化由 [kk_world::place::normalize] 完成，显式字段优先。
    pub fn place_object(&mut self, room: &str, kind: &str, spec: &Value) -> anyhow::Result<String> {
        self.place_objects(&[(room, kind, spec)])
            .map(|ids| ids[0].clone())
    }

    /// 一次候选编辑按顺序归一化并放置对象；后项失败不发布前缀。
    /// `mod` 类别使用 `{typeKey, fields}`，与 sim 场景的 spec 同形。
    pub fn place_objects(
        &mut self,
        objects: &[(&str, &str, &Value)],
    ) -> anyhow::Result<Vec<String>> {
        self.no_open_round("placeObjects")?;
        if objects.is_empty() {
            return Ok(Vec::new());
        }
        let links = self.engine.links();
        let stores = self.engine.stores();
        let buildings = self.engine.buildings().clone();
        let prepared = objects
            .iter()
            .map(|(_, kind, spec)| {
                if *kind != "mod" {
                    return Ok(None);
                }
                let parsed: ModPlacement = serde_json::from_value((*spec).clone())?;
                let descriptor = self
                    .engine
                    .mod_object_type(&parsed.type_key)
                    .ok_or_else(|| anyhow::anyhow!("类型 {} 不在已装配计划中", parsed.type_key))?;
                let mut payload = kk_mod_api::objects::prepare_fields(&parsed.fields, descriptor)
                    .map_err(anyhow::Error::msg)?;
                // store 声明类型放置即携带库存信封（G09.a），与结算期
                // 对象创建同形。
                if let Some(capacity) = descriptor.store_capacity {
                    kk_mod_api::objects::inject_store_envelope(&mut payload, capacity);
                }
                let (x, y) = kk_mod_api::objects::position(&payload, descriptor)
                    .map_err(anyhow::Error::msg)?;
                Ok(Some(kk_schema::room::RoomObject::Mod(
                    kk_schema::room::ModObj {
                        id: 0,
                        x,
                        y,
                        type_key: parsed.type_key,
                        data: payload.to_string(),
                    },
                )))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        self.engine
            .edit_world(|world| {
                let mut ids = Vec::with_capacity(objects.len());
                for (&(room, kind, spec), prepared) in objects.iter().zip(prepared) {
                    let object = if let Some(object) = prepared {
                        object
                    } else {
                        let ctx = PlaceCtx {
                            time: world.time(),
                            links,
                            stores,
                            buildings: buildings.clone(),
                            controller: world.room(room).and_then(|room| room.controller()),
                            name_seq: world.next_obj_id() as usize,
                        };
                        normalize(kind, spec, &ctx)?
                    };
                    ids.push(world.insert(room, object)?.as_str());
                }
                Ok(ids)
            })
            .map_err(anyhow::Error::msg)
    }

    /// 管理命令：按已装配计划里的 Mod 类型创建对象并放入房间，返回对象 ID。
    /// `fields` 须提供除带默认值外的全部声明字段（挂载字段同样要求）；
    /// 缺失的有默认值字段落入默认，整体通过 schema 校验后写入世界存储。
    /// 声明位置字段的类型以 payload 位置为准（信封坐标同步）。
    pub fn place_mod_object(
        &mut self,
        room: &str,
        type_key: &str,
        fields: &Value,
    ) -> anyhow::Result<String> {
        self.place_objects(&[(room, "mod", &json!({"typeKey": type_key, "fields": fields}))])
            .map(|ids| ids[0].clone())
    }
}
