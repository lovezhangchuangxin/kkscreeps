//! 嵌入式 API：以库形式驱动引擎的接口（ADR-008 第 8 条）。
//!
//! 这组方法与 screeps-ok 适配器契约（`ScreepsOkAdapter`）同构，也和将来 `kkscreeps sim`
//! 驱动引擎的方式是**同一个接口**——适配器（compat/，经 `kkscreeps embed` sidecar 的
//! JSON-RPC 传输）与模拟器都调用本模块，一石二鸟。
//!
//! M1 起内部世界是 typed world（`kk_world::World`），`tick`/`runPlayer` 的推进走
//! 真实结算（`settle_room`，intents 恒空——玩家 intent 收集通道是 M2-2A）。世界
//! 投影 JSON 保持 M0 契约形状（`world_json`）。

use std::collections::BTreeMap;

use kk_core::Terrain;
use kk_runner::{EvalReport, PlayerThreads, SandboxConfig};
use kk_world::place::{PlaceCtx, normalize};
use serde_json::Value;

use crate::settle::Engine;

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
    engine: Engine,
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

/// `controller: false` 与显式控制器字段互斥（screeps-ok 契约，adapter.ts:45-48）。
const CONTROLLER_ONLY_FIELDS: [&str; 6] = [
    "rcl",
    "owner",
    "safeMode",
    "safeModeAvailable",
    "ticksToDowngrade",
    "powerEnabled",
];

/// JSON 数组 → 地形字节（元素必须是 0..=255 内的整数，不静默丢弃；
/// 位域合法性由 [`Terrain::from_codes`] 校验）。
pub fn terrain_codes_from_json(v: &Value) -> anyhow::Result<Vec<u8>> {
    let array = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("terrain 必须是数值数组"))?;
    array
        .iter()
        .map(|x| {
            let n = x
                .as_i64()
                .filter(|n| *n >= 0 && *n <= u8::MAX as i64)
                .ok_or_else(|| anyhow::anyhow!("地形元素非法: {x}"))?;
            Ok(n as u8)
        })
        .collect()
}

impl EmbedEngine {
    pub fn new() -> Self {
        Self {
            players: None,
            engine: Engine::default(),
            handles: BTreeMap::new(),
            next_thread_id: 0,
            last_console: BTreeMap::new(),
        }
    }

