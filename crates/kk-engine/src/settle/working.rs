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
    /// 资源目录：内建 RESOURCES_ALL 加计划声明的命名空间资源。
    /// 搬运/掉落/超容回收与领域能力按它校验；市场/跨房发送不读它。
    pub resources: Rc<kk_mod_api::resources::ResourceCatalog>,
    /// 建筑目录：计划声明的可建造结构配方。工地创建、建造消耗、耐久、
    /// 寻路语义与生产结算按它执行；内建结构仍走官方常量表。
    pub buildings: Rc<kk_mod_api::buildings::BuildingCatalog>,
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
    /// 链内对象变换日志；在自有对象及战斗/建造族 Hosted 链
    /// 期间武装，正常拒绝时逆序撤销。链故障（Err）路径不解除武装——
    /// 它依赖"链 Err 使整轮候选丢弃、本字段随 Working 一起 drop"兜底；
    /// 若未来引入链失败但房间候选保留的路径，须先在此处补解除。
    /// Native 直调路径保持 None 零开销。
    pub chain_mutations: Option<Vec<ChainMutation>>,
    /// 本轮生效的贡献平面：link/仓库规则、槽执行表与扩展字段目录。
    pub plane: Plane,
}

/// remove_obj 移除既有对象时保存的全部伴随候选状态；拒绝恢复时整体回插。
#[derive(Debug, Clone)]
pub struct RemovedObject {
    pub id: ObjId,
    pub object: RoomObject,
    pub extensions: Option<BTreeMap<String, Value>>,
    pub old_fatigue: Option<u32>,
    pub tick_fatigue: Option<i64>,
    pub damage: Option<i64>,
    pub heal: Option<i64>,
}

/// 武装期间记录对象增删、Mod 字段首触和索引格首触；对象逆序恢复。
/// Removed 含整对象克隆，装箱避免让每次日志追加都按最大变体扩张。
#[derive(Debug, Clone)]
pub enum ChainMutation {
    /// 格内首触顺序。对象逆序撤销完毕后再整格恢复，防止追加顺序漂移。
    IndexCell((u8, u8), Vec<ObjId>),
    /// 自有对象字段/坐标的首触原值；拒绝时同步恢复位置索引。
    ModUpdated(ObjId, Box<RoomObject>),
    /// insert_new 分配的新对象；恢复时连同索引删除。
    Inserted(ObjId),
    /// remove_obj 移除的既有对象；恢复时对象与伴随状态一起回插。
    Removed(Box<RemovedObject>),
}

