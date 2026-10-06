//! 冻结计划的玩家面 TypeScript 声明生成（`mod types`）。
//!
//! 生成物是纯类型声明（.d.ts）：Mod 对象类型的投影 interface、安装 API
//! 的窄化签名（bind/emit/install 按冻结键字面量化）、声明资源联合与
//! 内建类清单。输出对同一计划确定性；头部只引用声明相关的事实
//! （包 namespace@revision 与声明清单），不写计划指纹——包内任意源码
//! 变化都会改变指纹，若头部携带会造成永假的"过期"判定。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::definition::{NumberDomain, ObjectTypeDescriptor, ValueKind, ValueSchema, Visibility};
use crate::plan::ContributionPlan;
use crate::player_surface::PlayerSurface;

/// Mod 玩家类可继承的内建类名。与玩家面安装运行时
/// （kk-runner `prelude/41_mod_player.js` 的 BUILTIN_CLASS_NAMES）同源，
/// kk-cli 测试断言两处一致；新增内建类须同时改这里。
pub const MOD_BUILTIN_PLAYER_CLASSES: [&str; 38] = [
    "ConstructionSite",
    "Creep",
    "Deposit",
    "Flag",
    "Mineral",
    "Nuke",
    "OwnedStructure",
    "PowerCreep",
    "Resource",
    "Ruin",
    "Room",
    "RoomObject",
    "RoomPosition",
    "Source",
    "Store",
    "Structure",
    "StructureContainer",
    "StructureController",
    "StructureExtension",
    "StructureExtractor",
    "StructureFactory",
    "StructureInvaderCore",
    "StructureKeeperLair",
    "StructureLab",
    "StructureLink",
    "StructureNuker",
    "StructureObserver",
    "StructurePortal",
    "StructurePowerBank",
    "StructurePowerSpawn",
    "StructureRampart",
    "StructureRoad",
    "StructureSpawn",
    "StructureStorage",
    "StructureTerminal",
    "StructureTower",
    "StructureWall",
    "Tombstone",
];

/// 生成玩家面声明文本。PlayerSurface/建筑目录的既有校验失败会原样
/// 传播（不产出半截声明）；意图域当前只接线对象域，与装配口径一致。
pub fn player_declarations(plan: &ContributionPlan) -> Result<String, String> {
    let surface = PlayerSurface::from_plan(plan)?;
    let buildings = crate::buildings::BuildingCatalog::from_plan(plan)?;
    let resources = crate::resources::ResourceCatalog::from_plan(plan);
    let by_key: BTreeMap<&str, &ObjectTypeDescriptor> = plan
        .projection()
        .types()
        .iter()
        .map(|projected| (projected.descriptor.key.as_str(), &projected.descriptor))
        .collect();
    for intent in plan.intents() {
        // 与引擎装配共用意图描述符门：域/合并/转换/授权/压制同口径
        // 拒绝；具体入口和执行绑定仍由引擎装配核验。
        if intent.key.namespace() != "builtin" {
            crate::intent_gate::executable_gate(intent)?;
        }
    }
    let mut out = String::new();
    let sources: Vec<String> = plan
        .sources()
        .iter()
        .filter(|(namespace, _)| namespace.as_str() != "builtin")
        .map(|(namespace, source)| format!("{namespace}@{}", source.revision))
        .collect();
    let _ = writeln!(
        out,
        "// 由 kkscreeps mod types 从冻结计划生成；请勿手改，过期用同命令刷新。"
    );
    let _ = writeln!(
        out,
        "// 刷新: kkscreeps mod types --root <Mod根> --out <本文件>（--select/--exclude 与生成时保持一致）"
    );
    let _ = writeln!(
        out,
        "// 声明: 包 {}；对象类型 {}、建筑 {}、意图 {}、资源 {}",
        if sources.is_empty() {
            "（无）".to_string()
        } else {
            sources.join(", ")
        },
        surface.types.len(),
        buildings.iter().count(),
        surface.intents.len(),
        resources.declared_resources().len(),
    );
    out.push_str(
        "// 说明: 声明文件放在 Mod 包目录内会改变计划指纹（包内全部文件字节计入\n\
         // 身份），推荐放在包外由 --out 指定；IDE 经 tsconfig paths 或 JSDoc import 引用。\n\
         // 本文件是全计划联合面：安装 API 在运行时按各包命名空间裁剪——某包的\n\
         // player.js 只能 bind/emit 本包命名空间的键，跨包键类型通过但运行时拒绝。\n\n",
    );
    let symbols = Symbols::new(plan, &surface, &buildings);
    emit_types(&mut out, &surface, &by_key, &symbols)?;
    let args_names = &symbols.arguments;
    emit_intents(&mut out, plan, &surface, args_names);
    emit_buildings(&mut out, &buildings, &symbols);
    emit_install_api(&mut out, &surface, plan, &buildings, args_names, &symbols);
    emit_resource_union(&mut out, &resources);
    emit_builtin_classes(&mut out);
    Ok(out)
}

