//! G25.b `sim run --verify-replay` 端到端验收：核对通过与段数、dice
//! 场景（Mod+RNG）、失败轮不核对、mods_switch 拒绝、缺 --input-log 拒绝。
use super::*;

fn sim_command(scene: &Path, report: &Path, log: &Path, ticks: u32) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .current_dir(std::env::temp_dir())
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--report")
        .arg(report)
        .arg("--ticks")
        .arg(ticks.to_string())
        .arg("--input-log")
        .arg(log)
        .arg("--verify-replay");
    command
}

fn result(command: &mut Command, report: &Path) -> anyhow::Result<(i32, Value)> {
    let output = command.output()?;
    let code = output.status.code().expect("子进程正常退出");
    let value: Value = serde_json::from_slice(&std::fs::read(report)?)?;
    Ok((code, value))
}

fn plain_scenario() -> Value {
    json!({
        "version": 1,
        "shard": {"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 3}]},
        "bots": [{"player": "p1", "modules": "modules"}]
    })
}

/// 普通场景：回放核对通过，段数与轮数一致，报告 v9 带 verify_replay；
/// 双通道（JSON/KKPV 投影）都核对。
#[test]
fn verify_replay_passes_on_plain_scenario() -> anyhow::Result<()> {
    for binary in [false, true] {
        let fixture = Fixture::new()?;
        let scene = fixture.scene(&plain_scenario())?;
        fixture.module("module.exports.loop = function () { Memory.n = (Memory.n || 0) + 1; };")?;
        let log = fixture.0.join("input.jsonl");
        let report = fixture.0.join("r.json");

        let mut command = sim_command(&scene, &report, &log, 3);
        if binary {
            command.arg("--kkpv");
        }
        let (code, report) = result(&mut command, &report)?;
        assert_eq!(code, 0);
        assert_eq!(report["report_version"], 10, "v10 起报告版本随周期保存升版");
        let verify = &report["verify_replay"];
        assert_eq!(verify["status"], "passed", "{verify}");
        assert_eq!(verify["segments"], 3);
        assert_eq!(verify["first_tick"], 0);
        assert_eq!(verify["last_tick"], 2);
    }
    Ok(())
}

/// 检查点续跑组合：--load-checkpoint 恢复后 --verify-replay——起点快照
/// 在恢复之后，配对规则跳过前次运行的旧段、只重放本次续跑段。
#[test]
fn verify_replay_after_checkpoint_resume_skips_prior_segments() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = fixture.scene(&plain_scenario())?;
    fixture.module("module.exports.loop = function () { Memory.n = (Memory.n || 0) + 1; };")?;
    let log = fixture.0.join("pair.jsonl");
    let checkpoint = fixture.0.join("cp.json");
    let report1 = fixture.0.join("r1.json");
    let report2 = fixture.0.join("r2.json");

    let mut command = sim_command(&scene, &report1, &log, 2);
    command.arg("--save-checkpoint").arg(&checkpoint);
    let (code, _) = result(&mut command, &report1)?;
    assert_eq!(code, 0);
    assert!(checkpoint.exists(), "检查点与日志同跑都落盘");

    let mut command = sim_command(&scene, &report2, &log, 2);
    command.arg("--load-checkpoint").arg(&checkpoint);
    let (code, report2) = result(&mut command, &report2)?;
    assert_eq!(code, 0);
    let verify = &report2["verify_replay"];
    assert_eq!(verify["status"], "passed", "{verify}");
    assert_eq!(
        verify["segments"], 2,
        "只重放本次续跑的两段（跳过前次旧段）"
    );
    assert_eq!(verify["first_tick"], 2, "续跑段从恢复点 tick 开始");
    assert_eq!(verify["last_tick"], 3);
    Ok(())
}

/// dice 场景（Mod+RNG，G22.b 夹具）：回放核对通过——抽值序列经结算
/// 重放逐位复现。
#[test]
fn verify_replay_passes_on_mod_dice_scenario() -> anyhow::Result<()> {
    let scene = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-determinism/scenario-dice.json")
        .canonicalize()?;
    let fixture = Fixture::new()?;
    let log = fixture.0.join("dice.jsonl");
    let report = fixture.0.join("r.json");

    let (code, report) = result(&mut sim_command(&scene, &report, &log, 4), &report)?;
    assert_eq!(code, 0, "{}", report["error"]);
    let verify = &report["verify_replay"];
    assert_eq!(verify["status"], "passed", "{verify}");
    assert_eq!(verify["segments"], 4);
    Ok(())
}

