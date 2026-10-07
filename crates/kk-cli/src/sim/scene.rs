//! 版本化场景输入及启动前校验，不持有运行中的世界。
use super::{
    assertions::{Assertion, Comparison},
    mods,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Scene {
    pub(super) version: u32,
    pub(super) shard: Shard,
    pub(super) bots: Vec<Bot>,
    /// 世界种子：随机流域与确定性时间的派生根；缺省 0 保持既有场景行为。
    #[serde(default)]
    pub(super) seed: u64,
    #[serde(default)]
    pub(super) mods: Option<mods::Mods>,
    #[serde(default)]
    pub(super) mods_switch: Option<mods::ModsSwitch>,
    #[serde(default)]
    pub(super) objects: Vec<Object>,
    #[serde(default)]
    pub(super) assertions: Vec<Assertion>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Shard {
    pub(super) players: Vec<Value>,
    pub(super) rooms: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bot {
    pub(super) player: String,
    pub(super) modules: PathBuf,
    #[serde(default = "main_entry")]
    pub(super) entry: String,
    #[serde(default = "empty_memory")]
    pub(super) memory: Value,
}

fn main_entry() -> String {
    "main".into()
}
fn empty_memory() -> Value {
    json!({})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Object {
    pub(super) alias: String,
    pub(super) room: String,
    pub(super) kind: String,
    pub(super) spec: Value,
}

pub(super) fn validate(scene: &Scene) -> anyhow::Result<()> {
    anyhow::ensure!(
        scene.version == 1,
        "不支持的场景 version {}（仅支持 1）",
        scene.version
    );
    let mut players = BTreeSet::new();
    for player in &scene.shard.players {
        let name = player
            .as_str()
            .or_else(|| player["name"].as_str())
            .unwrap_or("");
        anyhow::ensure!(
            !name.is_empty() && players.insert(name),
            "玩家名为空或重复：{name}"
        );
    }
    anyhow::ensure!(!players.is_empty(), "场景至少需要一名玩家");
    let mut bots = BTreeSet::new();
    for bot in &scene.bots {
        anyhow::ensure!(
            bots.insert(bot.player.as_str()),
            "重复 bot 玩家：{}",
            bot.player
        );
        anyhow::ensure!(!bot.entry.is_empty(), "bot entry 不能为空");
        anyhow::ensure!(
            bot.memory.is_object(),
            "玩家 {} 的初始 memory 必须是对象",
            bot.player
        );
    }
    anyhow::ensure!(
        bots == players,
        "bots 必须恰好覆盖 shard.players 中的每名玩家"
    );
    let mut aliases = BTreeSet::new();
    for object in &scene.objects {
        anyhow::ensure!(
            !object.alias.is_empty() && aliases.insert(&object.alias),
            "对象 alias 为空或重复：{}",
            object.alias
        );
        anyhow::ensure!(
            object.spec.is_object(),
            "对象 {} 的 spec 必须是对象",
            object.alias
        );
    }
    for assertion in &scene.assertions {
        anyhow::ensure!(
            valid_pointer(&assertion.pointer),
            "非法 JSON Pointer：{}",
            assertion.pointer
        );
        if matches!(assertion.op, Comparison::Gte | Comparison::DeltaGte) {
            anyhow::ensure!(
                assertion.value.as_f64().is_some(),
                "数值断言 {} 的 value 必须是数字",
                assertion.pointer
            );
        }
    }
    if let Some(switch) = &scene.mods_switch {
        anyhow::ensure!(
            scene.mods.is_some(),
            "mods_switch 需要 mods 声明根目录（切换复用同一根）"
        );
        anyhow::ensure!(switch.after_tick() >= 1, "mods_switch.after_tick 须 ≥ 1");
    }
    Ok(())
}

fn valid_pointer(pointer: &str) -> bool {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return false;
    }
    let mut chars = pointer.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}
