//! 嵌入式 API：以库形式驱动引擎的接口（ADR-008 第 8 条）。
//!
//! 这组方法与 screeps-ok 适配器契约（`ScreepsOkAdapter`）同构，也和将来 `kkscreeps sim`
//! 驱动引擎的方式是**同一个接口**——适配器（compat/，经 `kkscreeps embed` sidecar 的
//! JSON-RPC 传输）与模拟器都调用本模块，一石二鸟。
//!
//! M0 实现 = 哑世界（kk-world::DumbWorld，只有对象存取与时间计数）+ 真实玩家沙箱
//! （kk-runner::PlayerThreads，deno_core）。没有游戏规则：`tick` 只推进时间计数，
//! `runPlayer` 在真实沙箱里求值玩家代码并恰好推进 1 tick。规则结算从 M1 起填入，
//! 本接口的形状保持不变。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use kk_runner::{EvalReport, PlayerThreads, SandboxConfig};
use kk_world::DumbWorld;

/// 官方语义常量（common/lib/constants.js；M1 由 kk-core 生成取代）。
mod consts {
    pub const CONTROLLER_HITS: i64 = 10_000;
}

/// runPlayer 的结局（TS 适配器据此映射 RunPlayerError 三分类或普通 Error）。
#[derive(Debug)]
pub struct PlayerRun {
    /// 代码是否真的执行过（false = 跳帧惩罚等基础设施原因，适配器须抛普通 Error）。
    pub executed: bool,
    /// runExpr 结果：`{ok, value?} | {ok:false, kind: syntax|runtime|serialization, error}`。
    pub expr: Value,
    /// 该玩家本次执行期间的 console 输出。
    pub console: Vec<String>,
}

pub struct EmbedEngine {
    players: Option<PlayerThreads>,
    world: DumbWorld,
    /// 玩家 handle → 线程 id（BTreeMap：固定遍历序，确定性的前提）。
    handles: BTreeMap<String, u64>,
    next_thread_id: u64,
    last_console: BTreeMap<String, Vec<String>>,
}

impl Default for EmbedEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbedEngine {
    pub fn new() -> Self {
        Self {
            players: None,
            world: DumbWorld::new(),
            handles: BTreeMap::new(),
            next_thread_id: 0,
            last_console: BTreeMap::new(),
        }
    }

