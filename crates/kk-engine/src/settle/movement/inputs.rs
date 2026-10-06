//! 房间级移动槽的输入投影：把仲裁状态整房一次投影为各槽输入。
//!
//! 输入构造只读登记矩阵、胜者表与房间快照，不改变仲裁；原生整链
//! 不为这些构造付费（仅 Hosted 路径按需调用）。字段纪律与
//! [`super`] 的仲裁逻辑分离，二者同文件超限后按职责拆分。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::{TERRAIN_MASK_SWAMP, TERRAIN_MASK_WALL};
use kk_schema::room::RoomObject;
use kk_world::types::BodyKind;
use kk_world::{ObjId, RoomIndex, RoomState};

use super::{
    Movement, RateEntry, check_obstacle_at_xy, has_road, move_weight, movers_at, type_name_of,
};

impl Movement {
    /// 构造移动请求预检输入：全部登记请求（均已通过原生静态预检）
    /// 按对象 ID 升序、不截断，附目的格地形与占位摘要。调用时点为
    /// 全部意图登记完成后、竞争输入构造之前；只读，不改变登记。
    pub(crate) fn precheck_input(
        &self,
        room: &RoomState,
        index: &RoomIndex,
    ) -> kk_mod_api::precheck::MovePrecheckInput {
        // 矩阵按目的格组织；先按对象 ID 排序再取各自请求，保证输入
        // 顺序确定（单执行者单请求不变量下每 id 至多一条）。
        let mut by_mover: BTreeMap<ObjId, ((u8, u8), RateEntry)> = BTreeMap::new();
        for (&cell, entries) in &self.matrix {
            for entry in entries {
                by_mover.insert(entry.id, (cell, *entry));
            }
        }
        let mut requests = Vec::new();
        for (&id, &(cell, entry)) in &by_mover {
            let creep = room.objects.get(&id).and_then(|o| match o {
                RoomObject::Creep(c) => Some(c),
                _ => None,
            });
            let mut info = kk_mod_api::precheck::PrecheckRequestInfo {
                mover_id: id.as_str().to_string(),
                owner: String::new(),
                origin_x: entry.origin.0,
                origin_y: entry.origin.1,
                target_x: cell.0,
                target_y: cell.1,
                move_power: 0,
                weight: 1,
                pulled: self.pulled_by.contains_key(&id),
                pulling: self.pull_of.contains_key(&id),
                terrain_wall: room.terrain().check_mask(
                    cell.0 as usize,
                    cell.1 as usize,
                    TERRAIN_MASK_WALL,
                ),
                terrain_swamp: room.terrain().check_mask(
                    cell.0 as usize,
                    cell.1 as usize,
                    TERRAIN_MASK_SWAMP,
                ),
                has_road: has_road(room, index, cell),
                objects: Vec::new(),
            };
            if let Some(c) = creep {
                info.owner = c.owner.clone();
                info.move_power = kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Move,
                    "move",
                    "fatigue",
                    1,
                );
                info.weight = move_weight(c).max(1) as i64;
            }
            for oid in index.at(cell.0 as usize, cell.1 as usize) {
                let Some(obj) = room.objects.get(oid) else {
                    continue;
                };
                let (kind, owner_of, is_public, structure_type) = match obj {
                    RoomObject::Creep(c) => ("creep".to_string(), c.owner.clone(), false, None),
                    RoomObject::Generic(o) => {
                        let data: serde_json::Value =
                            serde_json::from_str(&o.data).unwrap_or_default();
                        (
                            data.get("structureType")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            data.get("owner")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            data.get("isPublic")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                            None,
                        )
                    }
                    RoomObject::ConstructionSite(site) => (
                        "constructionSite".to_string(),
                        site.owner.clone().unwrap_or_default(),
                        false,
                        Some(site.structure_type.clone()),
                    ),
                    other => (type_name_of(other).to_string(), String::new(), false, None),
                };
                info.objects.push(kk_mod_api::precheck::PrecheckObjectInfo {
                    id: oid.as_str().to_string(),
                    kind,
                    owner: owner_of,
                    is_public,
                    structure_type,
                });
            }
            requests.push(info);
        }
        kk_mod_api::precheck::MovePrecheckInput { requests }
    }

    /// 预检槽成功后的否决集应用：被否决请求从登记矩阵移除，等同从未
    /// 登记（不参与竞争评分、不让位、不占格）。Native/拒绝路径不调用。
    pub(crate) fn apply_precheck_blocks(&mut self, blocks: &BTreeSet<ObjId>) {
        for id in blocks {
            self.remove_requests(*id);
        }
    }

    /// 构造拉拽登记输入：全部有效配对按拉者对象 ID 升序（`pull_of`
    /// 为 BTreeMap，天然升序）、不截断。调用时点为全部拉拽意图登记
    /// 完成后、移动请求预检之前；只读，不改变登记。
    pub(crate) fn pull_registration_input(
        &self,
        room: &RoomState,
    ) -> kk_mod_api::pull_registration::PullRegistrationInput {
        let mut pulls = Vec::new();
        for (&puller, &target) in &self.pull_of {
            let (Some(RoomObject::Creep(p)), Some(RoomObject::Creep(t))) =
                (room.objects.get(&puller), room.objects.get(&target))
            else {
                continue;
            };
            pulls.push(kk_mod_api::pull_registration::PullPairInfo {
                puller_id: puller.as_str().to_string(),
                target_id: target.as_str().to_string(),
                puller_x: p.x,
                puller_y: p.y,
                target_x: t.x,
                target_y: t.y,
                puller_owner: p.owner.clone(),
                target_owner: t.owner.clone(),
            });
        }
        kk_mod_api::pull_registration::PullRegistrationInput { pulls }
    }

    /// 构造移动代价输入：终局幸存者（取消链结算后）按对象 ID 升序、
    /// 不截断，附原生代价与构成要素。调用时点为取消链之后、执行循环
    /// 之前；只读，不改变仲裁。hasRoad 为此时点快照——逐对象执行期
    /// 道路耐久衰减可能移除道路，缺项回退按执行时点实收，覆盖值为
    /// 绝对替换不受影响。
    pub(crate) fn cost_input(
        &self,
        room: &RoomState,
        index: &RoomIndex,
    ) -> kk_mod_api::move_cost::MoveCostInput {
        let mut movers = Vec::new();
        for (&id, &cell) in &self.winners {
            let creep = room.objects.get(&id).and_then(|o| match o {
                RoomObject::Creep(c) => Some(c),
                _ => None,
            });
            let Some(creep) = creep else { continue };
            // 与 movement_execute 实收同源（裸 move_weight，无下限）；
            // nativeFatigue 是缺项回退值，必须等于原生实际收费。
            let weight = super::move_weight(creep) as i64;
            let terrain_swamp =
                room.terrain()
                    .check_mask(cell.0 as usize, cell.1 as usize, TERRAIN_MASK_SWAMP);
            let has_road = has_road(room, index, cell);
            // 与 movement_execute 同源的费率规则：沼泽 10、路 1 覆盖沼泽。
            let rate = if has_road {
                1
            } else if terrain_swamp {
                10
            } else {
                2
            };
            movers.push(kk_mod_api::move_cost::MoveCostMoverInfo {
                id: id.as_str().to_string(),
                owner: creep.owner.clone(),
                origin_x: creep.x,
                origin_y: creep.y,
                target_x: cell.0,
                target_y: cell.1,
                weight,
                terrain_swamp,
                has_road,
                body_length: creep.body.len() as i64,
                // 疲劳原生转记给链首拉者（与 head_puller 同一链走法）。
                pulled_by: self
                    .pulled_by
                    .get(&id)
                    .map(|_| self.head_puller(room, id).as_str().to_string()),
                native_fatigue: weight * rate,
            });
        }
        kk_mod_api::move_cost::MoveCostInput { movers }
    }

    /// 代价槽成功后的覆盖应用（接管所有权）；Native/拒绝路径不调用，
    /// 缺项执行原生。
    pub(crate) fn apply_costs(&mut self, costs: BTreeMap<ObjId, u32>) {
        self.costs = costs;
    }

    /// 拉拽登记槽成功后的否决集应用：被否决拉者的配对双向移除，
    /// 等同从未登记（预检/竞争/准入视野、准入预填与疲劳转移同源
    /// 反映）。Native/拒绝路径不调用。
    pub(crate) fn apply_pull_denies(&mut self, denies: &BTreeSet<ObjId>) {
        for puller in denies {
            if let Some(target) = self.pull_of.remove(puller)
                && self.pulled_by.get(&target) == Some(puller)
            {
                self.pulled_by.remove(&target);
            }
        }
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

    /// 构造准入槽输入（对象 ID 升序、不截断）；只读预填后的配对与
    /// 终值状态，不改变仲裁。pulled 即预填复核后的有效被拉标记；
    /// requests 是仲裁前的整房原始请求视野，落选者带 won=false。
    pub(crate) fn admission_input(
        &self,
        room: &RoomState,
        old_fatigue: &BTreeMap<ObjId, u32>,
    ) -> kk_mod_api::admission::MoveAdmissionInput {
        let mut movers = Vec::new();
        for (&id, &cell) in &self.winners {
            let fatigue = old_fatigue.get(&id).copied().unwrap_or(0);
            let creep = room.objects.get(&id).and_then(|o| match o {
                RoomObject::Creep(c) => Some(c),
                _ => None,
            });
            // 缩减矩阵内该目的格恰登记胜者自身；防御性取 cell 兜底。
            let origin = self
                .matrix
                .get(&cell)
                .and_then(|entries| entries.first())
                .map(|entry| entry.origin)
                .unwrap_or(cell);
            let mut info = kk_mod_api::admission::MoverInfo {
                id: id.as_str().to_string(),
                owner: String::new(),
                origin_x: origin.0,
                origin_y: origin.1,
                target_x: cell.0,
                target_y: cell.1,
                old_fatigue: fatigue,
                active_moves: 0,
                move_power: 0,
                weight: 1,
                pulled: self.pulled_by.contains_key(&id),
                pulled_by: self
                    .pulled_by
                    .get(&id)
                    .map(|puller| puller.as_str().to_string()),
                pulling: self
                    .pull_of
                    .get(&id)
                    .map(|target| target.as_str().to_string()),
            };
            if let Some(c) = creep {
                info.owner = c.owner.clone();
                info.active_moves = BodyKind::count_active(&c.body, BodyKind::Move) as i64;
                info.move_power = kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Move,
                    "move",
                    "fatigue",
                    1,
                );
                info.weight = move_weight(c).max(1) as i64;
            }
            movers.push(info);
        }
        let mut requests = Vec::new();
        for (&cell, entries) in &self.original_requests {
            for (index, entry) in entries.iter().enumerate() {
                requests.push(kk_mod_api::admission::MoveRequestInfo {
                    mover_id: entry.id.as_str().to_string(),
                    x: cell.0,
                    y: cell.1,
                    index: index as u64,
                    origin_x: entry.origin.0,
                    origin_y: entry.origin.1,
                    won: self.winners.get(&entry.id) == Some(&cell),
                });
            }
        }
        kk_mod_api::admission::MoveAdmissionInput { movers, requests }
    }

    /// 构造障碍槽输入；只读胜者表与对象摘要，不改变仲裁。native
    /// 初判为预填时点（全部胜者未取消）的 `check_obstacle_at_xy`
    /// 结果，链内让位取消只会收紧不会放宽。仅 Hosted 路径调用。
    pub(crate) fn obstacle_input(
        &self,
        room: &RoomState,
        index: &RoomIndex,
        safe_mode_user: Option<&str>,
    ) -> kk_mod_api::obstacle::MoveObstacleInput {
        let mut movers = Vec::new();
        for (&id, &cell) in &self.winners {
            let owner = room
                .objects
                .get(&id)
                .and_then(|o| match o {
                    RoomObject::Creep(c) => Some(c.owner.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let native_blocked =
                check_obstacle_at_xy(room, index, self, cell, owner.as_str(), safe_mode_user);
            let mut objects = Vec::new();
            for oid in index.at(cell.0 as usize, cell.1 as usize) {
                let Some(obj) = room.objects.get(oid) else {
                    continue;
                };
                let (kind, owner_of, is_public, structure_type, yielding) = match obj {
                    RoomObject::Creep(c) => (
                        "creep".to_string(),
                        c.owner.clone(),
                        false,
                        None,
                        // 准入被拒的胜者必被链内取消，让位预期不成立；
                        // 收紧为胜者且准入终值为真（相对最终结果仍是下界）。
                        self.winners.contains_key(oid) && self.admitted(*oid),
                    ),
                    RoomObject::Generic(o) => {
                        let data: serde_json::Value =
                            serde_json::from_str(&o.data).unwrap_or_default();
                        (
                            data.get("structureType")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            data.get("owner")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            data.get("isPublic")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                            None,
                            false,
                        )
                    }
                    RoomObject::ConstructionSite(site) => (
                        "constructionSite".to_string(),
                        site.owner.clone().unwrap_or_default(),
                        false,
                        Some(site.structure_type.clone()),
                        false,
                    ),
                    other => (
                        type_name_of(other).to_string(),
                        String::new(),
                        false,
                        None,
                        false,
                    ),
                };
                objects.push(kk_mod_api::obstacle::ObstacleObjectInfo {
                    id: oid.as_str().to_string(),
                    kind,
                    owner: owner_of,
                    is_public,
                    structure_type,
                    yielding,
                });
            }
            movers.push(kk_mod_api::obstacle::ObstacleMoverInfo {
                id: id.as_str().to_string(),
                owner,
                target_x: cell.0,
                target_y: cell.1,
                terrain_wall: room.terrain().check_mask(
                    cell.0 as usize,
                    cell.1 as usize,
                    TERRAIN_MASK_WALL,
                ),
                has_road: has_road(room, index, cell),
                objects,
                native_blocked,
            });
        }
        kk_mod_api::obstacle::MoveObstacleInput { movers }
    }
}