/// 运行失败（玩家第 2 tick 抛错）不核对：verify_replay 如实记 skipped。
#[test]
fn verify_replay_skips_failed_runs() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = fixture.scene(&plain_scenario())?;
    fixture.module(
        "module.exports.loop = function () { if ((Memory.n || 0) >= 1) throw new Error('boom'); Memory.n = (Memory.n || 0) + 1; };",
    )?;
    let log = fixture.0.join("input.jsonl");
    let report = fixture.0.join("r.json");

    let (code, report) = result(&mut sim_command(&scene, &report, &log, 3), &report)?;
    assert_eq!(code, 2, "运行失败退出 2");
    assert_eq!(report["status"], "run_failed");
    assert_eq!(report["verify_replay"]["status"], "skipped");
    assert!(
        report["verify_replay"]["reason"]
            .as_str()
            .unwrap()
            .contains("未核对")
    );
    Ok(())
}

/// mods_switch 场景显式拒绝（切换的逐段身份重装属后续切片）。
#[test]
fn verify_replay_rejects_mods_switch_scenarios() -> anyhow::Result<()> {
    let scene = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-switch/scenario-add.json")
        .canonicalize()?;
    let fixture = Fixture::new()?;
    let log = fixture.0.join("switch.jsonl");
    let report = fixture.0.join("r.json");

    let mut command = sim_command(&scene, &report, &log, 6);
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(1), "setup 阶段拒绝");
    let report: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(report["status"], "setup_failed");
    assert!(
        report["error"].as_str().unwrap().contains("mods_switch"),
        "{}",
        report["error"]
    );
    assert_eq!(report["verify_replay"]["status"], "setup_failed", "v9 契约");
    Ok(())
}

/// --verify-replay 依赖 --input-log：缺省时 setup 拒绝。
#[test]
fn verify_replay_requires_input_log() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = fixture.scene(&plain_scenario())?;
    fixture.module("module.exports.loop = function () {};")?;
    let report = fixture.0.join("r.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .current_dir(std::env::temp_dir())
        .args(["sim", "run", "--scenario"])
        .arg(&scene)
        .arg("--report")
        .arg(&report)
        .arg("--ticks")
        .arg("2")
        .arg("--verify-replay");
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert!(
        report["error"].as_str().unwrap().contains("--input-log"),
        "{}",
        report["error"]
    );
    Ok(())
}

/// 效果表参与逐段重放；跨进程恢复后继续消费同一塔效果，终态等于直跑。
#[test]
fn tower_power_effect_replay_and_cold_resume_match_straight_run() -> anyhow::Result<()> {
    for binary in [false, true] {
        let fixture = Fixture::new()?;
        let scene = fixture.scene(&json!({
            "version":1,
            "shard":{"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]},
            "bots":[{"player":"p1","modules":"modules"}],
            "objects":[
                {"alias":"tower","room":"W1N1","kind":"structure","spec":{"structureType":"tower","owner":"p1","pos":[20,20],"store":{"energy":1000}}},
                {"alias":"operator","room":"W1N1","kind":"powerCreep","spec":{"name":"operator","owner":"p1","pos":[19,20],"powers":{"3":5},"store":{"ops":100}}},
                {"alias":"enemy","room":"W1N1","kind":"creep","spec":{"name":"enemy","owner":"p2","pos":[27,20],"body":vec!["move";30]}}
            ]
        }))?;
        fixture.module("exports.loop=()=>{const tower=Game.rooms.W1N1.find(FIND_MY_STRUCTURES).find(s=>s.structureType===STRUCTURE_TOWER);if(!Memory.cast){Game.powerCreeps.operator.usePower(PWR_OPERATE_TOWER,tower);Memory.cast=true;}else{tower.attack(Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0]);}};")?;
        let checkpoint = fixture.0.join("checkpoint.json");
        let invoke = |name: &str, ticks: u32, option: Option<&str>| -> anyhow::Result<Value> {
            let report = fixture.0.join(format!("{name}.json"));
            let log = fixture.0.join(format!("{name}.jsonl"));
            let mut command = sim_command(&scene, &report, &log, ticks);
            if binary {
                command.arg("--kkpv");
            }
            if let Some(option) = option {
                command.arg(option).arg(&checkpoint);
            }
            let (code, value) = result(&mut command, &report)?;
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["verify_replay"]["status"], "passed", "{value}");
            Ok(value)
        };
        let mut straight = invoke("straight", 3, None)?["final"].clone();
        invoke("first", 1, Some("--save-checkpoint"))?;
        let mut resumed = invoke("resumed", 2, Some("--load-checkpoint"))?["final"].clone();
        for value in [&mut straight, &mut resumed] {
            for player in value["players"].as_object_mut().unwrap().values_mut() {
                player.as_object_mut().unwrap().remove("cpu_bucket");
            }
        }
        assert_eq!(straight, resumed);
        assert_eq!(resumed["objects"]["enemy"]["hits"], 1380);
        assert_eq!(resumed["objects"]["operator"]["store"]["ops"], 90);
        assert_eq!(
            resumed["objects"]["tower"]["effects"][0]["ticksRemaining"],
            97
        );
    }
    Ok(())
}

