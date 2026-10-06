//! 对象能力只作用于自有槽的同房候选；绑定 namespace 是写权限归属。
use super::*;
use kk_engine::settle::objects;
use kk_mod_api::objects::{ObjectPage, ObjectView};

impl ModChainContext {
    fn check_objects(&self) -> Result<(), String> {
        if !matches!(self.capability, ChainCapability::Objects) {
            return Err("本次槽没有自有对象能力".into());
        }
        Ok(())
    }
}

// 主模块的 ChainHost 转发到此领域实现，候选借用不跨调用保留。
impl ModChainContext {
    pub(super) fn get_object(&self, id: &str) -> Result<Option<ObjectView>, String> {
        self.check_objects()?;
        let id = ObjId::parse(id).ok_or("对象 ID 非法")?;
        objects::get(&self.ctx.borrow(), id)
    }
    pub(super) fn list_objects(
        &self,
        type_key: &str,
        after: &str,
        limit: u32,
    ) -> Result<ObjectPage, String> {
        self.check_objects()?;
        objects::list(&self.ctx.borrow(), type_key, after, limit)
    }
    pub(super) fn create_object(
        &mut self,
        binding: &str,
        type_key: &str,
        fields: &Value,
    ) -> Result<ObjectView, String> {
        self.check_objects()?;
        let key = Key::new(binding)?;
        self.cached_fields = None;
        objects::create(
            &mut self.ctx.borrow_mut(),
            key.namespace(),
            type_key,
            fields,
        )
    }
    pub(super) fn update_object(
        &mut self,
        binding: &str,
        id: &str,
        fields: &Value,
    ) -> Result<ObjectView, String> {
        self.check_objects()?;
        let key = Key::new(binding)?;
        let id = ObjId::parse(id).ok_or("对象 ID 非法")?;
        self.cached_fields = None;
        objects::update(&mut self.ctx.borrow_mut(), key.namespace(), id, fields)
    }
    pub(super) fn remove_object(&mut self, binding: &str, id: &str) -> Result<(), String> {
        self.check_objects()?;
        let key = Key::new(binding)?;
        let id = ObjId::parse(id).ok_or("对象 ID 非法")?;
        self.cached_fields = None;
        objects::remove(&mut self.ctx.borrow_mut(), key.namespace(), id)
    }
}
