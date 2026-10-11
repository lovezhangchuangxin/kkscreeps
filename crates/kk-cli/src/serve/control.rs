//! 控制入口：SIGTERM/SIGINT 信号标志与可选的逐行 JSON stdio 通道。
//! 两者都只表达“操作者意图”，不直接触碰宿主——宿主在拍间轮询消费。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

/// 进程级停止请求（信号处理程序与驱动共用）。
pub(crate) static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn on_stop_signal(_signum: i32) {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

/// 注册 SIGTERM/SIGINT：只置原子标志（异步信号安全），宿主在拍间响应。
pub(crate) fn install_stop_signals() {
    // 仅 macOS/Linux（本特性的声明平台）；其他平台由驱动方以其他方式停止。
    #[cfg(unix)]
    unsafe {
        libc::signal(
            libc::SIGTERM,
            on_stop_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            on_stop_signal as *const () as libc::sighandler_t,
        );
    }
}

pub(crate) fn stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

/// stdio 控制命令。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ControlCommand {
    Status,
    Retry,
    Stop,
}

/// 解析一行控制输入：`{"command":"status|retry|stop"}`；非法输入返回
/// 错误消息（调用方回写后继续，不中断服务）。
pub(crate) fn parse_command(line: &str) -> Result<ControlCommand, String> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|error| format!("控制行不是合法 JSON: {error}"))?;
    let command = value["command"]
        .as_str()
        .ok_or_else(|| "控制行缺少 command 字符串字段".to_string())?;
    match command {
        "status" => Ok(ControlCommand::Status),
        "retry" => Ok(ControlCommand::Retry),
        "stop" => Ok(ControlCommand::Stop),
        other => Err(format!("未知控制命令 {other:?}（可用：status/retry/stop）")),
    }
}

/// 拍间等待的结果：到点推进、收到控制命令、或收到停止请求。
pub(crate) enum WaitOutcome {
    Deadline,
    Command(String),
    Stop,
}

/// 等到 `deadline`（控制/停止优先唤醒；25ms 轮询粒度兼顾响应与开销）。
pub(crate) fn wait_until(deadline: std::time::Instant, control: &Receiver<String>) -> WaitOutcome {
    loop {
        if stop_requested() {
            return WaitOutcome::Stop;
        }
        if let Ok(line) = control.try_recv() {
            return WaitOutcome::Command(line);
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return WaitOutcome::Deadline;
        }
        let remaining = deadline - now;
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(25)));
    }
}

/// 暂停/持久化故障下的等待：无 tick 截止点，只等控制或停止。
pub(crate) fn wait_for_control(control: &Receiver<String>) -> WaitOutcome {
    wait_until(std::time::Instant::now(), control)
}

/// 等待窗内的短休（暂停态轮询粒度）。
pub(crate) fn nap() {
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_known_commands() {
        assert_eq!(
            parse_command(r#"{"command":"status"}"#).unwrap(),
            ControlCommand::Status
        );
        assert_eq!(
            parse_command(r#"{"command":"retry"}"#).unwrap(),
            ControlCommand::Retry
        );
        assert_eq!(
            parse_command(r#"{"command":"stop"}"#).unwrap(),
            ControlCommand::Stop
        );
        assert!(parse_command("status").is_err());
        assert!(parse_command(r#"{"command":"explode"}"#).is_err());
        assert!(parse_command(r#"{"other":1}"#).is_err());
    }
}
