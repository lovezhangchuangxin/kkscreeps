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
    assert_eq!(resp["result"], 1, "getGameTime 创建即热身拍（官方同形）");

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
    assert_eq!(resp["result"]["time"], 2, "缺省 count=1 推进一 tick");
    let resp: Value = serde_json::from_str(lines.next().expect("tick 后时间响应"))?;
    assert_eq!(resp["id"], 13);
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["result"], 2, "被拒的 tick 不得推进世界");

    assert!(lines.next().is_none(), "无多余输出");
    Ok(())
}

/// G25.a RPC 面：enableInputLog 挂载、逐轮记段、teardown 摘除与
/// 同路径续接（先整读校验）。
#[test]
fn embed_rpc_input_log_lifecycle() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("kkscreeps-embed-log-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let log = dir.join("input.jsonl");

    let mut child = spawn_embed();
    let mut stdin = child.stdin.take().expect("stdin");
    let requests = [
        r#"{"id":1,"method":"createShard","params":{"players":["p1"],"rooms":[{"name":"W1N1","rcl":1,"owner":"p1"}]}}"#,
        r#"{"id":2,"method":"enableInputLog","params":{}}"#,
        &format!(
            r#"{{"id":3,"method":"enableInputLog","params":{{"path":"{}"}}}}"#,
            log.display().to_string().replace('\\', "\\\\")
        ),
        // 重复挂载拒绝：一个分片运行期只挂一份。
        &format!(
            r#"{{"id":4,"method":"enableInputLog","params":{{"path":"{}/second.jsonl"}}}}"#,
            dir.display()
        ),
        r#"{"id":5,"method":"inputLogSegments","params":{}}"#,
        r#"{"id":6,"method":"tick","params":{"count":2}}"#,
        r#"{"id":7,"method":"inputLogSegments","params":{}}"#,
        r#"{"id":8,"method":"teardown","params":{}}"#,
        r#"{"id":9,"method":"inputLogSegments","params":{}}"#,
        // teardown 摘除后同路径续接：先整读校验，段数按整个文件计。
        &format!(
            r#"{{"id":10,"method":"enableInputLog","params":{{"path":"{}"}}}}"#,
            log.display().to_string().replace('\\', "\\\\")
        ),
        r#"{"id":11,"method":"inputLogSegments","params":{}}"#,
    ]
    .map(str::to_string);
    // 写入失败也先等待子进程退出，避免留下未回收的进程。
    let mut write_err = None;
    for req in &requests {
        if let Err(e) = writeln!(stdin, "{req}") {
            write_err = Some(e);
            break;
        }
    }
    drop(stdin);

    let out = child.wait_with_output()?;
    if let Some(e) = write_err {
        return Err(anyhow::anyhow!("写入请求失败: {e}"));
    }
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout)?;
    let mut lines = stdout.lines();
    let mut next = |label: &str| -> anyhow::Result<Value> {
        Ok(serde_json::from_str(lines.next().expect(label))?)
    };

    assert_eq!(next("createShard")?["ok"], true);
    let resp = next("缺 path 拒绝")?;
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["ok"], false, "缺少 path 参数必须被拒绝");
    assert_eq!(next("挂载")?["ok"], true);
    let resp = next("重复挂载拒绝")?;
    assert_eq!(resp["id"], 4);
    assert_eq!(resp["ok"], false);
    assert_eq!(next("初始段数")?["result"], 0);
    assert_eq!(next("tick")?["ok"], true);
    assert_eq!(next("两段后")?["result"], 2, "每个推进轮一段");
    assert_eq!(next("teardown")?["ok"], true);
    assert_eq!(
        next("摘除后段数")?["result"],
        Value::Null,
        "teardown 摘除日志"
    );
    assert_eq!(next("续接")?["ok"], true, "干净日志可续接");
    assert_eq!(next("续接后段数")?["result"], 2, "既有段计入");
    assert!(lines.next().is_none(), "无多余输出");

    // RPC 面写入的日志与库面同一读回校验器兼容。
    let data = kk_runtime::embed::read_input_log(&log)?;
    assert_eq!(data.segments.len(), 2);
    assert!(data.segments.iter().all(|s| s["source"] == "advance"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
