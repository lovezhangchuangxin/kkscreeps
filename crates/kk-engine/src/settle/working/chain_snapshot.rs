//! 整链候选快照、对象变更日志与局部拒绝恢复。
use super::*;

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

impl Working {
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
            touched.extend(crate::settle::creep::combat_touch_targets(self, target_id));
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
        action: crate::settle::creep::BuildAction,
    ) -> ChainSnapshot {
        let mut touched: Vec<ObjId> = vec![id];
        if let Some(target_id) = target {
            touched.extend(crate::settle::creep::build_touch_targets(
                self, target_id, action,
            ));
        }
        if action == crate::settle::creep::BuildAction::Dismantle {
            touched.extend(crate::settle::creep::dismantle_drop_face(self, id));
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

    pub(super) fn remember_index_cell(&mut self, position: (u8, u8)) {
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
}
