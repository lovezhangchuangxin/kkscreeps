//! 同房资源能力及首次触达记录；字段缓存不能跨资源写入继续使用。
use super::*;
use kk_mod_api::resources::{ResourceTransfer, ResourceView};

impl ModChainContext {
    pub(super) fn read_resource(&self, id: &str, resource: &str) -> Result<ResourceView, String> {
        self.resource_journal.as_ref().ok_or("本次槽没有资源能力")?;
        let id = ObjId::parse(id).ok_or("资源对象 ID 非法")?;
        kk_engine::settle::resources::inspect(&self.ctx.borrow(), id, resource)
    }

    pub(super) fn transfer_resource(
        &mut self,
        from: &str,
        to: &str,
        resource: &str,
        amount: i64,
        loss: i64,
    ) -> Result<ResourceTransfer, String> {
        let journal = self.resource_journal.as_ref().ok_or("本次槽没有资源能力")?;
        let from = ObjId::parse(from).ok_or("资源来源 ID 非法")?;
        let to = ObjId::parse(to).ok_or("资源目标 ID 非法")?;
        let mut w = self.ctx.borrow_mut();
        let mut journal = journal.borrow_mut();
        let new_objects = [from, to]
            .into_iter()
            .filter(|id| !journal.contains_key(id))
            .count();
        // 原生对象副本位于 Rust 堆，不由 V8 堆门覆盖，故限制单动作触达量。
        if journal.len() + new_objects > 256 {
            return Err("单动作资源能力最多触达 256 个对象".into());
        }
        for id in [from, to] {
            let object = w
                .room
                .objects
                .get(&id)
                .ok_or("资源对象不在本次房间候选中")?;
            journal.entry(id).or_insert_with(|| object.clone());
        }
        let result =
            kk_engine::settle::resources::transfer(&mut w, from, to, resource, amount, loss)?;
        self.cached_fields = None;
        Ok(result)
    }
}
