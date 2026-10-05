//! 玩家面投影目录：从冻结计划确定性导出。
//!
//! 玩家沙箱与两条投影通道（JSON/KKPV）共享本目录：FIND 常量在分配后
//! 与类型键一一对应，同一计划恒同值；跨计划不承诺稳定（计划指纹变化
//! 即新面，玩家模块经安装 API 取注册常量，不硬抢全局整数）。

use crate::definition::{Key, ObjectVisibility, Visibility};
use crate::plan::ContributionPlan;
use crate::validate::field_name;

/// Mod FIND 常量的分配起点：避开官方 1..=123 的保留段。
pub const MOD_FIND_BASE: i64 = 1000;

/// 记录保留键：Mod 对象投影信封与查找约定的键，字段裸名不得占用。
/// `x`/`y` 不在列——声明的位置字段允许同名（信封坐标与字段值同步为
/// 同一数据，投影时由信封覆写为等值）。`structureType`/`owner`/`user`
/// 有意不在列：它们对 Mod 记录是惰性键——ext 键消费只按 structureType
/// 登记（扩展写入门不认 Mod 对象 id，恒不命中）、视野授予方已排除
/// Mod 记录，且字段值先进信封后写、不会被覆写穿透。
const RESERVED_RECORD_KEYS: [&str; 6] = ["type", "modType", "id", "room", "pos", "ext"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceField {
    /// 字段裸名（payload 键）。
    pub name: String,
    pub visibility: Visibility,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceType {
    pub key: Key,
    /// 注册分配的 FIND 常量（`api.queries.install` 返回值）。
    pub find_const: i64,
    /// LOOK 常量：类型键字符串本身。
    pub look_key: String,
    pub player_class: String,
    pub visibility: ObjectVisibility,
    /// 归属字段裸名（无声明则为 None，投影按无归属处理）。
    pub owner_field: Option<String>,
    pub fields: Vec<SurfaceField>,
}

/// 玩家面目录。`types` 只含会进入玩家投影的类型（RoomVisible/
/// OwnerCollection）；ServerOnly 类型不进任何玩家面，也不要求玩家类
/// 绑定。`intents` 是玩家可发出的 Mod 意图键（emit 预检目录）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerSurface {
    pub types: Vec<SurfaceType>,
    pub intents: Vec<Key>,
}

impl PlayerSurface {
    /// 从冻结计划导出。字段裸名占用保留键、条件可见性、非法或重复的
    /// 玩家类名都视为装配错误——宁可拒绝分片也不产出被覆写的投影。
    pub fn from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        let mut types = Vec::new();
        let mut class_names = std::collections::BTreeSet::new();
        for projected in plan.projection().types().iter() {
            let descriptor = &projected.descriptor;
            if descriptor.key.namespace() == "builtin" {
                continue;
            }
            match descriptor.visibility {
                ObjectVisibility::RoomVisible | ObjectVisibility::OwnerCollection => {}
                ObjectVisibility::ServerOnly => continue,
                ObjectVisibility::Conditional(ref key) => {
                    return Err(format!(
                        "类型 {} 的条件对象可见性未接线（判定绑定 {key}）",
                        descriptor.key
                    ));
                }
            }
            let position_names: [Option<&str>; 2] = match &descriptor.position_fields {
                Some(pair) => [Some(field_name(&pair[0])), Some(field_name(&pair[1]))],
                None => [None, None],
            };
            let mut fields = Vec::with_capacity(descriptor.fields.len());
            for field in &descriptor.fields {
                let name = field_name(&field.key);
                if let Visibility::Conditional(key) = &field.visibility {
                    return Err(format!(
                        "类型 {} 的字段 {name} 声明条件可见性（判定绑定 {key}）——未接线",
                        descriptor.key
                    ));
                }
                if RESERVED_RECORD_KEYS.contains(&name) {
                    return Err(format!(
                        "类型 {} 的字段裸名 {name} 占用投影保留键（保留：{:?}）",
                        descriptor.key, RESERVED_RECORD_KEYS
                    ));
                }
                if (name == "x" || name == "y") && !position_names.contains(&Some(name)) {
                    return Err(format!(
                        "类型 {} 的字段裸名 {name} 与信封坐标冲突（仅声明的位置字段允许此名）",
                        descriptor.key
                    ));
                }
                fields.push(SurfaceField {
                    name: name.to_string(),
                    visibility: field.visibility.clone(),
                });
            }
            if !is_js_identifier(&descriptor.player_class) {
                return Err(format!(
                    "类型 {} 的玩家类名 {:?} 不是合法的 JS 标识符",
                    descriptor.key, descriptor.player_class
                ));
            }
            if !class_names.insert(descriptor.player_class.clone()) {
                return Err(format!(
                    "玩家类名 {} 在多个类型上重复（玩家全局绑定须唯一）",
                    descriptor.player_class
                ));
            }
            types.push(SurfaceType {
                key: descriptor.key.clone(),
                find_const: MOD_FIND_BASE + types.len() as i64,
                look_key: descriptor.key.as_str().to_string(),
                player_class: descriptor.player_class.clone(),
                visibility: descriptor.visibility.clone(),
                owner_field: descriptor
                    .owner_field
                    .as_ref()
                    .map(|key| field_name(key).to_string()),
                fields,
            });
        }
        let intents = plan
            .intents()
            .iter()
            .filter(|intent| intent.key.namespace() != "builtin")
            .map(|intent| intent.key.clone())
            .collect();
        Ok(Self { types, intents })
    }

    pub fn find_const_for(&self, type_key: &str) -> Option<i64> {
        self.types
            .iter()
            .find(|ty| ty.key.as_str() == type_key)
            .map(|ty| ty.find_const)
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty() && self.intents.is_empty()
    }
}

