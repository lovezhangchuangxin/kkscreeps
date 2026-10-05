//! 提供世界 JSON 和查询快照的只读接口，目前由 kk-world::World 实现。
//!
//! 玩家执行期间通过不可变借用读取同一已提交世界，结算在执行循环结束后进行。
//! 世界 JSON 按 viewer 过滤；对象及 FIND 查询用于宿主管理接口，不执行玩家视野检查。

use serde_json::Value;

use kk_world::RoomVersionHandle;

/// 世界读取接口，注入 JSON 与查询快照各有自己的数据格式。
pub trait WorldSource {
    /// 生成按玩家视野过滤的世界 JSON；gcl_points 是玩家累计 GCL 点数。
    fn world_json_for_viewer(&self, viewer: &str, gcl_points: i64) -> Value;

    /// 按对象字符串 id 返回查询快照，不按观察者过滤；非法 id 或不支持的对象返回 None。
    fn object_snapshot(&self, id: &str) -> Option<Value>;

    /// 按 FIND 常量返回房间对象的查询快照，不执行观察者视野检查。
    fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value>;

    /// 房间 controller 坐标。
    fn controller_pos(&self, room: &str) -> Option<(u64, u64)>;

    /// 世界内容版本号，用于检测内容变化；它与世界 tick、沙箱派发轮次用途不同。
    fn generation(&self) -> u64;

    /// 地形版本号，用于使缓存的地形编码失效。
    fn terrain_epoch(&self) -> u64;

    /// 房间的世界时间与内容版本标识，不携带房间数据或维持数据生命周期。
    fn room_version(&self, room: &str) -> Option<RoomVersionHandle>;

    /// 世界时间（玩家可见 Game.time 的来源）。
    fn game_time(&self) -> u64;

    /// 带扩展字段目录与 Mod 玩家面的玩家投影；扩展字段按权限裁切进入
    /// 对象 ext 键，Mod 对象经 surface 按可见性裁切进入对象数组。
    /// 目录为空且 surface 为 None 时与 world_json_for_viewer 输出相同。
    fn world_json_for_viewer_with_extensions(
        &self,
        viewer: &str,
        gcl_points: i64,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
    ) -> Value;

    /// 为查看者生成 KKPV 投影：不含对象数组与地形的世界信封与逐房间
    /// 私有对象块输入。未实现二进制投影的源返回 None；启用 KKPV 通道的
    /// 调用方据此失败而不是静默回落（回落会掩盖接线缺陷）。
    fn kkpv_projection_for_viewer(
        &self,
        _viewer: &str,
        _gcl_points: i64,
        _catalog: &kk_mod_api::extension::ExtensionCatalog,
        _surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
        _plan_hash: [u8; 32],
        _audience_hash: [u8; 32],
    ) -> Option<Result<(Value, Vec<kk_schema::kkpv::KkpvBlockInput>), String>> {
        None
    }

    /// 生成公开地形块输入：全部注册房间、与查看者无关。None 语义与
    /// [`WorldSource::kkpv_projection_for_viewer`] 一致（启用 KKPV 通道
    /// 时 fail-loud，不静默回落）。
    fn kkpv_public_terrain_blocks(
        &self,
        _plan_hash: [u8; 32],
        _audience_hash: [u8; 32],
    ) -> Option<Vec<kk_schema::kkpv::KkpvBlockInput>> {
        None
    }
}

impl WorldSource for kk_world::World {
    fn world_json_for_viewer(&self, viewer: &str, gcl_points: i64) -> Value {
        kk_world::World::world_json_for_viewer(self, viewer, gcl_points)
    }

    fn world_json_for_viewer_with_extensions(
        &self,
        viewer: &str,
        gcl_points: i64,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
    ) -> Value {
        kk_world::World::world_json_for_viewer_with_extensions(
            self, viewer, gcl_points, catalog, surface,
        )
    }

    fn kkpv_projection_for_viewer(
        &self,
        viewer: &str,
        gcl_points: i64,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
        plan_hash: [u8; 32],
        audience_hash: [u8; 32],
    ) -> Option<Result<(Value, Vec<kk_schema::kkpv::KkpvBlockInput>), String>> {
        Some(
            kk_world::World::kkpv_room_blocks_for_viewer(
                self,
                viewer,
                catalog,
                surface,
                plan_hash,
                audience_hash,
            )
            .map(|blocks| {
                (
                    kk_world::World::kkpv_envelope_for_viewer(self, viewer, gcl_points),
                    blocks,
                )
            }),
        )
    }

    fn object_snapshot(&self, id: &str) -> Option<Value> {
        kk_world::ObjId::parse(id).and_then(|id| self.object_snapshot_by_id(id))
    }

