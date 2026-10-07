//! 扩展字段声明与值校验。
use super::*;

impl Working {
    /// 写入对象扩展字段的候选值。先解析对象类型（目前仅 Generic 结构
    /// 按 structureType 映射），再校验字段键合法、已在目录中声明，
    /// 最后按字段 schema 校验值（类型、取值域、长度限额、有限浮点）。
    /// 写入只修改本房候选，结算失败时随房间候选一并丢弃。
    pub fn set_extension(
        &mut self,
        id: ObjId,
        field: &str,
        value: serde_json::Value,
    ) -> Result<(), String> {
        let type_key = self.extension_type_of(id)?;
        let field_key =
            kk_mod_api::Key::new(field).map_err(|e| format!("扩展字段键非法 {field:?}: {e}"))?;
        let Some(rule) = self.plane.extensions.field(&type_key, &field_key) else {
            return Err(format!("扩展字段未声明：{field}"));
        };
        kk_mod_api::validate::check_value(&value, &rule.schema, field)?;
        self.room
            .extensions
            .entry(id)
            .or_default()
            .insert(field.to_string(), value);
        Ok(())
    }

    /// 移除对象的扩展字段候选；字段未在目录声明时拒绝。
    pub fn remove_extension(&mut self, id: ObjId, field: &str) -> Result<(), String> {
        let type_key = self.extension_type_of(id)?;
        let field_key =
            kk_mod_api::Key::new(field).map_err(|e| format!("扩展字段键非法 {field:?}: {e}"))?;
        if self.plane.extensions.field(&type_key, &field_key).is_none() {
            return Err(format!("扩展字段未声明：{field}"));
        }
        if let Some(fields) = self.room.extensions.get_mut(&id) {
            fields.remove(field);
            if fields.is_empty() {
                self.room.extensions.remove(&id);
            }
        }
        Ok(())
    }

    /// 解析对象在扩展目录中的类型键。目前只有 Generic 结构按
    /// structureType 映射 builtin:{structure_type}；其他对象族
    /// 尚未声明扩展类型，一律拒绝。
    fn extension_type_of(&self, id: ObjId) -> Result<kk_mod_api::Key, String> {
        let structure_type = match self.room.objects.get(&id) {
            Some(RoomObject::Generic(g)) => kk_world::domain::generic::structure_type(&g.data),
            _ => None,
        };
        let Some(st) = structure_type else {
            return Err(format!("对象 {} 的类型未接入扩展平面", id.as_str()));
        };
        let key = kk_mod_api::Key::new(format!("builtin:{st}"))
            .map_err(|e| format!("结构类型 {st:?} 不能构成类型键: {e}"))?;
        if self.plane.extensions.fields_of(&key).is_empty() {
            return Err(format!("结构类型 {st} 未声明扩展字段"));
        }
        Ok(key)
    }
}
