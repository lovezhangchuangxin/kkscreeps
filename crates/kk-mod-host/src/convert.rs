//! SDK 声明 JSON 到 kk-mod-api 描述符的权威转换：SDK 只做形状预检，
//! 命名空间归属、可执行绑定覆盖与数值域在此处定论。
//!
//! 键的约定（与 builtin 贡献同构）：
//! - 字段键 `{ns}:{name}`，作用域是所属类型，跨类型同名不冲突；
//! - 字段绑定 `{ns}:{类型名}.field.{name}`，绑定是全局集合，须带类型限定；
//! - 动作处理绑定 `{intent_key}.process`，自动入口 `{intent_key}.slot`
//!   以处理绑定为默认中心；
//! - 处理器绑定与其键同名；挂载字段绑定 `{ns}:mount.{name}`。

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::error::{ModHostError, ModHostResult};
use kk_mod_api::{
    ArgumentConversion, Contribution, FieldDefault, FieldDescriptor, FieldMountDescriptor,
    HandlerDescriptor, HandlerKind, IntentAuthority, IntentDescriptor, IntentScope, Key, Merge,
    Number, NumberDomain, ObjectTypeDescriptor, ObjectVisibility, RuleDescriptor, RuleOverride,
    SlotDescriptor, Source, Stage, TypeRef, ValueKind, ValueSchema, Visibility,
};

/// 一次 op_mod_declare 的载荷；`namespace` 由宿主打标，不由 SDK 自报。
#[derive(Clone, Debug)]
pub struct Declaration {
    pub namespace: String,
    pub kind: String,
    pub value: Value,
}

fn err<T>(namespace: &str, message: impl Into<String>) -> ModHostResult<T> {
    Err(ModHostError::new(namespace.to_string(), message))
}

fn own_key(namespace: &str, value: &str) -> ModHostResult<Key> {
    let key =
        Key::new(value).map_err(|message| ModHostError::new(namespace.to_string(), message))?;
    if key.namespace() != namespace {
        return err(namespace, format!("键 {key} 不属于本包命名空间"));
    }
    Ok(key)
}

fn any_key(namespace: &str, value: &str) -> ModHostResult<Key> {
    Key::new(value).map_err(|message| ModHostError::new(namespace.to_string(), message))
}

fn as_str<'a>(namespace: &str, value: &'a Value, field: &str) -> ModHostResult<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ModHostError::new(namespace.to_string(), format!("缺少字符串字段 {field}")))
}

fn optional_str<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn as_u32(namespace: &str, value: &Value, field: &str) -> ModHostResult<u32> {
    let raw = value.get(field).and_then(Value::as_u64).ok_or_else(|| {
        ModHostError::new(namespace.to_string(), format!("字段 {field} 须为非负整数"))
    })?;
    if raw > u32::MAX as u64 {
        return err(namespace, format!("字段 {field} 超出 u32 范围"));
    }
    Ok(raw as u32)
}

/// 存在但形状非法的数值选项一律报错：拼错形状静默落默认值等于改写
/// 校验语义，与装载器的 fail-loud 立场冲突。
fn optional_u32(namespace: &str, value: &Value, field: &str) -> ModHostResult<Option<u32>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(_)) => {
            let raw = value.get(field).and_then(Value::as_u64).ok_or_else(|| {
                ModHostError::new(namespace.to_string(), format!("字段 {field} 须为非负整数"))
            })?;
            if raw > u32::MAX as u64 {
                return err(namespace, format!("字段 {field} 超出 u32 范围"));
            }
            Ok(Some(raw as u32))
        }
        Some(_) => err(namespace, format!("字段 {field} 须为非负整数")),
    }
}

fn optional_i64(namespace: &str, value: &Value, field: &str) -> ModHostResult<Option<i64>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(number) => number.as_i64().map(Some).ok_or_else(|| {
            ModHostError::new(namespace.to_string(), format!("字段 {field} 须为整数"))
        }),
    }
}