    fn kkpv_public_terrain_blocks(
        &self,
        plan_hash: [u8; 32],
        audience_hash: [u8; 32],
    ) -> Option<Vec<kk_schema::kkpv::KkpvBlockInput>> {
        Some(kk_world::World::kkpv_public_terrain_blocks(
            self,
            plan_hash,
            audience_hash,
        ))
    }

    fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        self.find_snapshots_in_room(room, find_const)
    }

    fn controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        kk_world::World::controller_pos(self, room)
    }

    fn generation(&self) -> u64 {
        kk_world::World::generation(self)
    }

    fn terrain_epoch(&self) -> u64 {
        kk_world::World::terrain_epoch(self)
    }

    fn room_version(&self, room: &str) -> Option<RoomVersionHandle> {
        kk_world::World::room_version(self, room)
    }

    fn game_time(&self) -> u64 {
        kk_world::World::time(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_core::Terrain;

    /// 检查 WorldSource 转发的时间、版本、空查询和注入时间字段。
    #[test]
    fn world_source_delegates_to_world() {
        let mut world = kk_world::World::new(7);
        world.register_room("W1N1", Terrain::plains());
        let source: &dyn WorldSource = &world;
        assert_eq!(source.generation(), world.generation());
        assert_eq!(source.terrain_epoch(), world.terrain_epoch());
        assert_eq!(source.game_time(), world.time());
        assert_eq!(
            source.room_version("W1N1").unwrap(),
            world.room_version("W1N1").unwrap()
        );
        assert!(source.room_version("nope").is_none());
        assert_eq!(source.world_json_for_viewer("p", 0)["time"], world.time());
        assert!(source.object_snapshot("zz").is_none());
        assert!(source.find_in_room("W1N1", 1).is_empty());
    }
}

#[cfg(test)]
mod extension_projection_via_trait_tests {
    use super::*;

    #[test]
    fn nonempty_catalog_projects_ext_through_dyn_source() {
        // 非空目录经 &dyn WorldSource 分发端到端输出 ext 键；
        // 目录为空时同一分发不输出（早退）。
        let mut world = kk_world::World::new(3);
        world.register_room("W1N1", kk_core::Terrain::plains());
        let link = world
            .insert(
                "W1N1",
                kk_schema::room::RoomObject::Generic(kk_schema::room::GenericObj {
                    id: 0,
                    x: 10,
                    y: 10,
                    data: r#"{"type":"structure","structureType":"link","owner":"p1","hits":1,"hitsMax":1}"#.into(),
                }),
            )
            .unwrap();
        world
            .room_mut("W1N1")
            .unwrap()
            .extensions
            .entry(link)
            .or_default()
            .insert("testmod:mark".into(), serde_json::json!(7));

        let mut contrib = kk_mod_api::Contribution {
            source: kk_mod_api::Source {
                namespace: "testmod".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [1; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: std::collections::BTreeSet::from([
                kk_mod_api::Key::new("testmod:bind.mark").unwrap()
            ]),
            field_mounts: vec![kk_mod_api::FieldMountDescriptor {
                target: kk_mod_api::TypeRef {
                    key: kk_mod_api::Key::new("builtin:link").unwrap(),
                    schema_version: 1,
                },
                field: kk_mod_api::FieldDescriptor {
                    key: kk_mod_api::Key::new("testmod:mark").unwrap(),
                    schema: kk_mod_api::ValueSchema {
                        kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Signed {
                            min: 0,
                            max: 100,
                        }),
                        nullable: false,
                    },
                    default: kk_mod_api::FieldDefault::Missing,
                    visibility: kk_mod_api::Visibility::Public,
                    binding: kk_mod_api::Key::new("testmod:bind.mark").unwrap(),
                },
            }],
            object_types: Vec::new(),
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: std::collections::BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
            buildings: Vec::new(),
        };
        let _ = &mut contrib;
        let plan = kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contrib],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap();
        let catalog = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan).unwrap();
        let source: &dyn WorldSource = &world;
        let view = source.world_json_for_viewer_with_extensions("p1", 0, &catalog, None);
        let obj = view["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == serde_json::json!(link.as_str()))
            .unwrap();
        assert_eq!(obj["ext"]["testmod:mark"], serde_json::json!(7));

        let empty = kk_mod_api::extension::ExtensionCatalog::default();
        let plain = source.world_json_for_viewer_with_extensions("p1", 0, &empty, None);
        let obj = plain["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == serde_json::json!(link.as_str()))
            .unwrap();
        assert!(obj.get("ext").is_none());
    }
}
