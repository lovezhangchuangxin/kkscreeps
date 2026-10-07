//! 领域能力的对象首触日志；引擎拥有候选保存和恢复规则。
use super::*;

#[derive(Default)]
pub struct ObjectJournal {
    entries: BTreeMap<ObjId, ObjectBefore>,
}

struct ObjectBefore {
    object: RoomObject,
    queues: Option<(Option<i64>, Option<i64>)>,
}

impl ObjectJournal {
    /// 资源能力只改变对象库存；保存第一次触达时的记录，限制 Rust 堆日志。
    pub fn remember_resources(&mut self, w: &Working, targets: &[ObjId]) -> Result<(), String> {
        let new_objects = targets
            .iter()
            .filter(|id| !self.entries.contains_key(id))
            .count();
        if self.entries.len() + new_objects > 256 {
            return Err("单动作资源能力最多触达 256 个对象".into());
        }
        for id in targets {
            let object = w.room.objects.get(id).ok_or("资源对象不在本次房间候选中")?;
            self.entries.entry(*id).or_insert_with(|| ObjectBefore {
                object: object.clone(),
                queues: None,
            });
        }
        Ok(())
    }

    /// 战斗与建造改向同时保存延迟伤害/治疗队列，首触值不被后续写入覆盖。
    pub fn remember_redirects(&mut self, w: &Working, targets: &[ObjId]) {
        for id in targets {
            if !self.entries.contains_key(id)
                && let Some(object) = w.room.objects.get(id).cloned()
            {
                self.entries.insert(
                    *id,
                    ObjectBefore {
                        object,
                        queues: Some((
                            w.damage_to_apply.get(id).copied(),
                            w.heal_to_apply.get(id).copied(),
                        )),
                    },
                );
            }
        }
    }

    /// 正常拒绝时恢复首触值；对象增删/索引/ID 由外层 ChainSnapshot 随后恢复。
    /// 改向动作的 consumed 门保证 native 与能力执行互斥；资源槽不改位置或队列。
    pub fn restore(&mut self, w: &mut Working) {
        for (id, entry) in std::mem::take(&mut self.entries) {
            w.room.objects.insert(id, entry.object);
            if let Some((damage, heal)) = entry.queues {
                restore_queue(&mut w.damage_to_apply, id, damage);
                restore_queue(&mut w.heal_to_apply, id, heal);
            }
        }
    }
}

fn restore_queue(queue: &mut BTreeMap<ObjId, i64>, id: ObjId, previous: Option<i64>) {
    match previous {
        Some(value) => {
            queue.insert(id, value);
        }
        None => {
            queue.remove(&id);
        }
    }
}