/// 孵化时钟与两种效果在输入回放及跨进程恢复后保持一致。
#[test]
fn spawn_power_replay_and_cold_resume_match_straight_run() -> anyhow::Result<()> {
    for binary in [false, true] {
        let fixture = Fixture::new()?;
        let scene = fixture.scene(&json!({
            "version":1,
            "shard":{"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]},
            "bots":[{"player":"p1","modules":"modules"}],
            "objects":[
                {"alias":"spawn","room":"W1N1","kind":"structure","spec":{"structureType":"spawn","name":"Spawn1","owner":"p1","pos":[25,25],"store":{"energy":300}}},
                {"alias":"operator","room":"W1N1","kind":"powerCreep","spec":{"name":"operator","owner":"p1","pos":[22,25],"powers":{"2":3},"store":{"ops":200}}},
                {"alias":"jammer","room":"W1N1","kind":"powerCreep","spec":{"name":"jammer","owner":"p1","pos":[22,26],"powers":{"9":5},"store":{"ops":30}}}
            ]
        }))?;
        fixture.module("exports.loop=()=>{const spawn=Game.spawns.Spawn1;if(!Memory.phase){Game.powerCreeps.operator.usePower(PWR_OPERATE_SPAWN,spawn);Memory.phase=1;}else if(Memory.phase===1){spawn.spawnCreep([MOVE,MOVE,MOVE], 'child',{directions:[BOTTOM]});Game.powerCreeps.jammer.usePower(PWR_DISRUPT_SPAWN,spawn);Memory.phase=2;}};")?;
        let checkpoint = fixture.0.join("checkpoint.json");
        let invoke = |name: &str, ticks: u32, option: Option<&str>| -> anyhow::Result<Value> {
            let report = fixture.0.join(format!("{name}.json"));
            let log = fixture.0.join(format!("{name}.jsonl"));
            let mut command = sim_command(&scene, &report, &log, ticks);
            if binary {
                command.arg("--kkpv");
            }
            if let Some(option) = option {
                command.arg(option).arg(&checkpoint);
            }
            let (code, value) = result(&mut command, &report)?;
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["verify_replay"]["status"], "passed", "{value}");
            Ok(value)
        };
        let mut straight = invoke("straight", 12, None)?["final"].clone();
        invoke("first", 3, Some("--save-checkpoint"))?;
        let mut resumed = invoke("resumed", 9, Some("--load-checkpoint"))?["final"].clone();
        for value in [&mut straight, &mut resumed] {
            for player in value["players"].as_object_mut().unwrap().values_mut() {
                player.as_object_mut().unwrap().remove("cpu_bucket");
            }
        }
        assert_eq!(straight, resumed);
        assert!(resumed["objects"]["spawn"]["spawning"].is_null());
        assert_eq!(resumed["objects"]["operator"]["store"]["ops"], 100);
        assert_eq!(resumed["objects"]["jammer"]["store"]["ops"], 20);
    }
    Ok(())
}

