//! 作者后台服务验收：真实包、已提交世界副本、有限常规文件探针与请求报告。
use crate::support::{paths, write_report};
use anyhow::Context;
use kk_mod_host::services::{BackgroundServices, FileProbe, SERVICE_PROTOCOL, ServiceOptions};
use kk_runtime::EmbedEngine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

pub struct Options {
    pub root: PathBuf,
    pub configs: Option<PathBuf>,
    pub select: Vec<String>,
    pub exclude: Vec<String>,
    pub checkpoint: Option<PathBuf>,
    pub requests: PathBuf,
    pub probes: Vec<String>,
    pub report: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    service: String,
    args: Value,
}

fn probe_paths(options: &Options) -> anyhow::Result<Vec<(String, PathBuf)>> {
    options
        .probes
        .iter()
        .map(|value| {
            let (key, path) = value
                .split_once('=')
                .context("probe 须为 namespace:name=path")?;
            kk_mod_api::Key::new(key).map_err(anyhow::Error::msg)?;
            anyhow::ensure!(!path.is_empty(), "probe 路径不能为空");
            Ok((key.to_owned(), PathBuf::from(path)))
        })
        .collect()
}
fn output_path(options: &Options) -> anyhow::Result<PathBuf> {
    paths::json_report(&options.report, &options.root, options.configs.as_deref())?;
    paths::distinct(&options.report, &options.requests)?;
    if let Some(checkpoint) = &options.checkpoint {
        paths::distinct(&options.report, checkpoint)?;
    }
    for (_, path) in probe_paths(options)? {
        paths::distinct(&options.report, &path)?;
    }
    paths::normalized(&options.report)
}

pub fn run(options: Options) -> i32 {
    let output = match output_path(&options) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("mod services: {error:#}");
            return 1;
        }
    };
    let (code, report) = match execute(&options) {
        Ok(report) => (0, report),
        Err(error) => (
            1,
            json!({"report_version":1,"status":"failed","error":format!("{error:#}")}),
        ),
    };
    if let Err(error) = write_report(&output, &report) {
        eprintln!("mod services 报告写入失败: {error:#}");
        return 1;
    }
    if code != 0 {
        eprintln!("mod services: {}", report["error"]);
    }
    code
}
fn execute(options: &Options) -> anyhow::Result<Value> {
    let input = super::registration::prepare(
        &options.root,
        options.configs.as_deref(),
        &options.select,
        &options.exclude,
    )?;
    let bytes = std::fs::read(&options.requests).context("读取后台请求")?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "后台请求文件超过 1 MiB");
    let requests: Vec<Request> =
        serde_json::from_slice(&bytes).context("后台请求须为 service/args 数组")?;
    anyhow::ensure!(requests.len() <= 1024, "后台请求数超过 1024");
    // 在调用线程构造/恢复 gameplay VM；后台 async block 只消费脱离世界的副本。
    let mut engine = EmbedEngine::new();
    if let Some(path) = &options.checkpoint {
        engine.load_checkpoint(path, Some((&input.packages, &input.options)))?;
    } else {
        engine.create_shard_with_mods(
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":1}]}),
            &input.packages,
            input.options.clone(),
        )?;
    }
    let before = engine.world_snapshot();
    let mut service_options = ServiceOptions {
        selection: input.options.selection.clone(),
        configs: input.options.configs.clone(),
        ..Default::default()
    };
    for (key, path) in probe_paths(options)? {
        anyhow::ensure!(
            service_options
                .probes
                .insert(key, Arc::new(FileProbe::open(&path, 1024 * 1024)?))
                .is_none(),
            "探针键重复"
        );
    }
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let report=executor.block_on(async {
        let host=BackgroundServices::spawn(input.packages,service_options).await?;
        let services=host.services().to_vec();
        let start=host.start(before.clone())?.resolve().await?;
        let mut results=vec![];
        for request in requests {
            let result=host.request(&request.service,request.args,before.clone())?.resolve().await?;
            results.push(json!({"service":request.service,"result":result}));
        }
        let stop=host.shutdown(before.clone()).await?;
        Ok::<_,kk_mod_host::ModHostError>(json!({"report_version":1,"status":"passed","protocol":SERVICE_PROTOCOL,"services":services,"start":start,"requests":results,"stop":stop,"world_unchanged":true}))
    })?;
    anyhow::ensure!(
        engine.world_snapshot() == before,
        "后台读取改变了已提交世界"
    );
    Ok(report)
}
