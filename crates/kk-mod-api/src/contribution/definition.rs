use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Serialize;
use serde_json::Value;

/// 稳定身份；整数句柄与编码 tag 不能替代它。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Key(String);

impl<'de> serde::Deserialize<'de> for Key {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl Key {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let Some((namespace, name)) = value.split_once(':') else {
            return Err("键须为 namespace:name".into());
        };
        if value.len() > 128
            || !valid_namespace(namespace)
            || !name.starts_with(|c: char| c.is_ascii_alphabetic())
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err(format!("无效稳定键 {value:?}"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn namespace(&self) -> &str {
        self.0.split_once(':').expect("Key 构造已校验").0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn valid_namespace(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_lowercase())
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
}

/// 摘要由装配者提供；冻结器不读取文件，也不按包版本猜测源码是否变化。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Source {
    pub namespace: String,
    pub api_version: u32,
    pub revision: String,
    pub code_digest: [u8; 32],
    pub config_digest: [u8; 32],
    pub player_api_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Number {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum NumberDomain {
    Signed { min: i64, max: i64 },
    Unsigned { min: u64, max: u64 },
    Float { min: f64, max: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeRef {
    pub key: Key,
    pub schema_version: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValueSchema {
    pub kind: ValueKind,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ValueKind {
    Bool,
    Number(NumberDomain),
    String {
        max_bytes: u32,
    },
    ObjectRef(TypeRef),
    Enum(BTreeSet<String>),
    Array {
        element: Box<ValueSchema>,
        max_len: u32,
    },
    Map {
        value: Box<ValueSchema>,
        max_len: u32,
        max_key_bytes: u32,
    },
    /// 未列入 optional 的成员必需存在；可选与 nullable 分别表示缺失和 null。
    Record {
        fields: BTreeMap<String, ValueSchema>,
        optional: BTreeSet<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum FieldDefault {
    Missing,
    Value(Value),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Visibility {
    Public,
    Owner,
    ServerOnly,
    /// 具名判定绑定；权限须在宿主编码时计算，不能交给玩家 getter 判定。
    Conditional(Key),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ObjectVisibility {
    RoomVisible,
    OwnerCollection,
    ServerOnly,
    Conditional(Key),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldDescriptor {
    pub key: Key,
    pub schema: ValueSchema,
    pub default: FieldDefault,
    pub visibility: Visibility,
    pub binding: Key,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Capability {
    pub key: Key,
    pub consumers: BTreeSet<Key>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObjectTypeDescriptor {
    pub key: Key,
    pub schema_version: u32,
    pub owner_field: Option<Key>,
    pub position_fields: Option<[Key; 2]>,
    pub fields: Vec<FieldDescriptor>,
    pub capabilities: Vec<Capability>,
    pub lifecycle_slots: BTreeSet<Key>,
    pub visibility: ObjectVisibility,
    pub player_class: String,
    /// 总库存容量声明（G09.a）：Some 时对象创建即携带 store/storeCapacity
    /// 信封键（初始 store 为 {"energy":0}，与建筑完工同形），库存只能经
    /// 资源动作（creep transfer/withdraw、mod 资源槽）变动，字段更新拒绝
    /// 写信封键。容量随类型冻结，不进 StoreRules（那是内建结构规则族）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_capacity: Option<u64>,
    /// 声明式反击（G09.d）：与耐久声明组合的类型级反击比率（毫精度，
    /// 1..=1000 即 0.001..=1 倍），任意攻击类型触发（官方 powerBank
    /// damage×0.5 同构）。反击不耗自身耐久；creep 攻击者落延迟队列、
    /// 塔/结构攻击者立即扣减且禁递归。无运行时字段（不占信封/保留键）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retaliation_milli: Option<i64>,
    /// 耐久声明（G09.b）：Some 时对象创建即携带 hits/hitsMax 信封键
    /// （满血创建，hits=hitsMax=声明值），可被 creep 攻击与塔伤害命中，
    /// 扣至 0 即移除并发 objectDestroyed 事件（type 为类型键；不产废墟，
    /// 废墟绑定建筑成本模型）。耐久只能经伤害动作变动，字段更新拒绝
    /// 写信封键。与建筑 hits_max 同口径（>0 且 ≤u32::MAX）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hits_max: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Stage {
    ObjectAction,
    Movement,
    ObjectTick,
    Market,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum IntentScope {
    Object,
    Room,
    Player,
    World,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Merge {
    Last,
    Queue { max_len: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ArgumentConversion {
    Strict,
    /// 复用已有 vanilla 清洗器，不重新定义其 parseInt 等转换。
    Native(Key),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IntentDescriptor {
    pub key: Key,
    pub scope: IntentScope,
    pub receiver: Option<TypeRef>,
    pub arguments: ValueSchema,
    pub conversion: ArgumentConversion,
    pub authority: IntentAuthority,
    pub merge: Merge,
    pub slot: Key,
    pub suppressed_by: BTreeSet<Key>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum IntentAuthority {
    Player,
    ServerOnly,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuleDescriptor {
    pub key: Key,
    pub domain: NumberDomain,
    pub default: Number,
    pub consumers: BTreeSet<Key>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuleOverride {
    pub rule: Key,
    pub value: Number,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum RuleConstraint {
    LessOrEqual { left: Key, right: Key },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SlotDescriptor {
    pub key: Key,
    pub stage: Stage,
    pub native_binding: Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum HandlerKind {
    Before,
    Wrap,
    Replace,
    After,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HandlerDescriptor {
    pub key: Key,
    pub slot: Key,
    pub kind: HandlerKind,
    pub binding: Key,
    /// 同一 slot、同一种 kind 的拓扑约束；After 在运行时逆序执行。
    pub depends_on: BTreeSet<Key>,
}

/// 把本贡献的字段挂载到其他贡献声明的已有类型上。
/// 类型主体归其命名空间所有；挂载只追加字段，不改类型归属，
/// 也不给目标类型添加行为。目标类型须在计划中存在且版本匹配。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldMountDescriptor {
    pub target: TypeRef,
    pub field: FieldDescriptor,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Contribution {
    pub source: Source,
    /// 声明宿主必须安装的绑定标识，不包含函数指针或 JS 函数。
    pub bindings: BTreeSet<Key>,
    pub object_types: Vec<ObjectTypeDescriptor>,
    /// 挂载到已有类型上的扩展字段；字段键与绑定归本贡献命名空间。
    pub field_mounts: Vec<FieldMountDescriptor>,
    pub intents: Vec<IntentDescriptor>,
    pub rules: Vec<RuleDescriptor>,
    pub overrides: Vec<RuleOverride>,
    pub constraints: BTreeSet<RuleConstraint>,
    pub slots: Vec<SlotDescriptor>,
    pub handlers: Vec<HandlerDescriptor>,
    /// 本包声明的命名空间资源；键归本包命名空间，进 PlanHash 与玩家常量。
    pub resources: Vec<crate::resources::ResourceDescriptor>,
    /// 本包声明的可建造建筑（纯数据配方）；键归本包命名空间，进 PlanHash，
    /// 由宿主通用执行器消费（工地/建造/耐久/库存/生产/寻路）。
    pub buildings: Vec<crate::buildings::BuildingDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FreezeOptions {
    pub projection_version: u16,
    pub max_schema_depth: u16,
    /// 规则键 -> 被服主选中的贡献 namespace；不指定时多方覆盖报错。
    pub rule_sources: BTreeMap<Key, String>,
}

impl Default for FreezeOptions {
    fn default() -> Self {
        Self {
            projection_version: 1,
            max_schema_depth: 16,
            rule_sources: BTreeMap::new(),
        }
    }
}