    /// 建分片：完全重置世界与玩家沙箱（screeps-ok createShard 契约）。
    ///
    /// 默认布局：每个房间平原地形 + (1,1) controller（`controller: false` 除外）；
    /// owner/rcl 写进 controller 记录。
    pub fn create_shard(&mut self, spec: &Value) -> anyhow::Result<()> {
        let rooms: Vec<&Value> = spec
            .get("rooms")
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default();

        // 校验先行（契约要求报错且不改状态）：互斥字段 + 地形合法性全部通过，
        // 之后才回收沙箱、重置世界
        for room in &rooms {
            let name = room.get("name").and_then(Value::as_str).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            if room.get("controller").and_then(Value::as_bool) == Some(false) {
                let set: Vec<&str> = CONTROLLER_ONLY_FIELDS
                    .iter()
                    .copied()
                    .filter(|k| room.get(*k).is_some())
                    .collect();
                if !set.is_empty() {
                    anyhow::bail!(
                        "RoomSpec {name}: controller: false takes no {}",
                        set.join(", ")
                    );
                }
            }
            if let Some(t) = room.get("terrain") {
                Terrain::from_codes(&terrain_codes_from_json(t)?).map_err(anyhow::Error::msg)?;
            }
            // rcl 上界与非整数显式拒绝（typed level 是 u8，截断会得错值）
            if let Some(rcl) = room.get("rcl").and_then(Value::as_f64)
                && !(rcl.fract() == 0.0 && (0.0..=8.0).contains(&rcl))
            {
                anyhow::bail!("RoomSpec {name}: rcl 非法（需 0..=8 整数）: {rcl}");
            }
        }

        // 逆序回收旧沙箱（LIFO），再重置世界
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.handles.clear();
        self.last_console.clear();

        let players = self.players.get_or_insert_with(PlayerThreads::new);
        if let Some(handles) = spec.get("players").and_then(Value::as_array) {
            for entry in handles {
                // screeps-ok 契约：players 为 (string | PlayerSpec)[]，PlayerSpec={name,gcl?,power?}
                // （adapter.ts:16-19；套件 setup 实际使用对象形式）。取 name 注册 handle；
                // gcl/power 影响玩家可见的 Game.gcl/gpl 状态，M1 规则实装时接上（哑世界
                // 阶段写入无处生效，静默吞掉注册则会让 runPlayer 直接找不到玩家）。
                let handle = match entry {
                    Value::String(s) => s.clone(),
                    Value::Object(o) => o
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    _ => continue,
                };
                if handle.is_empty() {
                    continue;
                }
                self.next_thread_id += 1;
                let thread_id = self.next_thread_id;
                players.spawn_player(thread_id, SandboxConfig::default())?;
                self.handles.insert(handle, thread_id);
            }
        }

        for room in &rooms {
            let name = room.get("name").and_then(Value::as_str).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            // 默认平原；四角墙由 normalize 统一强制（官方地图生成器不变量）
            let mut terrain = match room.get("terrain") {
                Some(t) => {
                    Terrain::from_codes(&terrain_codes_from_json(t)?).map_err(anyhow::Error::msg)?
                }
                None => Terrain::plains(),
            };
            terrain.normalize();
            self.engine.world.register_room(name, terrain);
            let has_controller = room
                .get("controller")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if has_controller {
                // vanilla 双分支种子（index.ts:904/968）：未占房 rcl 缺省 0、
                // 占房缺省 1；占房另种 downgrade/safeMode 族——相对值换绝对
                // tick（+time+1，warm-up tick 消耗一拍，index.ts:966-993）
                let owner = room.get("owner").and_then(Value::as_str);
                let rcl = room
                    .get("rcl")
                    .and_then(Value::as_i64)
                    .unwrap_or(if owner.is_some() { 1 } else { 0 })
                    .clamp(0, 8);
                let time = self.engine.world.time;
                let mut spec = serde_json::json!({
                    "pos": [1, 1],
                    "structureType": "controller",
                    "level": rcl,
                    "isPowerEnabled": room
                        .get("powerEnabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
                if let Some(owner) = owner {
                    spec["owner"] = serde_json::json!(owner);
                    let downgrade_table = kk_core::constants::CONTROLLER_DOWNGRADE;
                    let default_clock = downgrade_table
                        .iter()
                        .find(|(k, _)| *k == rcl.to_string())
                        .map(|(_, v)| *v)
                        .unwrap_or(0);
                    let downgrade_time = match room.get("ticksToDowngrade").and_then(Value::as_i64)
                    {
                        Some(t) => Some(time.saturating_add(t.max(0) as u64).saturating_add(1)),
                        None if rcl > 0 => {
                            Some(time.saturating_add(default_clock as u64).saturating_add(1))
                        }
                        None => None,
                    };
                    if let Some(dt) = downgrade_time {
                        spec["downgradeTime"] = serde_json::json!(dt);
                    }
                    if let Some(sm) = room
                        .get("safeMode")
                        .and_then(Value::as_i64)
                        .filter(|v| *v > 0)
                    {
                        spec["safeMode"] =
                            serde_json::json!(time.saturating_add(sm as u64).saturating_add(1));
                    }
                    if let Some(avail) = room.get("safeModeAvailable").and_then(Value::as_i64) {
                        spec["safeModeAvailable"] = serde_json::json!(avail.max(0));
                    }
                }
                let ctx = PlaceCtx {
                    time,
                    controller: None,
                    name_seq: self.engine.world.next_obj_id() as usize,
                };
                let controller = normalize("structure", &spec, &ctx).map_err(anyhow::Error::msg)?;
                self.engine
                    .world
                    .insert(name, controller)
                    .map_err(anyhow::Error::msg)?;
            }
        }
        Ok(())
    }

    /// 摆放对象（全部 placeXxx 的统一后端；spec 是 screeps-ok 的 PlaceSpec JSON）。
    /// `kind` 为适配器通道类别（creep/structure/constructionSite/flag/resource/…），
    /// 归一化由 [`kk_world::place::normalize`] 完成（初态表 + spec 私有字段折算）。
    pub fn place_object(&mut self, room: &str, kind: &str, spec: &Value) -> anyhow::Result<String> {
        // 无名稳定名序号用 id 分配器（单调不减）——对象会随结算消亡，
        // 可回退的计数（如对象总数）会让 mid-run 摆放重名
        let ctx = PlaceCtx {
            time: self.engine.world.time,
            controller: self.engine.world.room(room).and_then(|r| r.controller()),
            name_seq: self.engine.world.next_obj_id() as usize,
        };
        let object = normalize(kind, spec, &ctx).map_err(anyhow::Error::msg)?;
        let id = self
            .engine
            .world
            .insert(room, object)
            .map_err(anyhow::Error::msg)?;
        Ok(id.as_str())
    }

    pub fn set_terrain(&mut self, room: &str, terrain: Vec<u8>) -> anyhow::Result<()> {
        let mut t = Terrain::from_codes(&terrain).map_err(anyhow::Error::msg)?;
        t.normalize();
        self.engine
            .world
            .set_terrain(room, t)
            .map_err(anyhow::Error::msg)
    }

    /// 执行一个玩家的代码并恰好推进 1 tick（screeps-ok runPlayer 契约）。
    /// 求值发生在结算前（玩家看见上一 tick 的世界），推进 = 空 intents 的真实结算。
    pub fn run_player(&mut self, handle: &str, code: &str) -> anyhow::Result<PlayerRun> {
        let thread_id = *self
            .handles
            .get(handle)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 handle: {handle}"))?;
        let players = self
            .players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?;
        let world = self.engine.world.world_json();
        let report: EvalReport = players.run_player(thread_id, &world, code)?;
        let out = self.finish_player_run(handle, report)?;
        self.engine.tick().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(out)
    }

    /// 多玩家对同一状态求值后合计推进 1 tick（runPlayers 契约）。
    pub fn run_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, PlayerRun>> {
        // 未知 handle 显式报错：静默跳过会让结果映射缺项，调用方无从得知玩家没跑
        for handle in codes.keys() {
            if !self.handles.contains_key(handle) {
                anyhow::bail!("未知玩家 handle: {handle}");
            }
        }
        let world = self.engine.world.world_json();
        let mut reports = BTreeMap::new();
        {
            let players = self
                .players
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("分片未创建"))?;
            // 固定顺序（handle 排序）求值——确定性
            for (handle, thread_id) in self.handles.clone() {
                let Some(code) = codes.get(&handle) else {
                    continue;
                };
                reports.insert(handle, players.run_player(thread_id, &world, code)?);
            }
        }
        let mut out = BTreeMap::new();
        for (handle, report) in reports {
            let run = self.finish_player_run(&handle, report)?;
            out.insert(handle, run);
        }
        self.engine.tick().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(out)
    }

    /// 求值报告 → PlayerRun（executed 判定 + console 捕获）。
    fn finish_player_run(&mut self, handle: &str, report: EvalReport) -> anyhow::Result<PlayerRun> {
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
        Ok(PlayerRun {
            executed,
            expr: report.expr,
            console,
        })
    }

    /// 推进 n tick（每 tick 一次全房间空 intents 结算）。
    ///
    /// 结算 Err 时世界整体回退至上一 tick（两阶段提交：全房在克隆副本上结算
    /// 成功后才写回）——time/能量/id 分配器全部保持，对同一 tick 重试安全。
    pub fn tick(&mut self, count: u64) -> anyhow::Result<()> {
        for _ in 0..count {
            self.engine.tick().map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(())
    }

    /// 对象快照（字段名=玩家 getter 名的投影，world_json 同一形状）。
    pub fn get_object(&self, id: &str) -> Option<Value> {
        kk_world::ObjId::parse(id).and_then(|id| self.engine.world.object_json_by_id(id))
    }

    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        self.engine
            .world
            .find_in_room(room, find_const)
            .into_iter()
            .map(|(_, v)| v)
            .collect()
    }

    pub fn get_game_time(&self) -> u64 {
        self.engine.world.time
    }

    pub fn capture_console_logs(&self, handle: &str) -> Vec<String> {
        self.last_console.get(handle).cloned().unwrap_or_default()
    }

    pub fn get_controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        self.engine.world.controller_pos(room)
    }

    pub fn teardown(&mut self) {
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.handles.clear();
        self.last_console.clear();
    }
}

impl Drop for EmbedEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(test)]
mod tests {
    use super::terrain_codes_from_json;
    use serde_json::json;

    /// 元素级严格性：非整数（小数/字符串/null）、负数、超 255 一律拒绝。
    #[test]
    fn terrain_codes_from_json_is_strict() {
        assert_eq!(
            terrain_codes_from_json(&json!([0, 1, 2, 3])).unwrap(),
            vec![0, 1, 2, 3]
        );
        for bad in [json!(1.5), json!("1"), json!(null), json!(-1), json!(256)] {
            assert!(
                terrain_codes_from_json(&json!([bad])).is_err(),
                "元素 {bad} 必须被拒绝"
            );
        }
        assert!(terrain_codes_from_json(&json!("not-array")).is_err());
    }
}
