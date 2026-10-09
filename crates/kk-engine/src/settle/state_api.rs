//! 权威状态的只读面和隔离编辑入口；恢复轮不能被外部修改。
use super::*;

impl Engine {
    pub fn world(&self) -> &kk_world::World {
        &self.world
    }
    pub fn players(&self) -> &BTreeMap<String, PlayerState> {
        &self.players
    }
    pub fn market(&self) -> &market::MarketState {
        &self.market
    }
    pub fn mod_states(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.mod_states
    }
    pub fn last_events(&self) -> &[serde_json::Value] {
        &self.last_events
    }

    fn ensure_editable(&self) -> Result<(), String> {
        if !matches!(self.run_state, RunState::Ready | RunState::Committed { .. }) {
            return Err("权威状态编辑被拒：存在开放轮次或待恢复候选".into());
        }
        if self.world.generation() >= u64::MAX - 1 {
            return Err("世界版本已耗尽".into());
        }
        Ok(())
    }

    /// 编辑隔离副本；闭包报错或内容校验失败均保留原状态。
    pub fn edit_world<R>(
        &mut self,
        edit: impl FnOnce(&mut kk_world::World) -> Result<R, String>,
    ) -> Result<R, String> {
        self.ensure_editable()?;
        let mut draft = self.world.clone();
        let result = edit(&mut draft)?;
        self.validate_world_for_plan(&draft)?;
        self.world.adopt_edit(draft)?;
        Ok(result)
    }

    pub fn insert_object(&mut self, room: &str, object: RoomObject) -> Result<ObjId, String> {
        self.insert_objects([(room.to_string(), object)])
            .map(|ids| ids[0])
    }

    /// 按输入顺序一次编辑/校验/提交；任意后项失败都保留原世界及 ID 游标。
    pub fn insert_objects(
        &mut self,
        objects: impl IntoIterator<Item = (String, RoomObject)>,
    ) -> Result<Vec<ObjId>, String> {
        self.ensure_editable()?;
        let objects: Vec<_> = objects.into_iter().collect();
        if objects.is_empty() {
            return Ok(Vec::new());
        }
        self.edit_world(|world| {
            objects
                .into_iter()
                .map(|(room, object)| world.insert(&room, object))
                .collect()
        })
    }
    pub fn register_room(
        &mut self,
        name: impl Into<String>,
        terrain: kk_core::Terrain,
    ) -> Result<(), String> {
        self.edit_world(|world| {
            world.register_room(name, terrain)?;
            Ok(())
        })
    }
    pub fn edit_object<R>(
        &mut self,
        id: ObjId,
        edit: impl FnOnce(&mut RoomObject) -> Result<R, String>,
    ) -> Result<R, String> {
        self.edit_world(|world| world.edit_object(id, edit))
    }
    pub fn edit_room<R>(
        &mut self,
        name: &str,
        edit: impl FnOnce(kk_world::RoomContents<'_>) -> Result<R, String>,
    ) -> Result<R, String> {
        self.edit_world(|world| world.edit_room(name, edit))
    }
    pub fn set_terrain(&mut self, name: &str, terrain: kk_core::Terrain) -> Result<(), String> {
        self.edit_world(|world| world.set_terrain(name, terrain))
    }

    /// 管理触发的 raid 生成器：一次候选编辑内执行官方 genInvaders 扫描
    ///（房态门、扇区核、出口资格、组合生成、阈值重掷）。随机源由调用方
    /// 注入：序列耗尽报错并保留原世界。
    pub fn run_invader_raid_spawner(
        &mut self,
        mut random: super::invader_raid::RaidRandom,
    ) -> Result<(), String> {
        self.edit_world(|world| super::invader_raid::run_spawner(world, &mut random))
    }

    /// 管理触发的 powerBank 生成器：一次候选编辑内执行官方
    /// genPowerBanks 扫描（高速路房调度、到期在墙体目标格生成）。
    /// 随机源由调用方注入：序列耗尽报错并保留原世界。
    pub fn run_power_bank_spawner(
        &mut self,
        mut random: super::invader_raid::RaidRandom,
    ) -> Result<(), String> {
        self.edit_world(|world| super::power_bank::run_spawner(world, &mut random))
    }

    /// 写入 powerBank 生成器的下次到期 tick（官方 powerBankTime 域的
    /// 管理面）：生成器测试与运维需要直接调度时使用。
    pub fn set_power_bank_time(&mut self, room: &str, value: Option<u64>) -> Result<(), String> {
        self.edit_world(|world| world.set_power_bank_time(room, value))
    }
    #[doc(hidden)]
    pub fn set_time_for_test(&mut self, time: u64) -> Result<(), String> {
        self.edit_world(|world| {
            world.set_time_for_test(time)?;
            Ok(())
        })
    }
    #[doc(hidden)]
    pub fn bump_next_obj_id_for_test(&mut self, next: u64) -> Result<(), String> {
        self.edit_world(|world| {
            world.bump_next_obj_id(next);
            Ok(())
        })
    }

    pub fn edit_players<R>(
        &mut self,
        edit: impl FnOnce(&mut BTreeMap<String, PlayerState>) -> Result<R, String>,
    ) -> Result<R, String> {
        self.ensure_editable()?;
        let mut draft = self.players.clone();
        let result = edit(&mut draft)?;
        Self::validate_players(&draft)?;
        self.players = draft;
        self.world.bump_generation()?;
        Ok(result)
    }

    pub fn edit_market<R>(
        &mut self,
        edit: impl FnOnce(&mut market::MarketState) -> Result<R, String>,
    ) -> Result<R, String> {
        self.ensure_editable()?;
        let mut draft = self.market.clone();
        let result = edit(&mut draft)?;
        draft.validate_checkpoint()?;
        self.market = draft;
        self.world.bump_generation()?;
        Ok(result)
    }

    pub fn edit_mod_states<R>(
        &mut self,
        edit: impl FnOnce(&mut BTreeMap<String, serde_json::Value>) -> Result<R, String>,
    ) -> Result<R, String> {
        self.ensure_editable()?;
        let mut draft = self.mod_states.clone();
        let result = edit(&mut draft)?;
        if draft
            .keys()
            .any(|name| !self.plan.sources().contains_key(name))
        {
            return Err("Mod state 含未安装命名空间".into());
        }
        self.mod_states = draft;
        self.world.bump_generation()?;
        Ok(result)
    }
}
