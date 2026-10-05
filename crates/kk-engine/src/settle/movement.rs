//! 仲裁房间内移动冲突，先选目的格胜者，再取消被阻塞的移动链。
//!
//! 请求登记不把 creep 当静态障碍，仲裁才考虑它是否获准让位。
//! 使用坐标排序的矩阵和稳定的竞争者排序，实际位移由 creep tick 完成。

use std::collections::BTreeMap;

use kk_core::TERRAIN_MASK_WALL;
use kk_schema::room::{CreepObj, RoomObject};
use kk_world::types::{BodyKind, is_obstacle_type};
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
    pull_of: BTreeMap<ObjId, ObjId>,
    pulled_by: BTreeMap<ObjId, ObjId>,
    buildings: std::rc::Rc<kk_mod_api::buildings::BuildingCatalog>,
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

    /// 携带建筑目录构造；仲裁期按它扩展障碍判定。
    pub fn with_buildings(buildings: std::rc::Rc<kk_mod_api::buildings::BuildingCatalog>) -> Self {
        Self {
            buildings,
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
        }
    }

    /// 移除 id 的全部请求登记；清空的格移除键，保持矩阵形状与
    /// is_tile_busy 的语义。不触碰其他执行者的请求与拉拽关系。
    pub fn remove_requests(&mut self, id: ObjId) {
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

    /// 拒绝自拉及循环链，再登记双向拉拽关系；配对一致性在仲裁时复核。
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
        self.pull_of.insert(puller, target);
        self.pulled_by.insert(target, puller);
    }

    /// 读取获准的目标格，不消费记录；名字中的 take 不表示移除。
    pub fn take_move(&self, id: ObjId) -> Option<(u8, u8)> {
        self.winners.get(&id).copied()
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

    /// 构造房间移动竞争输入：全部有竞争者的目的格及按登记序排列的
    /// 竞争者摘要，含默认四键评分要素；不截断房间请求。调用时点为
    /// 登记完成之后、仲裁开始之前，矩阵仍是完整原始形态。
    pub fn contest_input(&self, room: &RoomState) -> kk_mod_api::contest::MoveContestInput {
        let mut contests = Vec::new();
        for (&cell, entries) in &self.matrix {
            if entries.len() < 2 {
                continue;
            }
            let contenders = entries
                .iter()
                .map(|entry| self.contender_info(room, *entry, cell))
                .collect();
            contests.push(kk_mod_api::contest::ContestCell {
                x: cell.0,
                y: cell.1,
                contenders,
            });
        }
        kk_mod_api::contest::MoveContestInput { contests }
    }

    /// 单个竞争者摘要；评分要素与 [`rate_of`] 同源，对穿标志由原始
    /// 矩阵推导。非 creep 登记者按防御值填充（默认评分为全 0）。
    fn contender_info(
        &self,
        room: &RoomState,
        entry: RateEntry,
        cell: (u8, u8),
    ) -> kk_mod_api::contest::ContenderInfo {
        let mut info = kk_mod_api::contest::ContenderInfo {
            id: entry.id.as_str(),
            owner: String::new(),
            origin_x: entry.origin.0,
            origin_y: entry.origin.1,
            origin_request_count: self
                .matrix
                .get(&entry.origin)
                .map(|v| v.len() as i64)
                .unwrap_or(0),
            reciprocal: false,
            pulled: self.pulled_by.contains_key(&entry.id),
            pulling: self.pull_of.contains_key(&entry.id),
            move_power: 0,
            weight: 1,
            power_ratio: 0.0,
        };
        info.reciprocal = self
            .matrix
            .get(&entry.origin)
            .is_some_and(|movers| movers.iter().any(|m| movers_at(m, cell)));
        if let Some(RoomObject::Creep(creep)) = room.objects.get(&entry.id) {
            info.owner = creep.owner.clone();
            info.move_power = kk_world::types::calc_body_effectiveness(
                &creep.body,
                BodyKind::Move,
                "move",
                "fatigue",
                1,
            );
            info.weight = move_weight(creep).max(1) as i64;
            info.power_ratio = info.move_power as f64 / info.weight as f64;
        }
        info
    }

    /// 先逐目标格选胜者，再沿阻塞链取消移动。
    /// 来源：上游 engine/src/processor/intents/movement.js 的 check。
    /// priorities 是 JS 竞争策略的终值候选（缺项为 0），作为排序首键；
    /// 平手仍回落默认四键与登记序。
    pub fn check_impl(
        &mut self,
        room: &RoomState,
        index: &RoomIndex,
        old_fatigue: &BTreeMap<ObjId, u32>,
        safe_mode_user: Option<&str>,
        priorities: &BTreeMap<ObjId, i64>,
    ) {
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

        let keys: Vec<(u8, u8)> = self.matrix.keys().copied().collect();
        for key in keys {
            self.remove_from_matrix(room, index, old_fatigue, safe_mode_user, key);
        }
    }

    pub fn winners(&self) -> &BTreeMap<ObjId, (u8, u8)> {
        &self.winners
    }

    /// 取消不能移动或目的格被占的请求，并递归检查其原格上的其他请求。
    fn remove_from_matrix(
        &mut self,
        room: &RoomState,
        index: &RoomIndex,
        old_fatigue: &BTreeMap<ObjId, u32>,
        safe_mode_user: Option<&str>,
        key: (u8, u8),
    ) {
        let Some(entries) = self.matrix.get(&key) else {
            return;
        };
        let object_id = entries[0].id;
        let entry = entries[0];
        // 拉者须仍指向被拉者，被拉者须移动到拉者的当前格，否则解除配对。
        // 拉者已移除时跳过这项复核，保留现有标记。
        if let Some(&puller) = self.pulled_by.get(&object_id)
            && let Some(puller_obj) = room.objects.get(&puller)
        {
            let pair_intact = self.pull_of.get(&puller) == Some(&object_id);
            let dest_is_puller_pos = key == puller_obj.pos();
            if !pair_intact || !dest_is_puller_pos {
                self.pull_of.remove(&puller);
                self.pulled_by.remove(&object_id);
            }
        }
        let creep = room.objects.get(&object_id).and_then(|o| match o {
            RoomObject::Creep(c) => Some(c),
            _ => None,
        });
        let movable = creep.is_some_and(|c| {
            can_move(
                c,
                old_fatigue.get(&object_id).copied().unwrap_or(0),
                self.pulled_by.contains_key(&object_id),
            )
        });
        let obstacle = check_obstacle_at_xy(
            room,
            index,
            self,
            key,
            creep.map(|c| c.owner.as_str()).unwrap_or(""),
            safe_mode_user,
        );
        if !movable || obstacle {
            self.matrix.remove(&key);
            self.winners.remove(&object_id);
            if self.matrix.contains_key(&entry.origin) {
                self.remove_from_matrix(room, index, old_fatigue, safe_mode_user, entry.origin);
            }
        }
    }
}