fn doc_lines(out: &mut String, indent: &str, text: &str) {
    for line in text.lines() {
        // 字段名/枚举值等作者输入嵌入注释：`*/` 会提前闭合 JSDoc，
        // 破坏性弱化但可读性优先保留原文，替换为不断开的斜杠写法。
        let safe = line.replace("*/", "*\u{2044}");
        let _ = writeln!(out, "{indent}/** {safe} */");
    }
}

/// 属性名按 TS 形态输出：标识符裸写，其余（kebab/数字开头/含空格的
/// 合法 Key 名）退化为带引号的字符串字面量键。
fn ts_property(name: &str) -> String {
    let identifier = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if identifier {
        name.to_string()
    } else {
        format!("'{}'", ts_string(name))
    }
}

/// 字符串字面量转义（枚举成员与引号键共用）。
fn ts_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

fn emit_types(
    out: &mut String,
    surface: &PlayerSurface,
    by_key: &BTreeMap<&str, &ObjectTypeDescriptor>,
    symbols: &Symbols,
) -> Result<(), String> {
    if surface.types.is_empty() {
        return Ok(());
    }
    let _ = writeln!(
        out,
        "// —— Mod 对象类型投影（当轮已授权视野的字段；ServerOnly 字段不出现）——"
    );
    for ty in &surface.types {
        let descriptor = *by_key
            .get(ty.key.as_str())
            .ok_or_else(|| format!("玩家面类型 {} 不在计划投影中（目录与投影不一致）", ty.key))?;
        let scope = match ty.visibility {
            crate::definition::ObjectVisibility::RoomVisible => "房间可见",
            crate::definition::ObjectVisibility::OwnerCollection => "归属者集合可见",
            crate::definition::ObjectVisibility::ServerOnly => unreachable!("Surface 已过滤"),
            crate::definition::ObjectVisibility::Conditional(_) => unreachable!("Surface 已拒绝"),
        };
        doc_lines(
            out,
            "",
            &format!(
                "{} 的玩家投影（{scope}，运行时玩家类 {}）；查询常量经 api.queries.install('{}')。",
                ty.key, ty.player_class, ty.key
            ),
        );
        let _ = writeln!(
            out,
            "export interface {} {{",
            symbols.objects[ty.key.as_str()]
        );
        let _ = writeln!(out, "  id: string;");
        let _ = writeln!(out, "  room: string;");
        let _ = writeln!(out, "  x: number;");
        let _ = writeln!(out, "  y: number;");
        let _ = writeln!(out, "  pos: {{ x: number; y: number; roomName: string }};");
        let _ = writeln!(out, "  type: 'mod';");
        let _ = writeln!(out, "  modType: '{}';", ty.key);
        for field in &descriptor.fields {
            let field_name = crate::validate::field_name(&field.key);
            // 投影把信封键最后写入（world_json mod_record_json），同名
            // 声明字段被等值覆写（位置字段）或遮蔽（病理命名）——玩家面
            // 只按信封形态出现一次，不重复声明。
            if matches!(
                field_name,
                "id" | "room" | "x" | "y" | "pos" | "type" | "modType"
            ) {
                continue;
            }
            match field.visibility {
                Visibility::Public => {}
                Visibility::Owner => {}
                Visibility::ServerOnly | Visibility::Conditional(_) => continue,
            }
            let kind_note = kind_note(&field.schema);
            let owner_note = matches!(field.visibility, Visibility::Owner)
                .then(|| "仅归属者可见（非归属者投影不含此键，读取得 undefined）。")
                .unwrap_or("");
            let note = match (kind_note.is_empty(), owner_note.is_empty()) {
                (true, true) => String::new(),
                (false, true) => format!("{field_name}：{kind_note}。"),
                (true, false) => format!("{field_name}：{owner_note}"),
                (false, false) => format!("{field_name}：{kind_note}；{owner_note}"),
            };
            if !note.is_empty() {
                doc_lines(out, "  ", &note);
            }
            let optional = if matches!(field.visibility, Visibility::Owner) {
                "?"
            } else {
                ""
            };
            let ts = ts_type(&field.schema);
            let _ = writeln!(out, "  {}{optional}: {ts};", ts_property(field_name));
        }
        let _ = writeln!(out, "}}");
        let _ = writeln!(
            out,
            "/** api.objects.bind('{}') 需要的构造器形态：运行时仅作原型载体（从不构造），字段读取走当轮投影。 */",
            ty.key
        );
        let _ = writeln!(
            out,
            "export type {} = new (...args: never[]) => unknown;",
            symbols.constructors[ty.key.as_str()]
        );
        out.push('\n');
    }
    Ok(())
}