/// 整链开始前的候选快照：原生受影响对象克隆、延迟伤害/
/// 治疗条目、事件长度与 ID 游标。链内对象增删由 chain_mutations 记录，
/// 引擎侧在快照时武装。能力触达的链外对象由运行时首触日志恢复。
#[derive(Debug, Clone)]
pub struct ChainSnapshot {
    objects: Vec<(ObjId, RoomObject)>,
    queues: Vec<(ObjId, Option<i64>, Option<i64>)>,
    event_len: usize,
    next_id: u64,
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
        self.remember_index_cell((x, y));
        self.room
            .insert_with_id(id, object)
            .expect("结算 id 分配器不得复用已有对象");
        self.index.insert(ObjId(id), (x, y));
        let id = ObjId(id);
        if let Some(log) = &mut self.chain_mutations {
            log.push(ChainMutation::Inserted(id));
        }
        id
    }

    /// 删除对象及位置索引，同时清理它的疲劳、待伤害、待治疗和扩展字段。
    /// 链日志武装期间记录 Removed；同链先插后删的对象经逆序回放净效果
    /// 仍为删除，无须特判。
    pub fn remove_obj(&mut self, id: ObjId) -> Option<RoomObject> {
        if let Some(object) = self.room.objects.get(&id) {
            self.remember_index_cell(object.pos());
        }
        let journaled = self
            .chain_mutations
            .is_some()
            .then(|| {
                self.room.objects.get(&id).map(|object| RemovedObject {
                    id,
                    object: object.clone(),
                    extensions: self.room.extensions.get(&id).cloned(),
                    old_fatigue: self.old_fatigue.get(&id).copied(),
                    tick_fatigue: self.tick_fatigue.get(&id).copied(),
                    damage: self.damage_to_apply.get(&id).copied(),
                    heal: self.heal_to_apply.get(&id).copied(),
                })
            })
            .flatten();
        let obj = self.room.objects.remove(&id)?;
        let (x, y) = obj.pos();
        self.index.remove(id, (x, y));
        self.room.extensions.remove(&id);
        self.old_fatigue.remove(&id);
        self.tick_fatigue.remove(&id);
        self.damage_to_apply.remove(&id);
        self.heal_to_apply.remove(&id);
        if let (Some(log), Some(bundle)) = (&mut self.chain_mutations, journaled) {
            log.push(ChainMutation::Removed(Box::new(bundle)));
        }
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
        if self.chain_mutations.as_ref().is_some_and(|log| {
            !log.iter()
                .any(|entry| matches!(entry, ChainMutation::ModUpdated(old, _) if *old == id))
        }) {
            self.check_mod_mutation_budget()?;
            let original = self.room.objects.get(&id).expect("已确认 Mod 对象").clone();
            self.chain_mutations
                .as_mut()
                .expect("日志已武装")
                .push(ChainMutation::ModUpdated(id, Box::new(original)));
        }
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
                self.remember_index_cell((x, y));
                self.remember_index_cell((px, py));
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

    /// 战斗槽整链开始前的候选快照并武装链内增删日志。受影响集为
    /// 执行者、请求目标及其格上 rampart（攻击重定向面）；群体攻击
    /// 为切比雪夫 3 格内全部对象的急切全量克隆（命中集超集，无论
    /// 是否拒绝都支付）。
    pub fn snapshot_combat(
        &mut self,
        id: ObjId,
        target: Option<ObjId>,
        mass: bool,
    ) -> ChainSnapshot {
        let mut touched: Vec<ObjId> = vec![id];
        if let Some(target_id) = target {
            // 与能力触达面同源计算"目标+格上 rampart"，两处漂移会造成
            // 恢复覆盖缺口。
            touched.extend(super::creep::combat_touch_targets(self, target_id));
        }
        if mass && let Some(actor) = self.creep(id).map(|c| (c.x, c.y)) {
            for (oid, obj) in self.room.objects.iter() {
                let (ox, oy) = obj.pos();
                if (ox as i32 - actor.0 as i32).abs() <= 3
                    && (oy as i32 - actor.1 as i32).abs() <= 3
                {
                    touched.push(*oid);
                }
            }
        }
        self.snapshot_touched(touched)
    }

    /// 建造族槽（build/repair/dismantle）整链开始前的候选快照并武装
    /// 链内增删日志。受影响集为执行者与请求目标；dismantle 另含目标
    /// 格 rampart（重定向面）与执行者格上的掉落面（同格掉落堆与容器，
    /// 超容掉落只写这些对象）。完工结构插入、击毁废墟与对象移除经
    /// 增删日志逆序撤销，ID 游标随后回退。
    pub fn snapshot_build(
        &mut self,
        id: ObjId,
        target: Option<ObjId>,
        action: super::creep::BuildAction,
    ) -> ChainSnapshot {
        let mut touched: Vec<ObjId> = vec![id];
        if let Some(target_id) = target {
            touched.extend(super::creep::build_touch_targets(self, target_id, action));
        }
        if action == super::creep::BuildAction::Dismantle {
            touched.extend(super::creep::dismantle_drop_face(self, id));
        }
        self.snapshot_touched(touched)
    }

    /// 按受影响集构建快照：逐对象首触克隆与延迟伤害/治疗条目，事件
    /// 长度与 ID 游标随后记录，并武装链内增删日志。已武装时再次进入
    /// 属嵌套链（当前不存在：槽串行执行、链 Err 即弃整轮候选），
    /// 静默重置会丢弃旧日志，以 debug_assert 固化该前提。
    fn snapshot_touched(&mut self, mut touched: Vec<ObjId>) -> ChainSnapshot {
        debug_assert!(
            self.chain_mutations.is_none(),
            "链内增删日志已武装时不得再次进入快照"
        );
        touched.sort_unstable();
        touched.dedup();
        let objects: Vec<_> = touched
            .iter()
            .filter_map(|id| self.room.objects.get(id).cloned().map(|o| (*id, o)))
            .collect();
        let queues: Vec<_> = touched
            .iter()
            .map(|id| {
                (
                    *id,
                    self.damage_to_apply.get(id).copied(),
                    self.heal_to_apply.get(id).copied(),
                )
            })
            .collect();
        let snapshot = ChainSnapshot {
            objects,
            queues,
            event_len: self.events.len(),
            next_id: self.state.peek_next_id(),
        };
        self.chain_mutations = Some(Vec::new());
        snapshot
    }

    /// 自有对象能力沿同一日志记录增删和字段首触；不预克隆整个房间。
    pub fn snapshot_mod_objects(&mut self) -> ChainSnapshot {
        self.snapshot_touched(Vec::new())
    }

    /// JS 对象能力的日志在 Rust 堆，须独立于 V8 堆限制其条目数。
    pub fn check_mod_mutation_budget(&self) -> Result<(), String> {
        if self.chain_mutations.as_ref().is_some_and(|log| {
            log.iter()
                .filter(|entry| !matches!(entry, ChainMutation::IndexCell(..)))
                .count()
                >= 256
        }) {
            return Err("单槽对象能力撤销日志最多 256 条".into());
        }
        Ok(())
    }

    fn remember_index_cell(&mut self, position: (u8, u8)) {
        if let Some(log) = &mut self.chain_mutations
            && !log
                .iter()
                .any(|entry| matches!(entry, ChainMutation::IndexCell(old, _) if *old == position))
        {
            log.push(ChainMutation::IndexCell(
                position,
                self.index
                    .at(position.0 as usize, position.1 as usize)
                    .to_vec(),
            ));
        }
    }

    /// 处理链正常完成：解除增删日志武装并丢弃记录。
    pub fn finish_chain_snapshot(&mut self) {
        self.chain_mutations = None;
    }

    /// 链正常拒绝：按日志逆序撤销链内对象变换，再恢复
    /// 首触对象、延迟队列条目与事件长度，最后回退 ID 游标。其他执行者
    /// 的队列条目、此前动作的写入与移动登记不受影响。
    /// Removed 按移除时快照回插，其等于链前状态依赖当前链处理器没有
    /// 其他候选写通道（资源/移动能力默认拒绝、creep 字段只读）；给
    /// 这两族槽开放新写能力时须重审此对账前提。
    pub fn restore_chain_snapshot(&mut self, snapshot: ChainSnapshot) {
        let mutations = self
            .chain_mutations
            .take()
            .expect("对象变换链期间日志已武装");
        let mut cells = Vec::new();
        for mutation in mutations.into_iter().rev() {
            match mutation {
                ChainMutation::IndexCell(position, ids) => cells.push((position, ids)),
                ChainMutation::ModUpdated(id, original) => {
                    let position = original.pos();
                    if let Some(current) = self.room.objects.insert(id, *original) {
                        if current.pos() != position {
                            self.index.remove(id, current.pos());
                            self.index.insert(id, position);
                        }
                    } else {
                        self.index.insert(id, position);
                    }
                }
                ChainMutation::Inserted(id) => {
                    if let Some(object) = self.room.objects.remove(&id) {
                        let (x, y) = object.pos();
                        self.index.remove(id, (x, y));
                    }
                    self.room.extensions.remove(&id);
                    self.damage_to_apply.remove(&id);
                    self.heal_to_apply.remove(&id);
                }
                ChainMutation::Removed(bundle) => {
                    let (x, y) = bundle.object.pos();
                    self.room.objects.insert(bundle.id, bundle.object);
                    self.index.insert(bundle.id, (x, y));
                    match bundle.extensions {
                        Some(fields) => {
                            self.room.extensions.insert(bundle.id, fields);
                        }
                        None => {
                            self.room.extensions.remove(&bundle.id);
                        }
                    }
                    match bundle.old_fatigue {
                        Some(v) => {
                            self.old_fatigue.insert(bundle.id, v);
                        }
                        None => {
                            self.old_fatigue.remove(&bundle.id);
                        }
                    }
                    match bundle.tick_fatigue {
                        Some(v) => {
                            self.tick_fatigue.insert(bundle.id, v);
                        }
                        None => {
                            self.tick_fatigue.remove(&bundle.id);
                        }
                    }
                    match bundle.damage {
                        Some(v) => {
                            self.damage_to_apply.insert(bundle.id, v);
                        }
                        None => {
                            self.damage_to_apply.remove(&bundle.id);
                        }
                    }
                    match bundle.heal {
                        Some(v) => {
                            self.heal_to_apply.insert(bundle.id, v);
                        }
                        None => {
                            self.heal_to_apply.remove(&bundle.id);
                        }
                    }
                }
            }
        }
        for (id, object) in snapshot.objects {
            let position = object.pos();
            if let Some(current) = self.room.objects.insert(id, object) {
                if current.pos() != position {
                    self.index.remove(id, current.pos());
                    self.index.insert(id, position);
                }
            } else {
                self.index.insert(id, position);
            }
        }
        for (id, damage, heal) in snapshot.queues {
            match damage {
                Some(v) => {
                    self.damage_to_apply.insert(id, v);
                }
                None => {
                    self.damage_to_apply.remove(&id);
                }
            }
            match heal {
                Some(v) => {
                    self.heal_to_apply.insert(id, v);
                }
                None => {
                    self.heal_to_apply.remove(&id);
                }
            }
        }
        self.events.truncate(snapshot.event_len);
        for (position, ids) in cells {
            self.index.restore_cell(position, ids);
        }
        self.state.restore_next_id(snapshot.next_id);
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
    /// priorities 是 JS 竞争策略的终值候选，空表等同原生默认。
    pub fn rank_movement(&mut self, priorities: &BTreeMap<ObjId, i64>) {
        self.movement.rank_winners(&self.room, priorities);
    }

    /// 仲裁阶段二：复核胜者拉拽配对并预填原生准入终值（取消链消费）；
    /// Native 与 Hosted 准入路径都必须先执行。
    pub fn prefill_movement_admissions(&mut self) {
        self.movement
            .prefill_admissions(&self.room, &self.old_fatigue);
    }

    /// 准入槽输入构造；仅 Hosted 路径调用，原生路径不为其分配。
    pub fn movement_admission_input(&self) -> kk_mod_api::admission::MoveAdmissionInput {
        self.movement.admission_input(&self.room, &self.old_fatigue)
    }

    /// 原生准入终值表的拷贝；准入槽构造候选初值用。
    pub fn movement_admission_finals(&self) -> BTreeMap<ObjId, bool> {
        self.movement.admission_finals().clone()
    }

    /// 仲裁阶段三：准入槽成功终值整体替换原生预填。
    pub fn apply_movement_admissions(&mut self, finals: BTreeMap<ObjId, bool>) {
        self.movement.apply_admissions(finals);
    }

    /// 预检槽输入构造；全部意图登记完成后、竞争输入构造之前调用。
    /// 仅 Hosted 路径调用，原生路径不为其分配。
    pub fn movement_precheck_input(&self) -> kk_mod_api::precheck::MovePrecheckInput {
        self.movement.precheck_input(&self.room, &self.index)
    }

    /// 预检槽成功后的否决集应用；被否决请求从登记矩阵移除。
    pub fn apply_movement_precheck_blocks(&mut self, blocks: &std::collections::BTreeSet<ObjId>) {
        self.movement.apply_precheck_blocks(blocks);
    }

    /// 此刻生效的安全模式主人：控制器处于安全模式时取房主，否则
    /// None。障碍槽输入、链预演与正式取消链共用，规则变更只改此处。
    fn active_safe_mode_user(&self) -> Option<String> {
        self.controller().and_then(|ctl| {
            if ctl.safe_mode > self.time {
                ctl.owner.clone()
            } else {
                None
            }
        })
    }

    /// 障碍槽输入构造；使用与 finish_movement 同时点生效的安全模式
    /// 主人。仅 Hosted 路径调用。
    pub fn movement_obstacle_input(&self) -> kk_mod_api::obstacle::MoveObstacleInput {
        let safe_mode_user = self.active_safe_mode_user();
        self.movement
            .obstacle_input(&self.room, &self.index, safe_mode_user.as_deref())
    }

    /// 障碍槽成功后的追加集并入取消链。
    pub fn apply_movement_obstacle_blocks(&mut self, blocks: BTreeSet<ObjId>) {
        self.movement.apply_extra_blocks(blocks);
    }

    /// 准入槽的只读链预演：按给定准入终值在克隆状态上运行与
    /// `finish_movement` 完全相同的取消链，返回仍为胜者的对象集合。
    /// 安全模式主人与正式仲裁同源；只在准入槽时点调用（障碍追加集
    /// 尚为空，预演结果是最终取消结果的上界）。
    pub fn movement_simulate_survivors(
        &self,
        overrides: &BTreeMap<ObjId, bool>,
    ) -> BTreeSet<ObjId> {
        let safe_mode_user = self.active_safe_mode_user();
        self.movement.simulate_survivors(
            &self.room,
            &self.index,
            safe_mode_user.as_deref(),
            overrides,
        )
    }

    /// 仲裁阶段四：按准入终值取消阻塞链，使用此时生效的安全模式主人。
    pub fn finish_movement(&mut self) {
        let safe_mode_user = self.active_safe_mode_user();
        self.movement
            .cancel_blocked(&self.room, &self.index, safe_mode_user.as_deref());
    }

    pub fn push_event(&mut self, event: i64, object_id: ObjId, data: Value) {
        self.events.push(json!({
            "event": event,
            "objectId": object_id.as_str(),
            "data": data,
        }));
    }
}
