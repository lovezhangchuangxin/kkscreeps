//! Invader raid 管理命令：房间 raid 状态种子、显式生成器扫描与清理。
//! 对应套件 setInvaderRaidState / runInvaderRaidSpawner /
//! clearInvaderRaidCreeps 三个 fixture 入口（vanilla 适配器同形契约）。
use super::*;

/// 种子键的可接受形态：harvestedEnergy 非负整数（有源房才允许 >0）、
/// raidGoal 整数或 null（清除覆盖）、active 布尔、status 非空字符串、
/// controllerReservation 对象或 null（清除预定）。
pub fn validate_raid_seed(
    spec: &Value,
    has_source: bool,
    has_controller: bool,
) -> Result<(), String> {
    let Some(fields) = spec.as_object() else {
        return Err("raid 状态种子必须是对象".into());
    };
    for key in fields.keys() {
        if !matches!(
            key.as_str(),
            "harvestedEnergy" | "raidGoal" | "active" | "status" | "controllerReservation"
        ) {
            return Err(format!("raid 状态种子未知键: {key}"));
        }
    }
    if let Some(v) = fields.get("harvestedEnergy") {
        let n = v
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= 9.0e15)
            .ok_or_else(|| format!("harvestedEnergy 必须是非负整数: {v}"))?;
        if n > 0.0 && !has_source {
            return Err("harvestedEnergy > 0 需要房间内存在 source".into());
        }
    }
    if let Some(v) = fields.get("raidGoal")
        && !v.is_null()
    {
        v.as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= 9.0e15)
            .ok_or_else(|| format!("raidGoal 必须是非负整数或 null: {v}"))?;
    }
    if let Some(v) = fields.get("active")
        && !v.is_boolean()
    {
        return Err(format!("active 必须是布尔: {v}"));
    }
    if let Some(v) = fields.get("status") {
        v.as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("status 必须是非空字符串: {v}"))?;
    }
    if let Some(v) = fields.get("controllerReservation")
        && !v.is_null()
    {
        let fields = v
            .as_object()
            .ok_or_else(|| format!("controllerReservation 必须是对象或 null: {v}"))?;
        if !has_controller {
            return Err("controllerReservation 需要房间内存在控制器".into());
        }
        // 与顶层一致的严格度：对象内未知子键整单拒绝（写入只取两键，
        // 静默忽略会让打错的字段名无声丢失）。
        for key in fields.keys() {
            if !matches!(key.as_str(), "owner" | "ticksToEnd") {
                return Err(format!("controllerReservation 未知子键: {key}"));
            }
        }
        let owner = fields
            .get("owner")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "controllerReservation.owner 必须是非空字符串".to_string())?;
        if kk_world::domain::npc::is_npc_owner(owner) {
            return Err(format!(
                "controllerReservation.owner 不能是 NPC 保留名: {owner}"
            ));
        }
        if let Some(t) = fields.get("ticksToEnd")
            && !t
                .as_f64()
                .is_some_and(|n| n.fract() == 0.0 && (0.0..=9.0e15).contains(&n))
        {
            return Err(format!(
                "controllerReservation.ticksToEnd 必须是非负整数: {t}"
            ));
        }
    }
    Ok(())
}

