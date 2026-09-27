//! `kkscreeps embed` 子命令：嵌入式 API 的 stdio 传输层（M0）。
//!
//! screeps-ok 的 TS 适配器（compat/）spawn 本命令，在 stdin/stdout 上以
//! "一行一条 JSON-RPC" 通信：请求 `{"id":n,"method":"...","params":{...}}`，
//! 响应 `{"id":n,"ok":true,"result":...}` 或 `{"id":n,"ok":false,"error":"..."}`。
//! EOF 即退出。方法集与 kk-engine::EmbedEngine（嵌入式 API）一一对应——
//! 该接口同时是 `kkscreeps sim` 的驱动接口（ADR-008 第 8 条）。

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use kk_engine::EmbedEngine;

pub fn run() -> i32 {
    let mut engine = EmbedEngine::new();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let resp = match handle_line(&mut engine, &line) {
            Ok(result) => json!({ "id": rpc_id(&line), "ok": true, "result": result }),
            Err(e) => json!({ "id": rpc_id(&line), "ok": false, "error": e.to_string() }),
        };
        if writeln!(out, "{resp}").and_then(|_| out.flush()).is_err() {
            break; // 父进程已退出
        }
    }
    engine.teardown();
    0
}

fn rpc_id(line: &str) -> Value {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|v| v.get("id").cloned())
        .unwrap_or(Value::Null)
}

fn handle_line(engine: &mut EmbedEngine, line: &str) -> anyhow::Result<Value> {
    let req: Value = serde_json::from_str(line)?;
    let method = req
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("缺少 method 字段"))?;
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    fn s<'a>(v: &'a Value, k: &str) -> anyhow::Result<&'a str> {
        v.get(k)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("缺少参数 {k}"))
    }

    let result = match method {
        "createShard" => {
            engine.create_shard(&params)?;
            json!({ "done": true })
        }
        "placeObject" => {
            let room = s(&params, "room")?;
            let kind = s(&params, "type")?;
            let spec = params.get("spec").cloned().unwrap_or_else(|| json!({}));
            let id = engine.place_object(room, kind, &spec)?;
            json!({ "id": id })
        }
        "setTerrain" => {
            let room = s(&params, "room")?;
            let terrain: Vec<u8> = params
                .get("terrain")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_i64)
                        .map(|x| x as u8)
                        .collect()
                })
                .ok_or_else(|| anyhow::anyhow!("缺少 terrain 数组"))?;
            engine.set_terrain(room, terrain)?;
            json!({ "done": true })
        }
        "runPlayer" => {
            let handle = s(&params, "handle")?;
            let code = s(&params, "code")?;
            let run = engine.run_player(handle, code)?;
            player_run_json(&run)
        }
        "runPlayers" => {
            let codes = params
                .get("codes")
                .and_then(Value::as_object)
                .ok_or_else(|| anyhow::anyhow!("缺少 codes 映射"))?;
            let map: std::collections::BTreeMap<String, String> = codes
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect();
            let runs = engine.run_players(&map)?;
            let mut out = serde_json::Map::new();
            for (handle, run) in runs {
                out.insert(handle, player_run_json(&run));
            }
            Value::Object(out)
        }
        "tick" => {
            let count = params.get("count").and_then(Value::as_u64).unwrap_or(1);
            engine.tick(count)?;
            json!({ "time": engine.get_game_time() })
        }
        "getObject" => {
            let id = s(&params, "id")?;
            engine.get_object(id).unwrap_or(Value::Null)
        }
        "findInRoom" => {
            let room = s(&params, "room")?;
            let find_const = params
                .get("type")
                .and_then(Value::as_i64)
                .ok_or_else(|| anyhow::anyhow!("缺少 type（FIND 常量）"))?;
            json!(engine.find_in_room(room, find_const))
        }
        "getGameTime" => json!(engine.get_game_time()),
        "captureConsoleLogs" => {
            let handle = s(&params, "handle")?;
            json!(engine.capture_console_logs(handle))
        }
        "getControllerPos" => {
            let room = s(&params, "room")?;
            match engine.get_controller_pos(room) {
                Some((x, y)) => json!({ "x": x, "y": y }),
                None => Value::Null,
            }
        }
        "teardown" => {
            // 只重置（世界+沙箱回收），进程保留复用；真正退出靠父进程关 stdin（EOF）
            engine.teardown();
            json!({ "done": true })
        }
        other => anyhow::bail!("未知方法: {other}"),
    };
    Ok(result)
}

fn player_run_json(run: &kk_engine::PlayerRun) -> Value {
    json!({
        "executed": run.executed,
        "expr": run.expr,
        "console": run.console,
    })
}
