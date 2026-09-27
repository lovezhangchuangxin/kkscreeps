//! `kkscreeps embed` 传输层的最小 JSON-RPC 往返测试（管道驱动）。
//!
//! 覆盖 embed_main.rs 的方法分派、rpc_id 回退（非法请求也带回原 id 的错误响应）
//! 与 EOF 退出语义——此前该层只靠 CI 上恒 skip 的 smoke 测试覆盖。

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

fn spawn_embed() -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
        .arg("embed")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kkscreeps embed")
}

#[test]
fn embed_rpc_roundtrip() -> anyhow::Result<()> {
    let mut child = spawn_embed();
    let mut stdin = child.stdin.take().expect("stdin");
    // 依次发送：正常调用、未知方法、非法 JSON、缺 method 字段
    let requests = [
        r#"{"id":1,"method":"createShard","params":{"players":["p1"],"rooms":[{"name":"W1N1","rcl":1,"owner":"p1"}]}}"#
            .to_string(),
        r#"{"id":2,"method":"getGameTime","params":{}}"#.to_string(),
        r#"{"id":3,"method":"nopeMethod","params":{}}"#.to_string(),
        "{invalid json".to_string(),
        r#"{"id":5,"params":{}}"#.to_string(),
    ];
    // 所有路径都保证 wait（不留 zombie）；写失败也要等子进程退出后再返回错误
    let mut write_err = None;
    for req in &requests {
        if let Err(e) = writeln!(stdin, "{req}") {
            write_err = Some(e);
            break;
        }
    }
    drop(stdin); // EOF → 子进程退出

    let out = child.wait_with_output()?;
    if let Some(e) = write_err {
        return Err(anyhow::anyhow!("写入请求失败: {e}"));
    }
    assert!(out.status.success(), "EOF 后正常退出: {:?}", out.status);
    let stdout = String::from_utf8(out.stdout)?;
    let mut lines = stdout.lines();
    let resp: Value = serde_json::from_str(lines.next().expect("第 1 条响应"))?;
    assert_eq!(resp["id"], 1, "rpc_id 透传");
    assert_eq!(resp["ok"], true, "createShard 成功: {resp}");

    let resp: Value = serde_json::from_str(lines.next().expect("第 2 条响应"))?;
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"], 0, "getGameTime 初始为 0");

    // 未知方法：错误响应仍带回原 id
    let resp: Value = serde_json::from_str(lines.next().expect("第 3 条响应"))?;
    assert_eq!(resp["id"], 3, "错误响应也带回 rpc_id");
    assert_eq!(resp["ok"], false);
    assert!(
        resp["error"]
            .as_str()
            .is_some_and(|e| e.contains("未知方法")),
        "未知方法报错: {resp}"
    );

    // 非法 JSON：id 回退为 null，仍返回错误响应（进程不崩）
    let resp: Value = serde_json::from_str(lines.next().expect("第 4 条响应"))?;
    assert_eq!(resp["id"], Value::Null, "非法 JSON 的 id 回退");
    assert_eq!(resp["ok"], false);

    // 缺 method：同样带回原 id 的错误响应
    let resp: Value = serde_json::from_str(lines.next().expect("第 5 条响应"))?;
    assert_eq!(resp["id"], 5);
    assert_eq!(resp["ok"], false);

    assert!(lines.next().is_none(), "无多余输出");
    Ok(())
}
