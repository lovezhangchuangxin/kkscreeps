//! 验证玩家意图从沙箱传到结算，再出现在下一轮玩家视图中的完整过程。
//! 场景由 `embed_engine_suite` 顺序调用。

use super::*;

mod kkpv;
pub(super) use kkpv::*;

mod families;
pub(super) use families::*;

/// 束分组共享端到端：双玩家互驻军（对方房各有己方结构）→ 完整可见集
/// 相同、同组，KKPV 通道下组内首个查看者装配发布、次个命中共享句柄
///（组缓存恰一条），且两玩家读到的双房结构与逐查看者装配完全一致；
/// 出现归属旗（块字节依赖查看者裁切）后资格失效清空，回退逐查看者。
pub(super) fn grouped_bundle_overlap_impl() -> anyhow::Result<()> {
    use serde_json::json;
    use std::collections::BTreeMap;

    let mut e = kk_runtime::EmbedEngine::new();
    e.set_kkpv_channel(true);
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
        ],
    }))?;
    // 互驻军：p1 的 road 进 E1N1、p2 的 road 进 W1N1，两玩家的完整可见
    // 集都成为 {W1N1, E1N1}（打架驻军的典型视野重叠形态）。
    // 驻军用带归属的可占领结构（link）：road 等无主结构不授予视野。
    e.place_object(
        "E1N1",
        "structure",
        &json!({"pos":[10,10],"structureType":"link","owner":"p1","store":{"energy":0},"cooldown":0}),
    )?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,11],"structureType":"link","owner":"p2","store":{"energy":0},"cooldown":0}),
    )?;
    let code = "JSON.stringify([Game.rooms['W1N1'].find(FIND_STRUCTURES).length, Game.rooms['E1N1'].find(FIND_STRUCTURES).length])".to_string();
    let codes = BTreeMap::from([("p1".to_string(), code.clone()), ("p2".to_string(), code)]);
    let runs = e.run_players(&codes)?;
    let seen = |player: &str| -> String {
        runs[player].expr["value"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        seen("p1").starts_with('[') && seen("p2").starts_with('['),
        "两玩家都读到结构计数：p1={} p2={}",
        seen("p1"),
        seen("p2")
    );
    assert_eq!(
        seen("p1"),
        seen("p2"),
        "同组两玩家读到的双房结构必须一致（世界状态相同）"
    );
    let (eligible, groups) = e.kkpv_grouped_cache_for_test();
    assert!(eligible, "无旗/工地/Mod/扩展的世界分组资格成立");
    assert_eq!(groups, 1, "互驻军两玩家同一可见集，组缓存恰一条共享束");

    // 资格失效回退：归属旗使所在房不可共享（敌旗裁切依赖查看者），
    // 下一轮资格为假、组缓存清空，行为仍与逐查看者装配一致。
    e.place_object(
        "W1N1",
        "flag",
        &json!({"pos":[12,12],"name":"war","owner":"p1","color":1,"secondaryColor":1}),
    )?;
    let runs = e.run_players(&codes)?;
    let seen_again = |player: &str| -> String {
        runs[player].expr["value"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(seen_again("p1"), seen_again("p2"), "回退后两玩家读数仍一致");
    let (eligible, groups) = e.kkpv_grouped_cache_for_test();
    assert!(!eligible, "带归属旗的世界分组资格失效");
    assert_eq!(groups, 0, "资格失效时组缓存清空");
    Ok(())
}
