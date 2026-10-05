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
/// 贡献里不得出现 `builtin` 命名空间——它是内建贡献的保留名，经
/// [freeze_with_builtin] 的独立参数进入；本入口自带拒绝，使「Mod 不得
/// 冒充 builtin」不依赖调用方恰好同时传入内建贡献触发撞名。
pub fn freeze(
    contributions: Vec<Contribution>,
    options: FreezeOptions,
) -> Result<ContributionPlan, FreezeError> {
    reject_builtin_namespace(&contributions)?;
    freeze_inner(contributions, options)
}

/// 带内建贡献的冻结：builtin 经显式参数与 Mod 贡献分离，产出的计划
/// 与「单一 vec 混装」完全一致（内部按 namespace 排序归一）。builtin
/// 参数由宿主可信构造（本仓测试会传入改写后的 link 做指纹/排序断言），
/// 这里只校验其命名空间，不校验内容来源。
pub fn freeze_with_builtin(
    builtin: Contribution,
    contributions: Vec<Contribution>,
    options: FreezeOptions,
) -> Result<ContributionPlan, FreezeError> {
    if builtin.source.namespace != "builtin" {
        return Err(FreezeError::new(
            builtin.source.namespace.clone(),
            "builtin 参数的命名空间必须是 builtin",
        ));
    }
    reject_builtin_namespace(&contributions)?;
    let mut all = Vec::with_capacity(contributions.len() + 1);
    all.push(builtin);
    all.extend(contributions);
    freeze_inner(all, options)
}

fn reject_builtin_namespace(contributions: &[Contribution]) -> Result<(), FreezeError> {
    for c in contributions {
        if c.source.namespace == "builtin" {
            return Err(FreezeError::new(
                "builtin",
                "builtin 命名空间保留给内建贡献：Mod 贡献不得声明（内建贡献走 freeze_with_builtin 的独立参数）",
            ));
        }
    }
    Ok(())
}