fn emit_intents(
    out: &mut String,
    plan: &ContributionPlan,
    surface: &PlayerSurface,
    args_names: &BTreeMap<String, String>,
) {
    if surface.intents.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "// —— Mod 意图参数（对象域；emit 经玩家模块安装的类方法或原语发出）——"
    );
    for key in &surface.intents {
        let descriptor = plan
            .intents()
            .iter()
            .find(|intent| &intent.key == key)
            .expect("Surface 意图目录来自计划");
        let args_name = &args_names[descriptor.key.as_str()];
        match &descriptor.arguments.kind {
            ValueKind::Record { .. } => {
                doc_lines(out, "", &format!("{} 的参数对象。", descriptor.key));
                emit_record_interface(out, args_name, &descriptor.arguments);
            }
            _ => {
                doc_lines(
                    out,
                    "",
                    &format!("{} 的参数（顶层内联形态）。", descriptor.key),
                );
                let _ = writeln!(
                    out,
                    "export type {args_name} = {};",
                    ts_type(&descriptor.arguments)
                );
            }
        }
        out.push('\n');
    }
}

/// 意图参数类型命名：键的每段（非字母数字都是段边界）Pascal 化拼接，
/// 非标识符字符全部消失（`alpha:sa.lute`→AlphaSaLute）。段化会让
/// 不同的键坍缩为同名（`a-b:c` 与 `a:b-c`），因此按计划序预计算并对
/// 撞名追加序号，两个产出段（参数 interface 与 emit 签名）共用同一
/// 份映射。
fn args_type_names(
    plan: &ContributionPlan,
    used: &mut BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    for intent in plan.intents() {
        if intent.key.namespace() == "builtin" {
            continue;
        }
        let pascal: String = intent
            .key
            .as_str()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect();
        let mut name = format!("{}Args", if pascal.is_empty() { "Intent" } else { &pascal });
        let mut suffix = 1;
        while !used.insert(name.clone()) {
            suffix += 1;
            name = format!(
                "{}Args{suffix}",
                if pascal.is_empty() { "Intent" } else { &pascal }
            );
        }
        names.insert(intent.key.as_str().to_string(), name);
    }
    names
}

fn emit_record_interface(out: &mut String, name: &str, schema: &ValueSchema) {
    let ValueKind::Record { fields, optional } = &schema.kind else {
        unreachable!("Record 分支已在上游判别");
    };
    let _ = writeln!(out, "export interface {name} {{");
    for (field, field_schema) in fields {
        let opt = if optional.contains(field) { "?" } else { "" };
        let note = kind_note(field_schema);
        if !note.is_empty() {
            doc_lines(out, "  ", &format!("{field}：{note}。"));
        }
        // ts_type 已按 schema.nullable 拼 | null，这里不再重复。
        let _ = writeln!(
            out,
            "  {}{opt}: {};",
            ts_property(field),
            ts_type(field_schema)
        );
    }
    let _ = writeln!(out, "}}");
}

fn emit_buildings(
    out: &mut String,
    catalog: &crate::buildings::BuildingCatalog,
    symbols: &Symbols,
) {
    let mut buildings: Vec<_> = catalog.iter().collect();
    if buildings.is_empty() {
        return;
    }
    buildings.sort_by_key(|building| building.key.to_string());
    let _ = writeln!(out, "// —— Mod 建筑（api.buildings.bind 绑定玩家类）——");
    for building in buildings {
        let pathing = match building.pathing {
            crate::buildings::BuildingPathing::Obstacle => "阻挡",
            crate::buildings::BuildingPathing::Plain => "平地",
            crate::buildings::BuildingPathing::Road => "道路",
        };
        doc_lines(
            out,
            "",
            &format!(
                "{} 的玩家构造器（建筑；成本 {}；寻路{pathing}）。",
                building.key,
                building
                    .cost
                    .iter()
                    .map(|(resource, amount)| format!("{resource}×{amount}"))
                    .collect::<Vec<_>>()
                    .join("、"),
            ),
        );
        let _ = writeln!(
            out,
            "export type {} = new (...args: never[]) => unknown;",
            symbols.constructors[building.key.as_str()]
        );
        // 自定义字段经存储键（含命名空间冒号）直供玩家；声明为独立
        // 接口供玩家交叉类型使用，Missing 默认（记录可缺键）为可选成员。
        if !building.fields.is_empty() {
            let ctor = &symbols.constructors[building.key.as_str()];
            doc_lines(
                out,
                "",
                &format!(
                    "{} 的自定义字段：实例经存储键直供读取（如 obj['{}']），记录缺键为 undefined。",
                    building.key,
                    building.fields[0].key.as_str()
                ),
            );
            let members: Vec<String> = building
                .fields
                .iter()
                .map(|field| {
                    let optional = if matches!(field.default, crate::FieldDefault::Missing) {
                        "?"
                    } else {
                        ""
                    };
                    format!(
                        "{}{optional}: {}",
                        ts_property(field.key.as_str()),
                        ts_type(&field.schema)
                    )
                })
                .collect();
            let _ = writeln!(
                out,
                "export interface {ctor}Fields {{ {} }}",
                members.join("; ")
            );
        }
    }
    out.push('\n');
}

