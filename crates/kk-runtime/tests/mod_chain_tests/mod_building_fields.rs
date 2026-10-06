//! G08.a 建筑自定义字段与生命周期 tick：整读整写契约、原生键只读、
//! 槽拒绝即故障、先产后 tick、停用回退为惰性结构与玩家面记录直供。
use super::*;

fn gauge(body: &str) -> String {
    format!(
        r#"
export default function register(api) {{
  // 无生产无 tick 的对照建筑：完工默认值不被生命周期改写。
  api.buildings.define('bl:meter', {{
    playerClass: 'Meter', cost: {{energy: 100}}, hitsMax: 500, pathing: 'obstacle',
    fields: {{
      heat: {{type: 'u32', min: 0, max: 1000, default: 7}},
      label: {{type: 'string', maxBytes: 64, default: 'fresh'}},
    }},
  }});
  api.buildings.define('bl:gauge', {{
    playerClass: 'Gauge', cost: {{energy: 100}}, hitsMax: 500, pathing: 'obstacle',
    capacity: 200,
    production: {{ interval: 1, inputs: {{}}, outputs: {{energy: 5}} }},
    fields: {{
      heat: {{type: 'u32', min: 0, max: 1000, default: 7}},
      label: {{type: 'string', maxBytes: 64, default: ''}}
    }},
    tick(ctx) {{ {body} }}
  }});
}}
"#
    )
}

const PLAYER: &str = r#"
export default function install(api) {
  class Gauge extends api.builtins.OwnedStructure {}
  class Meter extends api.builtins.OwnedStructure {}
  api.buildings.bind('bl:gauge', Gauge);
  api.buildings.bind('bl:meter', Meter);
}
"#;

/// 建筑域包必须绑定玩家类（player.js 的 api.buildings.bind），
/// 共享 engine 助手不写 player 文件，这里按 mod_building_world 同构。
fn gauge_engine(body: &str, kkpv: bool) -> anyhow::Result<EmbedEngine> {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-building-fields-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    let dir = root.0.join("bl");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("mod.json"),
        json!({"id":"bl","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}).to_string(),
    )?;
    std::fs::write(dir.join("server.js"), gauge(body))?;
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

fn gauge_at(e: &mut EmbedEngine) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({
            "pos": [11, 10], "structureType": "bl:gauge", "owner": "p1",
            "hits": 500, "hitsMax": 500, "storeCapacity": 200, "store": {"energy": 0},
            "bl:heat": 7, "bl:label": ""
        }),
    )
}

/// tick 经 ctx.target.fields 整读整写：先产后 tick 的候选观察值、
/// 自定义键跨 tick 持久化、public 字段对玩家记录直供。
#[test]
fn building_tick_reads_and_writes_custom_fields_in_both_channels() -> anyhow::Result<()> {
    for kkpv in [false, true] {
        let mut e = gauge_engine(
            "ctx.state={seen:[...(ctx.state?.seen||[]),ctx.target.fields['bl:heat'],ctx.target.fields.store.energy]};ctx.target.fields['bl:heat']=(ctx.target.fields['bl:heat']||0)+1;ctx.target.fields['bl:label']=ctx.target.fields['bl:label']||'';",
            kkpv,
        )?;
        let id = gauge_at(&mut e)?;
        e.tick(2)?;
        let object = e.get_object(&id).unwrap();
        assert_eq!(object["bl:heat"], json!(9), "自定义键跨 tick 持久化");
        assert_eq!(object["store"]["energy"], json!(10), "生产照常结算");
        assert_eq!(
            e.engine_mod_state("bl"),
            json!({"seen": [7, 5, 8, 10]}),
            "先产后 tick：观察值来自本轮候选"
        );
        // 玩家面记录直供：public 字段随结构记录发布给玩家。
        // toJSON 面与缺键（Missing 默认未写）读 undefined 一并锁定。
        let bare = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos": [12, 10], "structureType": "bl:gauge", "owner": "p1",
                "hits": 500, "hitsMax": 500, "storeCapacity": 200, "store": {"energy": 0},
            }),
        )?;
        let probe = format!(
            "const g=Game.getObjectById('{id}'),b=Game.getObjectById('{bare}');\
             Memory.heat=g['bl:heat'];\
             Memory.serial=JSON.stringify(g);\
             Memory.bare=b['bl:heat']===undefined?'undef':b['bl:heat'];"
        );
        e.run_player("p1", &probe)?;
        let memory: Value =
            serde_json::from_str(&e.player_state_for_test("p1").unwrap().raw_memory).unwrap();
        assert_eq!(memory["heat"], json!(9));
        let serial = memory["serial"].as_str().unwrap_or_default();
        assert!(
            serial.contains("\"bl:heat\":9"),
            "toJSON 含自定义键：{serial}"
        );
        assert_eq!(memory["bare"], json!("undef"), "记录缺键读到 undefined");
    }
    Ok(())
}

