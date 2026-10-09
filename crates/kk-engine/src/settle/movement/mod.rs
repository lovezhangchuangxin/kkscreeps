//! 仲裁房间内移动冲突，先选目的格胜者，再取消被阻塞的移动链。
//!
//! 请求登记不把 creep 当静态障碍，仲裁才考虑它是否获准让位。
//! 使用坐标排序的矩阵和稳定的竞争者排序，实际位移由 creep tick 完成。
//! 各槽的整房输入投影在 [`inputs`] 子模块（按职责拆分，保持本文件
//! 聚焦仲裁状态机）。

pub mod admission;
pub mod contest;
pub mod move_cost;
pub mod obstacle;
pub mod precheck;
pub mod pull_registration;

mod inputs;
pub(crate) mod static_obstacles;

use std::collections::{BTreeMap, BTreeSet};

use kk_schema::room::{CreepObj, RoomObject};
use kk_world::types::BodyKind;
use kk_world::{ObjId, RoomIndex, RoomState};

/// 冲突评分，依次按优先级、原格被请求数量、被拉、拉者、MOVE 功率/重量
/// 降序比较。优先级来自 JS 竞争策略候选，未设置为 0（与原生默认等价）。
#[derive(Debug, Clone)]
struct Rate {
    object: ObjId,
    origin: (u8, u8),
    priority: i64,
    rate1: i64,
    rate2: i64,
    rate3: i64,
    rate4: f64,
}

/// 本轮移动请求、获准目的格与拉拽关系；不直接改变对象坐标。
/// 持有建筑目录副本：仲裁期的障碍判定要认 Mod 建筑的 obstacle 语义，
/// 而仲裁在仅有房间快照的上下文中执行。
#[derive(Clone, Default, Debug)]
pub struct Movement {
    matrix: BTreeMap<(u8, u8), Vec<RateEntry>>,
    winners: BTreeMap<ObjId, (u8, u8)>,
    /// 仲裁前的完整原始请求矩阵；`rank_winners` 时零拷贝暂存，仅供
    /// 准入槽构造整房请求视野。Working 每房每 tick 重建，天然瞬态。
    original_requests: BTreeMap<(u8, u8), Vec<RateEntry>>,
    pull_of: BTreeMap<ObjId, ObjId>,
    pulled_by: BTreeMap<ObjId, ObjId>,
    /// 获准执行者的准入终值；取消链与执行阶段同源读取，缺项视为
    /// 未获准（fail-closed），不存在二次 native 复核路径。
    admissions: BTreeMap<ObjId, bool>,
    /// 障碍槽追加的阻挡集（单向追加）；取消链与链内 native 障碍
    /// 判定取或，不进入检查点。
    extra_blocks: BTreeSet<ObjId>,
    /// 代价槽的幸存者疲劳覆盖；缺项为原生代价（重量×地形费率）。
    /// 执行阶段同源读取，不进入检查点。
    costs: BTreeMap<ObjId, u32>,
    /// Hosted 预检暂存登记时的拒绝，不按后续对象变异重新推导。
    registration_blocks: BTreeSet<ObjId>,
    /// 静态放行只作用于同一请求；动态占格仍在取消链中即时复核。
    static_allowed: BTreeSet<ObjId>,
    buildings: std::rc::Rc<kk_mod_api::buildings::BuildingCatalog>,
    /// 计划级静态通行快照；登记、仲裁/预演与默认寻路同源。
    /// 墙项只在仲裁消费，静态对象项同时用于登记。
    static_passability: kk_mod_api::builtin::StaticPassability,
}

#[derive(Debug, Clone, Copy)]
struct RateEntry {
    id: ObjId,
    origin: (u8, u8),
}

/// 快照中的一条请求登记：目的格、格内登记序与原格。
#[derive(Debug, Clone, Copy)]
pub struct MoveRequestRecord {
    pub cell: (u8, u8),
    pub index: usize,
    pub origin: (u8, u8),
}