/// 用完整请求矩阵评分。原格请求中包含站在目标格的竞争者时，对穿评分为 100；
/// MOVE 功率包含 boost，重量至少取 1 以避免除零。
fn rate_of(
    original: &BTreeMap<(u8, u8), Vec<RateEntry>>,
    room: &RoomState,
    pulled_by: &BTreeMap<ObjId, ObjId>,
    pull_of: &BTreeMap<ObjId, ObjId>,
    entry: RateEntry,
    cell: (u8, u8),
) -> Rate {
    let Some(RoomObject::Creep(creep)) = room.objects.get(&entry.id) else {
        return Rate {
            object: entry.id,
            origin: entry.origin,
            priority: 0,
            rate1: 0,
            rate2: 0,
            rate3: 0,
            rate4: 0.0,
        };
    };
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
    let moves =
        kk_world::types::calc_body_effectiveness(&creep.body, BodyKind::Move, "move", "fatigue", 1)
            as f64;
    let weight = move_weight(creep).max(1) as f64;
    Rate {
        object: entry.id,
        origin: entry.origin,
        priority: 0,
        rate1,
        rate2,
        rate3,
        rate4: moves / weight,
    }
}

fn movers_at(m: &RateEntry, cell: (u8, u8)) -> bool {
    m.origin == cell
}

/// 被拉对象免除疲劳和 MOVE 检查；其他对象须旧疲劳为 0 且有存活 MOVE。
pub fn can_move(creep: &CreepObj, old_fatigue: u32, is_pulled: bool) -> bool {
    is_pulled || (old_fatigue == 0 && BodyKind::count_active(&creep.body, BodyKind::Move) > 0)
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
/// rampart 和无路的墙地形也阻挡。
pub fn check_obstacle_at_xy(
    room: &RoomState,
    index: &RoomIndex,
    movement: &Movement,
    at: (u8, u8),
    mover_owner: &str,
    safe_mode_user: Option<&str>,
) -> bool {
    for id in index.at(at.0 as usize, at.1 as usize) {
        let Some(obj) = room.objects.get(id) else {
            continue;
        };
        match obj {
            RoomObject::Creep(target) => {
                let is_winner = movement.winners.contains_key(id);
                let blocked = !is_winner
                    && match safe_mode_user {
                        None => true,
                        Some(sm) => sm != mover_owner || target.owner == mover_owner,
                    };
                if blocked {
                    return true;
                }
            }
            RoomObject::Generic(o) => {
                let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
                let st = data
                    .get("structureType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if st == "rampart" {
                    let public = data
                        .get("isPublic")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let owner = data.get("owner").and_then(|v| v.as_str()).unwrap_or("");
                    if !public && owner != mover_owner {
                        return true;
                    }
                    continue;
                }
                if is_obstacle_type(st) || movement.buildings.is_obstacle(st) {
                    return true;
                }
            }
            RoomObject::ConstructionSite(s) => {
                if s.owner.as_deref() == Some(mover_owner)
                    && (is_obstacle_type(&s.structure_type)
                        || movement.buildings.is_obstacle(&s.structure_type))
                {
                    return true;
                }
            }
            other => {
                let name = type_name_of(other);
                if is_obstacle_type(&name) || movement.buildings.is_obstacle(&name) {
                    return true;
                }
            }
        }
    }
    if room
        .terrain
        .check_mask(at.0 as usize, at.1 as usize, TERRAIN_MASK_WALL)
        && !has_road(room, index, at)
    {
        return true;
    }
    false
}

/// 读取游戏对象类型名；Generic 必须使用 structureType，而不是固定的 type=structure。
/// JSON 无效或字段缺失时返回空串，公开性和所有权由调用方另查。
pub fn type_name_of(obj: &RoomObject) -> std::borrow::Cow<'static, str> {
    match obj {
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            std::borrow::Cow::Owned(
                data.get("structureType")
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
