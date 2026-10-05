//! Mod 建筑的事务边界：建造槽拒绝回滚完工变换、玩家写回失败后缓存
//! 重试不重复执行玩家代码且生产/库存结算正确。
use super::*;
use harness::{events, hits};

const SERVER_BODY: &str = r#"
  api.resources.define('bm:ore');
  api.resources.define('bm:ingot');
  api.buildings.define('bm:foundry', {
    playerClass: 'Foundry',
    cost: { energy: 1000, 'bm:ore': 100 },
    hitsMax: 2000,
    pathing: 'obstacle',
    capacity: 500,
    production: { interval: 10, inputs: { energy: 50, 'bm:ore': 20 }, outputs: { 'bm:ingot': 5 } },
  });
"#;

const PLAYER: &str = r#"
export default function install(api) {
  class Foundry extends api.builtins.OwnedStructure {}
  api.buildings.bind('bm:foundry', Foundry);
}
"#;

fn building_engine(extra_server: &str, kkpv: bool) -> anyhow::Result<EmbedEngine> {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-building-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    let dir = root.0.join("bm");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("mod.json"),
        json!({"id":"bm","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}).to_string(),
    )?;
    std::fs::write(
        dir.join("server.js"),
        format!("export default function register(api) {{\n{SERVER_BODY}\n{extra_server}\n}}"),
    )?;
    std::fs::write(dir.join("player.js"), PLAYER)?;
    let packages = discover_packages(&root.0)?;
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(kkpv);
    e.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "rcl": 6, "owner": "p1"}]}),
        &packages,
        RegisterOptions::default(),
    )?;
    Ok(e)
}

fn worker(
    e: &mut EmbedEngine,
    name: &str,
    x: u8,
    y: u8,
    energy: u32,
    ore: u32,
) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({"pos":[x,y],"owner":"p1","name":name,
            "body":["work","work","work","work","carry","carry","move","move","move","move"],
            "store":{"energy":energy,"bm:ore":ore}}),
    )
}

fn near_done_site(e: &mut EmbedEngine) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "constructionSite",
        &json!({"pos":[11,10],"structureType":"bm:foundry","owner":"p1","progress":990,"progressTotal":1000}),
    )
}

fn foundry_of(e: &EmbedEngine) -> Option<String> {
    let snap = e.world_snapshot();
    snap["objects"]
        .as_array()?
        .iter()
        .find(|o| o["type"] == "structure" && o["structureType"] == "bm:foundry")
        .and_then(|o| o["id"].as_str())
        .map(str::to_string)
}

/// 建造槽 after 拒绝：完工变换（工地移除+结构插入+ID 游标+事件）与
/// 多资源支付一起被撤销，世界回到动作前；控制组完工落地。
#[test]
fn mod_building_completion_rolls_back_on_build_slot_decline() -> anyhow::Result<()> {
    let reject = "api.handlers.after('builtin:creep.build',{id:'no'},()=>api.decline('halt'));";
    for (extra, completed, label) in [
        ("", true, "控制组：完工落地"),
        (reject, false, "拒绝组：整链撤销"),
    ] {
        for kkpv in [false, true] {
            let mut e = building_engine(extra, kkpv)?;
            let w = worker(&mut e, "w", 10, 10, 1000, 100)?;
            let site = near_done_site(&mut e)?;
            e.run_player(
                "p1",
                &format!("Game.creeps.w.build(Game.getObjectById('{site}'))"),
            )?;
            let building = foundry_of(&e);
            if completed {
                let id = building.expect("完工落成");
                assert_eq!(hits(&e, &id), 2000);
                assert_eq!(
                    e.get_object(&w).unwrap()["store"]["bm:ore"],
                    json!(99),
                    "尾款 1 矿已付"
                );
                assert_eq!(e.get_object(&w).unwrap()["store"]["energy"], json!(990));
            } else {
                assert!(building.is_none(), "{label}: 无结构残留");
                let site_obj = e.get_object(&site).unwrap();
                assert_eq!(site_obj["progress"], json!(990), "{label}: 进度回滚");
                assert_eq!(
                    e.get_object(&w).unwrap()["store"]["bm:ore"],
                    json!(100),
                    "{label}: 支付回滚"
                );
                assert_eq!(e.get_object(&w).unwrap()["store"]["energy"], json!(1000));
                assert_eq!(
                    events(&e, kk_core::constants::EVENT_BUILD).len(),
                    0,
                    "{label}: 事件截除"
                );
            }
        }
    }
    Ok(())
}

/// 玩家晚期写回失败：缓存重试不重复执行玩家代码；生产周期在重试后的
/// 同一 tick 恰好结算一次。
#[test]
fn writeback_failure_retry_settles_production_exactly_once() -> anyhow::Result<()> {
    for kkpv in [false, true] {
        let mut e = building_engine("", kkpv)?;
        // 完工建筑 + 周期输入就位；世界时间 0 是生产周期点。
        let foundry = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"bm:foundry","owner":"p1",
                "store":{"energy":50,"bm:ore":20},"storeCapacity":500}),
        )?;
        let _feeder = worker(&mut e, "f", 10, 10, 100, 0)?;
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let run = e.run_player("p1", "Memory.n=(Memory.n||0)+1; Game.creeps.f.say('hi');");
        assert!(run.is_err(), "晚期写回失败必须返回错误");
        assert_eq!(e.get_game_time(), 0, "失败轮不提交");
        assert!(
            e.get_object(&foundry).unwrap()["store"]
                .get("bm:ingot")
                .is_none()
                || e.get_object(&foundry).unwrap()["store"]["bm:ingot"].is_null(),
            "失败轮不发布候选（生产未结算）"
        );
        // 缓存重试：玩家代码不重复执行（第二次 run_player 的代码是占位）。
        e.run_player("p1", "throw Error('cached retry only')")?;
        let obj = e.get_object(&foundry).unwrap();
        assert_eq!(obj["store"]["bm:ingot"], json!(5), "生产周期恰好一次");
        assert_eq!(obj["store"]["energy"], json!(0));
        assert_eq!(obj["store"]["bm:ore"], json!(0));
        let raw = &e.player_state_for_test("p1").unwrap().raw_memory;
        let memory: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(memory["n"], json!(1), "玩家代码只执行了一次");
    }
    Ok(())
}
