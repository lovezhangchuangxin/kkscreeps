//! deployStronghold 布防首片：模板布局铺场、清格、确定性奖励与寿命绑定。
use super::*;
use kk_core::constants as c;
use kk_world::domain::npc::INVADER_OWNER;
use kk_world::domain::stronghold as sh;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(
        &json!({"players":["p1"],"rooms":[{"name":"W1N1"},{"name":"W2N1"},{"name":"W3N1"}]}),
    )?;
    Ok(e)
}

/// 放置一枚到期即部署的模板核心。
fn deploy_core(e: &mut EmbedEngine, template: &sh::StrongholdTemplate) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "invaderCore",
        &json!({
            "pos":[25,25],"level":template.core_level,"ticksToDeploy":1,
            "templateName":template.name,
            "strongholdId":format!("sh-{}",template.name)
        }),
    )
}

/// 摆一只 p1 侦察 creep 提供玩家 API 视野。
fn scout(e: &mut EmbedEngine) -> anyhow::Result<()> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({"name":"scout","owner":"p1","pos":[10,10],"body":["move"],"ticksToLive":2000}),
    )?;
    Ok(())
}

/// 全模板逐套核对：一 tick 后房间结构恰为官方布局，核心原位保留，
/// 全体布防共享同一坍塌截止。
#[test]
fn stronghold_deploy_places_canonical_layout_for_every_bunker() -> anyhow::Result<()> {
    for binary in [false, true] {
        for template in sh::TEMPLATES {
            let mut e = fixture(binary)?;
            let core_id = deploy_core(&mut e, template)?;
            scout(&mut e)?;
            e.tick(1)?;
            let all = e.find_in_room("W1N1", c::FIND_STRUCTURES);
            let cores: Vec<&Value> = all
                .iter()
                .filter(|s| s["structureType"] == "invaderCore")
                .collect();
            assert_eq!(cores.len(), 1, "{} 恰一枚核心", template.name);
            assert_eq!(cores[0]["id"], json!(core_id), "核心原位保留");
            let mut placed: Vec<String> = all
                .iter()
                .filter(|s| s["id"] != json!(core_id) && s["structureType"] != "controller")
                .map(|s| {
                    format!(
                        "{}@{},{}",
                        s["structureType"].as_str().unwrap(),
                        s["pos"]["x"].as_i64().unwrap() - 25,
                        s["pos"]["y"].as_i64().unwrap() - 25
                    )
                })
                .collect();
            placed.sort();
            let mut expected: Vec<String> = template
                .structures
                .iter()
                .map(|en| format!("{}@{},{}", en.kind, en.dx, en.dy))
                .collect();
            expected.sort();
            assert_eq!(placed, expected, "{} 布局与官方模板一致", template.name);
            // 部署清 deployTime（快照面 null），坍塌效果落在官方抽样窗口。
            let core = e.get_object(&core_id).unwrap();
            assert_eq!(core["ticksToDeploy"], Value::Null);
            let rampart_id = all
                .iter()
                .find(|s| {
                    s["structureType"] == "rampart"
                        && s["pos"]["x"] == json!(25)
                        && s["pos"]["y"] == json!(25)
                })
                .map(|s| s["id"].as_str().unwrap().to_string())
                .expect("核心格必有 rampart");
            // 同一拍读取核心与 rampart 的坍塌剩余，避免跨拍差一。
            let pair = e
                .run_player(
                    "p1",
                    &format!(
                        "[Game.getObjectById('{core_id}').effects.find(x=>x.effect===1002).ticksRemaining,\
                          Game.getObjectById('{rampart_id}').effects.find(x=>x.effect===1002).ticksRemaining]"
                    ),
                )?
                .expr["value"]
                .clone();
            let remaining = pair[0].as_i64().unwrap();
            assert!(
                (67_500..=82_500).contains(&remaining),
                "{} 衰减窗口 {remaining}",
                template.name
            );
            assert_eq!(pair[1], json!(remaining), "布防共享截止时刻");
        }
    }
    Ok(())
}

