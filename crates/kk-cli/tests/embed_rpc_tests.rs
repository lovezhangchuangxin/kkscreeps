//! 用子进程管道验证逐行 JSON 协议：方法分派、请求错误、id 回传及 EOF 退出。

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
    // 发送正常调用、非法请求和非法代码，确认请求错误不推进世界。
    let requests = [
        r#"{"id":1,"method":"createShard","params":{"players":["p1"],"rooms":[{"name":"W1N1","rcl":1,"owner":"p1"}]}}"#
            .to_string(),
        r#"{"id":2,"method":"getGameTime","params":{}}"#.to_string(),
        r#"{"id":3,"method":"nopeMethod","params":{}}"#.to_string(),
        "{invalid json".to_string(),
        r#"{"id":5,"params":{}}"#.to_string(),
        r#"{"id":6,"method":"runPlayers","params":{"codes":{"p1":42}}}"#.to_string(),
        r#"{"id":7,"method":"runPlayers","params":{"codes":{"p1":null}}}"#.to_string(),
        r#"{"id":8,"method":"tick","params":{"count":4294967296}}"#.to_string(),
        r#"{"id":9,"method":"tick","params":{"count":0}}"#.to_string(),
        r#"{"id":10,"method":"tick","params":{"count":"5"}}"#.to_string(),
        r#"{"id":11,"method":"tick","params":{"count":-1}}"#.to_string(),
        r#"{"id":12,"method":"tick","params":{}}"#.to_string(),
        r#"{"id":13,"method":"getGameTime","params":{}}"#.to_string(),
    ];
    // 写入失败也先关闭 stdin 并等待子进程退出，避免直接返回留下未回收的进程。
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

    for id in [6, 7] {
        let resp: Value = serde_json::from_str(lines.next().expect("非法代码的响应"))?;
        assert_eq!(resp["id"], id);
        assert_eq!(resp["ok"], false, "非字符串代码必须被拒绝: {resp}");
        assert!(
            resp["error"]
                .as_str()
                .is_some_and(|e| e.contains("p1") && e.contains("字符串")),
            "错误应指出玩家及代码类型要求: {resp}"
        );
    }

    // tick 次数与 CLI u32 同口径：越上限与零都拒绝，进程继续服务。
    let resp: Value = serde_json::from_str(lines.next().expect("tick 超上限响应"))?;
    assert_eq!(resp["id"], 8);
    assert_eq!(resp["ok"], false);
    assert!(
        resp["error"].as_str().is_some_and(|e| e.contains("u32")),
        "超上限报错: {resp}"
    );
    let resp: Value = serde_json::from_str(lines.next().expect("tick 零次响应"))?;
    assert_eq!(resp["id"], 9);
    assert_eq!(resp["ok"], false);
    assert!(
        resp["error"]
            .as_str()
            .is_some_and(|e| e.contains("至少为 1")),
        "零次报错: {resp}"
    );
    // 键存在但非非负整数：点名拒绝，不静默按缺省处理。
    for id in [10, 11] {
        let resp: Value = serde_json::from_str(lines.next().expect("非法 count 的响应"))?;
        assert_eq!(resp["id"], id);
        assert_eq!(resp["ok"], false, "非整数 count 必须被拒绝: {resp}");
        assert!(
            resp["error"]
                .as_str()
                .is_some_and(|e| e.contains("非负整数")),
            "非法 count 报错: {resp}"
        );
    }
    let resp: Value = serde_json::from_str(lines.next().expect("缺省一次 tick 响应"))?;
    assert_eq!(resp["id"], 12);
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["result"]["time"], 1, "缺省 count=1 推进一 tick");
    let resp: Value = serde_json::from_str(lines.next().expect("tick 后时间响应"))?;
    assert_eq!(resp["id"], 13);
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["result"], 1, "被拒的 tick 不得推进世界");

    assert!(lines.next().is_none(), "无多余输出");
    Ok(())
}
