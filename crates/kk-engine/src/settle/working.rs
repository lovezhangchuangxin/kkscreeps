//! 单房间候选及其对象、索引、资源和事件操作。
use super::*;

/// 一次结算携带的贡献平面快照：有效规则、槽执行表、扩展字段目录与
/// 冻结计划本体。由引擎装配时生成，结算全程只读；这些成员必须来自
/// 同一冻结计划。
///
/// 槽执行表、扩展目录和计划持 `Rc`/`Arc` 而非借用：整槽宿主在本次调用期间通过
/// OpState 持有候选句柄，返回时移除；借用栈上平面无法进入该拥有状态。
///
/// 克隆成本：两次非原子 `Rc` 递增 + 一次原子 `Arc` 递增，
/// 再按值拷贝 link 与仓库规则，每房每 tick 一次，无深拷贝。
#[derive(Clone)]
pub struct Plane {
    pub links: kk_mod_api::builtin::LinkRules,
    pub stores: kk_mod_api::builtin::StoreRules,
    pub contrib: Rc<contrib::ContributionDispatch>,
    pub extensions: Rc<kk_mod_api::extension::ExtensionCatalog>,
    /// 冻结计划：意图描述符与投影类型目录经它解析（键解析发生在
    /// 装配期以外此处，是扩展意图的低频路径）。
    pub plan: Arc<kk_mod_api::ContributionPlan>,
}

/// 单房间结算上下文：房间副本、位置索引及共享候选状态，
/// 以及本轮的事件、伤害与移动记录。
///
/// 房间按值持有而非借用，候选可以在一次宿主调用期间进入 OpState。
/// 宿主返回后移除句柄；native op 只取短借用，候选不跨 tick 留存。
pub struct Working {
    pub room: RoomState,
    pub index: RoomIndex,
    pub time: u64,
    /// Unix 毫秒墙钟，来自本轮 TickInput。
    pub now_ms: u64,
    pub state: TickState,
    pub events: Vec<Value>,
    /// 处理 move 时记录的疲劳，供移动仲裁判断是否能移动。
    pub old_fatigue: BTreeMap<ObjId, u32>,
    /// 本轮疲劳累加值；保留负值用于拉拽传递，写回对象时取 max(0, 值)。
    pub tick_fatigue: BTreeMap<ObjId, i64>,
    /// 本轮各控制器的累计升级量，用于限制 RCL 8 的升级。
    pub upgraded: BTreeMap<ObjId, i64>,
    /// 指令阶段累计的 creep 伤害，在该 creep 的 tick 阶段施加。
    pub damage_to_apply: BTreeMap<ObjId, i64>,
    /// 待施加的治疗量，与伤害一起在 creep 死亡判断前处理。
    pub heal_to_apply: BTreeMap<ObjId, i64>,
    /// attackController 设置的升级封锁到期 tick，由 controller tick 写回对象。
    pub upgrade_block_pending: BTreeMap<ObjId, u64>,
    /// 本轮激活安全模式的控制器，controller tick 据此处理时钟。
    pub safe_mode_activated: BTreeSet<ObjId>,
    /// 采矿时设置的提取器冷却，由 extractor tick 写回冷却计数。
    pub extractor_cooldown_pending: BTreeMap<ObjId, u64>,
    pub movement: movement::Movement,
    /// 本 tick 登记的跨房过渡：位于边缘格的 creep 在全部房间结算后由
    /// 引擎阶段移入邻房候选。
    pub pending_inter_room: Vec<inter_room::InterRoomMove>,
    /// 本轮生效的贡献平面：link/仓库规则、槽执行表与扩展字段目录。
    pub plane: Plane,
}

/// 共享的房间候选句柄。进入 JS 或执行 Rust next 前必须释放可变借用；
/// 该纪律由 RefCell 动态检查，类型不会自动证明。宿主在调用期间可以
/// 放入 OpState，但返回前必须清空；处理器不得跨调用留存候选句柄。
pub type RoomCtx = Rc<RefCell<Working>>;

