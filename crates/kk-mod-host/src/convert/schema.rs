//! 声明转换共用的数值域与值 schema。
use super::*;

/// 数值域的边界读取与类型分流：f64 的端点按浮点读（小数端点合法），
/// 整型按严格 i64 读；形状非法一律报错，不静默落默认域。
pub(super) fn number_domain(
    namespace: &str,
    kind: &str,
    value: &Value,
) -> ModHostResult<NumberDomain> {
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
pub(super) fn optional_f64(
    namespace: &str,
    value: &Value,
    field: &str,
) -> ModHostResult<Option<f64>> {
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

pub(super) fn number_of(namespace: &str, kind: &str, value: &Value) -> ModHostResult<Number> {
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
pub(super) fn schema(namespace: &str, value: &Value) -> ModHostResult<ValueSchema> {
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