/// 原生键漂移：写面拒绝使整轮候选丢弃，世界保持动作前。
#[test]
fn native_key_drift_in_building_tick_fails_the_round() -> anyhow::Result<()> {
    for kkpv in [false, true] {
        let mut e = gauge_engine(
            "ctx.target.fields.hits=499;ctx.target.fields['bl:heat']=(ctx.target.fields['bl:heat']||0)+1;ctx.target.fields['bl:label']=ctx.target.fields['bl:label']||'';",
            kkpv,
        )?;
        let id = gauge_at(&mut e)?;
        let err = e.tick(1).expect_err("原生键漂移必须整轮失败");
        assert!(err.to_string().contains("原生记录键"), "{err}");
        assert_eq!(e.get_game_time(), 0, "故障轮不提交");
        let object = e.get_object(&id).unwrap();
        assert_eq!(object["bl:heat"], json!(7));
        assert_eq!(object["store"]["energy"], json!(0), "生产与 tick 一起回滚");
    }
    Ok(())
}

/// 生命周期槽不支持正常拒绝：decline 升格为故障。
#[test]
fn building_tick_decline_is_a_fault() -> anyhow::Result<()> {
    for kkpv in [false, true] {
        let mut e = gauge_engine("ctx.state={touched:true};return api.decline('halt');", kkpv)?;
        let id = gauge_at(&mut e)?;
        let err = e.tick(1).expect_err("生命周期槽拒绝须失败");
        assert!(err.to_string().contains("不支持正常拒绝"), "{err}");
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(e.get_object(&id).unwrap()["store"]["energy"], json!(0));
    }
    Ok(())
}

/// 停用所属包：生命周期 tick（Mod 槽）随停用掩码停止，自定义键停在
/// 前值成为惰性数据；生产是 plane 上的 host 规则，与资源/仓储规则
/// 同口径存续（停用不改写已冻结的 plane），库存照常推进。
#[test]
fn disabled_mod_stops_building_tick_but_host_production_persists() -> anyhow::Result<()> {
    for kkpv in [false, true] {
        let mut e = gauge_engine(
            "ctx.target.fields['bl:heat']=(ctx.target.fields['bl:heat']||0)+1;ctx.target.fields['bl:label']=ctx.target.fields['bl:label']||'';",
            kkpv,
        )?;
        let id = gauge_at(&mut e)?;
        e.tick(1)?;
        assert_eq!(e.get_object(&id).unwrap()["bl:heat"], json!(8));
        e.disable_mod("bl")?;
        e.tick(2)?;
        assert_eq!(e.get_game_time(), 3);
        let object = e.get_object(&id).unwrap();
        assert_eq!(object["bl:heat"], json!(8), "tick 停止，字段停在停用前值");
        assert_eq!(
            object["store"]["energy"],
            json!(15),
            "生产按 plane 规则继续"
        );
        assert_eq!(object["hits"], json!(500), "结构本身不受停用影响");
        // 停用后玩家面仍可读惰性字段的冻结前值。
        e.run_player(
            "p1",
            &format!("Memory.frozen=Game.getObjectById('{id}')['bl:heat'];"),
        )?;
        let memory: Value =
            serde_json::from_str(&e.player_state_for_test("p1").unwrap().raw_memory).unwrap();
        assert_eq!(memory["frozen"], json!(8));
    }
    Ok(())
}

/// 完工默认值 → 玩家读成链：无 tick 的 meter 建造完工即落 Value 默认，
/// 玩家在下一轮读到默认值（不经生命周期改写）。
#[test]
fn completion_defaults_reach_the_player_surface() -> anyhow::Result<()> {
    for kkpv in [false, true] {
        let mut e = gauge_engine(
            "ctx.target.fields['bl:heat']=(ctx.target.fields['bl:heat']||0)+1;ctx.target.fields['bl:label']=ctx.target.fields['bl:label']||'';",
            kkpv,
        )?;
        let _worker = e.place_object(
            "W1N1",
            "creep",
            &json!({"pos":[10,10],"owner":"p1","name":"w",
                "body":["work","carry","move"],
                "store":{"energy":50}}),
        )?;
        let site = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[11,10],"structureType":"bl:meter","owner":"p1",
                "progress":99,"progressTotal":100}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.w.build(Game.getObjectById('{site}'));"),
        )?;
        let meter = e.world_snapshot()["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["structureType"] == "bl:meter")
            .and_then(|o| o["id"].as_str())
            .expect("meter 完工落成")
            .to_string();
        let object = e.get_object(&meter).unwrap();
        assert_eq!(object["bl:heat"], json!(7), "Value 默认完工落键");
        assert_eq!(object["bl:label"], json!("fresh"));
        e.run_player(
            "p1",
            &format!(
                "Memory.defaults=[Game.getObjectById('{meter}')['bl:heat'],\
                 Game.getObjectById('{meter}')['bl:label']];"
            ),
        )?;
        let memory: Value =
            serde_json::from_str(&e.player_state_for_test("p1").unwrap().raw_memory).unwrap();
        assert_eq!(memory["defaults"], json!([7, "fresh"]), "玩家读到完工默认");
    }
    Ok(())
}