fn emit_install_api(
    out: &mut String,
    surface: &PlayerSurface,
    plan: &ContributionPlan,
    buildings: &crate::buildings::BuildingCatalog,
    args_names: &BTreeMap<String, String>,
    symbols: &Symbols,
) {
    let _ = writeln!(
        out,
        "// —— 玩家模块安装 API（default(api) 的 api，按本计划窄化）——"
    );
    let _ = writeln!(out, "export interface ModInstallApi {{");
    let _ = writeln!(
        out,
        "  /** 安装期捕获的内建类表（供玩家类继承；替换全局绑定不影响此处）。 */"
    );
    let _ = writeln!(
        out,
        "  builtins: Readonly<Record<ModBuiltinClassName, ModBuiltinClass>>;"
    );
    if surface.types.is_empty() {
        let _ = writeln!(out, "  objects: {{}};");
    } else {
        let _ = writeln!(out, "  objects: {{");
        for ty in &surface.types {
            let _ = writeln!(
                out,
                "    bind(typeKey: '{}', cls: {}): void;",
                ty.key,
                symbols.constructors[ty.key.as_str()]
            );
        }
        let _ = writeln!(out, "  }};");
    }
    let mut building_list: Vec<_> = buildings.iter().collect();
    building_list.sort_by_key(|building| building.key.to_string());
    if building_list.is_empty() {
        let _ = writeln!(out, "  buildings: {{}};");
    } else {
        let _ = writeln!(out, "  buildings: {{");
        for building in building_list {
            let _ = writeln!(
                out,
                "    bind(typeKey: '{}', cls: {}): void;",
                building.key,
                symbols.constructors[building.key.as_str()]
            );
        }
        let _ = writeln!(out, "  }};");
    }
    let _ = writeln!(
        out,
        "  globals: {{ define(name: string, value: unknown): void }};"
    );
    let _ = writeln!(
        out,
        "  /** 动态字段逃生舱：读当轮已授权投影的裸字段；字段类型见各 interface。 */"
    );
    let _ = writeln!(
        out,
        "  view: {{ field(obj: {{ id: string }}, name: string): unknown }};"
    );
    if surface.intents.is_empty() {
        let _ = writeln!(out, "  intents: {{}};");
    } else {
        let _ = writeln!(out, "  intents: {{");
        for key in &surface.intents {
            let descriptor = plan
                .intents()
                .iter()
                .find(|intent| &intent.key == key)
                .expect("Surface 意图目录来自计划");
            let args_name = &args_names[key.as_str()];
            let receiver = descriptor
                .receiver
                .as_ref()
                .map(|r| format!("（接收者 {}）", r.key))
                .unwrap_or_default();
            // 运行时把缺省 args 归一为 null：仅 nullable schema 允许省略
            // （非 nullable 时省略会在结算侧被参数校验拒绝）。内联形态
            // 的 ts_type 已含 `| null`，只有 Record 引用名需要签名补拼。
            let args_nullable = descriptor.arguments.nullable;
            let optional_arg = if args_nullable { "?: " } else { ": " };
            let null_suffix = match (&descriptor.arguments.kind, args_nullable) {
                (ValueKind::Record { .. }, true) => " | null",
                _ => "",
            };
            doc_lines(
                out,
                "    ",
                &format!("返回 OK/ERR_INVALID_ARGS；参数形态见 {args_name}{receiver}。"),
            );
            let _ = writeln!(
                out,
                "    emit(intentKey: '{}', target: {{ id: string }}, args{}{}{}): number;",
                key, optional_arg, args_name, null_suffix
            );
        }
        let _ = writeln!(out, "  }};");
    }
    if surface.types.is_empty() {
        let _ = writeln!(out, "  queries: {{}};");
    } else {
        let _ = writeln!(out, "  queries: {{");
        for ty in &surface.types {
            let _ = writeln!(
                out,
                "    install(typeKey: '{}'): {{ find: {}; look: '{}' }};",
                ty.key, ty.find_const, ty.look_key
            );
        }
        let _ = writeln!(out, "  }};");
    }
    let _ = writeln!(out, "}}");
    out.push('\n');
}

