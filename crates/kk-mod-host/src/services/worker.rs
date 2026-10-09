//! 队列串行消费；生命周期严格配对，故障终止本域并拒绝剩余请求。
use super::realm::ServiceRealm;
use super::*;

pub(super) fn run(
    packages: BTreeMap<String, ModPackage>,
    options: ServiceOptions,
    mut receiver: mpsc::Receiver<Job>,
    ready: oneshot::Sender<ModHostResult<Vec<String>>>,
) {
    let initialized = (|| {
        let (selected, _) = crate::package::resolve_selection(&packages, &options.selection)?;
        let selected = selected.as_ref().unwrap_or(&packages);
        if let Some(id) = options
            .configs
            .keys()
            .find(|id| !selected.contains_key(*id))
        {
            return Err(error(format!("后台配置指向未选择的包 {id}")));
        }
        for name in options.probes.keys() {
            let key = kk_mod_api::Key::new(name).map_err(error)?;
            if !selected.contains_key(key.namespace()) {
                return Err(error("探针授予未选择的包"));
            }
        }
        let mut realm = ServiceRealm::new(options)?;
        let keys = realm.register(selected)?;
        Ok((realm, keys))
    })();
    let (mut realm, keys) = match initialized {
        Ok(result) => result,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(keys.clone())).is_err() {
        return;
    }
    let mut started = false;
    let mut last_view = Value::Null;
    while let Some(job) = receiver.blocking_recv() {
        let stop = matches!(job.command, Command::Stop);
        let result = match job.command {
            Command::Start if !started => {
                let result = lifecycle(&mut realm, &keys, "start", &job.view);
                if result.is_ok() {
                    started = true;
                }
                result
            }
            Command::Request(key, args) if started => {
                realm.invoke(&key, "request", args, job.view.clone())
            }
            Command::Stop if started => lifecycle(&mut realm, &keys, "stop", &job.view),
            Command::Stop => Ok(Value::Null),
            _ => Err(error("后台生命周期状态不允许此请求")),
        };
        last_view = job.view;
        let fault = result.as_ref().err().cloned();
        let _ = job.reply.send(result);
        if stop || fault.is_some() {
            receiver.close();
            while let Some(job) = receiver.blocking_recv() {
                let _ = job
                    .reply
                    .send(Err(fault.clone().unwrap_or_else(|| error("后台域已停止"))));
            }
            return;
        }
    }
    // 正常句柄丢弃也按有限预算 stop；未 start 不调用 stop，故障路径不再进入 VM。
    if started {
        let _ = lifecycle(&mut realm, &keys, "stop", &last_view);
    }
}

fn lifecycle(
    realm: &mut ServiceRealm,
    keys: &[String],
    phase: &str,
    view: &Value,
) -> ModHostResult<Value> {
    let mut results = serde_json::Map::new();
    let order: Vec<_> = if phase == "stop" {
        keys.iter().rev().collect()
    } else {
        keys.iter().collect()
    };
    for key in order {
        results.insert(
            key.clone(),
            realm.invoke(key, phase, Value::Null, view.clone())?,
        );
    }
    Ok(Value::Object(results))
}
