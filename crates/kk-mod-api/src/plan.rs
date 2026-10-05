use std::collections::BTreeMap;

use serde::Serialize;

use crate::*;

/// 句柄为计划内索引，调用方须先在作业/视图边界核对 PlanHash。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TypeHandle(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RuleHandle(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FieldHandle {
    pub(crate) ty: TypeHandle,
    pub(crate) index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct IntentHandle(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SlotHandle(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PlanHash(pub(crate) [u8; 32]);

impl PlanHash {
    pub fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// 按规则句柄直接读取的平坦数组；键解析只发生在装配时。
#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveRules {
    pub(crate) keys: Box<[Key]>,
    pub(crate) values: Box<[Number]>,
}

impl EffectiveRules {
    pub fn resolve(&self, key: &Key) -> Option<RuleHandle> {
        self.keys
            .binary_search(key)
            .ok()
            .map(|i| RuleHandle(i as u32))
    }

    pub fn get(&self, handle: RuleHandle) -> Option<&Number> {
        self.values.get(handle.0 as usize)
    }

    /// 稳定键序的全部有效条目；供宿主装配（如 JS 有效规则安装）遍历，
    /// 结算热路径仍应使用句柄读取。
    pub fn entries(&self) -> impl Iterator<Item = (&Key, &Number)> {
        self.keys.iter().zip(self.values.iter())
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Dispatch {
    Native(Key),
    Chained {
        before: Vec<Key>,
        wraps: Vec<Key>,
        center: Key,
        after: Vec<Key>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SlotPlan {
    pub key: Key,
    pub stage: Stage,
    pub dispatch: Dispatch,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProjectedField {
    pub handle: FieldHandle,
    pub descriptor: FieldDescriptor,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProjectedType {
    pub handle: TypeHandle,
    pub descriptor: ObjectTypeDescriptor,
    pub fields: Vec<ProjectedField>,
}

/// 字段目录保留权限信息；冻结计划不是已授权的玩家视图。
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionPlan {
    pub(crate) version: u16,
    pub(crate) types: Box<[ProjectedType]>,
}

impl ProjectionPlan {
    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn types(&self) -> &[ProjectedType] {
        &self.types
    }

    pub fn resolve(&self, key: &Key) -> Option<TypeHandle> {
        self.types
            .binary_search_by(|t| t.descriptor.key.cmp(key))
            .ok()
            .map(|i| TypeHandle(i as u32))
    }

    pub fn get(&self, handle: TypeHandle) -> Option<&ProjectedType> {
        self.types.get(handle.0 as usize)
    }

    pub fn field(&self, ty: TypeHandle, key: &Key) -> Option<FieldHandle> {
        let fields = &self.get(ty)?.fields;
        fields
            .binary_search_by(|f| f.descriptor.key.cmp(key))
            .ok()
            .map(|i| fields[i].handle)
    }

    pub fn get_field(&self, ty: TypeHandle, field: FieldHandle) -> Option<&ProjectedField> {
        if field.ty != ty {
            return None;
        }
        self.get(ty)?.fields.get(field.index as usize)
    }
}

#[derive(Clone, Debug)]
pub struct ContributionPlan {
    pub(crate) hash: PlanHash,
    pub(crate) rules: EffectiveRules,
    pub(crate) projection: ProjectionPlan,
    /// 计划声明的命名空间资源，排序键集合；内建 RESOURCES_ALL 不在其中，
    /// 由 [`crate::resources::ResourceCatalog`] 合并提供。随贡献内容参与
    /// PlanHash，不同的资源目录得到不同的计划身份。
    pub(crate) resources: Box<[Key]>,
    pub(crate) slots: Box<[SlotPlan]>,
    pub(crate) intents: Box<[IntentDescriptor]>,
    pub(crate) bindings: Box<[Key]>,
    pub(crate) sources: BTreeMap<String, Source>,
}

impl ContributionPlan {
    pub fn hash(&self) -> PlanHash {
        self.hash
    }
    pub fn rules(&self) -> &EffectiveRules {
        &self.rules
    }
    pub fn projection(&self) -> &ProjectionPlan {
        &self.projection
    }
    /// 计划声明的命名空间资源（不含内建 RESOURCES_ALL），排序键集合。
    pub fn resources(&self) -> &[Key] {
        &self.resources
    }
    pub fn slots(&self) -> &[SlotPlan] {
        &self.slots
    }
    pub fn intents(&self) -> &[IntentDescriptor] {
        &self.intents
    }
    pub fn resolve_intent(&self, key: &Key) -> Option<IntentHandle> {
        self.intents
            .binary_search_by(|i| i.key.cmp(key))
            .ok()
            .map(|i| IntentHandle(i as u32))
    }
    pub fn get_intent(&self, handle: IntentHandle) -> Option<&IntentDescriptor> {
        self.intents.get(handle.0 as usize)
    }
    pub fn resolve_slot(&self, key: &Key) -> Option<SlotHandle> {
        self.slots
            .binary_search_by(|s| s.key.cmp(key))
            .ok()
            .map(|i| SlotHandle(i as u32))
    }
    pub fn get_slot(&self, handle: SlotHandle) -> Option<&SlotPlan> {
        self.slots.get(handle.0 as usize)
    }
    /// 装配宿主须确认全部绑定实际存在，不能将声明视为可执行实现。
    pub fn required_bindings(&self) -> &[Key] {
        &self.bindings
    }
    pub fn sources(&self) -> &BTreeMap<String, Source> {
        &self.sources
    }
}