/// 单个执行者的请求登记快照：请求条目列表，外加登记时的意图疲劳。
/// 引擎在移动槽整链开始前保存，正常拒绝时按原序回插，不触碰
/// 其他执行者的请求与拉拽关系。
#[derive(Debug, Clone, Default)]
pub struct MoveRequestSnapshot {
    pub(crate) requests: Vec<MoveRequestRecord>,
    pub(crate) old_fatigue: Option<u32>,
    pub(crate) registration_blocked: bool,
}

/// 按位移符号换算八方向；同格返回无效方向 0。
pub fn direction_from_delta(dx: i32, dy: i32) -> u8 {
    match (dx.signum(), dy.signum()) {
        (0, -1) => 1,  // TOP
        (1, -1) => 2,  // TOP_RIGHT
        (1, 0) => 3,   // RIGHT
        (1, 1) => 4,   // BOTTOM_RIGHT
        (0, 1) => 5,   // BOTTOM
        (-1, 1) => 6,  // BOTTOM_LEFT
        (-1, 0) => 7,  // LEFT
        (-1, -1) => 8, // TOP_LEFT
        _ => 0,
    }
}

impl Movement {
    pub fn new() -> Self {
        Self::default()
    }

    /// 携带建筑目录与静态通行声明构造；仲裁期按它们扩展障碍判定。
    pub fn with_buildings(
        buildings: std::rc::Rc<kk_mod_api::buildings::BuildingCatalog>,
        static_passability: kk_mod_api::builtin::StaticPassability,
    ) -> Self {
        Self {
            buildings,
            static_passability,
            ..Self::default()
        }
    }

    /// 登记移动请求，目标钳在房间内 0..=49；不检查地形、疲劳或身体。
    /// 单执行者单请求：先移除既有登记再入队。纯 native 路径每 tick
    /// 恰一条 move 意图，此处没有既有登记可移除；移动槽内 JS 能力
    /// 先登记时，后到的中心（native 或再次 request）覆盖先前登记。
    pub fn add(&mut self, id: ObjId, origin: (u8, u8), dx: i32, dy: i32) {
        self.remove_requests(id);
        let to = (
            (origin.0 as i32 + dx).clamp(0, 49) as u8,
            (origin.1 as i32 + dy).clamp(0, 49) as u8,
        );
        self.matrix
            .entry(to)
            .or_default()
            .push(RateEntry { id, origin });
    }

    /// 移动槽拒绝恢复快照：执行者的请求登记与登记时的意图疲劳。
    /// 请求记录目的格、格内登记序与原格，恢复时按原序回插。
    pub fn snapshot_of(&self, id: ObjId) -> MoveRequestSnapshot {
        let mut requests = Vec::new();
        for (cell, entries) in &self.matrix {
            for (index, entry) in entries.iter().enumerate() {
                if entry.id == id {
                    requests.push(MoveRequestRecord {
                        cell: *cell,
                        index,
                        origin: entry.origin,
                    });
                }
            }
        }
        MoveRequestSnapshot {
            requests,
            old_fatigue: None,
            registration_blocked: self.registration_blocks.contains(&id),
        }
    }

    /// 移除 id 的全部请求登记；清空的格移除键，保持矩阵形状与
    /// is_tile_busy 的语义。不触碰其他执行者的请求与拉拽关系。
    pub fn remove_requests(&mut self, id: ObjId) {
        self.registration_blocks.remove(&id);
        self.static_allowed.remove(&id);
        for entries in self.matrix.values_mut() {
            entries.retain(|entry| entry.id != id);
        }
        self.matrix.retain(|_, entries| !entries.is_empty());
    }

