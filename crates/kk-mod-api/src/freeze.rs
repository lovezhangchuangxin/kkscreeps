use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::fingerprint::canonical_hash;
use crate::handlers::handler_plans;
use crate::rules::effective_rules;

use crate::validate;
use crate::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreezeError {
    pub context: String,
    pub message: String,
}

impl FreezeError {
    pub(crate) fn new(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for FreezeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.message)
    }
}
impl std::error::Error for FreezeError {}

fn insert<T>(map: &mut BTreeMap<Key, T>, key: Key, value: T) -> Result<(), FreezeError> {
    if map.insert(key.clone(), value).is_some() {
        return Err(FreezeError::new(key.to_string(), "重复定义"));
    }
    Ok(())
}

fn owned(key: &Key, namespace: &str) -> Result<(), FreezeError> {
    if key.namespace() != namespace {
        return Err(FreezeError::new(
            namespace,
            format!("不能定义其他命名空间的键 {key}"),
        ));
    }
    Ok(())
}

/// 消费启动输入并生成不可变计划；不安装绑定、不验证世界中的对象。
pub fn freeze(
    mut contributions: Vec<Contribution>,
    options: FreezeOptions,
) -> Result<ContributionPlan, FreezeError> {
    if options.projection_version != 1
        || options.max_schema_depth == 0
        || options.max_schema_depth > 32
    {
        return Err(FreezeError::new("freeze", "不支持的投影版本或 schema 深度"));
    }
    contributions.sort_by(|a, b| a.source.namespace.cmp(&b.source.namespace));
    let mut sources = BTreeMap::new();
    let mut bindings = BTreeSet::new();
    let mut types = BTreeMap::new();
    let mut rules = BTreeMap::new();
    let mut intents = BTreeMap::new();
    let mut slots = BTreeMap::new();
    let mut handlers = BTreeMap::new();
    let mut overrides: BTreeMap<Key, BTreeMap<String, Number>> = BTreeMap::new();
    for c in &mut contributions {
        let ns = &c.source.namespace;
        if !crate::definition::valid_namespace(ns)
            || c.source.api_version != 1
            || c.source.revision.is_empty()
        {
            return Err(FreezeError::new(ns, "namespace、API 版本或 revision 无效"));
        }
        if sources.insert(ns.clone(), c.source.clone()).is_some() {
            return Err(FreezeError::new(ns, "重复贡献来源"));
        }
        // Vec 表达的目录按稳定键归一化；集合和映射本身已有确定顺序。
        c.object_types.sort_by(|a, b| a.key.cmp(&b.key));
        c.rules.sort_by(|a, b| a.key.cmp(&b.key));
        c.intents.sort_by(|a, b| a.key.cmp(&b.key));
        c.slots.sort_by(|a, b| a.key.cmp(&b.key));
        c.handlers.sort_by(|a, b| a.key.cmp(&b.key));
        c.overrides.sort_by(|a, b| a.rule.cmp(&b.rule));
        for key in &c.bindings {
            owned(key, ns)?;
            bindings.insert(key.clone());
        }
        for ty in &mut c.object_types {
            owned(&ty.key, ns)?;
            ty.fields.sort_by(|a, b| a.key.cmp(&b.key));
            ty.capabilities.sort_by(|a, b| a.key.cmp(&b.key));
            for field in &ty.fields {
                owned(&field.key, ns)?;
                let context = format!("{} / {}", ty.key, field.key);
                validate::schema_depth(&field.schema, options.max_schema_depth, &context)?;
                if let FieldDefault::Value(value) = &field.default {
                    validate::default_value(value, &field.schema, &context)?;
                }
            }
            insert(&mut types, ty.key.clone(), ty.clone())?;
        }
        for r in &c.rules {
            owned(&r.key, ns)?;
            insert(&mut rules, r.key.clone(), r.clone())?;
        }
        for i in &c.intents {
            owned(&i.key, ns)?;
            validate::schema_depth(&i.arguments, options.max_schema_depth, i.key.as_str())?;
            insert(&mut intents, i.key.clone(), i.clone())?;
        }
        for s in &c.slots {
            owned(&s.key, ns)?;
            insert(&mut slots, s.key.clone(), s.clone())?;
        }
        for h in &c.handlers {
            owned(&h.key, ns)?;
            insert(&mut handlers, h.key.clone(), h.clone())?;
        }
        for r in &c.overrides {
            if overrides
                .entry(r.rule.clone())
                .or_default()
                .insert(ns.clone(), r.value.clone())
                .is_some()
            {
                return Err(FreezeError::new(
                    format!("{ns} / {}", r.rule),
                    "同一贡献重复覆盖规则",
                ));
            }
        }
    }
    for slot in slots.values() {
        validate::binding(&slot.native_binding, &bindings, slot.key.as_str())?;
    }
    for ty in types.values() {
        validate::object(ty, &types, &bindings, &slots, options.max_schema_depth)?;
    }
    for intent in intents.values() {
        let context = intent.key.as_str();
        if (intent.scope == IntentScope::Object) != intent.receiver.is_some() {
            return Err(FreezeError::new(context, "只有对象动作必须声明接收者"));
        }
        if let Some(receiver) = &intent.receiver {
            validate::type_ref(receiver, &types, context)?;
        }
        validate::schema(
            &intent.arguments,
            &types,
            1,
            options.max_schema_depth,
            context,
        )?;
        if let ArgumentConversion::Native(key) = &intent.conversion {
            validate::binding(key, &bindings, context)?;
        }
        if matches!(intent.merge, Merge::Queue { max_len: 0 }) {
            return Err(FreezeError::new(context, "队列上限须大于零"));
        }
        if !slots.contains_key(&intent.slot) {
            return Err(FreezeError::new(
                context,
                format!("未知动作入口 {}", intent.slot),
            ));
        }
        for key in &intent.suppressed_by {
            let other = intents
                .get(key)
                .ok_or_else(|| FreezeError::new(context, format!("未知压制动作 {key}")))?;
            if key == &intent.key
                || other.scope != intent.scope
                || other.receiver != intent.receiver
            {
                return Err(FreezeError::new(
                    context,
                    "压制动作须是同一接收者上的其他动作",
                ));
            }
        }
    }
    let values = effective_rules(&rules, &overrides, &options, &bindings)?;
    let effective: BTreeMap<_, _> = rules.keys().zip(&values).collect();
    for c in &contributions {
        for RuleConstraint::LessOrEqual { left, right } in &c.constraints {
            let a = effective.get(left).ok_or_else(|| {
                FreezeError::new(&c.source.namespace, format!("约束引用未知规则 {left}"))
            })?;
            let b = effective.get(right).ok_or_else(|| {
                FreezeError::new(&c.source.namespace, format!("约束引用未知规则 {right}"))
            })?;
            let valid = match (a, b) {
                (Number::Signed(a), Number::Signed(b)) => a <= b,
                (Number::Unsigned(a), Number::Unsigned(b)) => a <= b,
                (Number::Float(a), Number::Float(b)) => a <= b,
                _ => false,
            };
            if !valid {
                return Err(FreezeError::new(
                    &c.source.namespace,
                    format!("规则约束不满足：{left} <= {right}"),
                ));
            }
        }
    }
    let slot_plans = handler_plans(&slots, &handlers, &bindings)?;
    if types.len() > u32::MAX as usize
        || rules.len() > u32::MAX as usize
        || intents.len() > u32::MAX as usize
        || slots.len() > u32::MAX as usize
        || types.values().any(|t| t.fields.len() > u32::MAX as usize)
    {
        return Err(FreezeError::new("freeze", "目录超过句柄宽度"));
    }
    let projection = ProjectionPlan {
        version: options.projection_version,
        types: types
            .values()
            .enumerate()
            .map(|(i, ty)| ProjectedType {
                handle: TypeHandle(i as u32),
                descriptor: ty.clone(),
                fields: ty
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(j, f)| ProjectedField {
                        handle: FieldHandle {
                            ty: TypeHandle(i as u32),
                            index: j as u32,
                        },
                        descriptor: f.clone(),
                    })
                    .collect(),
            })
            .collect(),
    };
    // JSON 只作启动时的规范编码，不参与结算热路径。
    let hash = canonical_hash(&(
        &contributions,
        &options,
        &values,
        &slot_plans,
        &projection.types,
    ))?;
    Ok(ContributionPlan {
        hash,
        projection,
        rules: EffectiveRules {
            keys: rules.into_keys().collect(),
            values: values.into_boxed_slice(),
        },
        slots: slot_plans.into_boxed_slice(),
        intents: intents.into_values().collect(),
        bindings: bindings.into_iter().collect(),
        sources,
    })
}