/// 官方 objectOptions 数值与奖励：rampart 耐久取档位表、tower 满能量、
/// container 显式 0 容量且奖励按密度凑总、结构带 strongholdId。
#[test]
fn stronghold_deploy_uses_official_structure_values_and_rewards() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let template = sh::template("bunker2").unwrap();
        let core_id = deploy_core(&mut e, template)?;
        e.tick(1)?;
        let all = e.find_in_room("W1N1", c::FIND_STRUCTURES);
        let by = |kind: &str, x: i64, y: i64| {
            all.iter()
                .find(|s| {
                    s["structureType"] == kind
                        && s["pos"]["x"] == json!(25 + x)
                        && s["pos"]["y"] == json!(25 + y)
                })
                .map(|s| s["id"].as_str().unwrap().to_string())
                .expect("模板结构存在")
        };
        // rampart：hits 按奖励档位，hitsMax 取 8 级上限，归属 Invader；
        // decayTime 数据键与核心绝对截止一致（效果表另一通道在布局测试钉）。
        let rampart = e.get_object(&by("rampart", 0, 0)).unwrap();
        assert_eq!(rampart["hits"], json!(200_000));
        let max8 = c::RAMPART_HITS_MAX
            .iter()
            .find(|(k, _)| *k == "8")
            .unwrap()
            .1;
        assert_eq!(rampart["hitsMax"], json!(max8));
        assert_eq!(rampart["hitsTarget"], json!(200_000));
        assert_eq!(rampart["strongholdId"], json!("sh-bunker2"));
        assert_eq!(
            rampart["decayTime"],
            e.get_object(&core_id).unwrap()["decayTime"],
            "rampart 数据键截止与核心一致"
        );
        // tower：满能量、容量常量、归属 Invader。
        let tower = e.get_object(&by("tower", 1, 1)).unwrap();
        assert_eq!(tower["store"]["energy"], json!(c::TOWER_CAPACITY));
        assert_eq!(tower["owner"], json!(INVADER_OWNER));
        // container：容量显式 0，奖励 3 项资源钉官方 calcReward 怪癖金标：
        // 末位 liquid 密度 30，官方除数取密度表第三项 5 → 28（按自密度
        // 只会得 round(140/30)=5），总量 310×10+38×20+28×30=4700 > 4000
        // 为官方 utils.js:695 位置下标怪癖的超发形态。衰减截止与核心一致。
        let container = e.get_object(&by("container", 1, 0)).unwrap();
        assert_eq!(container["storeCapacity"], json!(0));
        assert_eq!(
            container["decayTime"],
            e.get_object(&core_id).unwrap()["decayTime"],
            "container 数据键截止与核心一致"
        );
        assert_eq!(
            container["store"],
            json!({"battery": 310, "ghodium_melt": 38, "liquid": 28}),
            "bunker2 奖励金标（官方末项除数=密度表第三项 5 的复刻值）"
        );
        // 多据点共存：异房核心同拍独立部署（奖励为 3 项非负资源）。
        let other = e.place_object(
            "W3N1",
            "invaderCore",
            &json!({"pos":[25,25],"level":2,"ticksToDeploy":1,
                    "templateName":"bunker2","strongholdId":"sh-w3"}),
        )?;
        e.tick(1)?;
        let other_container = e
            .find_in_room("W3N1", c::FIND_STRUCTURES)
            .into_iter()
            .find(|s| s["structureType"] == "container")
            .map(|s| s["id"].as_str().unwrap().to_string())
            .expect("异房容器存在");
        let other_store = e.get_object(&other_container).unwrap()["store"].clone();
        assert_eq!(other_store.as_object().map(serde_json::Map::len), Some(3));
        assert!(
            other_store
                .as_object()
                .is_some_and(|m| m.values().all(|v| v.as_i64().is_some_and(|n| n >= 0)))
        );
        assert_eq!(e.get_object(&other).unwrap()["ticksToDeploy"], Value::Null);
        // 核心满血。
        assert_eq!(
            e.get_object(&core_id).unwrap()["hits"],
            json!(c::INVADER_CORE_HITS)
        );
    }
    Ok(())
}