    /// 按快照回插一条请求；登记序超出现长竞争者数时钳到末尾。
    pub fn restore_request(&mut self, id: ObjId, record: MoveRequestRecord) {
        let entries = self.matrix.entry(record.cell).or_default();
        let index = record.index.min(entries.len());
        entries.insert(
            index,
            RateEntry {
                id,
                origin: record.origin,
            },
        );
    }

    /// 读取 id 当前登记的请求目的格；单请求不变量下至多一个。
    pub fn request_cell_of(&self, id: ObjId) -> Option<(u8, u8)> {
        self.matrix
            .iter()
            .find(|(_, entries)| entries.iter().any(|entry| entry.id == id))
            .map(|(cell, _)| *cell)
    }

    pub fn is_tile_busy(&self, x: u8, y: u8) -> bool {
        self.matrix.contains_key(&(x, y))
    }

    pub fn is_pulled(&self, id: ObjId) -> bool {
        self.pulled_by.contains_key(&id)
    }

    /// 沿被拉关系找链首，拉者已从房间移除时断链。
    pub fn head_puller(&self, room: &RoomState, id: ObjId) -> ObjId {
        let mut cur = id;
        while let Some(&puller) = self.pulled_by.get(&cur)
            && room.objects.contains_key(&puller)
        {
            cur = puller;
        }
        cur
    }

    /// 拒绝自拉及循环链，再登记双向拉拽关系。重复登记后到覆盖先到：
    /// 拉者换目标时清除旧目标的被拉标记，目标换拉者时清除旧拉者的
    /// 拉动标记，两表始终保持双射。否则首个失败配对的复核清理会连带
    /// 清除当前配对（结果依赖胜者遍历序），`validate_pull_pair` 的
    /// 复核因此与遍历顺序结构无关。
    pub fn add_pulling(&mut self, room: &RoomState, puller: ObjId, target: ObjId) {
        let mut t = target;
        loop {
            if t == puller {
                return;
            }
            match self.pull_of.get(&t) {
                Some(&next) if room.objects.contains_key(&next) => t = next,
                _ => break,
            }
        }
        if let Some(&old_target) = self.pull_of.get(&puller)
            && self.pulled_by.get(&old_target) == Some(&puller)
        {
            self.pulled_by.remove(&old_target);
        }
        if let Some(&old_puller) = self.pulled_by.get(&target)
            && self.pull_of.get(&old_puller) == Some(&target)
        {
            self.pull_of.remove(&old_puller);
        }
        self.pull_of.insert(puller, target);
        self.pulled_by.insert(target, puller);
    }

    /// 读取获准的目标格，不消费记录；名字中的 take 不表示移除。
    pub fn take_move(&self, id: ObjId) -> Option<(u8, u8)> {
        self.winners.get(&id).copied()
    }

    /// 幸存者的疲劳代价覆盖；缺项 None 表示原生代价。
    pub fn cost_of(&self, id: ObjId) -> Option<u32> {
        self.costs.get(&id).copied()
    }

    pub fn debug_matrix(&self) -> Vec<((u8, u8), Vec<ObjId>)> {
        self.matrix
            .iter()
            .map(|(k, v)| (*k, v.iter().map(|e| e.id).collect()))
            .collect()
    }

    /// 是否存在多竞争者目的格；竞争策略槽据此决定是否进入 JS。
    pub fn has_contested_cells(&self) -> bool {
        self.matrix.values().any(|entries| entries.len() > 1)
    }

    /// 先逐目标格选胜者，再沿阻塞链取消移动。
    /// 来源：上游 engine/src/processor/intents/movement.js 的 check。
    /// priorities 是 JS 竞争策略的终值候选（缺项为 0），作为排序首键；
    /// 平手仍回落默认四键与登记序。原生整链组合；宿主准入/障碍槽
    /// 路径由编排方分阶段调用 rank_winners / prefill_admissions /
    /// admission_input / apply_admissions / obstacle_input /
    /// apply_extra_blocks / cancel_blocked。
    pub fn check_impl(
        &mut self,
        room: &RoomState,
        index: &RoomIndex,
        old_fatigue: &BTreeMap<ObjId, u32>,
        safe_mode_user: Option<&str>,
        priorities: &BTreeMap<ObjId, i64>,
        gate: &kk_mod_api::builtin::MoveGate,
    ) {
        self.rank_winners(room, priorities);
        self.prefill_admissions(room, old_fatigue, gate);
        self.cancel_blocked(room, index, safe_mode_user);
    }

