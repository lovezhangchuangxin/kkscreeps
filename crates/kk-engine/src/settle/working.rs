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
    /// 单目标战斗动作射程规则；attack/rangedAttack/heal/rangedHeal
    /// 的结算距离判定与 runtime 玩家常量注入同源读取。
    pub combat: kk_mod_api::builtin::CombatRanges,
    /// 移动玩家门规则；仲裁原生准入预填（can_move）与玩家面
    /// move()/moveTo() 预检常量注入同源读取。
    pub move_gate: kk_mod_api::builtin::MoveGate,
    /// 地形静态通行规则；仲裁静态障碍判定的墙项与玩家面原生寻路
    /// 地形同源读取。
    pub terrain_passability: kk_mod_api::builtin::TerrainPassability,
    /// 内建结构 RCL 配额；结算配额判定与玩家面常量注入同源读取。
    pub quotas: Rc<kk_mod_api::builtin::StructureQuotaSnapshot>,
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
    pub(crate) old_fatigue: BTreeMap<ObjId, u32>,
    /// 本轮疲劳累加值；保留负值用于拉拽传递，写回对象时取 max(0, 值)。
    pub(crate) tick_fatigue: BTreeMap<ObjId, i64>,
    /// 本轮各控制器的累计升级量，用于限制 RCL 8 的升级。
    pub(crate) upgraded: BTreeMap<ObjId, i64>,
    /// 指令阶段累计的 creep 伤害，在该 creep 的 tick 阶段施加。
    pub(crate) damage_to_apply: BTreeMap<ObjId, i64>,
    /// 待施加的治疗量，与伤害一起在 creep 死亡判断前处理。
    pub(crate) heal_to_apply: BTreeMap<ObjId, i64>,
    /// attackController 设置的升级封锁到期 tick，由 controller tick 写回对象。
    pub(crate) upgrade_block_pending: BTreeMap<ObjId, u64>,
    /// 本轮激活安全模式的控制器，controller tick 据此处理时钟。
    pub(crate) safe_mode_activated: BTreeSet<ObjId>,
    /// 采矿时设置的提取器冷却，由 extractor tick 写回冷却计数。
    pub(crate) extractor_cooldown_pending: BTreeMap<ObjId, u64>,
    pub(crate) movement: crate::settle::movement::Movement,
    /// 本 tick 登记的跨房过渡：位于边缘格的 creep 在全部房间结算后由
    /// 引擎阶段移入邻房候选。
    pub(crate) pending_inter_room: Vec<inter_room::InterRoomMove>,
    /// 链内对象变换日志；在自有对象及战斗/建造族 Hosted 链
    /// 期间武装，正常拒绝时逆序撤销。链故障（Err）路径不解除武装——
    /// 它依赖"链 Err 使整轮候选丢弃、本字段随 Working 一起 drop"兜底；
    /// 若未来引入链失败但房间候选保留的路径，须先在此处补解除。
    /// Native 直调路径保持 None 零开销。
    pub(crate) chain_mutations: Option<Vec<ChainMutation>>,
    /// 本轮生效的贡献平面：link/仓库规则、槽执行表与扩展字段目录。
    pub plane: Plane,
}

/// 共享的房间候选句柄。进入 JS 或执行 Rust next 前必须释放可变借用；
/// 该纪律由 RefCell 动态检查，类型不会自动证明。宿主在调用期间可以
/// 放入 OpState，但返回前必须清空；处理器不得跨调用留存候选句柄。
pub type RoomCtx = Rc<RefCell<Working>>;

mod extensions;
mod journal;
mod objects;
pub use journal::ObjectJournal;
mod chain_snapshot;
mod drops;
mod movement;
pub use chain_snapshot::{ChainMutation, ChainSnapshot, RemovedObject};

impl Working {
    /// 从已提交房间建立单轮候选；索引与领域瞬态只在此初始化。
    /// state 与 plane 必须属于调用方当前轮次和冻结计划。
    pub fn new(room: RoomState, time: u64, now_ms: u64, state: TickState, plane: Plane) -> Self {
        Self {
            index: RoomIndex::build(&room.objects),
            room,
            time,
            now_ms,
            state,
            events: Vec::new(),
            old_fatigue: BTreeMap::new(),
            tick_fatigue: BTreeMap::new(),
            upgraded: BTreeMap::new(),
            damage_to_apply: BTreeMap::new(),
            heal_to_apply: BTreeMap::new(),
            upgrade_block_pending: BTreeMap::new(),
            safe_mode_activated: BTreeSet::new(),
            extractor_cooldown_pending: BTreeMap::new(),
            movement: crate::settle::movement::Movement::with_buildings(
                plane.buildings.clone(),
                plane.terrain_passability.wall_passable,
            ),
            pending_inter_room: Vec::new(),
            chain_mutations: None,
            plane,
        }
    }

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

    pub fn push_event(&mut self, event: i64, object_id: ObjId, data: Value) {
        self.events.push(json!({
            "event": event,
            "objectId": object_id.as_str(),
            "data": data,
        }));
    }
}
