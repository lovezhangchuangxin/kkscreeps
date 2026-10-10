//! 扩展字段的运行期目录：从冻结计划提取可在已有对象上写入的
//! 非内建字段及其权限。存储键使用字段稳定键全名（namespace:name），
//! 命名空间隔离使扩展键不能与 `builtin:` 内建字段冲突。

use std::collections::BTreeMap;

use crate::*;

/// 一个扩展字段的写入与投影规则。
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionField {
    /// 字段稳定键全名（namespace:name），也是存储键与投影键。
    pub key: Key,
    /// 字段绑定键；目录本身就是该绑定的安装。
    pub binding: Key,
    /// 投影可见性；条件权限在冻结时拒绝，目录内不会出现。
    pub visibility: Visibility,
    /// 值的形态与限额；写入时逐值校验。
    pub schema: ValueSchema,
}

/// 全部类型的扩展字段目录。写入入口按类型键与字段键查规则，
/// 不逐值解析描述。无扩展贡献的计划得到空目录。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionCatalog {
    fields: BTreeMap<Key, BTreeMap<Key, ExtensionField>>,
}

impl ExtensionCatalog {
    pub fn from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        let mut catalog: BTreeMap<Key, BTreeMap<Key, ExtensionField>> = BTreeMap::new();
        for ty in plan.projection().types() {
            // 扩展目录只承载内建类型的追加字段（`room.extensions` 存储）。
            // Mod 声明类型的字段——含挂载到 Mod 类型上的字段——整体存放在
            // 对象 payload 中随类型描述符校验，不进本目录；其默认值由
            // 创建与处理器写入，不受扩展字段"禁默认值"约束。
            if ty.descriptor.key.namespace() != "builtin" {
                continue;
            }
            for field in &ty.fields {
                let descriptor = &field.descriptor;
                // 内建适配字段直接访问权威对象，不进扩展表。
                if descriptor.binding.namespace() == "builtin" {
                    continue;
                }
                if descriptor.default != FieldDefault::Missing {
                    return Err(format!(
                        "扩展字段 {} 声明了默认值；默认值只能由处理器或初始化写入，\
                         目录不自动落值",
                        descriptor.key
                    ));
                }
                if matches!(descriptor.visibility, Visibility::Conditional(_)) {
                    return Err(format!(
                        "扩展字段 {} 使用条件权限；条件投影判定入口尚未提供，\
                         暂不能声明",
                        descriptor.key
                    ));
                }
                catalog
                    .entry(ty.descriptor.key.clone())
                    .or_default()
                    .insert(
                        descriptor.key.clone(),
                        ExtensionField {
                            key: descriptor.key.clone(),
                            binding: descriptor.binding.clone(),
                            visibility: descriptor.visibility.clone(),
                            schema: descriptor.schema.clone(),
                        },
                    );
            }
        }
        Ok(Self { fields: catalog })
    }

    /// 查类型的扩展字段；未声明该类型的扩展时返回空表。
    pub fn fields_of(&self, type_key: &Key) -> &BTreeMap<Key, ExtensionField> {
        static EMPTY: std::sync::OnceLock<BTreeMap<Key, ExtensionField>> =
            std::sync::OnceLock::new();
        self.fields
            .get(type_key)
            .unwrap_or_else(|| EMPTY.get_or_init(BTreeMap::new))
    }

    /// 按稳定键查字段规则；类型键限定字段所属类型。
    pub fn field(&self, type_key: &Key, field_key: &Key) -> Option<&ExtensionField> {
        self.fields.get(type_key)?.get(field_key)
    }

    /// 按结构类型短名查内建类型键（builtin:{structure_type}）。
    /// 目录未声明该类型的扩展时返回 None。空目录免拼键直返 None——
    /// 共享房间块构建用空目录逐对象查询，避免每次 format! 分配与键
    /// 校验（采样热点 ~5ms/拍）。
    pub fn type_for_structure(&self, structure_type: &str) -> Option<&Key> {
        if self.fields.is_empty() {
            return None;
        }
        let key = Key::new(format!("builtin:{structure_type}")).ok()?;
        self.fields.get_key_value(&key).map(|(k, _)| k)
    }

    /// 目录内全部字段的绑定键；装配核对"声明绑定已安装"时与
    /// 处理器键一并视为已安装——扩展字段的实现就是目录与写入门。
    pub fn field_bindings(&self) -> impl Iterator<Item = &Key> {
        self.fields
            .values()
            .flat_map(|fields| fields.values().map(|f| &f.binding))
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}