    /// 仲裁阶段一：逐目标格选胜者，写出缩减矩阵与胜者表；原始矩阵
    /// 零拷贝暂存，供准入槽的整房请求视野读取。
    pub(crate) fn rank_winners(&mut self, room: &RoomState, priorities: &BTreeMap<ObjId, i64>) {
        let mut new_matrix: BTreeMap<(u8, u8), Vec<RateEntry>> = BTreeMap::new();
        let mut winners = BTreeMap::new();
        // 评分必须读取完整的原始请求矩阵，不能边选胜者边删除竞争信息。
        let original = std::mem::take(&mut self.matrix);
        for (&cell, entries) in &original {
            let winner = if entries.len() > 1 {
                let mut rates: Vec<Rate> = entries
                    .iter()
                    .map(|e| {
                        let mut rate =
                            rate_of(&original, room, &self.pulled_by, &self.pull_of, *e, cell);
                        rate.priority = priorities.get(&e.id).copied().unwrap_or(0);
                        rate
                    })
                    .collect();
                // 优先级与四项降序，完全平手保持登记顺序；不可改成不稳定排序。
                rates.sort_by(|a, b| {
                    b.priority
                        .cmp(&a.priority)
                        .then(b.rate1.cmp(&a.rate1))
                        .then(b.rate2.cmp(&a.rate2))
                        .then(b.rate3.cmp(&a.rate3))
                        .then(
                            b.rate4
                                .partial_cmp(&a.rate4)
                                .expect("rate4 有限（weight≥1）"),
                        )
                });
                RateEntry {
                    id: rates[0].object,
                    origin: rates[0].origin,
                }
            } else {
                entries[0]
            };
            winners.insert(winner.id, cell);
            new_matrix.insert(cell, vec![winner]);
        }
        self.matrix = new_matrix;
        self.winners = winners;
        self.original_requests = original;
    }

    /// 仲裁阶段二：对全部胜者复核拉拽配对，按原生 can_move 语义预填
    /// 准入终值。准入槽无贡献或正常拒绝时这份原生预填就是终值。
    /// 输入构造（含字符串分配）只由准入槽的 Hosted 路径按需执行
    /// （`admission_input`），原生整链不为其付费。
    ///
    /// 配对复核的集合与旧 `remove_from_matrix` 取消时点等价（每个
    /// 胜者恰复核一次），遍历顺序为胜者对象 ID 升序（旧为目的格访问
    /// 序）。`add_pulling` 保持两表双射后，每个失败配对的清理只触及
    /// 自身两侧，复核结果与遍历顺序结构无关（重复登记的旧残标在
    /// 登记时即被清除，不会到达复核）。
    pub(crate) fn prefill_admissions(
        &mut self,
        room: &RoomState,
        old_fatigue: &BTreeMap<ObjId, u32>,
        gate: &kk_mod_api::builtin::MoveGate,
    ) {
        self.admissions.clear();
        // 拆分字段借用：胜者表只读迭代，配对残标与终值表可变写入，
        // 避免为绕过整表可变借用而复制胜者表。
        let Movement {
            winners,
            pull_of,
            pulled_by,
            admissions,
            ..
        } = self;
        for (&id, cell) in winners {
            let effective_pulled = validate_pull_pair(room, pull_of, pulled_by, id, *cell);
            let fatigue = old_fatigue.get(&id).copied().unwrap_or(0);
            let movable = match room.objects.get(&id) {
                Some(RoomObject::Creep(c)) => can_move(c, fatigue, effective_pulled, gate),
                // powerCreep 无部件与疲劳面，官方 canMove 首支恒真
                // （movement.js:11-14）。
                Some(RoomObject::Generic(g)) => {
                    kk_world::domain::generic::type_name(&g.data).as_deref() == Some("powerCreep")
                }
                _ => false,
            };
            admissions.insert(id, movable);
        }
    }

