//! `mod types`：从冻结计划生成玩家面 TypeScript 声明，`--check` 校验新鲜度。
//!
//! 输出与报告都必须在 Mod 包根之外：包内全部文件字节计入计划指纹，
//! 任一产物放进包目录都会改变下一次的计划身份（生成头部有同样的
//! 提示）。生成内容对同一（注册输入、选择集）恒定；--check 与磁盘
//! **字节**对比（编辑器改行尾同样判过期），缺失、过期或被手改都以
//! 非零退出（区别于注册失败的 1），供作者与 CI 当门禁。
use crate::report::write_report;
use anyhow::Context;
use kk_mod_host::{ContentSelection, ModRealm, RegisterOptions, discover_packages};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Options {
    pub root: PathBuf,
    pub configs: Option<PathBuf>,
    pub select: Vec<String>,
    pub exclude: Vec<String>,
    pub out: PathBuf,
    pub check: bool,
    pub report: PathBuf,
}

pub fn run(options: Options) -> i32 {
    // 路径守卫先于一切：被拒绝的输出/报告路径不落任何文件（含失败
    // 报告——写进包目录同样改变计划指纹），与 mod inspect 同一手法。
    if let Err(error) = validated_out(&options.out, &options.root)
        .and_then(|()| validated_report(&options.report, &options.out, &options.root))
    {
        eprintln!("mod types: {error:#}");
        return 1;
    }
    let (code, summary) = match execute(&options) {
        Ok(pair) => pair,
        Err(error) => (
            1,
            json!({"status":"setup_failed","error":format!("{error:#}"),"out":options.out}),
        ),
    };
    if let Err(error) = write_report(&options.report, &summary) {
        eprintln!("mod types 报告写入失败: {error:#}");
        return 1;
    }
    let message = summary["message"]
        .as_str()
        .or(summary["error"].as_str())
        .unwrap_or("失败");
    if code == 0 {
        println!("mod types: {message}");
    } else {
        eprintln!("mod types: {message}");
    }
    code
}

/// 退出码语义：0 新鲜或已写入；1 注册/参数失败（报告 status=
/// setup_failed）；2 声明缺失、过期或被手改（status=stale）。
fn execute(options: &Options) -> anyhow::Result<(i32, Value)> {
    let declarations = generate(options)?;
    if !options.check {
        write_declarations(&options.out, &declarations)?;
        return Ok((
            0,
            json!({"status":"ok","message":format!("已生成 {}", options.out.display()),"out":options.out}),
        ));
    }
    // 只读对比，不创建目录也不重写文件；非 UTF-8 与缺失同样按"被
    // 手改/缺失"判 stale（退出 2），不与注册失败混淆。
    let existing = match std::fs::read(&options.out) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error).context("读取既有声明"),
    };
    if existing == declarations.as_bytes() {
        Ok((
            0,
            json!({"status":"ok","message":"声明新鲜","out":options.out}),
        ))
    } else {
        Ok((
            2,
            json!({
                "status":"stale",
                "message":format!(
                    "声明缺失、过期或被手改：{}；运行 mod types（不带 --check）重新生成",
                    options.out.display()
                ),
                "out":options.out,
            }),
        ))
    }
}

fn generate(options: &Options) -> anyhow::Result<String> {
    let root = std::fs::canonicalize(&options.root).context("读取 Mod 包根目录")?;
    let packages = discover_packages(&root)?;
    let configs = match &options.configs {
        Some(path) => serde_json::from_slice::<BTreeMap<String, Value>>(
            &std::fs::read(path).context("读取 Mod 配置")?,
        )
        .context("配置须为按包 ID 分组的 JSON 对象")?,
        None => BTreeMap::new(),
    };
    for id in configs.keys() {
        anyhow::ensure!(packages.contains_key(id), "Mod 配置指定了未加载的包 {id}");
    }
    let selection = ContentSelection {
        include: options.select.iter().cloned().collect(),
        exclude: options.exclude.iter().cloned().collect(),
    };
    let register_options = RegisterOptions {
        configs,
        selection,
        ..Default::default()
    };
    let mut realm = ModRealm::with_options(&register_options)?;
    let registered = realm.register(&packages, register_options)?;
    kk_mod_api::typegen::player_declarations(&registered.plan)
        .map_err(anyhow::Error::msg)
        .context("生成玩家面声明")
}

/// 把路径按"已存在取实体、否则父目录存在取父拼名、再否则词法绝对
/// 化"归一，全程不创建任何目录——守卫拒绝发生在落盘之前，包根内
/// 不留下半成品目录（空目录会让后续 discover_packages 把它当包读）。
/// 词法分支做 `.`/`..` 归一（std::path::absolute 不消 `..`，带 `..`
/// 的路径会绕过 starts_with 包内判定）；符号链接别名只有已存在的
/// 路径经 canonicalize 才能识破，与 mod inspect 守卫同一残余边界。
fn normalized(path: &Path) -> anyhow::Result<PathBuf> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if parent.exists() {
        let name = path.file_name().context("路径须指向文件")?;
        return Ok(std::fs::canonicalize(parent)?.join(name));
    }
    Ok(lexical_absolute(path))
}

/// 词法绝对化并消解 `.`/`..`（前导 `..` 保留在首个根组件之后不可再消）。
fn lexical_absolute(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// 输出路径守卫：`.d.ts` 双段后缀（Path::extension 只取最后一段）、
/// 不得位于包根内（包内文件计入计划指纹）。
fn validated_out(out: &Path, root: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        out.file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".d.ts")),
        "声明文件必须使用 .d.ts 后缀"
    );
    if let Ok(root) = std::fs::canonicalize(root) {
        anyhow::ensure!(
            !normalized(out)?.starts_with(&root),
            "声明文件须放在 Mod 包根目录之外（包内全部文件字节计入计划指纹）"
        );
    }
    Ok(())
}

/// 报告守卫：.json 后缀、包根之外，且不得与声明输出同一路径（同值
/// 会先写声明再被报告覆盖，产出"声明实为 JSON"的静默错误工件）。
fn validated_report(report: &Path, out: &Path, root: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        report.extension().is_some_and(|ext| ext == "json"),
        "报告须使用 .json 后缀"
    );
    if let Ok(root) = std::fs::canonicalize(root) {
        anyhow::ensure!(
            !normalized(report)?.starts_with(&root),
            "报告须放在 Mod 包根目录之外（包内全部文件字节计入计划指纹）"
        );
    }
    anyhow::ensure!(
        normalized(report)? != normalized(out)?,
        "报告不能覆盖声明输出"
    );
    Ok(())
}

fn write_declarations(out: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, text)?;
    Ok(())
}