/// 部署清格：格上玩家 creep 死亡留墓碑、玩家结构摧毁留废墟、工地移除
/// 折半掉能量；旧 Invader rampart 整体移除不废墟。
#[test]
fn stronghold_deploy_clears_tiles_and_removes_old_invader_ramparts() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        // 旧 Invader rampart：官方 deploy 先整体移除（无废墟）。
        e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[10,10],"structureType":"rampart","owner":"Invader"}),
        )?;
        // bunker1 的 (1,0) 格=(26,25)：玩家 creep。
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"victim","owner":"p1","pos":[26,25],"body":["move"],"ticksToLive":1000}),
        )?;
        // (0,1) 格=(25,26)：玩家 road。
        e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[25,26],"structureType":"road","owner":"p1"}),
        )?;
        // (1,1) 格=(26,26)：工地 progress 10 → 掉 5 能量。
        e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[26,26],"structureType":"extension","owner":"p1","progress":10}),
        )?;
        // 核心格 (25,25)：已出生 creep 同样被清格击杀。
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"central","owner":"p1","pos":[25,25],"body":["move"],"ticksToLive":1000}),
        )?;
        // (0,1) 格再摆一枚 progress=1 工地：官方 >1 才折半掉能量，无掉落。
        e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[25,26],"structureType":"extension","owner":"p1","progress":1}),
        )?;
        deploy_core(&mut e, sh::template("bunker1").unwrap())?;
        e.tick(1)?;
        let at = |find: i64, x: i64, y: i64| {
            e.find_in_room("W1N1", find)
                .into_iter()
                .filter(|o| o["pos"]["x"] == json!(x) && o["pos"]["y"] == json!(y))
                .collect::<Vec<_>>()
        };
        assert_eq!(at(c::FIND_CREEPS, 26, 25).len(), 0, "格上 creep 被击杀");
        assert_eq!(at(c::FIND_TOMBSTONES, 26, 25).len(), 1, "击杀留墓碑");
        assert!(
            e.find_in_room("W1N1", c::FIND_CREEPS)
                .iter()
                .all(|cr| cr["name"] != "victim")
        );
        // 玩家 road 摧毁留废墟；模板 road 同格重铺。
        let ruins = at(c::FIND_RUINS, 25, 26);
        assert_eq!(ruins.len(), 1, "玩家 road 摧毁留废墟");
        assert_eq!(ruins[0]["structureType"], json!("road"));
        // 工地移除且折半掉能量。
        let energy: Vec<_> = e
            .find_in_room("W1N1", c::FIND_DROPPED_RESOURCES)
            .into_iter()
            .filter(|o| o["pos"]["x"] == json!(26) && o["pos"]["y"] == json!(26))
            .collect();
        assert_eq!(energy.len(), 1, "工地折半掉能量");
        assert_eq!(energy[0]["amount"], json!(5));
        // 旧 Invader rampart 无废墟移除；模板 rampart 不受旧 rampart
        // 清除波及（移除先于铺场）。
        assert!(
            e.find_in_room("W1N1", c::FIND_STRUCTURES)
                .iter()
                .all(|s| s["structureType"] != "rampart" || s["pos"]["x"] != json!(10)),
            "旧 rampart 移除"
        );
        assert_eq!(at(c::FIND_RUINS, 10, 10).len(), 0, "旧 rampart 无废墟");
        assert!(
            e.find_in_room("W1N1", c::FIND_STRUCTURES).iter().any(|s| {
                s["structureType"] == "rampart"
                    && s["pos"]["x"] == json!(25)
                    && s["pos"]["y"] == json!(26)
            }),
            "模板 rampart 存活"
        );
        // 核心格 creep 被清格击杀留墓碑；progress=1 的工地无能量掉落。
        assert_eq!(at(c::FIND_TOMBSTONES, 25, 25).len(), 1, "核心格击杀留墓碑");
        assert_eq!(
            e.find_in_room("W1N1", c::FIND_DROPPED_RESOURCES)
                .iter()
                .filter(|o| o["pos"]["x"] == json!(25) && o["pos"]["y"] == json!(25))
                .count(),
            0,
            "progress=1 不折半掉能量"
        );
    }
    Ok(())
}