/// 化学库存与账号 power 在结算重放及跨进程恢复后保持一致。
#[test]
fn production_power_replay_and_cold_resume_match_straight_run() -> anyhow::Result<()> {
    for binary in [false, true] {
        let fixture = Fixture::new()?;
        let scene = fixture.scene(&json!({
            "version":1,
            "shard":{"players":[{"name":"p1","power":100000}],"rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]},
            "bots":[{"player":"p1","modules":"modules"}],
            "objects":[
                {"alias":"lab","room":"W1N1","kind":"structure","spec":{"structureType":"lab","owner":"p1","pos":[20,20],"store":{}}},
                {"alias":"first","room":"W1N1","kind":"structure","spec":{"structureType":"lab","owner":"p1","pos":[21,20],"store":{"H":100}}},
                {"alias":"second","room":"W1N1","kind":"structure","spec":{"structureType":"lab","owner":"p1","pos":[22,20],"store":{"O":100}}},
                {"alias":"powerSpawn","room":"W1N1","kind":"structure","spec":{"structureType":"powerSpawn","owner":"p1","pos":[21,21],"store":{"energy":5000,"power":100}}},
                {"alias":"operator","room":"W1N1","kind":"powerCreep","spec":{"name":"operator","owner":"p1","pos":[20,19],"powers":{"5":5},"store":{"ops":100}}},
                {"alias":"processor","room":"W1N1","kind":"powerCreep","spec":{"name":"processor","owner":"p1","pos":[20,21],"powers":{"16":5},"store":{"ops":300}}}
            ]
        }))?;
        fixture.module("exports.loop=()=>{const labs=Game.rooms.W1N1.find(FIND_MY_STRUCTURES).filter(s=>s.structureType===STRUCTURE_LAB);const ps=Game.rooms.W1N1.find(FIND_MY_STRUCTURES).find(s=>s.structureType===STRUCTURE_POWER_SPAWN);if(!Memory.cast){Game.powerCreeps.operator.usePower(PWR_OPERATE_LAB,labs[0]);Game.powerCreeps.processor.usePower(PWR_OPERATE_POWER,ps);Memory.cast=true;}else{labs[0].runReaction(labs[1],labs[2]);ps.processPower();}};")?;
        let checkpoint = fixture.0.join("checkpoint.json");
        let invoke = |name: &str, ticks: u32, option: Option<&str>| -> anyhow::Result<Value> {
            let report = fixture.0.join(format!("{name}.json"));
            let log = fixture.0.join(format!("{name}.jsonl"));
            let mut command = sim_command(&scene, &report, &log, ticks);
            if binary {
                command.arg("--kkpv");
            }
            if let Some(option) = option {
                command.arg(option).arg(&checkpoint);
            }
            let (code, value) = result(&mut command, &report)?;
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["verify_replay"]["status"], "passed", "{value}");
            Ok(value)
        };
        let mut straight = invoke("straight", 4, None)?["final"].clone();
        invoke("first", 2, Some("--save-checkpoint"))?;
        let mut resumed = invoke("resumed", 2, Some("--load-checkpoint"))?["final"].clone();
        for value in [&mut straight, &mut resumed] {
            for player in value["players"].as_object_mut().unwrap().values_mut() {
                player.as_object_mut().unwrap().remove("cpu_bucket");
            }
        }
        assert_eq!(straight, resumed);
        assert_eq!(resumed["objects"]["lab"]["store"]["OH"], 15);
        assert_eq!(resumed["objects"]["powerSpawn"]["store"]["power"], 82);
        assert_eq!(resumed["objects"]["powerSpawn"]["store"]["energy"], 4100);
        assert_eq!(resumed["objects"]["operator"]["store"]["ops"], 90);
        assert_eq!(resumed["objects"]["processor"]["store"]["ops"], 100);
    }
    Ok(())
}

