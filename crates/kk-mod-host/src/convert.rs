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

mod value;
use value::*;
mod schema;
use schema::*;
mod fields;
use fields::*;
mod buildings;
mod handlers;
mod intents;
mod objects;
mod rules;

/// 单个包的声明候选；只有完成全部转换及可执行绑定校验才交出贡献。
struct ContributionBuilder<'a> {
    namespace: &'a str,
    contribution: Contribution,
    required_executables: BTreeSet<Key>,
}

/// 把一个包的注册声明组装为贡献。executables 是密封后的真实函数键全集。
pub fn build_contribution(
    namespace: &str,
    source: Source,
    declarations: &[Declaration],
    executables: &BTreeSet<Key>,
) -> ModHostResult<Contribution> {
    let mut builder = ContributionBuilder {
        namespace,
        contribution: Contribution {
            source,
            bindings: BTreeSet::new(),
            object_types: Vec::new(),
            field_mounts: Vec::new(),
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
            buildings: Vec::new(),
        },
        required_executables: BTreeSet::new(),
    };
    for declaration in declarations {
        if declaration.namespace != namespace {
            return err(namespace, "声明与当前注册包不一致（宿主状态被绕过）");
        }
        let value = &declaration.value;
        match declaration.kind.as_str() {
            "object" => builder.add_object(value)?,
            "intent" => builder.add_intent(value)?,
            "handler" => builder.add_handler(value)?,
            "rule" => builder.add_rule(value)?,
            "override" => builder.add_override(value)?,
            "resource" => builder.add_resource(value)?,
            "building" => builder.add_building(value)?,
            "mount" => builder.add_mount(value)?,
            other => return err(namespace, format!("未知声明种类 {other:?}")),
        }
    }
    let missing: Vec<String> = builder
        .required_executables
        .difference(executables)
        .map(|key| key.to_string())
        .collect();
    if !missing.is_empty() {
        return err(
            namespace,
            format!("声明的绑定没有注册函数：{}", missing.join(", ")),
        );
    }
    Ok(builder.contribution)
}