/// 部署随机流确定性：同夹具双引擎世界逐位一致；失败轮整回滚后重放轮
/// 部署照常；冷恢复引擎独立部署后世界等值。
#[test]
fn stronghold_deploy_is_deterministic_across_retry_and_cold_restore() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let mut twin = fixture(binary)?;
        let core_id = deploy_core(&mut e, sh::template("bunker3").unwrap())?;
        deploy_core(&mut twin, sh::template("bunker3").unwrap())?;
        // 晚期写回失败：整轮回滚，部署未发生。
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", "1").is_err());
        assert_eq!(e.world_snapshot(), before, "失败轮整轮回滚");
        // 重试轮重放缓存输入：部署在重放轮发生。
        e.run_player("p1", "1")?;
        twin.run_player("p1", "1")?;
        assert_eq!(
            e.world_snapshot(),
            twin.world_snapshot(),
            "重放轮与直跑轮世界等值（奖励与衰减确定性）"
        );
        // 冷恢复：部署前保存，恢复引擎独立跑到部署后，双世界等值。
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        let mut early = fixture(binary)?;
        deploy_core(&mut early, sh::template("bunker3").unwrap())?;
        let saved = early.checkpoint(Value::Null)?;
        restored.restore_checkpoint(saved, None)?;
        early.run_player("p1", "1")?;
        restored.run_player("p1", "1")?;
        assert_eq!(early.world_snapshot(), restored.world_snapshot());
        assert!(
            e.get_object(&core_id).is_some(),
            "重试轮后核心仍在（已部署形态）"
        );
        assert_eq!(
            e.get_object(&core_id).unwrap()["ticksToDeploy"],
            Value::Null
        );
    }
    Ok(())
}

/// 放置路径孵化守军寿命绑定核心 decayTime；布防到期整体坍塌移除
///（截止时刻经检查点改短驱动，不跑 67500 拍）。
#[test]
fn stronghold_incubation_binds_core_decay_and_structures_collapse_together() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let core_id = e.place_object(
            "W1N1",
            "invaderCore",
            &json!({
                "pos":[25,25],"level":2,"ticksToDeploy":1,"templateName":"bunker2",
                "strongholdId":"sh-bunker2",
                "spawning":{"name":"guard","body":["attack","move"],"needTime":50,"remainingTime":50}
            }),
        )?;
        scout(&mut e)?;
        e.tick(1)?;
        // 守军寿命 = 核心 decayTime（绝对键直接比对）。
        let guard = e.world_snapshot()["objects"]
            .as_array()
            .expect("世界对象数组")
            .iter()
            .find(|o| o["type"] == "creep" && o["name"] == "guard")
            .cloned()
            .expect("孵化守军存在");
        let decay_time = e.get_object(&core_id).unwrap()["decayTime"]
            .as_i64()
            .expect("部署写 decayTime");
        assert!(decay_time > 0);
        assert_eq!(
            guard["ageTime"],
            json!(decay_time),
            "守军寿命绑定核心 decayTime"
        );
        // 玩家面剩余读数与绝对截止一致（同拍读取）。
        let t = e.get_game_time();
        let remaining = e
            .run_player(
                "p1",
                &format!(
                    "Game.getObjectById('{core_id}').effects.find(x=>x.effect===1002).ticksRemaining"
                ),
            )?
            .expr["value"]
            .as_i64()
            .unwrap();
        assert_eq!(remaining, decay_time - t as i64);
        // 把布防全体的坍塌截止改短驱动整体移除。
        let mut checkpoint = e.checkpoint(Value::Null)?;
        let effects = &mut checkpoint["engine"]["world"]["rooms"]["W1N1"]["effects"];
        let effects = effects.as_object_mut().expect("效果表存在");
        let short = e.get_game_time() + 3;
        for (_id, entries) in effects.iter_mut() {
            for entry in entries.as_array_mut().unwrap() {
                if entry["effect"] == json!(1002) {
                    entry["end_time"] = json!(short);
                }
            }
        }
        e.restore_checkpoint(checkpoint, None)?;
        e.tick(5)?;
        assert!(e.get_object(&core_id).is_none(), "核心随坍塌移除");
        // 除 controller 外结构全清（rampart/tower/road/container 一并）。
        assert!(
            e.find_in_room("W1N1", c::FIND_STRUCTURES)
                .iter()
                .all(|s| s["structureType"] == "controller"),
            "布防结构随坍塌移除"
        );
        assert_eq!(
            e.find_in_room("W1N1", c::FIND_RUINS).len(),
            0,
            "坍塌移除不落废墟"
        );
        // 布防坍塌沿核心坍塌路径：控制器按官方重置清单清空。
        let ctl = e
            .find_in_room("W1N1", c::FIND_STRUCTURES)
            .into_iter()
            .find(|s| s["structureType"] == "controller")
            .expect("控制器存在");
        assert_eq!(ctl["owner"], Value::Null);
        assert_eq!(ctl["level"], 0);
    }
    Ok(())
}

