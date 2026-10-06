//! 同房对象宿主能力；默认拒绝，具体槽显式开放。
use super::*;

pub trait ObjectsHost {
    fn object_get(&mut self, _id: &str) -> Result<Option<kk_mod_api::objects::ObjectView>, String> {
        Err("本次槽没有对象读取能力".into())
    }
    fn object_list(
        &mut self,
        _type_key: &str,
        _after: &str,
        _limit: u32,
    ) -> Result<kk_mod_api::objects::ObjectPage, String> {
        Err("本次槽没有对象读取能力".into())
    }
    fn object_create(
        &mut self,
        _binding: &str,
        _type_key: &str,
        _fields: &Value,
    ) -> Result<kk_mod_api::objects::ObjectView, String> {
        Err("本次槽没有对象创建能力".into())
    }
    fn object_update(
        &mut self,
        _binding: &str,
        _id: &str,
        _fields: &Value,
    ) -> Result<kk_mod_api::objects::ObjectView, String> {
        Err("本次槽没有对象更新能力".into())
    }
    fn object_remove(&mut self, _binding: &str, _id: &str) -> Result<(), String> {
        Err("本次槽没有对象删除能力".into())
    }
}