    /// 只读链预演：按给定准入终值在克隆状态上运行与正式仲裁完全
    /// 相同的取消链（同一 `cancel_blocked` 代码路径，无第二套判定），
    /// 返回仍为胜者的对象集合。仅在准入槽时点可用——障碍槽追加集
    /// 此时必为空（`arbitrate_room` 槽序固定 admission→obstacle，且
    /// 预演能力随准入槽 chain 令牌过期失效，双重保障），预演结果因此
    /// 是最终取消结果的上界（追加只会收紧）。
    ///
    /// 克隆只复制取消链消费的字段（matrix/winners/extra_blocks/
    /// static_allowed/buildings；`cancel_cell` 与 `check_obstacle_at_xy` 不读拉拽表
    /// 与原始请求矩阵），避免为迭代式策略的多次预演放大深拷贝成本。
    pub(crate) fn simulate_survivors(
        &self,
        room: &RoomState,
        index: &RoomIndex,
        safe_mode_user: Option<&str>,
        overrides: &BTreeMap<ObjId, bool>,
    ) -> BTreeSet<ObjId> {
        debug_assert!(self.extra_blocks.is_empty(), "链预演只在障碍槽追加之前可用");
        self.simulate_with_blocks(room, index, safe_mode_user, overrides, &BTreeSet::new())
    }

    /// 障碍槽按当前追加集预演；准入终值已经固定，仍复用同一取消链。
    pub(crate) fn simulate_obstacle_survivors(
        &self,
        room: &RoomState,
        index: &RoomIndex,
        safe_mode_user: Option<&str>,
        blocks: &BTreeSet<ObjId>,
    ) -> BTreeSet<ObjId> {
        self.simulate_with_blocks(room, index, safe_mode_user, &self.admissions, blocks)
    }

    fn simulate_with_blocks(
        &self,
        room: &RoomState,
        index: &RoomIndex,
        safe_mode_user: Option<&str>,
        admissions: &BTreeMap<ObjId, bool>,
        blocks: &BTreeSet<ObjId>,
    ) -> BTreeSet<ObjId> {
        let mut sim = Movement {
            matrix: self.matrix.clone(),
            winners: self.winners.clone(),
            original_requests: BTreeMap::new(),
            pull_of: BTreeMap::new(),
            pulled_by: BTreeMap::new(),
            admissions: admissions.clone(),
            extra_blocks: self.extra_blocks.union(blocks).copied().collect(),
            // 链预演不消费代价覆盖；取消链不读代价表。
            costs: BTreeMap::new(),
            buildings: self.buildings.clone(),
            // 链预演与正式链读同一通行声明；取消链的静态障碍语义一致。
            static_passability: self.static_passability,
            // 取消链不再读取登记期过滤快照。
            registration_blocks: BTreeSet::new(),
            static_allowed: self.static_allowed.clone(),
        };
        sim.cancel_blocked(room, index, safe_mode_user);
        sim.winners.keys().copied().collect()
    }

    /// 准入终值当前表（原生预填或槽终值）。
    pub fn admission_finals(&self) -> &BTreeMap<ObjId, bool> {
        &self.admissions
    }

    /// 仲裁阶段三：准入槽成功终值整体替换原生预填。终值表由候选
    /// 以原生值初始化并经 JS 覆盖产生，此处不再逐项校验；接管
    /// 所有权避免第三次克隆。
    pub fn apply_admissions(&mut self, finals: BTreeMap<ObjId, bool>) {
        self.admissions = finals;
    }