impl Working {
    pub fn creep(&self, id: ObjId) -> Option<&CreepObj> {
        match self.room.objects.get(&id) {
            Some(RoomObject::Creep(c)) => Some(c),
            _ => None,
        }
    }

    pub fn creep_mut(&mut self, id: ObjId) -> Option<&mut CreepObj> {
        match self.room.objects.get_mut(&id) {
            Some(RoomObject::Creep(c)) => Some(c),
            _ => None,
        }
    }

    pub fn controller(&self) -> Option<&kk_schema::room::ControllerObj> {
        self.room.objects.values().find_map(|o| match o {
            RoomObject::Controller(c) => Some(c),
            _ => None,
        })
    }

    /// 写入对象扩展字段的候选值。先解析对象类型（目前仅 Generic 结构
    /// 按 structureType 映射），再校验字段键合法、已在目录中声明，
    /// 最后按字段 schema 校验值（类型、取值域、长度限额、有限浮点）。
    /// 写入只修改本房候选，结算失败时随房间候选一并丢弃。
    pub fn set_extension(
        &mut self,
        id: ObjId,
        field: &str,
        value: serde_json::Value,
    ) -> Result<(), String> {
        let type_key = self.extension_type_of(id)?;
        let field_key =
            kk_mod_api::Key::new(field).map_err(|e| format!("扩展字段键非法 {field:?}: {e}"))?;
        let Some(rule) = self.plane.extensions.field(&type_key, &field_key) else {
            return Err(format!("扩展字段未声明：{field}"));
        };
        kk_mod_api::validate::check_value(&value, &rule.schema, field)?;
        self.room
            .extensions
            .entry(id)
            .or_default()
            .insert(field.to_string(), value);
        Ok(())
    }

    /// 移除对象的扩展字段候选；字段未在目录声明时拒绝。
    pub fn remove_extension(&mut self, id: ObjId, field: &str) -> Result<(), String> {
        let type_key = self.extension_type_of(id)?;
        let field_key =
            kk_mod_api::Key::new(field).map_err(|e| format!("扩展字段键非法 {field:?}: {e}"))?;
        if self.plane.extensions.field(&type_key, &field_key).is_none() {
            return Err(format!("扩展字段未声明：{field}"));
        }
        if let Some(fields) = self.room.extensions.get_mut(&id) {
            fields.remove(field);
            if fields.is_empty() {
                self.room.extensions.remove(&id);
            }
        }
        Ok(())
    }

    /// 解析对象在扩展目录中的类型键。目前只有 Generic 结构按
    /// structureType 映射 builtin:{structure_type}；其他对象族
    /// 尚未声明扩展类型，一律拒绝。
    fn extension_type_of(&self, id: ObjId) -> Result<kk_mod_api::Key, String> {
        let structure_type = match self.room.objects.get(&id) {
            Some(RoomObject::Generic(g)) => {
                let v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
                v.get("structureType")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            }
            _ => None,
        };
        let Some(st) = structure_type else {
            return Err(format!("对象 {} 的类型未接入扩展平面", id.as_str()));
        };
        let key = kk_mod_api::Key::new(format!("builtin:{st}"))
            .map_err(|e| format!("结构类型 {st:?} 不能构成类型键: {e}"))?;
        if self.plane.extensions.fields_of(&key).is_empty() {
            return Err(format!("结构类型 {st} 未声明扩展字段"));
        }
        Ok(key)
    }

    /// 使用共享游标创建对象，并登记位置索引。
    /// 调用方须保证游标不与已有 ID 冲突；游标饱和后无法继续可靠分配。
    pub fn insert_new(&mut self, object: RoomObject) -> ObjId {
        let id = self.state.allocate_id();
        let (x, y) = object.pos();
        self.room
            .insert_with_id(id, object)
            .expect("结算 id 分配器不得复用已有对象");
        self.index.insert(ObjId(id), (x, y));
        ObjId(id)
    }