fn is_js_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::*;
    use std::collections::BTreeSet;

    fn surface_type(key: &str, visibility: ObjectVisibility) -> ObjectTypeDescriptor {
        ObjectTypeDescriptor {
            key: Key::new(key).unwrap(),
            schema_version: 1,
            owner_field: None,
            position_fields: None,
            fields: Vec::new(),
            capabilities: Vec::new(),
            lifecycle_slots: BTreeSet::new(),
            visibility,
            player_class: "Counter".to_string(),
        }
    }

    fn typed_field(name: &str, visibility: Visibility, kind: ValueKind) -> FieldDescriptor {
        FieldDescriptor {
            key: Key::new(format!("extmod:{name}")).unwrap(),
            schema: ValueSchema {
                kind,
                nullable: false,
            },
            default: FieldDefault::Missing,
            visibility,
            binding: Key::new(format!("extmod:t.field.{name}")).unwrap(),
        }
    }

    fn field(name: &str, visibility: Visibility) -> FieldDescriptor {
        typed_field(name, visibility, ValueKind::Bool)
    }

    fn u32_0_49() -> ValueKind {
        ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 })
    }

    fn plan_with(ty: ObjectTypeDescriptor) -> ContributionPlan {
        let contribution = Contribution {
            source: Source {
                namespace: "extmod".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [0; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: ty
                .fields
                .iter()
                .map(|f| f.binding.clone())
                .collect::<BTreeSet<_>>(),
            object_types: vec![ty],
            field_mounts: Vec::new(),
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
        };
        crate::freeze_with_builtin(
            crate::builtin::link(),
            vec![contribution],
            FreezeOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn allocates_find_constants_and_skips_server_only() {
        let mut server = surface_type("extmod:hidden", ObjectVisibility::ServerOnly);
        server.player_class = "Hidden".into();
        let visible = surface_type("extmod:counter", ObjectVisibility::RoomVisible);
        // ServerOnly 类型仍占描述符位置但不分配玩家面条目；FIND 序号按
        // 玩家面类型顺序分配，不依赖计划索引。
        let plan = plan_with(visible.clone());
        let surface = PlayerSurface::from_plan(&plan).unwrap();
        assert_eq!(surface.types.len(), 1);
        assert_eq!(surface.types[0].find_const, MOD_FIND_BASE);
        assert_eq!(surface.types[0].look_key, "extmod:counter");
        assert_eq!(surface.types[0].player_class, "Counter");
        assert!(surface.intents.is_empty());

        let plan = plan_with(server);
        let surface = PlayerSurface::from_plan(&plan).unwrap();
        assert!(surface.types.is_empty());
        assert!(surface.is_empty());
    }

    #[test]
    fn reserved_field_names_rejected() {
        let mut ty = surface_type("extmod:counter", ObjectVisibility::RoomVisible);
        ty.fields = vec![field("type", Visibility::Public)];
        let err = PlayerSurface::from_plan(&plan_with(ty.clone())).unwrap_err();
        assert!(err.contains("保留键"), "{err}");

        // x/y 只允许出现在声明的位置字段上。
        ty.fields = vec![field("x", Visibility::Public)];
        let err = PlayerSurface::from_plan(&plan_with(ty.clone())).unwrap_err();
        assert!(err.contains("信封坐标冲突"), "{err}");

        let x = Key::new("extmod:x").unwrap();
        let y = Key::new("extmod:y").unwrap();
        ty.fields = vec![
            typed_field("x", Visibility::Public, u32_0_49()),
            typed_field("y", Visibility::Public, u32_0_49()),
        ];
        ty.position_fields = Some([x, y]);
        PlayerSurface::from_plan(&plan_with(ty)).unwrap();
    }

    #[test]
    fn player_class_identifier_and_uniqueness() {
        let mut ty = surface_type("extmod:counter", ObjectVisibility::RoomVisible);
        ty.player_class = "not a name".into();
        let err = PlayerSurface::from_plan(&plan_with(ty.clone())).unwrap_err();
        assert!(err.contains("JS 标识符"), "{err}");

        ty.player_class = "$Counter".into();
        PlayerSurface::from_plan(&plan_with(ty)).unwrap();
    }

    #[test]
    fn conditional_visibility_rejected() {
        let mut ty = surface_type("extmod:counter", ObjectVisibility::RoomVisible);
        ty.fields = vec![field(
            "secret",
            Visibility::Conditional(Key::new("extmod:judge").unwrap()),
        )];
        let contribution = Contribution {
            source: Source {
                namespace: "extmod".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [0; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: BTreeSet::from([
                Key::new("extmod:judge").unwrap(),
                Key::new("extmod:t.field.secret").unwrap(),
            ]),
            object_types: vec![ty],
            field_mounts: Vec::new(),
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
        };
        let plan = crate::freeze_with_builtin(
            crate::builtin::link(),
            vec![contribution],
            FreezeOptions::default(),
        )
        .unwrap();
        let err = PlayerSurface::from_plan(&plan).unwrap_err();
        assert!(err.contains("条件可见性"), "{err}");
    }

    #[test]
    fn derivation_is_deterministic_for_same_plan() {
        let plan = plan_with(surface_type(
            "extmod:counter",
            ObjectVisibility::RoomVisible,
        ));
        let a = PlayerSurface::from_plan(&plan).unwrap();
        let b = PlayerSurface::from_plan(&plan).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.find_const_for("extmod:counter"), Some(MOD_FIND_BASE));
        assert_eq!(a.find_const_for("extmod:missing"), None);
    }

    #[test]
    fn intents_catalog_collects_mod_keys_only() {
        let contribution = Contribution {
            source: Source {
                namespace: "extmod".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [0; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: BTreeSet::from([Key::new("extmod:increment.process").unwrap()]),
            object_types: Vec::new(),
            field_mounts: Vec::new(),
            intents: vec![IntentDescriptor {
                key: Key::new("extmod:increment").unwrap(),
                scope: IntentScope::Object,
                receiver: Some(TypeRef {
                    key: Key::new("builtin:link").unwrap(),
                    schema_version: 1,
                }),
                arguments: ValueSchema {
                    kind: ValueKind::Bool,
                    nullable: false,
                },
                conversion: ArgumentConversion::Strict,
                authority: IntentAuthority::Player,
                merge: Merge::Last,
                slot: Key::new("extmod:increment.slot").unwrap(),
                suppressed_by: BTreeSet::new(),
            }],
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: vec![SlotDescriptor {
                key: Key::new("extmod:increment.slot").unwrap(),
                stage: Stage::ObjectAction,
                native_binding: Key::new("extmod:increment.process").unwrap(),
            }],
            handlers: Vec::new(),
            resources: Vec::new(),
        };
        let plan = crate::freeze_with_builtin(
            crate::builtin::link(),
            vec![contribution],
            FreezeOptions::default(),
        )
        .unwrap();
        let surface = PlayerSurface::from_plan(&plan).unwrap();
        assert_eq!(surface.intents, vec![Key::new("extmod:increment").unwrap()]);
    }

    #[test]
    fn owner_field_bare_name_exported() {
        let mut ty = surface_type("extmod:counter", ObjectVisibility::OwnerCollection);
        ty.fields = vec![typed_field(
            "owner",
            Visibility::Public,
            ValueKind::String { max_bytes: 128 },
        )];
        ty.owner_field = Some(Key::new("extmod:owner").unwrap());
        let surface = PlayerSurface::from_plan(&plan_with(ty)).unwrap();
        assert_eq!(surface.types[0].owner_field.as_deref(), Some("owner"));
    }
}