fn freeze_inner(
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
    // 挂载记录延后处理：目标类型可能在后续贡献中才声明。
    let mut mounts: Vec<(String, FieldMountDescriptor)> = Vec::new();
    let mut touched: BTreeSet<Key> = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    let mut types = BTreeMap::new();
    let mut rules = BTreeMap::new();
    let mut intents = BTreeMap::new();
    let mut slots = BTreeMap::new();
    let mut handlers = BTreeMap::new();
    let mut overrides: BTreeMap<Key, BTreeMap<String, Number>> = BTreeMap::new();
    let mut resources: BTreeSet<Key> = BTreeSet::new();
    let mut buildings: BTreeMap<Key, crate::buildings::BuildingDescriptor> = BTreeMap::new();
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
        c.field_mounts.sort_by(|a, b| a.field.key.cmp(&b.field.key));
        c.rules.sort_by(|a, b| a.key.cmp(&b.key));
        c.intents.sort_by(|a, b| a.key.cmp(&b.key));
        c.slots.sort_by(|a, b| a.key.cmp(&b.key));
        c.handlers.sort_by(|a, b| a.key.cmp(&b.key));
        c.overrides.sort_by(|a, b| a.rule.cmp(&b.rule));
        c.resources.sort_by(|a, b| a.key.cmp(&b.key));
        c.buildings.sort_by(|a, b| a.key.cmp(&b.key));
        for resource in &c.resources {
            owned(&resource.key, ns)?;
            if !resources.insert(resource.key.clone()) {
                return Err(FreezeError::new(resource.key.to_string(), "重复声明的资源"));
            }
        }
        for building in &c.buildings {
            owned(&building.key, ns)?;
            insert(&mut buildings, building.key.clone(), building.clone())?;
        }
        for key in &c.bindings {
            owned(key, ns)?;
            bindings.insert(key.clone());
        }
        for mount in &c.field_mounts {
            owned(&mount.field.key, ns)?;
            owned(&mount.field.binding, ns)?;
            let context = format!("{} / {}", mount.target.key, mount.field.key);
            validate::schema_depth(&mount.field.schema, options.max_schema_depth, &context)?;
            if let FieldDefault::Value(value) = &mount.field.default {
                validate::default_value(value, &mount.field.schema, &context)?;
            }
            mounts.push((ns.to_string(), mount.clone()));
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
    // 建筑目录的形状与引用校验：类型目录与资源目录此时已完整合并，
    // 跨包引用（成本/配方消费他包声明的资源）在这里一并定论。
    for building in buildings.values() {
        let context = building.key.to_string();
        if types.contains_key(&building.key) {
            return Err(FreezeError::new(
                context,
                "同一键不能既是对象类型又是建筑类型",
            ));
        }
        let known_resource = |name: &str| {
            Key::new(name)
                .map(|key| resources.contains(&key))
                .unwrap_or(false)
                || kk_core::constants::RESOURCES_ALL.contains(&name)
        };
        let check_entries = |entries: &[(String, u64)], what: &str| -> Result<(), FreezeError> {
            for (name, amount) in entries {
                if !known_resource(name) {
                    return Err(FreezeError::new(
                        context.clone(),
                        format!("建筑 {what} 引用未知资源 {name:?}（不在内建表或声明目录中）"),
                    ));
                }
                if *amount == 0 || *amount > 1_000_000_000 {
                    return Err(FreezeError::new(
                        context.clone(),
                        format!("建筑 {what} 的资源 {name:?} 数量须在 1..=1e9"),
                    ));
                }
            }
            Ok(())
        };
        if building.cost.is_empty() {
            return Err(FreezeError::new(context, "建筑成本至少声明一种资源"));
        }
        check_entries(&building.cost, "成本")?;
        if building.hits_max == 0 || building.hits_max > u32::MAX as u64 {
            return Err(FreezeError::new(
                context,
                format!("建筑耐久上限须在 1..={}（u32 投影域）", u32::MAX),
            ));
        }
        if building
            .store_capacity
            .is_some_and(|cap| cap == 0 || cap > u32::MAX as u64)
        {
            return Err(FreezeError::new(
                context,
                format!(
                    "建筑仓库容量须在 1..={}（u32 投影域；不建仓库请省略 capacity）",
                    u32::MAX
                ),
            ));
        }
        if let Some(production) = &building.production {
            if production.interval == 0 || production.interval > 100_000 {
                return Err(FreezeError::new(context, "生产周期须在 1..=100000 tick"));
            }
            check_entries(&production.inputs, "生产输入")?;
            check_entries(&production.outputs, "生产输出")?;
            if (!production.inputs.is_empty() || !production.outputs.is_empty())
                && building.store_capacity.is_none()
            {
                return Err(FreezeError::new(
                    context,
                    "有输入或产出的建筑必须声明仓库容量（否则库存无处存放）",
                ));
            }
            // 容量装不下单周期总量时生产永久无法触发（输入攒不齐或产出
            // 放不下），冻结期拒绝而不是留给运行期静默 no-op。
            if let Some(cap) = building.store_capacity {
                let inputs_total: u64 = production.inputs.iter().map(|(_, v)| *v).sum();
                let outputs_total: u64 = production.outputs.iter().map(|(_, v)| *v).sum();
                if cap < inputs_total || cap < outputs_total {
                    return Err(FreezeError::new(
                        context,
                        format!(
                            "仓库容量 {cap} 须能容纳单周期的全部输入（{inputs_total}）与全部产出（{outputs_total}），否则生产永远无法触发"
                        ),
                    ));
                }
            }
        }
    }
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
    for (ns, mount) in mounts {
        // 挂载通道不得使用 builtin 绑定冒充内建适配字段。
        if mount.field.binding.namespace() == "builtin" {
            return Err(FreezeError::new(
                ns,
                format!(
                    "挂载字段 {} 的绑定不能占用 builtin 命名空间",
                    mount.field.key
                ),
            ));
        }
        let target = types.get_mut(&mount.target.key).ok_or_else(|| {
            FreezeError::new(
                ns.clone(),
                format!("字段挂载的目标类型不存在：{}", mount.target.key),
            )
        })?;
        if target.schema_version != mount.target.schema_version {
            return Err(FreezeError::new(
                ns,
                format!(
                    "字段挂载的版本不匹配：{} 期望 {}，实得 {}",
                    mount.target.key, mount.target.schema_version, target.schema_version
                ),
            ));
        }
        if target.fields.iter().any(|f| f.key == mount.field.key) {
            return Err(FreezeError::new(
                ns,
                format!("字段挂载重复：{} / {}", mount.target.key, mount.field.key),
            ));
        }
        touched.insert(mount.target.key.clone());
        target.fields.push(mount.field);
        target.fields.sort_by(|a, b| a.key.cmp(&b.key));
    }
    // 挂载并入后重跑类型级校验：挂载字段与自带字段经过同一套
    // schema 引用/绑定/owner 权限检查，不因挂载通道而绕过。
    for ty in types.values() {
        if !touched.contains(&ty.key) {
            continue;
        }
        validate::object(ty, &types, &bindings, &slots, options.max_schema_depth)?;
    }
    // Mod 类型的字段 payload 以裸名存取，挂载可引入跨命名空间的同名
    // 字段（全键不同、裸名相同）——同一 payload 键会被要求同时满足两
    // 个 schema。此处冻结期拒绝；builtin 类型的扩展字段按全键存储，
    // 不受此约束。
    for ty in types.values() {
        if ty.key.namespace() == "builtin" {
            continue;
        }
        let mut bare: BTreeSet<&str> = BTreeSet::new();
        for field in &ty.fields {
            let name = validate::field_name(&field.key);
            if !bare.insert(name) {
                return Err(FreezeError::new(
                    ty.key.to_string(),
                    format!("字段裸名 {name} 冲突（payload 以裸名存取，同类型内须唯一）"),
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
        resources: resources.into_iter().collect(),
        buildings: buildings.into_values().collect::<Box<[_]>>(),
        slots: slot_plans.into_boxed_slice(),
        intents: intents.into_values().collect(),
        bindings: bindings.into_iter().collect(),
        sources,
    })
}
