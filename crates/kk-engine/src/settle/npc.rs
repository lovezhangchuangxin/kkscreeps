//! NPC 预意图生成与合并；Keeper 巡守/战斗、Invader 无目标退场/远程攻击。
mod invader;
mod keeper;
mod path;
use std::borrow::Cow;

use kk_world::domain::npc::INVADER_OWNER;

use crate::intent::IntentBatch;

/// NPC 与玩家共用动作派发/候选提交；显式同名动作覆盖自动指令，按官方
/// processor.js 的 assign(ii, existing) 合并。无 NPC 指令时不复制玩家 batch。
/// Keeper 记忆写入房间候选，重试/回放从同一房间输入确定性重建。
pub(super) fn with_npc_intents<'a>(
    w: &mut super::Working,
    batch: &'a IntentBatch,
) -> Result<Cow<'a, IntentBatch>, super::SettleError> {
    let mut all = vec![(INVADER_OWNER, invader::intents(&w.room))];
    all.push((kk_world::domain::keeper::OWNER, keeper::intents(w)?));
    let mut batch = Cow::Borrowed(batch);
    for (owner, generated) in all {
        if !generated.objects.is_empty() {
            let user = batch.to_mut().users.entry(owner.into()).or_default();
            for (id, actions) in generated.objects {
                let existing = user.objects.entry(id).or_default();
                for (action, data) in actions {
                    existing.entry(action).or_insert(data);
                }
            }
        }
    }
    Ok(batch)
}
