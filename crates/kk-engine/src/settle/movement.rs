//! 移动仲裁：官方 `engine/src/processor/intents/movement.js` 的完整移植。
//!
//! 形态：move intent 只做静态障碍预检后入矩阵（creep 不预检——挤格语义的根基，
//! move.js:33-37）；`check_impl` 逐目标格裁决（四键排序）+ 级联清链；
//! `take_move`+落格结算在 creep tick 阶段（creep.rs 的 movement_execute）。
//!
//! 矩阵用 BTreeMap<(x,y), _>（坐标序）；官方为 JS 对象插入序——逐格裁决相互
//! 独立、级联删除幂等，结果与迭代序无关，确定性由坐标序保证（灰区登记）。

use std::collections::BTreeMap;

use kk_core::TERRAIN_MASK_WALL;
use kk_schema::room::{CreepObj, RoomObject};
use kk_world::types::{BodyKind, is_obstacle_type};
use kk_world::{ObjId, RoomIndex, RoomState};

/// 官方四键（movement.js:124-136）。
#[derive(Debug, Clone)]
struct Rate {
    object: ObjId,
    origin: (u8, u8),
    rate1: i64,
    rate2: i64,
    rate3: i64,
    rate4: f64,
}

#[derive(Default, Debug)]
pub struct Movement {
    /// 目标格 → 竞争者（登记序）。
    matrix: BTreeMap<(u8, u8), Vec<RateEntry>>,
    /// 获准移动者 → 目标格。
    winners: BTreeMap<ObjId, (u8, u8)>,
}

#[derive(Debug, Clone, Copy)]
struct RateEntry {
    id: ObjId,
    origin: (u8, u8),
}

impl Movement {
    pub fn new() -> Self {
        Self::default()
    }

    /// 官方 add（movement.js:84-98）：坐标钳 0..49。
    pub fn add(&mut self, id: ObjId, origin: (u8, u8), dx: i32, dy: i32) {
        let to = (
            (origin.0 as i32 + dx).clamp(0, 49) as u8,
            (origin.1 as i32 + dy).clamp(0, 49) as u8,
        );
        self.matrix
            .entry(to)
            .or_default()
            .push(RateEntry { id, origin });
    }

    pub fn is_tile_busy(&self, x: u8, y: u8) -> bool {
        self.matrix.contains_key(&(x, y))
    }

    /// 本轮获准的目标格（creep 落格结算用；消费即不移除——官方 objects 映射同寿）。
    pub fn take_move(&self, id: ObjId) -> Option<(u8, u8)> {
        self.winners.get(&id).copied()
    }

    /// 调试探针。
    pub fn debug_matrix(&self) -> Vec<((u8, u8), Vec<ObjId>)> {
        self.matrix
            .iter()
            .map(|(k, v)| (*k, v.iter().map(|e| e.id).collect()))
            .collect()
    }