fn emit_resource_union(out: &mut String, catalog: &crate::resources::ResourceCatalog) {
    let declared = catalog.declared_resources();
    if declared.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "// —— 本计划声明的命名空间资源（RESOURCES_ALL 的 Mod 部分）——"
    );
    let members: Vec<String> = declared
        .iter()
        .map(|key| format!("'{}'", key.as_str()))
        .collect();
    let _ = writeln!(
        out,
        "export type ModResourceName = {};",
        members.join(" | ")
    );
    out.push('\n');
}

fn emit_builtin_classes(out: &mut String) {
    let _ = writeln!(
        out,
        "// —— 内建类清单（与玩家面安装运行时同源；kk-cli 测试断言一致）——"
    );
    let members: Vec<String> = MOD_BUILTIN_PLAYER_CLASSES
        .iter()
        .map(|name| format!("'{name}'"))
        .collect();
    let _ = writeln!(
        out,
        "export type ModBuiltinClassName = {};\n",
        members.join(" | ")
    );
    // 实例类型须为对象类型：玩家类 `class X extends api.builtins.Room`
    // 才能在 strict 下作为基类使用（unknown 会被 TS 拒绝继承）。
    let _ = writeln!(
        out,
        "export type ModBuiltinClass = abstract new (...args: never[]) => object;"
    );
}

/// ValueKind → TS 类型表达式。ObjectRef 投影为对象 id 字符串。
fn ts_type(schema: &ValueSchema) -> String {
    if schema.nullable {
        return format!("{} | null", ts_kind(&schema.kind));
    }
    ts_kind(&schema.kind)
}

fn ts_kind(kind: &ValueKind) -> String {
    match kind {
        ValueKind::Bool => "boolean".into(),
        ValueKind::Number(_) => "number".into(),
        ValueKind::String { .. } => "string".into(),
        ValueKind::ObjectRef(r) => format!("string /* {} */", r.key),
        ValueKind::Enum(values) => values
            .iter()
            .map(|value| format!("'{}'", ts_string(value)))
            .collect::<Vec<_>>()
            .join(" | "),
        ValueKind::Array { element, .. } => format!("ReadonlyArray<{}>", ts_type(element)),
        ValueKind::Map { value, .. } => format!("Record<string, {}>", ts_type(value)),
        ValueKind::Record { fields, optional } => {
            let members: Vec<String> = fields
                .iter()
                .map(|(name, field_schema)| {
                    let opt = if optional.contains(name) { "?" } else { "" };
                    format!("{}{opt}: {}", ts_property(name), ts_type(field_schema))
                })
                .collect();
            format!("{{ {} }}", members.join("; "))
        }
    }
}

/// 字段注释里的数值域/长度/引用说明；空串表示无补充。
fn kind_note(schema: &ValueSchema) -> String {
    let mut notes = Vec::new();
    match &schema.kind {
        ValueKind::Number(NumberDomain::Signed { min, max }) => {
            notes.push(format!("整数 {min}..={max}"));
        }
        ValueKind::Number(NumberDomain::Unsigned { min, max }) => {
            notes.push(format!("整数 {min}..={max}"));
        }
        ValueKind::Number(NumberDomain::Float { min, max }) => {
            notes.push(format!("浮点 {min}..={max}"));
        }
        ValueKind::String { max_bytes } => notes.push(format!("最长 {max_bytes} 字节")),
        ValueKind::ObjectRef(r) => notes.push(format!("对象 {} 的 id", r.key)),
        ValueKind::Array { max_len, .. } => notes.push(format!("至多 {max_len} 项")),
        ValueKind::Map {
            max_len,
            max_key_bytes,
            ..
        } => notes.push(format!("至多 {max_len} 项、键至多 {max_key_bytes} 字节")),
        _ => {}
    }
    notes.join("，")
}

#[path = "typegen/symbols.rs"]
mod symbols;
use symbols::Symbols;

#[cfg(test)]
#[path = "typegen/tests.rs"]
mod tests;
