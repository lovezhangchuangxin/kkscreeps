use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::manifest::ModManifest;
use crate::error::{ModHostError, ModHostResult};

/// 单包文件数、总字节与目录深度的装载上限：注册期把整个包读进内存
/// 并参与指纹，超限说明这不是一个可评审的玩法包。目录深度独立设限，
/// 防止纯目录链在文件限额生效前先耗尽栈。
const MAX_FILES: usize = 512;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

/// 已装载的 Mod 包：manifest、全部文件字节（相对路径键）与派生指纹。
/// 模块加载与指纹共享同一份字节，注册期不再触碰磁盘。
#[derive(Clone, Debug)]
pub struct ModPackage {
    pub manifest: ModManifest,
    pub root: PathBuf,
    pub files: BTreeMap<PathBuf, Vec<u8>>,
}

impl ModPackage {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn file(&self, relative: &Path) -> Option<&[u8]> {
        self.files.get(relative).map(|bytes| bytes.as_slice())
    }

    /// 入口模块的绝对路径（包根 + entry；根已规范化，entry 已限制为包内相对路径）。
    pub fn entry_path(&self) -> PathBuf {
        self.root.join(&self.manifest.entry)
    }

    /// 服务端代码摘要：除玩家模块外的全部包文件，按相对路径排序后
    /// 逐文件混入路径与字节。任何服务端文件变化都会改变摘要。
    pub fn code_digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for (path, bytes) in &self.files {
            if let Some(player) = &self.manifest.player
                && path == Path::new(player)
            {
                continue;
            }
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(bytes.len().to_le_bytes());
            hasher.update(bytes);
        }
        hasher.finalize().into()
    }

    /// 玩家 API 摘要：player 模块字节；未声明玩家面的包为零摘要。
    pub fn player_api_digest(&self) -> [u8; 32] {
        match &self.manifest.player {
            Some(player) => {
                let bytes = self
                    .file(Path::new(player))
                    .expect("装载已校验 player 模块存在");
                Sha256::digest(bytes).into()
            }
            None => [0; 32],
        }
    }

    /// 配置摘要：规范化 JSON（对象键排序）的摘要；未提供配置的包为零摘要。
    /// 数值字面量形态（0 与 0.0）不做归一——配置以宿主传入的值对象为准，
    /// 不存在同值异形的稳定来源。
    pub fn config_digest(config: Option<&Value>) -> [u8; 32] {
        match config {
            Some(value) => Sha256::digest(canonical_json(value).as_bytes()).into(),
            None => [0; 32],
        }
    }

    fn load(root: &Path) -> ModHostResult<Self> {
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        load_dir(root, root, 0, &mut files, &mut total)?;
        // manifest 从快照字节解析：解析结果与指纹/装载用的是同一份字节，
        // 不存在"解析后再被改写"的脱钩窗口。mod.json 保留在快照内——
        // 指纹按设计覆盖包的全部实际字节，manifest 改动必须改变身份。
        let manifest_bytes = files
            .get(Path::new("mod.json"))
            .cloned()
            .ok_or_else(|| ModHostError::new(root.display().to_string(), "包缺少 mod.json"))?;
        let text = String::from_utf8(manifest_bytes)
            .map_err(|_| ModHostError::new(root.display().to_string(), "mod.json 须为 UTF-8"))?;
        let manifest = ModManifest::parse(&text, &root.display().to_string())?;
        let mut declared = vec![PathBuf::from(&manifest.entry)];
        if let Some(player) = &manifest.player {
            declared.push(PathBuf::from(player));
        }
        for required in declared {
            if !files.contains_key(&required) {
                return Err(ModHostError::new(
                    manifest.id.clone(),
                    format!("manifest 声明的模块缺失：{}", required.display()),
                ));
            }
        }
        Ok(Self {
            manifest,
            root: root.to_path_buf(),
            files,
        })
    }
}