    /// 删除对象及位置索引，同时清理它的疲劳、待伤害、待治疗和扩展字段。
    pub fn remove_obj(&mut self, id: ObjId) -> Option<RoomObject> {
        let obj = self.room.objects.remove(&id)?;
        let (x, y) = obj.pos();
        self.index.remove(id, (x, y));
        self.room.extensions.remove(&id);
        self.old_fatigue.remove(&id);
        self.tick_fatigue.remove(&id);
        self.damage_to_apply.remove(&id);
        self.heal_to_apply.remove(&id);
        Some(obj)
    }

    /// 读取 Mod 对象的字段 payload；非 Mod 对象或 data 非法 JSON 返回 None。
    pub fn mod_fields(&self, id: ObjId) -> Option<Value> {
        match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => serde_json::from_str(&o.data).ok(),
            _ => None,
        }
    }

    /// 校验并整值写入 Mod 对象的字段 payload：类型必须在计划中声明，
    /// 键集合与字段描述完全一致；声明的位置字段同步到对象信封坐标与
    /// 位置索引。只修改本房候选，失败随房间候选一并丢弃。
    pub fn update_mod_fields(&mut self, id: ObjId, fields: &Value) -> Result<(), String> {
        let mut fields = fields.clone();
        let type_key = match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => o.type_key.clone(),
            _ => return Err(format!("对象 {} 不是 Mod 对象", id.as_str())),
        };
        let key = kk_mod_api::Key::new(type_key.clone())
            .map_err(|e| format!("Mod 对象类型键非法 {type_key:?}: {e}"))?;
        let Some(handle) = self.plane.plan.projection().resolve(&key) else {
            return Err(format!("Mod 对象类型 {type_key} 不在计划中"));
        };
        let ty = self
            .plane
            .plan
            .projection()
            .get(handle)
            .expect("句柄来自计划");
        kk_mod_api::validate::check_object_payload(&fields, &ty.descriptor.fields, &type_key)?;
        // 数值 backing 规范化（Float 域统一浮点 backing）：KKPV 列式编码
        // 按值分派列型，整数/浮点 backing 混桶会使整轮投影失败。
        kk_mod_api::validate::normalize_object_payload(&mut fields, &ty.descriptor.fields);
        let (mut x, mut y) = match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => (o.x, o.y),
            _ => unreachable!("上面已判 Mod"),
        };
        if let Some([fx, fy]) = &ty.descriptor.position_fields {
            let px = u8::try_from(
                fields
                    .get(kk_mod_api::validate::field_name(fx))
                    .and_then(Value::as_u64)
                    .expect("payload 已通过 schema 校验，位置字段必为 0..49 整数"),
            )
            .expect("0..49 在 u8 内");
            let py = u8::try_from(
                fields
                    .get(kk_mod_api::validate::field_name(fy))
                    .and_then(Value::as_u64)
                    .expect("payload 已通过 schema 校验，位置字段必为 0..49 整数"),
            )
            .expect("0..49 在 u8 内");
            if px != x || py != y {
                self.index.apply_move(id, (x, y), (px, py));
                x = px;
                y = py;
            }
        }
        if let Some(RoomObject::Mod(o)) = self.room.objects.get_mut(&id) {
            o.data = fields.to_string();
            o.x = x;
            o.y = y;
        }
        Ok(())
    }

    /// 对象移动（索引增量更新）。Mod 对象拒绝：其位置同时存在于
    /// payload 字段与信封坐标，发布前校验要求两者一致，原始索引移动
    /// 不维护该不变量——Mod 的位置变更走 update_mod_fields。
    pub fn move_obj(&mut self, id: ObjId, to: (u8, u8)) -> Result<(), String> {
        if let Some(obj) = self.room.objects.get_mut(&id) {
            if matches!(obj, RoomObject::Mod(_)) {
                return Err(format!(
                    "对象 {} 是 Mod 对象：位置变更须走 update_mod_fields（payload 字段与信封坐标同步）",
                    id.as_str()
                ));
            }
            let from = obj.pos();
            obj.set_pos(to.0, to.1);
            self.index.apply_move(id, from, to);
        }
        Ok(())
    }

    /// 累加疲劳，沿拉拽链传到链首。
    /// 负贡献先抵消本体已有疲劳，剩余部分才传给拉者；对象上只保存非负值。
    /// 来源：上游 engine/src/processor/intents/creeps/_add-fatigue.js。
    pub fn add_fatigue(&mut self, id: ObjId, mut d: i64) {
        // 第一次累加时以对象现有疲劳为起点。
        let mut acc = match self.tick_fatigue.get(&id) {
            Some(v) => *v,
            None => self.creep(id).map(|c| c.fatigue as i64).unwrap_or(0),
        };
        if acc > 0 && d < 0 {
            let resting = acc.min(-d);
            acc -= resting;
            d += resting;
            self.tick_fatigue.insert(id, acc);
            if let Some(creep) = self.creep_mut(id) {
                creep.fatigue = acc.max(0) as u32;
            }
            if d == 0 {
                return;
            }
        }
        // 链上的拉者不存在时停止向上查找。
        let head = self.movement.head_puller(&self.room, id);
        let mut acc = match self.tick_fatigue.get(&head) {
            Some(v) => *v,
            None => self.creep(head).map(|c| c.fatigue as i64).unwrap_or(0),
        };
        acc += d;
        self.tick_fatigue.insert(head, acc);
        if let Some(creep) = self.creep_mut(head) {
            creep.fatigue = acc.max(0) as u32;
        }
    }

    /// 移动槽整链开始前的执行者请求登记快照：请求条目与意图时疲劳。
    /// 正常拒绝时按快照逐项恢复，只触碰本执行者的登记。
    pub fn snapshot_move_requests(&self, id: ObjId) -> movement::MoveRequestSnapshot {
        let mut snapshot = self.movement.snapshot_of(id);
        snapshot.old_fatigue = self.old_fatigue.get(&id).copied();
        snapshot
    }

    /// 恢复执行者的请求登记：先移除本执行者的现行登记，再按快照的
    /// 原格与格内登记序回插；执行者已不在候选时不回插请求（避免为
    /// 不存在的对象留下登记），仅恢复疲劳记录。
    pub fn restore_move_requests(&mut self, id: ObjId, snapshot: movement::MoveRequestSnapshot) {
        self.movement.remove_requests(id);
        if self.creep(id).is_some() {
            for record in snapshot.requests {
                self.movement.restore_request(id, record);
            }
        }
        match snapshot.old_fatigue {
            Some(fatigue) => {
                self.old_fatigue.insert(id, fatigue);
            }
            None => {
                self.old_fatigue.remove(&id);
            }
        }
    }

    /// 在指定位置放下 energy：先填充存活容器，再合并同资源掉落堆。
    pub fn create_energy(&mut self, x: u8, y: u8, amount: i64) {
        self.create_energy_typed(x, y, amount, 0);
    }

    /// 按旧资源编码放下资源：1 为 power，2 为 G，其余为 energy。
    pub fn create_energy_typed(&mut self, x: u8, y: u8, amount: i64, res_code: u8) {
        let name = match res_code {
            1 => "power",
            2 => kk_core::constants::RESOURCE_GHODIUM,
            _ => "energy",
        };
        self.create_resource(x, y, amount, name);
    }

    /// 放下任意命名资源；非正量不处理。
    /// 同格存活容器先接收，按全部资源合计计算余量；剩余与同格同名堆合并。
    /// 来源：上游 engine/src/processor/intents/_create-energy.js。
    pub fn create_resource(&mut self, x: u8, y: u8, amount: i64, resource: &str) {
        if amount <= 0 {
            return;
        }
        let amount = self.deposit_into_container(x, y, resource, amount);
        if amount <= 0 {
            return;
        }
        let existing = self.index.at(x as usize, y as usize).iter().find_map(|id| {
            match self.room.objects.get(id) {
                Some(RoomObject::Energy(e)) if energy_resource_name(e) == resource => Some(*id),
                _ => None,
            }
        });
        match existing {
            Some(id) => {
                if let Some(RoomObject::Energy(e)) = self.room.objects.get_mut(&id) {
                    // 数量存储为 u32，超出范围时饱和，避免溢出回绕。
                    e.amount = e
                        .amount
                        .saturating_add(u32::try_from(amount).unwrap_or(u32::MAX));
                }
            }
            None => {
                let (res_code, named) = match resource {
                    kk_core::constants::RESOURCE_GHODIUM => (2u8, None),
                    other => (0u8, Some(other.to_string())),
                };
                self.insert_new(RoomObject::Energy(kk_schema::room::EnergyObj {
                    id: 0,
                    x,
                    y,
                    resource_type: res_code,
                    amount: u32::try_from(amount).unwrap_or(u32::MAX),
                    resource: named,
                }));
            }
        }
    }

    /// 放入同格第一个存活容器，返回未放入的数量。
    /// 容器使用 Generic.data 的 JSON store，容量按全部资源之和计算。
    /// 自动承接沿用 _die/资源创建的全局容量口径，读取冻结规则。
    pub(super) fn deposit_into_container(
        &mut self,
        x: u8,
        y: u8,
        resource: &str,
        amount: i64,
    ) -> i64 {
        let container = self
            .index
            .at(x as usize, y as usize)
            .iter()
            .find(|id| {
                self.room.objects.get(id).is_some_and(|o| {
                    matches!(o, RoomObject::Generic(_))
                        && movement::type_name_of(o) == "container"
                        && damage::object_hits(o) > 0
                })
            })
            .copied();
        let Some(cid) = container else {
            return amount;
        };
        let Some(RoomObject::Generic(g)) = self.room.objects.get_mut(&cid) else {
            return amount;
        };
        let mut data: Value = serde_json::from_str(&g.data).unwrap_or_default();
        // 非整数槽位按 0 计入容量，写入槽位时同样按 0 处理。
        let total_before = data
            .pointer("/store")
            .and_then(Value::as_object)
            .map(|s| s.values().map(|v| v.as_i64().unwrap_or(0)).sum::<i64>())
            .unwrap_or(0);
        let headroom = self.plane.stores.container - total_before;
        let taken = amount.min(headroom);
        if taken <= 0 {
            return amount;
        }
        // 缺少 store 时创建空对象；JSON 或 store 类型错误时，保留全部掉落量。
        let Some(obj) = data.as_object_mut() else {
            return amount;
        };
        let store = obj.entry("store".to_string()).or_insert_with(|| json!({}));
        let Some(store) = store.as_object_mut() else {
            return amount;
        };
        let slot = store.entry(resource.to_string()).or_insert(json!(0));
        let v = slot.as_i64().unwrap_or(0);
        *slot = json!(v.saturating_add(taken));
        g.data = data.to_string();
        amount - taken
    }

    /// 在指令处理后仲裁移动冲突，使用此时生效的安全模式主人。
    pub fn check_movement(&mut self) {
        let safe_mode_user: Option<String> = self.controller().and_then(|ctl| {
            if ctl.safe_mode > self.time {
                ctl.owner.clone()
            } else {
                None
            }
        });
        self.movement.check_impl(
            &self.room,
            &self.index,
            &self.old_fatigue,
            safe_mode_user.as_deref(),
        );
    }

    pub fn push_event(&mut self, event: i64, object_id: ObjId, data: Value) {
        self.events.push(json!({
            "event": event,
            "objectId": object_id.as_str(),
            "data": data,
        }));
    }
}
