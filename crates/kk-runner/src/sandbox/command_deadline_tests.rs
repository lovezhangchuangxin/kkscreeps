use super::*;

#[test]
fn command_boundaries_clear_delivered_termination() -> anyhow::Result<()> {
    let mut sandbox = Sandbox::new(SandboxConfig::default())?;
    // 模拟上一代看门狗已经投递，但宿主调用成功返回的交错。
    sandbox.runtime.v8_isolate().terminate_execution();
    sandbox.set_command_deadline(Instant::now() + Duration::from_secs(1))?;
    sandbox.set_memory_state("{}", "[]", "null")?;
    // 解除期限必须同时清掉已投递标志，后续管理命令也应能复用实例。
    sandbox.runtime.v8_isolate().terminate_execution();
    sandbox.clear_command_deadline();
    sandbox.set_memory_state("{}", "[]", "null")?;
    Ok(())
}