impl EmbedEngine {
    /// 种子房间 raid 状态；提供的键各自生效，未提供的键保持不变
    ///（vanilla 逐键 $set 语义）。controllerReservation 以绝对 endTime
    /// 写入控制器预定（缺省 ticksToEnd 5000，同 vanilla）。
    pub fn set_invader_raid_state(&mut self, room: &str, spec: &Value) -> anyhow::Result<()> {
        self.no_open_round("setInvaderRaidState")?;
        let world = self.engine.world();
        let Some(room_state) = world.room(room) else {
            anyhow::bail!("房间 {room} 不存在");
        };
        let has_source = room_state
            .objects
            .values()
            .any(|o| matches!(o, kk_schema::room::RoomObject::Source(_)));
        let has_controller = room_state.controller().is_some();
        validate_raid_seed(spec, has_source, has_controller).map_err(anyhow::Error::msg)?;
        let owner = spec
            .get("controllerReservation")
            .and_then(Value::as_object)
            .map(|fields| {
                fields
                    .get("owner")
                    .and_then(Value::as_str)
                    .expect("validate_raid_seed 已校验")
                    .to_string()
            });
        if let Some(owner) = &owner
            && !self.engine.players().contains_key(owner)
        {
            anyhow::bail!("controllerReservation.owner {owner} 不是本分片玩家");
        }
        // 整数经 as_f64 读取再取整（JSON 5000.0 与 5000 等价，as_i64 对
        // 浮点形态返回 None 会误当清除）。
        let read_int =
            |v: &Value| -> i64 { v.as_f64().expect("validate_raid_seed 已校验整数") as i64 };
        let read_int_opt = |n: f64| -> i64 { n as i64 };
        self.engine
            .edit_world(|world| {
                let time = world.time();
                let reservation_spec = spec.get("controllerReservation");
                world.edit_room(room, |mut contents| {
                    let raid = &mut contents.raid;
                    if let Some(v) = spec.get("harvestedEnergy") {
                        raid.harvested_energy = read_int(v);
                    }
                    if let Some(v) = spec.get("raidGoal") {
                        raid.invader_goal = v.as_f64().map(read_int_opt);
                    }
                    if let Some(v) = spec.get("active") {
                        raid.active = v.as_bool().expect("已校验布尔");
                    }
                    if let Some(v) = spec.get("status") {
                        raid.status = v.as_str().expect("已校验字符串").to_string();
                    }
                    if let Some(v) = reservation_spec {
                        // null 清除在无控制器房间是无操作（vanilla 的
                        // `if (controller)` 同形，不 panic）；非 null 已由
                        // 校验挡在「有预定必有控制器」前置门。
                        if let Some(ctl) = contents.objects.values_mut().find_map(|o| match o {
                            kk_schema::room::RoomObject::Controller(c) => Some(c),
                            _ => None,
                        }) {
                            ctl.reservation =
                                v.as_object().map(|fields| kk_schema::room::Reservation {
                                    user: owner.clone().expect("非 null 必有 owner"),
                                    // as_f64 取整与 harvestedEnergy/raidGoal 同策：
                                    // as_u64 对 100.0 这类浮点整数形态返回 None。
                                    end_time: time.saturating_add(
                                        fields
                                            .get("ticksToEnd")
                                            .and_then(Value::as_f64)
                                            .map(|n| n as u64)
                                            .unwrap_or(5000),
                                    ),
                                });
                        }
                    }
                    Ok(())
                })
            })
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    /// 显式执行一次官方 genInvaders 扫描。`random` 为注入序列（每值须
    /// 落在 [0,1)，耗尽报错）；缺省用世界种子派生流（确定性回放，官方
    /// 为真随机，登记分歧）。
    pub fn run_invader_raid_spawner(&mut self, random: Option<Vec<f64>>) -> anyhow::Result<()> {
        self.no_open_round("runInvaderRaidSpawner")?;
        let raid_random = match random {
            Some(values) => {
                for (index, value) in values.iter().enumerate() {
                    if !value.is_finite() || *value < 0.0 || *value >= 1.0 {
                        anyhow::bail!("random[{index}] 必须落在 [0,1): {value}");
                    }
                }
                kk_engine::settle::RaidRandom::sequence(values)
            }
            None => kk_engine::settle::RaidRandom::seeded(
                self.engine.world().seed(),
                self.engine.world().time(),
            ),
        };
        self.engine
            .run_invader_raid_spawner(raid_random)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    /// 移除房间内全部 Invader 归属 creep（fixture 清理，无墓碑无掉落）。
    pub fn clear_invader_raid_creeps(&mut self, room: &str) -> anyhow::Result<()> {
        self.no_open_round("clearInvaderRaidCreeps")?;
        if self.engine.world().room(room).is_none() {
            anyhow::bail!("房间 {room} 不存在");
        }
        self.engine
            .edit_world(|world| {
                let ids: Vec<kk_world::ObjId> = world
                    .room(room)
                    .into_iter()
                    .flat_map(|r| r.objects.iter())
                    .filter(|(_, o)| {
                        matches!(o, kk_schema::room::RoomObject::Creep(c)
                            if c.owner == kk_world::domain::npc::INVADER_OWNER)
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for id in ids {
                    world.remove(id)?;
                }
                Ok(())
            })
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }
}