    /// 执行阶段读取准入终值；缺项（未参与仲裁或已取消）一律未获准。
    pub fn admitted(&self, id: ObjId) -> bool {
        self.admissions.get(&id).copied().unwrap_or(false)
    }

    /// 障碍槽成功后的追加集并入取消链；Native/拒绝路径不调用，
    /// 追加集保持为空。
    pub(crate) fn apply_extra_blocks(&mut self, blocks: BTreeSet<ObjId>) {
        // 每房每 tick 恰调用一次；重入路径若出现（G03），整体替换会
        // 静默丢弃先前的追加集，先以断言暴露。
        debug_assert!(self.extra_blocks.is_empty() || blocks.is_empty());
        self.extra_blocks = blocks;
    }

    /// 仲裁阶段四：取消未获准或目的格被占的请求，并递归检查其原格上
    /// 的其他请求。获准与否只读准入终值，不再二次 native 复核。
    pub(crate) fn cancel_blocked(
        &mut self,
        room: &RoomState,
        index: &RoomIndex,
        safe_mode_user: Option<&str>,
    ) {
        let keys: Vec<(u8, u8)> = self.matrix.keys().copied().collect();
        for key in keys {
            self.cancel_cell(room, index, safe_mode_user, key);
        }
    }

    fn cancel_cell(
        &mut self,
        room: &RoomState,
        index: &RoomIndex,
        safe_mode_user: Option<&str>,
        key: (u8, u8),
    ) {
        let Some(entries) = self.matrix.get(&key) else {
            return;
        };
        let object_id = entries[0].id;
        let entry = entries[0];
        let mover_owner = dynamic_owner(room.objects.get(&object_id));
        let movable = self.admitted(object_id);
        // 障碍槽只允许追加阻挡：追加集与链内 native 判定取或；让位
        // 链的增量语义（取消 A 后 A 阻挡其原格请求）保留在 native。
        let obstacle = self.extra_blocks.contains(&object_id)
            || check_obstacle_at_xy(room, index, self, key, &mover_owner, safe_mode_user);
        if !movable || obstacle {
            self.matrix.remove(&key);
            self.winners.remove(&object_id);
            if self.matrix.contains_key(&entry.origin) {
                self.cancel_cell(room, index, safe_mode_user, entry.origin);
            }
        }
    }

    pub fn winners(&self) -> &BTreeMap<ObjId, (u8, u8)> {
        &self.winners
    }
}

/// 用完整请求矩阵评分。原格请求中包含站在目标格的竞争者时，对穿评分为 100；
/// MOVE 功率包含 boost，重量至少取 1 以避免除零。rate1/2/3 与对象类型无关；
/// rate4 仅 creep 有身体面——powerCreep 官方 moves=0/weight=1 恒为 0
/// （movement.js:117-121），移动平局对任何带 MOVE 的 creep 落败。
fn rate_of(
    original: &BTreeMap<(u8, u8), Vec<RateEntry>>,
    room: &RoomState,
    pulled_by: &BTreeMap<ObjId, ObjId>,
    pull_of: &BTreeMap<ObjId, ObjId>,
    entry: RateEntry,
    cell: (u8, u8),
) -> Rate {
    let mut rate1 = original
        .get(&entry.origin)
        .map(|v| v.len() as i64)
        .unwrap_or(0);
    if original
        .get(&entry.origin)
        .is_some_and(|movers| movers.iter().any(|m| movers_at(m, cell)))
    {
        rate1 = 100;
    }
    let rate2 = i64::from(pulled_by.contains_key(&entry.id));
    let rate3 = i64::from(pull_of.contains_key(&entry.id));
    let rate4 = match room.objects.get(&entry.id) {
        Some(RoomObject::Creep(creep)) => {
            let moves = kk_world::types::calc_body_effectiveness(
                &creep.body,
                BodyKind::Move,
                "move",
                "fatigue",
                1,
            ) as f64;
            let weight = move_weight(creep).max(1) as f64;
            moves / weight
        }
        _ => 0.0,
    };
    Rate {
        object: entry.id,
        origin: entry.origin,
        priority: 0,
        rate1,
        rate2,
        rate3,
        rate4,
    }
}