    /// 官方 check（movement.js:104-187）。
    pub fn check_impl(
        &mut self,
        room: &RoomState,
        index: &RoomIndex,
        old_fatigue: &BTreeMap<ObjId, u32>,
        safe_mode_user: Option<&str>,
    ) {
        // ---- 第一轮：逐目标格裁决（movement.js:107-152）----
        let mut new_matrix: BTreeMap<(u8, u8), Vec<RateEntry>> = BTreeMap::new();
        let mut winners = BTreeMap::new();
        // 官方第一轮循环读原始 matrix（rate1/对穿 100 依赖完整矩阵）——先快照再消费
        let original = std::mem::take(&mut self.matrix);
        let cells: Vec<((u8, u8), Vec<RateEntry>)> = original.clone().into_iter().collect();
        for (cell, entries) in cells {
            let winner = if entries.len() > 1 {
                let mut rates: Vec<Rate> = entries
                    .iter()
                    .map(|e| rate_of(&original, room, *e, cell))
                    .collect();
                // 官方 sort（movement.js:139）：四键降序；V8 稳定排序 ⇒ 平手保登记序，
                // sort_by 稳定性同语义
                rates.sort_by(|a, b| {
                    b.rate1
                        .cmp(&a.rate1)
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

        // ---- 第二轮：级联清链（movement.js:167-186）----
        let keys: Vec<(u8, u8)> = self.matrix.keys().copied().collect();
        for key in keys {
            self.remove_from_matrix(room, index, old_fatigue, safe_mode_user, key);
        }
    }

    /// 仲裁结果访问（测试用）。
    pub fn winners(&self) -> &BTreeMap<ObjId, (u8, u8)> {
        &self.winners
    }

    /// 官方 removeFromMatrix（movement.js:154-165）：删格 + 沿"对象原格也是目标格"级联。
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
        let creep = room.objects.get(&object_id).and_then(|o| match o {
            RoomObject::Creep(c) => Some(c),
            _ => None,
        });
        // canMove 或目标格静态障碍不过 → 移除（官方 :183-186）
        let movable =
            creep.is_some_and(|c| can_move(c, old_fatigue.get(&object_id).copied().unwrap_or(0)));
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
            // 级联：对象原格若也是目标格（有人在往那走），递归移除
            if self.matrix.contains_key(&entry.origin) {
                self.remove_from_matrix(room, index, old_fatigue, safe_mode_user, entry.origin);
            }
        }
    }
}

/// 官方 rates 计算（movement.js:116-137）：读原始矩阵（第一轮期间矩阵不变）。
fn rate_of(
    original: &BTreeMap<(u8, u8), Vec<RateEntry>>,
    room: &RoomState,
    entry: RateEntry,
    cell: (u8, u8),
) -> Rate {
    let Some(RoomObject::Creep(creep)) = room.objects.get(&entry.id) else {
        return Rate {
            object: entry.id,
            origin: entry.origin,
            rate1: 0,
            rate2: 0,
            rate3: 0,
            rate4: 0.0,
        };
    };
    // rate1（affectedCnt）：本格被多少竞争者当作原格盯上；对穿者（站在目标格
    // 的竞争者）=100
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
    let rate2 = 0; // pulled（M1 无 pull）
    let rate3 = 0; // pull
    // rate4 = moves / weight：weight = 非 move/carry 部件 + 资源重量
    // （movement.js:120-123，weight||1）
    let moves = BodyKind::count_active(&creep.body, BodyKind::Move) as f64;
    let weight = move_weight(creep).max(1) as f64;
    Rate {
        object: entry.id,
        origin: entry.origin,
        rate1,
        rate2,
        rate3,
        rate4: moves / weight,
    }
}

fn movers_at(m: &RateEntry, cell: (u8, u8)) -> bool {
    // 官方 _.any(matrix[key], {x,y})：竞争者的当前坐标 == 目标格（对穿）
    m.origin == cell
}

/// 官方 canMove（movement.js:11-14，无 pull 的 M1 形态）。
pub fn can_move(creep: &CreepObj, old_fatigue: u32) -> bool {
    old_fatigue == 0 && BodyKind::count_active(&creep.body, BodyKind::Move) > 0
}

/// 官方 calcResourcesWeight（movement.js:41-59）：从 body 尾部数起，
/// 装下当前携带量所需的 CARRY 部件数。
pub fn resources_weight(creep: &CreepObj) -> usize {
    let mut total_carry = creep.store.energy as i64;
    let mut weight = 0usize;
    for part in creep.body.iter().rev() {
        if total_carry <= 0 {
            break;
        }
        if part.kind != BodyKind::Carry.code() || part.hits == 0 {
            continue;
        }
        total_carry -= total_carry.min(kk_core::constants::CARRY_CAPACITY);
        weight += 1;
    }
    weight
}

/// 移动体重（movement.js:120-122）：非 move/carry 部件数 + 资源重量。
pub fn move_weight(creep: &CreepObj) -> usize {
    creep
        .body
        .iter()
        .filter(|p| p.kind != BodyKind::Move.code() && p.kind != BodyKind::Carry.code())
        .count()
        + resources_weight(creep)
}

/// 官方 checkObstacleAtXY（movement.js:16-39）。
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
            // 官方 movement.js:22：未获准移动的 creep 即障碍（对穿的另一半——
            // winner 即将让位所以不挡）。safeMode 分支按官方布尔式
            // `!SM || SM != mover || (SM == mover && mover == target_owner)`：
            // 自己 safeMode 下敌方 creep 被冻结可穿（不挡）、己方 creep 照挡
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
            // rampart：非公开且非本家（M1 无 rampart 变体，经 Generic 检查 isPublic）
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
                if is_obstacle_type(st) {
                    return true;
                }
            }
            // 官方 movement.js:25-26：自家 obstacle 结构类型的工地挡路
            // （move.js 预检无此条，靠 check 兜底——"别把自己关在工地里"）
            RoomObject::ConstructionSite(s) => {
                if s.owner.as_deref() == Some(mover_owner) && is_obstacle_type(&s.structure_type) {
                    return true;
                }
            }
            other => {
                // 类型化对象按官方类型名判障碍（source/controller/spawn/extension/wall/storage）
                if is_obstacle_type(&type_name_of(other)) {
                    return true;
                }
            }
        }
    }
    // 地形墙且无路（movement.js:37）
    if room
        .terrain
        .check_mask(at.0 as usize, at.1 as usize, TERRAIN_MASK_WALL)
        && !has_road(room, index, at)
    {
        return true;
    }
    false
}

/// 类型化对象的官方 type 名；Generic 透传桶（tower/lab/…）按 data 的
/// structureType 报告——官方 OBSTACLE 障碍、RCL 配额、工地共置判定都吃
/// 真实类型名（"structure" 不是官方类型，恒不在 OBSTACLE 表内）。
/// rampart 的公开性特判不在本函数（movement 预检/仲裁各有专门分支）。
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
    }
}

pub fn has_road(room: &RoomState, index: &RoomIndex, at: (u8, u8)) -> bool {
    index
        .at(at.0 as usize, at.1 as usize)
        .iter()
        .any(|id| matches!(room.objects.get(id), Some(RoomObject::Road(_))))
}

/// 官方 getOffsetsByDirection（utils.js）：1..8 → (dx, dy)。
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