/// 数值域的边界读取与类型分流：f64 的端点按浮点读（小数端点合法），
/// 整型按严格 i64 读；形状非法一律报错，不静默落默认域。
fn number_domain(namespace: &str, kind: &str, value: &Value) -> ModHostResult<NumberDomain> {
    if kind == "f64" {
        let min = optional_f64(namespace, value, "min")?;
        let max = optional_f64(namespace, value, "max")?;
        if min.is_none() || max.is_none() {
            return err(
                namespace,
                "f64 数值域须显式给出 min 与 max（冻结校验要求有限端点，默认 ±∞ 的域是产出即死的形状）",
            );
        }
        let (min, max) = (min.expect("上面已判存在"), max.expect("上面已判存在"));
        if min > max {
            return err(namespace, "f64 的数值域下界大于上界");
        }
        return Ok(NumberDomain::Float { min, max });
    }
    let min = optional_i64(namespace, value, "min")?;
    let max = optional_i64(namespace, value, "max")?;
    if let (Some(low), Some(high)) = (min, max)
        && low > high
    {
        return err(namespace, format!("{kind} 的数值域下界大于上界"));
    }
    Ok(match kind {
        "i32" => NumberDomain::Signed {
            min: min.unwrap_or(i32::MIN.into()),
            max: max.unwrap_or(i32::MAX.into()),
        },
        "i64" => NumberDomain::Signed {
            min: min.unwrap_or(i64::MIN),
            max: max.unwrap_or(i64::MAX),
        },
        "u32" => NumberDomain::Unsigned {
            min: min.map(|v| (v.max(0)) as u64).unwrap_or(0),
            max: max.map(|v| (v.max(0)) as u64).unwrap_or(u32::MAX as u64),
        },
        "u64" => NumberDomain::Unsigned {
            min: min.map(|v| (v.max(0)) as u64).unwrap_or(0),
            max: max.map(|v| (v.max(0)) as u64).unwrap_or(u64::MAX),
        },
        other => return err(namespace, format!("未知数值类型 {other:?}")),
    })
}

/// 存在但形状非法的浮点选项一律报错（与整型选项同一条纪律）。
fn optional_f64(namespace: &str, value: &Value, field: &str) -> ModHostResult<Option<f64>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(number) => number
            .as_f64()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| {
                ModHostError::new(namespace.to_string(), format!("字段 {field} 须为有限数"))
            }),
    }
}

fn number_of(namespace: &str, kind: &str, value: &Value) -> ModHostResult<Number> {
    if kind == "f64" {
        let raw = value
            .as_f64()
            .ok_or_else(|| ModHostError::new(namespace.to_string(), "规则值须为数"))?;
        return Ok(Number::Float(raw));
    }
    let raw = value
        .as_i64()
        .ok_or_else(|| ModHostError::new(namespace.to_string(), "整型规则的值须为整数"))?;
    if kind == "u32" || kind == "u64" {
        if raw < 0 {
            return err(namespace, "无符号规则的值不得为负");
        }
        return Ok(Number::Unsigned(raw as u64));
    }
    Ok(Number::Signed(raw))
}

/// 值 schema 的 JSON 说明（见 sdk.js schemaOf）转描述符。
/// ref 的 target 允许指向其他命名空间的类型——引用与挂载本就跨包。
fn schema(namespace: &str, value: &Value) -> ModHostResult<ValueSchema> {
    let kind = as_str(namespace, value, "type")?;
    let nullable = value
        .get("nullable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let kind = match kind {
        "bool" => ValueKind::Bool,
        "i32" | "i64" | "u32" | "u64" | "f64" => {
            ValueKind::Number(number_domain(namespace, kind, value)?)
        }
        "string" => ValueKind::String {
            max_bytes: match optional_u32(namespace, value, "maxBytes")? {
                Some(bytes) => bytes,
                None => {
                    return err(namespace, "string 类型缺少 maxBytes");
                }
            },
        },
        "ref" => ValueKind::ObjectRef(TypeRef {
            key: any_key(namespace, as_str(namespace, value, "target")?)?,
            schema_version: as_u32(namespace, value, "version")?,
        }),
        "enum" => {
            let Some(values) = value.get("values").and_then(Value::as_array) else {
                return err(namespace, "enum 类型缺少 values 数组");
            };
            let mut members = BTreeSet::new();
            for member in values {
                let Some(member) = member.as_str() else {
                    return err(namespace, "enum 成员须为字符串");
                };
                members.insert(member.to_string());
            }
            ValueKind::Enum(members)
        }
        "array" => ValueKind::Array {
            element: Box::new(schema(
                namespace,
                value.get("element").ok_or_else(|| {
                    ModHostError::new(namespace.to_string(), "array 类型缺少 element")
                })?,
            )?),
            max_len: optional_u32(namespace, value, "maxLen")?.unwrap_or(16),
        },
        "map" => ValueKind::Map {
            value: Box::new(schema(
                namespace,
                value.get("value").ok_or_else(|| {
                    ModHostError::new(namespace.to_string(), "map 类型缺少 value")
                })?,
            )?),
            max_len: optional_u32(namespace, value, "maxLen")?.unwrap_or(16),
            max_key_bytes: optional_u32(namespace, value, "maxKeyBytes")?.unwrap_or(128),
        },
        "record" => {
            let Some(fields) = value.get("fields").and_then(Value::as_object) else {
                return err(namespace, "record 类型缺少 fields");
            };
            let mut members = BTreeMap::new();
            for (name, member) in fields {
                members.insert(name.clone(), schema(namespace, member)?);
            }
            let mut optional = BTreeSet::new();
            if let Some(names) = value.get("optional").and_then(Value::as_array) {
                for name in names {
                    let Some(name) = name.as_str() else {
                        return err(namespace, "optional 须为字符串数组");
                    };
                    optional.insert(name.to_string());
                }
            }
            ValueKind::Record {
                fields: members,
                optional,
            }
        }
        other => return err(namespace, format!("未知类型 {other:?}")),
    };
    Ok(ValueSchema { kind, nullable })
}

