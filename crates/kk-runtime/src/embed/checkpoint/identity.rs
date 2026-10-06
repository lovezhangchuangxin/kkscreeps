//! 包/配置身份与玩家工作者注册数据；仅保存数据，VM 安装清单由原计划再生。
use super::*;
use kk_runner::{ModuleKind, ModuleTable, SandboxConfig};
use sha2::{Digest, Sha256};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModIdentity {
    pub packages: BTreeMap<String, Value>,
    options: Value,
}

impl ModIdentity {
    pub fn capture(
        packages: &BTreeMap<String, ModPackage>,
        options: &RegisterOptions,
    ) -> anyhow::Result<Self> {
        let packages = packages.iter().map(|(id, package)| {
            Ok((id.clone(), json!({
                "manifest": package.manifest,
                "files_sha256": format!("{:x}", Sha256::digest(serde_json::to_vec(&package.files)?)),
            })))
        }).collect::<anyhow::Result<_>>()?;
        let limits = &options.vm_limits;
        Ok(Self {
            packages,
            options: json!({
                "configs": options.configs, "freeze": options.freeze, "selection": options.selection,
                "fault_disable_threshold": options.fault_disable_threshold,
            "profiling": options.profiler.is_some(),
                "vm_limits": {"memory_bytes": limits.memory_bytes,
                    "registration_cpu": limits.registration.cpu, "registration_wall": limits.registration.wall,
                    "settlement_cpu": limits.settlement.cpu, "settlement_wall": limits.settlement.wall},
            }),
        })
    }
    pub fn verify(
        &self,
        packages: &BTreeMap<String, ModPackage>,
        options: &RegisterOptions,
    ) -> anyhow::Result<()> {
        let current = Self::capture(packages, options)?;
        anyhow::ensure!(
            self.packages == current.packages,
            "检查点 Mod 包清单/版本/源码身份不匹配（包括未选择的发现包）"
        );
        anyhow::ensure!(
            self.options == current.options,
            "检查点 Mod 配置/选择/规则/预算/停用策略身份不匹配"
        );
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Worker {
    heap_limit_bytes: usize,
    cpu_limit: f64,
    bucket_cap: f64,
    max_per_tick: f64,
    initial_bucket: f64,
    constant_overrides: BTreeMap<String, Value>,
    install_wall_limit: Option<Duration>,
    modules: BTreeMap<String, Module>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Module {
    js: Option<String>,
    binary: Option<Vec<u8>>,
}

impl Worker {
    pub fn capture(config: &SandboxConfig, table: &ModuleTable) -> Self {
        Self {
            heap_limit_bytes: config.heap_limit_bytes,
            cpu_limit: config.cpu_limit,
            bucket_cap: config.bucket_cap,
            max_per_tick: config.max_per_tick,
            initial_bucket: config.initial_bucket,
            constant_overrides: config.constant_overrides.clone(),
            install_wall_limit: config.install_wall_limit,
            modules: table
                .names()
                .map(|name| {
                    let module = match table.kind(name).expect("模块目录中的键存在") {
                        ModuleKind::Js => Module {
                            js: Some(table.load_js(name).unwrap().into()),
                            binary: None,
                        },
                        ModuleKind::Binary => Module {
                            js: None,
                            binary: Some(table.load_binary(name).unwrap().into()),
                        },
                    };
                    (name.into(), module)
                })
                .collect(),
        }
    }
    pub fn config(&self) -> anyhow::Result<SandboxConfig> {
        anyhow::ensure!(
            self.heap_limit_bytes >= 16 * 1024 * 1024
                && self.heap_limit_bytes as u64 <= 4 * 1024 * 1024 * 1024,
            "检查点玩家堆预算非法"
        );
        kk_core::cpu::Bucket::new(
            self.cpu_limit,
            self.bucket_cap,
            self.max_per_tick,
            self.initial_bucket,
        )
        .validate_checkpoint()
        .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            self.modules
                .values()
                .all(|module| module.js.is_some() != module.binary.is_some()),
            "检查点玩家模块必须恰有一种编码"
        );
        anyhow::ensure!(self.install_wall_limit.is_none_or(|duration| !duration.is_zero() && duration <= Duration::from_secs(3600)), "检查点玩家安装预算非法");
        Ok(SandboxConfig {
            heap_limit_bytes: self.heap_limit_bytes,
            cpu_limit: self.cpu_limit,
            bucket_cap: self.bucket_cap,
            max_per_tick: self.max_per_tick,
            initial_bucket: self.initial_bucket,
            constant_overrides: self.constant_overrides.clone(),
            install_wall_limit: self.install_wall_limit,
            ..Default::default()
        })
    }
    pub fn modules(&self) -> ModuleTable {
        let mut table = ModuleTable::new();
        for (name, module) in &self.modules {
            if let Some(source) = &module.js {
                table.insert_js(name, source);
            }
            if let Some(bytes) = &module.binary {
                table.insert_binary(name, bytes.clone());
            }
        }
        table
    }
}
