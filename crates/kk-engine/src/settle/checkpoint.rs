//! 已提交引擎状态的持久清单；规则与执行器由宿主重建后再核验装入。
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineCheckpoint {
    pub world: kk_world::checkpoint::WorldCheckpoint,
    pub players: BTreeMap<String, PlayerState>,
    pub market: market::MarketState,
    pub next_id: u64,
    pub last_events: Vec<Value>,
    pub mod_states: BTreeMap<String, Value>,
    pub plan_hash: String,
}

impl Engine {
    pub fn checkpoint(&self) -> Result<EngineCheckpoint, String> {
        if !matches!(self.run_state, RunState::Ready | RunState::Committed { .. }) {
            return Err("只能保存无开放轮次的已提交引擎状态".into());
        }
        Ok(EngineCheckpoint {
            world: self.world.checkpoint(),
            players: self.players.clone(),
            market: self.market.clone(),
            next_id: self.next_id,
            last_events: self.last_events.clone(),
            mod_states: self.mod_states.clone(),
            plan_hash: self.plan_hash().hex(),
        })
    }

    /// 只在隔离的新引擎上使用；全部检查通过后一次装入，不推进时间。
    pub fn restore_checkpoint(&mut self, saved: EngineCheckpoint) -> Result<(), String> {
        if !matches!(self.run_state, RunState::Ready | RunState::Committed { .. })
            || saved.plan_hash != self.plan_hash().hex()
        {
            return Err("检查点计划身份不匹配或引擎存在开放轮次".into());
        }
        self.adopt_committed_state(saved)
    }

    /// 换计划收养（包切换专用）：宿主已先在本引擎装好新计划，载荷的
    /// `plan_hash` 描述切换前世界，跳过等值门；mod_states 命名空间、
    /// Mod 对象类型与扩展字段 schema 的内容校验对**新计划**照常执行
    /// ——它们同时是去包残留的硬门（残留即拒绝，不清理、不修补）。
    pub fn restore_checkpoint_replanned(&mut self, saved: EngineCheckpoint) -> Result<(), String> {
        if !matches!(self.run_state, RunState::Ready | RunState::Committed { .. }) {
            return Err("换计划收养被拒：引擎存在开放轮次".into());
        }
        self.adopt_committed_state(saved)
    }

    /// 已提交载荷的公共装入路径：结构/限额/内容校验全通过后一次换入。
    fn adopt_committed_state(&mut self, saved: EngineCheckpoint) -> Result<(), String> {
        let world = kk_world::World::from_checkpoint(saved.world)?;
        if saved.next_id == 0 || saved.next_id > world.next_obj_id() {
            return Err("检查点引擎 ID 游标非法".into());
        }
        for (name, player) in &saved.players {
            if name.is_empty() || player.gcl < 0 || player.notifications.len() > NOTIFY_RECORD_CAP {
                return Err(format!("检查点玩家 {name:?} 状态非法"));
            }
            player.cpu.validate_checkpoint()?;
            if player.raw_memory.encode_utf16().count() > 2 * 1024 * 1024
                || player
                    .segments
                    .iter()
                    .any(|(id, value)| *id > 99 || value.encode_utf16().count() > 100 * 1024)
                || player.active_request.len() > 10
                || player
                    .active_request
                    .iter()
                    .chain(&player.public_ids)
                    .any(|id| *id > 99)
                || player.default_public.is_some_and(|id| id > 99)
                || player
                    .foreign_request
                    .as_ref()
                    .is_some_and(|(_, id)| id.is_some_and(|id| id > 99))
            {
                return Err(format!("检查点玩家 {name} Memory/segments 超出限额"));
            }
        }
        saved.market.validate_checkpoint()?;
        if saved
            .mod_states
            .keys()
            .any(|namespace| !self.plan.sources().contains_key(namespace))
        {
            return Err("检查点含未安装命名空间的 Mod state".into());
        }
        let mut rooms: Vec<_> = world
            .rooms()
            .iter()
            .map(|(name, room)| (name.clone(), room.clone()))
            .collect();
        self.validate_mod_object_rooms(&rooms)
            .map_err(|e| e.to_string())?;
        let before: Vec<_> = rooms
            .iter()
            .map(|(_, room)| room.extensions.clone())
            .collect();
        self.validate_extension_rooms(&mut rooms)
            .map_err(|e| e.to_string())?;
        if before
            != rooms
                .iter()
                .map(|(_, room)| room.extensions.clone())
                .collect::<Vec<_>>()
        {
            return Err("检查点含未收敛的扩展对象引用".into());
        }
        for (_, room) in &rooms {
            for (id, fields) in &room.extensions {
                let object = room.objects.get(id).expect("世界解码已检查扩展对象");
                let RoomObject::Generic(g) = object else {
                    return Err("扩展对象不是 Generic".into());
                };
                let data: Value = serde_json::from_str(&g.data).map_err(|e| e.to_string())?;
                let key = kk_mod_api::Key::new(format!(
                    "builtin:{}",
                    data["structureType"].as_str().unwrap_or("")
                ))?;
                for (field, value) in fields {
                    let rule = self
                        .extensions
                        .field(&key, &kk_mod_api::Key::new(field.clone())?)
                        .ok_or("未声明的扩展字段")?;
                    kk_mod_api::validate::check_value(value, &rule.schema, field)?;
                }
            }
        }
        self.world = world;
        self.players = saved.players;
        self.market = saved.market;
        self.next_id = saved.next_id;
        self.last_events = saved.last_events;
        self.mod_states = saved.mod_states;
        Ok(())
    }
}