fn field_visibility(namespace: &str, value: &str) -> ModHostResult<Visibility> {
    match value {
        "public" => Ok(Visibility::Public),
        "owner" => Ok(Visibility::Owner),
        "server-only" => Ok(Visibility::ServerOnly),
        other => err(namespace, format!("未知字段可见性 {other:?}")),
    }
}

fn object_visibility(namespace: &str, value: Option<&str>) -> ModHostResult<ObjectVisibility> {
    match value {
        None => Ok(ObjectVisibility::RoomVisible),
        Some("room-visible") => Ok(ObjectVisibility::RoomVisible),
        Some("owner-collection") => Ok(ObjectVisibility::OwnerCollection),
        Some("server-only") => Ok(ObjectVisibility::ServerOnly),
        Some(other) => err(namespace, format!("未知对象可见性 {other:?}")),
    }
}

fn field_descriptor(
    namespace: &str,
    name: &str,
    value: &Value,
    binding_for: impl Fn(&str) -> String,
) -> ModHostResult<FieldDescriptor> {
    // 字段名禁点号：字段绑定 {ns}:{类型名}.field.{name} 依赖名字段单射，
    // 含点的名字可与"其他类型名+字段名"拼出同一绑定而在集合内静默合并。
    if name.is_empty() || name.bytes().any(|b| b == b'.' || b == b':') {
        return err(namespace, format!("字段名 {name:?} 不得为空、含点号或冒号"));
    }
    let field_key = own_key(namespace, &format!("{namespace}:{name}"))?;
    let binding = own_key(namespace, &format!("{namespace}:{}", binding_for(name)))?;
    Ok(FieldDescriptor {
        key: field_key,
        schema: schema(
            namespace,
            value.get("schema").ok_or_else(|| {
                ModHostError::new(namespace.to_string(), format!("字段 {name} 缺少 schema"))
            })?,
        )?,
        default: match value.get("default") {
            None | Some(Value::Null) => FieldDefault::Missing,
            Some(other) => FieldDefault::Value(other.clone()),
        },
        visibility: field_visibility(
            namespace,
            optional_str(value, "visibility").unwrap_or("public"),
        )?,
        binding,
    })
}

/// 动作自动入口的阶段映射。object/room 域动作与对象动作同段处理；
/// player/world 域动作现阶段统一归入全局段——更细的阶段划分待
/// 这些作用域出现真实包时再开放（登记为装载器残留）。
fn stage_of_scope(scope: &IntentScope) -> Stage {
    match scope {
        IntentScope::Object | IntentScope::Room => Stage::ObjectAction,
        IntentScope::Player | IntentScope::World => Stage::Market,
    }
}