/// 首次锁级和商品生产在回放及跨进程恢复后与直跑一致。
#[test]
fn factory_power_replay_and_cold_resume_match_straight_run() -> anyhow::Result<()> {
    for binary in [false, true] {
        let fixture = Fixture::new()?;
        let scene = fixture.scene(&json!({
            "version":1,
            "shard":{"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]},
            "bots":[{"player":"p1","modules":"modules"}],
            "objects":[
                {"alias":"factory","room":"W1N1","kind":"structure","spec":{"structureType":"factory","owner":"p1","pos":[20,20],"store":{"utrium_bar":20,"zynthium_bar":20,"energy":20}}},
                {"alias":"operator","room":"W1N1","kind":"powerCreep","spec":{"name":"operator","owner":"p1","pos":[19,20],"powers":{"19":1},"store":{"ops":200}}}
            ]
        }))?;
        fixture.module("exports.loop=()=>{const f=Game.rooms.W1N1.find(FIND_MY_STRUCTURES).find(s=>s.structureType===STRUCTURE_FACTORY);if(!Memory.cast){Game.powerCreeps.operator.usePower(PWR_OPERATE_FACTORY,f);Memory.cast=true;}else{f.produce(RESOURCE_COMPOSITE);}};")?;
        let checkpoint = fixture.0.join("checkpoint.json");
        let invoke = |name: &str, ticks: u32, option: Option<&str>| -> anyhow::Result<Value> {
            let report = fixture.0.join(format!("{name}.json"));
            let log = fixture.0.join(format!("{name}.jsonl"));
            let mut command = sim_command(&scene, &report, &log, ticks);
            if binary {
                command.arg("--kkpv");
            }
            if let Some(option) = option {
                command.arg(option).arg(&checkpoint);
            }
            let (code, value) = result(&mut command, &report)?;
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["verify_replay"]["status"], "passed", "{value}");
            Ok(value)
        };
        let mut straight = invoke("straight", 3, None)?["final"].clone();
        invoke("first", 1, Some("--save-checkpoint"))?;
        let mut resumed = invoke("resumed", 2, Some("--load-checkpoint"))?["final"].clone();
        for value in [&mut straight, &mut resumed] {
            for player in value["players"].as_object_mut().unwrap().values_mut() {
                player.as_object_mut().unwrap().remove("cpu_bucket");
            }
        }
        assert_eq!(straight, resumed);
        assert_eq!(resumed["objects"]["factory"]["level"], 1);
        assert_eq!(resumed["objects"]["factory"]["store"]["composite"], 20);
        assert_eq!(resumed["objects"]["operator"]["store"]["ops"], 100);
    }
    Ok(())
}

/// 周期补充的相位和被延后的普通再生时钟在跨进程恢复后保持一致。
#[test]
fn source_power_replay_and_cold_resume_match_straight_run() -> anyhow::Result<()> {
    for binary in [false, true] {
        let fixture = Fixture::new()?;
        let scene = fixture.scene(&json!({
            "version":1,
            "shard":{"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]},
            "bots":[{"player":"p1","modules":"modules"}],
            "objects":[
                {"alias":"source","room":"W1N1","kind":"structure","spec":{"structureType":"source","pos":[20,20],"energy":0,"ticksToRegeneration":10}},
                {"alias":"regen","room":"W1N1","kind":"powerCreep","spec":{"name":"regen","owner":"p1","pos":[19,20],"powers":{"13":2}}},
                {"alias":"disrupt","room":"W1N1","kind":"powerCreep","spec":{"name":"disrupt","owner":"p1","pos":[19,20],"powers":{"11":1},"store":{"ops":200}}}
            ]
        }))?;
        fixture.module("exports.loop=()=>{if(!Memory.cast){const s=Game.rooms.W1N1.find(FIND_SOURCES)[0];Game.powerCreeps.regen.usePower(PWR_REGEN_SOURCE,s);Game.powerCreeps.disrupt.usePower(PWR_DISRUPT_SOURCE,s);Memory.cast=true;}};")?;
        let checkpoint = fixture.0.join("checkpoint.json");
        let invoke = |name: &str, ticks: u32, option: Option<&str>| -> anyhow::Result<Value> {
            let report = fixture.0.join(format!("{name}.json"));
            let log = fixture.0.join(format!("{name}.jsonl"));
            let mut command = sim_command(&scene, &report, &log, ticks);
            if binary {
                command.arg("--kkpv");
            }
            if let Some(option) = option {
                command.arg(option).arg(&checkpoint);
            }
            let (code, value) = result(&mut command, &report)?;
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["verify_replay"]["status"], "passed", "{value}");
            Ok(value)
        };
        let mut straight = invoke("straight", 110, None)?["final"].clone();
        invoke("first", 13, Some("--save-checkpoint"))?;
        let mut resumed = invoke("resumed", 97, Some("--load-checkpoint"))?["final"].clone();
        for value in [&mut straight, &mut resumed] {
            for player in value["players"].as_object_mut().unwrap().values_mut() {
                player.as_object_mut().unwrap().remove("cpu_bucket");
            }
        }
        assert_eq!(straight, resumed);
        assert_eq!(resumed["objects"]["source"]["energy"], 3000);
        assert!(resumed["objects"]["source"]["ticksToRegeneration"].is_null());
        assert_eq!(resumed["objects"]["disrupt"]["store"]["ops"], 100);
    }
    Ok(())
}
