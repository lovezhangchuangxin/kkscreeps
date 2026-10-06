//! 同一声明模块的符号分配；作者类、构造器、参数与 SDK 类型不能互相遮蔽。
use super::*;

pub(super) struct Symbols {
    pub objects: BTreeMap<String, String>,
    // 冻结门禁止对象与建筑共用类型键，构造器引用可共用这一映射。
    pub constructors: BTreeMap<String, String>,
    pub arguments: BTreeMap<String, String>,
}

impl Symbols {
    pub fn new(
        plan: &ContributionPlan,
        surface: &PlayerSurface,
        buildings: &crate::buildings::BuildingCatalog,
    ) -> Self {
        let mut used: BTreeSet<String> = [
            "ModInstallApi",
            "ModResourceName",
            "ModBuiltinClassName",
            "ModBuiltinClass",
            "Readonly",
            "Record",
            "ReadonlyArray",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut objects = BTreeMap::new();
        for ty in &surface.types {
            objects.insert(ty.key.to_string(), allocate(&ty.player_class, &mut used));
        }
        let mut constructors = BTreeMap::new();
        for ty in &surface.types {
            constructors.insert(
                ty.key.to_string(),
                allocate(&format!("{}Constructor", ty.player_class), &mut used),
            );
        }
        let mut sorted: Vec<_> = buildings.iter().collect();
        sorted.sort_by_key(|b| b.key.to_string());
        for building in sorted {
            constructors.insert(
                building.key.to_string(),
                allocate(&format!("{}Constructor", building.player_class), &mut used),
            );
        }
        let arguments = args_type_names(plan, &mut used);
        Self {
            objects,
            constructors,
            arguments,
        }
    }
}

fn allocate(base: &str, used: &mut BTreeSet<String>) -> String {
    // playerClass 是 JS 属性标识符，合法值也可能是 TS 保留类型名。
    let reserved = matches!(
        base,
        "any"
            | "unknown"
            | "never"
            | "number"
            | "string"
            | "boolean"
            | "symbol"
            | "object"
            | "bigint"
            | "void"
            | "null"
            | "undefined"
            | "true"
            | "false"
            | "this"
            | "class"
            | "interface"
            | "type"
            | "export"
            | "default"
            | "new"
            | "function"
            | "extends"
            | "import"
            | "const"
            | "let"
            | "var"
            | "return"
            | "delete"
            | "typeof"
            | "instanceof"
            | "in"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "break"
            | "continue"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "yield"
            | "await"
            | "enum"
            | "super"
            | "implements"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
    );
    let base = if reserved {
        format!("{base}Projection")
    } else {
        base.to_string()
    };
    let mut name = base.clone();
    let mut suffix = 1;
    while !used.insert(name.clone()) {
        suffix += 1;
        name = format!("{base}{suffix}");
    }
    name
}