/// 符号链接与非常规文件（FIFO、设备）一律拒绝：装载的字节与指纹必须
/// 来自包自身的目录树，不允许通过链接引用包外内容、制造装载/摘要
/// 不一致，或让装载阻塞在无写端的管道上。
fn load_dir(
    root: &Path,
    dir: &Path,
    depth: usize,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    total: &mut u64,
) -> ModHostResult<()> {
    if depth > MAX_DEPTH {
        return Err(ModHostError::new(
            root.display().to_string(),
            format!("包目录嵌套超过 {MAX_DEPTH} 层"),
        ));
    }
    let entries = std::fs::read_dir(dir).map_err(|error| {
        ModHostError::new(root.display().to_string(), format!("读取目录失败：{error}"))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ModHostError::new(root.display().to_string(), format!("遍历目录失败：{error}"))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            ModHostError::new(
                root.display().to_string(),
                format!("读取文件类型失败：{error}"),
            )
        })?;
        let path = entry.path();
        if file_type.is_symlink() {
            return Err(ModHostError::new(
                root.display().to_string(),
                format!("包内不允许符号链接：{}", path.display()),
            ));
        }
        if file_type.is_dir() {
            load_dir(root, &path, depth + 1, files, total)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(ModHostError::new(
                root.display().to_string(),
                format!("包内不允许非常规文件：{}", path.display()),
            ));
        }
        let bytes = read_bounded(&path, root, MAX_TOTAL_BYTES - *total)?;
        *total += bytes.len() as u64;
        if files.len() + 1 > MAX_FILES {
            return Err(ModHostError::new(
                root.display().to_string(),
                format!("包体超过装载上限（{MAX_FILES} 文件 / 8 MiB）"),
            ));
        }
        let relative = path.strip_prefix(root).expect("遍历起点在包根内");
        files.insert(relative.to_path_buf(), bytes);
    }
    Ok(())
}

/// 限额内的整文件读取：先按 metadata 预检（避免对超限文件做巨大预分配），
/// 再用 `take` 硬顶读取长度（封住 stat 之后文件被改大的窗口）。
fn read_bounded(path: &Path, root: &Path, budget: u64) -> ModHostResult<Vec<u8>> {
    let declared = std::fs::metadata(path)
        .map_err(|error| {
            ModHostError::new(
                root.display().to_string(),
                format!("读取元数据失败：{error}"),
            )
        })?
        .len();
    if declared > budget {
        return Err(ModHostError::new(
            root.display().to_string(),
            format!("包体超过装载上限（{MAX_FILES} 文件 / 8 MiB）"),
        ));
    }
    let file = std::fs::File::open(path).map_err(|error| {
        ModHostError::new(root.display().to_string(), format!("打开文件失败：{error}"))
    })?;
    let mut bytes = Vec::with_capacity(declared as usize);
    // 预算 +1 字节的硬顶：读满即说明实际长度超预算，拒绝而不是截断。
    file.take(budget + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ModHostError::new(root.display().to_string(), format!("读取文件失败：{error}"))
        })?;
    if bytes.len() as u64 > budget {
        return Err(ModHostError::new(
            root.display().to_string(),
            format!("包体超过装载上限（{MAX_FILES} 文件 / 8 MiB）"),
        ));
    }
    Ok(bytes)
}

/// 依对象键排序的规范化 JSON 文本，与进程内 serde_json 的键序特性
/// （preserve_order）无关；数组保序。
pub(crate) fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("键为字符串"),
                        canonical_json(&map[key])
                    )
                })
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => {
            let body: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", body.join(","))
        }
        other => serde_json::to_string(other).expect("标量可序列化"),
    }
}

/// 扫描 Mod 根目录：每个一级子目录须是一个包（含 mod.json）。
/// 返回按包 id 排序的包表；id 重复、目录遍历失败与符号链接目录
/// 都是装载错误（不静默跳过任何目录）。根路径先规范化，包根与
/// 模块包含检查共享同一绝对形式。
pub fn discover_packages(root: &Path) -> ModHostResult<BTreeMap<String, ModPackage>> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        ModHostError::new(
            root.display().to_string(),
            format!("Mod 根目录不可用：{error}"),
        )
    })?;
    let entries = std::fs::read_dir(&root)
        .map_err(|error| {
            ModHostError::new(
                root.display().to_string(),
                format!("读取根目录失败：{error}"),
            )
        })?
        .map(|entry| {
            entry.map_err(|error| {
                ModHostError::new(
                    root.display().to_string(),
                    format!("遍历根目录失败：{error}"),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let file_type = entry.file_type().ok().map(|t| t.is_dir()).unwrap_or(false);
        let path = entry.path();
        // file_type 不跟随链接：符号链接目录显式报错，普通文件静默跳过
        //（mods 根允许放 README 等杂项）。
        if path.is_dir() && !file_type {
            return Err(ModHostError::new(
                root.display().to_string(),
                format!("包目录不得为符号链接：{}", path.display()),
            ));
        }
        if file_type {
            dirs.push(path);
        }
    }
    dirs.sort();
    let mut packages = BTreeMap::new();
    for dir in dirs {
        if !dir.join("mod.json").is_file() {
            return Err(ModHostError::new(
                dir.display().to_string(),
                "Mod 子目录缺少 mod.json",
            ));
        }
        let package = ModPackage::load(&dir)?;
        if let Some(previous) = packages.insert(package.id().to_string(), package.clone()) {
            return Err(ModHostError::new(
                root.display().to_string(),
                format!(
                    "重复的包 id {:?}：{} 与 {}",
                    package.id(),
                    previous.root.display(),
                    package.root.display()
                ),
            ));
        }
    }
    Ok(packages)
}
