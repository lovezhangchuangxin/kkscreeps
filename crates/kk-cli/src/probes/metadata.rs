//! 各性能命令共用的环境证据，采样时读取当前进程与工作区。
use serde_json::{Value, json};

pub(crate) fn metadata() -> Value {
    let command = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "logical_cpus": std::thread::available_parallelism().ok().map(|n| n.get()),
        "system": command("uname", &["-a"]),
        "rustc_on_path": command("rustc", &["--version"]),
        "git_commit": command("git", &["rev-parse", "HEAD"]),
        "git_status": command("git", &["status", "--porcelain"]),
        "debug_assertions": cfg!(debug_assertions),
        "unix_time_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis()),
    })
}
