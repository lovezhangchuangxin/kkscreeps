//! 通过 stdin/stdout 提供 kk-runtime::EmbedEngine 的逐行 JSON 调用协议。
//!
//! 请求格式为 `{"id":n,"method":"...","params":{...}}`；响应为
//! `{"id":n,"ok":true,"result":...}` 或 `{"id":n,"ok":false,"error":"..."}`。
//! 请求串行处理，每条响应立即刷新 stdout。空行忽略；缺少 id 或 JSON 无法解析时，
//! 响应 id 为 null。方法调用出错后仍可处理后续请求。
//!
//! 关闭 stdin、读取失败或写出失败时回收玩家线程并退出。teardown 只重置引擎，
//! 不结束进程。此协议使用自定义 ok/error 字段，不采用标准 JSON-RPC 2.0 消息格式。

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use kk_runtime::EmbedEngine;

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
            break; // 输出管道不可用时停止服务，随后回收沙箱。
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
        "placeMarketOrder" => {
            let spec = params.get("spec").cloned().unwrap_or_else(|| json!({}));
            let id = engine.place_market_order(&spec)?;
            json!({ "id": id })
        }
        "setTerrain" => {
            let room = s(&params, "room")?;
            let terrain_value = params
                .get("terrain")
                .ok_or_else(|| anyhow::anyhow!("缺少 terrain 数组"))?;
            let terrain = kk_runtime::embed::terrain_codes_from_json(terrain_value)?;
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
                .map(|(handle, value)| {
                    value
                        .as_str()
                        .map(|code| (handle.clone(), code.to_string()))
                        .ok_or_else(|| anyhow::anyhow!("玩家 {handle} 的 code 必须是字符串"))
                })
                .collect::<anyhow::Result<_>>()?;
            let runs = engine.run_players(&map)?;
            let mut out = serde_json::Map::new();
            for (handle, run) in runs {
                out.insert(handle, player_run_json(&run));
            }
            Value::Object(out)
        }
        "tick" => {
            // 键缺失时缺省一次；键存在但非非负整数时点名拒绝，不静默
            // 回退——RPC 面的类型校验不弱于 CLI 面。
            let count = match params.get("count") {
                None => 1,
                Some(v) => v
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("count 必须是非负整数"))?,
            };
            // 与 CLI `--ticks` 的 u32 同口径：u64 级次数会让主循环长期
            // 不返回、期间无法处理后续请求。
            let count = u32::try_from(count)
                .map_err(|_| anyhow::anyhow!("tick 次数 {count} 超出 u32 上限"))?;
            if count == 0 {
                anyhow::bail!("tick 次数须至少为 1");
            }
            engine.tick(u64::from(count))?;
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
        "enableInputLog" => {
            // G25.a：挂载持久输入日志。path 必填；metadata 可选（诊断用，
            // 原样入头段）。重复挂载/有开放轮次由引擎侧拒绝。
            let path = s(&params, "path")?;
            let metadata = params.get("metadata").cloned().unwrap_or_else(|| json!({}));
            engine.enable_input_log(std::path::Path::new(path), metadata)?;
            json!({ "done": true })
        }
        "inputLogSegments" => json!(engine.input_log_segments()),
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
        "setInvaderRaidState" => {
            let room = s(&params, "room")?;
            let spec = params
                .get("spec")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("缺少参数 spec"))?;
            engine.set_invader_raid_state(room, &spec)?;
            json!({ "done": true })
        }
        "runInvaderRaidSpawner" => {
            let random = match params.get("random") {
                None => None,
                Some(v) => Some(
                    v.as_array()
                        .ok_or_else(|| anyhow::anyhow!("random 必须是数值数组"))?
                        .iter()
                        .map(|x| {
                            x.as_f64()
                                .ok_or_else(|| anyhow::anyhow!("random 元素必须是数字: {x}"))
                        })
                        .collect::<anyhow::Result<Vec<f64>>>()?,
                ),
            };
            engine.run_invader_raid_spawner(random)?;
            json!({ "done": true })
        }
        "clearInvaderRaidCreeps" => {
            let room = s(&params, "room")?;
            engine.clear_invader_raid_creeps(room)?;
            json!({ "done": true })
        }
        "teardown" => {
            // 只重置世界和沙箱，保留进程供下一次 createShard 使用。
            engine.teardown();
            json!({ "done": true })
        }
        other => anyhow::bail!("未知方法: {other}"),
    };
    Ok(result)
}

fn player_run_json(run: &kk_runtime::PlayerRun) -> Value {
    json!({
        "executed": run.executed,
        "expr": run.expr,
        "console": run.console,
    })
}
