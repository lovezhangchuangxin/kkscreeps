//! 宿主输入的计划/授权门与玩家冲突合并；与玩家执行编排分开维护。
use super::*;
use kk_engine::intent::IntentBatch;

impl EmbedEngine {
    /// 受限宿主输入的当前授权/描述符门；结算仍再次核验权威候选。
    pub fn validate_host_intent(&self, intent: &crate::HostIntent) -> Result<(), String> {
        let key = kk_mod_api::Key::new(&intent.intent).map_err(|e| e.to_string())?;
        if !self.engine.players().contains_key(&intent.player) {
            return Err("代表玩家未注册".into());
        }
        if self.disabled_mods().contains(key.namespace()) {
            return Err("输入包已停用".into());
        }
        let descriptor = self.engine.mod_intent(&key).ok_or("意图不在当前冻结目录")?;
        if descriptor.scope != kk_mod_api::IntentScope::Object
            || descriptor.slot.namespace() == "builtin"
        {
            return Err("输入只允许已实现的 Mod 对象意图".into());
        }
        kk_mod_api::validate::check_value(&intent.args, &descriptor.arguments, "宿主输入")?;
        let object = self.get_object(&intent.target).ok_or("输入目标不存在")?;
        let type_key = object["typeKey"].as_str().ok_or("输入目标不是 Mod 对象")?;
        if descriptor
            .receiver
            .as_ref()
            .map(|receiver| receiver.key.as_str())
            != Some(type_key)
        {
            return Err("输入目标与意图接收者不一致".into());
        }
        let ty = self
            .engine
            .mod_object_type(type_key)
            .ok_or("目标类型不在冻结目录")?;
        let owner = ty.owner_field.as_ref().ok_or("目标类型未声明归属字段")?;
        if object["fields"][kk_mod_api::validate::field_name(owner)].as_str()
            != Some(&intent.player)
        {
            return Err("输入目标不属于代表玩家".into());
        }
        Ok(())
    }

    /// 合并已清洗并绑定玩家身份的意图，不使用 JavaScript 自报的身份。
    #[cfg(test)]
    pub(super) fn batch_from_results(results: &BTreeMap<String, RunResult>) -> IntentBatch {
        Self::batch_with_host_intents(results, &[])
    }

    /// 结算批构造：宿主侧受限 Mod 输入（S2.3）先注入，玩家 wire 的
    /// 同对象同意图后合并覆盖——外部命令永不压制玩家提交（结构性
    /// 玩家优先，不依赖到达先后）。宿主意图的结算核验（计划目录/
    /// schema/接收者/归属）由结算层原样执行，不过即丢弃。
    pub(super) fn batch_with_host_intents(
        results: &BTreeMap<String, RunResult>,
        host_intents: &[crate::HostIntent],
    ) -> IntentBatch {
        let mut batch = IntentBatch::default();
        for intent in host_intents {
            // 与玩家 wire 的 to_id 同口径（kk 前缀十六进制→ObjId）；
            // 键形或 ID 非法即丢弃（命令面已校验，此处防御性兜底）。
            if let (Some(obj), Ok(key)) = (
                kk_world::ObjId::parse(&intent.target),
                kk_mod_api::Key::new(&intent.intent),
            ) {
                batch.add_ext_intent(&intent.player, obj, key, intent.args.clone());
            }
        }
        for (registered_player, result) in results {
            // 注册表键是身份真相源；自报 player 和其他玩家键均不能越过此边界。
            if let Some(intents) = result.intents.users.get(registered_player) {
                let entry = batch.users.entry(registered_player.clone()).or_default();
                // 玩家 wire 的自有意图整体并入（宿主只注入 ext，不冲突）。
                for (id, actions) in &intents.objects {
                    entry
                        .objects
                        .entry(*id)
                        .or_default()
                        .extend(actions.clone());
                }
                entry.room.extend(intents.room.clone());
                entry.notify.extend(intents.notify.clone());
                entry.market.extend(intents.market.clone());
                entry.power.extend(intents.power.clone());
                // ext 逐键合并：玩家提交的键覆盖宿主注入（显式玩家优先，
                // 不依赖到达先后）；玩家未提交的宿主键保留（不同意图键
                // 不构成同槽冲突）。
                for (id, ext) in &intents.ext {
                    entry.ext.entry(*id).or_default().extend(ext.clone());
                }
            }
        }
        batch
    }
}