/// 把一个包的注册声明组装为贡献。`executables` 是 realm 密封后的
/// 绑定函数键全集：处理器与动作处理函数必须真实存在，声明不算实现。
pub fn build_contribution(
    namespace: &str,
    source: Source,
    declarations: &[Declaration],
    executables: &BTreeSet<Key>,
) -> ModHostResult<Contribution> {
    let mut bindings: BTreeSet<Key> = BTreeSet::new();
    let mut object_types = Vec::new();
    let mut field_mounts = Vec::new();
    let mut intents = Vec::new();
    let mut rules = Vec::new();
    let mut overrides = Vec::new();
    let mut slots = Vec::new();
    let mut handlers = Vec::new();
    let mut required_executables: BTreeSet<Key> = BTreeSet::new();

    for declaration in declarations {
        if declaration.namespace != namespace {
            return err(namespace, "声明与当前注册包不一致（宿主状态被绕过）");
        }
        let value = &declaration.value;
        match declaration.kind.as_str() {
            "object" => {
                let key = own_key(namespace, as_str(namespace, value, "key")?)?;
                let type_key_label = key.to_string();
                let type_name = key
                    .as_str()
                    .split_once(':')
                    .expect("Key 含分隔符")
                    .1
                    .to_string();
                let mut fields = Vec::new();
                let Some(field_values) = value.get("fields").and_then(Value::as_array) else {
                    return err(namespace, format!("类型 {key} 缺少字段列表"));
                };
                for field in field_values {
                    let name = as_str(namespace, field, "name")?;
                    let descriptor = field_descriptor(namespace, name, field, |name| {
                        format!("{type_name}.field.{name}")
                    })?;
                    bindings.insert(descriptor.binding.clone());
                    fields.push(descriptor);
                }
                let named_key = |name: &str| own_key(namespace, &format!("{namespace}:{name}"));
                let owner_field = match optional_str(value, "ownerField") {
                    Some(name) => Some(named_key(name)?),
                    None => None,
                };
                let position_fields = match value.get("positionFields").and_then(Value::as_array) {
                    Some(pair) if pair.len() == 2 => {
                        let mut names = Vec::new();
                        for name in pair {
                            let Some(name) = name.as_str() else {
                                return err(
                                    namespace,
                                    format!("类型 {key} 的位置字段名须为字符串"),
                                );
                            };
                            names.push(named_key(name)?);
                        }
                        Some([names[0].clone(), names[1].clone()])
                    }
                    Some(_) => {
                        return err(
                            namespace,
                            format!("类型 {key} 的 positionFields 须为两个名字"),
                        );
                    }
                    None => None,
                };
                object_types.push(ObjectTypeDescriptor {
                    key,
                    schema_version: as_u32(namespace, value, "version")?,
                    owner_field,
                    position_fields,
                    fields,
                    capabilities: Vec::new(),
                    lifecycle_slots: BTreeSet::new(),
                    visibility: object_visibility(namespace, optional_str(value, "visibility"))?,
                    player_class: {
                        match optional_str(value, "playerClass") {
                            Some(name) if !name.is_empty() => name.to_string(),
                            _ => {
                                return err(
                                    namespace,
                                    format!(
                                        "类型 {type_key_label} 缺少 playerClass（冻结校验要求非空玩家类名；\
                                         纯服务端类型也须显式命名）"
                                    ),
                                );
                            }
                        }
                    }
                });
            }
            "intent" => {
                let key = own_key(namespace, as_str(namespace, value, "key")?)?;
                let scope = match as_str(namespace, value, "scope")? {
                    "object" => IntentScope::Object,
                    "room" => IntentScope::Room,
                    "player" => IntentScope::Player,
                    "world" => IntentScope::World,
                    other => return err(namespace, format!("未知动作作用域 {other:?}")),
                };
                let receiver = match value.get("target") {
                    Some(target) => Some(TypeRef {
                        key: any_key(namespace, as_str(namespace, target, "key")?)?,
                        schema_version: as_u32(namespace, target, "version")?,
                    }),
                    None => None,
                };
                let merge = match optional_str(value, "mode").unwrap_or("last") {
                    "last" => Merge::Last,
                    "queue" => Merge::Queue {
                        max_len: optional_u32(namespace, value, "queueMax")?
                            .unwrap_or(8)
                            .max(1),
                    },
                    other => return err(namespace, format!("未知合并模式 {other:?}")),
                };
                let process_binding = own_key(namespace, &format!("{}.process", key))?;
                let slot_key = match optional_str(value, "slot") {
                    Some(slot) => any_key(namespace, slot)?,
                    None => {
                        let auto = own_key(namespace, &format!("{}.slot", key))?;
                        slots.push(SlotDescriptor {
                            key: auto.clone(),
                            stage: stage_of_scope(&scope),
                            native_binding: process_binding.clone(),
                        });
                        auto
                    }
                };
                required_executables.insert(process_binding.clone());
                bindings.insert(process_binding);
                intents.push(IntentDescriptor {
                    key,
                    scope,
                    receiver,
                    arguments: schema(
                        namespace,
                        value.get("args").ok_or_else(|| {
                            ModHostError::new(namespace.to_string(), "动作缺少参数 schema")
                        })?,
                    )?,
                    conversion: ArgumentConversion::Strict,
                    authority: IntentAuthority::Player,
                    merge,
                    slot: slot_key,
                    suppressed_by: BTreeSet::new(),
                });
            }
            "handler" => {
                let key = own_key(namespace, as_str(namespace, value, "key")?)?;
                let slot = any_key(namespace, as_str(namespace, value, "slot")?)?;
                let kind = match as_str(namespace, value, "kind")? {
                    "before" => HandlerKind::Before,
                    "wrap" => HandlerKind::Wrap,
                    "replace" => HandlerKind::Replace,
                    "after" => HandlerKind::After,
                    other => return err(namespace, format!("未知处理器种类 {other:?}")),
                };
                let binding = key.clone();
                bindings.insert(binding.clone());
                required_executables.insert(binding.clone());
                handlers.push(HandlerDescriptor {
                    key,
                    slot,
                    kind,
                    binding,
                    depends_on: BTreeSet::new(),
                });
            }
            "rule" => {
                let key = own_key(namespace, as_str(namespace, value, "key")?)?;
                let kind = as_str(namespace, value, "type")?;
                if !matches!(kind, "i32" | "i64" | "u32" | "u64" | "f64") {
                    return err(namespace, format!("规则 {key} 的类型 {kind:?} 未知"));
                }
                let Some(consumers) = value.get("consumers").and_then(Value::as_array) else {
                    return err(namespace, format!("规则 {key} 缺少消费者列表"));
                };
                let mut consumer_keys = BTreeSet::new();
                for consumer in consumers {
                    let Some(consumer) = consumer.as_str() else {
                        return err(namespace, "规则消费者须为绑定键字符串");
                    };
                    consumer_keys.insert(any_key(namespace, consumer)?);
                }
                rules.push(RuleDescriptor {
                    key: key.clone(),
                    domain: number_domain(namespace, kind, value)?,
                    default: number_of(
                        namespace,
                        kind,
                        value.get("default").ok_or_else(|| {
                            ModHostError::new(
                                namespace.to_string(),
                                format!("规则 {key} 缺少默认值"),
                            )
                        })?,
                    )?,
                    consumers: consumer_keys,
                });
            }
            "override" => {
                overrides.push(RuleOverride {
                    rule: any_key(namespace, as_str(namespace, value, "rule")?)?,
                    value: match value.get("value").and_then(Value::as_i64) {
                        Some(integral) => Number::Signed(integral),
                        None => {
                            Number::Float(value.get("value").and_then(Value::as_f64).ok_or_else(
                                || ModHostError::new(namespace.to_string(), "规则覆盖值须为数"),
                            )?)
                        }
                    },
                });
            }
            "mount" => {
                let target = value
                    .get("target")
                    .ok_or_else(|| ModHostError::new(namespace.to_string(), "挂载缺少目标类型"))?;
                let name = as_str(
                    namespace,
                    value
                        .get("field")
                        .ok_or_else(|| ModHostError::new(namespace.to_string(), "挂载缺少字段"))?,
                    "name",
                )?;
                let field = field_descriptor(
                    namespace,
                    name,
                    value.get("field").expect("上面已判存在"),
                    |name| format!("mount.{name}"),
                )?;
                bindings.insert(field.binding.clone());
                field_mounts.push(FieldMountDescriptor {
                    target: TypeRef {
                        key: any_key(namespace, as_str(namespace, target, "key")?)?,
                        schema_version: as_u32(namespace, target, "version")?,
                    },
                    field,
                });
            }
            other => return err(namespace, format!("未知声明种类 {other:?}")),
        }
    }

    let missing: Vec<String> = required_executables
        .difference(executables)
        .map(|key| key.to_string())
        .collect();
    if !missing.is_empty() {
        return err(
            namespace,
            format!("声明的绑定没有注册函数：{}", missing.join(", ")),
        );
    }

    Ok(Contribution {
        source,
        bindings,
        object_types,
        field_mounts,
        intents,
        rules,
        overrides,
        constraints: BTreeSet::new(),
        slots,
        handlers,
    })
}
