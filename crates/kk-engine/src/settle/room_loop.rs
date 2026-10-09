//! 房间意图派发、移动仲裁与对象 tick 循环。
use super::*;

/// 更新 creep 和可建造结构的受袭通知开关，拒绝修改他人对象。
/// 这里只保存标志，不发送受袭通知。
/// 来源：上游 engine/src/processor.js 的 notifyWhenAttacked 通用处理。
fn notify_when_attacked(w: &mut Working, player: &str, id: ObjId, data: &IntentData) {
    let Some(obj) = w.room.objects.get(&id) else {
        return;
    };
    if kk_world::domain::object_owner(obj).is_some_and(|owner| owner != player) {
        return; // 拒绝他人对象；无主结构不在此拦截。
    }
    // 控制器、资源源、工地和旗帜不接受此开关；无主可建造结构可以接受。
    // powerCreep（Generic 承载）与 creep 同形接受。
    let type_gated = matches!(obj, RoomObject::Creep(_))
        || super::powers::is_power_creep(obj)
        || super::buildings::cost_bearing(w, &movement::type_name_of(obj));
    if !type_gated {
        return;
    }
    let enabled = data.enabled.unwrap_or(false);
    match w.room.objects.get_mut(&id) {
        Some(RoomObject::Creep(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Spawn(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Extension(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Storage(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::ConstructedWall(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Road(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Extractor(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Generic(o)) => {
            // Generic 将标志保存在 JSON 中，供世界投影读取。
            let mut v: Value = serde_json::from_str(&o.data).unwrap_or_default();
            v["notifyWhenAttacked"] = Value::Bool(enabled);
            o.data = v.to_string();
        }
        _ => {}
    }
}

/// 读取掉落资源名：命名字段优先，否则按旧编码 1=power、2=G、其余=energy。
pub fn energy_resource_name(o: &kk_schema::room::EnergyObj) -> &str {
    match (&o.resource, o.resource_type) {
        (Some(name), _) => name.as_str(),
        (None, 1) => "power",
        (None, 2) => kk_core::constants::RESOURCE_GHODIUM,
        (None, _) => "energy",
    }
}

/// 原地结算房间，返回事件并替换 room.events。
///
/// 本函数不创建副本，也不回滚 room 与共享状态。调用方须提供
/// 候选，返回 Err 后丢弃整个候选。Engine 负责跨房间的提交边界。
/// room.name 须为世界目录中的房名，用于筛选房间级指令；time 是结算前
/// 的绝对 tick，now_ms 是 Unix 毫秒墙钟。对象级指令仅处理本房间存在的 ID。
pub fn settle_room(
    room: RoomState,
    time: u64,
    now_ms: u64,
    state: &TickState,
    batch: &IntentBatch,
    plane: &Plane,
) -> Result<(RoomState, Vec<Value>, Vec<inter_room::InterRoomMove>), SettleError> {
    // 候选以共享句柄持有：Rust 链与 JS 整槽宿主在本次调用期间共享
    // 同一候选，见 RoomCtx 的说明。
    let ctx: RoomCtx = Rc::new(RefCell::new(Working::new(
        room,
        time,
        now_ms,
        state.clone(),
        plane.clone(),
    )));

    // 先清空动作记录和说话载荷，避免上一轮内容继续可见。
    {
        let mut w = ctx.borrow_mut();
        for creep in w.room.objects.values_mut() {
            if let RoomObject::Creep(c) = creep {
                c.action_log = 0;
                c.say_message = None;
                c.say_public = None;
            } else if let RoomObject::Generic(g) = creep
                && g.data.contains("\"observeRoom\"")
                && let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&g.data)
                && v.get("structureType").and_then(serde_json::Value::as_str) == Some("observer")
                && v.as_object().is_some_and(|m| m.contains_key("observeRoom"))
            {
                // observer 的观察标记在预意图阶段清零（官方 processor.js
                // :115-117 仅对 type==observer 同形）：意图臂随后覆盖写，
                // 无新意图的观察视野恰一拍后失效。前置 contains 判避免逐
                // 对象序列化；类型门防 Mod 配方字段名同名被逐拍剥离。
                v.as_object_mut().unwrap().remove("observeRoom");
                g.data = v.to_string();
            } else if super::powers::is_power_creep(creep)
                && let RoomObject::Generic(g) = creep
                && let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&g.data)
                && v.as_object().is_some_and(|m| m.contains_key("actionLog"))
            {
                // powerCreep 的动作记录按官方 processor.js:151-157 每 tick
                // 开头重置为六键全 null；键整体移除在读取链（saying getter 的
                // falsy 守卫、墓碑遗言分支）下与之等价，且免去每 tick 全量
                // JSON 序列化。变更时由写入方整体回写。
                v.as_object_mut().unwrap().remove("actionLog");
                g.data = v.to_string();
            }
        }
        // 处理指令前，按控制器配额计算 spawn/extension 是否启用。
        spawn::calc_spawns(&mut w);
    }

    // 官方 bulk.insert 的新 creep 下一轮才进入对象 tick；记录初始集合，
    // 避免一拍孵化的任务清除后，新 creep 同轮提前走孤儿出生路径。
    let initial_creeps: std::collections::BTreeSet<ObjId> = ctx
        .borrow()
        .room
        .objects
        .iter()
        .filter_map(|(id, object)| matches!(object, RoomObject::Creep(_)).then_some(*id))
        .collect();

    // 玩家按名字排序，先处理房间指令，再按对象 ID 处理对象动作。
    for (player, intents) in &batch.users {
        room::process_room_intents(&mut ctx.borrow_mut(), player, &intents.room);
        for (obj_id, actions) in &intents.objects {
            // 通知开关是对象通用指令，在类型分派前处理。
            if let Some(data) = actions.get(&Action::NotifyWhenAttacked) {
                notify_when_attacked(&mut ctx.borrow_mut(), player, *obj_id, data);
            }
            // Creep 和 spawn 须属于当前玩家；controller 另允许无主对象进入处理器。
            // 先取对象类别再释放借用：处理器链需要共享句柄，不能持借用进入。
            let kind = {
                let w = ctx.borrow();
                match w.room.objects.get(obj_id) {
                    Some(RoomObject::Creep(o)) if o.owner == *player => Some("creep"),
                    Some(RoomObject::Spawn(o)) if o.owner == *player => Some("spawn"),
                    Some(RoomObject::Controller(o))
                        if o.owner.as_deref().is_none_or(|u| u == player.as_str()) =>
                    {
                        Some("controller")
                    }
                    // powerCreep 是 Generic 承载的非结构对象，先于结构臂拦截。
                    Some(RoomObject::Generic(g))
                        if kk_world::domain::generic::type_name(&g.data).as_deref()
                            == Some("powerCreep") =>
                    {
                        Some("powerCreep")
                    }
                    // 真实类型在 structureType 中，处理器再检查结构类型和所有权。
                    Some(RoomObject::Generic(_)) => Some("structure"),
                    _ => None,
                }
            };
            match kind {
                Some("creep") => {
                    creep::process_creep_intents(&ctx, *obj_id, actions).map_err(SettleError)?;
                }
                Some("spawn") => {
                    spawn::process_spawn_intents(&mut ctx.borrow_mut(), *obj_id, actions);
                }
                Some("controller") => {
                    controller::process_controller_intents(&mut ctx.borrow_mut(), *obj_id, actions);
                }
                Some("powerCreep") => {
                    powers::process_power_creep_intents(
                        &mut ctx.borrow_mut(),
                        *obj_id,
                        player,
                        actions,
                    );
                }
                Some("structure") => {
                    structures::process_structure_intents(&ctx, *obj_id, player, actions)
                        .map_err(SettleError)?;
                }
                _ => continue,
            }
        }
        // Mod 对象的扩展意图在既有动作之后按对象 ID 顺序派发；目标不是
        // Mod 对象的扩展意图在此静默丢弃（与未知 vanilla 动作同待遇）。
        for (obj_id, ext_intents) in &intents.ext {
            let is_mod = matches!(
                ctx.borrow().room.objects.get(obj_id),
                Some(RoomObject::Mod(_))
            );
            if is_mod {
                mod_intents::process_object_ext_intents(&ctx, player, *obj_id, ext_intents)
                    .map_err(SettleError)?;
            }
        }
    }

    damage::drain_mod_damage(&ctx)?;

    // 所有 move/pull 登记完成后，再统一仲裁目的格冲突。有竞争策略
    // 贡献时先释放借用经整槽宿主生成优先级候选；正常拒绝回退默认评分。
    contest::arbitrate_room(&ctx).map_err(SettleError)?;

    // 按指令处理后的存量重算可用能量，供 spawn tick 的低能量补给判断。
    let _energy_available = spawn::energy_available(&ctx.borrow());

    // 此时固定 ID 列表：指令阶段创建的对象参加 tick（新 creep 除外），
    // 逐对象 tick 中新建的对象不参加这一轮。对象按 ID 升序处理。
    let ids: Vec<ObjId> = ctx.borrow().room.objects.keys().copied().collect();
    for id in ids {
        let kind = {
            let w = ctx.borrow();
            match w.room.objects.get(&id) {
                Some(RoomObject::Energy(_)) => "energy",
                Some(RoomObject::Source(_)) => "source",
                Some(RoomObject::Creep(_)) => "creep",
                Some(RoomObject::Spawn(_)) => "spawn",
                Some(RoomObject::Controller(_)) => "controller",
                Some(RoomObject::Tombstone(_)) => "tombstone",
                Some(RoomObject::Ruin(_)) => "ruin",
                Some(RoomObject::Mineral(_)) => "mineral",
                Some(RoomObject::Extractor(_)) => "extractor",
                Some(RoomObject::Road(_)) => "road",
                Some(RoomObject::Storage(_)) => "storage",
                Some(RoomObject::Mod(_)) => "mod",
                Some(RoomObject::Generic(g)) => {
                    if kk_world::domain::generic::type_name(&g.data).as_deref() == Some("nuke") {
                        "nuke"
                    } else if kk_world::domain::generic::type_name(&g.data).as_deref()
                        == Some("powerCreep")
                    {
                        "powerCreep"
                    } else if kk_world::domain::generic::type_name(&g.data).as_deref()
                        == Some("deposit")
                    {
                        "deposit"
                    } else {
                        match kk_world::domain::generic::structure_type(&g.data).as_deref() {
                            Some("container" | "rampart") => "decay",
                            Some("link") => "link",
                            Some("terminal") => "terminal",
                            // Mod 建筑：生产周期或生命周期 tick 任一声明即进入
                            //（同访先产后 tick）；两者皆无的建筑不进入。
                            Some(st)
                                if plane.buildings.get(st).is_some_and(|b| {
                                    b.production.is_some() || !b.lifecycle_slots.is_empty()
                                }) =>
                            {
                                "mod-building"
                            }
                            _ => "",
                        }
                    }
                }
                _ => "",
            }
        };
        match kind {
            "energy" => economy::energy_tick(&mut ctx.borrow_mut(), id),
            "source" => economy::source_tick(&mut ctx.borrow_mut(), id),
            "creep" => {
                if !initial_creeps.contains(&id) {
                    continue;
                }
                creep::creep_tick_prepare(&mut ctx.borrow_mut(), id)?;
                let contrib = ctx.borrow().plane.contrib.clone();
                let selected = contrib.run_incoming(&ctx, id).map_err(SettleError)?;
                damage::apply_pending_hits_selected(&mut ctx.borrow_mut(), id, selected);
            }
            "spawn" => spawn::spawn_tick(
                &mut ctx.borrow_mut(),
                id,
                _energy_available,
                &initial_creeps,
            ),
            "controller" => economy::controller_tick(&mut ctx.borrow_mut(), id),
            "tombstone" => economy::tombstone_tick(&mut ctx.borrow_mut(), id),
            "ruin" => economy::ruin_tick(&mut ctx.borrow_mut(), id),
            "mineral" => economy::mineral_tick(&mut ctx.borrow_mut(), id),
            "deposit" => economy::deposit_tick(&mut ctx.borrow_mut(), id),
            "extractor" => economy::extractor_tick(&mut ctx.borrow_mut(), id),
            "road" => economy::road_decay_tick(&mut ctx.borrow_mut(), id),
            "decay" => economy::generic_decay_tick(&mut ctx.borrow_mut(), id),
            "link" => {
                // 贡献链经共享句柄执行：wrap 的 next() 会重新借用候选，
                // 此处不得持有借用。
                let contrib = ctx.borrow().plane.contrib.clone();
                contrib.run_tick(&ctx, id).map_err(SettleError)?
            }
            "storage" => storage::tick(&mut ctx.borrow_mut(), id),
            "terminal" => economy::terminal_capacity_tick(&mut ctx.borrow_mut(), id),
            "nuke" => {
                nukes::nuke_tick(&mut ctx.borrow_mut(), id);
                // 核弹在对象 tick 中产生托管结构伤害；释放 Working 借用后
                // 立即跑受击链，使后续对象/核弹读取已应用的候选耐久。
                damage::drain_mod_damage(&ctx)?;
            }
            "powerCreep" => powers::power_creep_tick(&mut ctx.borrow_mut(), id)?,
            "mod-building" => buildings::building_object_tick(&ctx, id).map_err(SettleError)?,
            "mod" => mod_intents::process_object_tick(&ctx, id).map_err(SettleError)?,
            _ => {}
        }
    }

    state.check_fatal()?;
    // 整体替换本轮事件，下一轮玩家执行时由 room.getEventLog 读取。
    // settle_room 是本房间候选的唯一持有者，Rc 强计数为 1；
    // 处理器链只在调用期间借用，不留存句柄。
    let mut w = Rc::into_inner(ctx)
        .expect("房间候选在结算期间不应被处理器留存")
        .into_inner();
    w.room.events = w.events.iter().map(Value::to_string).collect();

    let pending_inter_room = std::mem::take(&mut w.pending_inter_room);
    Ok((w.room, w.events, pending_inter_room))
}