/// 靠边部署的坐标裁剪：越界条目跳过、界内子集照铺；放置预写的
/// ticksToCollapse 短截止被部署改写到衰减窗口（先放置后部署延寿）。
#[test]
fn stronghold_edge_placement_clips_and_deploy_overwrites_collapse_clock() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        // bunker5 半径 3：核心 (2,2) 时 dx/dy<-2 的条目越界裁剪。
        let core_id = e.place_object(
            "W1N1",
            "invaderCore",
            &json!({"pos":[2,2],"level":5,"ticksToDeploy":1,
                        "templateName":"bunker5","strongholdId":"sh-edge",
                        "ticksToCollapse":2}),
        )?;
        scout(&mut e)?;
        e.tick(1)?;
        let remaining = e
            .run_player(
                "p1",
                &format!(
                    "Game.getObjectById('{core_id}').effects.find(x=>x.effect===1002).ticksRemaining"
                ),
            )?
            .expr["value"]
            .as_i64()
            .unwrap_or_default();
        let all = e.find_in_room("W1N1", c::FIND_STRUCTURES);
        let expected: Vec<(i64, i64)> = sh::template("bunker5")
            .unwrap()
            .structures
            .iter()
            .filter(|en| 2 + en.dx as i64 >= 0 && 2 + en.dy as i64 >= 0)
            .map(|en| (2 + en.dx as i64, 2 + en.dy as i64))
            .collect();
        let placed: Vec<(i64, i64)> = all
            .iter()
            .filter(|s| s["structureType"] != "invaderCore" && s["structureType"] != "controller")
            .map(|s| {
                (
                    s["pos"]["x"].as_i64().unwrap(),
                    s["pos"]["y"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(placed.len(), expected.len(), "越界条目被裁剪");
        for pos in expected {
            assert!(placed.contains(&pos), "界内条目 {pos:?} 照铺");
        }
        // 部署把放置预写的 2 tick 短截止改写到官方衰减窗口。
        assert!(
            (67_500..=82_500).contains(&remaining),
            "部署改写坍塌截止 {remaining}"
        );
    }
    Ok(())
}

/// 非法布防放置整单拒绝：未知模板名、非字符串 strongholdId。
#[test]
fn stronghold_invalid_placement_is_rejected_atomically() -> anyhow::Result<()> {
    let mut e = fixture(false)?;
    for bad in [
        json!({"pos":[25,25],"level":1,"ticksToDeploy":1,"templateName":"bunker9"}),
        json!({"pos":[25,25],"level":1,"ticksToDeploy":1,"templateName":3}),
        json!({"pos":[25,25],"level":1,"strongholdId":17}),
    ] {
        assert!(
            e.place_object("W1N1", "invaderCore", &bad).is_err(),
            "非法布防 spec {bad}"
        );
    }
    let valid_creep = json!({"name":"worker","owner":"p1","pos":[5,5],"body":["move"]});
    assert!(
        e.place_objects(&[
            ("W1N1", "creep", &valid_creep),
            (
                "W1N1",
                "invaderCore",
                &json!({"pos":[25,25],"level":1,"ticksToDeploy":1,"templateName":"bunker6"})
            ),
        ])
        .is_err()
    );
    assert!(
        e.find_in_room("W1N1", c::FIND_CREEPS)
            .iter()
            .all(|cr| cr["name"] != "worker"),
        "批量失败不发布前缀"
    );
    Ok(())
}
