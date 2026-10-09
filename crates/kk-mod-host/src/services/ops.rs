//! 后台域唯一 I/O op：活动回调内、同包命名空间、显式授予的有限只读探针。
use super::*;
use deno_core::{OpState, extension, op2};
use deno_error::JsErrorBox;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) struct ProbeState {
    pub options: ServiceOptions,
    pub active: Option<(u32, String)>,
    pub registering: Option<String>,
    pub registered: Vec<String>,
    pub calls: usize,
    pub pending: usize,
    pub fault: Option<String>,
}

fn fault(state: &mut ProbeState, message: &str) -> JsErrorBox {
    state.fault.get_or_insert_with(|| message.to_owned());
    JsErrorBox::generic(message.to_owned())
}

struct Pending(Rc<RefCell<OpState>>);
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.borrow_mut().borrow_mut::<ProbeState>().pending -= 1;
    }
}

#[op2]
#[serde]
async fn op_service_probe(
    state: Rc<RefCell<OpState>>,
    token: u32,
    #[serde] name: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let (reader, limit) = {
        let mut cell = state.borrow_mut();
        let probes = cell.borrow_mut::<ProbeState>();
        let Some(name) = name.as_str() else {
            return Err(fault(probes, "后台探针名须为字符串"));
        };
        let Some((active, namespace)) = probes.active.as_ref() else {
            return Err(fault(probes, "后台能力不在活动回调内"));
        };
        if *active != token || !name.starts_with(&format!("{namespace}:")) {
            return Err(fault(probes, "后台能力已过期或探针跨包"));
        }
        let Some(reader) = probes.options.probes.get(name).cloned() else {
            return Err(fault(probes, "未授予此后台探针"));
        };
        if probes.calls >= probes.options.max_probes_per_call {
            return Err(fault(probes, "后台探针调用次数超限"));
        }
        probes.calls += 1;
        probes.pending += 1;
        (reader, probes.options.max_value_bytes)
    };
    let _pending = Pending(state.clone());
    let result = reader.read().await.map_err(JsErrorBox::generic)?;
    if check_value(&result, limit).is_err() {
        return Err(fault(
            state.borrow_mut().borrow_mut::<ProbeState>(),
            "后台探针输出超限",
        ));
    }
    Ok(result)
}

#[op2(fast)]
fn op_service_register(state: &mut OpState, #[string] key: String) -> Result<(), JsErrorBox> {
    let probes = state.borrow_mut::<ProbeState>();
    let Some(namespace) = probes.registering.as_ref() else {
        return Err(fault(probes, "后台注册窗口已关闭"));
    };
    let valid = kk_mod_api::Key::new(&key).is_ok_and(|key| key.namespace() == namespace);
    if !valid || probes.registered.contains(&key) || probes.registered.len() >= 1024 {
        return Err(fault(probes, "服务声明须属于当前包且唯一，总数不得超限"));
    }
    probes.registered.push(key);
    Ok(())
}
extension!(kk_services, ops = [op_service_probe, op_service_register]);