/// 动态占格单位（creep/powerCreep）的主人名；非单位或无主返回空串。
fn dynamic_owner(obj: Option<&RoomObject>) -> String {
    match obj {
        Some(RoomObject::Creep(c)) => c.owner.clone(),
        Some(RoomObject::Generic(g)) => serde_json::from_str::<serde_json::Value>(&g.data)
            .ok()
            .and_then(|v| v.get("owner").and_then(|v| v.as_str()).map(str::to_string))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn movers_at(m: &RateEntry, cell: (u8, u8)) -> bool {
    m.origin == cell
}

/// 被拉对象免除疲劳和 MOVE 检查；其他对象按移动玩家门规则判定：
/// 意图时疲劳不超过 `fatigue_limit` 且存活 MOVE 数不少于
/// `min_active_moves`。默认值即官方行为（疲劳须为 0、至少 1 个存活
/// MOVE），与玩家面 `move()`/`moveTo()` 预检同源（同一 MoveGate 快照）。
pub fn can_move(
    creep: &CreepObj,
    old_fatigue: u32,
    is_pulled: bool,
    gate: &kk_mod_api::builtin::MoveGate,
) -> bool {
    is_pulled
        || (i64::from(old_fatigue) <= gate.fatigue_limit
            && BodyKind::count_active(&creep.body, BodyKind::Move) as i64 >= gate.min_active_moves)
}

/// 按全资源携带量，从身体尾部计算需要的存活 CARRY 数，包含容量 boost。
pub fn resources_weight(creep: &CreepObj) -> usize {
    kk_world::types::resources_weight(&creep.body, &creep.store, &creep.store_extra)
}

/// 非 MOVE/CARRY 的部件数（包括损毁件），加上实际负载占用的 CARRY 数。
pub fn move_weight(creep: &CreepObj) -> usize {
    creep
        .body
        .iter()
        .filter(|p| p.kind != BodyKind::Move.code() && p.kind != BodyKind::Carry.code())
        .count()
        + resources_weight(creep)
}

/// 检查仲裁阶段的障碍。获准移走的 creep 不阻挡；房主安全模式下可穿过
/// 敌方 creep，但仍被己方未移动的 creep 阻挡。自家障碍工地、私有敌方
/// rampart 和无路的墙地形也阻挡；墙项遵循计划级通行声明
/// （`terrain.wallPassable`，见 static_obstacles）。
pub fn check_obstacle_at_xy(
    room: &RoomState,
    index: &RoomIndex,
    movement: &Movement,
    at: (u8, u8),
    mover_owner: &str,
    safe_mode_user: Option<&str>,
) -> bool {
    let dynamic_blocked = index.at(at.0 as usize, at.1 as usize).iter().any(|id| {
        let Some(target) = room.objects.get(id) else {
            return false;
        };
        // 未获准移动的 creep/powerCreep 占格阻挡（官方 checkObstacleAtXY 首支
        // 同列两种类型）；其他类型走静态障碍门。
        let is_unit = match target {
            RoomObject::Creep(_) => true,
            RoomObject::Generic(g) => {
                kk_world::domain::generic::type_name(&g.data).as_deref() == Some("powerCreep")
            }
            _ => false,
        };
        if !is_unit || movement.winners.contains_key(id) {
            return false;
        }
        let target_owner = dynamic_owner(Some(target));
        match safe_mode_user {
            None => true,
            Some(sm) => sm != mover_owner || target_owner == mover_owner,
        }
    });
    // 仲裁与障碍输入均在选胜后调用：缩减矩阵每格只保留该胜者，
    // 静态放行绑定请求者，不能借同格落选者的许可放行胜者。
    let allowed = movement
        .matrix
        .get(&at)
        .and_then(|entries| entries.first())
        .is_some_and(|entry| movement.static_allowed.contains(&entry.id));
    dynamic_blocked
        || (!allowed
            && static_obstacles::arbitration_blocked(
                room,
                index,
                &movement.buildings,
                movement.static_passability,
                at,
                mover_owner,
            ))
}

/// 读取游戏对象类型名；Generic 结构使用 structureType，非结构对象使用 type。
/// JSON 无效或字段缺失时返回空串，公开性和所有权由调用方另查。
pub fn type_name_of(obj: &RoomObject) -> std::borrow::Cow<'static, str> {
    match obj {
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            std::borrow::Cow::Owned(
                data.get("structureType")
                    .or_else(|| data.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        }
        RoomObject::Creep(_) => "creep".into(),
        RoomObject::Source(_) => "source".into(),
        RoomObject::Controller(_) => "controller".into(),
        RoomObject::Spawn(_) => "spawn".into(),
        RoomObject::Extension(_) => "extension".into(),
        RoomObject::ConstructedWall(_) => "constructedWall".into(),
        RoomObject::ConstructionSite(_) => "constructionSite".into(),
        RoomObject::Energy(_) => "energy".into(),
        RoomObject::Flag(_) => "flag".into(),
        RoomObject::Road(_) => "road".into(),
        RoomObject::Storage(_) => "storage".into(),
        RoomObject::Tombstone(_) => "tombstone".into(),
        RoomObject::Ruin(_) => "ruin".into(),
        RoomObject::Mineral(_) => "mineral".into(),
        RoomObject::Extractor(_) => "extractor".into(),
        // Mod 对象不参与既有结构规则（阻挡/寻路/耐久尚未声明能力面），
        // 类型名空串使其与未识别结构同样对待。
        RoomObject::Mod(_) => "".into(),
    }
}

pub fn has_road(room: &RoomState, index: &RoomIndex, at: (u8, u8)) -> bool {
    index
        .at(at.0 as usize, at.1 as usize)
        .iter()
        .any(|id| matches!(room.objects.get(id), Some(RoomObject::Road(_))))
}

pub fn offsets_by_direction(direction: u8) -> Option<(i32, i32)> {
    Some(match direction {
        1 => (0, -1),
        2 => (1, -1),
        3 => (1, 0),
        4 => (1, 1),
        5 => (0, 1),
        6 => (-1, 1),
        7 => (-1, 0),
        8 => (-1, -1),
        _ => return None,
    })
}

/// 复核单个胜者的拉拽配对（判定与旧 remove_from_matrix 逐行等价）：
/// 拉者须仍指向被拉者且被拉者获准移向拉者当前格，否则解除双向
/// 残标；拉者已从房间移除时跳过复核并保留现有标记。返回有效被拉
/// 标记。独立自由函数以支持 prefill 的拆分字段借用；遍历顺序语义
/// 见 prefill_admissions 注释。
fn validate_pull_pair(
    room: &RoomState,
    pull_of: &mut BTreeMap<ObjId, ObjId>,
    pulled_by: &mut BTreeMap<ObjId, ObjId>,
    id: ObjId,
    cell: (u8, u8),
) -> bool {
    let Some(&puller) = pulled_by.get(&id) else {
        return false;
    };
    let Some(puller_obj) = room.objects.get(&puller) else {
        return true;
    };
    let pair_intact = pull_of.get(&puller) == Some(&id);
    let dest_is_puller_pos = cell == puller_obj.pos();
    if !pair_intact || !dest_is_puller_pos {
        pull_of.remove(&puller);
        pulled_by.remove(&id);
        return false;
    }
    true
}