    /// 建分片：完全重置世界与玩家沙箱（screeps-ok createShard 契约）。
    ///
    /// 默认布局：每个房间平原地形 + (1,1) controller（`controller: false` 除外）+
    /// 四角强制墙；owner/rcl 写进 controller 记录。
    pub fn create_shard(&mut self, spec: &Value) -> anyhow::Result<()> {
        // 逆序回收旧沙箱（LIFO），再重置世界
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.world.reset();
        self.handles.clear();
        self.last_console.clear();

        let players = self.players.get_or_insert_with(PlayerThreads::new);
        if let Some(handles) = spec.get("players").and_then(Value::as_array) {
            for handle in handles {
                let handle = handle.as_str().unwrap_or_default().to_string();
                if handle.is_empty() {
                    continue;
                }
                self.next_thread_id += 1;
                let thread_id = self.next_thread_id;
                players.spawn_player(thread_id, SandboxConfig::default())?;
                self.handles.insert(handle, thread_id);
            }
        }

        if let Some(rooms) = spec.get("rooms").and_then(Value::as_array) {
            for room in rooms {
                let name = room.get("name").and_then(Value::as_str).unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let terrain: Vec<u8> = room
                    .get("terrain")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_i64)
                            .map(|x| x as u8)
                            .collect()
                    })
                    .unwrap_or_else(default_terrain_with_corner_walls);
                self.world.register_room(name);
                self.world
                    .set_terrain(name, terrain)
                    .map_err(anyhow::Error::msg)?;
                let has_controller = room
                    .get("controller")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                if has_controller {
                    let owner = room.get("owner").and_then(Value::as_str);
                    let rcl = room.get("rcl").and_then(Value::as_i64).unwrap_or(1).max(0);
                    self.world.place(json!({
                        "type": "structure",
                        "structureType": "controller",
                        "room": name,
                        "x": 1,
                        "y": 1,
                        "hits": consts::CONTROLLER_HITS,
                        "hitsMax": consts::CONTROLLER_HITS,
                        "level": rcl,
                        "progress": 0,
                        "progressTotal": 0,
                        "owner": owner,
                    }));
                }
            }
        }
        Ok(())
    }

    /// 摆放对象（全部 placeXxx 的统一后端；spec 是 screeps-ok 的 PlaceSpec JSON）。
    /// `kind` 决定世界记录的 type 字段：creep/spawn/structure/constructionSite/flag/…。
    pub fn place_object(&mut self, room: &str, kind: &str, spec: &Value) -> anyhow::Result<String> {
        let pos = spec
            .get("pos")
            .and_then(Value::as_array)
            .unwrap_or(&vec![])
            .to_vec();
        let (x, y) = (
            pos.first().and_then(Value::as_i64).unwrap_or(0),
            pos.get(1).and_then(Value::as_i64).unwrap_or(0),
        );
        let mut record = json!({
            "type": kind,
            "room": room,
            "x": x,
            "y": y,
        });
        let obj = record.as_object_mut().unwrap();
        // 透传 spec 的其余字段（owner/body/store/name/hits/structureType/…）
        if let Some(spec_obj) = spec.as_object() {
            for (k, v) in spec_obj {
                if k == "pos" {
                    continue;
                }
                obj.insert(k.clone(), v.clone());
            }
        }
        // 结构类：structureType 从 spec.type / spec.structureType 归一
        if matches!(kind, "structure" | "spawn" | "constructionSite")
            && !obj.contains_key("structureType")
            && let Some(st) = spec.get("type").and_then(Value::as_str)
            && st != kind
        {
            obj.insert("structureType".into(), json!(st));
        }
        // creep 无名时生成一个（prelude 按 name 建索引）
        if kind == "creep" && !obj.contains_key("name") {
            let name = format!("creep_{}", self.world.objects.len());
            obj.insert("name".into(), json!(name));
        }
        Ok(self.world.place(record))
    }

    pub fn set_terrain(&mut self, room: &str, terrain: Vec<u8>) -> anyhow::Result<()> {
        self.world
            .set_terrain(room, terrain)
            .map_err(anyhow::Error::msg)
    }

    /// 执行一个玩家的代码并恰好推进 1 tick（screeps-ok runPlayer 契约）。
    pub fn run_player(&mut self, handle: &str, code: &str) -> anyhow::Result<PlayerRun> {
        let thread_id = *self
            .handles
            .get(handle)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 handle: {handle}"))?;
        let players = self
            .players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?;
        let world = self.world.world_json();
        let report: EvalReport = players.run_player(thread_id, &world, code)?;
        let executed = report
            .expr
            .get("kind")
            .and_then(Value::as_str)
            .map(|k| k != "terminated")
            .unwrap_or(true);
        let console = report
            .console
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>();
        self.last_console
            .insert(handle.to_string(), console.clone());
        // runPlayer 恰好 +1 tick
        self.world.time += 1;
        self.world.player_ticks += 1;
        Ok(PlayerRun {
            executed,
            expr: report.expr,
            console,
        })
    }

    /// 多玩家对同一状态求值后合计推进 1 tick（runPlayers 契约）。
    pub fn run_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, PlayerRun>> {
        let players = self
            .players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?;
        let world = self.world.world_json();
        let mut out = BTreeMap::new();
        // 固定顺序（handle 排序）求值——确定性
        for (handle, thread_id) in self.handles.clone() {
            let Some(code) = codes.get(&handle) else {
                continue;
            };
            let report = players.run_player(thread_id, &world, code)?;
            let executed = report
                .expr
                .get("kind")
                .and_then(Value::as_str)
                .map(|k| k != "terminated")
                .unwrap_or(true);
            let console = report
                .console
                .iter()
                .map(|l| l.text.clone())
                .collect::<Vec<_>>();
            self.last_console.insert(handle.clone(), console.clone());
            out.insert(
                handle,
                PlayerRun {
                    executed,
                    expr: report.expr,
                    console,
                },
            );
        }
        self.world.time += 1;
        self.world.player_ticks += 1;
        Ok(out)
    }

    /// 推进 n tick（M0：只推进时间计数，无规则结算）。
    pub fn tick(&mut self, count: u64) -> anyhow::Result<()> {
        self.world.time += count;
        self.world.player_ticks += count;
        Ok(())
    }

    /// 对象快照（字段名=玩家 getter 名的尽力而为投影；M0 无规则，深度语义从 M1 起对齐）。
    pub fn get_object(&self, id: &str) -> Option<Value> {
        let o = self.world.get(id)?;
        Some(snapshot_of(o))
    }

    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        self.world
            .find_in_room(room, find_const)
            .iter()
            .map(|o| snapshot_of(o))
            .collect()
    }

    pub fn get_game_time(&self) -> u64 {
        self.world.time
    }

    pub fn capture_console_logs(&self, handle: &str) -> Vec<String> {
        self.last_console.get(handle).cloned().unwrap_or_default()
    }

    pub fn get_controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        self.world.controller_pos(room)
    }

    pub fn teardown(&mut self) {
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.world.reset();
        self.handles.clear();
        self.last_console.clear();
    }
}

impl Drop for EmbedEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// 世界记录 → 快照 JSON（id/pos/owner 字段名对齐玩家 getter）。
fn snapshot_of(o: &Value) -> Value {
    let mut v = o.clone();
    if let Some(obj) = v.as_object_mut() {
        let x = obj.get("x").cloned().unwrap_or(json!(0));
        let y = obj.get("y").cloned().unwrap_or(json!(0));
        let room = obj.get("room").cloned().unwrap_or(json!(""));
        obj.insert("pos".into(), json!({ "x": x, "y": y, "roomName": room }));
    }
    v
}

fn default_terrain_with_corner_walls() -> Vec<u8> {
    let mut t = vec![0u8; 2500]; // 0 = plain
    let corner = |x: usize, y: usize| y * 50 + x;
    t[corner(0, 0)] = 1; // 1 = wall
    t[corner(49, 0)] = 1;
    t[corner(0, 49)] = 1;
    t[corner(49, 49)] = 1;
    t
}
